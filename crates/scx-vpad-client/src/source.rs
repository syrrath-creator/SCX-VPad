//! Normal-user source isolation. The installed service owns the short lease;
//! authorization attaches only to explicit HID handles, never to all user apps.
use crate::{client, source_wire::*, win::*, wire::*};
use std::{
    io, ptr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use windows_sys::{
    core::GUID,
    Win32::{
        Devices::DeviceAndDriverInstallation::*, Foundation::*, Storage::FileSystem::*,
        System::Threading::*,
    },
};
const HID: GUID = GUID::from_u128(0x4d1e55b2_f16f_11cf_88cb_001111000030);
const FILTER: GUID = GUID::from_u128(0x8cfcc3b9_d6f8_46d2_9baa_d64a3e7f5c51);
const RENEW_INTERVAL_MS: u32 = 400;
const RENEW_TIMEOUT_MS: u64 = 1000;
const CLIENT_TTL_MS: u64 = 1800;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetTickCount64() -> u64;
}
pub fn clock_ms() -> u64 {
    unsafe { GetTickCount64() }
}
struct DeviceSet(HDEVINFO);
impl Drop for DeviceSet {
    fn drop(&mut self) {
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}
fn endpoints(guid: &GUID) -> io::Result<Vec<(String, String)>> {
    let set = DeviceSet(unsafe {
        SetupDiGetClassDevsW(
            guid,
            ptr::null(),
            ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    });
    if set.0 == INVALID_HANDLE_VALUE as isize {
        return Err(io::Error::last_os_error());
    }
    let mut out = Vec::new();
    for i in 0..4096 {
        let mut item = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        if unsafe { SetupDiEnumDeviceInterfaces(set.0, ptr::null(), guid, i, &mut item) } == 0 {
            if unsafe { GetLastError() } == ERROR_NO_MORE_ITEMS {
                return Ok(out);
            }
            return Err(io::Error::last_os_error());
        }
        let mut n = 0;
        unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set.0,
                &item,
                ptr::null_mut(),
                0,
                &mut n,
                ptr::null_mut(),
            );
        }
        if !(8..=65536).contains(&n) {
            return Err(fail("invalid source interface length"));
        }
        let mut storage = vec![0u64; (n as usize).div_ceil(8)];
        let detail = storage
            .as_mut_ptr()
            .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
        unsafe {
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        }
        let mut dev = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        check(unsafe {
            SetupDiGetDeviceInterfaceDetailW(set.0, &item, detail, n, ptr::null_mut(), &mut dev)
        })?;
        let chars = unsafe {
            std::slice::from_raw_parts((*detail).DevicePath.as_ptr(), (n as usize - 4) / 2)
        };
        let end = chars
            .iter()
            .position(|v| *v == 0)
            .ok_or_else(|| fail("unterminated source path"))?;
        let path = String::from_utf16(&chars[..end]).map_err(|_| fail("invalid source path"))?;
        let mut id = [0u16; 256];
        check(unsafe {
            SetupDiGetDeviceInstanceIdW(
                set.0,
                &dev,
                id.as_mut_ptr(),
                id.len() as u32,
                ptr::null_mut(),
            )
        })?;
        let end = id
            .iter()
            .position(|v| *v == 0)
            .ok_or_else(|| fail("unterminated source identity"))?;
        let id = String::from_utf16(&id[..end]).map_err(|_| fail("invalid source identity"))?;
        out.push((path, id.to_ascii_uppercase()));
    }
    Err(fail("too many source interfaces"))
}
fn open(path: &str) -> io::Result<Handle> {
    Handle::new(unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            0xc0000000,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        )
    })
}
/// Resolves only the interface registered by the installed source filter on the
/// exact allowlisted physical gamepad instance. No arbitrary paths are opened.
pub fn open_verified_source(instance: &str) -> io::Result<Handle> {
    if !supported_instance(instance) {
        return Err(fail("source model is not qualified"));
    }
    let (path, _) = endpoints(&FILTER)?
        .into_iter()
        .find(|(_, id)| id.eq_ignore_ascii_case(instance))
        .ok_or_else(|| fail("SCX source filter is not installed for this device"))?;
    verify_source_path(&path)
}
fn verify_source_path(path: &str) -> io::Result<Handle> {
    let h = open(path)?;
    let event = Handle::event()?;
    let mut status = [0u8; 64];
    if ioctl(&h, &event, SOURCE_STATUS, &[], &mut status, 1000)? != 64
        || u64::from_le_bytes(status[..8].try_into().unwrap()) != 2
        || u64::from_le_bytes(status[32..40].try_into().unwrap()) != 1
    {
        return Err(fail("invalid source filter status"));
    }
    Ok(h)
}
#[derive(Clone)]
pub struct SourceAccess {
    capability: Arc<Capability>,
    deadline: Arc<AtomicU64>,
}
impl SourceAccess {
    pub fn check(&self) -> io::Result<()> {
        if clock_ms() < self.deadline.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(fail(
                "Source isolation lease expired; virtual output must stop",
            ))
        }
    }
    /// # Safety
    /// `handle` must be an open overlapped physical HID handle owned by the caller.
    pub unsafe fn authorize_handle(&self, handle: HANDLE) -> io::Result<()> {
        let borrowed = std::mem::ManuallyDrop::new(Handle(handle));
        let event = Handle::event()?;
        for _ in 0..20 {
            self.check()?;
            match ioctl(
                &borrowed,
                &event,
                SOURCE_AUTH,
                self.capability.bytes(),
                &mut [],
                200,
            ) {
                Ok(_) => return Ok(()),
                Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
                    thread::sleep(Duration::from_millis(25))
                }
                Err(e) => return Err(e),
            }
        }
        Err(fail("Physical source did not accept its handle capability"))
    }
}
pub struct SourceGuard {
    access: SourceAccess,
    stop: Arc<Handle>,
    worker: Option<thread::JoinHandle<()>>,
}
impl SourceGuard {
    /// No filter means no change to old installations. A present but broken filter
    /// is an error; never silently fall back to duplicate physical/virtual input.
    pub fn acquire_if_installed(hid_path: &str) -> io::Result<Option<Self>> {
        let Some((_, instance)) = endpoints(&HID)?
            .into_iter()
            .find(|(p, _)| p.eq_ignore_ascii_case(hid_path))
        else {
            return Err(fail("physical HID source disappeared"));
        };
        if !supported_instance(&instance) {
            return Ok(None);
        }
        let Some((filter_path, _)) = endpoints(&FILTER)?
            .into_iter()
            .find(|(_, id)| id == &instance)
        else {
            return Ok(None);
        };
        // This path came from the exact registered filter instance above.
        // Re-enumerating the same device set adds no authentication guarantee.
        drop(verify_source_path(&filter_path)?);
        let pipe = client::connect()?;
        let begin = clock_ms();
        let mut request = Request {
            op: OP_SOURCE_ACQUIRE,
            profile: 0,
            lease: 0,
        }
        .encode();
        let request_len = request.len();
        if transfer(&pipe, &mut request, true, ptr::null_mut(), 1500)? != request_len {
            return Err(fail("short source request"));
        }
        let mut payload = encode_instance(&instance)?;
        if transfer(&pipe, &mut payload, true, ptr::null_mut(), 1500)? != payload.len() {
            return Err(fail("short source identity"));
        }
        let mut bytes = [0; MAX_RESPONSE];
        let received = transfer(&pipe, &mut bytes, false, ptr::null_mut(), 1500);
        let decoded = received.and_then(|n| Response::decode(&bytes[..n]));
        wipe(&mut bytes);
        let response = decoded?;
        if response.op != OP_SOURCE_ACQUIRE
            || response.status != STATUS_OK
            || response.lease == 0
            || response.instance != instance
            || response.capability.is_empty()
        {
            return Err(fail("Service rejected source isolation"));
        }
        let initial_deadline = begin.saturating_add(CLIENT_TTL_MS);
        if clock_ms() >= initial_deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Source isolation acquisition timed out",
            ));
        }
        let access = SourceAccess {
            capability: Arc::new(response.capability),
            deadline: Arc::new(AtomicU64::new(initial_deadline)),
        };
        let stop = Arc::new(Handle::event()?);
        let worker_stop = stop.clone();
        let deadline = access.deadline.clone();
        let lease = response.lease;
        let worker = thread::Builder::new()
            .name("SCX source lease".into())
            .spawn(move || renew_source(pipe, worker_stop, deadline, lease))?;
        Ok(Some(Self {
            access,
            stop,
            worker: Some(worker),
        }))
    }
    pub fn check(&self) -> io::Result<()> {
        self.access.check()
    }
    pub fn access(&self) -> SourceAccess {
        self.access.clone()
    }
}
impl Drop for SourceGuard {
    fn drop(&mut self) {
        // Revoke all cloned SourceAccess objects before waiting for the worker.
        // Its compare/exchange cannot publish a new deadline after this zero.
        self.access.deadline.store(0, Ordering::Release);
        unsafe {
            SetEvent(self.stop.0);
        }
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

fn advance_deadline(deadline: &AtomicU64, sent: u64, now: u64) -> bool {
    // Only this heartbeat thread advances it; Drop may concurrently set zero.
    // A failed compare/exchange therefore means revocation, never a retry.
    let current = deadline.load(Ordering::Acquire);
    now < current
        && deadline
            .compare_exchange(
                current,
                sent.saturating_add(CLIENT_TTL_MS),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
}

fn renewal_exchange(pipe: &Handle, io: &mut Transfer, stop: &Handle, lease: u64) -> io::Result<()> {
    let until = Instant::now() + Duration::from_millis(RENEW_TIMEOUT_MS);
    let remaining = || until.saturating_duration_since(Instant::now()).as_millis() as u32;
    let mut request = Request {
        op: OP_SOURCE_RENEW,
        profile: 0,
        lease,
    }
    .encode();
    if io.run(
        pipe,
        &mut request,
        true,
        stop.0,
        ptr::null_mut(),
        remaining(),
    )? != request.len()
    {
        return Err(fail("short source renewal"));
    }
    let mut bytes = [0; MAX_RESPONSE];
    let received = io.run(
        pipe,
        &mut bytes,
        false,
        stop.0,
        ptr::null_mut(),
        remaining(),
    );
    let decoded = received.and_then(|n| Response::decode(&bytes[..n]));
    wipe(&mut bytes);
    let response = decoded?;
    if response.op != OP_SOURCE_RENEW
        || response.status != STATUS_OK
        || response.lease != lease
        || !response.instance.is_empty()
        || !response.capability.is_empty()
    {
        return Err(fail("invalid source renewal response"));
    }
    Ok(())
}

fn renew_source(pipe: Handle, stop: Arc<Handle>, deadline: Arc<AtomicU64>, lease: u64) {
    struct Revoke(Arc<AtomicU64>);
    impl Drop for Revoke {
        fn drop(&mut self) {
            self.0.store(0, Ordering::Release);
        }
    }
    let _revoke = Revoke(deadline.clone());
    let Ok(mut io) = Transfer::new() else { return };
    while unsafe { WaitForSingleObject(stop.0, RENEW_INTERVAL_MS) } == WAIT_TIMEOUT {
        let sent = clock_ms();
        if sent >= deadline.load(Ordering::Acquire)
            || renewal_exchange(&pipe, &mut io, &stop, lease).is_err()
            || !advance_deadline(&deadline, sent, clock_ms())
        {
            break;
        }
    }
    // Pipe drop revokes the service lease; cancellation drains pending I/O first.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_or_expired_source_access_cannot_be_renewed() {
        let deadline = AtomicU64::new(1800);
        assert!(advance_deadline(&deadline, 400, 500));
        assert_eq!(deadline.load(Ordering::Acquire), 2200);
        assert!(!advance_deadline(&deadline, 2100, 2200));
        deadline.store(0, Ordering::Release);
        assert!(!advance_deadline(&deadline, 600, 700));
        assert_eq!(deadline.load(Ordering::Acquire), 0);
    }

    #[test]
    fn dropping_guard_interrupts_a_stalled_renewal_and_revokes_clones() {
        // Private test pipe only: no driver, registry, hardware or installed service.
        let (server, client) = crate::win::tests::pair();
        let stop = Arc::new(Handle::event().unwrap());
        let deadline = Arc::new(AtomicU64::new(clock_ms() + CLIENT_TTL_MS));
        let access = SourceAccess {
            capability: Arc::new(Capability::new([17; 32])),
            deadline: deadline.clone(),
        };
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || renew_source(client, worker_stop, deadline, 9));
        let guard = SourceGuard {
            access: access.clone(),
            stop,
            worker: Some(worker),
        };
        let mut request = [0; REQUEST_SIZE];
        assert_eq!(
            transfer(&server, &mut request, false, ptr::null_mut(), 1500).unwrap(),
            REQUEST_SIZE
        );
        assert_eq!(Request::decode(&request).unwrap().op, OP_SOURCE_RENEW);
        // Keep the peer open but never reply, reproducing the prior 15-second join.
        let started = Instant::now();
        drop(guard);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "shutdown must cancel the pending reply"
        );
        assert!(access.check().is_err());
        assert_eq!(access.deadline.load(Ordering::Acquire), 0);
        assert!(transfer(&server, &mut [0; 1], false, ptr::null_mut(), 100).is_err());
    }

    #[test]
    fn wrong_lease_response_revokes_source_access() {
        let (server, client) = crate::win::tests::pair();
        let deadline = Arc::new(AtomicU64::new(clock_ms() + CLIENT_TTL_MS));
        let worker_deadline = deadline.clone();
        let worker = thread::spawn(move || {
            renew_source(
                client,
                Arc::new(Handle::event().unwrap()),
                worker_deadline,
                9,
            )
        });
        let mut request = [0; REQUEST_SIZE];
        transfer(&server, &mut request, false, ptr::null_mut(), 1500).unwrap();
        let mut response = Response {
            op: OP_SOURCE_RENEW,
            status: STATUS_OK,
            lease: 10,
            instance: String::new(),
            capability: Capability::default(),
        }
        .encode()
        .unwrap();
        transfer(&server, &mut response, true, ptr::null_mut(), 1000).unwrap();
        worker.join().unwrap();
        assert_eq!(deadline.load(Ordering::Acquire), 0);
    }
}
