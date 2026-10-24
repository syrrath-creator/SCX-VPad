//! Fixed-length, little-endian service protocol. No client-controlled paths,
//! descriptors, hardware IDs, registry values or executable names are accepted.
use std::io;

pub const PIPE_NAME: &str = r"\\.\pipe\SCX.VirtualPad.v2";
pub const SERVICE_NAME: &str = "ScxVPad";
pub const VERSION: u16 = 2;
pub const REQUEST_SIZE: usize = 32;
pub const RESPONSE_HEADER_SIZE: usize = 64;
pub const MAX_INSTANCE_CHARS: usize = 256;
pub const MAX_RESPONSE: usize = RESPONSE_HEADER_SIZE + MAX_INSTANCE_CHARS * 2;
pub const OP_STATUS: u16 = 1;
pub const OP_CREATE: u16 = 2;
pub const OP_RELEASE: u16 = 3;
pub const STATUS_OK: u32 = 0;
pub const STATUS_INVALID: u32 = 1;
pub const STATUS_UNSUPPORTED: u32 = 2;
pub const STATUS_QUOTA: u32 = 3;
pub const STATUS_DEVICE: u32 = 4;
pub const GENERIC_HID: u32 = 1;
const MAGIC: &[u8; 4] = b"SCXV";

/// Keep the Sony identity visible in HIDClass's public interface path as well
/// as HID attributes. Other personas retain their existing enumeration policy.
pub fn device_enumerator(profile: scx_vpad_protocol::Profile) -> &'static str {
    use scx_vpad_protocol::Profile;
    match profile {
        Profile::DualShock4 => "SCXVPAD&VID_054C&PID_09CC",
        Profile::DualSense => "SCXVPAD&VID_054C&PID_0CE6",
        _ => "SCXVPAD",
    }
}

/// Accept the legacy service and only the exact new namespace for the requested
/// persona. Never accept a generic SCXVPAD prefix or a foreign device path.
pub fn valid_device_instance(instance: &str, profile: scx_vpad_protocol::Profile) -> bool {
    let mut parts = instance.split('\\');
    let (Some(bus), Some(enumerator), Some(id), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    bus.eq_ignore_ascii_case("SWD")
        && (enumerator.eq_ignore_ascii_case("SCXVPAD")
            || enumerator.eq_ignore_ascii_case(device_enumerator(profile)))
        && !id.is_empty()
        && !id.chars().any(char::is_control)
}

/// A lease capability is never formatted in logs and is erased on drop.
#[derive(Default, PartialEq, Eq)]
pub struct Capability([u8; 32]);
impl Capability {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|&b| b == 0)
    }
}
impl core::fmt::Debug for Capability {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Capability([REDACTED])")
    }
}
impl Drop for Capability {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}
pub fn wipe(bytes: &mut [u8]) {
    for byte in bytes {
        unsafe {
            core::ptr::write_volatile(byte, 0);
        }
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub op: u16,
    pub profile: u32,
    pub lease: u64,
}
impl Request {
    pub fn encode(self) -> [u8; REQUEST_SIZE] {
        let mut b = [0; REQUEST_SIZE];
        b[..4].copy_from_slice(MAGIC);
        b[4..6].copy_from_slice(&VERSION.to_le_bytes());
        b[6..8].copy_from_slice(&self.op.to_le_bytes());
        b[8..12].copy_from_slice(&self.profile.to_le_bytes());
        b[12..20].copy_from_slice(&self.lease.to_le_bytes());
        b
    }
    pub fn decode(b: &[u8]) -> io::Result<Self> {
        if b.len() != REQUEST_SIZE
            || &b[..4] != MAGIC
            || u16_at(b, 4) != VERSION
            || b[20..].iter().any(|&x| x != 0)
        {
            return Err(invalid("invalid service frame"));
        }
        let r = Self {
            op: u16_at(b, 6),
            profile: u32_at(b, 8),
            lease: u64_at(b, 12),
        };
        match r.op {
            OP_STATUS if r.profile == 0 && r.lease == 0 => Ok(r),
            OP_CREATE if r.profile != 0 && r.lease == 0 => Ok(r),
            OP_RELEASE if r.profile == 0 && r.lease != 0 => Ok(r),
            _ => Err(invalid("invalid operation fields")),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Response {
    pub op: u16,
    pub status: u32,
    pub lease: u64,
    pub instance: String,
    pub capability: Capability,
}
impl Response {
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let chars: Vec<u16> = self.instance.encode_utf16().collect();
        if chars.len() > MAX_INSTANCE_CHARS || chars.contains(&0) {
            return Err(invalid("invalid instance ID"));
        }
        let mut b = vec![0; RESPONSE_HEADER_SIZE + chars.len() * 2];
        b[..4].copy_from_slice(MAGIC);
        b[4..6].copy_from_slice(&VERSION.to_le_bytes());
        b[6..8].copy_from_slice(&self.op.to_le_bytes());
        b[8..12].copy_from_slice(&self.status.to_le_bytes());
        b[12..20].copy_from_slice(&self.lease.to_le_bytes());
        b[20..24].copy_from_slice(&((chars.len() * 2) as u32).to_le_bytes());
        b[24..56].copy_from_slice(self.capability.bytes());
        for (pair, ch) in b[RESPONSE_HEADER_SIZE..]
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(chars)
        {
            pair.copy_from_slice(&ch.to_le_bytes());
        }
        Ok(b)
    }
    pub fn decode(b: &[u8]) -> io::Result<Self> {
        if b.len() < RESPONSE_HEADER_SIZE
            || b.len() > MAX_RESPONSE
            || &b[..4] != MAGIC
            || u16_at(b, 4) != VERSION
            || b[56..64].iter().any(|&x| x != 0)
        {
            return Err(invalid("invalid response header"));
        }
        let size = u32_at(b, 20) as usize;
        if !size.is_multiple_of(2) || size != b.len() - RESPONSE_HEADER_SIZE {
            return Err(invalid("invalid response length"));
        }
        let chars: Vec<u16> = b[RESPONSE_HEADER_SIZE..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        if chars.contains(&0) {
            return Err(invalid("embedded null"));
        }
        let instance = String::from_utf16(&chars).map_err(|_| invalid("invalid UTF-16"))?;
        Ok(Self {
            op: u16_at(b, 6),
            status: u32_at(b, 8),
            lease: u64_at(b, 12),
            instance,
            capability: Capability::new(b[24..56].try_into().expect("validated header bounds")),
        })
    }
}
fn u16_at(b: &[u8], n: usize) -> u16 {
    u16::from_le_bytes(b[n..n + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(b[n..n + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], n: usize) -> u64 {
    u64::from_le_bytes(b[n..n + 8].try_into().unwrap())
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_roundtrip_and_reserved_bytes() {
        let r = Request {
            op: OP_CREATE,
            profile: GENERIC_HID,
            lease: 0,
        };
        let mut b = r.encode();
        assert_eq!(Request::decode(&b).unwrap(), r);
        for i in 20..32 {
            b[i] = 1;
            assert!(Request::decode(&b).is_err());
            b[i] = 0;
        }
        for len in 0..32 {
            assert!(Request::decode(&b[..len]).is_err());
        }
    }
    #[test]
    fn rejects_unknown_operations_and_wrong_fields() {
        for r in [
            Request {
                op: 0,
                profile: 0,
                lease: 0,
            },
            Request {
                op: OP_STATUS,
                profile: 1,
                lease: 0,
            },
            Request {
                op: OP_RELEASE,
                profile: 0,
                lease: 0,
            },
            Request {
                op: OP_CREATE,
                profile: 1,
                lease: 1,
            },
        ] {
            assert!(Request::decode(&r.encode()).is_err());
        }
    }
    #[test]
    fn response_bounds_and_utf16() {
        let r = Response {
            op: OP_CREATE,
            status: 0,
            lease: 42,
            instance: r"SWD\SCXVPAD\PAD-42".into(),
            capability: Capability::new([0x42; 32]),
        };
        let b = r.encode().unwrap();
        assert_eq!(Response::decode(&b).unwrap(), r);
        for len in 0..b.len() {
            assert!(Response::decode(&b[..len]).is_err());
        }
        assert!(Response {
            instance: "a".repeat(257),
            ..r
        }
        .encode()
        .is_err());
    }
    #[test]
    fn device_namespaces_are_profile_bound_and_legacy_compatible() {
        use scx_vpad_protocol::Profile;
        for profile in [
            Profile::GenericHid,
            Profile::Xbox360,
            Profile::DualShock4,
            Profile::DualSense,
            Profile::Wheel,
            Profile::FlightStick,
            Profile::ArcadeStick,
        ] {
            assert!(valid_device_instance(r"SWD\SCXVPAD\PAD-42", profile));
            let id = format!("SWD\\{}\\PAD-42", device_enumerator(profile));
            assert!(valid_device_instance(&id, profile));
            assert!(valid_device_instance(&id.to_lowercase(), profile));
            for invalid in [
                r"HID\SCXVPAD\PAD-42",
                r"SWD\SCXVPAD_FOREIGN\42",
                r"SWD\SCXVPAD\",
                r"SWD\SCXVPAD\42\OTHER",
                "SWD\\SCXVPAD\\42\0",
            ] {
                assert!(!valid_device_instance(invalid, profile), "{invalid:?}");
            }
        }
        assert!(!valid_device_instance(
            r"SWD\SCXVPAD&VID_054C&PID_09CC\42",
            Profile::DualSense
        ));
        assert!(!valid_device_instance(
            r"SWD\SCXVPAD&VID_054C&PID_0CE6\42",
            Profile::DualShock4
        ));
        assert!(!valid_device_instance(
            r"SWD\SCXVPAD&VID_054C&PID_0CE6\42",
            Profile::Xbox360
        ));
    }
    #[test]
    fn root_experiment_namespace_is_always_rejected() {
        use scx_vpad_protocol::Profile;
        for profile in [Profile::DualShock4, Profile::DualSense] {
            for slot in 0..4 {
                let id = format!(
                    "ROOT\\{}\\SCX-ROOT-TEST-1234567890ABCDEF-{slot:02}",
                    device_enumerator(profile)
                );
                assert!(!valid_device_instance(&id, profile));
                assert!(!valid_device_instance(&id.to_ascii_lowercase(), profile));
                for other in [
                    Profile::GenericHid,
                    Profile::Xbox360,
                    if profile == Profile::DualSense {
                        Profile::DualShock4
                    } else {
                        Profile::DualSense
                    },
                ] {
                    assert!(!valid_device_instance(&id, other));
                }
            }
            for tail in [
                "SCX-ROOT-TEST-1234567890ABCDEF-04",
                "SCX-ROOT-TEST-1234567890ABCDEZ-00",
                "SCX-ROOT-TEST-1234567890ABCDEF-00\\CHILD",
                "SCX-ROOT-TEST-1234567890ABCDEF-00\0",
                "PAD-42",
                "SCX-ROOT-TEST-1234567890ABCDEF-000",
            ] {
                assert!(!valid_device_instance(
                    &format!("ROOT\\{}\\{tail}", device_enumerator(profile)),
                    profile
                ));
            }
        }
        assert!(!valid_device_instance(
            r"ROOT\HIDCLASS\SCX-ROOT-TEST-1234567890ABCDEF-00",
            Profile::DualSense
        ));
    }
    #[test]
    fn capabilities_are_redacted_and_response_reserved_bytes_rejected() {
        let response = Response {
            op: OP_CREATE,
            status: 0,
            lease: 1,
            instance: "SWD\\SCXVPAD\\TEST".into(),
            capability: Capability::new([0xab; 32]),
        };
        let debug = format!("{response:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("171"));
        let mut bytes = response.encode().unwrap();
        assert_eq!(&bytes[24..56], &[0xab; 32]);
        for i in 56..64 {
            bytes[i] = 1;
            assert!(Response::decode(&bytes).is_err());
            bytes[i] = 0;
        }
        wipe(&mut bytes);
        assert!(bytes.iter().all(|&x| x == 0));
    }
}
