//! Opt-in public XUSB asynchronous input/cancellation regression test.
#[cfg(not(windows))]
fn main() {}
#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use scx_vpad_client::{
        win::{ioctl, Handle},
        VirtualPad,
    };
    use scx_vpad_protocol::*;
    use std::{
        io,
        time::{Duration, Instant},
    };
    if !std::env::args().any(|a| a == "--run-hardware-test") {
        println!("Opt-in: --run-hardware-test");
        return Ok(());
    }
    let event = Handle::event()?;
    let mut pad = VirtualPad::create_profile(Profile::Xbox360)?;
    let game = pad.open_xusb_consumer()?;
    let wait = |output: &mut [u8], timeout| ioctl(&game, &event, 0x8000e3ac, &[], output, timeout);
    let mut report = [0u8; 29];
    assert_eq!(wait(&mut report, 2000)?, 29);
    assert_eq!(report[2], 3);
    assert_eq!(report[10], 0x14);
    assert_eq!(&report[11..23], &[0; 12]);
    assert_eq!(
        wait(&mut report, 100).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert!(wait(&mut report[..28], 100).is_err());
    assert!(ioctl(&game, &event, 0x8000e3ac, &[3, 1, 1], &mut report, 100).is_err());
    assert!(ioctl(&game, &event, 0x8000e3ac, &[2, 1, 0, 255], &mut report, 100).is_err());
    // The public endpoint must not grant producer access.
    assert!(ioctl(
        &game,
        &event,
        0x22a000,
        &InputState::NEUTRAL.encode(),
        &mut [],
        100
    )
    .is_err());
    let mut pressed = InputState {
        buttons: BUTTON_SOUTH,
        right_trigger: 65535,
        right_x: 12345,
        ..InputState::NEUTRAL
    };
    pad.submit(&pressed)?;
    assert_eq!(wait(&mut report, 2000)?, 29);
    assert_eq!(u16::from_le_bytes([report[11], report[12]]), 0x1000);
    assert_eq!(report[14], 255);
    assert_eq!(i16::from_le_bytes([report[19], report[20]]), 12345);
    let independent = pad.open_xusb_consumer()?;
    assert_eq!(
        ioctl(
            &independent,
            &event,
            0x8000e3ac,
            &[2, 1, 0, 2],
            &mut report,
            2000
        )?,
        29
    );
    assert_eq!(u16::from_le_bytes([report[11], report[12]]), 0x1000);
    // Repeated equivalent reports must not wake the consumer or grow a queue.
    // Windows registers its active sink after the mode-2 discovery snapshot.
    assert_eq!(
        ioctl(
            &independent,
            &event,
            0x8000e3ac,
            &[2, 1, 0, 3],
            &mut report,
            2000
        )?,
        29
    );
    assert_eq!(
        ioctl(
            &independent,
            &event,
            0x8000e3ac,
            &[2, 1, 0, 3],
            &mut report,
            100
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::TimedOut
    );
    let wgi_reader = pad.open_xusb_consumer()?;
    assert_eq!(
        ioctl(
            &wgi_reader,
            &event,
            0x8000e3ac,
            &[2, 1, 0, 3],
            &mut report,
            2000
        )?,
        29
    );
    assert_eq!(u16::from_le_bytes([report[11], report[12]]), 0x1000);
    pad.submit(&pressed)?;
    assert_eq!(
        wait(&mut report, 100).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    pad.neutral()?;
    assert_eq!(wait(&mut report, 2000)?, 29);
    assert_eq!(&report[11..23], &[0; 12]);
    pressed.buttons = BUTTON_EAST;
    pad.submit(&pressed)?;
    assert_eq!(wait(&mut report, 2000)?, 29);
    let start = Instant::now();
    assert_eq!(wait(&mut report, 1500)?, 29);
    assert!(start.elapsed() >= Duration::from_millis(350));
    assert_eq!(&report[11..23], &[0; 12]);
    drop(pad);
    assert!(wait(&mut report, 2000).is_err());
    println!("PASS: public async snapshot, input, independent readers, idle cancellation, bounds, denied producer, release, watchdog and removal.");
    Ok(())
}
