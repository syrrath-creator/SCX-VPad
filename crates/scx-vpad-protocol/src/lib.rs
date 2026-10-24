#![no_std]
//! Original SCX binary ABI. No allocation, implicit layout, or native-endian casts.
use core::fmt;

pub const ABI_VERSION: u16 = 1;
/// Device metadata/transport generation. Inner Input64 and Feedback32 stay v1.
pub const DEVICE_INFO_VERSION: u16 = 2;
pub const MAX_DEVICES: usize = 16;
pub const INPUT_PACKET_SIZE: usize = 64;
pub const HID_REPORT_SIZE: usize = 18;
pub const INFO_PACKET_SIZE: usize = 32;
pub const GAMEPAD_REPORT_ID: u8 = 1;
pub const AUTH_CAPABILITY_SIZE: usize = 32;
pub const HID_SUBMIT_PAYLOAD_SIZE: usize = AUTH_CAPABILITY_SIZE + INPUT_PACKET_SIZE;
pub const HID_FEATURE_REPORT_SIZE: usize = 1 + HID_SUBMIT_PAYLOAD_SIZE;
pub const SUBMIT_FEATURE_ID: u8 = 0x7a;
pub const INFO_FEATURE_ID: u8 = 0x7f;
pub const FEEDBACK_FEATURE_ID: u8 = 0x7d;
pub const FEEDBACK_PACKET_SIZE: usize = 32;
pub const FEEDBACK_RUMBLE: u32 = 1;
pub const FEEDBACK_RGB: u32 = 2;
pub const FEEDBACK_PLAYER: u32 = 4;
pub const FEEDBACK_MICROPHONE: u32 = 8;
pub const FEEDBACK_BLINK: u32 = 16;
pub const CONTROL_USAGE_PAGE: u16 = 0xff53;
pub const CONTROL_USAGE: u16 = 1;
pub const CAP_GENERIC_GAMEPAD: u32 = 1;
pub const CAP_MOTION: u32 = 2;
pub const CAP_TOUCH: u32 = 4;
/// Separate XUSB function-device transport; not a generic HID gamepad.
pub const CAP_XINPUT: u32 = 8;
/// Every HID submission requires the secret capability issued with its lease.
pub const CAP_AUTHENTICATED_INPUT: u32 = 16;
/// A generic HID joystick persona; this does not advertise force feedback.
pub const CAP_RAW_JOYSTICK: u32 = 32;
pub const CAP_ARCADE_STICK: u32 = 64;
pub const CAP_EXPERIMENTAL: u32 = 0x8000_0000;
pub const BUTTON_SOUTH: u32 = 1 << 0;
pub const BUTTON_EAST: u32 = 1 << 1;
pub const BUTTON_WEST: u32 = 1 << 2;
pub const BUTTON_NORTH: u32 = 1 << 3;
pub const BUTTON_LEFT_SHOULDER: u32 = 1 << 4;
pub const BUTTON_RIGHT_SHOULDER: u32 = 1 << 5;
pub const BUTTON_SELECT: u32 = 1 << 6;
pub const BUTTON_START: u32 = 1 << 7;
pub const BUTTON_LEFT_STICK: u32 = 1 << 8;
pub const BUTTON_RIGHT_STICK: u32 = 1 << 9;
pub const BUTTON_GUIDE: u32 = 1 << 10;
pub const BUTTON_TOUCHPAD: u32 = 1 << 11;
pub const BUTTON_MICROPHONE: u32 = 1 << 12;
pub const BUTTON_LEFT_TRIGGER: u32 = 1 << 13;
pub const BUTTON_RIGHT_TRIGGER: u32 = 1 << 14;
pub const FLAG_GYRO_VALID: u8 = 1;
pub const FLAG_ACCEL_VALID: u8 = 2;
pub const FLAG_TOUCH_VALID: u8 = 4;
pub const HAT_NEUTRAL: u8 = 8;
pub const TOUCH_ACTIVE: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Profile {
    GenericHid = 1,
    Xbox360 = 2,
    DualShock4 = 3,
    DualSense = 4,
    Steam = 5,
    Wheel = 6,
    FlightStick = 7,
    ArcadeStick = 8,
}
impl Profile {
    /// Reserved profile identifiers do not imply an implemented emulation mode.
    pub const fn is_supported(self) -> bool {
        matches!(
            self,
            Self::GenericHid
                | Self::Xbox360
                | Self::DualShock4
                | Self::DualSense
                | Self::Wheel
                | Self::FlightStick
                | Self::ArcadeStick
        )
    }
    pub const fn is_experimental(self) -> bool {
        matches!(
            self,
            Self::Xbox360
                | Self::DualShock4
                | Self::DualSense
                | Self::Wheel
                | Self::FlightStick
                | Self::ArcadeStick
        )
    }
    /// These original HID personas share the authenticated vendor collection.
    /// Sony uses its separate mapped producer channel and must not match here.
    pub const fn is_generic_hid(self) -> bool {
        matches!(
            self,
            Self::GenericHid | Self::Wheel | Self::FlightStick | Self::ArcadeStick
        )
    }
    pub const fn report_size(self) -> usize {
        match self {
            Self::GenericHid | Self::Wheel | Self::FlightStick | Self::ArcadeStick => {
                HID_REPORT_SIZE
            }
            Self::DualShock4 | Self::DualSense => 64,
            _ => 0,
        }
    }
    pub const fn submit_size(self) -> usize {
        match self {
            Self::GenericHid
            | Self::DualShock4
            | Self::DualSense
            | Self::Wheel
            | Self::FlightStick
            | Self::ArcadeStick => HID_SUBMIT_PAYLOAD_SIZE,
            Self::Xbox360 => INPUT_PACKET_SIZE,
            _ => 0,
        }
    }
    pub const fn capabilities(self) -> u32 {
        match self {
            Self::GenericHid => CAP_GENERIC_GAMEPAD | CAP_AUTHENTICATED_INPUT,
            Self::Wheel | Self::FlightStick => {
                CAP_GENERIC_GAMEPAD | CAP_AUTHENTICATED_INPUT | CAP_RAW_JOYSTICK | CAP_EXPERIMENTAL
            }
            Self::ArcadeStick => {
                CAP_GENERIC_GAMEPAD | CAP_AUTHENTICATED_INPUT | CAP_ARCADE_STICK | CAP_EXPERIMENTAL
            }
            Self::Xbox360 => CAP_XINPUT | CAP_EXPERIMENTAL,
            Self::DualShock4 | Self::DualSense => {
                CAP_GENERIC_GAMEPAD
                    | CAP_MOTION
                    | CAP_TOUCH
                    | CAP_EXPERIMENTAL
                    | CAP_AUTHENTICATED_INPUT
            }
            _ => 0,
        }
    }
}
impl TryFrom<u32> for Profile {
    type Error = ValidationError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::GenericHid),
            2 => Ok(Self::Xbox360),
            3 => Ok(Self::DualShock4),
            4 => Ok(Self::DualSense),
            5 => Ok(Self::Steam),
            6 => Ok(Self::Wheel),
            7 => Ok(Self::FlightStick),
            8 => Ok(Self::ArcadeStick),
            _ => Err(ValidationError::Profile),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationError {
    Length,
    Version,
    Reserved,
    Hat,
    Flags,
    Touch,
    Magic,
    Profile,
    Capabilities,
}
impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Length => "incorrect SCX packet length",
            Self::Version => "unsupported SCX protocol version",
            Self::Reserved => "nonzero SCX reserved field",
            Self::Hat => "invalid directional hat value",
            Self::Flags => "unsupported SCX input flags",
            Self::Touch => "invalid SCX touch state",
            Self::Magic => "incorrect SCX device identity",
            Self::Profile => "unsupported SCX device profile",
            Self::Capabilities => "unsupported SCX device capabilities",
        })
    }
}
impl core::error::Error for ValidationError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TouchPoint {
    pub x: u16,
    pub y: u16,
    pub id: u8,
    pub flags: u8,
}
impl TouchPoint {
    pub const NONE: Self = Self {
        x: 0,
        y: 0,
        id: 0,
        flags: 0,
    };
    pub const fn active(self) -> bool {
        self.flags & TOUCH_ACTIVE != 0
    }
}

/// Normalized input. Stick X is positive right, Y positive down; touch
/// coordinates cover the full unsigned 16-bit range;
/// gyro is 1/16 degree/s and acceleration is milli-g. Motion and touch are
/// exposed by the Sony profiles; GenericHid only exposes buttons/hat/axes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputState {
    pub sequence: u32,
    pub buttons: u32,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
    pub left_trigger: u16,
    pub right_trigger: u16,
    pub hat: u8,
    pub flags: u8,
    pub gyro: [i16; 3],
    pub accel: [i16; 3],
    pub touches: [TouchPoint; 2],
    /// Producer timestamp for diagnostics; never trusted for authorization.
    pub timestamp_us: u64,
}
impl Default for InputState {
    fn default() -> Self {
        Self::NEUTRAL
    }
}
impl InputState {
    pub const NEUTRAL: Self = Self {
        sequence: 0,
        buttons: 0,
        left_x: 0,
        left_y: 0,
        right_x: 0,
        right_y: 0,
        left_trigger: 0,
        right_trigger: 0,
        hat: HAT_NEUTRAL,
        flags: 0,
        gyro: [0; 3],
        accel: [0; 3],
        touches: [TouchPoint::NONE; 2],
        timestamp_us: 0,
    };

    /// No held action. Ignore timestamps, sequence and gravity/acceleration;
    /// valid nonzero angular motion and active touches still require release.
    pub fn is_neutral(&self) -> bool {
        self.buttons == 0
            && self.left_x == 0
            && self.left_y == 0
            && self.right_x == 0
            && self.right_y == 0
            && self.left_trigger == 0
            && self.right_trigger == 0
            && self.hat == HAT_NEUTRAL
            && (self.flags & FLAG_GYRO_VALID == 0 || self.gyro == [0; 3])
            && (self.flags & FLAG_TOUCH_VALID == 0 || self.touches.iter().all(|p| !p.active()))
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.hat > HAT_NEUTRAL {
            return Err(ValidationError::Hat);
        }
        if self.flags & !(FLAG_GYRO_VALID | FLAG_ACCEL_VALID | FLAG_TOUCH_VALID) != 0 {
            return Err(ValidationError::Flags);
        }
        for point in self.touches {
            if point.flags & !TOUCH_ACTIVE != 0
                || (point.active() && self.flags & FLAG_TOUCH_VALID == 0)
                || (point.active() && point.id >= 128)
            {
                return Err(ValidationError::Touch);
            }
        }
        if self.touches[0].active()
            && self.touches[1].active()
            && self.touches[0].id == self.touches[1].id
        {
            return Err(ValidationError::Touch);
        }
        Ok(())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ValidationError> {
        let b: &[u8; INPUT_PACKET_SIZE] = bytes.try_into().map_err(|_| ValidationError::Length)?;
        if read_u16(b, 0) != ABI_VERSION {
            return Err(ValidationError::Version);
        }
        if read_u16(b, 2) as usize != INPUT_PACKET_SIZE {
            return Err(ValidationError::Length);
        }
        if read_u16(b, 26) != 0 || read_u32(b, 60) != 0 {
            return Err(ValidationError::Reserved);
        }
        let state = Self {
            sequence: read_u32(b, 4),
            buttons: read_u32(b, 8),
            left_x: read_u16(b, 12) as i16,
            left_y: read_u16(b, 14) as i16,
            right_x: read_u16(b, 16) as i16,
            right_y: read_u16(b, 18) as i16,
            left_trigger: read_u16(b, 20),
            right_trigger: read_u16(b, 22),
            hat: b[24],
            flags: b[25],
            gyro: [
                read_u16(b, 28) as i16,
                read_u16(b, 30) as i16,
                read_u16(b, 32) as i16,
            ],
            accel: [
                read_u16(b, 34) as i16,
                read_u16(b, 36) as i16,
                read_u16(b, 38) as i16,
            ],
            touches: [
                TouchPoint {
                    x: read_u16(b, 40),
                    y: read_u16(b, 42),
                    id: b[44],
                    flags: b[45],
                },
                TouchPoint {
                    x: read_u16(b, 46),
                    y: read_u16(b, 48),
                    id: b[50],
                    flags: b[51],
                },
            ],
            timestamp_us: u64::from_le_bytes([
                b[52], b[53], b[54], b[55], b[56], b[57], b[58], b[59],
            ]),
        };
        state.validate()?;
        Ok(state)
    }

    pub fn encode(&self) -> [u8; INPUT_PACKET_SIZE] {
        let mut b = [0; INPUT_PACKET_SIZE];
        put_u16(&mut b, 0, ABI_VERSION);
        put_u16(&mut b, 2, INPUT_PACKET_SIZE as u16);
        put_u32(&mut b, 4, self.sequence);
        put_u32(&mut b, 8, self.buttons);
        put_u16(&mut b, 12, self.left_x as u16);
        put_u16(&mut b, 14, self.left_y as u16);
        put_u16(&mut b, 16, self.right_x as u16);
        put_u16(&mut b, 18, self.right_y as u16);
        put_u16(&mut b, 20, self.left_trigger);
        put_u16(&mut b, 22, self.right_trigger);
        b[24] = self.hat;
        b[25] = self.flags;
        for i in 0..3 {
            put_u16(&mut b, 28 + i * 2, self.gyro[i] as u16);
            put_u16(&mut b, 34 + i * 2, self.accel[i] as u16);
        }
        for i in 0..2 {
            let offset = 40 + i * 6;
            let point = self.touches[i];
            put_u16(&mut b, offset, point.x);
            put_u16(&mut b, offset + 2, point.y);
            b[offset + 4] = point.id;
            b[offset + 5] = point.flags;
        }
        b[52..60].copy_from_slice(&self.timestamp_us.to_le_bytes());
        b
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceInfo {
    pub profile: Profile,
    pub capabilities: u32,
    pub implementation_version: u32,
}
impl DeviceInfo {
    pub const CURRENT: Self = Self {
        profile: Profile::GenericHid,
        capabilities: CAP_GENERIC_GAMEPAD | CAP_AUTHENTICATED_INPUT,
        implementation_version: 0x00000100,
    };
    pub const fn for_profile(profile: Profile) -> Self {
        Self {
            profile,
            capabilities: profile.capabilities(),
            implementation_version: 0x00000100,
        }
    }
    pub fn encode(self) -> [u8; INFO_PACKET_SIZE] {
        let mut b = [0; INFO_PACKET_SIZE];
        b[0..4].copy_from_slice(b"SCVP");
        put_u16(&mut b, 4, DEVICE_INFO_VERSION);
        put_u16(&mut b, 6, INFO_PACKET_SIZE as u16);
        put_u32(&mut b, 8, self.profile as u32);
        put_u32(&mut b, 12, self.capabilities);
        put_u16(&mut b, 16, self.profile.submit_size() as u16);
        put_u16(&mut b, 18, self.profile.report_size() as u16);
        put_u16(&mut b, 20, MAX_DEVICES as u16);
        put_u32(&mut b, 24, self.implementation_version);
        b
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, ValidationError> {
        let b: &[u8; INFO_PACKET_SIZE] = bytes.try_into().map_err(|_| ValidationError::Length)?;
        if &b[0..4] != b"SCVP" {
            return Err(ValidationError::Magic);
        }
        if read_u16(b, 4) != DEVICE_INFO_VERSION {
            return Err(ValidationError::Version);
        }
        if read_u16(b, 6) as usize != INFO_PACKET_SIZE || read_u16(b, 20) as usize != MAX_DEVICES {
            return Err(ValidationError::Length);
        }
        if read_u16(b, 22) != 0 || read_u32(b, 28) != 0 {
            return Err(ValidationError::Reserved);
        }
        let profile = Profile::try_from(read_u32(b, 8))?;
        if !profile.is_supported() {
            return Err(ValidationError::Profile);
        }
        if read_u16(b, 18) as usize != profile.report_size()
            || read_u16(b, 16) as usize != profile.submit_size()
        {
            return Err(ValidationError::Length);
        }
        let capabilities = read_u32(b, 12);
        if capabilities != profile.capabilities() {
            return Err(ValidationError::Capabilities);
        }
        Ok(Self {
            profile,
            capabilities,
            implementation_version: read_u32(b, 24),
        })
    }
}

/// Latest valid game-to-controller output; validity bits distinguish "unchanged"
/// from "set to zero". The driver supplies its own monotonically wrapping sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Feedback {
    pub sequence: u32,
    pub valid: u32,
    pub left_motor: u16,
    pub right_motor: u16,
    pub rgb: [u8; 3],
    pub player: u8,
    pub microphone: u8,
    pub blink_on: u8,
    pub blink_off: u8,
    pub profile: Profile,
}
impl Feedback {
    pub const fn empty(profile: Profile) -> Self {
        Self {
            sequence: 0,
            valid: 0,
            left_motor: 0,
            right_motor: 0,
            rgb: [0; 3],
            player: 0,
            microphone: 0,
            blink_on: 0,
            blink_off: 0,
            profile,
        }
    }
    pub fn encode(self) -> [u8; FEEDBACK_PACKET_SIZE] {
        let mut b = [0; FEEDBACK_PACKET_SIZE];
        put_u16(&mut b, 0, ABI_VERSION);
        put_u16(&mut b, 2, FEEDBACK_PACKET_SIZE as u16);
        put_u32(&mut b, 4, self.sequence);
        put_u32(&mut b, 8, self.valid);
        put_u16(&mut b, 12, self.left_motor);
        put_u16(&mut b, 14, self.right_motor);
        b[16..19].copy_from_slice(&self.rgb);
        b[19] = self.player;
        b[20] = self.microphone;
        b[21] = self.blink_on;
        b[22] = self.blink_off;
        put_u32(&mut b, 24, self.profile as u32);
        b
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, ValidationError> {
        let b: &[u8; FEEDBACK_PACKET_SIZE] =
            bytes.try_into().map_err(|_| ValidationError::Length)?;
        if read_u16(b, 0) != ABI_VERSION {
            return Err(ValidationError::Version);
        }
        if read_u16(b, 2) as usize != FEEDBACK_PACKET_SIZE {
            return Err(ValidationError::Length);
        }
        if b[23] != 0 || read_u32(b, 28) != 0 {
            return Err(ValidationError::Reserved);
        }
        let valid = read_u32(b, 8);
        if valid & !31 != 0 {
            return Err(ValidationError::Flags);
        }
        let profile = Profile::try_from(read_u32(b, 24))?;
        if !profile.is_supported() {
            return Err(ValidationError::Profile);
        }
        Ok(Self {
            sequence: read_u32(b, 4),
            valid,
            left_motor: read_u16(b, 12),
            right_motor: read_u16(b, 14),
            rgb: [b[16], b[17], b[18]],
            player: b[19],
            microphone: b[20],
            blink_on: b[21],
            blink_off: b[22],
            profile,
        })
    }
    /// Apply only explicitly valid fields and preserve prior values otherwise.
    pub fn merge(&mut self, update: Self) -> Result<(), ValidationError> {
        if self.profile != update.profile {
            return Err(ValidationError::Profile);
        }
        if update.valid & !31 != 0 {
            return Err(ValidationError::Flags);
        }
        let before = *self;
        if update.valid & FEEDBACK_RUMBLE != 0 {
            self.left_motor = update.left_motor;
            self.right_motor = update.right_motor;
        }
        if update.valid & FEEDBACK_RGB != 0 {
            self.rgb = update.rgb;
        }
        if update.valid & FEEDBACK_PLAYER != 0 {
            self.player = update.player;
        }
        if update.valid & FEEDBACK_MICROPHONE != 0 {
            self.microphone = update.microphone;
        }
        if update.valid & FEEDBACK_BLINK != 0 {
            self.blink_on = update.blink_on;
            self.blink_off = update.blink_off;
        }
        self.valid |= update.valid;
        // Repeated identical commands must not wake feedback consumers.
        if *self != before {
            self.sequence = self.sequence.wrapping_add(1);
        }
        Ok(())
    }
}

/// Strict serial-number ordering, allowing wraparound but rejecting ambiguous jumps.
pub const fn sequence_after(new: u32, previous: u32) -> bool {
    let delta = new.wrapping_sub(previous);
    delta != 0 && delta < 0x80000000
}

fn read_u16(b: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([b[offset], b[offset + 1]])
}
fn read_u32(b: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([b[offset], b[offset + 1], b[offset + 2], b[offset + 3]])
}
fn put_u16(b: &mut [u8], offset: usize, value: u16) {
    b[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn put_u32(b: &mut [u8], offset: usize, value: u32) {
    b[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neutral_is_canonical_and_roundtrips() {
        let packet = InputState::NEUTRAL.encode();
        assert_eq!(&packet[0..4], &[1, 0, 64, 0]);
        assert_eq!(packet[24], 8);
        assert_eq!(InputState::decode(&packet), Ok(InputState::NEUTRAL));
    }
    #[test]
    fn all_fields_roundtrip_without_alignment_or_sign_loss() {
        let s = InputState {
            sequence: u32::MAX,
            buttons: 0x81824344,
            left_x: i16::MIN,
            right_x: i16::MAX,
            left_y: -111,
            right_y: 456,
            left_trigger: u16::MAX,
            right_trigger: 12345,
            hat: 7,
            flags: 7,
            gyro: [-32768, 0, 32767],
            accel: [123, -456, 789],
            touches: [
                TouchPoint {
                    x: 65535,
                    y: 2345,
                    id: 17,
                    flags: 1,
                },
                TouchPoint {
                    x: 333,
                    y: 444,
                    id: 18,
                    flags: 1,
                },
            ],
            timestamp_us: u64::MAX,
        };
        assert_eq!(InputState::decode(&s.encode()), Ok(s));
        assert_eq!(&s.encode()[8..12], &[0x44, 0x43, 0x82, 0x81]);
    }
    #[test]
    fn every_truncated_packet_and_oversize_is_rejected() {
        let packet = InputState::NEUTRAL.encode();
        for size in 0..INPUT_PACKET_SIZE {
            assert_eq!(
                InputState::decode(&packet[..size]),
                Err(ValidationError::Length)
            );
        }
        assert_eq!(InputState::decode(&[0; 65]), Err(ValidationError::Length));
    }
    #[test]
    fn reserved_flags_and_hat_are_rejected() {
        for offset in [26, 27, 60, 61, 62, 63] {
            let mut p = InputState::NEUTRAL.encode();
            p[offset] = 1;
            assert_eq!(InputState::decode(&p), Err(ValidationError::Reserved));
        }
        for hat in 9..=255 {
            let mut s = InputState::NEUTRAL;
            s.hat = hat;
            assert_eq!(s.validate(), Err(ValidationError::Hat));
        }
        for flags in 8..=255 {
            let mut s = InputState::NEUTRAL;
            s.flags = flags;
            assert_eq!(s.validate(), Err(ValidationError::Flags));
        }
    }
    #[test]
    fn touch_ids_and_validity_are_checked() {
        let mut s = InputState::NEUTRAL;
        s.touches[0].flags = 1;
        assert_eq!(s.validate(), Err(ValidationError::Touch));
        s.flags = FLAG_TOUCH_VALID;
        assert!(s.validate().is_ok());
        s.touches[1].flags = 1;
        assert_eq!(s.validate(), Err(ValidationError::Touch));
        s.touches[1].id = 1;
        assert!(s.validate().is_ok());
        s.touches[0].flags = 2;
        assert_eq!(s.validate(), Err(ValidationError::Touch));
    }
    #[test]
    fn neutral_ignores_time_sequence_and_gravity_but_not_held_actions() {
        let mut s = InputState {
            sequence: 42,
            timestamp_us: u64::MAX,
            flags: FLAG_ACCEL_VALID,
            accel: [0, 1000, 0],
            ..InputState::NEUTRAL
        };
        assert!(s.is_neutral());
        s.left_trigger = 1;
        assert!(!s.is_neutral());
        s.left_trigger = 0;
        s.flags |= FLAG_GYRO_VALID;
        s.gyro[0] = 1;
        assert!(!s.is_neutral());
        s.gyro[0] = 0;
        s.flags |= FLAG_TOUCH_VALID;
        s.touches[0].flags = TOUCH_ACTIVE;
        assert!(!s.is_neutral());
        s.touches[0].id = 128;
        assert_eq!(s.validate(), Err(ValidationError::Touch));
    }
    #[test]
    fn info_is_strict_and_reserved_profiles_are_not_supported() {
        assert_eq!(
            DeviceInfo::decode(&DeviceInfo::CURRENT.encode()),
            Ok(DeviceInfo::CURRENT)
        );
        assert!(!Profile::Steam.is_supported());
        let mut reserved = DeviceInfo::CURRENT;
        reserved.profile = Profile::Steam;
        assert_eq!(
            DeviceInfo::decode(&reserved.encode()),
            Err(ValidationError::Profile)
        );
        for profile in [Profile::Xbox360, Profile::DualShock4, Profile::DualSense] {
            assert!(profile.is_experimental());
            let i = DeviceInfo::for_profile(profile);
            assert_eq!(DeviceInfo::decode(&i.encode()), Ok(i));
        }
        assert_eq!(Profile::Xbox360.report_size(), 0);
        assert_eq!(
            Profile::Xbox360.capabilities(),
            CAP_XINPUT | CAP_EXPERIMENTAL
        );
        let f = Feedback::empty(Profile::Xbox360);
        assert_eq!(Feedback::decode(&f.encode()), Ok(f));
    }
    #[test]
    fn authenticated_metadata_rejects_transport_downgrades_and_bad_sizes() {
        assert_eq!(HID_FEATURE_REPORT_SIZE, 97);
        assert_eq!(AUTH_CAPABILITY_SIZE, 32);
        assert_eq!(SUBMIT_FEATURE_ID, 0x7a);
        for profile in [
            Profile::GenericHid,
            Profile::DualShock4,
            Profile::DualSense,
            Profile::Wheel,
            Profile::FlightStick,
            Profile::ArcadeStick,
        ] {
            let info = DeviceInfo::for_profile(profile);
            let canonical = info.encode();
            assert_eq!(read_u16(&canonical, 4), DEVICE_INFO_VERSION);
            assert_eq!(read_u16(&canonical, 16), 96);
            assert_ne!(info.capabilities & CAP_AUTHENTICATED_INPUT, 0);
            for length in 0..INFO_PACKET_SIZE {
                assert_eq!(
                    DeviceInfo::decode(&canonical[..length]),
                    Err(ValidationError::Length)
                );
            }
            assert_eq!(
                DeviceInfo::decode(&[0; INFO_PACKET_SIZE + 1]),
                Err(ValidationError::Length)
            );
            let mut altered = canonical;
            put_u16(&mut altered, 4, 1);
            assert_eq!(DeviceInfo::decode(&altered), Err(ValidationError::Version));
            altered = canonical;
            put_u16(&mut altered, 16, 64);
            assert_eq!(DeviceInfo::decode(&altered), Err(ValidationError::Length));
            altered = canonical;
            put_u32(
                &mut altered,
                12,
                info.capabilities & !CAP_AUTHENTICATED_INPUT,
            );
            assert_eq!(
                DeviceInfo::decode(&altered),
                Err(ValidationError::Capabilities)
            );
            for offset in [22, 23, 28, 29, 30, 31] {
                altered = canonical;
                altered[offset] = 1;
                assert_eq!(DeviceInfo::decode(&altered), Err(ValidationError::Reserved));
            }
        }
        let xbox = DeviceInfo::for_profile(Profile::Xbox360).encode();
        assert_eq!(read_u16(&xbox, 16), 64);
        assert_eq!(read_u32(&xbox, 12) & CAP_AUTHENTICATED_INPUT, 0);
    }
    #[test]
    fn sequence_handles_wrap_duplicates_and_ambiguous_jumps() {
        assert!(sequence_after(0, u32::MAX));
        assert!(!sequence_after(1, 1));
        assert!(!sequence_after(9, 10));
        assert!(sequence_after(10, 9));
        assert!(!sequence_after(0x80000000, 0));
    }
    #[test]
    fn simulation_personas_extend_ids_without_changing_existing_abi() {
        assert_eq!(
            (ABI_VERSION, INPUT_PACKET_SIZE, FEEDBACK_PACKET_SIZE),
            (1, 64, 32)
        );
        for (raw, profile, capability) in [
            (6, Profile::Wheel, CAP_RAW_JOYSTICK),
            (7, Profile::FlightStick, CAP_RAW_JOYSTICK),
            (8, Profile::ArcadeStick, CAP_ARCADE_STICK),
        ] {
            assert_eq!(Profile::try_from(raw), Ok(profile));
            assert!(
                profile.is_supported() && profile.is_experimental() && profile.is_generic_hid()
            );
            assert_eq!((profile.report_size(), profile.submit_size()), (18, 96));
            assert_eq!(
                profile.capabilities(),
                CAP_GENERIC_GAMEPAD | CAP_AUTHENTICATED_INPUT | CAP_EXPERIMENTAL | capability
            );
            assert_eq!(
                profile.capabilities() & (CAP_XINPUT | CAP_MOTION | CAP_TOUCH),
                0
            );
            let feedback = Feedback::empty(profile);
            assert_eq!(Feedback::decode(&feedback.encode()), Ok(feedback));
        }
        for profile in [
            Profile::Xbox360,
            Profile::DualShock4,
            Profile::DualSense,
            Profile::Steam,
        ] {
            assert!(!profile.is_generic_hid());
        }
        assert!(!Profile::Steam.is_supported());
        for raw in [0, 9, 32, u32::MAX] {
            assert_eq!(Profile::try_from(raw), Err(ValidationError::Profile));
        }
    }
    #[test]
    fn adversarial_packet_mutations_never_panic_or_bypass_validation() {
        let canonical = InputState::NEUTRAL.encode();
        // Every possible byte value at every position, preserving the surrounding
        // valid packet, exercises fields beyond the early header checks.
        for offset in 0..INPUT_PACKET_SIZE {
            for value in 0..=255u8 {
                let mut packet = canonical;
                packet[offset] = value;
                if let Ok(state) = InputState::decode(&packet) {
                    assert!(state.validate().is_ok());
                    assert_eq!(state.encode(), packet);
                }
            }
        }
        let mut seed = 0x36fa_8371_2baa_9813u64;
        for _ in 0..50000 {
            let mut packet = canonical;
            for byte in &mut packet[4..60] {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                *byte = seed as u8;
            }
            packet[24] %= 9;
            packet[25] &= 7;
            packet[26] = 0;
            packet[27] = 0;
            packet[45] &= 1;
            packet[51] &= 1;
            if let Ok(state) = InputState::decode(&packet) {
                assert_eq!(state.encode(), packet);
            }
        }
    }
}
