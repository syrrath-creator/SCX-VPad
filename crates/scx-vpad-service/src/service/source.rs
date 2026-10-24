//! Protected, short-lived source leases. No input reports pass through service.
use super::*;
use scx_source_policy::Policy;
use scx_vpad_client::{
    source::{clock_ms, open_verified_source},
    source_wire::*,
};
use std::time::{Duration, Instant};
const ROOT: &str = r"SOFTWARE\SCX\VirtualPad\IsolationLeases";
const SDDL: &str = "O:SYG:SYD:P(A;;KA;;;SY)(A;;KA;;;BA)(A;;KR;;;UD)";
const LEASE_VALUE: [u16; 6] = [
    b'L' as u16,
    b'e' as u16,
    b'a' as u16,
    b's' as u16,
    b'e' as u16,
    0,
];
static SOURCES: Mutex<Vec<(u32, String)>> = Mutex::new(Vec::new());
fn has_capacity(slots: &[(u32, String)], pid: u32, id: &str) -> bool {
    slots.len() < 16
        && slots.iter().filter(|(p, _)| *p == pid).count() < 4
        && !slots.iter().any(|(_, source)| source == id)
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn result(code: u32) -> io::Result<()> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code as i32))
    }
}
fn root() -> io::Result<Key> {
    let sd = SecurityDescriptor::new(SDDL)?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let mut key = ptr::null_mut();
    result(unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            wide(ROOT).as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_VOLATILE,
            KEY_ALL_ACCESS | KEY_WOW64_64KEY,
            &sa,
            &mut key,
            ptr::null_mut(),
        )
    })?;
    let key = Key(key);
    result(unsafe {
        RegSetKeySecurity(
            key.0,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            sd.0,
        )
    })?;
    Ok(key)
}
pub(super) fn initialize() -> io::Result<()> {
    let key = root()?;
    for _ in 0..256 {
        let mut name = [0u16; 65];
        let mut len = 65;
        let code = unsafe {
            RegEnumKeyExW(
                key.0,
                0,
                name.as_mut_ptr(),
                &mut len,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if code == ERROR_NO_MORE_ITEMS {
            return Ok(());
        }
        result(code)?;
        let name = String::from_utf16(&name[..len as usize])
            .map_err(|_| fail("invalid source lease key"))?;
        if name.len() != 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(fail("unexpected source lease key"));
        }
        result(unsafe { RegDeleteTreeW(key.0, wide(&name).as_ptr()) })?;
    }
    Err(fail("too many orphan source leases"))
}
struct Reservation {
    pid: u32,
    id: String,
}
impl Reservation {
    fn new(pid: u32, id: &str) -> io::Result<Self> {
        let mut slots = SOURCES.lock().map_err(|_| fail("source quota poisoned"))?;
        if !has_capacity(&slots, pid, id) {
            return Err(fail("physical source already leased or quota reached"));
        }
        slots.push((pid, id.into()));
        Ok(Self { pid, id: id.into() })
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut slots) = SOURCES.lock() {
            slots.retain(|(p, id)| *p != self.pid || id != &self.id);
        }
    }
}
struct Lease {
    root: Key,
    key: Key,
    name: String,
    capability: Capability,
    number: u64,
}
impl Lease {
    fn create(id: &str, number: u64) -> io::Result<Self> {
        let mut alg = ptr::null_mut();
        if unsafe { BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_SHA256_ALGORITHM, ptr::null(), 0) }
            < 0
        {
            return Err(fail("source hash provider failed"));
        }
        let bytes: Vec<u8> = id.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut digest = [0u8; 32];
        let status = unsafe {
            BCryptHash(
                alg,
                ptr::null(),
                0,
                bytes.as_ptr(),
                bytes.len() as u32,
                digest.as_mut_ptr(),
                32,
            )
        };
        unsafe {
            BCryptCloseAlgorithmProvider(alg, 0);
        }
        if status < 0 {
            return Err(fail("source identity hash failed"));
        }
        let name: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        let root = root()?;
        let sd = SecurityDescriptor::new(SDDL)?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        let mut key = ptr::null_mut();
        result(unsafe {
            RegCreateKeyExW(
                root.0,
                wide(&name).as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_VOLATILE,
                KEY_ALL_ACCESS,
                &sa,
                &mut key,
                ptr::null_mut(),
            )
        })?;
        let key = Key(key);
        result(unsafe {
            RegSetKeySecurity(
                key.0,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                sd.0,
            )
        })?;
        let mut secret = [0; 32];
        if unsafe {
            BCryptGenRandom(
                ptr::null_mut(),
                secret.as_mut_ptr(),
                32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        } < 0
        {
            wipe(&mut secret);
            return Err(fail("source capability generation failed"));
        }
        let capability = Capability::new(secret);
        wipe(&mut secret);
        let lease = Self {
            root,
            key,
            name,
            capability,
            number,
        };
        lease.renew()?;
        Ok(lease)
    }
    fn renew(&self) -> io::Result<()> {
        let mut policy = Policy {
            lease: self.number,
            deadline: clock_ms().saturating_add(3000),
            capability: *self.capability.bytes(),
        };
        let mut frame = policy.encode();
        wipe(&mut policy.capability);
        let code = unsafe {
            RegSetValueExW(
                self.key.0,
                LEASE_VALUE.as_ptr(),
                0,
                REG_BINARY,
                frame.as_ptr(),
                frame.len() as u32,
            )
        };
        wipe(&mut frame);
        result(code)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        unsafe {
            RegDeleteValueW(self.key.0, LEASE_VALUE.as_ptr());
            RegDeleteTreeW(self.root.0, wide(&self.name).as_ptr());
        }
    }
}
pub(super) fn serve_source(pipe: &Handle, stop: &Handle, identity: &Identity) -> io::Result<()> {
    let mut payload = [0u8; SOURCE_FRAME_SIZE];
    let size =
        transfer_with_cancellation(pipe, &mut payload, false, stop.0, identity.process.0, 1500)?;
    let id = decode_instance(&payload[..size])?;
    let _source = match open_verified_source(&id) {
        Ok(h) => h,
        Err(_) => {
            reply(pipe, stop, OP_SOURCE_ACQUIRE, STATUS_UNSUPPORTED, 0, "")?;
            return Ok(());
        }
    };
    let _reservation = match Reservation::new(identity.pid, &id) {
        Ok(r) => r,
        Err(_) => {
            reply(pipe, stop, OP_SOURCE_ACQUIRE, STATUS_QUOTA, 0, "")?;
            return Ok(());
        }
    };
    let number = NEXT_LEASE.fetch_add(1, Ordering::Relaxed);
    let lease = Lease::create(&id, number)?;
    reply_capability(
        pipe,
        stop,
        OP_SOURCE_ACQUIRE,
        STATUS_OK,
        number,
        &id,
        &lease.capability,
    )?;
    let mut last = Instant::now();
    // The lease number and reply fields never change. Keep one bounded frame
    // and completion event for this control-plane connection, not fresh heap
    // buffers/events on every 400 ms renewal. Input reports still bypass us.
    let mut renewal_reply = Response {
        op: OP_SOURCE_RENEW,
        status: STATUS_OK,
        lease: number,
        instance: String::new(),
        capability: Capability::default(),
    }
    .encode()?;
    let mut io = Transfer::new()?;
    loop {
        let mut bytes = [0u8; REQUEST_SIZE];
        let size = match io.run(pipe, &mut bytes, false, stop.0, identity.process.0, 1500) {
            Ok(n) => n,
            Err(_) => break,
        };
        let req = Request::decode(&bytes[..size])?;
        if req.lease != number
            || req.op != OP_SOURCE_RENEW
            || last.elapsed() < Duration::from_millis(150)
        {
            break;
        }
        lease.renew()?;
        last = Instant::now();
        if io.run(
            pipe,
            &mut renewal_reply,
            true,
            stop.0,
            identity.process.0,
            1500,
        )? != renewal_reply.len()
        {
            return Err(fail("short source renewal reply"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::has_capacity;
    #[test]
    fn source_reservations_are_bounded_and_device_exclusive() {
        let mut slots = Vec::new();
        for i in 0..16 {
            let pid = i / 4 + 1;
            let id = format!("source-{i}");
            assert!(has_capacity(&slots, pid, &id));
            slots.push((pid, id));
            assert!(!has_capacity(&slots, pid + 10, &format!("source-{i}")));
            if i % 4 == 3 {
                assert!(!has_capacity(&slots, pid, "extra-source"));
            }
        }
        assert!(!has_capacity(&slots, 99, "extra-source"));
        slots.retain(|(pid, _)| *pid != 2);
        assert!(has_capacity(&slots, 99, "source-4"));
        assert!(!has_capacity(&slots, 1, "source-4"));
        assert!(!has_capacity(&slots, 99, "source-0"));
    }
}
