//! Explicit diagnostic only: reads the live endpoint ACL and performs a scoped
//! restricted-token open. No persistent account, privilege or ACL is changed.
use crate::{
    win::{check, fail, wide, Handle},
    wire::PIPE_NAME,
};
use std::{io, ptr};
use windows_sys::Win32::{
    Devices::{
        DeviceAndDriverInstallation::*, HumanInterfaceDevice::HidD_SetFeature, Properties::*,
    },
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{
        Registry::*,
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

struct Local(*mut core::ffi::c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
pub(crate) fn descriptor(handle: &Handle) -> io::Result<String> {
    descriptor_kind(handle, SE_FILE_OBJECT)
}
fn descriptor_kind(handle: &Handle, object_kind: SE_OBJECT_TYPE) -> io::Result<String> {
    let flags = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut sd = ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            handle.0,
            object_kind,
            flags,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut sd,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let sd = Local(sd);
    let mut text = ptr::null_mut();
    let mut length = 0;
    check(unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            sd.0,
            SDDL_REVISION_1,
            flags,
            &mut text,
            &mut length,
        )
    })?;
    let allocation = Local(text.cast());
    if length == 0 || length > 32768 {
        return Err(fail("invalid security descriptor length"));
    }
    let chars = unsafe { std::slice::from_raw_parts(allocation.0.cast::<u16>(), length as usize) };
    let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
    String::from_utf16(&chars[..end]).map_err(|_| fail("invalid security descriptor text"))
}

pub(crate) fn restricted_writer_denied(path: &str, xusb: bool) -> io::Result<()> {
    let mut token = ptr::null_mut();
    check(unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            &mut token,
        )
    })?;
    let token = Handle::new(token)?;
    let kinds = [
        WinWorldSid,
        WinAuthenticatedUserSid,
        WinBuiltinUsersSid,
        WinInteractiveSid,
    ];
    // Restricting SIDs require a second independent allow check. The owner SID
    // is deliberately absent, while broad ACLs would still permit the open.
    let mut storage = [[0u32; 17]; 4];
    let mut sids = [SID_AND_ATTRIBUTES::default(); 4];
    for (i, kind) in kinds.iter().enumerate() {
        let mut size = size_of_val(&storage[i]) as u32;
        check(unsafe {
            CreateWellKnownSid(
                *kind,
                ptr::null_mut(),
                storage[i].as_mut_ptr().cast(),
                &mut size,
            )
        })?;
        sids[i] = SID_AND_ATTRIBUTES {
            Sid: storage[i].as_mut_ptr().cast(),
            Attributes: 0,
        };
    }
    let mut restricted = ptr::null_mut();
    check(unsafe {
        CreateRestrictedToken(
            token.0,
            DISABLE_MAX_PRIVILEGE,
            0,
            ptr::null(),
            0,
            ptr::null(),
            sids.len() as u32,
            sids.as_ptr(),
            &mut restricted,
        )
    })?;
    let restricted = Handle::new(restricted)?;
    probe_writer_denied(path, xusb, &restricted)
}

pub(crate) fn low_integrity_writer_denied(path: &str, xusb: bool) -> io::Result<()> {
    const SE_GROUP_INTEGRITY: u32 = 0x20;
    let mut original = ptr::null_mut();
    check(unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            &mut original,
        )
    })?;
    let original = Handle::new(original)?;
    let mut low = ptr::null_mut();
    check(unsafe {
        DuplicateTokenEx(
            original.0,
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE | TOKEN_ADJUST_DEFAULT,
            ptr::null(),
            SecurityImpersonation,
            TokenImpersonation,
            &mut low,
        )
    })?;
    let low = Handle::new(low)?;
    let mut storage = [0u64; 16];
    let label = storage.as_mut_ptr().cast::<TOKEN_MANDATORY_LABEL>();
    let sid = unsafe {
        storage
            .as_mut_ptr()
            .cast::<u8>()
            .add(size_of::<TOKEN_MANDATORY_LABEL>())
    }
    .cast();
    let mut sid_size = (size_of_val(&storage) - size_of::<TOKEN_MANDATORY_LABEL>()) as u32;
    check(unsafe { CreateWellKnownSid(WinLowLabelSid, ptr::null_mut(), sid, &mut sid_size) })?;
    unsafe {
        *label = TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: sid,
                Attributes: SE_GROUP_INTEGRITY,
            },
        };
    }
    check(unsafe {
        SetTokenInformation(
            low.0,
            TokenIntegrityLevel,
            label.cast(),
            size_of::<TOKEN_MANDATORY_LABEL>() as u32 + sid_size,
        )
    })?;
    probe_writer_denied(path, xusb, &low)
}

fn probe_writer_denied(path: &str, xusb: bool, restricted: &Handle) -> io::Result<()> {
    let channel = path.starts_with(r"Global\SCX.VPad.");
    let path = wide(path);
    check(unsafe { ImpersonateLoggedOnUser(restricted.0) })?;
    struct Revert;
    impl Drop for Revert {
        fn drop(&mut self) {
            // Never continue under an unexpected thread identity.
            if unsafe { RevertToSelf() } == 0 {
                std::process::abort();
            }
        }
    }
    let guard = Revert;
    let pipe = Handle::new(unsafe {
        CreateFileW(
            wide(PIPE_NAME).as_ptr(),
            0x12019b,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        )
    });
    match pipe {
        Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {}
        Err(e) => {
            return Err(fail(&format!(
                "negative capability-issuance test returned {e}; expected ACCESS_DENIED"
            )))
        }
        Ok(_) => {
            return Err(fail(
                "SECURITY FAILURE: untrusted token opened the lease pipe",
            ))
        }
    }
    if channel {
        let opened = Handle::new(unsafe {
            windows_sys::Win32::System::Memory::OpenFileMappingW(
                windows_sys::Win32::System::Memory::FILE_MAP_WRITE,
                0,
                path.as_ptr(),
            )
        });
        drop(guard);
        return match opened {
            Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => Ok(()),
            Err(e) => Err(e),
            Ok(_) => Err(fail(
                "SECURITY FAILURE: restricted token opened channel for writing",
            )),
        };
    }
    let opened = Handle::new(unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        )
    });
    let result = match opened {
        Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => Ok(()),
        Err(e) => Err(fail(&format!(
            "negative producer-open test returned {e}; expected ACCESS_DENIED"
        ))),
        Ok(handle) if !xusb => {
            let state = scx_vpad_protocol::InputState {
                sequence: 2,
                ..scx_vpad_protocol::InputState::NEUTRAL
            };
            let mut report = [0u8; scx_vpad_protocol::HID_FEATURE_REPORT_SIZE];
            report[0] = scx_vpad_protocol::SUBMIT_FEATURE_ID;
            // Unknown capability is all zero. Only the medium-integrity owner
            // can receive a real capability from the authenticated lease pipe.
            report[33..].copy_from_slice(&state.encode());
            match check(unsafe {
                HidD_SetFeature(handle.0, report.as_mut_ptr().cast(), report.len() as u32)
            } as i32)
            {
                Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => Ok(()),
                Err(e) => Err(fail(&format!(
                    "negative producer submission returned {e}; expected ACCESS_DENIED"
                ))),
                Ok(()) => Err(fail(
                    "SECURITY FAILURE: a restricted broad-user token submitted a producer packet",
                )),
            }
        }
        Ok(_) => Err(fail(
            "SECURITY FAILURE: a restricted broad-user token opened the XUSB producer endpoint",
        )),
    };
    drop(guard);
    result
}

pub(crate) fn bootstrap_unreadable(instance: &str) -> io::Result<()> {
    let mut instance = wide(instance);
    let mut node = 0;
    if unsafe { CM_Locate_DevNodeW(&mut node, instance.as_mut_ptr(), CM_LOCATE_DEVNODE_NORMAL) }
        != CR_SUCCESS
    {
        return Err(fail("owned device unavailable for bootstrap ACL test"));
    }
    let key = DEVPROPKEY {
        fmtid: windows_sys::core::GUID::from_u128(0x19f81049_3679_40f2_9689_2a2f61f8bd29),
        pid: 6,
    };
    let mut kind = 0;
    let mut id = [0u16; 33];
    let mut size = size_of_val(&id) as u32;
    if unsafe {
        CM_Get_DevNode_PropertyW(node, &key, &mut kind, id.as_mut_ptr().cast(), &mut size, 0)
    } != CR_SUCCESS
        || kind != DEVPROP_TYPE_STRING
        || size != size_of_val(&id) as u32
        || id[32] != 0
    {
        return Err(fail("invalid bootstrap identifier property"));
    }
    let id = String::from_utf16(&id[..32]).map_err(|_| fail("invalid bootstrap identifier"))?;
    if !id
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(fail("invalid bootstrap identifier"));
    }
    let mut opened = ptr::null_mut();
    let code = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            wide(&format!(r"SOFTWARE\SCX\VirtualPad\Bootstrap\{id}")).as_ptr(),
            0,
            KEY_QUERY_VALUE | KEY_WOW64_64KEY,
            &mut opened,
        )
    };
    if code == ERROR_ACCESS_DENIED {
        return Ok(());
    }
    if code == ERROR_SUCCESS {
        unsafe {
            RegCloseKey(opened);
        }
        return Err(fail(
            "SECURITY FAILURE: normal owner can read protected driver bootstrap",
        ));
    }
    Err(io::Error::from_raw_os_error(code as i32))
}

pub(crate) fn channel_descriptor(name: &str) -> io::Result<String> {
    let h = Handle::new(unsafe {
        windows_sys::Win32::System::Memory::OpenFileMappingW(READ_CONTROL, 0, wide(name).as_ptr())
    })?;
    descriptor_kind(&h, SE_KERNEL_OBJECT)
}
