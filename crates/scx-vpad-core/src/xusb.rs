//! Original, allocation-free experimental XUSB userspace wire implementation.
//! Factual IOCTL numbers/byte offsets are documented in XUSB-CONTRACT.md.
//! This module does not claim Windows discovery or native DLL compatibility:
//! those require a separate UMDF function device and actual integration tests.
use super::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_LENGTH, STATUS_INVALID_PACKET, STATUS_INVALID_POINTER,
    STATUS_OK,
};
use scx_vpad_protocol::*;

pub const GET_INFORMATION: u32 = 0x80006000;
pub const GET_CAPABILITIES: u32 = 0x8000e004;
pub const GET_LED_STATE: u32 = 0x8000e008;
pub const GET_STATE: u32 = 0x8000e00c;
pub const SET_STATE: u32 = 0x8000a010;
pub const WAIT_GUIDE: u32 = 0x8000e014;
pub const GET_BATTERY: u32 = 0x8000e018;
pub const WAIT_FOR_INPUT: u32 = 0x8000e3ac;
pub const GET_INFORMATION_EX: u32 = 0x8000e3fc;
pub const VERSION: u16 = 0x0103;
const UNSUPPORTED: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XusbError {
    Unsupported,
    Length,
    Version,
    DeviceIndex,
    InvalidInput,
}
pub struct Reply {
    pub bytes: [u8; 64],
    pub length: usize,
}
impl Reply {
    fn new(length: usize) -> Self {
        Self {
            bytes: [0; 64],
            length,
        }
    }
}

/// Canonical SCX axes: positive X right, positive Y down. XInput Y is up.
pub fn gamepad(state: &InputState) -> [u8; 12] {
    let mut b = [0; 12];
    let mut buttons = match state.hat {
        0 => 1,
        1 => 9,
        2 => 8,
        3 => 10,
        4 => 2,
        5 => 6,
        6 => 4,
        7 => 5,
        _ => 0u16,
    };
    for (source, target) in [
        (BUTTON_START, 0x10),
        (BUTTON_SELECT, 0x20),
        (BUTTON_LEFT_STICK, 0x40),
        (BUTTON_RIGHT_STICK, 0x80),
        (BUTTON_LEFT_SHOULDER, 0x100),
        (BUTTON_RIGHT_SHOULDER, 0x200),
        (BUTTON_GUIDE, 0x400),
        (BUTTON_SOUTH, 0x1000),
        (BUTTON_EAST, 0x2000),
        (BUTTON_WEST, 0x4000),
        (BUTTON_NORTH, 0x8000),
    ] {
        if state.buttons & source != 0 {
            buttons |= target;
        }
    }
    word(&mut b, 0, buttons);
    b[2] = (u32::from(state.left_trigger) * 255 / 65535) as u8;
    b[3] = (u32::from(state.right_trigger) * 255 / 65535) as u8;
    word(&mut b, 4, state.left_x as u16);
    word(&mut b, 6, state.left_y.saturating_neg() as u16);
    word(&mut b, 8, state.right_x as u16);
    word(&mut b, 10, state.right_y.saturating_neg() as u16);
    b
}

pub fn state_report(state: &InputState, packet_number: u32, resumed: bool) -> [u8; 29] {
    let mut b = [0; 29];
    word(&mut b, 0, VERSION);
    b[2] = if resumed { 3 } else { 1 };
    b[5..9].copy_from_slice(&packet_number.to_le_bytes());
    if resumed {
        b[10] = 0x14;
    }
    b[11..23].copy_from_slice(&gamepad(state));
    b
}

fn request_version(request: &[u8], expected_length: usize) -> Result<u16, XusbError> {
    if request.len() != expected_length {
        return Err(XusbError::Length);
    }
    let version = u16::from_le_bytes([request[0], request[1]]);
    if !(0x0101..=0x0104).contains(&version) {
        return Err(XusbError::Version);
    }
    if request[2] != 0 {
        return Err(XusbError::DeviceIndex);
    }
    Ok(version)
}

pub fn query(
    code: u32,
    request: &[u8],
    state: Option<&InputState>,
    packet_number: u32,
    player: u8,
    led: u8,
) -> Result<Reply, XusbError> {
    if player > 3 {
        return Err(XusbError::DeviceIndex);
    }
    match code {
        GET_INFORMATION => {
            if !request.is_empty() {
                return Err(XusbError::Length);
            }
            let mut r = Reply::new(12);
            word(&mut r.bytes, 0, VERSION);
            r.bytes[2] = 1;
            word(&mut r.bytes, 8, 0x045e);
            word(&mut r.bytes, 10, 0x028e);
            Ok(r)
        }
        GET_INFORMATION_EX => {
            if !request.is_empty() {
                return Err(XusbError::Length);
            }
            let mut r = Reply::new(64);
            word(&mut r.bytes, 0, VERSION);
            r.bytes[2] = 1;
            r.bytes[3] = 1;
            word(&mut r.bytes, 8, 0x045e);
            word(&mut r.bytes, 10, 0x028e);
            Ok(r)
        }
        GET_STATE => {
            let Some(state) = state else {
                return Err(XusbError::InvalidInput);
            };
            if state.validate().is_err() {
                return Err(XusbError::InvalidInput);
            }
            if request == [0] {
                let mut r = Reply::new(20);
                r.bytes[0] = 1;
                r.bytes[3..7].copy_from_slice(&packet_number.to_le_bytes());
                r.bytes[8..20].copy_from_slice(&gamepad(state));
                return Ok(r);
            }
            request_version(request, 3)?;
            let mut r = Reply::new(29);
            r.bytes[..29].copy_from_slice(&state_report(state, packet_number, false));
            Ok(r)
        }
        GET_CAPABILITIES => {
            let version = request_version(request, 3)?;
            let extended = version >= 0x0102;
            let mut r = Reply::new(if extended { 36 } else { 24 });
            word(&mut r.bytes, 0, version);
            r.bytes[2] = 1;
            r.bytes[3] = 1;
            let controls = if extended {
                word(&mut r.bytes, 4, 1); // force feedback, no voice/wireless claim
                word(&mut r.bytes, 6, 0x045e);
                word(&mut r.bytes, 8, 0x028e);
                word(&mut r.bytes, 10, 0x0100);
                r.bytes[12] = player;
                16
            } else {
                4
            };
            word(&mut r.bytes, controls, 0xf7ff);
            r.bytes[controls + 2] = 255;
            r.bytes[controls + 3] = 255;
            for offset in [controls + 4, controls + 6, controls + 8, controls + 10] {
                word(&mut r.bytes, offset, 0xffff);
            }
            // Published packed XUSB capability formats end with 8-bit motor maxima.
            r.bytes[r.length - 2] = 255;
            r.bytes[r.length - 1] = 255;
            Ok(r)
        }
        GET_LED_STATE => {
            request_version(request, 3)?;
            let mut r = Reply::new(3);
            word(&mut r.bytes, 0, VERSION);
            r.bytes[2] = if led == 0xff { 6 + player } else { led };
            Ok(r)
        }
        GET_BATTERY => {
            request_version(request, 4)?;
            // 0=gamepad,1=headset. No fabricated headset is advertised.
            if request[3] > 1 {
                return Err(XusbError::InvalidInput);
            }
            let mut r = Reply::new(4);
            word(&mut r.bytes, 0, VERSION);
            if request[3] == 0 {
                r.bytes[2] = 1;
                r.bytes[3] = 3;
            }
            Ok(r)
        }
        // Unknown/private extensions, audio and power commands are deliberately
        // not guessed. Native code must pend supported asynchronous requests.
        _ => Err(XusbError::Unsupported),
    }
}

pub fn decode_output(request: &[u8]) -> Result<Feedback, XusbError> {
    // WGI sends nine bytes, with the same five-byte motor/LED prefix as
    // XInputSetState. Its four-byte extension is not an advertised SCX feature.
    // Accept the observed bounded envelope; decode only the supported prefix.
    if request.len() != 5 && request.len() != 9 {
        return Err(XusbError::Length);
    }
    if request[0] != 0 {
        return Err(XusbError::DeviceIndex);
    }
    if request[4] & !3 != 0 {
        return Err(XusbError::InvalidInput);
    }
    let mut feedback = Feedback::empty(Profile::Xbox360);
    if request[4] & 1 != 0 {
        feedback.valid |= FEEDBACK_PLAYER;
        feedback.player = request[1];
    }
    if request[4] & 2 != 0 {
        feedback.valid |= FEEDBACK_RUMBLE;
        feedback.left_motor = u16::from(request[2]) * 257;
        feedback.right_motor = u16::from(request[3]) * 257;
    }
    Ok(feedback)
}
fn word(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn status(error: XusbError) -> u32 {
    match error {
        XusbError::Unsupported => UNSUPPORTED,
        XusbError::Length => STATUS_INVALID_LENGTH,
        _ => STATUS_INVALID_PACKET,
    }
}

/// # Safety
/// Pointers name readable/writable caller-owned regions of their given sizes.
/// Null request is allowed only with length0. Input64 required only for GET_STATE.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_xusb_query(
    code: u32,
    request: *const u8,
    request_length: usize,
    input: *const u8,
    input_length: usize,
    packet_number: u32,
    player: u8,
    led: u8,
    output: *mut u8,
    capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if output.is_null() || output_length.is_null() || (request.is_null() && request_length != 0) {
        return STATUS_INVALID_POINTER;
    }
    // SAFETY: caller owns valid writable output length slot.
    unsafe {
        core::ptr::write_unaligned(output_length, 0);
    }
    if request_length > 64 {
        return STATUS_INVALID_LENGTH;
    }
    let request = if request_length == 0 {
        &[]
    } else {
        // SAFETY: non-null bounded request and caller guarantees readable memory.
        unsafe { core::slice::from_raw_parts(request, request_length) }
    };
    let state = if code == GET_STATE {
        if input.is_null() {
            return STATUS_INVALID_POINTER;
        }
        if input_length != INPUT_PACKET_SIZE {
            return STATUS_INVALID_LENGTH;
        }
        // SAFETY: fixed-size non-null input; valid memory is a caller obligation.
        match InputState::decode(unsafe { core::slice::from_raw_parts(input, input_length) }) {
            Ok(s) => Some(s),
            Err(_) => return STATUS_INVALID_PACKET,
        }
    } else {
        None
    };
    let reply = match query(code, request, state.as_ref(), packet_number, player, led) {
        Ok(r) => r,
        Err(e) => return status(e),
    };
    if capacity < reply.length {
        return STATUS_BUFFER_TOO_SMALL;
    }
    // SAFETY: sufficient capacity, local source cannot overlap output; length pointer valid.
    unsafe {
        core::ptr::copy_nonoverlapping(reply.bytes.as_ptr(), output, reply.length);
        core::ptr::write_unaligned(output_length, reply.length);
    }
    STATUS_OK
}

/// # Safety
/// Request readable for length, feedback writable for capacity. Overlap allowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_xusb_decode_output(
    request: *const u8,
    length: usize,
    feedback: *mut u8,
    capacity: usize,
) -> u32 {
    if request.is_null() || feedback.is_null() {
        return STATUS_INVALID_POINTER;
    }
    if length != 5 && length != 9 {
        return STATUS_INVALID_LENGTH;
    }
    if capacity < FEEDBACK_PACKET_SIZE {
        return STATUS_BUFFER_TOO_SMALL;
    }
    // SAFETY: fixed readable request; output is written after decoding is complete.
    let reply = match decode_output(unsafe { core::slice::from_raw_parts(request, length) }) {
        Ok(f) => f.encode(),
        Err(e) => return status(e),
    };
    // SAFETY: output capacity checked and local reply cannot overlap destination.
    unsafe {
        core::ptr::copy_nonoverlapping(reply.as_ptr(), feedback, reply.len());
    }
    STATUS_OK
}

/// # Safety
/// Input readable for length and output writable for capacity. Overlap allowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_xusb_state_report(
    input: *const u8,
    length: usize,
    packet_number: u32,
    resumed: u8,
    output: *mut u8,
    capacity: usize,
) -> u32 {
    if input.is_null() || output.is_null() {
        return STATUS_INVALID_POINTER;
    }
    if length != INPUT_PACKET_SIZE {
        return STATUS_INVALID_LENGTH;
    }
    if capacity < 29 {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if resumed > 1 {
        return STATUS_INVALID_PACKET;
    }
    // SAFETY: fixed readable packet; caller owns the memory.
    let state = match InputState::decode(unsafe { core::slice::from_raw_parts(input, length) }) {
        Ok(s) => s,
        Err(_) => return STATUS_INVALID_PACKET,
    };
    let reply = state_report(&state, packet_number, resumed != 0);
    // SAFETY: output capacity checked; local reply independent of output.
    unsafe {
        core::ptr::copy_nonoverlapping(reply.as_ptr(), output, reply.len());
    }
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extended_discovery_identifies_one_wired_xbox_and_is_bounded() {
        let reply = query(GET_INFORMATION_EX, &[], None, 0, 0, 6).unwrap();
        assert_eq!(reply.length, 64);
        assert_eq!(&reply.bytes[..4], &[3, 1, 1, 1]);
        assert_eq!(&reply.bytes[8..12], &[0x5e, 4, 0x8e, 2]);
        assert_eq!(&reply.bytes[12..], &[0; 52]);
        assert!(query(GET_INFORMATION_EX, &[0], None, 0, 0, 6).is_err());
    }
    #[test]
    fn ffi_query_rejects_bad_bounds_without_writing_and_supports_overlap() {
        let mut packet = InputState::NEUTRAL.encode();
        let request = [3, 1, 0];
        let mut written = usize::MAX;
        let mut destination = [0xcc; 31];
        // SAFETY: invalid logical lengths are tested only where validated before
        // dereferencing; every actually read/written region is local storage.
        unsafe {
            assert_eq!(
                scx_vpad_xusb_query(
                    GET_STATE,
                    request.as_ptr(),
                    3,
                    packet.as_ptr(),
                    64,
                    7,
                    0,
                    6,
                    destination.as_mut_ptr().add(1),
                    28,
                    &mut written
                ),
                STATUS_BUFFER_TOO_SMALL
            );
            assert_eq!(written, 0);
            assert_eq!(destination, [0xcc; 31]);
            assert_eq!(
                scx_vpad_xusb_query(
                    GET_STATE,
                    request.as_ptr(),
                    65,
                    packet.as_ptr(),
                    64,
                    7,
                    0,
                    6,
                    destination.as_mut_ptr(),
                    31,
                    &mut written
                ),
                STATUS_INVALID_LENGTH
            );
            assert_eq!(destination, [0xcc; 31]);
            assert_eq!(
                scx_vpad_xusb_query(
                    GET_STATE,
                    request.as_ptr(),
                    3,
                    packet.as_ptr(),
                    64,
                    7,
                    0,
                    6,
                    destination.as_mut_ptr().add(1),
                    29,
                    &mut written
                ),
                STATUS_OK
            );
            assert_eq!(written, 29);
            assert_eq!(destination[0], 0xcc);
            assert_eq!(destination[30], 0xcc);
            assert_eq!(
                scx_vpad_xusb_state_report(packet.as_ptr(), 64, 7, 0, packet.as_mut_ptr(), 64),
                STATUS_OK
            );
            assert_eq!(&packet[..29], &destination[1..30]);
        }
    }
    #[test]
    fn state_offsets_signs_and_packet_counter_match_wire() {
        let s = InputState {
            buttons: BUTTON_SOUTH | BUTTON_GUIDE,
            hat: 1,
            left_trigger: 65535,
            left_x: i16::MIN,
            left_y: i16::MIN,
            right_y: 1234,
            ..InputState::NEUTRAL
        };
        let r = state_report(&s, 0x12345678, false);
        assert_eq!(&r[5..9], &[0x78, 0x56, 0x34, 0x12]);
        assert_eq!(u16::from_le_bytes([r[11], r[12]]), 0x1409);
        assert_eq!(r[13], 255);
        assert_eq!(i16::from_le_bytes([r[15], r[16]]), i16::MIN);
        assert_eq!(i16::from_le_bytes([r[17], r[18]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([r[21], r[22]]), -1234);
        let wait = state_report(&s, 7, true);
        assert_eq!(wait[2], 3);
        assert_eq!(wait[9], 0);
        assert_eq!(wait[10], 0x14);
    }
    #[test]
    fn capability_versions_use_distinct_offsets_and_real_motor_fields() {
        let v1 = query(GET_CAPABILITIES, &[1, 1, 0], None, 0, 0, 255).unwrap();
        let v2 = query(GET_CAPABILITIES, &[2, 1, 0], None, 0, 0, 255).unwrap();
        assert_eq!(v1.length, 24);
        assert_eq!(v2.length, 36);
        assert_eq!(&v1.bytes[4..6], &[0xff, 0xf7]);
        assert_eq!(&v2.bytes[16..18], &[0xff, 0xf7]);
        assert_eq!(&v1.bytes[22..24], &[255, 255]);
        assert_eq!(&v2.bytes[34..36], &[255, 255]);
    }
    #[test]
    fn requests_are_bounded_versioned_and_scoped_to_local_device_zero() {
        for r in [&[][..], &[1][..], &[1, 1][..], &[1, 1, 0, 0][..]] {
            assert!(matches!(
                query(GET_STATE, r, Some(&InputState::NEUTRAL), 0, 0, 0),
                Err(XusbError::Length)
            ));
        }
        assert!(matches!(
            query(GET_STATE, &[1, 1, 1], Some(&InputState::NEUTRAL), 0, 0, 0),
            Err(XusbError::DeviceIndex)
        ));
        assert!(matches!(
            query(GET_STATE, &[0, 2, 0], Some(&InputState::NEUTRAL), 0, 0, 0),
            Err(XusbError::Version)
        ));
        assert!(matches!(
            query(WAIT_GUIDE, &[], None, 0, 0, 0),
            Err(XusbError::Unsupported)
        ));
    }
    #[test]
    fn setstate_rumble_and_led_flags_are_independent() {
        let rumble = decode_output(&[0, 9, 255, 32, 2]).unwrap();
        assert_eq!(rumble.valid, FEEDBACK_RUMBLE);
        assert_eq!(rumble.left_motor, 65535);
        assert_eq!(rumble.right_motor, 32 * 257);
        let led = decode_output(&[0, 7, 255, 255, 1]).unwrap();
        assert_eq!(led.valid, FEEDBACK_PLAYER);
        assert_eq!(led.left_motor, 0);
        assert_eq!(led.player, 7);
        assert_eq!(decode_output(&[1, 0, 0, 0, 2]), Err(XusbError::DeviceIndex));
        assert_eq!(
            decode_output(&[0, 0, 0, 0, 128]),
            Err(XusbError::InvalidInput)
        );
    }
    #[test]
    fn wgi_extended_output_keeps_standard_motors_and_ignores_extension() {
        for tail in [[1, 1, 0, 0], [255; 4]] {
            let mut packet = [0, 0, 63, 127, 2, 0, 0, 0, 0];
            packet[5..].copy_from_slice(&tail);
            let f = decode_output(&packet).unwrap();
            assert_eq!(f.valid, FEEDBACK_RUMBLE);
            assert_eq!((f.left_motor, f.right_motor), (63 * 257, 127 * 257));
            packet[2..4].fill(0);
            let stop = decode_output(&packet).unwrap();
            assert_eq!((stop.left_motor, stop.right_motor), (0, 0));
            packet[0] = 1;
            assert_eq!(decode_output(&packet), Err(XusbError::DeviceIndex));
        }
        for length in [0, 4, 6, 7, 8, 10] {
            assert_eq!(decode_output(&[0; 10][..length]), Err(XusbError::Length));
        }
    }
    #[test]
    fn battery_uses_two_byte_header_and_legacy_state_is_twenty_bytes() {
        let b = query(GET_BATTERY, &[2, 1, 0, 0], None, 0, 0, 255).unwrap();
        assert_eq!(&b.bytes[2..4], &[1, 3]);
        let legacy = query(GET_STATE, &[0], Some(&InputState::NEUTRAL), 42, 0, 255).unwrap();
        assert_eq!(legacy.length, 20);
        assert_eq!(&legacy.bytes[3..7], &42u32.to_le_bytes());
    }

    #[test]
    fn cached_controls_match_complete_report_changes_and_watchdog_neutral() {
        // The WDF binding caches only bytes 11..23. Establish independently
        // that no other byte of a packet-number-zero state report changes
        // with accepted Input64, including unsupported controls and sensors.
        let mut previous = state_report(&InputState::NEUTRAL, 0, false);
        let mut cached = [0; 12];
        for i in 0..20_000u32 {
            let state = InputState {
                sequence: i,
                buttons: i.wrapping_mul(0x9e37_79b9),
                hat: (i % 9) as u8,
                left_x: i.wrapping_mul(1297) as i16,
                left_y: i.wrapping_mul(2531) as i16,
                right_x: i.wrapping_mul(541) as i16,
                right_y: i.wrapping_mul(711) as i16,
                left_trigger: i.wrapping_mul(257) as u16,
                right_trigger: i.wrapping_mul(701) as u16,
                flags: FLAG_GYRO_VALID | FLAG_ACCEL_VALID,
                gyro: [i as i16, i.wrapping_neg() as i16, i as i16],
                accel: [1, 2, 3],
                timestamp_us: u64::from(i) * 997,
                ..InputState::NEUTRAL
            };
            let encoded = state.encode();
            let mut next = [0xcc; 29];
            // SAFETY: disjoint local arrays have the exact declared sizes.
            assert_eq!(
                unsafe {
                    scx_vpad_xusb_state_report(
                        encoded.as_ptr(),
                        encoded.len(),
                        0,
                        0,
                        next.as_mut_ptr(),
                        next.len(),
                    )
                },
                STATUS_OK
            );
            assert_eq!(previous != next, cached != next[11..23]);
            assert_eq!(&next[..11], &previous[..11]);
            assert_eq!(&next[23..], &previous[23..]);
            assert_eq!(next[11..23] == [0; 12], gamepad(&state) == [0; 12]);
            cached.copy_from_slice(&next[11..23]);
            previous = next;
            if i % 37 == 0 {
                // A timeout releases controls and resets the corresponding
                // cache, retaining the separate replay sequence in the driver.
                previous = state_report(&InputState::NEUTRAL, 0, false);
                cached.fill(0);
                assert_ne!(cached, next[11..23]);
            }
        }
        let baseline = state_report(&InputState::NEUTRAL, 0, false);
        let unused = InputState {
            buttons: BUTTON_TOUCHPAD
                | BUTTON_MICROPHONE
                | BUTTON_LEFT_TRIGGER
                | BUTTON_RIGHT_TRIGGER,
            gyro: [123, 456, 789],
            flags: FLAG_GYRO_VALID,
            sequence: 999,
            timestamp_us: 88_888,
            ..InputState::NEUTRAL
        };
        assert_eq!(state_report(&unused, 0, false), baseline);
        // Quantized trigger movement below one XInput unit stays neutral.
        for value in [0, 1, 128, 256] {
            assert_eq!(
                gamepad(&InputState {
                    left_trigger: value,
                    ..unused
                }),
                [0; 12]
            );
        }
        assert_ne!(
            gamepad(&InputState {
                left_trigger: 257,
                ..unused
            }),
            [0; 12]
        );
    }

    #[test]
    fn single_pass_state_encoder_rejects_invalid_input_without_output_mutation() {
        let valid = InputState::NEUTRAL.encode();
        let mut output = [0xcc; 31];
        for length in 0..=128 {
            if length == INPUT_PACKET_SIZE {
                continue;
            }
            // SAFETY: unsupported lengths are rejected before dereferencing.
            assert_eq!(
                unsafe {
                    scx_vpad_xusb_state_report(
                        valid.as_ptr(),
                        length,
                        0,
                        0,
                        output.as_mut_ptr().add(1),
                        29,
                    )
                },
                STATUS_INVALID_LENGTH
            );
            assert_eq!(output, [0xcc; 31]);
        }
        for (offset, invalid) in [
            (0, 2),
            (2, 63),
            (24, 9),
            (25, 8),
            (26, 1),
            (45, 2),
            (51, 2),
            (60, 1),
        ] {
            let mut packet = valid;
            packet[offset] = invalid;
            // SAFETY: local input/output buffers are disjoint and sized.
            assert_eq!(
                unsafe {
                    scx_vpad_xusb_state_report(
                        packet.as_ptr(),
                        packet.len(),
                        0,
                        0,
                        output.as_mut_ptr().add(1),
                        29,
                    )
                },
                STATUS_INVALID_PACKET
            );
            assert_eq!(output, [0xcc; 31]);
        }
    }
}
