//! Isolated ROOT-enumeration experiment. Compiled out of ordinary builds.
//! Reuses the installed .30 driver, public identity, ROOT container sentinel,
//! owner ACL and authenticated sideband. No foreign device or driver update.
use super::*;
use std::{
    fs::OpenOptions,
    io::Write,
    time::{Duration, Instant},
};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;

const HID_CLASS: GUID = GUID::from_u128(0x745a17a0_74d3_11d0_b6fe_00a0c90f57da);
const MARKER: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x19f81049_3679_40f2_9689_2a2f61f8bd29),
    pid: 90,
};
const OWNER: DEVPROPKEY = DEVPROPKEY {
    fmtid: MARKER.fmtid,
    pid: 5,
};
const DRIVER_VERSION: u64 = (1u64 << 32) | 30;
const LOG: &str = r"C:\Program Files\SCX VPad\root-test\events.log";
fn record(text: &str) {
    // This directory is provisioned by the admin launcher, never by a client.
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(LOG) {
        let _ = writeln!(file, "{text}");
    }
}
fn win(ok: i32, operation: &str) -> io::Result<()> {
    if ok != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    record(&format!("{operation}: {error}"));
    Err(io::Error::new(
        error.kind(),
        format!("{operation}: {error}"),
    ))
}
struct Set(HDEVINFO);
impl Drop for Set {
    fn drop(&mut self) {
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}
fn set(handle: HDEVINFO) -> io::Result<Set> {
    if handle == INVALID_HANDLE_VALUE as isize {
        Err(io::Error::last_os_error())
    } else {
        Ok(Set(handle))
    }
}
fn data() -> SP_DEVINFO_DATA {
    SP_DEVINFO_DATA {
        cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
        ..Default::default()
    }
}
fn text(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|c| *c == 0).unwrap_or(value.len())])
}
fn get_property(set: HDEVINFO, dev: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<(u32, Vec<u8>)> {
    let mut bytes = vec![0u8; 2048];
    let mut kind = 0;
    let mut size = 0;
    if unsafe {
        SetupDiGetDevicePropertyW(
            set,
            dev,
            key,
            &mut kind,
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            &mut size,
            0,
        )
    } == 0
        || size as usize > bytes.len()
    {
        return None;
    }
    bytes.truncate(size as usize);
    Some((kind, bytes))
}
fn utf16_property(bytes: &[u8]) -> Option<Vec<u16>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    if units.last() != Some(&0) {
        return None;
    }
    Some(units)
}
fn instance(set: HDEVINFO, dev: &SP_DEVINFO_DATA) -> Option<String> {
    let mut value = [0u16; 256];
    if unsafe {
        SetupDiGetDeviceInstanceIdW(
            set,
            dev,
            value.as_mut_ptr(),
            value.len() as u32,
            ptr::null_mut(),
        )
    } == 0
    {
        None
    } else {
        Some(text(&value))
    }
}
fn owned(set: HDEVINFO, dev: &SP_DEVINFO_DATA, owner: &str) -> bool {
    let Some(id) = instance(set, dev) else {
        return false;
    };
    if ![Profile::DualSense, Profile::DualShock4]
        .into_iter()
        .any(|p| valid_root_test_instance(&id, p))
    {
        return false;
    }
    if get_property(set, dev, &MARKER) != Some((DEVPROP_TYPE_UINT32, 1u32.to_le_bytes().to_vec())) {
        return false;
    }
    let Some((kind, value)) = get_property(set, dev, &OWNER) else {
        return false;
    };
    if kind != DEVPROP_TYPE_STRING || utf16_property(&value).is_none_or(|v| text(&v) != owner) {
        return false;
    }
    let Some((kind, value)) = get_property(set, dev, &DEVPKEY_Device_HardwareIds) else {
        return false;
    };
    kind == DEVPROP_TYPE_STRING_LIST
        && utf16_property(&value).is_some_and(|v| {
            v.ends_with(&[0, 0])
                && v.split(|c| *c == 0)
                    .any(|s| text(s).eq_ignore_ascii_case(r"SCX\VirtualPad"))
        })
}
/// Explicit recovery visits only the two strict test namespaces AND requires
/// marker, installed owner SID, HID class and the SCX binding hardware ID.
pub(super) fn cleanup(owner: &str) -> io::Result<()> {
    let set = set(unsafe {
        SetupDiGetClassDevsW(&HID_CLASS, wide("ROOT").as_ptr(), ptr::null_mut(), 0)
    })?;
    let mut targets = Vec::new();
    for index in 0..4096 {
        let mut dev = data();
        if unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut dev) } == 0 {
            if unsafe { GetLastError() } == ERROR_NO_MORE_ITEMS {
                break;
            }
            return Err(io::Error::last_os_error());
        }
        if owned(set.0, &dev, owner) {
            targets.push(dev);
        }
    }
    for dev in targets {
        win(
            unsafe { SetupDiCallClassInstaller(DIF_REMOVE, set.0, &dev) },
            "remove stale owned ROOT test device",
        )?;
    }
    Ok(())
}
pub(super) struct RootDevice {
    set: Set,
    dev: SP_DEVINFO_DATA,
    pub(super) instance: String,
    registered: bool,
}
impl Drop for RootDevice {
    fn drop(&mut self) {
        if self.registered {
            let result = win(
                unsafe { SetupDiCallClassInstaller(DIF_REMOVE, self.set.0, &self.dev) },
                "remove owned ROOT device",
            );
            record(&format!(
                "remove {} success={}",
                self.instance,
                result.is_ok()
            ));
        } else {
            unsafe {
                SetupDiDeleteDeviceInfo(self.set.0, &self.dev);
            }
        }
    }
}
impl RootDevice {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn create(
        profile: Profile,
        identity: u64,
        slot: usize,
        owner: &str,
        hardware: &[u16],
        name: &[u16],
        container: &GUID,
        properties: &[DEVPROPERTY],
        stop: &Handle,
    ) -> io::Result<Self> {
        if slot >= 4 || !matches!(profile, Profile::DualSense | Profile::DualShock4) {
            return Err(fail("ROOT test supports four Sony slots only"));
        }
        let instance = format!(
            "ROOT\\{}\\SCX-ROOT-TEST-{identity:016X}-{slot:02}",
            device_enumerator(profile)
        );
        if !valid_root_test_instance(&instance, profile) {
            return Err(fail("invalid ROOT test identity"));
        }
        let set = set(unsafe { SetupDiCreateDeviceInfoList(&HID_CLASS, ptr::null_mut()) })?;
        let mut dev = data();
        win(
            unsafe {
                SetupDiCreateDeviceInfoW(
                    set.0,
                    wide(&instance).as_ptr(),
                    &HID_CLASS,
                    name.as_ptr(),
                    ptr::null_mut(),
                    0,
                    &mut dev,
                )
            },
            "create ROOT test devinfo",
        )?;
        let mut node = Self {
            set,
            dev,
            instance,
            registered: false,
        };
        win(
            unsafe {
                SetupDiSetDeviceRegistryPropertyW(
                    node.set.0,
                    &mut node.dev,
                    SPDRP_HARDWAREID,
                    hardware.as_ptr().cast(),
                    std::mem::size_of_val(hardware) as u32,
                )
            },
            "set own ROOT hardware IDs",
        )?;
        let acl = wide(&format!("O:SYG:SYD:P(A;;GA;;;SY)(A;;GRGW;;;{owner})"));
        win(
            unsafe {
                SetupDiSetDeviceRegistryPropertyW(
                    node.set.0,
                    &mut node.dev,
                    SPDRP_SECURITY_SDS,
                    acl.as_ptr().cast(),
                    (acl.len() * 2) as u32,
                )
            },
            "set owner ROOT ACL",
        )?;
        for property in properties {
            win(
                unsafe {
                    SetupDiSetDevicePropertyW(
                        node.set.0,
                        &node.dev,
                        &property.CompKey.Key,
                        property.Type,
                        property.Buffer.cast(),
                        property.BufferSize,
                        0,
                    )
                },
                "set ROOT bootstrap/identity property",
            )?;
        }
        win(
            unsafe {
                SetupDiSetDevicePropertyW(
                    node.set.0,
                    &node.dev,
                    &MARKER,
                    DEVPROP_TYPE_UINT32,
                    1u32.to_le_bytes().as_ptr(),
                    4,
                    0,
                )
            },
            "set ROOT experiment marker",
        )?;
        // ContainerId is read-only to installation applications. ROOT PnP
        // assigns it; unlike SwDeviceCreate, SetupDi has no container creation
        // argument. Do not write the property or force the registry. Verify
        // Windows' result against the existing Sony/Gears invariant below.
        // https://learn.microsoft.com/windows-hardware/drivers/install/devpkey-device-containerid
        let mut params = SP_DEVINSTALL_PARAMS_W {
            cbSize: size_of::<SP_DEVINSTALL_PARAMS_W>() as u32,
            ..Default::default()
        };
        win(
            unsafe { SetupDiGetDeviceInstallParamsW(node.set.0, &node.dev, &mut params) },
            "get install params",
        )?;
        params.Flags |= DI_QUIETINSTALL;
        win(
            unsafe { SetupDiSetDeviceInstallParamsW(node.set.0, &node.dev, &params) },
            "set quiet install params",
        )?;
        win(
            unsafe { SetupDiCallClassInstaller(DIF_REGISTERDEVICE, node.set.0, &node.dev) },
            "register own ROOT device",
        )?;
        node.registered = true;
        win(
            unsafe { SetupDiBuildDriverInfoList(node.set.0, &mut node.dev, SPDIT_COMPATDRIVER) },
            "list already installed compatible drivers",
        )?;
        let selected = (|| -> io::Result<SP_DRVINFO_DATA_V2_W> {
            for index in 0..256 {
                let mut driver = SP_DRVINFO_DATA_V2_W {
                    cbSize: size_of::<SP_DRVINFO_DATA_V2_W>() as u32,
                    ..Default::default()
                };
                if unsafe {
                    SetupDiEnumDriverInfoW(
                        node.set.0,
                        &node.dev,
                        SPDIT_COMPATDRIVER,
                        index,
                        &mut driver,
                    )
                } == 0
                {
                    break;
                }
                if driver.DriverVersion == DRIVER_VERSION
                    && text(&driver.ProviderName) == "SCX"
                    && text(&driver.Description) == "SCX Virtual Gamepad"
                {
                    return Ok(driver);
                }
            }
            Err(fail(
                "installed SCX driver 0.1.0.30 not found; no driver download or fallback",
            ))
        })();
        let install = selected.and_then(|driver| {
            let mut reboot = 0;
            win(
                unsafe {
                    DiInstallDevice(
                        ptr::null_mut(),
                        node.set.0,
                        &node.dev,
                        &driver,
                        DIIDFLAG_NOFINISHINSTALLUI,
                        &mut reboot,
                    )
                },
                "bind existing .30 driver to own ROOT node",
            )?;
            if reboot != 0 {
                return Err(fail("ROOT test requested reboot; refusing live comparison"));
            }
            Ok(())
        });
        unsafe {
            SetupDiDestroyDriverInfoList(node.set.0, &node.dev, SPDIT_COMPATDRIVER);
        }
        install?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let mut status = 0;
            let mut problem = 0;
            if unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, node.dev.DevInst, 0) }
                == CR_SUCCESS
                && status & DN_STARTED != 0
                && problem == 0
            {
                break;
            }
            if Instant::now() >= deadline
                || unsafe { WaitForSingleObject(stop.0, 25) } != WAIT_TIMEOUT
            {
                return Err(fail(&format!(
                    "ROOT start failed/timed out: status={status:x} problem={problem}"
                )));
            }
        }
        if !owned(node.set.0, &node.dev, owner) {
            return Err(fail("ROOT ownership read-back failed"));
        }
        let actual = get_property(node.set.0, &node.dev, &DEVPKEY_Device_ContainerId);
        let expected =
            unsafe { std::slice::from_raw_parts((container as *const GUID).cast::<u8>(), 16) };
        record(&format!("Windows-assigned ROOT container: {actual:?}"));
        if actual != Some((DEVPROP_TYPE_GUID, expected.to_vec())) {
            return Err(fail("ROOT container read-back differs"));
        }
        record(&format!("ready {} driver=0.1.0.30", node.instance));
        Ok(node)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_utf16_properties() {
        assert_eq!(utf16_property(&[65, 0, 0, 0]), Some(vec![65, 0]));
        for invalid in [&[][..], &[1][..], &[65, 0][..]] {
            assert!(utf16_property(invalid).is_none());
        }
    }
}
