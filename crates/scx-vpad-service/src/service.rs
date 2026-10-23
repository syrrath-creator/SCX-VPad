use scx_vpad_client::{win::*, wire::*};
use scx_vpad_protocol::Profile;
use std::{
    io, ptr,
    sync::{
        atomic::{AtomicPtr, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use windows_sys::{
    core::GUID,
    Win32::{
        Devices::{Enumeration::Pnp::*, Properties::*},
        Foundation::*,
        Security::{Authorization::*, Cryptography::*, *},
        Storage::FileSystem::*,
        System::{Pipes::*, Registry::*, Services::*, Threading::*, IO::*},
    },
};
mod bootstrap;
#[cfg(feature = "sony-root-test")]
mod root_test;

const MAX_CONNECTIONS: u32 = 24;
const MAX_DEVICES: usize = 16;
const MAX_PROCESS_DEVICES: usize = 4;
static STOP: Mutex<Option<Arc<Handle>>> = Mutex::new(None);
static STATUS: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(ptr::null_mut());
static NEXT_LEASE: AtomicU64 = AtomicU64::new(1);

pub fn dispatch() -> io::Result<()> {
    #[cfg(feature = "sony-root-test")]
    if std::env::args().skip(1).eq(["--cleanup-root-test"]) {
        return root_test::cleanup(&install_owner()?);
    }
    if std::env::args_os().len() != 1 {
        return Err(fail("Run via Windows Service Control Manager after installation; no elevation or console fallback is performed."));
    }
    let mut name = wide(SERVICE_NAME);
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_mut_ptr(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    check(unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) })
}
unsafe extern "system" fn service_main(_: u32, _: *mut *mut u16) {
    let status = unsafe {
        RegisterServiceCtrlHandlerExW(wide(SERVICE_NAME).as_ptr(), Some(control), ptr::null_mut())
    };
    if status.is_null() {
        return;
    }
    STATUS.store(status, Ordering::Release);
    report(SERVICE_START_PENDING, 0);
    let result = std::panic::catch_unwind(run);
    let code = match result {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => {
            eprintln!("SCX service stopped: {e}");
            e.raw_os_error()
                .unwrap_or(ERROR_SERVICE_SPECIFIC_ERROR as i32) as u32
        }
        Err(_) => ERROR_SERVICE_SPECIFIC_ERROR,
    };
    report(SERVICE_STOPPED, code);
}
unsafe extern "system" fn control(
    code: u32,
    _: u32,
    _: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
) -> u32 {
    match code {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            report(SERVICE_STOP_PENDING, 0);
            if let Ok(slot) = STOP.lock() {
                if let Some(stop) = slot.as_ref() {
                    unsafe {
                        SetEvent(stop.0);
                    }
                }
            }
            0
        }
        SERVICE_CONTROL_INTERROGATE => 0,
        _ => ERROR_CALL_NOT_IMPLEMENTED,
    }
}
fn report(state: u32, error: u32) {
    let h = STATUS.load(Ordering::Acquire);
    if h.is_null() {
        return;
    }
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if state == SERVICE_RUNNING {
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
        } else {
            0
        },
        dwWin32ExitCode: error,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: if state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING {
            1
        } else {
            0
        },
        dwWaitHint: if state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING {
            15000
        } else {
            0
        },
    };
    unsafe {
        SetServiceStatus(h, &status);
    }
}
fn run() -> io::Result<()> {
    let owner = Arc::new(install_owner()?);
    let identity_seed = install_identity_seed()?;
    #[cfg(feature = "sony-root-test")]
    root_test::cleanup(&owner)?;
    bootstrap::initialize()?;
    let stop = Arc::new(Handle::event()?);
    // No generic write right on the client ACE: FILE_GENERIC_WRITE includes the
    // FILE_CREATE_PIPE_INSTANCE bit. Explicit rights allow only client I/O.
    let acl = SecurityDescriptor::new(&format!(
        "D:P(A;;GA;;;SY)(A;;0x12019b;;;{owner})S:(ML;;NW;;;ME)"
    ))?;
    let quota = Arc::new(Mutex::new(Quota::default()));
    let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();
    // Keep a self-connected first instance alive for the service lifetime. This
    // prevents name squatting in the interval between two client connections.
    let _anchor = create_pipe(&acl, true)?;
    let _anchor_client = Handle::new(unsafe {
        CreateFileW(
            wide(PIPE_NAME).as_ptr(),
            0x12019b,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            ptr::null_mut(),
        )
    })?;
    accept(&_anchor, &stop)?;
    *STOP.lock().map_err(|_| fail("stop state poisoned"))? = Some(Arc::clone(&stop));
    report(SERVICE_RUNNING, 0);
    let result = (|| -> io::Result<()> {
        while unsafe { WaitForSingleObject(stop.0, 0) } == WAIT_TIMEOUT {
            // Reap completed handlers, keeping thread accounting bounded.
            let mut i = 0;
            while i < workers.len() {
                if workers[i].is_finished() {
                    let _ = workers.swap_remove(i).join();
                } else {
                    i += 1;
                }
            }
            if workers.len() >= MAX_CONNECTIONS as usize - 2 {
                unsafe {
                    WaitForSingleObject(stop.0, 50);
                }
                continue;
            }
            let pipe = create_pipe(&acl, false)?;
            if let Err(e) = accept(&pipe, &stop) {
                if unsafe { WaitForSingleObject(stop.0, 0) } == WAIT_OBJECT_0 {
                    break;
                }
                return Err(e);
            }
            let owner = Arc::clone(&owner);
            let quota = Arc::clone(&quota);
            let stop = Arc::clone(&stop);
            workers.push(std::thread::spawn(move || {
                let _ = serve(pipe, stop, owner, quota, identity_seed);
            }));
        }
        Ok(())
    })();
    unsafe {
        SetEvent(stop.0);
    }
    for worker in workers {
        let _ = worker.join();
    }
    *STOP.lock().map_err(|_| fail("stop state poisoned"))? = None;
    result
}
struct SecurityDescriptor(*mut std::ffi::c_void);
impl SecurityDescriptor {
    fn new(s: &str) -> io::Result<Self> {
        let mut p = ptr::null_mut();
        check(unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(s).as_ptr(),
                SDDL_REVISION_1,
                &mut p,
                ptr::null_mut(),
            )
        })?;
        Ok(Self(p))
    }
}
impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn create_pipe(acl: &SecurityDescriptor, first: bool) -> io::Result<Handle> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: acl.0,
        bInheritHandle: 0,
    };
    Handle::new(unsafe {
        CreateNamedPipeW(
            wide(PIPE_NAME).as_ptr(),
            PIPE_ACCESS_DUPLEX
                | FILE_FLAG_OVERLAPPED
                | if first {
                    FILE_FLAG_FIRST_PIPE_INSTANCE
                } else {
                    0
                },
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
            MAX_CONNECTIONS,
            MAX_RESPONSE as u32,
            REQUEST_SIZE as u32,
            5000,
            &sa,
        )
    })
}
fn accept(pipe: &Handle, stop: &Handle) -> io::Result<()> {
    let event = Handle::event()?;
    let mut ov = OVERLAPPED {
        hEvent: event.0,
        ..Default::default()
    };
    if unsafe { ConnectNamedPipe(pipe.0, &mut ov) } != 0 {
        return Ok(());
    }
    match unsafe { GetLastError() } {
        ERROR_PIPE_CONNECTED => Ok(()),
        ERROR_IO_PENDING => wait_io(pipe, &mut ov, stop.0, INFINITE).map(|_| ()),
        _ => Err(io::Error::last_os_error()),
    }
}
fn receive(pipe: &Handle, stop: &Handle, timeout: u32) -> io::Result<Request> {
    let mut b = [0u8; REQUEST_SIZE];
    let n = transfer(pipe, &mut b, false, stop.0, timeout)?;
    Request::decode(&b[..n])
}
fn reply(
    pipe: &Handle,
    stop: &Handle,
    op: u16,
    status: u32,
    lease: u64,
    instance: &str,
) -> io::Result<()> {
    reply_capability(
        pipe,
        stop,
        op,
        status,
        lease,
        instance,
        &Capability::default(),
    )
}
fn reply_capability(
    pipe: &Handle,
    stop: &Handle,
    op: u16,
    status: u32,
    lease: u64,
    instance: &str,
    capability: &Capability,
) -> io::Result<()> {
    let mut b = Response {
        op,
        status,
        lease,
        instance: instance.into(),
        capability: Capability::new(*capability.bytes()),
    }
    .encode()?;
    let len = b.len();
    let written = transfer(pipe, &mut b, true, stop.0, 5000);
    wipe(&mut b);
    if written? != len {
        return Err(fail("short response"));
    }
    Ok(())
}
fn serve(
    pipe: Handle,
    stop: Arc<Handle>,
    owner: Arc<String>,
    quota: Arc<Mutex<Quota>>,
    identity_seed: u64,
) -> io::Result<()> {
    let request = receive(&pipe, &stop, 5000)?;
    let identity = authenticate(&pipe, &owner)?;
    if request.op == OP_STATUS {
        reply(&pipe, &stop, request.op, STATUS_OK, 0, "")?;
        return Ok(());
    }
    if request.op != OP_CREATE {
        reply(&pipe, &stop, request.op, STATUS_INVALID, 0, "")?;
        return Ok(());
    }
    let profile = match Profile::try_from(request.profile) {
        Ok(p) if p.is_supported() => p,
        _ => {
            reply(&pipe, &stop, request.op, STATUS_UNSUPPORTED, 0, "")?;
            return Ok(());
        }
    };
    let lease = NEXT_LEASE.fetch_add(1, Ordering::Relaxed);
    let reservation = match Reservation::new(Arc::clone(&quota), identity.pid, lease, profile) {
        Some(r) => r,
        None => {
            reply(&pipe, &stop, request.op, STATUS_QUOTA, 0, "")?;
            return Ok(());
        }
    };
    let device = match Device::create(
        &identity.sid,
        reservation.slot,
        reservation.player_index,
        profile,
        identity_seed,
        &stop,
    ) {
        Ok(d) => d,
        Err(error) => {
            let diagnostic: String = error.to_string().chars().take(MAX_INSTANCE_CHARS).collect();
            reply(&pipe, &stop, request.op, STATUS_DEVICE, 0, &diagnostic)?;
            return Ok(());
        }
    };
    if let Some(bootstrap) = &device.bootstrap {
        reply_capability(
            &pipe,
            &stop,
            OP_CREATE,
            STATUS_OK,
            lease,
            &device.instance,
            &bootstrap.capability,
        )?;
    } else {
        reply(&pipe, &stop, OP_CREATE, STATUS_OK, lease, &device.instance)?;
    }
    // This thread sleeps in overlapped ReadFile. No timer/polling or per-report
    // service work occurs. Broken pipe, cancellation, panic or valid release all
    // drop the software-device handle before releasing its quota.
    let result = receive(&pipe, &stop, INFINITE);
    if let Ok(request) = result {
        if request.op == OP_RELEASE && request.lease == lease {
            drop(device);
            drop(reservation);
            return Ok(());
        }
    }
    drop(device);
    drop(reservation);
    Ok(())
}
#[derive(Default)]
struct Quota {
    leases: Vec<Slot>,
}
#[derive(Clone, Copy)]
struct Slot {
    pid: u32,
    lease: u64,
    slot: usize,
    profile: Profile,
    player_index: u32,
}
impl Quota {
    fn acquire(&mut self, pid: u32, lease: u64, profile: Profile) -> Option<Slot> {
        if self.leases.len() >= MAX_DEVICES
            || self.leases.iter().filter(|s| s.pid == pid).count() >= MAX_PROCESS_DEVICES
            || self.leases.iter().any(|s| s.lease == lease)
        {
            return None;
        }
        let slot = (0..MAX_DEVICES).find(|index| !self.leases.iter().any(|s| s.slot == *index))?;
        let player_index = if profile == Profile::Xbox360 {
            (0..4).find(|index| {
                !self
                    .leases
                    .iter()
                    .any(|s| s.profile == Profile::Xbox360 && s.player_index == *index)
            })?
        } else {
            0
        };
        let reservation = Slot {
            pid,
            lease,
            slot,
            profile,
            player_index,
        };
        self.leases.push(reservation);
        Some(reservation)
    }
    fn release(&mut self, lease: u64) {
        self.leases.retain(|s| s.lease != lease);
    }
}
struct Reservation {
    quota: Arc<Mutex<Quota>>,
    lease: u64,
    slot: usize,
    player_index: u32,
}
impl Reservation {
    fn new(quota: Arc<Mutex<Quota>>, pid: u32, lease: u64, profile: Profile) -> Option<Self> {
        let slot = quota.lock().ok()?.acquire(pid, lease, profile)?;
        Some(Self {
            quota,
            lease,
            slot: slot.slot,
            player_index: slot.player_index,
        })
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut quota) = self.quota.lock() {
            quota.release(self.lease);
        }
    }
}

struct Identity {
    sid: String,
    pid: u32,
    _process: Handle,
}
fn authenticate(pipe: &Handle, owner: &str) -> io::Result<Identity> {
    let mut pid = 0;
    let mut session = 0;
    check(unsafe { GetNamedPipeClientProcessId(pipe.0, &mut pid) })?;
    check(unsafe { GetNamedPipeClientSessionId(pipe.0, &mut session) })?;
    if pid == 0 || session == 0 {
        return Err(fail("interactive caller required"));
    }
    check(unsafe { ImpersonateNamedPipeClient(pipe.0) })?;
    struct Revert;
    impl Drop for Revert {
        fn drop(&mut self) {
            if unsafe { RevertToSelf() } == 0 {
                std::process::abort();
            }
        }
    }
    let _revert = Revert;
    let mut token = ptr::null_mut();
    check(unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) })?;
    let token = Handle::new(token)?;
    let sid = token_sid(&token)?;
    let mut token_session = 0u32;
    let mut needed = 0;
    check(unsafe {
        GetTokenInformation(
            token.0,
            TokenSessionId,
            (&mut token_session as *mut u32).cast(),
            4,
            &mut needed,
        )
    })?;
    if sid != owner || session != token_session {
        return Err(fail("caller is not the configured owner/session"));
    }
    let mut interactive = [0u32; 17];
    let mut sid_size = size_of_val(&interactive) as u32;
    check(unsafe {
        CreateWellKnownSid(
            WinInteractiveSid,
            ptr::null_mut(),
            interactive.as_mut_ptr().cast(),
            &mut sid_size,
        )
    })?;
    let mut member = 0;
    check(unsafe { CheckTokenMembership(token.0, interactive.as_mut_ptr().cast(), &mut member) })?;
    if member == 0 {
        return Err(fail("interactive logon required"));
    }
    let mut integrity = [0u64; 128];
    check(unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            integrity.as_mut_ptr().cast(),
            size_of_val(&integrity) as u32,
            &mut needed,
        )
    })?;
    let label = unsafe { &*integrity.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    if unsafe { IsValidSid(label.Label.Sid) } == 0 {
        return Err(fail("invalid integrity SID"));
    }
    let count = unsafe { *GetSidSubAuthorityCount(label.Label.Sid) };
    if count == 0 || unsafe { *GetSidSubAuthority(label.Label.Sid, (count - 1) as u32) } < 0x2000 {
        return Err(fail("medium-integrity caller required"));
    }
    drop(_revert);
    // Retain the authenticated process object for the lease, preventing PID
    // reuse from changing the identity behind per-process quota accounting.
    let process = Handle::new(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })?;
    let mut process_token = ptr::null_mut();
    check(unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut process_token) })?;
    let process_token = Handle::new(process_token)?;
    if token_sid(&process_token)? != sid {
        return Err(fail("pipe process identity changed"));
    }
    Ok(Identity {
        sid,
        pid,
        _process: process,
    })
}
fn token_sid(token: &Handle) -> io::Result<String> {
    let mut storage = [0u64; 128];
    let mut needed = 0;
    check(unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            storage.as_mut_ptr().cast(),
            size_of_val(&storage) as u32,
            &mut needed,
        )
    })?;
    let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    let mut text = ptr::null_mut();
    check(unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) })?;
    let result = unsafe { read_wide(text, 256) };
    unsafe {
        LocalFree(text.cast());
    }
    result
}
fn install_owner() -> io::Result<String> {
    let mut data = [0u16; 256];
    let mut size = size_of_val(&data) as u32;
    let code = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(r"SOFTWARE\SCX\VirtualPad").as_ptr(),
            wide("InstallOwnerSid").as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    if size < 4 || size as usize > size_of_val(&data) || !size.is_multiple_of(2) {
        return Err(fail("invalid install owner"));
    }
    let sid = unsafe { read_wide(data.as_ptr(), data.len()) }?;
    if !sid.starts_with("S-1-5-21-") && !sid.starts_with("S-1-12-1-") {
        return Err(fail("installation owner must be a user SID"));
    }
    let mut parsed = ptr::null_mut();
    check(unsafe { ConvertStringSidToSidW(wide(&sid).as_ptr(), &mut parsed) })?;
    unsafe {
        LocalFree(parsed);
    }
    Ok(sid)
}
/// A persistent, machine-local identity seed. This is not an authentication
/// secret. Only the installed SYSTEM service writes its admin-protected key.
fn install_identity_seed() -> io::Result<u64> {
    let key = wide(r"SOFTWARE\SCX\VirtualPad");
    let name = wide("DeviceIdentitySeed");
    let mut value = 0u64;
    let mut size = 8;
    let code = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_QWORD | RRF_SUBKEY_WOW6464KEY,
            ptr::null_mut(),
            (&mut value as *mut u64).cast(),
            &mut size,
        )
    };
    if code == ERROR_SUCCESS {
        return if size == 8 && value != 0 {
            Ok(value)
        } else {
            Err(fail("invalid device identity seed"))
        };
    }
    if code != ERROR_FILE_NOT_FOUND {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let mut random = [0u8; 8];
    if unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            random.as_mut_ptr(),
            8,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    } < 0
    {
        return Err(fail("device identity generation failed"));
    }
    value = u64::from_le_bytes(random) | 1;
    let mut handle = ptr::null_mut();
    let code = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            0,
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            &mut handle,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let bytes = value.to_le_bytes();
    let code = unsafe { RegSetValueExW(handle, name.as_ptr(), 0, REG_QWORD, bytes.as_ptr(), 8) };
    unsafe {
        RegCloseKey(handle);
    }
    if code != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    Ok(value)
}
fn device_identity(seed: u64, profile: Profile, slot: usize) -> u64 {
    (seed & !0xfff) | ((profile as u64) << 8) | (slot as u64 + 1)
}
unsafe fn read_wide(p: *const u16, max: usize) -> io::Result<String> {
    if p.is_null() {
        return Err(fail("null UTF-16 pointer"));
    }
    let mut len = 0;
    while len < max && unsafe { *p.add(len) } != 0 {
        len += 1
    }
    if len == max {
        return Err(fail("unterminated UTF-16 string"));
    }
    String::from_utf16(unsafe { std::slice::from_raw_parts(p, len) })
        .map_err(|_| fail("invalid UTF-16 string"))
}
struct Created {
    result: Mutex<Option<io::Result<String>>>,
    complete: Handle,
}
unsafe extern "system" fn created(
    _: HSWDEVICE,
    result: i32,
    context: *const std::ffi::c_void,
    instance: *const u16,
) {
    // Device owns the stable Box. SwDeviceClose guarantees all callbacks have
    // completed before the Box is freed, including a cancelled creation.
    let context = unsafe { &*context.cast::<Created>() };
    let value = if result < 0 {
        Err(fail(&format!(
            "SwDevice creation HRESULT 0x{:08x}",
            result as u32
        )))
    } else {
        unsafe { read_wide(instance, MAX_INSTANCE_CHARS) }
    };
    if let Ok(mut slot) = context.result.lock() {
        *slot = Some(value);
    };
    unsafe {
        SetEvent(context.complete.0);
    }
}
struct Device {
    handle: HSWDEVICE,
    instance: String,
    context: Box<Created>,
    bootstrap: Option<bootstrap::Bootstrap>,
    #[cfg(feature = "sony-root-test")]
    root: Option<root_test::RootDevice>,
}

fn device_container(identity_seed: u64, identity: u64) -> GUID {
    // Group one virtual controller's devnodes across input APIs, separately
    // from other controllers and the PC's system container. The installation
    // seed plus profile/slot stays stable on reconnect; ownership, public Sony
    // identities and authenticated leases are independent of this metadata.
    GUID::from_u128(
        ((identity_seed as u128) << 64) | 0x5343580000000000u128 | (identity as u128 & 0xfff),
    )
}

impl Drop for Device {
    fn drop(&mut self) {
        #[cfg(feature = "sony-root-test")]
        drop(self.root.take());
        if self.handle.is_null() {
            return;
        }
        unsafe {
            SwDeviceClose(self.handle);
        }
    }
}
impl Device {
    fn create(
        owner: &str,
        slot: usize,
        player_index: u32,
        profile: Profile,
        identity_seed: u64,
        stop: &Handle,
    ) -> io::Result<Self> {
        let bootstrap = if profile == Profile::Xbox360 {
            None
        } else {
            Some(bootstrap::Bootstrap::create(owner, profile)?)
        };
        let mut bootstrap_id = bootstrap.as_ref().map(|b| wide(&b.id));
        let acl = SecurityDescriptor::new(&format!("O:SYG:SYD:P(A;;GA;;;SY)(A;;GRGW;;;{owner})"))?;
        // Reuse a bounded set of owned PnP identities. The slot is released only
        // after SwDeviceClose; Windows serializes recreation after old removal.
        let persona = match profile {
            Profile::Xbox360 => "VID_045E&PID_028E&IG_00",
            Profile::DualShock4 => "VID_054C&PID_09CC",
            Profile::DualSense => "VID_054C&PID_0CE6",
            _ => "GENERIC",
        };
        let id = wide(&format!(
            "{persona}&PAD-{owner}-{}-{slot:02}",
            profile as u32
        ));
        // HIDClass derives the game collection's hardware IDs from the first
        // parent ID, not solely from IOCTL_HID_GET_DEVICE_ATTRIBUTES. Omitting
        // the PID makes native game parsers see a generic pad despite correct
        // attributes. The trailing SCX ID remains the driver binding target.
        let hardware = wide(match profile {
            Profile::Xbox360 => "SCX\\VID_045E&PID_028E&IG_00\0SCX\\VirtualXbox\0",
            Profile::DualShock4 => "SCX\\VID_054C&PID_09CC\0SCX\\VirtualPad\0",
            Profile::DualSense => "SCX\\VID_054C&PID_0CE6\0SCX\\VirtualPad\0",
            _ => "SCX\\VirtualPad\0",
        });
        let name = wide(match profile {
            Profile::Xbox360 => "SCX Xbox 360",
            Profile::DualShock4 => "SCX DualShock 4",
            Profile::DualSense => "SCX DualSense",
            _ => "SCX Virtual Gamepad",
        });
        let mut identity = device_identity(identity_seed, profile, slot);
        let container = device_container(identity_seed, identity);
        let info = SW_DEVICE_CREATE_INFO {
            cbSize: size_of::<SW_DEVICE_CREATE_INFO>() as u32,
            pszInstanceId: id.as_ptr(),
            pszzHardwareIds: hardware.as_ptr(),
            pszzCompatibleIds: ptr::null(),
            pContainerId: &container,
            CapabilityFlags: (SWDeviceCapabilitiesDriverRequired
                | SWDeviceCapabilitiesRemovable
                | SWDeviceCapabilitiesSilentInstall) as u32,
            pszDeviceDescription: name.as_ptr(),
            pszDeviceLocation: ptr::null(),
            pSecurityDescriptor: acl.0.cast(),
        };
        let enumerator = wide(device_enumerator(profile));
        let mut profile = profile as u32;
        let mut player_index = player_index;
        let mut owner_sid = wide(owner);
        let key = |pid| DEVPROPCOMPKEY {
            Key: DEVPROPKEY {
                fmtid: GUID::from_u128(0x19f81049_3679_40f2_9689_2a2f61f8bd29),
                pid,
            },
            Store: DEVPROP_STORE_SYSTEM,
            LocaleName: ptr::null(),
        };
        let mut properties = vec![
            DEVPROPERTY {
                CompKey: key(2),
                Type: DEVPROP_TYPE_UINT32,
                BufferSize: 4,
                Buffer: (&mut profile as *mut u32).cast(),
            },
            DEVPROPERTY {
                CompKey: key(3),
                Type: DEVPROP_TYPE_UINT64,
                BufferSize: 8,
                Buffer: (&mut identity as *mut u64).cast(),
            },
            DEVPROPERTY {
                CompKey: key(4),
                Type: DEVPROP_TYPE_UINT32,
                BufferSize: 4,
                Buffer: (&mut player_index as *mut u32).cast(),
            },
            DEVPROPERTY {
                CompKey: key(5),
                Type: DEVPROP_TYPE_STRING,
                BufferSize: (owner_sid.len() * 2) as u32,
                Buffer: owner_sid.as_mut_ptr().cast(),
            },
        ];
        if let Some(id) = &mut bootstrap_id {
            properties.push(DEVPROPERTY {
                CompKey: key(6),
                Type: DEVPROP_TYPE_STRING,
                BufferSize: (id.len() * 2) as u32,
                Buffer: id.as_mut_ptr().cast(),
            });
        }
        let context = Box::new(Created {
            result: Mutex::new(None),
            complete: Handle::event()?,
        });
        #[cfg(feature = "sony-root-test")]
        if matches!(
            Profile::try_from(profile),
            Ok(Profile::DualShock4 | Profile::DualSense)
        ) {
            // Only the explicit ROOT diagnostic uses Windows' system
            // container; it never changes normal SwDeviceCreate grouping.
            let root_container = GUID::from_u128(0x00000000_0000_0000_ffff_ffffffffffff);
            let root = root_test::RootDevice::create(
                Profile::try_from(profile).map_err(|_| fail("invalid profile"))?,
                identity,
                slot,
                owner,
                &hardware,
                &name,
                &root_container,
                &properties,
                stop,
            )?;
            return Ok(Self {
                handle: ptr::null_mut(),
                instance: root.instance.clone(),
                context,
                bootstrap,
                root: Some(root),
            });
        }
        let mut handle = ptr::null_mut();
        let result = unsafe {
            SwDeviceCreate(
                enumerator.as_ptr(),
                wide("HTREE\\ROOT\\0").as_ptr(),
                &info,
                properties.len() as u32,
                properties.as_ptr(),
                Some(created),
                (&*context as *const Created).cast(),
                &mut handle,
            )
        };
        if result < 0 {
            return Err(fail(&format!(
                "SwDeviceCreate HRESULT 0x{:08x}",
                result as u32
            )));
        }
        let mut device = Self {
            handle,
            instance: String::new(),
            context,
            bootstrap,
            #[cfg(feature = "sony-root-test")]
            root: None,
        };
        let events = [device.context.complete.0, stop.0];
        match unsafe { WaitForMultipleObjects(2, events.as_ptr(), 0, 10000) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "device creation timed out",
                ))
            }
            _ => return Err(fail("device creation cancelled")),
        }
        let result = device
            .context
            .result
            .lock()
            .map_err(|_| fail("creation state poisoned"))?
            .take()
            .ok_or_else(|| fail("missing creation result"))?;
        device.instance = result?;
        Ok(device)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotas_are_per_process_and_global() {
        let mut q = Quota::default();
        for lease in 1..=4 {
            assert!(q.acquire(1, lease, Profile::GenericHid).is_some());
        }
        assert!(q.acquire(1, 5, Profile::GenericHid).is_none());
        for lease in 5..=16 {
            assert!(q
                .acquire(lease as u32, lease, Profile::GenericHid)
                .is_some());
        }
        assert!(q.acquire(90, 17, Profile::GenericHid).is_none());
        q.release(2);
        assert!(q.acquire(1, 17, Profile::GenericHid).is_some());
        assert!(q.acquire(99, 17, Profile::GenericHid).is_none());
    }
    #[test]
    fn reservation_released_after_error_path() {
        let quota = Arc::new(Mutex::new(Quota::default()));
        {
            let _r = Reservation::new(Arc::clone(&quota), 1, 1, Profile::GenericHid).unwrap();
            assert_eq!(quota.lock().unwrap().leases.len(), 1);
        }
        assert!(quota.lock().unwrap().leases.is_empty());
    }
    #[test]
    fn stable_slots_and_xinput_player_indices_are_reused_without_collisions() {
        let mut q = Quota::default();
        for i in 0..4 {
            let r = q.acquire(i + 1, i as u64 + 1, Profile::Xbox360).unwrap();
            assert_eq!(r.player_index, i);
        }
        assert!(q.acquire(20, 20, Profile::Xbox360).is_none());
        assert!(q.acquire(20, 20, Profile::GenericHid).is_some());
        q.release(2);
        let r = q.acquire(30, 30, Profile::Xbox360).unwrap();
        assert_eq!(r.slot, 1);
        assert_eq!(r.player_index, 1);
        assert!(q.leases.iter().filter(|s| s.slot == 1).count() == 1);
    }
    #[test]
    fn device_identities_are_stable_and_unique_for_every_profile_and_slot() {
        let mut seen = std::collections::HashSet::new();
        for p in [
            Profile::GenericHid,
            Profile::Xbox360,
            Profile::DualShock4,
            Profile::DualSense,
        ] {
            for s in 0..16 {
                let id = device_identity(0x123456789abcdef0, p, s);
                assert_ne!(id, 0);
                assert_eq!(id, device_identity(0x123456789abcdef0, p, s));
                assert!(seen.insert(id));
            }
        }
    }
    #[test]
    fn containers_are_stable_unique_and_not_system_for_all_profiles_and_slots() {
        let mut seen = std::collections::HashSet::new();
        for seed in [
            0,
            1,
            0x123456789abcdef0,
            0x123456789abcdef1,
            0x9876543210fedcba,
            u64::MAX,
        ] {
            for profile in [
                Profile::GenericHid,
                Profile::Xbox360,
                Profile::DualShock4,
                Profile::DualSense,
            ] {
                for slot in 0..MAX_DEVICES {
                    let container = device_container(seed, device_identity(seed, profile, slot));
                    let key = (
                        container.data1,
                        container.data2,
                        container.data3,
                        container.data4,
                    );
                    assert_eq!(
                        key,
                        {
                            let again =
                                device_container(seed, device_identity(seed, profile, slot));
                            (again.data1, again.data2, again.data3, again.data4)
                        },
                        "container identity must stay stable for each installation/profile/slot"
                    );
                    assert_ne!(key, (0, 0, 0, [0; 8]), "null container is invalid");
                    assert_ne!(key, (0, 0, 0, [0xff; 8]), "not the PC's container");
                    assert!(
                        seen.insert(key),
                        "independent virtual devices must not merge"
                    );
                }
            }
        }
    }
    #[test]
    fn container_change_preserves_existing_public_identity_and_unique_scheme() {
        let seed = 0x123456789abcdef0;
        for (profile, first, last, container_tail) in [
            (
                Profile::GenericHid,
                0x123456789abcd101,
                0x123456789abcd110,
                1,
            ),
            (Profile::Xbox360, 0x123456789abcd201, 0x123456789abcd210, 2),
            (
                Profile::DualShock4,
                0x123456789abcd301,
                0x123456789abcd310,
                3,
            ),
            (
                Profile::DualSense,
                0x123456789abcd401,
                0x123456789abcd410,
                4,
            ),
        ] {
            assert_eq!(device_identity(seed, profile, 0), first);
            assert_eq!(device_identity(seed, profile, MAX_DEVICES - 1), last);
            let container = device_container(seed, first);
            assert_eq!(container.data1, 0x12345678);
            assert_eq!(container.data2, 0x9abc);
            assert_eq!(container.data3, 0xdef0);
            assert_eq!(
                container.data4,
                [0x53, 0x43, 0x58, 0, 0, 0, container_tail, 1]
            );
            // Constructing grouping metadata does not allocate a new public
            // identity or rotate the existing identity on reconnect.
            assert_eq!(device_identity(seed, profile, 0), first);
        }
    }
    #[test]
    fn pipe_acl_allows_client_io_but_denies_creating_another_server_instance() {
        let mut raw = ptr::null_mut();
        check(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) }).unwrap();
        let token = Handle::new(raw).unwrap();
        let sid = token_sid(&token).unwrap();
        // The service itself is SYSTEM; this negative test intentionally uses
        // an ordinary developer account, never changes an account/token.
        if sid == "S-1-5-18" {
            return;
        }
        let acl =
            SecurityDescriptor::new(&format!("D:P(A;;GA;;;SY)(A;;0x12019b;;;{sid})")).unwrap();
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: acl.0,
            bInheritHandle: 0,
        };
        let name = wide(&format!(
            r"\\.\pipe\SCX.VPad.AclTest.{}",
            std::process::id()
        ));
        let server = Handle::new(unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
                2,
                1024,
                1024,
                1000,
                &sa,
            )
        })
        .unwrap();
        let client = Handle::new(unsafe {
            CreateFileW(
                name.as_ptr(),
                0x12019b,
                0,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        })
        .unwrap();
        let other = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
                2,
                1024,
                1024,
                1000,
                &sa,
            )
        };
        if other != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(other);
            }
            panic!("client ACL unexpectedly allows creating a pipe server instance")
        }
        assert_eq!(unsafe { GetLastError() }, ERROR_ACCESS_DENIED);
        drop(client);
        drop(server);
    }
}
