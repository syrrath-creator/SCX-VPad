//! Bounded state queue. Accepted button/hat/trigger/touch transitions cannot be
//! overwritten by another input. Full queues reject input explicitly for retry.
use super::{sequence_after, InputState};

pub const REPORT_QUEUE_CAPACITY: usize = 32;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueError {
    Disconnected,
    InvalidInput,
    StaleSequence,
    Full,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PushResult {
    Queued,
    Coalesced,
}

pub struct ReportQueue {
    reports: [InputState; REPORT_QUEUE_CAPACITY],
    head: usize,
    length: usize,
    last_sequence: Option<u32>,
    connected: bool,
}
impl Default for ReportQueue {
    fn default() -> Self {
        Self::new()
    }
}
impl ReportQueue {
    pub const fn new() -> Self {
        Self {
            reports: [InputState::NEUTRAL; REPORT_QUEUE_CAPACITY],
            head: 0,
            length: 0,
            last_sequence: None,
            connected: true,
        }
    }
    pub fn len(&self) -> usize {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn push(&mut self, state: InputState) -> Result<PushResult, QueueError> {
        if !self.connected {
            return Err(QueueError::Disconnected);
        }
        if state.validate().is_err() {
            return Err(QueueError::InvalidInput);
        }
        if self
            .last_sequence
            .is_some_and(|previous| !sequence_after(state.sequence, previous))
        {
            return Err(QueueError::StaleSequence);
        }
        // Replace pending analogue-only motion to avoid obsolete stick reports.
        // Trigger values are compared exactly: a quick trigger pull must not vanish.
        if self.length > 0 {
            let tail = (self.head + self.length - 1) % REPORT_QUEUE_CAPACITY;
            if may_coalesce(&self.reports[tail], &state) {
                self.reports[tail] = state;
                self.last_sequence = Some(state.sequence);
                return Ok(PushResult::Coalesced);
            }
        }
        if self.length == REPORT_QUEUE_CAPACITY {
            return Err(QueueError::Full);
        }
        let tail = (self.head + self.length) % REPORT_QUEUE_CAPACITY;
        self.reports[tail] = state;
        self.length += 1;
        self.last_sequence = Some(state.sequence);
        Ok(PushResult::Queued)
    }
    pub fn pop(&mut self) -> Option<InputState> {
        if self.length == 0 {
            return None;
        }
        let value = self.reports[self.head];
        self.reports[self.head] = InputState::NEUTRAL;
        self.head = (self.head + 1) % REPORT_QUEUE_CAPACITY;
        self.length -= 1;
        Some(value)
    }
    /// Disconnect cancels pending input and guarantees exactly one neutral report,
    /// including when the queue was full. Caller must dispatch it before teardown.
    pub fn disconnect(&mut self) {
        self.reports.fill(InputState::NEUTRAL);
        self.head = 0;
        self.length = 1;
        self.connected = false;
        self.last_sequence = None;
    }
    /// A fresh owner/device lifetime must not inherit a previous producer's sequence.
    pub fn reconnect(&mut self) {
        *self = Self::new();
    }
}
fn may_coalesce(a: &InputState, b: &InputState) -> bool {
    a.buttons == b.buttons
        && a.hat == b.hat
        && a.left_trigger == b.left_trigger
        && a.right_trigger == b.right_trigger
        && a.flags == b.flags
        && a.touches[0].flags == b.touches[0].flags
        && a.touches[1].flags == b.touches[1].flags
        && a.touches[0].id == b.touches[0].id
        && a.touches[1].id == b.touches[1].id
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(sequence: u32, buttons: u32) -> InputState {
        InputState {
            sequence,
            buttons,
            ..InputState::NEUTRAL
        }
    }
    #[test]
    fn preserves_short_button_press_and_release_in_order() {
        let mut q = ReportQueue::new();
        q.push(state(1, 0)).unwrap();
        q.push(state(2, 1)).unwrap();
        q.push(state(3, 0)).unwrap();
        assert_eq!(q.len(), 3);
        for buttons in [0, 1, 0] {
            assert_eq!(q.pop().unwrap().buttons, buttons);
        }
        assert!(q.pop().is_none());
    }
    #[test]
    fn coalesces_motion_but_not_trigger_or_hat_transitions() {
        let mut q = ReportQueue::new();
        q.push(state(1, 0)).unwrap();
        let mut s = state(2, 0);
        s.left_x = 100;
        assert_eq!(q.push(s), Ok(PushResult::Coalesced));
        assert_eq!(q.len(), 1);
        s.sequence += 1;
        s.left_trigger = 1;
        assert_eq!(q.push(s), Ok(PushResult::Queued));
        s.sequence += 1;
        s.left_trigger = 0;
        assert_eq!(q.push(s), Ok(PushResult::Queued));
        s.sequence += 1;
        s.hat = 0;
        assert_eq!(q.push(s), Ok(PushResult::Queued));
        assert_eq!(q.pop().unwrap().left_x, 100);
    }
    #[test]
    fn full_queue_does_not_advance_sequence_and_allows_retry() {
        let mut q = ReportQueue::new();
        for i in 0..32 {
            q.push(state(i, i % 2)).unwrap();
        }
        assert_eq!(q.push(state(32, 0)), Err(QueueError::Full));
        assert_eq!(q.len(), 32);
        q.pop();
        assert_eq!(q.push(state(32, 0)), Ok(PushResult::Queued));
        for i in 1..=32 {
            assert_eq!(q.pop().unwrap().sequence, i);
        }
    }
    #[test]
    fn disconnect_cancels_pending_input_and_guarantees_neutral_even_when_full() {
        let mut q = ReportQueue::new();
        for i in 0..32 {
            q.push(state(i, i % 2)).unwrap();
        }
        q.disconnect();
        assert!(!q.is_connected());
        assert_eq!(q.len(), 1);
        assert_eq!(q.push(state(33, 1)), Err(QueueError::Disconnected));
        assert_eq!(q.pop(), Some(InputState::NEUTRAL));
        assert!(q.pop().is_none());
        q.reconnect();
        assert_eq!(q.push(state(0, 1)), Ok(PushResult::Queued));
    }
    #[test]
    fn rejects_replay_and_invalid_state_without_changing_queue() {
        let mut q = ReportQueue::new();
        q.push(state(u32::MAX, 0)).unwrap();
        q.push(state(0, 1)).unwrap();
        assert_eq!(q.push(state(0, 0)), Err(QueueError::StaleSequence));
        assert_eq!(q.push(state(u32::MAX, 0)), Err(QueueError::StaleSequence));
        let mut s = state(1, 0);
        s.hat = 9;
        assert_eq!(q.push(s), Err(QueueError::InvalidInput));
        assert_eq!(q.len(), 2);
    }
    #[test]
    fn circular_wrap_retains_all_transitions() {
        let mut q = ReportQueue::new();
        for i in 0..10000 {
            q.push(state(i, i % 2)).unwrap();
            assert_eq!(q.pop().unwrap().sequence, i);
        }
    }
}
