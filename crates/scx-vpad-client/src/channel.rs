//! Per-lease, service-created Sony transport, separate from the public HID.
//! Fixed atomic words only: no shared pointers, sizes, handles or allocations.
//! One synchronous producer; feedback has an independent sequence and event.
use crate::{win::*, wire::Capability};
use scx_vpad_protocol::{Feedback, InputState, Profile};
use std::{
    io,
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};
use windows_sys::{
    core::GUID,
    Win32::{
        Devices::{DeviceAndDriverInstallation::*, Properties::*},
        Foundation::*,
        Security::SECURITY_ATTRIBUTES,
        System::{Memory::*, Threading::*},
    },
};

pub const BYTES: usize = 512;
const MAGIC: u32 = 0x53435843;
pub struct Channel {
    _section: Handle,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    pub input: Handle,
    done: Handle,
    feedback: Handle,
    pub name: String,
    poisoned: bool,
    #[cfg(feature = "latency-probe")]
    spin_us: u32,
    #[cfg(feature = "latency-probe")]
    legacy_payload_barriers: bool,
}
// Mapped words are accessed atomically; each mutable producer is uniquely owned.
unsafe impl Send for Channel {}
impl Drop for Channel {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.view);
        }
    }
}
impl Channel {
    /// Called by the trusted SYSTEM service before publishing the device ID.
    /// Reject pre-existing named objects instead of trusting their contents/ACL.
    pub fn create(id: &str, sa: &SECURITY_ATTRIBUTES, profile: Profile) -> io::Result<Self> {
        let name = Self::name(id)?;
        let section = Handle::new(unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                sa,
                PAGE_READWRITE,
                0,
                BYTES as u32,
                wide(&name).as_ptr(),
            )
        })?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Err(fail("channel name collision"));
        }
        let event = |suffix: &str| -> io::Result<Handle> {
            let h = Handle::new(unsafe {
                CreateEventW(sa, 0, 0, wide(&format!("{name}.{suffix}")).as_ptr())
            })?;
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                return Err(fail("channel event collision"));
            }
            Ok(h)
        };
        let input = event("in")?;
        let done = event("done")?;
        let feedback = event("feedback")?;
        let c = Self::mapped(section, input, done, feedback, name)?;
        c.word(0).store(MAGIC, Ordering::Release);
        c.word(4).store(1, Ordering::Release);
        c.word(8).store(profile as u32, Ordering::Release);
        Ok(c)
    }
    fn name(id: &str) -> io::Result<String> {
        if id.len() != 32
            || !id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(fail("invalid channel identifier"));
        }
        Ok(format!(r"Global\SCX.VPad.{id}"))
    }
    pub fn open(instance: &str, profile: Profile) -> io::Result<Self> {
        let mut instance = wide(instance);
        let mut node = 0;
        if unsafe { CM_Locate_DevNodeW(&mut node, instance.as_mut_ptr(), 0) } != CR_SUCCESS {
            return Err(fail("channel device missing"));
        }
        let key = DEVPROPKEY {
            fmtid: GUID::from_u128(0x19f81049_3679_40f2_9689_2a2f61f8bd29),
            pid: 6,
        };
        let mut kind = 0;
        let mut id = [0u16; 33];
        let mut size = 66;
        if unsafe {
            CM_Get_DevNode_PropertyW(node, &key, &mut kind, id.as_mut_ptr().cast(), &mut size, 0)
        } != CR_SUCCESS
            || kind != DEVPROP_TYPE_STRING
            || size != 66
            || id[32] != 0
        {
            return Err(fail("channel identity unavailable"));
        }
        let id = String::from_utf16(&id[..32]).map_err(|_| fail("invalid channel identifier"))?;
        let name = Self::name(&id)?;
        let section = Handle::new(unsafe {
            OpenFileMappingW(FILE_MAP_READ | FILE_MAP_WRITE, 0, wide(&name).as_ptr())
        })?;
        let event = |suffix: &str| {
            Handle::new(unsafe {
                OpenEventW(
                    SYNCHRONIZATION_SYNCHRONIZE | EVENT_MODIFY_STATE,
                    0,
                    wide(&format!("{name}.{suffix}")).as_ptr(),
                )
            })
        };
        let c = Self::mapped(
            section,
            event("in")?,
            event("done")?,
            event("feedback")?,
            name,
        )?;
        if c.load(0) != MAGIC || c.load(4) != 1 || c.load(8) != profile as u32 {
            return Err(fail("incompatible channel"));
        }
        if c.load(12) != 1 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "channel driver is starting",
            ));
        }
        Ok(c)
    }
    fn mapped(
        section: Handle,
        input: Handle,
        done: Handle,
        feedback: Handle,
        name: String,
    ) -> io::Result<Self> {
        let view = unsafe { MapViewOfFile(section.0, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, BYTES) };
        if view.Value.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            _section: section,
            view,
            input,
            done,
            feedback,
            name,
            poisoned: false,
            #[cfg(feature = "latency-probe")]
            spin_us: 0,
            #[cfg(feature = "latency-probe")]
            legacy_payload_barriers: false,
        })
    }
    fn word(&self, offset: usize) -> &AtomicU32 {
        assert!(offset.is_multiple_of(4) && offset < BYTES);
        // Section alignment and the fixed offset guarantee atomic alignment.
        unsafe { &*self.view.Value.cast::<u8>().add(offset).cast::<AtomicU32>() }
    }
    fn load(&self, offset: usize) -> u32 {
        self.word(offset).load(Ordering::SeqCst)
    }
    pub fn bytes<const N: usize>(&self, offset: usize) -> [u8; N] {
        assert!(N.is_multiple_of(4) && offset + N <= BYTES);
        let mut out = [0; N];
        for (n, chunk) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            chunk.copy_from_slice(&self.load(offset + n * 4).to_le_bytes());
        }
        out
    }
    /// Benchmarks may compare the old payload barriers and a bounded spin.
    /// These controls are absent from ordinary release builds.
    #[cfg(feature = "latency-probe")]
    pub fn set_submit_tuning(
        &mut self,
        spin_us: u32,
        legacy_payload_barriers: bool,
    ) -> io::Result<()> {
        if spin_us > 50 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "spin budget exceeds 50 us",
            ));
        }
        self.spin_us = spin_us;
        self.legacy_payload_barriers = legacy_payload_barriers;
        Ok(())
    }
    fn store_packet(&self, packet: &[u8; 100]) {
        let ordering = Ordering::Relaxed;
        #[cfg(feature = "latency-probe")]
        let ordering = if self.legacy_payload_barriers {
            Ordering::SeqCst
        } else {
            ordering
        };
        // The completed-ticket check prevents the single producer from writing
        // over an outstanding packet. Publishing ChannelTicket below is the
        // release barrier; the native worker reads it with InterlockedCompareExchange
        // before accessing these aligned atomic words. One publication orders all
        // 25 stores, so a separate full barrier for each word is unnecessary.
        // Keep the atomic words: even a hostile peer must not cause a data race.
        for (n, chunk) in packet.as_chunks::<4>().0.iter().enumerate() {
            self.word(64 + n * 4)
                .store(u32::from_le_bytes(*chunk), ordering);
        }
    }
    pub fn submit(&mut self, capability: &Capability, state: &InputState) -> io::Result<()> {
        if self.poisoned || self.load(12) != 1 {
            return Err(fail("channel stopped or timed out; reconnect"));
        }
        // Never overwrite an outstanding packet, including after a timeout.
        let previous = self.load(16);
        if self.load(20) != previous {
            return Err(fail("channel already has an outstanding request"));
        }
        let mut packet = [0u8; 100];
        packet[0] = 0x7a;
        packet[1..33].copy_from_slice(capability.bytes());
        packet[33..97].copy_from_slice(&state.encode());
        self.store_packet(&packet);
        crate::wire::wipe(&mut packet);
        let ticket = previous.wrapping_add(1);
        self.word(16).store(ticket, Ordering::SeqCst);
        check(unsafe { SetEvent(self.input.0) })?;
        let until = Instant::now() + Duration::from_secs(2);
        #[cfg(feature = "latency-probe")]
        if self.spin_us != 0 {
            let spin_until = Instant::now() + Duration::from_micros(self.spin_us.into());
            while self.load(20) != ticket && self.load(12) == 1 && Instant::now() < spin_until {
                std::hint::spin_loop();
            }
        }
        while self.load(20) != ticket {
            if self.load(12) != 1 {
                self.poisoned = true;
                return Err(fail("channel disconnected"));
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                self.poisoned = true;
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "driver acknowledgement timeout; reconnect",
                ));
            }
            let wait = unsafe {
                WaitForSingleObject(self.done.0, left.as_millis().min(u32::MAX as u128) as u32)
            };
            if wait != WAIT_OBJECT_0 && wait != WAIT_TIMEOUT {
                self.poisoned = true;
                return Err(io::Error::last_os_error());
            }
        }
        match self.load(24) {
            0 => Ok(()),
            code => Err(io::Error::from_raw_os_error(code as i32)),
        }
    }
    pub fn snapshot(&self, profile: Profile) -> io::Result<Feedback> {
        for _ in 0..128 {
            let before = self.load(28);
            if before & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            let bytes = self.bytes::<32>(192);
            if before != self.load(28) {
                continue;
            }
            let f = Feedback::decode(&bytes).map_err(|_| fail("invalid channel feedback"))?;
            if f.profile != profile {
                return Err(fail("feedback profile mismatch"));
            }
            return Ok(f);
        }
        // The driver can be descheduled between the odd/even publications.
        // Contention is transient, not a malformed packet or disconnected pad.
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "feedback publication in progress",
        ))
    }
    pub fn read_feedback(
        &self,
        profile: Profile,
        last: &mut u32,
        timeout: u32,
    ) -> io::Result<Feedback> {
        let until = Instant::now() + Duration::from_millis(timeout.into());
        loop {
            if self.load(12) != 1 {
                return Err(fail("feedback channel disconnected"));
            }
            match self.snapshot(profile) {
                Ok(f) if f.sequence != *last => {
                    *last = f.sequence;
                    return Ok(f);
                }
                Ok(_) => (),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                Err(e) => return Err(e),
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "feedback timeout"));
            }
            let result = unsafe {
                WaitForSingleObject(
                    self.feedback.0,
                    left.as_millis().min(u32::MAX as u128) as u32,
                )
            };
            if result != WAIT_OBJECT_0 && result != WAIT_TIMEOUT {
                return Err(io::Error::last_os_error());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scx_vpad_protocol::{
        TouchPoint, FLAG_ACCEL_VALID, FLAG_GYRO_VALID, FLAG_TOUCH_VALID, TOUCH_ACTIVE,
    };
    use std::sync::{Arc, Barrier};

    fn pair() -> (Channel, Channel) {
        let section = Handle::new(unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                BYTES as u32,
                std::ptr::null(),
            )
        })
        .unwrap();
        // Match the auto-reset events created by the service. A manual-reset
        // event remains signalled and can conceal missed wakeups in these tests.
        let event = || {
            Handle::new(unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) }).unwrap()
        };
        let input = event();
        let done = event();
        let feedback = event();
        let duplicate = |h: &Handle| {
            let mut copy = std::ptr::null_mut();
            check(unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    h.0,
                    GetCurrentProcess(),
                    &mut copy,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            })
            .unwrap();
            Handle::new(copy).unwrap()
        };
        let reader = Channel::mapped(
            duplicate(&section),
            duplicate(&input),
            duplicate(&done),
            duplicate(&feedback),
            String::new(),
        )
        .unwrap();
        let writer = Channel::mapped(section, input, done, feedback, String::new()).unwrap();
        writer.word(12).store(1, Ordering::SeqCst);
        let f = Feedback::empty(Profile::DualSense).encode();
        for (i, c) in f.as_chunks::<4>().0.iter().enumerate() {
            writer
                .word(192 + i * 4)
                .store(u32::from_le_bytes(*c), Ordering::SeqCst);
        }
        (writer, reader)
    }

    // This models the existing native driver's InterlockedCompareExchange(p,0,0),
    // including its successful read/modify/write when the word contains zero.
    // It checks the protocol on real Windows mappings, not the C compiler ABI.
    fn native_read(channel: &Channel, offset: usize) -> u32 {
        channel
            .word(offset)
            .compare_exchange(0, 0, Ordering::SeqCst, Ordering::SeqCst)
            .unwrap_or_else(|value| value)
    }

    fn synthetic_state(channel: u32, round: u32) -> InputState {
        let value = round.wrapping_mul(0x9e37_79b9) ^ channel.wrapping_mul(0x85eb_ca6b);
        let mut state = InputState {
            sequence: round,
            timestamp_us: ((channel as u64 + 1) << 56) | (value as u64) << 16 | round as u64,
            ..InputState::NEUTRAL
        };
        // Every press is immediately followed by a complete neutral release.
        // Presses vary all non-reserved fields, not just the sequence word.
        if round & 1 == 0 {
            state.buttons = (value & 0x7fff) | 1;
            state.left_x = value as i16;
            state.left_y = value.rotate_left(5) as i16;
            state.right_x = value.rotate_left(11) as i16;
            state.right_y = value.rotate_left(17) as i16;
            state.left_trigger = value.rotate_left(3) as u16;
            state.right_trigger = value.rotate_left(19) as u16;
            state.hat = (value % 8) as u8;
            state.flags = FLAG_GYRO_VALID | FLAG_ACCEL_VALID | FLAG_TOUCH_VALID;
            state.gyro = [
                value as i16,
                value.rotate_left(7) as i16,
                value.rotate_left(13) as i16,
            ];
            state.accel = [
                value.rotate_left(2) as i16,
                value.rotate_left(9) as i16,
                value.rotate_left(23) as i16,
            ];
            state.touches = std::array::from_fn(|index| TouchPoint {
                x: value.rotate_left(index as u32 * 7) as u16,
                y: value.rotate_left(9 + index as u32 * 11) as u16,
                id: ((round + channel + index as u32) % 128) as u8,
                flags: TOUCH_ACTIVE,
            });
        }
        state.validate().unwrap();
        state
    }

    fn synthetic_capability(channel: u32) -> [u8; 32] {
        std::array::from_fn(|index| {
            (index as u8)
                .wrapping_mul(7)
                .wrapping_add(channel as u8 + 1)
        })
    }

    fn synthetic_status(round: u32) -> u32 {
        match round % 257 {
            0 => ERROR_ACCESS_DENIED,
            1 => ERROR_INVALID_PARAMETER,
            _ => ERROR_SUCCESS,
        }
    }

    #[test]
    fn four_channels_preserve_every_packet_release_status_and_wrapped_ticket() {
        const ROUNDS: u32 = 4096;
        let start = Arc::new(Barrier::new(8));
        std::thread::scope(|scope| {
            for channel in 0..4 {
                let (mut producer, consumer) = pair();
                let initial = u32::MAX - channel * 17 - 3;
                producer.word(16).store(initial, Ordering::SeqCst);
                producer.word(20).store(initial, Ordering::SeqCst);
                #[cfg(feature = "latency-probe")]
                producer.set_submit_tuning(channel * 15, false).unwrap();
                let consumer_start = Arc::clone(&start);
                scope.spawn(move || {
                    let mut completed = initial;
                    consumer_start.wait();
                    for round in 0..ROUNDS {
                        let ticket = loop {
                            assert_eq!(
                                unsafe { WaitForSingleObject(consumer.input.0, 5000) },
                                WAIT_OBJECT_0
                            );
                            let ticket = native_read(&consumer, 16);
                            if ticket != completed {
                                break ticket;
                            }
                        };
                        assert_eq!(ticket, initial.wrapping_add(round + 1));
                        let mut packet = [0u8; 100];
                        for (word_index, bytes) in
                            packet.as_chunks_mut::<4>().0.iter_mut().enumerate()
                        {
                            bytes.copy_from_slice(
                                &native_read(&consumer, 64 + word_index * 4).to_le_bytes(),
                            );
                            if word_index == 12 && round % 257 == 0 {
                                // Give the producer chances to run mid-copy; it
                                // must still wait for this packet's completion.
                                std::thread::yield_now();
                            }
                        }
                        assert_eq!(packet[0], 0x7a);
                        assert_eq!(packet[1..33], synthetic_capability(channel));
                        let expected = synthetic_state(channel, round);
                        assert_eq!(
                            packet[33..97],
                            expected.encode(),
                            "channel {channel}, round {round}"
                        );
                        assert_eq!(packet[97..], [0; 3]);
                        assert_eq!(
                            InputState::decode(&packet[33..97]).unwrap().is_neutral(),
                            round & 1 != 0
                        );
                        // Native InterlockedExchange publishes status before
                        // completion. Consecutive error/success cases catch a
                        // completion being paired with the previous status.
                        consumer
                            .word(24)
                            .swap(synthetic_status(round), Ordering::SeqCst);
                        completed = ticket;
                        consumer.word(20).swap(completed, Ordering::SeqCst);
                        check(unsafe { SetEvent(consumer.done.0) }).unwrap();
                    }
                });
                let producer_start = Arc::clone(&start);
                scope.spawn(move || {
                    let capability = Capability::new(synthetic_capability(channel));
                    producer_start.wait();
                    for round in 0..ROUNDS {
                        if round % 131 == 0 {
                            // An event alone must never authorize completion.
                            check(unsafe { SetEvent(producer.done.0) }).unwrap();
                        }
                        let result = producer.submit(&capability, &synthetic_state(channel, round));
                        let expected = synthetic_status(round);
                        if expected == ERROR_SUCCESS {
                            result.unwrap();
                        } else {
                            assert_eq!(result.unwrap_err().raw_os_error(), Some(expected as i32));
                        }
                        assert_eq!(producer.load(20), initial.wrapping_add(round + 1));
                        assert!(!producer.poisoned);
                    }
                });
            }
        });
    }

    #[test]
    fn timed_out_packet_is_never_overwritten_even_after_a_late_acknowledgement() {
        let (mut producer, consumer) = pair();
        #[cfg(feature = "latency-probe")]
        producer.set_submit_tuning(50, false).unwrap();
        let capability = Capability::new(synthetic_capability(2));
        assert_eq!(
            producer
                .submit(&capability, &synthetic_state(2, 0))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(producer.poisoned);
        let packet = consumer.bytes::<100>(64);
        let ticket = native_read(&consumer, 16);
        assert_eq!(ticket, 1);
        // A driver may finish a packet after the caller has already timed out.
        consumer.word(20).swap(ticket, Ordering::SeqCst);
        check(unsafe { SetEvent(consumer.done.0) }).unwrap();
        assert!(producer
            .submit(&capability, &synthetic_state(2, 1))
            .is_err());
        assert_eq!(consumer.bytes::<100>(64), packet);
        assert_eq!(native_read(&consumer, 16), ticket);
    }

    #[test]
    fn disconnect_during_a_pending_submit_poisoned_the_channel_without_overwriting() {
        let (mut producer, consumer) = pair();
        let worker = std::thread::spawn(move || {
            assert_eq!(
                unsafe { WaitForSingleObject(consumer.input.0, 5000) },
                WAIT_OBJECT_0
            );
            let packet = consumer.bytes::<100>(64);
            consumer.word(12).swap(0, Ordering::SeqCst);
            check(unsafe { SetEvent(consumer.done.0) }).unwrap();
            (consumer, packet)
        });
        assert!(producer
            .submit(&Capability::default(), &synthetic_state(1, 0))
            .is_err());
        assert!(producer.poisoned);
        let (consumer, packet) = worker.join().unwrap();
        assert!(producer
            .submit(&Capability::default(), &synthetic_state(1, 1))
            .is_err());
        assert_eq!(consumer.bytes::<100>(64), packet);
        assert_eq!(native_read(&consumer, 16), 1);
    }

    #[cfg(feature = "latency-probe")]
    #[test]
    fn submit_spin_budget_is_opt_in_and_bounded() {
        let (mut channel, _) = pair();
        assert_eq!(channel.spin_us, 0);
        assert!(!channel.legacy_payload_barriers);
        channel.set_submit_tuning(50, true).unwrap();
        assert_eq!(
            channel.set_submit_tuning(51, false).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(channel.spin_us, 50);
        assert!(channel.legacy_payload_barriers);
    }

    #[cfg(feature = "latency-probe")]
    #[test]
    #[ignore = "release-mode diagnostic: payload stores only, not driver or input latency"]
    fn payload_store_microbenchmark() {
        use std::hint::black_box;
        const ITERATIONS: usize = 200_000;
        let (mut channel, observer) = pair();
        let packets: [[u8; 100]; 64] = std::array::from_fn(|packet| {
            std::array::from_fn(|byte| (packet as u8).wrapping_mul(11).wrapping_add(byte as u8))
        });
        let mut timings = [Vec::new(), Vec::new()];
        for sample in 0..8 {
            // Alternate order to limit systematic warmup/frequency bias.
            for mode in [sample % 2, 1 - sample % 2] {
                channel.set_submit_tuning(0, mode == 0).unwrap();
                for iteration in 0..16_384 {
                    channel.store_packet(black_box(&packets[iteration % packets.len()]));
                }
                let begin = Instant::now();
                for iteration in 0..ITERATIONS {
                    channel.store_packet(black_box(&packets[iteration % packets.len()]));
                }
                let ns_per_packet = begin.elapsed().as_nanos() as f64 / ITERATIONS as f64;
                assert_eq!(
                    observer.bytes::<100>(64),
                    packets[(ITERATIONS - 1) % packets.len()]
                );
                timings[mode].push(ns_per_packet);
            }
        }
        for (label, samples) in ["legacy SeqCst", "Relaxed payload"]
            .into_iter()
            .zip(&mut timings)
        {
            samples.sort_by(f64::total_cmp);
            println!("{label}: median {:.2} ns / 100-byte store, min {:.2} ns (8 x {ITERATIONS}; payload only; no ticket, event, driver, or end-to-end latency)", (samples[3] + samples[4]) / 2.0, samples[0]);
        }
    }
    #[test]
    fn feedback_timeout_does_not_consume_next_change_and_disconnect_wakes_reader() {
        let (writer, reader) = pair();
        let mut sequence = 0;
        assert_eq!(
            reader
                .read_feedback(Profile::DualSense, &mut sequence, 5)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        writer.word(28).store(1, Ordering::SeqCst);
        writer.word(196).store(1, Ordering::SeqCst); // feedback packet sequence
        writer.word(28).store(2, Ordering::SeqCst);
        unsafe {
            SetEvent(writer.feedback.0);
        }
        assert_eq!(
            reader
                .read_feedback(Profile::DualSense, &mut sequence, 100)
                .unwrap()
                .sequence,
            1
        );
        writer.word(12).store(0, Ordering::SeqCst);
        assert!(reader
            .read_feedback(Profile::DualSense, &mut sequence, 100)
            .is_err());
    }
    #[test]
    fn stopped_or_outstanding_channel_never_overwrites_an_unacknowledged_packet() {
        let (mut c, _) = pair();
        c.word(16).store(7, Ordering::SeqCst);
        c.word(64).store(0x12345678, Ordering::SeqCst);
        assert!(c
            .submit(&Capability::default(), &InputState::NEUTRAL)
            .is_err());
        assert_eq!(c.load(64), 0x12345678);
        c.word(12).store(0, Ordering::SeqCst);
        assert!(c
            .submit(&Capability::default(), &InputState::NEUTRAL)
            .is_err());
    }
    #[test]
    fn preempted_feedback_writer_waits_without_killing_the_connection() {
        let (writer, reader) = pair();
        writer.word(28).store(1, Ordering::SeqCst);
        assert_eq!(
            reader.snapshot(Profile::DualSense).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let mut sequence = 0;
        // A genuinely stalled writer remains bounded by the caller's timeout,
        // and no partially written feedback or new sequence is exposed.
        assert_eq!(
            reader
                .read_feedback(Profile::DualSense, &mut sequence, 5)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(sequence, 0);
        let publishing = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            writer.word(196).store(7, Ordering::SeqCst);
            writer.word(28).store(2, Ordering::SeqCst);
            check(unsafe { SetEvent(writer.feedback.0) }).unwrap();
        });
        assert_eq!(
            reader
                .read_feedback(Profile::DualSense, &mut sequence, 1000)
                .unwrap()
                .sequence,
            7
        );
        publishing.join().unwrap();
        assert_eq!(sequence, 7);
    }
    #[test]
    fn channel_name_cannot_escape_lease_namespace() {
        assert!(Channel::name("0123456789abcdef0123456789abcdef").is_ok());
        for id in [
            "",
            "..",
            "0123456789abcdef0123456789abcde/",
            "0123456789ABCDEF0123456789abcdef",
        ] {
            assert!(Channel::name(id).is_err());
        }
    }
}
