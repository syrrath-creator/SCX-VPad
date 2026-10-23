//! Small, allocation-free C boundary used by the real WDF binding.
use super::{
    hid_state::{HidState, QueueError},
    *,
};
use scx_vpad_protocol::{Feedback, FEEDBACK_PACKET_SIZE};

const UNSUPPORTED: u32 = 5;
const QUEUE_FULL: u32 = 6;
const _: () = assert!(core::mem::size_of::<HidState>() == 4248);
const _: () = assert!(core::mem::align_of::<HidState>() == 4);

/// # Safety
/// State is aligned writable storage of the C-declared size. On success it is
/// initialized; subsequent state calls require this initialization, exclusive
/// synchronization for mutation, and no overlap with packet/output pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_hid_init(state: *mut HidState, size: usize, profile: u32) -> u32 {
    if state.is_null() {
        return STATUS_INVALID_POINTER;
    }
    if size != core::mem::size_of::<HidState>() {
        return STATUS_INVALID_LENGTH;
    }
    let Some(value) = Profile::try_from(profile).ok().and_then(HidState::new) else {
        return UNSUPPORTED;
    };
    // SAFETY: caller provides aligned writable storage; no uninitialized read.
    unsafe {
        state.write(value);
    }
    STATUS_OK
}

/// # Safety
/// State obeys init's contract. Input is readable for length; neutral is writable
/// for one byte. All regions are disjoint. Rejected input changes no state/output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_hid_submit(
    state: *mut HidState,
    input: *const u8,
    length: usize,
    neutral: *mut u8,
) -> u32 {
    if state.is_null() || input.is_null() || neutral.is_null() {
        return STATUS_INVALID_POINTER;
    }
    if length != INPUT_PACKET_SIZE {
        return STATUS_INVALID_LENGTH;
    }
    // SAFETY: initialized/exclusive state and readable disjoint packet per contract.
    match unsafe { (&mut *state).submit(core::slice::from_raw_parts(input, length)) } {
        Ok(value) => {
            unsafe {
                neutral.write(value as u8);
            }
            STATUS_OK
        }
        Err(QueueError::Full) => QUEUE_FULL,
        Err(_) => STATUS_INVALID_PACKET,
    }
}

/// # Safety
/// Initialized state; returned report pointer is borrowed until the next state
/// mutation. Caller keeps the device lock across peek, copy and consume.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_hid_front(state: *const HidState) -> *const u8 {
    if state.is_null() {
        return core::ptr::null();
    }
    unsafe { (&*state).front().map_or(core::ptr::null(), |r| r.as_ptr()) }
}
/// # Safety
/// Same lifetime/synchronization contract as scx_vpad_hid_front.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_hid_current(state: *const HidState) -> *const u8 {
    if state.is_null() {
        return core::ptr::null();
    }
    unsafe { (&*state).current().as_ptr() }
}
/// # Safety
/// Initialized state exclusively owned/locked by caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_hid_consume(state: *mut HidState) {
    if !state.is_null() {
        unsafe {
            (&mut *state).consume();
        }
    }
}
/// # Safety
/// Initialized state exclusively owned/locked by caller. Replay state is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_hid_neutralize(state: *mut HidState) {
    if !state.is_null() {
        unsafe {
            (&mut *state).neutralize();
        }
    }
}
/// # Safety
/// Initialized state exclusively owned/locked by caller; clears pending reports.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_hid_clear(state: *mut HidState) {
    if !state.is_null() {
        unsafe {
            (&mut *state).clear();
        }
    }
}

/// # Safety
/// Output points to capacity writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_feedback_init(
    profile: u32,
    output: *mut u8,
    capacity: usize,
) -> u32 {
    if output.is_null() {
        return STATUS_INVALID_POINTER;
    }
    if capacity < FEEDBACK_PACKET_SIZE {
        return STATUS_BUFFER_TOO_SMALL;
    }
    let Ok(p) = Profile::try_from(profile) else {
        return UNSUPPORTED;
    };
    if !p.is_supported() {
        return UNSUPPORTED;
    }
    let bytes = Feedback::empty(p).encode();
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
    }
    STATUS_OK
}
/// # Safety
/// Current is readable/writable for length; update readable for update_length;
/// changed is a disjoint writable byte. Packet overlap is permitted. Invalid
/// updates change neither packet nor changed; unchanged valid updates set it to 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scx_vpad_feedback_merge(
    current: *mut u8,
    length: usize,
    update: *const u8,
    update_length: usize,
    changed: *mut u8,
) -> u32 {
    if current.is_null() || update.is_null() || changed.is_null() {
        return STATUS_INVALID_POINTER;
    }
    if length != FEEDBACK_PACKET_SIZE || update_length != FEEDBACK_PACKET_SIZE {
        return STATUS_INVALID_LENGTH;
    }
    let Ok(mut value) = Feedback::decode(unsafe { core::slice::from_raw_parts(current, length) })
    else {
        return STATUS_INVALID_PACKET;
    };
    let Ok(next) = Feedback::decode(unsafe { core::slice::from_raw_parts(update, update_length) })
    else {
        return STATUS_INVALID_PACKET;
    };
    let before = value;
    if value.merge(next).is_err() {
        return STATUS_INVALID_PACKET;
    }
    let differs = before != value;
    if differs {
        let bytes = value.encode();
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), current, bytes.len());
        }
    }
    unsafe {
        changed.write(differs as u8);
    }
    STATUS_OK
}

#[unsafe(no_mangle)]
pub extern "C" fn scx_vpad_sequence_after(new: u32, previous: u32) -> u8 {
    sequence_after(new, previous) as u8
}
