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
    // Syntax only, NOT authorization. The service independently verifies live
    // physical ancestry, the HID collection descriptor and our filter interface.
    if id.len() >= 256 || !id.is_ascii() {
        return false;
    }
    let mut parts = id.split('\\');
    let bus = parts.next().unwrap_or("");
    let model = parts.next().unwrap_or("");
    let instance = parts.next().unwrap_or("");
    bus.eq_ignore_ascii_case("HID")
        && parts.next().is_none()
        && !model.is_empty()
        && !instance.is_empty()
        && model
            .bytes()
            .chain(instance.bytes())
            .all(|b| b.is_ascii_alphanumeric() || b"&{}_-".contains(&b))
        && !model.to_ascii_uppercase().contains("&IG_")
}
/// Strict package-model syntax, distinct from a live device instance identity.
/// USB revisions and collection suffixes must be retained to avoid accidentally
/// matching virtual gamepads or unrelated collections. This is syntax only;
/// callers must also verify the live physical ancestry and HID descriptor.
pub fn package_source_model(model: &str) -> bool {
    if model.len() > 240 || !model.is_ascii() {
        return false;
    }
    let model = model.to_ascii_uppercase();
    let Some(model) = model.strip_prefix("HID\\") else {
        return false;
    };
    let hex = |value: &str, len| value.len() == len && value.bytes().all(|b| b.is_ascii_hexdigit());
    let (suffix, usb) = if let Some(value) = model.strip_prefix("VID_") {
        if value.len() < 13
            || !hex(&value[..4], 4)
            || &value[4..9] != "&PID_"
            || !hex(&value[9..13], 4)
        {
            return false;
        }
        (&value[13..], true)
    } else if let Some(value) = model.strip_prefix("{00001124-0000-1000-8000-00805F9B34FB}_VID&") {
        if value.len() < 17
            || !hex(&value[..8], 8)
            || &value[8..13] != "_PID&"
            || !hex(&value[13..17], 4)
        {
            return false;
        }
        (&value[17..], false)
    } else {
        return false;
    };
    let mut seen = 0u8;
    if !suffix.is_empty() {
        let Some(suffix) = suffix.strip_prefix('&') else {
            return false;
        };
        for part in suffix.split('&') {
            let flag = if part.strip_prefix("REV_").is_some_and(|s| hex(s, 4)) {
                1
            } else if part.strip_prefix("MI_").is_some_and(|s| hex(s, 2)) {
                2
            } else if part.strip_prefix("COL").is_some_and(|s| hex(s, 2)) {
                4
            } else {
                return false;
            };
            if seen & flag != 0 {
                return false;
            }
            seen |= flag;
        }
    }
    !usb || seen & 1 != 0
}
/// Game-controller top-level collections only. Mouse, keyboard, consumer,
/// digitizer and vendor-private interfaces cannot be reserved by this route.
pub fn controller_collection(page: u16, usage: u16) -> bool {
    (page == 1 && matches!(usage, 4 | 5 | 8)) || (page == 2 && (1..=12).contains(&usage))
}
pub fn source_collection(page: u16, usage: u16, vendor: u16, product: u16) -> bool {
    controller_collection(page, usage)
        || (vendor == 0x28de && matches!(product, 0x1302..=0x1305) && page == 0xff00 && usage == 1)
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
    fn package_models_are_exact_and_keep_usb_revision() {
        for model in [
            r"HID\VID_046D&PID_C24F&REV_0111&MI_00&COL01",
            r"hid\vid_0810&pid_0001&rev_0100",
            r"HID\{00001124-0000-1000-8000-00805F9B34FB}_VID&0002054C_PID&0DF2",
        ] {
            assert!(package_source_model(model), "{model}");
        }
        for model in [
            "",
            r"HID\VID_054C&PID_0DF2",
            r"HID\VID_054C&PID_0DF2&REV_0100\SERIAL",
            r"HID\VID_046D&PID_C24F&REV_0111&IG_00",
            r"HID\VID_046D&PID_C24F&REV_0111&COL01&COL02",
            r"HID_DEVICE_UP:0001_U:0005",
            r"HID\*",
            r"HID\VID_046D&PID_C24F&REV_0111&",
            r"USB\VID_046D&PID_C24F&REV_0111",
            r"HID\VID_046D&PID_C24F&REV_0111&MI_0",
            "HID\\VID_046D&PID_C24F&REV_0111\0",
        ] {
            assert!(!package_source_model(model), "{model}");
        }
    }
    #[test]
    fn strict_source_only() {
        for id in [
            "",
            r"SWD\SCXVPAD\x",
            r"HID\VID_054C&PID_0DF2&IG_00\x",
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
    #[test]
    fn physical_model_independent_frames_and_collection_boundary() {
        for id in [
            r"HID\VID_054C&PID_0CE6\7&ABC&0&0000",
            r"HID\VID_046D&PID_C24F&MI_00&COL01\8&ABC&0&0000",
            r"HID\VID_0810&PID_0001\ADAPTER_01",
        ] {
            assert_eq!(decode_instance(&encode_instance(id).unwrap()).unwrap(), id);
        }
        for id in [
            r"HID\X\a\b",
            r"HID\X\..",
            r"USB\X\a",
            r"HID\X\a/b",
            r"HID\X\a\",
            "HID\\X\\a\0",
            "HID\\X\\ü",
        ] {
            assert!(!supported_instance(id));
        }
        assert!(!supported_instance(&format!("HID\\X\\{}", "a".repeat(256))));
        for usage in [4, 5, 8] {
            assert!(controller_collection(1, usage));
        }
        for usage in [1, 2, 6, 7, 0x80] {
            assert!(!controller_collection(1, usage));
        }
        for page in [0, 3, 7, 0xc, 0xd, 0xff00, 0xffff] {
            assert!(!controller_collection(page, 5));
        }
        for usage in 1..=12 {
            assert!(controller_collection(2, usage));
        }
        assert!(!controller_collection(2, 13));
    }
    #[test]
    fn steam_private_collection_is_narrowly_qualified() {
        for product in 0x1302..=0x1305 {
            assert!(source_collection(0xff00, 1, 0x28de, product));
            for usage in [0, 2, 3, 4, 0xffff] {
                assert!(!source_collection(0xff00, usage, 0x28de, product));
            }
            for (page, usage) in [(1, 2), (1, 6), (0xc, 1), (0xd, 5)] {
                assert!(!source_collection(page, usage, 0x28de, product));
            }
        }
        for (vendor, product) in [(0x28de, 0x1301), (0x28de, 0x1306), (0x054c, 0x1304)] {
            assert!(!source_collection(0xff00, 1, vendor, product));
        }
    }
}
