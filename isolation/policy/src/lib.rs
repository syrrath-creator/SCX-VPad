#![cfg_attr(not(any(test, feature = "std")), no_std)]
//! Bounded source leases, authenticated per open HID handle. Only the service
//! writes protected policy. No PID trust, allocation, I/O or WDF dependency.
pub const FRAME_SIZE: usize = 64;
pub const MAX_TTL_MS: u64 = 3000;
pub const CAPABILITY_SIZE: usize = 32;
fn game_collection_id(id: &[u16]) -> bool {
    let template = b"HID_DEVICE_UP:0000_U:0000";
    if id.len() != template.len() {
        return false;
    }
    for (i, expected) in template.iter().enumerate() {
        if (14..18).contains(&i) || (21..25).contains(&i) {
            continue;
        }
        if id[i] > 127 || (id[i] as u8).to_ascii_uppercase() != *expected {
            return false;
        }
    }
    fn hex(s: &[u16]) -> Option<u16> {
        s.iter().try_fold(0u16, |value, c| {
            let digit = match *c {
                48..=57 => *c - 48,
                65..=70 => *c - 65 + 10,
                97..=102 => *c - 97 + 10,
                _ => return None,
            };
            Some((value << 4) | digit)
        })
    }
    matches!(
        (hex(&id[14..18]), hex(&id[21..25])),
        (Some(1), Some(4 | 5 | 8)) | (Some(2), Some(1..=12))
    )
}
/// # Safety
/// `id` references `length` readable UTF-16 code units, without terminator.
#[no_mangle]
pub unsafe extern "C" fn scx_source_collection(id: *const u16, length: usize) -> u32 {
    if id.is_null() || length != b"HID_DEVICE_UP:0000_U:0000".len() {
        return 0;
    }
    u32::from(game_collection_id(unsafe {
        core::slice::from_raw_parts(id, length)
    }))
}
fn steam_model(id: &[u16]) -> bool {
    let prefix = b"HID\\VID_28DE&PID_130";
    id.len() > prefix.len()
        && id[..prefix.len()]
            .iter()
            .zip(prefix)
            .all(|(c, b)| *c < 128 && (*c as u8).to_ascii_uppercase() == *b)
        && (b'2' as u16..=b'5' as u16).contains(&id[prefix.len()])
        && (id.len() == prefix.len() + 1 || id[prefix.len() + 1] == b'&' as u16)
}
/// # Safety
/// `id` references `length` readable UTF-16 code units, without terminator.
#[no_mangle]
pub unsafe extern "C" fn scx_source_steam_model(id: *const u16, length: usize) -> u32 {
    if id.is_null() || length > 256 {
        return 0;
    }
    u32::from(steam_model(unsafe {
        core::slice::from_raw_parts(id, length)
    }))
}
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Policy {
    pub lease: u64,
    pub deadline: u64,
    pub capability: [u8; CAPABILITY_SIZE],
}
impl Policy {
    pub fn decode(b: &[u8], now: u64) -> Option<Self> {
        if b.len() != FRAME_SIZE
            || &b[..4] != b"SCXS"
            || b[4..8] != 2u32.to_le_bytes()
            || b[56..64] != [0; 8]
        {
            return None;
        }
        let p = Self {
            lease: u64::from_le_bytes(b[8..16].try_into().ok()?),
            deadline: u64::from_le_bytes(b[16..24].try_into().ok()?),
            capability: b[24..56].try_into().ok()?,
        };
        p.active(now).then_some(p)
    }
    pub fn encode(&self) -> [u8; FRAME_SIZE] {
        let mut b = [0; FRAME_SIZE];
        b[..4].copy_from_slice(b"SCXS");
        b[4..8].copy_from_slice(&2u32.to_le_bytes());
        b[8..16].copy_from_slice(&self.lease.to_le_bytes());
        b[16..24].copy_from_slice(&self.deadline.to_le_bytes());
        b[24..56].copy_from_slice(&self.capability);
        b
    }
    pub fn active(&self, now: u64) -> bool {
        self.lease != 0
            && self.capability != [0; 32]
            && now < self.deadline
            && self.deadline - now <= MAX_TTL_MS
    }
    pub fn authenticates(&self, now: u64, candidate: &[u8; 32]) -> bool {
        // Volatile reads keep comparison work independent of the first mismatch.
        let mut difference = 0u8;
        for (a, b) in self.capability.iter().zip(candidate) {
            difference |= unsafe { core::ptr::read_volatile(a) ^ core::ptr::read_volatile(b) };
        }
        self.active(now) && difference == 0
    }
    pub fn allows(&self, now: u64, candidate: &[u8; 32]) -> bool {
        !self.active(now) || self.authenticates(now, candidate)
    }
}
/// # Safety
/// `bytes` references `length` readable bytes; `out` is writable and disjoint.
#[no_mangle]
pub unsafe extern "C" fn scx_source_decode(
    bytes: *const u8,
    length: usize,
    now: u64,
    out: *mut Policy,
) -> u32 {
    if out.is_null() {
        return 0;
    }
    unsafe {
        out.write(Policy::default());
    }
    if bytes.is_null() || length != FRAME_SIZE {
        return 0;
    }
    if let Some(p) = Policy::decode(unsafe { core::slice::from_raw_parts(bytes, length) }, now) {
        unsafe {
            out.write(p);
        }
        return 1;
    }
    0
}
/// # Safety
/// `policy` is initialized and synchronized; `capability` references 32 bytes.
#[no_mangle]
pub unsafe extern "C" fn scx_source_allowed(
    policy: *const Policy,
    now: u64,
    capability: *const [u8; 32],
    authenticate: u32,
) -> u32 {
    if policy.is_null() || capability.is_null() {
        return 0;
    }
    let (p, c) = unsafe { (&*policy, &*capability) };
    u32::from(if authenticate != 0 {
        p.authenticates(now, c)
    } else {
        p.allows(now, c)
    })
}
#[cfg(not(any(test, feature = "std")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collection_scope_rejects_system_and_vendor_interfaces() {
        let check = |s: &str| game_collection_id(&s.encode_utf16().collect::<Vec<_>>());
        for id in [
            "HID_DEVICE_UP:0001_U:0004",
            "hid_device_up:0001_u:0005",
            "HID_DEVICE_UP:0001_U:0008",
            "HID_DEVICE_UP:0002_U:000C",
        ] {
            assert!(check(id), "{id}");
        }
        for id in [
            "HID_DEVICE_UP:0001_U:0002",
            "HID_DEVICE_UP:0001_U:0006",
            "HID_DEVICE_UP:000C_U:0001",
            "HID_DEVICE_UP:FF00_U:0001",
            "HID_DEVICE_UP:0002_U:000D",
            "HID_DEVICE_UP:0001_U:0005extra",
            "HID_DEVICE_UP:0001_U:0005\0",
            "HID_DEVICE_UP:0001_U:ZZZZ",
        ] {
            assert!(!check(id), "{id}");
        }
    }
    #[test]
    fn vendor_exception_is_exact_steam_family_only() {
        let check = |s: &str| steam_model(&s.encode_utf16().collect::<Vec<_>>());
        assert!(check("HID\\VID_28DE&PID_1304&MI_02&COL03"));
        for id in [
            "HID\\VID_28DE&PID_1304E",
            "HID\\VID_28DE&PID_1306",
            "HID\\VID_1234&PID_1304",
            "HID\\VID_28DE&PID_130",
            "USB\\VID_28DE&PID_1304",
        ] {
            assert!(!check(id), "{id}");
        }
    }
    fn policy() -> Policy {
        Policy {
            lease: 77,
            deadline: 4000,
            capability: [42; 32],
        }
    }
    #[test]
    fn round_trip_abi() {
        assert_eq!(core::mem::size_of::<Policy>(), 48);
        assert!(Policy::decode(&policy().encode(), 1000) == Some(policy()));
    }
    #[test]
    fn only_matching_handle() {
        let p = policy();
        assert!(p.allows(1000, &p.capability));
        for i in 0..32 {
            let mut c = p.capability;
            c[i] ^= 1;
            assert!(!p.allows(1000, &c));
        }
    }
    #[test]
    fn missing_passes_but_cannot_authenticate() {
        let p = Policy::default();
        assert!(p.allows(1000, &[0; 32]));
        assert!(!p.authenticates(1000, &[0; 32]));
    }
    #[test]
    fn inclusive_expiry() {
        let p = policy();
        assert!(!p.allows(3999, &[0; 32]));
        assert!(p.allows(4000, &[0; 32]));
        assert!(!p.authenticates(4000, &p.capability));
        assert!(p.allows(u64::MAX, &[0; 32]));
    }
    #[test]
    fn renewal_and_rotation() {
        let p = policy();
        let mut n = Policy {
            deadline: 5000,
            ..p
        };
        assert!(n.allows(2000, &p.capability));
        n.capability[31] ^= 1;
        assert!(!n.allows(2000, &p.capability));
    }
    #[test]
    fn service_restart_lease_number_reuse() {
        let p = policy();
        let n = Policy {
            capability: [43; 32],
            ..p
        };
        assert!(!n.allows(1000, &p.capability));
    }
    #[test]
    fn malformed_frames() {
        let b = policy().encode();
        for len in 0..64 {
            assert!(Policy::decode(&b[..len], 1000).is_none());
        }
        for n in [0, 4, 7, 56, 63] {
            let mut bad = b;
            bad[n] ^= 255;
            assert!(Policy::decode(&bad, 1000).is_none());
        }
    }
    #[test]
    fn excessive_lifetime() {
        assert!(Policy::decode(&policy().encode(), 999).is_none());
        assert!(Policy::decode(&policy().encode(), 4000).is_none());
        let p = Policy {
            deadline: u64::MAX,
            ..policy()
        };
        assert!(p.allows(1000, &[0; 32]));
    }
    #[test]
    fn empty_authority() {
        for p in [
            Policy {
                lease: 0,
                ..policy()
            },
            Policy {
                capability: [0; 32],
                ..policy()
            },
        ] {
            assert!(Policy::decode(&p.encode(), 1000).is_none());
        }
    }
    #[test]
    fn four_independent_sources() {
        let ps = core::array::from_fn::<_, 4, _>(|i| Policy {
            capability: [42 + i as u8; 32],
            ..policy()
        });
        for (i, p) in ps.iter().enumerate() {
            for (j, q) in ps.iter().enumerate() {
                assert_eq!(p.allows(1000, &q.capability), i == j);
            }
        }
    }
    #[test]
    fn ffi_clears_rejected() {
        let mut p = policy();
        assert_eq!(
            unsafe { scx_source_decode(core::ptr::null(), 0, 1000, &mut p) },
            0
        );
        assert!(p == Policy::default());
    }
}
