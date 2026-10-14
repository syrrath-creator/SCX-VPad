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
        for (n, chunk) in packet.as_chunks::<4>().0.iter().enumerate() {
            self.word(64 + n * 4)
                .store(u32::from_le_bytes(*chunk), Ordering::SeqCst);
        }
        crate::wire::wipe(&mut packet);
        let ticket = previous.wrapping_add(1);
        self.word(16).store(ticket, Ordering::SeqCst);
        check(unsafe { SetEvent(self.input.0) })?;
        let until = Instant::now() + Duration::from_secs(2);
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
        Err(fail("unstable channel feedback"))
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
            let f = self.snapshot(profile)?;
            if f.sequence != *last {
                *last = f.sequence;
                return Ok(f);
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
        let input = Handle::event().unwrap();
        let done = Handle::event().unwrap();
        let feedback = Handle::event().unwrap();
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
