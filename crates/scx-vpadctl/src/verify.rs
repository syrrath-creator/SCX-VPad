//! Explicit opt-in native Windows HID integration tests. No physical controller
//! is opened, no existing virtual controller is modified, and no elevation occurs.
use scx_vpad_client::{device_present, FeedbackReader, GamepadReader, VirtualPad};
use scx_vpad_protocol::{InputState, Profile};
use std::time::{Duration, Instant};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn verify_report(report: &[u8; 18], state: &InputState) -> Result<()> {
    let words = [
        state.left_x as u16,
        state.left_y as u16,
        state.right_x as u16,
        state.right_y as u16,
        state.left_trigger,
        state.right_trigger,
    ];
    if report[0] != 1
        || u32::from_le_bytes(report[1..5].try_into()?) != state.buttons
        || report[5] != state.hat
    {
        return Err("HID button/hat state differs from submitted state".into());
    }
    for (i, word) in words.iter().enumerate() {
        if u16::from_le_bytes(report[6 + i * 2..8 + i * 2].try_into()?) != *word {
            return Err(format!("HID axis {i} differs from submitted state").into());
        }
    }
    Ok(())
}
fn send_read(
    pad: &mut VirtualPad,
    reader: &mut GamepadReader,
    state: &InputState,
) -> Result<Duration> {
    let mut sent = Instant::now();
    let mut report = reader.read_after(2000, || {
        sent = Instant::now();
        pad.submit(state)
    })?;
    // A HID class reader may already have the driver's initial neutral report
    // buffered. Read until this submitted state arrives, never count it as the
    // latency result for the state we just sent.
    for _ in 0..64 {
        if verify_report(&report, state).is_ok() {
            return Ok(sent.elapsed());
        }
        report = reader.read(2000)?;
    }
    Err("submitted HID state not observed within bounded report queue".into())
}
pub fn self_test() -> Result<()> {
    let mut pads = Vec::new();
    let mut readers = Vec::new();
    for _ in 0..4 {
        let pad = VirtualPad::create()?;
        let reader = GamepadReader::open(&pad)?;
        pads.push(pad);
        readers.push(reader);
    }
    println!("PASS: four independently leased controllers created without elevation");
    let mut states = [InputState::NEUTRAL; 4];
    for i in 0..4 {
        states[i].buttons = 1u32 << i;
        states[i].hat = i as u8;
        states[i].left_x = -16000 + i as i16 * 2000;
        states[i].left_y = 12000;
        states[i].right_x = i16::MIN;
        states[i].right_y = i16::MAX;
        states[i].left_trigger = 1234 + i as u16;
        states[i].right_trigger = u16::MAX;
        send_read(&mut pads[i], &mut readers[i], &states[i])?;
    }
    for i in 0..4 {
        verify_report(&readers[i].snapshot()?, &states[i])?;
    }
    println!(
        "PASS: buttons, hats, four signed axes and independent triggers reach all four HID readers"
    );
    states[0] = InputState::NEUTRAL;
    send_read(&mut pads[0], &mut readers[0], &states[0])?;
    for i in 1..4 {
        verify_report(&readers[i].snapshot()?, &states[i])?;
        pads[i].submit(&states[i])?;
    }
    println!("PASS: release on controller 1 leaves controllers 2-4 unchanged");
    std::thread::sleep(Duration::from_millis(900));
    for reader in &readers {
        verify_report(&reader.snapshot()?, &InputState::NEUTRAL)?;
    }
    println!("PASS: stalled producers are neutralized by the driver watchdog");
    let removed = pads[3].instance_id().to_owned();
    readers.pop();
    pads.pop();
    let deadline = Instant::now() + Duration::from_secs(5);
    while device_present(&removed) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
    if device_present(&removed) {
        return Err("released device remained present after 5 seconds".into());
    }
    for pad in &pads {
        if !device_present(pad.instance_id()) {
            return Err("removing one lease removed another device".into());
        }
    }
    println!("PASS: dropping one lease removes only that owned device");
    let mut held = InputState::NEUTRAL;
    held.buttons = 0x80000001;
    send_read(&mut pads[0], &mut readers[0], &held)?;
    send_read(&mut pads[0], &mut readers[0], &InputState::NEUTRAL)?;
    println!("PASS: remaining controller continues delivering press/release after sibling removal");
    println!("SELF-TEST PASS; created test devices will now be removed. No physical input or end-to-end game latency claim.");
    Ok(())
}
pub fn benchmark(count: usize) -> Result<()> {
    let mut pad = VirtualPad::create()?;
    let mut reader = GamepadReader::open(&pad)?;
    let mut state = InputState::NEUTRAL;
    for i in 0..100 {
        state.buttons = if i % 2 == 0 { 1 } else { 0 };
        send_read(&mut pad, &mut reader, &state)?;
    }
    let mut samples = Vec::with_capacity(count);
    for i in 0..count {
        state.buttons = if i % 2 == 0 { 1 } else { 0 };
        samples.push(send_read(&mut pad, &mut reader, &state)?.as_nanos());
    }
    pad.neutral()?;
    samples.sort_unstable();
    println!("Direct SCX submit -> Windows HID consumer completion, same process, {count} samples. Includes user scheduling/HID stack; excludes physical input, game and display. This is not a comparison with other drivers.");
    println!(
        "p50={:.3} us p95={:.3} us p99={:.3} us max={:.3} us",
        samples[count / 2] as f64 / 1000.0,
        samples[(count - 1) * 95 / 100] as f64 / 1000.0,
        samples[(count - 1) * 99 / 100] as f64 / 1000.0,
        samples[count - 1] as f64 / 1000.0
    );
    Ok(())
}

pub fn sony() -> Result<()> {
    for profile in [Profile::DualShock4, Profile::DualSense] {
        let mut pad = VirtualPad::create_profile(profile)?;
        let mut reader = GamepadReader::<64>::open(&pad)?;
        let mut feedback = FeedbackReader::open(&pad)?;
        let state = InputState {
            buttons: 1 | (1 << 5) | (1 << 10),
            hat: 2,
            left_x: i16::MIN,
            left_y: i16::MAX,
            right_x: 0,
            right_y: -32768,
            left_trigger: 65535,
            right_trigger: 21845,
            ..InputState::NEUTRAL
        };
        let check_input = |r: &[u8; 64]| -> bool {
            let (buttons, triggers) = if profile == Profile::DualShock4 {
                (5, 8)
            } else {
                (8, 5)
            };
            r[0] == 1
                && r[1..5] == [0, 255, 128, 0]
                && r[buttons] == 0x22
                && r[buttons + 1] == 0x0e
                && r[buttons + 2] & 3 == 1
                && r[triggers..triggers + 2] == [255, 85]
        };
        let mut report = reader.read_after(2000, || pad.submit(&state))?;
        for _ in 0..64 {
            if check_input(&report) {
                break;
            }
            report = reader.read(2000)?;
        }
        if !check_input(&report) {
            return Err(format!("{profile:?} consumer input mismatch").into());
        }
        println!("PASS {profile:?}: actual Windows HID consumer receives face/shoulder/system buttons, hat, sticks and triggers");
        let (cal, identity, firmware) = if profile == Profile::DualShock4 {
            (2, 0x12, 0xa3)
        } else {
            (5, 9, 0x20)
        };
        let calibration = reader.feature(cal)?;
        if i16::from_le_bytes(calibration[7..9].try_into()?) != 1440
            || i16::from_le_bytes(calibration[9..11].try_into()?) != -1440
        {
            return Err("synthetic calibration feature mismatch".into());
        }
        if reader.feature(identity)?[6] != 2 || reader.feature(firmware)?[1..4] != *b"Oct" {
            return Err("synthetic identity/firmware feature mismatch".into());
        }
        println!("PASS {profile:?}: calibration, device identity and firmware features reach the Windows HID consumer");
        let mut output = if profile == Profile::DualShock4 {
            let mut r = vec![0; 32];
            r[0] = 5;
            r[1] = 3;
            r[4] = 0x55;
            r[5] = 0xaa;
            r[6..9].copy_from_slice(&[17, 34, 51]);
            r
        } else {
            let mut r = vec![0; 63];
            r[0] = 2;
            r[1] = 3;
            r[2] = 4;
            r[3] = 0x55;
            r[4] = 0xaa;
            r[45..48].copy_from_slice(&[17, 34, 51]);
            r
        };
        reader.output(&mut output)?;
        let expected = |f: &scx_vpad_protocol::Feedback| {
            f.left_motor == 0xaaaa && f.right_motor == 0x5555 && f.rgb == [17, 34, 51]
        };
        let mut received = feedback.read(2000)?;
        for _ in 0..32 {
            if expected(&received) {
                break;
            }
            received = feedback.read(2000)?;
        }
        if !expected(&received) || !expected(&pad.feedback_snapshot()?) {
            return Err("Sony rumble/RGB feedback mismatch".into());
        }
        println!("PASS {profile:?}: game-side HID rumble/RGB output reaches event-driven SCX feedback and snapshot");
        pad.neutral()?;
        let snapshot = reader.snapshot()?;
        let b = if profile == Profile::DualShock4 { 5 } else { 8 };
        if snapshot[1..5] != [128; 4]
            || snapshot[b] != 8
            || snapshot[b + 1] != 0
            || snapshot[b + 2] & 3 != 0
        {
            return Err("Sony neutral/release mismatch".into());
        }
        println!("PASS {profile:?}: button/axis release; owned device removed on completion");
    }
    Ok(())
}

pub fn xbox_feedback() -> Result<()> {
    use windows_sys::Win32::UI::Input::XboxController::*;
    let mut pad = VirtualPad::create_profile(Profile::Xbox360)?;
    let mut feedback = FeedbackReader::open(&pad)?;
    let state = InputState {
        buttons: 1 | 4,
        left_x: 12345,
        right_x: -23456,
        right_trigger: 65535,
        ..InputState::NEUTRAL
    };
    pad.submit(&state)?;
    let mut slot = None;
    for i in 0..4 {
        let mut current = XINPUT_STATE::default();
        if unsafe { XInputGetState(i, &mut current) } == 0
            && current.Gamepad.wButtons == 0x5000
            && current.Gamepad.sThumbLX == 12345
            && current.Gamepad.sThumbRX == -23456
            && current.Gamepad.bRightTrigger == 255
        {
            slot = Some(i);
            break;
        }
    }
    let slot = slot.ok_or("own distinctive test state not found through Microsoft XInput1_4")?;
    let vibration = XINPUT_VIBRATION {
        wLeftMotorSpeed: 0x1234,
        wRightMotorSpeed: 0xabcd,
    };
    if unsafe { XInputSetState(slot, &vibration) } != 0 {
        return Err("Microsoft XInputSetState failed".into());
    }
    // Public 16-bit XInput amplitudes travel through 8-bit XUSB output.
    let matches =
        |f: &scx_vpad_protocol::Feedback| f.left_motor == 0x1212 && f.right_motor == 0xabab;
    let mut received = feedback.read(2000)?;
    for _ in 0..32 {
        if matches(&received) {
            break;
        }
        received = feedback.read(2000)?;
    }
    if !matches(&received) || !matches(&pad.feedback_snapshot()?) {
        return Err("XInput rumble feedback mismatch".into());
    }
    println!("PASS Xbox slot {slot}: Microsoft XInputGetState sees the owned input; XInputSetState reaches event-driven SCX feedback and snapshot");
    if unsafe { XInputSetState(slot, &XINPUT_VIBRATION::default()) } != 0 {
        return Err("Microsoft XInput rumble stop failed".into());
    }
    let stopped = feedback.read(2000)?;
    if stopped.left_motor != 0 || stopped.right_motor != 0 {
        return Err("XInput rumble stop mismatch".into());
    }
    pad.neutral()?;
    println!("PASS Xbox: rumble stop propagates; owned device removed on completion");
    Ok(())
}
