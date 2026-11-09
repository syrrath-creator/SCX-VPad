//! Bounded off/on/off DirectInput regression. No input reports are written.
//! The optional lease uses the public normal-user service API and drops on exit.
#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use scx_vpad_client::{
        source::SourceGuard,
        source_wire::SOURCE_AUTH,
        win::{ioctl, transfer, wide, Handle},
    };
    use std::{
        io::{self, BufRead, Write},
        os::windows::process::CommandExt,
        process::{Child, Command, Stdio},
        sync::mpsc,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Devices::HumanInterfaceDevice::{
            HidD_FreePreparsedData, HidD_GetPreparsedData, HidP_GetCaps, HIDP_CAPS,
            HIDP_STATUS_SUCCESS,
        },
        Foundation::ERROR_ACCESS_DENIED,
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };

    fn owner_contracts(path: &str, guard: &SourceGuard) -> io::Result<()> {
        let handle = Handle::new(unsafe {
            CreateFileW(
                wide(path).as_ptr(),
                0xc0000000,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        })?;
        println!("OWNER_OPEN allowed=true");
        let mut preparsed = 0isize;
        if !unsafe { HidD_GetPreparsedData(handle.0, &mut preparsed) } {
            return Err(io::Error::last_os_error());
        }
        let mut caps = HIDP_CAPS::default();
        let status = unsafe { HidP_GetCaps(preparsed, &mut caps) };
        unsafe {
            HidD_FreePreparsedData(preparsed);
        }
        if status != HIDP_STATUS_SUCCESS || !(1..=4096).contains(&caps.InputReportByteLength) {
            return Err(io::Error::other("invalid source input report size"));
        }
        let mut report = vec![0; caps.InputReportByteLength as usize];
        let denied = |label: &str, result: io::Result<usize>| -> io::Result<()> {
            let error = result.as_ref().err().and_then(io::Error::raw_os_error);
            println!("OWNER_CHECK {label} win32={error:?}");
            if error != Some(ERROR_ACCESS_DENIED as i32) {
                return Err(io::Error::other(format!("{label} was not denied")));
            }
            Ok(())
        };
        denied(
            "unauthenticated-read",
            transfer(&handle, &mut report, false, std::ptr::null_mut(), 1000),
        )?;
        let event = Handle::event()?;
        denied(
            "wrong-token",
            ioctl(&handle, &event, SOURCE_AUTH, &[0; 32], &mut [], 1000),
        )?;
        denied(
            "read-after-wrong-token",
            transfer(&handle, &mut report, false, std::ptr::null_mut(), 1000),
        )?;
        unsafe {
            guard.access().authorize_handle(handle.0)?;
        }
        let bytes = transfer(&handle, &mut report, false, std::ptr::null_mut(), 1000)?;
        println!("OWNER_CHECK authenticated-read bytes={bytes}");
        if bytes == 0 {
            return Err(io::Error::other("authenticated read returned no report"));
        }
        Ok(())
    }

    // The native supervisor places its worker in a kill-on-close Windows job.
    // Killing the supervisor on any early Rust error also cleans up that worker.
    struct Supervisor(Child);
    impl Drop for Supervisor {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn has_successful_state(output: &str) -> bool {
        output
            .lines()
            .filter(|line| line.starts_with("CYCLES "))
            .flat_map(str::split_whitespace)
            .filter_map(|part| part.strip_prefix("success="))
            .filter_map(|count| count.parse::<u32>().ok())
            .any(|count| count > 0)
    }

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 || (args[2] != "--baseline-only" && args[2] != "--with-guard") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Usage: source_dinput <dinput-probe.exe> <instance-guid> --baseline-only|--with-guard",
        ));
    }
    let run = |phase: &str| -> io::Result<(i32, String)> {
        println!("PHASE {phase}");
        let output = Command::new(&args[0])
            .arg(&args[1])
            .arg("5000")
            .creation_flags(0x08000000)
            .output()?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        print!("{text}");
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        let code = output.status.code().unwrap_or(127);
        println!("PHASE_RESULT {phase} code={code}");
        Ok((code, text))
    };
    let (before, output) = run("guard-off-before")?;
    if !matches!(before, 0 | 25) || !has_successful_state(&output) {
        return Err(io::Error::other(format!("baseline failed: {before}")));
    }
    if args[2] == "--baseline-only" {
        return Ok(());
    }
    let path = output
        .lines()
        .find_map(|line| line.strip_prefix("HID_PATH="))
        .ok_or_else(|| io::Error::other("probe did not return its exact physical HID path"))?;
    let guard = SourceGuard::acquire_if_installed(path)?
        .ok_or_else(|| io::Error::other("Source Guard is not installed"))?;
    guard.check()?;
    std::thread::sleep(Duration::from_millis(250));
    let owner_result = owner_contracts(path, &guard);
    if let Err(error) = &owner_result {
        println!("OWNER_CHECK_ERROR {error}");
    }
    println!("PHASE foreign-readwrite-open");
    let foreign = Command::new(&args[0])
        .arg(path)
        .arg("5000")
        .arg("foreign-path")
        .creation_flags(0x08000000)
        .output()?;
    print!("{}", String::from_utf8_lossy(&foreign.stdout));
    let foreign_code = foreign.status.code().unwrap_or(127);
    println!("PHASE_RESULT foreign-readwrite-open code={foreign_code}");
    let guarded = run("guard-on");
    let healthy = guard.check();
    drop(guard);
    // The public lease may remain briefly visible until expiry on disconnect.
    std::thread::sleep(Duration::from_millis(2200));
    let after = run("guard-off-after");
    let (guarded_code, guarded_output) = guarded?;
    healthy?;
    let (after_code, after_output) = after?;
    if !matches!(after_code, 0 | 25) || !has_successful_state(&after_output) {
        return Err(io::Error::other(format!("recovery failed: {after_code}")));
    }

    println!("PHASE preopen-before-guard");
    let mut child = Supervisor(
        Command::new(&args[0])
            .arg(&args[1])
            .arg("5000")
            .arg("preopen")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .creation_flags(0x08000000)
            .spawn()?,
    );
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing child stdout"))?;
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in io::BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    let ready_deadline = Instant::now() + Duration::from_millis(4000);
    loop {
        let line = receiver
            .recv_timeout(ready_deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| io::Error::other("preopen did not reach its READY barrier"))?;
        println!("{line}");
        if line == "READY waiting_for_lease" {
            break;
        }
    }
    let guard = SourceGuard::acquire_if_installed(path)?
        .ok_or_else(|| io::Error::other("Source Guard disappeared"))?;
    std::thread::sleep(Duration::from_millis(350));
    guard.check()?;
    println!("LEASE activated after DirectInput preopen");
    if let Some(mut stdin) = child.0.stdin.take() {
        stdin.write_all(b"GO\n")?;
    }
    let preopen_code = child.0.wait()?.code().unwrap_or(127);
    let preopen_healthy = guard.check();
    drop(guard);
    let _ = reader.join();
    for line in receiver {
        println!("{line}");
    }
    println!("PHASE_RESULT preopen-before-guard code={preopen_code}");
    std::thread::sleep(Duration::from_millis(2200));
    let (final_code, final_output) = run("guard-off-final")?;
    preopen_healthy?;
    println!("SUMMARY before={before} guarded={guarded_code} after={after_code} preopen={preopen_code} final={final_code} foreign={foreign_code}");
    // Disappearing only from DirectInput while guarded is a valid result only
    // if the same known physical path independently denied a foreign RW open,
    // the owning lease remained healthy, and unguarded states recovered.
    let guarded_hidden = guarded_code == 10
        && guarded_output.contains("CALL EnumDevices hr=00000000 ")
        && guarded_output
            .lines()
            .any(|line| line == "ENUM_MATCHES count=0")
        && foreign_code == 0
        && owner_result.is_ok()
        && has_successful_state(&after_output)
        && has_successful_state(&final_output);
    if guarded_hidden {
        println!("GUARDED_HIDDEN validated=raw-open-denied+owner-lease-healthy+physical-recovery");
    }
    if !matches!(final_code, 0 | 25)
        || !has_successful_state(&final_output)
        || !(matches!(guarded_code, 0 | 24 | 25 | 26) || guarded_hidden)
        || !matches!(preopen_code, 0 | 24 | 25 | 26)
    {
        return Err(io::Error::other(format!(
            "regression failed: guarded={guarded_code}, preopen={preopen_code}, final={final_code}"
        )));
    }
    owner_result?;
    if foreign_code != 0 {
        return Err(io::Error::other(format!(
            "foreign RW open was not denied: {foreign_code}"
        )));
    }
    println!("PASS: DirectInput guard-on operation returned and guard-off access recovered; this probe alone does not prove report isolation.");
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows is required.");
    std::process::exit(2);
}
