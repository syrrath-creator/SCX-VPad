//! Bounded physical-source request payload, separate from virtual-pad frames.
use std::io;
pub const OP_SOURCE_ACQUIRE: u16 = 6;
pub const OP_SOURCE_RENEW: u16 = 7;
pub const SOURCE_FRAME_SIZE: usize = 520;
pub const SOURCE_AUTH: u32 = 0x22a504;
pub const SOURCE_STATUS: u32 = 0x226500;
pub const SOURCE_PREFIX: &str =
    r"HID\{00001124-0000-1000-8000-00805F9B34FB}_VID&0002054C_PID&0DF2\";
pub fn supported_instance(id: &str) -> bool {
    let upper = id.to_ascii_uppercase();
    upper.starts_with(SOURCE_PREFIX)
        && upper.len() > SOURCE_PREFIX.len()
        && upper.len() < 256
        && upper[SOURCE_PREFIX.len()..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'&')
}
pub fn encode_instance(id: &str) -> io::Result<[u8; SOURCE_FRAME_SIZE]> {
    if !supported_instance(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported physical source",
        ));
    }
    let chars: Vec<u16> = id.to_ascii_uppercase().encode_utf16().collect();
    let mut b = [0; SOURCE_FRAME_SIZE];
    b[..4].copy_from_slice(b"SCXS");
    b[4..6].copy_from_slice(&(chars.len() as u16).to_le_bytes());
    for (i, c) in chars.iter().enumerate() {
        b[8 + 2 * i..10 + 2 * i].copy_from_slice(&c.to_le_bytes());
    }
    Ok(b)
}
pub fn decode_instance(b: &[u8]) -> io::Result<String> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "invalid source frame");
    if b.len() != SOURCE_FRAME_SIZE || &b[..4] != b"SCXS" || b[6..8] != [0; 2] {
        return Err(bad());
    }
    let n = u16::from_le_bytes([b[4], b[5]]) as usize;
    if n == 0 || n >= 256 || b[8 + 2 * n..].iter().any(|v| *v != 0) {
        return Err(bad());
    }
    let chars: Vec<u16> = b[8..8 + 2 * n]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect();
    let id = String::from_utf16(&chars).map_err(|_| bad())?;
    if !supported_instance(&id) {
        return Err(bad());
    }
    Ok(id.to_ascii_uppercase())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_source_only() {
        for id in [
            "",
            r"SWD\SCXVPAD\x",
            r"HID\VID_054C&PID_0DF2\x",
            r"..\Keyboard",
        ] {
            assert!(encode_instance(id).is_err());
        }
        let id = format!("{SOURCE_PREFIX}1&ABC&0&0000");
        let b = encode_instance(&id).unwrap();
        assert_eq!(decode_instance(&b).unwrap(), id);
        for n in 0..SOURCE_FRAME_SIZE {
            assert!(decode_instance(&b[..n]).is_err());
        }
        for n in [0, 6, 7, 519] {
            let mut bad = b;
            bad[n] ^= 1;
            assert!(decode_instance(&bad).is_err());
        }
    }
    #[test]
    fn rejects_embedded_or_trailing_payload() {
        let id = format!("{SOURCE_PREFIX}1&0");
        let mut b = encode_instance(&id).unwrap();
        b[8] = 0;
        assert!(decode_instance(&b).is_err());
        assert!(!supported_instance(&format!("{SOURCE_PREFIX}ABC\\Other")));
    }
}
