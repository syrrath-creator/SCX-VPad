//! Actual HID driver queue: bounded, allocation-free and device-local.
//! Readers peek then consume only after a successful copy. No accepted
//! transition is coalesced away; backpressure does not advance the sequence.
use super::{profiles, sequence_after, InputState, Profile};

pub const CAPACITY: usize = 64;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueError {
    InvalidInput,
    StaleSequence,
    Full,
}

/// Private C/Rust storage ABI, NOT a wire packet. C never accesses fields.
/// Layout assertions live in runtime_ffi.rs and driver/abi_check.c.
#[repr(C)]
pub struct HidState {
    profile: u32,
    report_length: u32,
    head: u32,
    count: u32,
    last_sequence: u32,
    has_sequence: u32,
    reports: [[u8; 64]; CAPACITY],
    current: [u8; 64],
    neutral: [u8; 64],
}
impl HidState {
    pub fn new(profile: Profile) -> Option<Self> {
        let neutral = profiles::encode(profile, &InputState::NEUTRAL)?;
        let mut state = Self {
            profile: profile as u32,
            report_length: profile.report_size() as u32,
            head: 0,
            count: 0,
            last_sequence: 0,
            has_sequence: 0,
            reports: [[0; 64]; CAPACITY],
            current: neutral,
            neutral,
        };
        state.neutralize();
        Some(state)
    }
    /// Decode once. All validation/capacity checks precede mutation.
    /// Returns canonical neutrality for the existing watchdog policy.
    pub fn submit(&mut self, packet: &[u8]) -> Result<bool, QueueError> {
        let input = InputState::decode(packet).map_err(|_| QueueError::InvalidInput)?;
        if self.has_sequence != 0 && !sequence_after(input.sequence, self.last_sequence) {
            return Err(QueueError::StaleSequence);
        }
        let profile = Profile::try_from(self.profile).map_err(|_| QueueError::InvalidInput)?;
        let report = profiles::encode(profile, &input).ok_or(QueueError::InvalidInput)?;
        let length = self.report_length as usize;
        if report[..length] != self.current[..length] {
            if self.count as usize == CAPACITY {
                return Err(QueueError::Full);
            }
            let tail = (self.head as usize + self.count as usize) % CAPACITY;
            self.reports[tail] = report;
            self.count += 1;
            self.current = report;
        }
        self.last_sequence = input.sequence;
        self.has_sequence = 1;
        Ok(input.is_neutral())
    }
    pub fn front(&self) -> Option<&[u8]> {
        (self.count != 0).then(|| &self.reports[self.head as usize][..self.report_length as usize])
    }
    pub fn consume(&mut self) {
        if self.count != 0 {
            self.head = (self.head + 1) % CAPACITY as u32;
            self.count -= 1;
        }
    }
    pub fn current(&self) -> &[u8] {
        &self.current[..self.report_length as usize]
    }
    /// Drop stale presses, retaining replay protection. Only a NEW lease/device
    /// initializes the sequence again; timeout/reactivation must not reset it.
    pub fn neutralize(&mut self) {
        self.head = 0;
        self.count = 1;
        self.current = self.neutral;
        self.reports[0] = self.neutral;
    }
    pub fn clear(&mut self) {
        self.head = 0;
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(sequence: u32, buttons: u32) -> [u8; 64] {
        InputState {
            sequence,
            buttons,
            ..InputState::NEUTRAL
        }
        .encode()
    }
    #[test]
    fn preserves_press_release_and_failed_reader_does_not_consume() {
        for profile in [Profile::GenericHid, Profile::DualShock4, Profile::DualSense] {
            let mut q = HidState::new(profile).unwrap();
            assert_eq!(q.front().unwrap(), q.current());
            q.consume();
            for (seq, buttons) in [(1, 1), (2, 0)] {
                q.submit(&packet(seq, buttons)).unwrap();
            }
            for (seq, buttons) in [(1, 1), (2, 0)] {
                let expected =
                    profiles::encode(profile, &InputState::decode(&packet(seq, buttons)).unwrap())
                        .unwrap();
                assert_eq!(q.front().unwrap(), &expected[..profile.report_size()]);
                assert_eq!(q.front().unwrap(), &expected[..profile.report_size()]);
                q.consume();
            }
            assert!(q.front().is_none());
        }
    }
    #[test]
    fn full_queue_allows_same_sequence_retry_and_keeps_current() {
        let mut q = HidState::new(Profile::GenericHid).unwrap();
        q.consume();
        for i in 0..64 {
            q.submit(&packet(i, 1 + i % 2)).unwrap();
        }
        let before = q.current().to_vec();
        assert_eq!(q.submit(&packet(64, 1)), Err(QueueError::Full));
        assert_eq!(q.current(), before);
        q.consume();
        assert_eq!(q.submit(&packet(64, 1)), Ok(false));
        for i in 1..=64 {
            assert_eq!(q.front().unwrap()[1], (1 + i % 2) as u8);
            q.consume();
        }
        assert!(q.front().is_none());
    }
    #[test]
    fn replay_wrap_timeout_and_new_lease() {
        let mut q = HidState::new(Profile::DualSense).unwrap();
        q.submit(&packet(u32::MAX, 1)).unwrap();
        q.submit(&packet(0, 0)).unwrap();
        for seq in [0, u32::MAX, 0x80000000] {
            assert_eq!(q.submit(&packet(seq, 1)), Err(QueueError::StaleSequence));
        }
        q.neutralize();
        assert_eq!(q.submit(&packet(0, 1)), Err(QueueError::StaleSequence));
        q.consume();
        assert!(q.front().is_none());
        q.submit(&packet(1, 1)).unwrap();
        q = HidState::new(Profile::DualSense).unwrap();
        assert_eq!(q.submit(&packet(0, 1)), Ok(false));
    }
    #[test]
    fn invalid_packet_does_not_change_queue_or_sequence() {
        let mut q = HidState::new(Profile::GenericHid).unwrap();
        let initial = q.current().to_vec();
        let mut invalid = packet(1, 1);
        invalid[24] = 9;
        assert_eq!(q.submit(&invalid), Err(QueueError::InvalidInput));
        assert_eq!(q.current(), initial);
        q.consume();
        assert!(q.front().is_none());
        q.submit(&packet(1, 1)).unwrap();
    }
    #[test]
    fn identical_reports_do_not_fill_queue_and_devices_are_independent() {
        let mut queues: [HidState; 4] =
            core::array::from_fn(|_| HidState::new(Profile::GenericHid).unwrap());
        for q in &mut queues {
            q.consume();
        }
        for i in 0..10000 {
            for (index, q) in queues.iter_mut().enumerate() {
                let buttons = 1 << index;
                q.submit(&packet(i, buttons)).unwrap();
                assert_eq!(q.current()[1], buttons as u8);
            }
        }
        for q in &mut queues {
            q.consume();
            assert!(q.front().is_none());
        }
    }
    #[test]
    fn neutralize_full_queue_guarantees_one_release() {
        let mut q = HidState::new(Profile::GenericHid).unwrap();
        q.consume();
        for i in 0..64 {
            q.submit(&packet(i, 1 + i % 2)).unwrap();
        }
        q.neutralize();
        assert_eq!(q.front().unwrap()[1], 0);
        q.consume();
        assert!(q.front().is_none());
        assert_eq!(q.submit(&packet(63, 1)), Err(QueueError::StaleSequence));
        q.submit(&packet(64, 1)).unwrap();
    }
}
