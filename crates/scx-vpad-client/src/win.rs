//! Small audited Win32 ownership and cancellable-I/O helpers.
use std::{io, ptr};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{Threading::*, IO::*},
};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn fail(s: &str) -> io::Error {
    io::Error::other(s)
}
pub fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub struct Handle(pub HANDLE);
// Win32 handles can be moved between threads; this wrapper closes exactly once.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Handle {
    pub fn new(h: HANDLE) -> io::Result<Self> {
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(h))
        }
    }
    pub fn event() -> io::Result<Self> {
        Self::new(unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) })
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// A pinned OVERLAPPED and its event live until completion, including cancellation.
/// `stop` is a manual-reset shutdown event or null. Message-mode reads reject
/// oversized frames instead of draining unbounded input.
pub fn transfer(
    handle: &Handle,
    buffer: &mut [u8],
    write: bool,
    stop: HANDLE,
    timeout: u32,
) -> io::Result<usize> {
    let event = Handle::event()?;
    let mut ov = OVERLAPPED {
        hEvent: event.0,
        ..Default::default()
    };
    let mut n = 0;
    let ok = unsafe {
        if write {
            WriteFile(
                handle.0,
                buffer.as_ptr(),
                buffer.len() as u32,
                &mut n,
                &mut ov,
            )
        } else {
            ReadFile(
                handle.0,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut n,
                &mut ov,
            )
        }
    };
    if ok != 0 {
        return Ok(n as usize);
    }
    if unsafe { GetLastError() } != ERROR_IO_PENDING {
        return Err(io::Error::last_os_error());
    }
    wait_io(handle, &mut ov, stop, timeout).map(|n| n as usize)
}
pub fn wait_io(
    handle: &Handle,
    ov: &mut OVERLAPPED,
    stop: HANDLE,
    timeout: u32,
) -> io::Result<u32> {
    let handles = [ov.hEvent, stop];
    let count = if stop.is_null() { 1 } else { 2 };
    let wait = unsafe { WaitForMultipleObjects(count, handles.as_ptr(), 0, timeout) };
    let mut n = 0;
    if wait == WAIT_OBJECT_0 {
        check(unsafe { GetOverlappedResult(handle.0, ov, &mut n, 0) })?;
        return Ok(n);
    }
    // Never release OVERLAPPED/buffer memory while the kernel still owns it.
    unsafe {
        CancelIoEx(handle.0, ov);
        GetOverlappedResult(handle.0, ov, &mut n, 1);
    }
    Err(io::Error::new(
        if wait == WAIT_TIMEOUT {
            io::ErrorKind::TimedOut
        } else {
            io::ErrorKind::Interrupted
        },
        "I/O cancelled or timed out",
    ))
}

/// Bounded direct driver request using a caller-owned reusable event. The caller
/// serializes access to the event; the report buffers remain live until complete.
pub fn ioctl(
    handle: &Handle,
    event: &Handle,
    code: u32,
    input: &[u8],
    output: &mut [u8],
    timeout: u32,
) -> io::Result<usize> {
    check(unsafe { ResetEvent(event.0) })?;
    let mut ov = OVERLAPPED {
        hEvent: event.0,
        ..Default::default()
    };
    let mut n = 0;
    if unsafe {
        DeviceIoControl(
            handle.0,
            code,
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr().cast()
            },
            input.len() as u32,
            if output.is_empty() {
                ptr::null_mut()
            } else {
                output.as_mut_ptr().cast()
            },
            output.len() as u32,
            &mut n,
            &mut ov,
        )
    } != 0
    {
        return Ok(n as usize);
    }
    if unsafe { GetLastError() } != ERROR_IO_PENDING {
        return Err(io::Error::last_os_error());
    }
    wait_io(handle, &mut ov, ptr::null_mut(), timeout).map(|n| n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows_sys::Win32::System::Pipes::*;
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    fn pair() -> (Handle, Handle) {
        let name = wide(&format!(
            r"\\.\pipe\SCX.VPad.Test.{}.{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let server = Handle::new(unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                1024,
                1024,
                1000,
                ptr::null(),
            )
        })
        .unwrap();
        let client = Handle::new(unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        })
        .unwrap();
        let event = Handle::event().unwrap();
        let mut ov = OVERLAPPED {
            hEvent: event.0,
            ..Default::default()
        };
        assert!(
            unsafe { ConnectNamedPipe(server.0, &mut ov) } != 0
                || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED
        );
        (server, client)
    }
    #[test]
    fn timeout_cancels_and_drains_before_reusing_handle() {
        let (server, client) = pair();
        let mut b = [0; 32];
        assert_eq!(
            transfer(&server, &mut b, false, ptr::null_mut(), 10)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        let mut sent = [42; 32];
        assert_eq!(
            transfer(&client, &mut sent, true, ptr::null_mut(), 1000).unwrap(),
            32
        );
        assert_eq!(
            transfer(&server, &mut b, false, ptr::null_mut(), 1000).unwrap(),
            32
        );
        assert_eq!(b, sent);
    }
    #[test]
    fn stop_event_interrupts_pending_read() {
        let (server, _client) = pair();
        let stop = Handle::event().unwrap();
        unsafe {
            SetEvent(stop.0);
        }
        assert_eq!(
            transfer(&server, &mut [0; 32], false, stop.0, 1000)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
    }
    #[test]
    fn oversized_message_is_rejected_not_truncated() {
        let (server, client) = pair();
        transfer(&client, &mut [99; 64], true, ptr::null_mut(), 1000).unwrap();
        assert_eq!(
            transfer(&server, &mut [0; 32], false, ptr::null_mut(), 1000)
                .unwrap_err()
                .raw_os_error(),
            Some(ERROR_MORE_DATA as i32)
        );
    }
}
