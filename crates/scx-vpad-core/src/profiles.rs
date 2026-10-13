//! Original experimental USB-profile codecs. These are new encoders and newly
//! synthesized descriptors, not copies of physical device descriptors or drivers.
//! Report field positions are interoperability facts published in Sony's Linux
//! hid-playstation driver and SDL's HIDAPI PS4/PS5 implementations. See PROVENANCE.md.
//! Hardware/game compatibility remains subject to live integration testing.
use scx_vpad_protocol::*;

pub const MAX_REPORT_SIZE: usize = 64;

/// Synthesizes the 64-byte USB report; no Bluetooth CRC/transport is involved.
pub fn encode(profile: Profile, state: &InputState) -> Option<[u8; MAX_REPORT_SIZE]> {
    if !matches!(
        profile,
        Profile::GenericHid | Profile::DualShock4 | Profile::DualSense
    ) || state.validate().is_err()
    {
        return None;
    }
    let mut r = [0; MAX_REPORT_SIZE];
    if profile == Profile::GenericHid {
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
        motion(state, &mut r, 16, 22);
        // DualSense sensor tick is one third microsecond, wrapping at 32 bits.
        r[28..32].copy_from_slice(&(state.timestamp_us.wrapping_mul(3) as u32).to_le_bytes());
        contact(&mut r[33..37], state, 0, 1080);
        contact(&mut r[37..41], state, 1, 1080);
        r[53] = 0x2a; // synthetic wired full battery
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

/// Synthetic, internally consistent motion calibration (zero bias, 16 gyro units
/// per degree/s, 8192 acceleration units/g). This is not captured Sony firmware.
pub fn feature(profile: Profile, report_id: u8, instance_id: u64) -> Option<([u8; 64], usize)> {
    let mut r = [0; 64];
    r[0] = report_id;
    match (profile, report_id) {
        (Profile::DualShock4, 2) | (Profile::DualSense, 5) => {
            for offset in [7, 11, 15] {
                signed(&mut r, offset, 1440);
                signed(&mut r, offset + 2, -1440);
            }
            signed(&mut r, 19, 90);
            signed(&mut r, 21, 90);
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
            r[1..12].copy_from_slice(b"Oct 13 2026");
            r[12..20].copy_from_slice(b"00:00:00");
            signed(&mut r, 35, 1);
            signed(&mut r, 41, 0x0100);
            Some((r, 49))
        }
        (Profile::DualSense, 0x20) => {
            r[1..12].copy_from_slice(b"Oct 13 2026");
            r[12..20].copy_from_slice(b"00:00:00");
            r[24..28].copy_from_slice(&1u32.to_le_bytes());
            r[28..32].copy_from_slice(&0x100u32.to_le_bytes());
            // Deliberately identifies the baseline feature level rather than
            // claiming newer adaptive/audio capabilities we have not implemented.
            signed(&mut r, 44, 0x0100);
            Some((r, 64))
        }
        _ => None,
    }
}
fn signed(r: &mut [u8; 64], offset: usize, value: i16) {
    r[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub fn attributes(profile: Profile) -> Option<(u16, u16, u16)> {
    match profile {
        Profile::GenericHid => Some((0, 0x5343, 0x0100)),
        Profile::DualShock4 => Some((0x054c, 0x09cc, 0x0100)),
        Profile::DualSense => Some((0x054c, 0x0ce6, 0x0100)),
        _ => None,
    }
}

/// Supported USB output controls only. Audio streams/adaptive trigger programs
/// are rejected rather than silently acknowledged as if implemented.
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
            if report[1] & !7 != 0 || report[2] != 0 {
                return Err(ValidationError::Flags);
            }
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
            if report.len() != 63 {
                return Err(ValidationError::Length);
            }
            if report[0] != 2 {
                return Err(ValidationError::Magic);
            }
            if report[1] & !3 != 0 || report[2] & !0x1d != 0 || report[39] & !6 != 0 {
                return Err(ValidationError::Flags);
            }
            if report[1] & 3 != 0 || report[39] & 4 != 0 {
                feedback.valid |= FEEDBACK_RUMBLE;
                feedback.right_motor = u16::from(report[3]) * 257;
                feedback.left_motor = u16::from(report[4]) * 257;
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

struct Descriptor {
    bytes: [u8; 384],
    length: usize,
}
impl Descriptor {
    const fn new() -> Self {
        Self {
            bytes: [0; 384],
            length: 0,
        }
    }
    const fn append(mut self, bytes: &[u8]) -> Self {
        let mut i = 0;
        while i < bytes.len() {
            self.bytes[self.length] = bytes[i];
            self.length += 1;
            i += 1;
        }
        self
    }
    const fn feature(self, id: u8, count: u8) -> Self {
        self.append(&[0x85, id, 0x09, id, 0x75, 8, 0x95, count, 0xb1, 2])
    }
}
const fn build_descriptor(ds5: bool) -> Descriptor {
    let mut d = Descriptor::new().append(&[
        0x05, 1, 0x09, 5, 0xa1, 1, 0x85, 1, 0x15, 0, 0x26, 0xff, 0, 0x75, 8, 0x95, 4, 0x09, 0x30,
        0x09, 0x31, 0x09, 0x33, 0x09, 0x34, 0x81, 2,
    ]);
    if ds5 {
        d = d.append(&[
            0x09, 0x32, 0x09, 0x35, 0x95, 2, 0x81, 2, 0x06, 0x53, 0xff, 0x09, 0x10, 0x95, 1, 0x81,
            2, 0x05, 1,
        ]);
    }
    d = d.append(&[
        0x09,
        0x39,
        0x15,
        0,
        0x25,
        7,
        0x35,
        0,
        0x46,
        0x3b,
        1,
        0x65,
        0x14,
        0x75,
        4,
        0x95,
        1,
        0x81,
        0x42,
        0x65,
        0,
        0x35,
        0,
        0x45,
        0,
        0x05,
        9,
        0x19,
        1,
        0x29,
        if ds5 { 15 } else { 14 },
        0x15,
        0,
        0x25,
        1,
        0x75,
        1,
        0x95,
        if ds5 { 15 } else { 14 },
        0x81,
        2,
        0x06,
        0x53,
        0xff,
        0x09,
        0x11,
        0x75,
        1,
        0x95,
        if ds5 { 13 } else { 6 },
        0x81,
        2,
    ]);
    if !ds5 {
        d = d.append(&[
            0x05, 1, 0x09, 0x32, 0x09, 0x35, 0x15, 0, 0x26, 0xff, 0, 0x75, 8, 0x95, 2, 0x81, 2,
        ]);
    }
    d = d.append(&[
        0x06,
        0x53,
        0xff,
        0x09,
        0x12,
        0x15,
        0,
        0x26,
        0xff,
        0,
        0x75,
        8,
        0x95,
        if ds5 { 52 } else { 54 },
        0x81,
        2,
        0x85,
        if ds5 { 2 } else { 5 },
        0x09,
        0x13,
        0x95,
        if ds5 { 62 } else { 31 },
        0x91,
        2,
    ]);
    if ds5 {
        d = d.feature(5, 40).feature(9, 19).feature(0x20, 63);
    } else {
        d = d.feature(2, 36).feature(0x12, 15).feature(0xa3, 48);
    }
    d.append(&[
        0xc0, 0x06, 0x53, 0xff, 0x09, 1, 0xa1, 1, 0x85, 0x7a, 0x09, 2, 0x15, 0, 0x26, 0xff, 0,
        0x75, 8, 0x95, 96, 0xb1, 2, 0x85, 0x7f, 0x09, 3, 0x95, 32, 0xb1, 2, 0x85, 0x7d, 0x09, 4,
        0x95, 32, 0xb1, 2, 0x09, 4, 0x81, 2, 0x85, 0x7c, 0x09, 5, 0x95, 16, 0xb1, 2, 0xc0,
    ])
}
static DS4_DESCRIPTOR: Descriptor = build_descriptor(false);
static DS5_DESCRIPTOR: Descriptor = build_descriptor(true);
pub fn descriptor(profile: Profile) -> Option<&'static [u8]> {
    match profile {
        Profile::GenericHid => Some(super::REPORT_DESCRIPTOR),
        Profile::DualShock4 => Some(&DS4_DESCRIPTOR.bytes[..DS4_DESCRIPTOR.length]),
        Profile::DualSense => Some(&DS5_DESCRIPTOR.bytes[..DS5_DESCRIPTOR.length]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn unknown_features_fail_and_device_identities_differ() {
        assert!(feature(Profile::DualSense, 0xff, 1).is_none());
        assert_ne!(
            feature(Profile::DualSense, 9, 1),
            feature(Profile::DualSense, 9, 2)
        );
        assert!(encode(Profile::Xbox360, &InputState::NEUTRAL).is_none());
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
            assert_eq!(features[0x7a], 768);
            assert_eq!(features[0x7e], 0);
            assert_eq!(features[0x7f], 256);
            assert_eq!(features[0x7c], 128);
            assert_eq!(input[0x7d], 256); // asynchronous feedback, separate vendor TLC
            assert_eq!(
                output[if profile == Profile::DualSense { 2 } else { 5 }],
                if profile == Profile::DualSense {
                    496
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
    fn outputs_reject_wrong_lengths_ids_audio_and_adaptive_requests() {
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
        assert_eq!(
            parse_output(Profile::DualSense, &r),
            Err(ValidationError::Flags)
        );
        r[1] = 0x20;
        assert_eq!(
            parse_output(Profile::DualSense, &r),
            Err(ValidationError::Flags)
        );
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
}
