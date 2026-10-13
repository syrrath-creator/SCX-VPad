use crate::{win::*, wire::*};
use scx_vpad_protocol::{
    DeviceInfo, Feedback, InputState, Profile, HID_FEATURE_REPORT_SIZE, SUBMIT_FEATURE_ID,
};
use std::{
    io, ptr,
    time::{Duration, Instant},
};
use windows_sys::{
    core::GUID,
    Win32::{
        Devices::{DeviceAndDriverInstallation::*, HumanInterfaceDevice::*},
        Foundation::*,
        Storage::FileSystem::*,
        System::{Pipes::*, Services::*, Threading::ResetEvent, IO::*},
    },
};

const HID_GUID: GUID = GUID::from_u128(0x4d1e55b2_f16f_11cf_88cb_001111000030);
const XUSB_CONTROL_GUID: GUID = GUID::from_u128(0x582a87fd_a2dd_43f1_9224_d7c0dfdcf45d);
const XUSB_SUBMIT: u32 = 0x22a000;
const XUSB_INFO: u32 = 0x226004;
const XUSB_FEEDBACK_WAIT: u32 = 0x226008;
const XUSB_FEEDBACK_GET: u32 = 0x22600c;

/// Holds one service lease and one direct HID channel. Dropping it removes only
/// this controller. A crash closes the pipe, which also releases the device.
pub struct VirtualPad {
    device: Handle,
    pipe: Handle,
    lease: u64,
    instance: String,
    control_path: String,
    sequence: u32,
    profile: Profile,
    io_event: Handle,
    capability: Capability,
}
impl VirtualPad {
    pub fn create() -> io::Result<Self> {
        Self::create_profile(Profile::GenericHid)
    }
    pub fn create_profile(profile: Profile) -> io::Result<Self> {
        if !profile.is_supported() {
            return Err(fail("output profile is not implemented"));
        }
        let pipe = connect()?;
        let response = exchange(
            &pipe,
            Request {
                op: OP_CREATE,
                profile: profile as u32,
                lease: 0,
            },
        )?;
        if response.lease == 0
            || !response
                .instance
                .to_ascii_uppercase()
                .starts_with("SWD\\SCXVPAD\\")
        {
            return Err(fail("invalid device lease"));
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        let (device, control_path) = loop {
            if let Some(endpoint) =
                find_endpoint(&response.instance, true, profile == Profile::Xbox360)?
            {
                break endpoint;
            }
            if Instant::now() >= deadline {
                return Err(fail(
                    "SCX HID interface unavailable; verify the SCX driver installation",
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        if (profile != Profile::Xbox360) == response.capability.is_empty() {
            return Err(fail("invalid capability for requested profile"));
        }
        let mut info = [0u8; HID_FEATURE_REPORT_SIZE];
        info[0] = 0x7f;
        let io_event = Handle::event()?;
        if profile == Profile::Xbox360 {
            if ioctl(&device, &io_event, XUSB_INFO, &[], &mut info[1..33], 2000)? != 32 {
                return Err(fail("invalid XUSB info length"));
            }
        } else {
            check(
                unsafe { HidD_GetFeature(device.0, info.as_mut_ptr().cast(), info.len() as u32) }
                    as i32,
            )?;
        }
        let info = DeviceInfo::decode(&info[1..33])
            .map_err(|_| fail("incompatible SCX virtual-pad driver"))?;
        if info.profile != profile {
            return Err(fail("driver profile differs from requested profile"));
        }
        Ok(Self {
            device,
            pipe,
            lease: response.lease,
            instance: response.instance,
            control_path,
            sequence: 0,
            profile,
            io_event,
            capability: response.capability,
        })
    }
    pub fn instance_id(&self) -> &str {
        &self.instance
    }
    /// Inspect the real producer endpoint, including HID collection security.
    /// This does not modify ACLs or grant permissions.
    pub fn control_security(&self) -> io::Result<String> {
        crate::security::descriptor(&self.device)
    }
    /// Bounded diagnostic stages only; no token/SID or credential is exposed.
    pub fn authorization_diagnostic(&self) -> io::Result<String> {
        if self.profile == Profile::Xbox360 {
            return Err(fail("HID authorization diagnostic not available for XUSB"));
        }
        let mut bytes = [0u8; HID_FEATURE_REPORT_SIZE];
        bytes[0] = 0x7c;
        check(unsafe {
            HidD_GetFeature(self.device.0, bytes.as_mut_ptr().cast(), bytes.len() as u32)
        } as i32)?;
        if u16::from_le_bytes([bytes[1], bytes[2]]) != 1
            || u16::from_le_bytes([bytes[3], bytes[4]]) != 16
        {
            return Err(fail("invalid authorization diagnostic"));
        }
        let value = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("fixed bounds"));
        Ok(format!(
            "authorization stage={} error=0x{:08x} requestor_mode={}",
            value(5),
            value(9),
            value(13)
        ))
    }
    /// Development diagnostic for HIDClass routing. Sends only a neutral packet
    /// to this lease's own collection, never to an external controller.
    pub fn probe_hid_route(&self, game_collection: bool, zero_access: bool) -> io::Result<String> {
        if self.profile == Profile::Xbox360 {
            return Err(fail("HID route probe requires HID profile"));
        }
        let (_, path) = find_endpoint(&self.instance, !game_collection, false)?
            .ok_or_else(|| fail("own HID endpoint missing"))?;
        let device = Handle::new(unsafe {
            CreateFileW(
                wide(&path).as_ptr(),
                if zero_access {
                    0
                } else {
                    GENERIC_READ | GENERIC_WRITE
                },
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                ptr::null_mut(),
            )
        })?;
        let before = self.authorization_diagnostic()?;
        let mut report = [0u8; 65];
        report[0] = 0x7e;
        report[1..].copy_from_slice(
            &InputState {
                sequence: 2,
                ..InputState::NEUTRAL
            }
            .encode(),
        );
        let result = check(unsafe {
            HidD_SetFeature(device.0, report.as_mut_ptr().cast(), report.len() as u32)
        } as i32);
        Ok(format!("game_collection={game_collection} zero_access={zero_access}; before {before}; SetFeature={result:?}; after {}", self.authorization_diagnostic()?))
    }
    /// Negative access test using an ephemeral restricted token whose access
    /// must be allowed by broad user groups rather than the installed owner.
    /// Success means Windows denied the new producer handle or the driver denied
    /// a neutral input packet with ACCESS_DENIED. A HID consumer handle may be public.
    pub fn verify_restricted_writer_denied(&self) -> io::Result<()> {
        crate::security::restricted_writer_denied(
            &self.control_path,
            self.profile == Profile::Xbox360,
        )
    }
    /// Repeats the negative write test with the same owner SID at low integrity.
    /// Only an ephemeral duplicate token is lowered; the caller stays unchanged.
    pub fn verify_low_integrity_writer_denied(&self) -> io::Result<()> {
        crate::security::low_integrity_writer_denied(
            &self.control_path,
            self.profile == Profile::Xbox360,
        )
    }
    pub fn verify_bootstrap_unreadable(&self) -> io::Result<()> {
        if self.profile == Profile::Xbox360 {
            return Ok(());
        }
        crate::security::bootstrap_unreadable(&self.instance)
    }
    /// Explicit neutral-only negative tests on newly owned devices. The secret
    /// never leaves this module or appears in test output.
    pub fn verify_capability_rejections(&mut self) -> io::Result<()> {
        if self.profile == Profile::Xbox360 {
            return Ok(());
        }
        let other = Self::create_profile(self.profile)?;
        let state = InputState {
            sequence: self.sequence.wrapping_add(1),
            ..InputState::NEUTRAL
        };
        let zero_handle = open_path(&self.control_path, 0)?;
        let mut wrong = Capability::new(*self.capability.bytes());
        let mut wrong_bytes = *wrong.bytes();
        wrong_bytes[0] ^= 0x80;
        wrong = Capability::new(wrong_bytes);
        wipe(&mut wrong_bytes);
        for capability in [&Capability::default(), &wrong, &other.capability] {
            denied(
                submit_authenticated(&zero_handle, capability, &state),
                &[ERROR_ACCESS_DENIED],
                "zero/wrong/cross-device capability",
            )?;
        }
        let (_, path) = find_endpoint(&self.instance, false, false)?
            .ok_or_else(|| fail("own game collection missing"))?;
        for access in [0, GENERIC_READ | GENERIC_WRITE] {
            let game = open_path(&path, access)?;
            denied(
                submit_authenticated(&game, &self.capability, &state),
                &[
                    ERROR_INVALID_FUNCTION,
                    ERROR_NOT_SUPPORTED,
                    ERROR_INVALID_PARAMETER,
                    ERROR_ACCESS_DENIED,
                ],
                "producer packet via game collection",
            )?;
        }
        let mut legacy = [0u8; HID_FEATURE_REPORT_SIZE];
        legacy[0] = 0x7e;
        legacy[1..65].copy_from_slice(&state.encode());
        denied(
            check(unsafe {
                HidD_SetFeature(
                    zero_handle.0,
                    legacy.as_mut_ptr().cast(),
                    legacy.len() as u32,
                )
            } as i32),
            &[
                ERROR_INVALID_FUNCTION,
                ERROR_NOT_SUPPORTED,
                ERROR_INVALID_PARAMETER,
                ERROR_ACCESS_DENIED,
            ],
            "legacy unauthenticated feature",
        )?;
        self.neutral()?;
        // Invalid capabilities must not consume a valid packet's sequence.
        denied(
            submit_authenticated(
                &self.device,
                &self.capability,
                &InputState {
                    sequence: self.sequence,
                    ..InputState::NEUTRAL
                },
            ),
            &[ERROR_INVALID_PARAMETER],
            "replayed authenticated packet",
        )?;
        // An independently issued capability expires with its lease and must
        // not authorize a later device, even when the service reuses a slot.
        let old = Capability::new(*other.capability.bytes());
        drop(other);
        let mut fresh = Self::create_profile(self.profile)?;
        fresh.neutral()?;
        denied(
            submit_authenticated(
                &fresh.device,
                &old,
                &InputState {
                    sequence: 2,
                    ..InputState::NEUTRAL
                },
            ),
            &[ERROR_ACCESS_DENIED],
            "retired lease capability",
        )?;
        Ok(())
    }
    /// Synchronous, allocation-free submission of one normalized report. Timing
    /// this measures API completion, not physical input-to-screen latency.
    /// Resubmit unchanged held state at least every 250 ms: the driver's safety
    /// watchdog neutralizes a stale source after 500 ms.
    pub fn submit(&mut self, state: &InputState) -> io::Result<()> {
        let mut state = *state;
        self.sequence = self.sequence.wrapping_add(1);
        state.sequence = self.sequence;
        let bytes = state.encode();
        InputState::decode(&bytes).map_err(|_| fail("invalid input state"))?;
        if self.profile == Profile::Xbox360 {
            ioctl(
                &self.device,
                &self.io_event,
                XUSB_SUBMIT,
                &bytes,
                &mut [],
                2000,
            )?;
            return Ok(());
        }
        submit_authenticated(&self.device, &self.capability, &state)
    }
    pub fn neutral(&mut self) -> io::Result<()> {
        self.submit(&InputState::NEUTRAL)
    }
    pub fn feedback_snapshot(&mut self) -> io::Result<Feedback> {
        if self.profile == Profile::GenericHid {
            return Err(fail("this profile has no output feedback"));
        }
        let mut report = [0u8; HID_FEATURE_REPORT_SIZE];
        report[0] = 0x7d;
        if self.profile == Profile::Xbox360 {
            if ioctl(
                &self.device,
                &self.io_event,
                XUSB_FEEDBACK_GET,
                &[],
                &mut report[1..33],
                2000,
            )? != 32
            {
                return Err(fail("invalid XUSB feedback length"));
            }
        } else {
            check(unsafe {
                HidD_GetFeature(
                    self.device.0,
                    report.as_mut_ptr().cast(),
                    report.len() as u32,
                )
            } as i32)?;
        }
        let feedback =
            Feedback::decode(&report[1..33]).map_err(|_| fail("invalid feedback packet"))?;
        if feedback.profile != self.profile {
            return Err(fail("mismatched feedback profile"));
        }
        Ok(feedback)
    }
}
impl Drop for VirtualPad {
    fn drop(&mut self) {
        // Closing the lease is sufficient and cannot deadlock behind a failed
        // service response. Explicit release is best effort and bounded.
        let mut request = Request {
            op: OP_RELEASE,
            profile: 0,
            lease: self.lease,
        }
        .encode();
        let _ = transfer(&self.pipe, &mut request, true, ptr::null_mut(), 250);
    }
}

pub fn service_status() -> io::Result<()> {
    let pipe = connect()?;
    exchange(
        &pipe,
        Request {
            op: OP_STATUS,
            profile: 0,
            lease: 0,
        },
    )?;
    Ok(())
}
fn exchange(pipe: &Handle, request: Request) -> io::Result<Response> {
    let mut bytes = request.encode();
    if transfer(pipe, &mut bytes, true, ptr::null_mut(), 5000)? != bytes.len() {
        return Err(fail("short service write"));
    }
    let mut response = [0u8; MAX_RESPONSE];
    let received = transfer(pipe, &mut response, false, ptr::null_mut(), 15000);
    let n = match received {
        Ok(n) => n,
        Err(e) => {
            wipe(&mut response);
            return Err(e);
        }
    };
    let decoded = Response::decode(&response[..n]);
    wipe(&mut response);
    let r = decoded?;
    if r.op != request.op {
        return Err(fail("mismatched service response"));
    }
    if r.status != STATUS_OK {
        return Err(fail(&format!(
            "SCX service rejected request (status {}): {}",
            r.status, r.instance
        )));
    }
    Ok(r)
}
fn open_path(path: &str, access: u32) -> io::Result<Handle> {
    Handle::new(unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        )
    })
}
fn denied(result: io::Result<()>, allowed_errors: &[u32], label: &str) -> io::Result<()> {
    match result {
        Err(e)
            if e.raw_os_error()
                .is_some_and(|code| allowed_errors.contains(&(code as u32))) =>
        {
            Ok(())
        }
        Err(e) => Err(fail(&format!("{label}: unexpected error {e}"))),
        Ok(()) => Err(fail(&format!("SECURITY FAILURE: {label} accepted"))),
    }
}
fn submit_authenticated(
    device: &Handle,
    capability: &Capability,
    state: &InputState,
) -> io::Result<()> {
    let mut report = [0u8; HID_FEATURE_REPORT_SIZE];
    report[0] = SUBMIT_FEATURE_ID;
    report[1..33].copy_from_slice(capability.bytes());
    report[33..].copy_from_slice(&state.encode());
    let result =
        check(
            unsafe { HidD_SetFeature(device.0, report.as_mut_ptr().cast(), report.len() as u32) }
                as i32,
        );
    wipe(&mut report[1..33]);
    result
}
fn connect() -> io::Result<Handle> {
    start_service()?;
    let name = wide(PIPE_NAME);
    // Identification-level impersonation lets the service authenticate callers
    // but prevents it acting on the user's behalf against unrelated resources.
    let open = || {
        Handle::new(unsafe {
            CreateFileW(
                name.as_ptr(),
                0x12019b,
                0,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                ptr::null_mut(),
            )
        })
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let h = loop {
        match open() {
            Ok(h) => break h,
            Err(e)
                if Instant::now() < deadline
                    && (e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                        || e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32)) =>
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(e),
        }
    };
    let mode = PIPE_READMODE_MESSAGE;
    check(unsafe { SetNamedPipeHandleState(h.0, &mode, ptr::null(), ptr::null()) })?;
    verify_service(&h)?;
    Ok(h)
}
struct ScHandle(SC_HANDLE);
impl Drop for ScHandle {
    fn drop(&mut self) {
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}
fn start_service() -> io::Result<()> {
    let manager = unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(io::Error::last_os_error());
    }
    let manager = ScHandle(manager);
    let service = unsafe {
        OpenServiceW(
            manager.0,
            wide(SERVICE_NAME).as_ptr(),
            SERVICE_START | SERVICE_QUERY_STATUS,
        )
    };
    if service.is_null() {
        return Err(io::Error::last_os_error());
    }
    let service = ScHandle(service);
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut requested = false;
    loop {
        let mut status = SERVICE_STATUS_PROCESS::default();
        let mut needed = 0;
        check(unsafe {
            QueryServiceStatusEx(
                service.0,
                SC_STATUS_PROCESS_INFO,
                (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
                size_of::<SERVICE_STATUS_PROCESS>() as u32,
                &mut needed,
            )
        })?;
        if status.dwCurrentState == SERVICE_RUNNING {
            return Ok(());
        }
        if status.dwCurrentState == SERVICE_STOPPED && !requested {
            let ok = unsafe { StartServiceW(service.0, 0, ptr::null()) };
            if ok == 0 && unsafe { GetLastError() } != ERROR_SERVICE_ALREADY_RUNNING {
                return Err(io::Error::last_os_error());
            }
            requested = true;
        } else if status.dwCurrentState == SERVICE_STOPPED && requested {
            return Err(fail(
                "SCX service failed to start; verify installation/owner SID",
            ));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "SCX service start timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn verify_service(pipe: &Handle) -> io::Result<()> {
    let mut pid = 0;
    check(unsafe { GetNamedPipeServerProcessId(pipe.0, &mut pid) })?;
    let manager = unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(io::Error::last_os_error());
    }
    let manager = ScHandle(manager);
    let service =
        unsafe { OpenServiceW(manager.0, wide(SERVICE_NAME).as_ptr(), SERVICE_QUERY_STATUS) };
    if service.is_null() {
        return Err(io::Error::last_os_error());
    }
    let service = ScHandle(service);
    let mut status = SERVICE_STATUS_PROCESS::default();
    let mut needed = 0;
    check(unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        )
    })?;
    if status.dwCurrentState != SERVICE_RUNNING || status.dwProcessId != pid || pid == 0 {
        return Err(fail("pipe server is not the installed SCX service"));
    }
    Ok(())
}
fn find_collection(instance: &str, control: bool, xusb: bool) -> io::Result<Option<Handle>> {
    Ok(find_endpoint(instance, control, xusb)?.map(|(handle, _)| handle))
}
fn find_endpoint(
    instance: &str,
    control: bool,
    xusb: bool,
) -> io::Result<Option<(Handle, String)>> {
    let guid = if xusb { &XUSB_CONTROL_GUID } else { &HID_GUID };
    let set = unsafe {
        SetupDiGetClassDevsW(
            guid,
            ptr::null(),
            ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };
    if set == -1 {
        return Err(io::Error::last_os_error());
    }
    struct DevSet(HDEVINFO);
    impl Drop for DevSet {
        fn drop(&mut self) {
            unsafe {
                SetupDiDestroyDeviceInfoList(self.0);
            }
        }
    }
    let set = DevSet(set);
    for index in 0..4096 {
        let mut interface = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        if unsafe { SetupDiEnumDeviceInterfaces(set.0, ptr::null(), guid, index, &mut interface) }
            == 0
        {
            if unsafe { GetLastError() } == ERROR_NO_MORE_ITEMS {
                break;
            }
            return Err(io::Error::last_os_error());
        }
        let mut needed = 0;
        unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set.0,
                &interface,
                ptr::null_mut(),
                0,
                &mut needed,
                ptr::null_mut(),
            );
        }
        if !(8..=32768).contains(&needed) {
            continue;
        }
        // Vec<u64> ensures SP_DEVICE_INTERFACE_DETAIL_DATA_W alignment on x64.
        let mut storage = vec![0u64; (needed as usize).div_ceil(8)];
        let detail = storage
            .as_mut_ptr()
            .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
        unsafe {
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        }
        let mut devinfo = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        check(unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set.0,
                &interface,
                detail,
                needed,
                ptr::null_mut(),
                &mut devinfo,
            )
        })?;
        if !belongs_to(devinfo.DevInst, instance) {
            continue;
        }
        let path = unsafe { (*detail).DevicePath.as_ptr() };
        let path_chars = unsafe { std::slice::from_raw_parts(path, (needed as usize - 4) / 2) };
        let path_length = path_chars
            .iter()
            .position(|&c| c == 0)
            .ok_or_else(|| fail("unterminated Windows interface path"))?;
        let path_string = String::from_utf16(&path_chars[..path_length])
            .map_err(|_| fail("invalid Windows interface path"))?;
        // Sharing does not grant access. Windows checks this endpoint's actual
        // ACL; HID collection ACLs can differ from the software device PDO.
        let Ok(h) = Handle::new(unsafe {
            CreateFileW(
                path,
                if control {
                    GENERIC_READ | GENERIC_WRITE
                } else {
                    GENERIC_READ
                },
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                ptr::null_mut(),
            )
        }) else {
            continue;
        };
        if xusb {
            return Ok(Some((h, path_string)));
        }
        let mut preparsed = 0;
        if !unsafe { HidD_GetPreparsedData(h.0, &mut preparsed) } {
            continue;
        }
        let mut caps = HIDP_CAPS::default();
        let result = unsafe { HidP_GetCaps(preparsed, &mut caps) };
        unsafe {
            HidD_FreePreparsedData(preparsed);
        }
        if result == HIDP_STATUS_SUCCESS
            && if control {
                caps.UsagePage == 0xff53
                    && caps.Usage == 1
                    && caps.FeatureReportByteLength as usize == HID_FEATURE_REPORT_SIZE
            } else {
                caps.UsagePage == 1
                    && caps.Usage == 5
                    && matches!(caps.InputReportByteLength, 18 | 64)
            }
        {
            return Ok(Some((h, path_string)));
        }
    }
    Ok(None)
}

/// Event-driven feedback reader for experimental DS4/DS5 profiles. Blocking
/// waits consume no CPU; a timeout cancels and drains the Windows request before
/// returning, so the reader can safely be reused or moved to an input worker.
pub struct FeedbackReader {
    device: Handle,
    event: Handle,
    profile: Profile,
    last_sequence: u32,
}
impl FeedbackReader {
    pub fn open(pad: &VirtualPad) -> io::Result<Self> {
        if pad.profile == Profile::GenericHid {
            return Err(fail("this profile has no output feedback"));
        }
        let device = find_collection(pad.instance_id(), true, pad.profile == Profile::Xbox360)?
            .ok_or_else(|| fail("control HID collection unavailable"))?;
        Ok(Self {
            device,
            event: Handle::event()?,
            profile: pad.profile,
            last_sequence: 0,
        })
    }
    pub fn read(&mut self, timeout_ms: u32) -> io::Result<Feedback> {
        if self.profile == Profile::Xbox360 {
            let mut report = [0u8; 32];
            if ioctl(
                &self.device,
                &self.event,
                XUSB_FEEDBACK_WAIT,
                &self.last_sequence.to_le_bytes(),
                &mut report,
                timeout_ms,
            )? != 32
            {
                return Err(fail("invalid XUSB feedback length"));
            }
            let feedback =
                Feedback::decode(&report).map_err(|_| fail("invalid XUSB feedback packet"))?;
            if feedback.profile != self.profile {
                return Err(fail("mismatched feedback profile"));
            }
            self.last_sequence = feedback.sequence;
            return Ok(feedback);
        }
        check(unsafe { ResetEvent(self.event.0) })?;
        let mut ov = OVERLAPPED {
            hEvent: self.event.0,
            ..Default::default()
        };
        let mut report = [0u8; 33];
        let mut n = 0;
        if unsafe {
            ReadFile(
                self.device.0,
                report.as_mut_ptr(),
                report.len() as u32,
                &mut n,
                &mut ov,
            )
        } == 0
        {
            if unsafe { GetLastError() } != ERROR_IO_PENDING {
                return Err(io::Error::last_os_error());
            }
            n = wait_io(&self.device, &mut ov, ptr::null_mut(), timeout_ms)?;
        }
        if n != report.len() as u32 || report[0] != 0x7d {
            return Err(fail("invalid feedback report size/ID"));
        }
        let feedback =
            Feedback::decode(&report[1..]).map_err(|_| fail("invalid feedback packet"))?;
        if feedback.profile != self.profile {
            return Err(fail("mismatched feedback profile"));
        }
        Ok(feedback)
    }
}

/// Native HID consumer used by the explicit hardware verifier. This is not on
/// SCX's normal output path. It tests what a game-side HID reader receives.
pub struct GamepadReader<const N: usize = 18> {
    device: Handle,
    event: Handle,
    profile: Profile,
}
impl<const N: usize> GamepadReader<N> {
    /// Interpret a captured report through Windows' descriptor-based HID parser.
    /// Diagnostic only: catches semantic descriptor errors which raw-byte tests miss.
    pub fn windows_controls(&self, report: &[u8; N]) -> io::Result<([u32; 7], Vec<u16>)> {
        let mut preparsed = 0;
        check(unsafe { HidD_GetPreparsedData(self.device.0, &mut preparsed) } as i32)?;
        let result = (|| {
            let mut values = [0u32; 7];
            for (index, usage) in [0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x39]
                .into_iter()
                .enumerate()
            {
                let status = unsafe {
                    HidP_GetUsageValue(
                        HidP_Input,
                        1,
                        0,
                        usage,
                        &mut values[index],
                        preparsed,
                        report.as_ptr(),
                        N as u32,
                    )
                };
                if status != HIDP_STATUS_SUCCESS {
                    return Err(fail(&format!("HID usage {usage:#x}: {status:#x}")));
                }
            }
            let mut buttons = [0u16; 32];
            let mut count = buttons.len() as u32;
            let mut button_report = *report;
            let status = unsafe {
                HidP_GetUsages(
                    HidP_Input,
                    9,
                    0,
                    buttons.as_mut_ptr(),
                    &mut count,
                    preparsed,
                    button_report.as_mut_ptr(),
                    N as u32,
                )
            };
            if status != HIDP_STATUS_SUCCESS {
                return Err(fail(&format!("HID buttons: {status:#x}")));
            }
            Ok((values, buttons[..count as usize].to_vec()))
        })();
        unsafe {
            HidD_FreePreparsedData(preparsed);
        }
        result
    }

    pub fn open(pad: &VirtualPad) -> io::Result<Self> {
        let expected = match pad.profile {
            Profile::GenericHid => 18,
            Profile::DualShock4 | Profile::DualSense => 64,
            _ => 0,
        };
        if N != expected || expected == 0 {
            return Err(fail("reader length/profile mismatch"));
        }
        let (device, path) = find_endpoint(pad.instance_id(), false, false)?
            .ok_or_else(|| fail("gamepad HID collection unavailable"))?;
        let device = if pad.profile != Profile::GenericHid {
            Handle::new(unsafe {
                CreateFileW(
                    wide(&path).as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    ptr::null_mut(),
                )
            })?
        } else {
            device
        };
        Ok(Self {
            device,
            event: Handle::event()?,
            profile: pad.profile,
        })
    }
    pub fn snapshot(&self) -> io::Result<[u8; N]> {
        let mut b = [0; N];
        b[0] = 1;
        check(
            unsafe { HidD_GetInputReport(self.device.0, b.as_mut_ptr().cast(), b.len() as u32) }
                as i32,
        )?;
        Ok(b)
    }
    pub fn read(&mut self, timeout: u32) -> io::Result<[u8; N]> {
        self.read_after(timeout, || Ok(()))
    }
    pub fn read_after(
        &mut self,
        timeout: u32,
        action: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<[u8; N]> {
        check(unsafe { ResetEvent(self.event.0) })?;
        let mut ov = OVERLAPPED {
            hEvent: self.event.0,
            ..Default::default()
        };
        let mut report = [0u8; N];
        let mut n = 0;
        let ok = unsafe {
            ReadFile(
                self.device.0,
                report.as_mut_ptr(),
                report.len() as u32,
                &mut n,
                &mut ov,
            )
        };
        let pending = ok == 0 && unsafe { GetLastError() } == ERROR_IO_PENDING;
        if ok == 0 && !pending {
            return Err(io::Error::last_os_error());
        }
        let action_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action));
        match action_result {
            Ok(Ok(())) => {}
            failure => {
                if pending {
                    unsafe {
                        CancelIoEx(self.device.0, &ov);
                        GetOverlappedResult(self.device.0, &ov, &mut n, 1);
                    }
                }
                match failure {
                    Ok(Err(error)) => return Err(error),
                    Err(panic) => std::panic::resume_unwind(panic),
                    _ => unreachable!(),
                }
            }
        }
        if pending {
            n = wait_io(&self.device, &mut ov, ptr::null_mut(), timeout)?;
        }
        if n != report.len() as u32 {
            return Err(fail("unexpected HID report size"));
        }
        Ok(report)
    }
    /// Requests one allowlisted Sony feature through the game-facing HID API.
    pub fn feature(&self, id: u8) -> io::Result<Vec<u8>> {
        let length = match (self.profile, id) {
            (Profile::DualShock4, 2 | 0x12 | 0xa3) => 49,
            (Profile::DualSense, 5 | 9 | 0x20) => 64,
            _ => return Err(fail("unsupported consumer feature")),
        };
        let mut bytes = vec![0; length];
        bytes[0] = id;
        check(unsafe {
            HidD_GetFeature(self.device.0, bytes.as_mut_ptr().cast(), bytes.len() as u32)
        } as i32)?;
        Ok(bytes)
    }
    /// Sends a fixed-length Sony output to the game-facing HID collection.
    pub fn output(&self, bytes: &mut [u8]) -> io::Result<()> {
        if !matches!(
            (self.profile, bytes.len(), bytes.first()),
            (Profile::DualShock4, 32, Some(5)) | (Profile::DualSense, 63, Some(2))
        ) {
            return Err(fail("invalid consumer output report"));
        }
        check(unsafe {
            HidD_SetOutputReport(self.device.0, bytes.as_mut_ptr().cast(), bytes.len() as u32)
        } as i32)
    }
}
pub fn device_present(instance: &str) -> bool {
    let mut id = wide(instance);
    let mut node = 0;
    (unsafe { CM_Locate_DevNodeW(&mut node, id.as_mut_ptr(), CM_LOCATE_DEVNODE_NORMAL) })
        == CR_SUCCESS
}
fn belongs_to(mut node: u32, instance: &str) -> bool {
    for _ in 0..16 {
        let mut id = [0u16; MAX_INSTANCE_CHARS];
        if unsafe { CM_Get_Device_IDW(node, id.as_mut_ptr(), id.len() as u32, 0) } == CR_SUCCESS {
            let n = id.iter().position(|&x| x == 0).unwrap_or(id.len());
            if String::from_utf16_lossy(&id[..n]).eq_ignore_ascii_case(instance) {
                return true;
            }
        }
        let mut parent = 0;
        if unsafe { CM_Get_Parent(&mut parent, node, 0) } != CR_SUCCESS {
            break;
        }
        node = parent;
    }
    false
}
