#![cfg_attr(not(any(test, feature = "std")), no_std)]
//! Original SCX driver core: fixed-size serialization, bounded queues, and ownership.
//! HID bytes follow the USB HID specification; no third-party driver source is used.
use scx_vpad_protocol::{
    sequence_after, DeviceInfo, InputState, Profile, HID_REPORT_SIZE, INPUT_PACKET_SIZE,
    MAX_DEVICES,
};

pub mod hid_state;
pub mod ownership;
mod profile_ffi;
pub mod profiles;
mod runtime_ffi;
pub mod xusb;

/// Two independently addressable top-level collections. The first is an ordinary
/// HID gamepad, not an XInput device. The vendor collection carries strict feature
/// packets and is never presented as an extra game controller.
pub static REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x05, // Usage (Game Pad)
    0xa1, 0x01, // Collection (Application)
    0x85, 0x01, // Report ID 1
    0x05, 0x09, // Usage Page (Button)
    0x19, 0x01, // Usage Minimum 1
    0x29, 0x20, // Usage Maximum 32
    0x15, 0x00, // Logical Minimum 0
    0x25, 0x01, // Logical Maximum 1
    0x75, 0x01, // Report Size 1
    0x95, 0x20, // Report Count 32
    0x81, 0x02, // Input (Data, Variable, Absolute)
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x39, // Usage (Hat switch)
    0x15, 0x00, // Logical Minimum 0
    0x25, 0x07, // Logical Maximum 7
    0x35, 0x00, // Physical Minimum 0
    0x46, 0x3b, 0x01, // Physical Maximum 315
    0x65, 0x14, // Unit (Degrees)
    0x75, 0x04, 0x95, 0x01, 0x81, 0x42, // Input with Null state (8=neutral)
    0x75, 0x04, 0x95, 0x01, 0x81, 0x03, // Four constant padding bits
    0x65, 0x00, // Unit none
    0x35, 0x00, 0x45, 0x00, // Unspecified physical range
    0x09, 0x30, 0x09, 0x31, 0x09, 0x33, 0x09, 0x34, // X,Y,Rx,Ry
    0x16, 0x00, 0x80, // Logical Minimum -32768
    0x26, 0xff, 0x7f, // Logical Maximum 32767
    0x75, 0x10, 0x95, 0x04, 0x81, 0x02, 0x09, 0x32, 0x09, 0x35, // Z,Rz (independent triggers)
    0x15, 0x00, // Logical Minimum 0
    0x27, 0xff, 0xff, 0x00, 0x00, // Logical Maximum 65535
    0x75, 0x10, 0x95, 0x02, 0x81, 0x02, 0xc0, // End Game Pad collection
    0x06, 0x53, 0xff, // Usage Page vendor 0xff53
    0x09, 0x01, 0xa1, 0x01, // Usage 1, Application Collection
    0x85, 0x7a, // Authenticated input submission; legacy 0x7e is absent
    0x09, 0x02, 0x15, 0x00, 0x26, 0xff, 0x00, 0x75, 0x08, 0x95, 0x60, 0xb1,
    0x02, // capability32 + Input64 = 96 payload bytes
    0x85, 0x7f, // Device information feature report ID
    0x09, 0x03, 0x95, 0x20, 0xb1, 0x02, // 32 payload bytes
    0xc0,
];

/// Report ID + 32 buttons + padded hat nibble + four signed axes + two triggers.
/// Caller must first validate an untrusted InputState (decode does this).
pub fn encode_report(state: &InputState) -> [u8; HID_REPORT_SIZE] {
    let mut r = [0; HID_REPORT_SIZE];
    r[0] = 1;
    r[1..5].copy_from_slice(&state.buttons.to_le_bytes());
    r[5] = state.hat;
    r[6..8].copy_from_slice(&state.left_x.to_le_bytes());
    r[8..10].copy_from_slice(&state.left_y.to_le_bytes());
    r[10..12].copy_from_slice(&state.right_x.to_le_bytes());
    r[12..14].copy_from_slice(&state.right_y.to_le_bytes());
    r[14..16].copy_from_slice(&state.left_trigger.to_le_bytes());
    r[16..18].copy_from_slice(&state.right_trigger.to_le_bytes());
    r
}

pub const STATUS_OK: u32 = 0;
pub const STATUS_INVALID_POINTER: u32 = 1;
pub const STATUS_INVALID_LENGTH: u32 = 2;
pub const STATUS_INVALID_PACKET: u32 = 3;
pub const STATUS_BUFFER_TOO_SMALL: u32 = 4;

/// # Safety
/// `input` must be readable for exactly `length` bytes if non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_validate_input(input: *const u8, length: usize) -> u32 {
    if input.is_null() {
        return STATUS_INVALID_POINTER;
    }
    if length != INPUT_PACKET_SIZE {
        return STATUS_INVALID_LENGTH;
    }
    // SAFETY: checked non-null and bounded length; readable memory is the C caller contract.
    let bytes = unsafe { core::slice::from_raw_parts(input, length) };
    if InputState::decode(bytes).is_ok() {
        STATUS_OK
    } else {
        STATUS_INVALID_PACKET
    }
}

/// # Safety
/// Input is readable for length; neutral points to one writable byte.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_input_is_neutral(
    input: *const u8,
    length: usize,
    neutral: *mut u8,
) -> u32 {
    if input.is_null() || neutral.is_null() {
        return STATUS_INVALID_POINTER;
    }
    if length != INPUT_PACKET_SIZE {
        return STATUS_INVALID_LENGTH;
    }
    // SAFETY: non-null bounded input; caller guarantees readable memory.
    let state = match InputState::decode(unsafe { core::slice::from_raw_parts(input, length) }) {
        Ok(state) => state,
        Err(_) => return STATUS_INVALID_PACKET,
    };
    // SAFETY: caller guarantees one writable byte; input borrow ended before write.
    unsafe {
        core::ptr::write(neutral, state.is_neutral() as u8);
    }
    STATUS_OK
}

/// # Safety
/// `input` must be readable for `length` bytes and `output` writable for
/// `capacity` bytes. Overlap is allowed; no input reference survives the write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_encode_report(
    input: *const u8,
    length: usize,
    output: *mut u8,
    capacity: usize,
) -> u32 {
    if input.is_null() || output.is_null() {
        return STATUS_INVALID_POINTER;
    }
    if length != INPUT_PACKET_SIZE {
        return STATUS_INVALID_LENGTH;
    }
    if capacity < HID_REPORT_SIZE {
        return STATUS_BUFFER_TOO_SMALL;
    }
    // SAFETY: input is non-null and fixed-length; caller guarantees readable memory.
    let state = match InputState::decode(unsafe { core::slice::from_raw_parts(input, length) }) {
        Ok(state) => state,
        Err(_) => return STATUS_INVALID_PACKET,
    };
    let report = encode_report(&state);
    // SAFETY: output has sufficient capacity by contract and does not alias the local report.
    unsafe {
        core::ptr::copy_nonoverlapping(report.as_ptr(), output, report.len());
    }
    STATUS_OK
}

/// # Safety
/// `output` must be writable for `capacity` bytes if non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_neutral_report(output: *mut u8, capacity: usize) -> u32 {
    if output.is_null() {
        return STATUS_INVALID_POINTER;
    }
    if capacity < HID_REPORT_SIZE {
        return STATUS_BUFFER_TOO_SMALL;
    }
    let report = encode_report(&InputState::NEUTRAL);
    // SAFETY: caller supplies enough writable memory; the local report cannot overlap.
    unsafe {
        core::ptr::copy_nonoverlapping(report.as_ptr(), output, report.len());
    }
    STATUS_OK
}

/// # Safety
/// `output` must be writable for `capacity` bytes if non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_get_info(output: *mut u8, capacity: usize) -> u32 {
    if output.is_null() {
        return STATUS_INVALID_POINTER;
    }
    let info = DeviceInfo::CURRENT.encode();
    if capacity < info.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    // SAFETY: sufficient capacity is checked; caller supplies valid writable storage.
    unsafe {
        core::ptr::copy_nonoverlapping(info.as_ptr(), output, info.len());
    }
    STATUS_OK
}

/// # Safety
/// `output_length` must point to writable storage for one usize; unaligned is allowed.
/// Returned memory is immutable and remains valid for the DLL lifetime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_report_descriptor(output_length: *mut usize) -> *const u8 {
    if output_length.is_null() {
        return core::ptr::null();
    }
    // SAFETY: non-null destination validity is guaranteed by the C caller.
    unsafe {
        core::ptr::write_unaligned(output_length, REPORT_DESCRIPTOR.len());
    }
    REPORT_DESCRIPTOR.as_ptr()
}

// A driver build never unwinds into WDF. A violated internal Rust invariant
// terminates its user-mode host immediately rather than continuing with corrupt state.
#[cfg(all(not(test), not(feature = "std"), target_os = "windows"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn RaiseFailFastException(
            record: *const core::ffi::c_void,
            context: *const core::ffi::c_void,
            flags: u32,
        );
    }
    // SAFETY: null record/context requests Windows to construct a fail-fast exception.
    unsafe {
        RaiseFailFastException(core::ptr::null(), core::ptr::null(), 0);
    }
    // Windows never returns. Keep a bounded instruction sequence if a hostile hook does.
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(all(not(test), not(feature = "std"), not(target_os = "windows")))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_layout_matches_descriptor_and_full_ranges() {
        let s = InputState {
            buttons: 0x81828384,
            left_x: i16::MIN,
            left_y: -1,
            right_x: i16::MAX,
            right_y: 42,
            left_trigger: 0xffff,
            right_trigger: 0x1234,
            hat: 7,
            ..InputState::NEUTRAL
        };
        assert_eq!(
            encode_report(&s),
            [
                1, 0x84, 0x83, 0x82, 0x81, 7, 0, 0x80, 0xff, 0xff, 0xff, 0x7f, 42, 0, 0xff, 0xff,
                0x34, 0x12
            ]
        );
    }
    #[test]
    fn neutral_releases_every_button_axis_and_trigger() {
        let r = encode_report(&InputState::NEUTRAL);
        assert_eq!(r[0], 1);
        assert_eq!(r[5], 8);
        for (i, byte) in r.iter().enumerate() {
            if i != 0 && i != 5 {
                assert_eq!(*byte, 0);
            }
        }
    }
    #[test]
    fn ffi_rejects_bad_arguments_without_writing_output() {
        let p = InputState::NEUTRAL.encode();
        let mut output = [0xa5; HID_REPORT_SIZE];
        // SAFETY: every non-null pointer is into a live, adequately sized local array.
        unsafe {
            assert_eq!(
                scx_vpad_validate_input(core::ptr::null(), 64),
                STATUS_INVALID_POINTER
            );
            assert_eq!(
                scx_vpad_validate_input(p.as_ptr(), 63),
                STATUS_INVALID_LENGTH
            );
            assert_eq!(
                scx_vpad_encode_report(p.as_ptr(), 64, output.as_mut_ptr(), 17),
                STATUS_BUFFER_TOO_SMALL
            );
            assert_eq!(output, [0xa5; HID_REPORT_SIZE]);
            let mut bad = p;
            bad[25] = 255;
            assert_eq!(
                scx_vpad_encode_report(bad.as_ptr(), 64, output.as_mut_ptr(), 18),
                STATUS_INVALID_PACKET
            );
            assert_eq!(output, [0xa5; HID_REPORT_SIZE]);
            assert_eq!(
                scx_vpad_encode_report(p.as_ptr(), 64, output.as_mut_ptr(), 18),
                STATUS_OK
            );
        }
        assert_eq!(output, encode_report(&InputState::NEUTRAL));
    }
    #[test]
    fn ffi_handles_overlapping_buffers_and_exact_output_boundaries() {
        let s = InputState {
            buttons: 1,
            left_x: -32,
            ..InputState::NEUTRAL
        };
        let mut p = s.encode();
        // SAFETY: shared buffer is valid; function explicitly permits overlap.
        unsafe {
            assert_eq!(
                scx_vpad_encode_report(p.as_ptr(), 64, p.as_mut_ptr(), 64),
                STATUS_OK
            );
        }
        assert_eq!(&p[..18], &encode_report(&s));
        let mut info = [0xcc; 34];
        // SAFETY: selected interior region has exactly the required 32-byte capacity.
        unsafe {
            assert_eq!(scx_vpad_get_info(info.as_mut_ptr().add(1), 32), STATUS_OK);
        }
        assert_eq!(info[0], 0xcc);
        assert_eq!(info[33], 0xcc);
        assert_eq!(DeviceInfo::decode(&info[1..33]), Ok(DeviceInfo::CURRENT));
    }
    #[test]
    fn descriptor_has_balanced_collections_and_expected_report_sizes() {
        // Independent minimal short-item parser verifies emitted bit counts.
        let mut index = 0;
        let mut depth = 0;
        let mut size = 0;
        let mut count = 0;
        let mut id = 0;
        let mut input = [0u32; 256];
        let mut feature = [0u32; 256];
        while index < REPORT_DESCRIPTOR.len() {
            let tag = REPORT_DESCRIPTOR[index];
            index += 1;
            let n = match tag & 3 {
                3 => 4,
                n => n as usize,
            };
            let mut value = 0u32;
            for j in 0..n {
                value |= (REPORT_DESCRIPTOR[index + j] as u32) << (j * 8);
            }
            index += n;
            match tag & 0xfc {
                0x74 => size = value,
                0x94 => count = value,
                0x84 => id = value as usize,
                0x80 => input[id] += size * count,
                0xb0 => feature[id] += size * count,
                0xa0 => depth += 1,
                0xc0 => {
                    depth -= 1;
                    assert!(depth >= 0);
                }
                _ => (),
            }
        }
        assert_eq!(depth, 0);
        assert_eq!(input[1], 136);
        assert_eq!(feature[0x7a], 768);
        assert_eq!(feature[0x7e], 0); // unauthenticated legacy transport removed
        assert_eq!(feature[0x7f], 256);
        assert_eq!(feature[0x7c], 0); // bring-up diagnostics are not a public HID API
        assert_eq!(input.iter().sum::<u32>(), 136);
    }
}
