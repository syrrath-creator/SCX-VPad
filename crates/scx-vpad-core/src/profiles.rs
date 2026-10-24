//! Original USB-profile encoders with MIT-attributed Sony compatibility tables.
//! The public Sony descriptors are byte-exact, with no private HID collections.
//! Report field positions are interoperability facts published in Sony's Linux
//! hid-playstation driver and SDL's HIDAPI PS4/PS5 implementations. See PROVENANCE.md.
//! Hardware/game compatibility remains subject to live integration testing.
use scx_vpad_protocol::*;

pub const MAX_REPORT_SIZE: usize = 64;

// Offsets include the USB report ID. SDL's PS5StatePacket_t excludes it.
const DS5_PACKET_SEQUENCE: core::ops::Range<usize> = 12..16;
const DS5_SENSOR_TIME: core::ops::Range<usize> = 28..32;
const DS5_REPORT_TIME: core::ops::Range<usize> = 49..53;
const DS5_CONNECTION_STATE: usize = 54;
const DS5_USB_CONNECTED: u8 = 0x08;

/// Synthesizes the 64-byte USB report; no Bluetooth CRC/transport is involved.
pub fn encode(profile: Profile, state: &InputState) -> Option<[u8; MAX_REPORT_SIZE]> {
    state.validate().ok()?;
    encode_validated(profile, state)
}

/// Internal fast path after strict InputState::decode/validate. Wire-facing and
/// public callers still validate before entering here, once per submission.
pub(crate) fn encode_validated(
    profile: Profile,
    state: &InputState,
) -> Option<[u8; MAX_REPORT_SIZE]> {
    if !(profile.is_generic_hid() || matches!(profile, Profile::DualShock4 | Profile::DualSense)) {
        return None;
    }
    let mut r = [0; MAX_REPORT_SIZE];
    if profile.is_generic_hid() {
        r[..HID_REPORT_SIZE].copy_from_slice(&super::encode_report(state));
        return Some(r);
    }
    r[0] = 1;
    r[1] = axis(state.left_x);
    r[2] = axis(state.left_y);
    r[3] = axis(state.right_x);
    r[4] = axis(state.right_y);
    let face = state.hat
        | bit(state, BUTTON_WEST, 4)
        | bit(state, BUTTON_SOUTH, 5)
        | bit(state, BUTTON_EAST, 6)
        | bit(state, BUTTON_NORTH, 7);
    let shoulders = bit(state, BUTTON_LEFT_SHOULDER, 0)
        | bit(state, BUTTON_RIGHT_SHOULDER, 1)
        | ((state.buttons & BUTTON_LEFT_TRIGGER != 0 || state.left_trigger != 0) as u8) << 2
        | ((state.buttons & BUTTON_RIGHT_TRIGGER != 0 || state.right_trigger != 0) as u8) << 3
        | bit(state, BUTTON_SELECT, 4)
        | bit(state, BUTTON_START, 5)
        | bit(state, BUTTON_LEFT_STICK, 6)
        | bit(state, BUTTON_RIGHT_STICK, 7);
    let system = bit(state, BUTTON_GUIDE, 0) | bit(state, BUTTON_TOUCHPAD, 1);
    if profile == Profile::DualShock4 {
        r[5] = face;
        r[6] = shoulders;
        r[7] = system | ((state.sequence as u8 & 63) << 2);
        r[8] = trigger(state.left_trigger);
        r[9] = trigger(state.right_trigger);
        // DS4 sensor ticks are 16/3 microseconds. Quotient/remainder avoids
        // u64 overflow and an expensive 128-bit division on every input.
        let ticks = (state.timestamp_us / 16 * 3 + state.timestamp_us % 16 * 3 / 16) as u16;
        r[10..12].copy_from_slice(&ticks.to_le_bytes());
        motion(state, &mut r, 13, 19);
        r[30] = 0x1b; // wired power/full battery, synthetic (not the physical battery)
        r[33] = 1;
        r[34] = state.sequence as u8;
        contact(&mut r[35..39], state, 0, 942);
        contact(&mut r[39..43], state, 1, 942);
        // Historical touch slots are empty, not phantom active fingers.
        for offset in [44, 48, 53, 57] {
            r[offset] = 0x80;
        }
    } else {
        r[5] = trigger(state.left_trigger);
        r[6] = trigger(state.right_trigger);
        r[7] = state.sequence as u8;
        r[8] = face;
        r[9] = shoulders;
        r[10] = system | bit(state, BUTTON_MICROPHONE, 2);
        // Native Sony consumers also inspect the full 32-bit packet sequence;
        // the 8-bit HID counter alone does not describe report freshness.
        r[DS5_PACKET_SEQUENCE].copy_from_slice(&state.sequence.to_le_bytes());
        motion(state, &mut r, 16, 22);
        // DualSense sensor tick is one third microsecond, wrapping at 32 bits.
        let ticks = (state.timestamp_us.wrapping_mul(3) as u32).to_le_bytes();
        r[DS5_SENSOR_TIME].copy_from_slice(&ticks);
        // Sensors and the synthetic USB report are sampled together.
        r[DS5_REPORT_TIME].copy_from_slice(&ticks);
        contact(&mut r[33..37], state, 0, 1080);
        contact(&mut r[37..41], state, 1, 1080);
        r[53] = 0x2a; // synthetic wired full battery
        r[DS5_CONNECTION_STATE] = DS5_USB_CONNECTED;
    }
    Some(r)
}

fn axis(value: i16) -> u8 {
    ((i32::from(value) + 32768) >> 8) as u8
}
fn trigger(value: u16) -> u8 {
    (u32::from(value) * 255 / 65535) as u8
}
fn bit(state: &InputState, mask: u32, shift: u8) -> u8 {
    ((state.buttons & mask != 0) as u8) << shift
}
fn motion(state: &InputState, report: &mut [u8; 64], gyro_offset: usize, accel_offset: usize) {
    for i in 0..3 {
        let gyro = if state.flags & FLAG_GYRO_VALID != 0 {
            state.gyro[i]
        } else {
            0
        };
        let accel = if state.flags & FLAG_ACCEL_VALID != 0 {
            (i32::from(state.accel[i]) * 8192 / 1000).clamp(i16::MIN as i32, i16::MAX as i32) as i16
        } else {
            0
        };
        report[gyro_offset + i * 2..gyro_offset + i * 2 + 2].copy_from_slice(&gyro.to_le_bytes());
        report[accel_offset + i * 2..accel_offset + i * 2 + 2]
            .copy_from_slice(&accel.to_le_bytes());
    }
}
fn contact(report: &mut [u8], state: &InputState, index: usize, height: u32) {
    let point = state.touches[index];
    let active = state.flags & FLAG_TOUCH_VALID != 0 && point.active();
    let x = u32::from(point.x) * 1919 / 65535;
    let y = u32::from(point.y) * (height - 1) / 65535;
    report[0] = (point.id & 127) | if active { 0 } else { 128 };
    report[1] = x as u8;
    report[2] = ((x >> 8) as u8 & 15) | ((y as u8 & 15) << 4);
    report[3] = (y >> 4) as u8;
}

#[path = "sony_compat.rs"]
mod sony_compat;
mod universal_descriptors;
#[cfg(test)]
mod universal_tests;

/// Synthetic, internally consistent motion calibration (zero bias, 16 gyro units
/// per degree/s, 8192 acceleration units/g). This is not captured Sony firmware.
pub fn feature(profile: Profile, report_id: u8, instance_id: u64) -> Option<([u8; 64], usize)> {
    let mut r = [0; 64];
    r[0] = report_id;
    match (profile, report_id) {
        (Profile::DualShock4, 2) | (Profile::DualSense, 5) => {
            // Use the observed Maestro reference range (+/-500 deg/s,
            // +/-8000 raw). The former +/-90 and +/-1440 had the same
            // 16 raw units/deg/s ratio, but a different feature payload.
            // This isolates that real difference without rescaling motion.
            // Capture: sony-protocol-20261023-145408-617-maestro-protocol.txt.
            for offset in [7, 11, 15] {
                signed(&mut r, offset, 8000);
                signed(&mut r, offset + 2, -8000);
            }
            signed(&mut r, 19, 500);
            signed(&mut r, 21, 500);
            for offset in [23, 27, 31] {
                signed(&mut r, offset, 8192);
                signed(&mut r, offset + 2, -8192);
            }
            Some((
                r,
                if profile == Profile::DualShock4 {
                    37
                } else {
                    41
                },
            ))
        }
        (Profile::DualShock4, 0x12) | (Profile::DualSense, 9) => {
            // A locally administered, unicast synthetic identity, unique to this
            // service-owned device. Report stores the address least-significant first.
            let identity = instance_id.to_le_bytes();
            r[1..6].copy_from_slice(&identity[..5]);
            r[6] = 0x02;
            Some((
                r,
                if profile == Profile::DualShock4 {
                    16
                } else {
                    20
                },
            ))
        }
        (Profile::DualShock4, 0xa3) => {
            r[..49].copy_from_slice(sony_compat::DS4_FIRMWARE_INFO);
            Some((r, 49))
        }
        (Profile::DualSense, 0x20) => {
            r.copy_from_slice(sony_compat::DS5_FIRMWARE_INFO);
            Some((r, 64))
        }
        // A zero patch version is the valid "no Bluetooth patch" response.
        (Profile::DualSense, 0x22) => Some((r, 64)),
        _ => None,
    }
}
fn signed(r: &mut [u8; 64], offset: usize, value: i16) {
    r[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub fn attributes(profile: Profile) -> Option<(u16, u16, u16)> {
    match profile {
        Profile::GenericHid => Some((0, 0x5343, 0x0100)),
        Profile::Wheel => Some((0, 0x5346, 0x0100)),
        Profile::FlightStick => Some((0, 0x5347, 0x0100)),
        Profile::ArcadeStick => Some((0, 0x5348, 0x0100)),
        Profile::DualShock4 => Some((0x054c, 0x09cc, 0x0100)),
        Profile::DualSense => Some((0x054c, 0x0ce6, 0x0100)),
        _ => None,
    }
}

/// Decode supported USB output controls. Audio/adaptive fields in mixed packets
/// are ignored; they are not represented as implemented in the feedback ABI.
pub fn parse_output(profile: Profile, report: &[u8]) -> Result<Feedback, ValidationError> {
    let mut feedback = Feedback::empty(profile);
    match profile {
        Profile::DualShock4 => {
            if report.len() != 32 {
                return Err(ValidationError::Length);
            }
            if report[0] != 5 {
                return Err(ValidationError::Magic);
            }
            // Public Sony reports can combine supported rumble/LED controls
            // with audio and hardware-only flags. Ignore those fields rather
            // than rejecting the entire fixed-size packet during game init.
            if report[1] & 1 != 0 {
                feedback.valid |= FEEDBACK_RUMBLE;
                feedback.right_motor = u16::from(report[4]) * 257;
                feedback.left_motor = u16::from(report[5]) * 257;
            }
            if report[1] & 2 != 0 {
                feedback.valid |= FEEDBACK_RGB;
                feedback.rgb.copy_from_slice(&report[6..9]);
            }
            if report[1] & 4 != 0 {
                feedback.valid |= FEEDBACK_BLINK;
                feedback.blink_on = report[9];
                feedback.blink_off = report[10];
            }
        }
        Profile::DualSense => {
            // The HID report is 48 bytes. USB clients may pad it to the
            // 63-byte wire packet; every supported field fits in the first 48.
            if !matches!(report.len(), 48 | 63) {
                return Err(ValidationError::Length);
            }
            if report[0] != 2 {
                return Err(ValidationError::Magic);
            }
            // Gears initializes with 0x0d/0x17: rumble, both adaptive triggers,
            // power control and LEDs in the same packet. The virtual pad has
            // no audio/trigger engine. Ignore unsupported fields, but still
            // apply the supported fields and acknowledge the complete report.
            // Exact length/ID checks above keep every offset bounded.
            if report[1] & 3 != 0 || report[39] & 4 != 0 {
                feedback.valid |= FEEDBACK_RUMBLE;
                feedback.right_motor = u16::from(report[3]) * 257;
                feedback.left_motor = u16::from(report[4]) * 257;
            } else if report[1] == 0
                && report[2] == 0
                && report[39] == 0
                && report[3] == 0
                && report[4] == 0
            {
                // SDL's empty effects packet restores audio haptics and stops
                // emulated rumble. Do not keep the previous motor snapshot.
                // Updates explicitly selecting other fields keep merge rules.
                feedback.valid |= FEEDBACK_RUMBLE;
            }
            if report[2] & 4 != 0 {
                feedback.valid |= FEEDBACK_RGB;
                feedback.rgb.copy_from_slice(&report[45..48]);
            }
            if report[2] & 0x10 != 0 {
                feedback.valid |= FEEDBACK_PLAYER;
                feedback.player = report[44];
            }
            if report[2] & 1 != 0 {
                feedback.valid |= FEEDBACK_MICROPHONE;
                feedback.microphone = report[9];
            }
        }
        _ => return Err(ValidationError::Profile),
    }
    Ok(feedback)
}

pub fn descriptor(profile: Profile) -> Option<&'static [u8]> {
    match profile {
        Profile::GenericHid | Profile::ArcadeStick => Some(super::REPORT_DESCRIPTOR),
        Profile::Wheel => Some(universal_descriptors::WHEEL),
        Profile::FlightStick => Some(universal_descriptors::FLIGHT_STICK),
        Profile::DualShock4 => Some(sony_compat::DS4_USB_DESCRIPTOR),
        Profile::DualSense => Some(sony_compat::DS5_USB_DESCRIPTOR),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // A small independent HID item interpreter: bind fields by semantic usage,
    // never by the encoder's hard-coded offsets. Native HidP + WGI probes cover
    // the installed descriptor separately on Windows.
    fn desktop_values(profile: Profile, state: InputState) -> [u32; 7] {
        let bytes = descriptor(profile).unwrap();
        let report = encode(profile, &state).unwrap();
        let (mut pos, mut page, mut id, mut size, mut count) = (0, 0, 0, 0, 0);
        let mut offsets = [8usize; 256]; // report ID occupies the first byte
        let mut usages = Vec::new();
        let mut result = [u32::MAX; 7];
        while pos < bytes.len() {
            let tag = bytes[pos];
            pos += 1;
            let n = if tag & 3 == 3 { 4 } else { (tag & 3) as usize };
            let mut value = 0u32;
            for i in 0..n {
                value |= (bytes[pos + i] as u32) << (8 * i);
            }
            pos += n;
            match tag & 0xfc {
                0x04 => page = value,
                0x08 => usages.push(value),
                0x74 => size = value as usize,
                0x94 => count = value as usize,
                0x84 => id = value as usize,
                0x80 => {
                    if page == 1 && id == 1 && value & 1 == 0 {
                        assert_eq!(usages.len(), count);
                        for (field, usage) in usages.iter().enumerate() {
                            if let Some(index) = [0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x39]
                                .iter()
                                .position(|u| u == usage)
                            {
                                let mut decoded = 0;
                                for bit in 0..size {
                                    let offset = offsets[id] + field * size + bit;
                                    decoded |=
                                        u32::from((report[offset / 8] >> (offset % 8)) & 1) << bit;
                                }
                                assert_eq!(result[index], u32::MAX, "duplicate HID usage");
                                result[index] = decoded;
                            }
                        }
                    }
                    offsets[id] += size * count;
                }
                _ => (),
            }
            if tag & 0x0c == 0 {
                usages.clear();
            }
        }
        result
    }
    #[test]
    fn sony_descriptors_separate_sticks_and_triggers_by_hid_usage() {
        for profile in [Profile::DualShock4, Profile::DualSense] {
            assert_eq!(
                desktop_values(profile, InputState::NEUTRAL),
                [128, 128, 128, 0, 0, 128, 8]
            );
            let fields: [fn(&mut InputState); 6] = [
                |s| s.left_x = i16::MAX,
                |s| s.left_y = i16::MIN,
                |s| s.right_x = i16::MIN,
                |s| s.left_trigger = u16::MAX,
                |s| s.right_trigger = u16::MAX,
                |s| s.right_y = i16::MAX,
            ];
            for (index, set) in fields.iter().enumerate() {
                let mut state = InputState::NEUTRAL;
                set(&mut state);
                let mut expected = [128, 128, 128, 0, 0, 128, 8];
                expected[index] = if index == 1 || index == 2 { 0 } else { 255 };
                assert_eq!(desktop_values(profile, state), expected);
            }
            for hat in 0..=8 {
                let mut expected = [128, 128, 128, 0, 0, 128, 8];
                expected[6] = u32::from(hat);
                assert_eq!(
                    desktop_values(
                        profile,
                        InputState {
                            hat,
                            ..InputState::NEUTRAL
                        }
                    ),
                    expected
                );
            }
        }
    }
    #[test]
    fn neutral_profiles_have_centered_axes_released_hat_and_no_phantom_fingers() {
        for profile in [Profile::DualShock4, Profile::DualSense] {
            let r = encode(profile, &InputState::NEUTRAL).unwrap();
            assert_eq!(&r[1..5], &[128; 4]);
            let (hat, t0, t1) = if profile == Profile::DualShock4 {
                (5, 35, 39)
            } else {
                (8, 33, 37)
            };
            assert_eq!(r[hat] & 15, 8);
            assert_ne!(r[t0] & 128, 0);
            assert_ne!(r[t1] & 128, 0);
        }
    }
    #[test]
    fn faces_triggers_motion_and_touch_pack_at_documented_usb_positions() {
        let s = InputState {
            sequence: 7,
            buttons: BUTTON_SOUTH | BUTTON_NORTH | BUTTON_GUIDE,
            left_x: i16::MIN,
            right_x: i16::MAX,
            left_trigger: 65535,
            flags: 7,
            gyro: [160, -320, 0],
            accel: [1000, -1000, 32767],
            touches: [
                TouchPoint {
                    x: 65535,
                    y: 65535,
                    id: 2,
                    flags: 1,
                },
                TouchPoint::NONE,
            ],
            timestamp_us: 160,
            ..InputState::NEUTRAL
        };
        let d4 = encode(Profile::DualShock4, &s).unwrap();
        let d5 = encode(Profile::DualSense, &s).unwrap();
        assert_eq!(d4[1], 0);
        assert_eq!(d4[3], 255);
        assert_eq!(d4[5], 0xa8);
        assert_eq!(d5[8], 0xa8);
        assert_eq!(d4[8], 255);
        assert_eq!(d5[5], 255);
        assert_eq!(&d4[10..12], &30u16.to_le_bytes());
        assert_eq!(&d5[28..32], &480u32.to_le_bytes());
        assert_eq!(&d4[13..15], &160i16.to_le_bytes());
        assert_eq!(&d5[22..24], &8192i16.to_le_bytes());
        assert_eq!(&d5[26..28], &32767i16.to_le_bytes());
        assert_eq!(&d5[33..37], &[2, 0x7f, 0x77, 0x43]);
    }
    #[test]
    fn dualsense_usb_status_and_extended_counters_survive_wrap() {
        // Independent wire offsets from the published 64-byte USB packet.
        // Exercise both counter widths and a sensor-clock overflow while the
        // controls stay held: fresh reports must not look like duplicate zero.
        for (sequence, timestamp_us, ticks) in [
            (0xfe, 123_456, 370_368u32),
            (0xff, 123_457, 370_371),
            (0x100, 123_458, 370_374),
            (u32::MAX, 1_431_655_765, u32::MAX),
            (0, 1_431_655_766, 2),
            (1, u64::MAX, u32::MAX - 2),
        ] {
            let r = encode(
                Profile::DualSense,
                &InputState {
                    sequence,
                    timestamp_us,
                    buttons: BUTTON_SOUTH | BUTTON_START,
                    ..InputState::NEUTRAL
                },
            )
            .unwrap();
            assert_eq!(r[0], 1);
            assert_eq!(r[7], sequence as u8);
            assert_eq!(u32::from_le_bytes(r[12..16].try_into().unwrap()), sequence);
            assert_eq!(u32::from_le_bytes(r[28..32].try_into().unwrap()), ticks);
            assert_eq!(u32::from_le_bytes(r[49..53].try_into().unwrap()), ticks);
            assert_eq!(r[53], 0x2a);
            assert_eq!(r[54], 0x08); // USB present, no fictitious headset
            assert_eq!(r[8], 0x28); // Cross held, d-pad neutral
            assert_eq!(r[9], 0x20); // Options held
            assert_eq!(r[33] & 0x80, 0x80);
            assert_eq!(r[37] & 0x80, 0x80);
        }
        let neutral = encode(Profile::DualSense, &InputState::NEUTRAL).unwrap();
        assert_eq!(neutral[54], 0x08); // neutral does not mean disconnected
    }
    #[test]
    fn synthetic_calibration_is_nonzero_and_matches_encoder_units() {
        for (profile, id) in [(Profile::DualShock4, 2), (Profile::DualSense, 5)] {
            let (r, _) = feature(profile, id, 1).unwrap();
            let read = |offset| i16::from_le_bytes([r[offset], r[offset + 1]]) as i32;
            assert_eq!((read(19) + read(21)) * 1024 / (read(7) - read(9)), 64);
            assert_eq!(16384 / (read(23) - read(25)), 1);
            assert_eq!(read(1), 0);
        }
    }
    #[test]
    fn sony_calibration_reference_preserves_motion_on_all_axes() {
        // Independent observed feature-05 reference, not generated by encode().
        let expected: [u8; 41] = [
            5, 0, 0, 0, 0, 0, 0, 0x40, 0x1f, 0xc0, 0xe0, 0x40, 0x1f, 0xc0, 0xe0, 0x40, 0x1f, 0xc0,
            0xe0, 0xf4, 1, 0xf4, 1, 0, 0x20, 0, 0xe0, 0, 0x20, 0, 0xe0, 0, 0x20, 0, 0xe0, 0, 0, 0,
            0, 0, 0,
        ];
        let (r, len) = feature(Profile::DualSense, 5, 1).unwrap();
        assert_eq!(len, expected.len());
        assert_eq!(&r[..len], &expected);
        for (profile, id) in [(Profile::DualSense, 5), (Profile::DualShock4, 2)] {
            let (calibration, _) = feature(profile, id, 1).unwrap();
            let read = |offset| {
                i64::from(i16::from_le_bytes([
                    calibration[offset],
                    calibration[offset + 1],
                ]))
            };
            let speed = read(19) + read(21);
            for (axis, offset) in [7, 11, 15].into_iter().enumerate() {
                let range = read(offset) - read(offset + 2);
                for raw in [-16000, -8000, -1600, -16, 0, 16, 1600, 8000, 16000] {
                    // Compare the previous and observed calibrations at real
                    // positive/negative motion samples, not just zero bias.
                    assert_eq!(raw * speed * 1024 / range, raw * 180 * 1024 / 2880);
                    assert_eq!(read(1 + axis * 2), 0);
                }
            }
        }
    }
    #[test]
    fn unknown_features_fail_and_device_identities_differ() {
        assert!(feature(Profile::DualSense, 0xff, 1).is_none());
        assert_ne!(
            feature(Profile::DualSense, 9, 1),
            feature(Profile::DualSense, 9, 2)
        );
        assert!(encode(Profile::Xbox360, &InputState::NEUTRAL).is_none());
    }
    #[test]
    fn malformed_states_cannot_bypass_validation_at_public_or_wire_entry_points() {
        for profile in [
            Profile::GenericHid,
            Profile::DualShock4,
            Profile::DualSense,
            Profile::Wheel,
            Profile::FlightStick,
            Profile::ArcadeStick,
        ] {
            for invalid in [
                InputState {
                    hat: 9,
                    ..InputState::NEUTRAL
                },
                InputState {
                    flags: 0x80,
                    ..InputState::NEUTRAL
                },
                InputState {
                    touches: [
                        TouchPoint {
                            flags: TOUCH_ACTIVE,
                            ..TouchPoint::NONE
                        },
                        TouchPoint::NONE,
                    ],
                    ..InputState::NEUTRAL
                },
            ] {
                assert!(encode(profile, &invalid).is_none());
                let packet = invalid.encode();
                let mut output = [0xa5; MAX_REPORT_SIZE];
                assert_eq!(
                    unsafe {
                        crate::profile_ffi::scx_vpad_encode_report_for_profile(
                            profile as u32,
                            packet.as_ptr(),
                            packet.len(),
                            output.as_mut_ptr(),
                            output.len(),
                        )
                    },
                    crate::STATUS_INVALID_PACKET
                );
                assert_eq!(output, [0xa5; MAX_REPORT_SIZE]);
            }
        }
    }
    #[test]
    fn sony_firmware_handshake_has_valid_types_dates_and_patch_response() {
        let (ds5, n) = feature(Profile::DualSense, 0x20, 1).unwrap();
        assert_eq!(n, 64);
        assert!(matches!(u16::from_le_bytes([ds5[20], ds5[21]]), 2 | 3));
        assert_eq!(u16::from_le_bytes([ds5[22], ds5[23]]), 4);
        assert!(u32::from_le_bytes(ds5[24..28].try_into().unwrap()) > 777);
        assert!(u32::from_le_bytes(ds5[28..32].try_into().unwrap()) > 65655);
        let (patch, n) = feature(Profile::DualSense, 0x22, 1).unwrap();
        assert_eq!(n, 64);
        assert_eq!(patch[0], 0x22);
        assert_eq!(&patch[1..], &[0; 63]);
        let (ds4, n) = feature(Profile::DualShock4, 0xa3, 1).unwrap();
        assert_eq!(n, 49);
        assert_eq!(&ds4[17..25], b"07:01:12");
        assert_eq!(ds4[12], 0); // DS4 date/time are padded, unlike DS5.
    }

    #[test]
    fn descriptor_report_totals_match_all_implemented_features() {
        for profile in [Profile::DualShock4, Profile::DualSense] {
            let d = descriptor(profile).unwrap();
            let mut pos = 0;
            let mut bits = 0;
            let mut count = 0;
            let mut id = 0;
            let mut input = [0u32; 256];
            let mut output = [0u32; 256];
            let mut features = [0u32; 256];
            while pos < d.len() {
                let tag = d[pos];
                pos += 1;
                let n = match tag & 3 {
                    3 => 4,
                    n => n as usize,
                };
                let mut v = 0;
                for i in 0..n {
                    v |= (d[pos + i] as u32) << (i * 8);
                }
                pos += n;
                match tag & 0xfc {
                    0x74 => bits = v,
                    0x94 => count = v,
                    0x84 => id = v as usize,
                    0x80 => input[id] += bits * count,
                    0x90 => output[id] += bits * count,
                    0xb0 => features[id] += bits * count,
                    _ => (),
                }
            }
            assert_eq!(input[1], 504);
            assert_eq!(features[0x7a], 0);
            assert_eq!(features[0x7e], 0);
            assert_eq!(features[0x7f], 0);
            assert_eq!(features[0x7c], 0);
            assert_eq!(input[0x7d], 0); // no second input controller
            assert_eq!(features[0x78], 0); // feedback is outside the public HID
            assert_eq!(
                output[if profile == Profile::DualSense { 2 } else { 5 }],
                if profile == Profile::DualSense {
                    376
                } else {
                    248
                }
            );
            for id in 0..=255u8 {
                if let Some((_, size)) = feature(profile, id, 1) {
                    assert_eq!(features[id as usize], (size as u32 - 1) * 8);
                }
            }
        }
    }
    #[test]
    fn dualsense_hid_and_padded_usb_effects_match() {
        let mut r = [0; 63];
        r[0] = 2;
        r[1] = 3;
        r[2] = 4;
        r[3] = 27;
        r[4] = 111;
        r[45..48].copy_from_slice(&[10, 20, 30]);
        assert_eq!(
            parse_output(Profile::DualSense, &r[..48]),
            parse_output(Profile::DualSense, &r)
        );
        let mut native = [0; FEEDBACK_PACKET_SIZE];
        let status = unsafe {
            crate::profile_ffi::scx_vpad_parse_output_for_profile(
                Profile::DualSense as u32,
                r.as_ptr(),
                48,
                native.as_mut_ptr(),
                native.len(),
            )
        };
        assert_eq!(
            status, 0,
            "Native binding must accept the advertised HID length"
        );
        assert_eq!(
            Feedback::decode(&native).unwrap(),
            parse_output(Profile::DualSense, &r).unwrap()
        );
        assert_eq!(
            parse_output(Profile::DualSense, &r[..47]),
            Err(ValidationError::Length)
        );
        assert_eq!(
            parse_output(Profile::DualSense, &r[..49]),
            Err(ValidationError::Length)
        );
    }
    #[test]
    fn sony_device_has_exactly_one_application_collection_and_no_private_reports() {
        for profile in [Profile::DualShock4, Profile::DualSense] {
            let d = descriptor(profile).unwrap();
            let mut pos = 0;
            let mut id = 0;
            let mut collections = 0;
            while pos < d.len() {
                let tag = d[pos];
                let n = match tag & 3 {
                    3 => 4,
                    n => n as usize,
                };
                let value = if n > 0 { d[pos + 1] } else { 0 };
                if tag & 0xfc == 0x84 {
                    id = value;
                }
                if tag & 0xfc == 0x80 {
                    assert_eq!(id, 1, "Only the gamepad may expose an input report");
                }
                if tag == 0xa1 && value == 1 {
                    collections += 1;
                }
                if tag & 0xfc == 0xb0 {
                    assert!(!(0x78..=0x7f).contains(&id));
                }
                pos += 1 + n;
            }
            assert_eq!(collections, 1);
        }
    }
    #[test]
    fn output_updates_merge_without_stopping_unchanged_rumble() {
        let mut r = [0; 63];
        r[0] = 2;
        r[1] = 3;
        r[3] = 30;
        r[4] = 255;
        let mut latest = Feedback::empty(Profile::DualSense);
        latest
            .merge(parse_output(Profile::DualSense, &r).unwrap())
            .unwrap();
        assert_eq!(latest.left_motor, 65535);
        assert_eq!(latest.right_motor, 30 * 257);
        r[1] = 0;
        r[2] = 4;
        r[45] = 10;
        r[46] = 20;
        r[47] = 30;
        latest
            .merge(parse_output(Profile::DualSense, &r).unwrap())
            .unwrap();
        assert_eq!(latest.rgb, [10, 20, 30]);
        assert_eq!(latest.left_motor, 65535);
        assert_eq!(latest.sequence, 2);
        assert_eq!(Feedback::decode(&latest.encode()), Ok(latest));
    }
    #[test]
    fn dualsense_empty_effects_stop_emulated_rumble() {
        let mut packet = [0; 63];
        packet[0] = 2;
        packet[1] = 3;
        packet[3] = 63;
        packet[4] = 31;
        let mut state = Feedback::empty(Profile::DualSense);
        state
            .merge(parse_output(Profile::DualSense, &packet).unwrap())
            .unwrap();
        assert_ne!(state.left_motor, 0);
        packet[1..].fill(0);
        let stop = parse_output(Profile::DualSense, &packet).unwrap();
        assert_eq!(stop.valid, FEEDBACK_RUMBLE);
        state.merge(stop).unwrap();
        assert_eq!((state.left_motor, state.right_motor), (0, 0));
    }
    #[test]
    fn outputs_reject_wrong_lengths_and_ids_but_ignore_hardware_only_fields() {
        assert_eq!(
            parse_output(Profile::DualSense, &[0; 62]),
            Err(ValidationError::Length)
        );
        let mut r = [0; 63];
        assert_eq!(
            parse_output(Profile::DualSense, &r),
            Err(ValidationError::Magic)
        );
        r[0] = 2;
        r[1] = 4;
        assert_eq!(parse_output(Profile::DualSense, &r).unwrap().valid, 0);
        r[1] = 0x20;
        assert_eq!(parse_output(Profile::DualSense, &r).unwrap().valid, 0);
        let mut d4 = [0; 32];
        d4[0] = 5;
        d4[1] = 7;
        d4[4] = 10;
        d4[5] = 20;
        d4[6..9].copy_from_slice(&[1, 2, 3]);
        let f = parse_output(Profile::DualShock4, &d4).unwrap();
        assert_eq!(f.valid, 19);
        assert_eq!(f.rgb, [1, 2, 3]);
    }
    #[test]
    fn mixed_native_game_outputs_preserve_supported_feedback_and_stop() {
        let mut ds5 = [0x7f; 48];
        ds5[0] = 2;
        ds5[1] = 0x0d; // Observed Gears flags, not a captured game payload.
        ds5[2] = 0x17;
        ds5[3] = 0x55;
        ds5[4] = 0xaa;
        ds5[9] = 1;
        ds5[39] = 0;
        ds5[44] = 3;
        ds5[45..48].copy_from_slice(&[17, 34, 51]);
        let f = parse_output(Profile::DualSense, &ds5).unwrap();
        assert_eq!(
            f.valid,
            FEEDBACK_RUMBLE | FEEDBACK_RGB | FEEDBACK_PLAYER | FEEDBACK_MICROPHONE
        );
        assert_eq!((f.left_motor, f.right_motor), (0xaaaa, 0x5555));
        assert_eq!(f.rgb, [17, 34, 51]);
        assert_eq!((f.player, f.microphone), (3, 1));
        let mut latest = Feedback::empty(Profile::DualSense);
        latest.merge(f).unwrap();
        ds5[3..5].fill(0);
        latest
            .merge(parse_output(Profile::DualSense, &ds5).unwrap())
            .unwrap();
        assert_eq!((latest.left_motor, latest.right_motor), (0, 0));
        assert_eq!(latest.rgb, [17, 34, 51]);

        let mut ds4 = [0x7f; 32];
        ds4[0] = 5;
        ds4[1] = 0xff;
        ds4[2] = 0xff;
        ds4[4] = 0x55;
        ds4[5] = 0xaa;
        ds4[6..9].copy_from_slice(&[17, 34, 51]);
        let f = parse_output(Profile::DualShock4, &ds4).unwrap();
        assert_eq!(f.valid, FEEDBACK_RUMBLE | FEEDBACK_RGB | FEEDBACK_BLINK);
        assert_eq!((f.left_motor, f.right_motor), (0xaaaa, 0x5555));
        assert_eq!(f.rgb, [17, 34, 51]);
    }
}
