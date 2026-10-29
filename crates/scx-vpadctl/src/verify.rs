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

/// Exercises the four generic classes together through Windows' HID parser,
/// including class/axis usages. This must be run explicitly after installing .33.
pub fn generic_classes() -> Result<()> {
    let profiles = [
        Profile::GenericHid,
        Profile::Wheel,
        Profile::FlightStick,
        Profile::ArcadeStick,
    ];
    let mut pads = Vec::new();
    let mut readers = Vec::new();
    let mut states = [InputState::NEUTRAL; 4];
    for (index, profile) in profiles.into_iter().enumerate() {
        let mut pad = VirtualPad::create_profile(profile)?;
        let mut reader = GamepadReader::open(&pad)?;
        let usage = if matches!(profile, Profile::Wheel | Profile::FlightStick) {
            4
        } else {
            5
        };
        if reader.collection_usage()? != (1, usage) {
            return Err(format!("{profile:?}: wrong Windows HID collection class").into());
        }
        if FeedbackReader::open(&pad).is_ok() || pad.feedback_snapshot().is_ok() {
            return Err(format!("{profile:?}: input-only class advertised output feedback").into());
        }
        let state = &mut states[index];
        state.buttons = (1u32 << index) | 0x80000000;
        state.hat = index as u8;
        state.left_x = -12345;
        state.left_y = 23456;
        state.right_x = i16::MIN;
        state.right_y = i16::MAX;
        state.left_trigger = 1234;
        state.right_trigger = u16::MAX;
        send_read(&mut pad, &mut reader, state)?;
        let report = reader.snapshot()?;
        verify_report(&report, state)?;
        let (axes, buttons) = reader.windows_controls(&report)?;
        let expected = [
            state.left_x as u16 as u32,
            state.left_y as u16 as u32,
            state.right_x as u16 as u32,
            state.right_y as u16 as u32,
            state.left_trigger as u32,
            state.right_trigger as u32,
            state.hat as u32,
        ];
        if axes != expected || buttons != vec![index as u16 + 1, 32] {
            return Err(format!(
                "{profile:?}: Windows semantic axes/buttons mismatch: {axes:?}/{buttons:?}"
            )
            .into());
        }
        println!("PASS {profile:?}: correct HID class, 32 buttons, hat and all six semantic axes; no invented force feedback");
        pads.push(pad);
        readers.push(reader);
    }
    for i in 0..4 {
        // Refresh held states before checking isolation; the watchdog deliberately
        // neutralizes a stalled producer while the next device is being created.
        // A duplicate held state refreshes only the watchdog, not the transition
        // queue. Release first so the blocking consumer has a real edge to read.
        pads[i].neutral()?;
        send_read(&mut pads[i], &mut readers[i], &states[i])?;
    }
    send_read(&mut pads[0], &mut readers[0], &InputState::NEUTRAL)?;
    for i in 1..4 {
        verify_report(&readers[i].snapshot()?, &states[i])?;
    }
    std::thread::sleep(Duration::from_millis(900));
    for reader in &readers {
        verify_report(&reader.snapshot()?, &InputState::NEUTRAL)?;
    }
    println!("PASS: four distinct generic classes retain isolated input state and all neutralize on a stalled producer");
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
        "p50={:.3} us p95={:.3} us p99={:.3} us p99.9={:.3} us max={:.3} us",
        samples[count / 2] as f64 / 1000.0,
        samples[(count - 1) * 95 / 100] as f64 / 1000.0,
        samples[(count - 1) * 99 / 100] as f64 / 1000.0,
        samples[(count - 1) * 999 / 1000] as f64 / 1000.0,
        samples[count - 1] as f64 / 1000.0
    );
    Ok(())
}

/// Check the complete synthetic calibration contract independently of its
/// encoder. Errors include the observed field, so stale test expectations are
/// distinguishable from bad HID responses.
fn verify_sony_calibration(profile: Profile, report: &[u8]) -> Result<()> {
    let (id, length) = match profile {
        Profile::DualShock4 => (2, 37),
        Profile::DualSense => (5, 41),
        _ => return Err("Sony calibration requested for a non-Sony profile".into()),
    };
    if report.len() < length || report[0] != id {
        return Err(format!(
            "{profile:?} calibration report header/length mismatch: id {:?}, length {}, expected id {id}, at least {length}",
            report.first(), report.len()
        ).into());
    }
    let field = |offset: usize, expected: i16| -> Result<()> {
        let actual = i16::from_le_bytes([report[offset], report[offset + 1]]);
        if actual != expected {
            return Err(format!(
                "{profile:?} calibration offset {offset}: got {actual}, expected {expected} (driver .31 reference)"
            ).into());
        }
        Ok(())
    };
    for offset in [1, 3, 5] {
        field(offset, 0)?;
    }
    for offset in [7, 11, 15] {
        field(offset, 8000)?;
        field(offset + 2, -8000)?;
    }
    field(19, 500)?;
    field(21, 500)?;
    for offset in [23, 27, 31] {
        field(offset, 8192)?;
        field(offset + 2, -8192)?;
    }
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
        if profile == Profile::DualSense {
            let previous = u32::from_le_bytes(report[12..16].try_into()?);
            let timed = InputState {
                timestamp_us: 123_456,
                ..state
            };
            let mut fresh = reader.read_after(2000, || pad.submit(&timed))?;
            for _ in 0..64 {
                if u32::from_le_bytes(fresh[12..16].try_into()?) != previous {
                    break;
                }
                fresh = reader.read(2000)?;
            }
            let sequence = u32::from_le_bytes(fresh[12..16].try_into()?);
            if !check_input(&fresh)
                || sequence != previous.wrapping_add(1)
                || fresh[7] != sequence as u8
                || u32::from_le_bytes(fresh[28..32].try_into()?) != 370_368
                || u32::from_le_bytes(fresh[49..53].try_into()?) != 370_368
                || fresh[54] != 0x08
                || reader.snapshot()? != fresh
            {
                return Err(
                    "DualSense USB sequence/time/status differs between stream and snapshot".into(),
                );
            }
            println!("PASS DualSense: fresh 32-bit sequence, USB connection flag and both timers reach ReadFile and GetInputReport");
        }
        let (cal, identity, firmware) = if profile == Profile::DualShock4 {
            (2, 0x12, 0xa3)
        } else {
            (5, 9, 0x20)
        };
        let calibration = reader.feature(cal)?;
        verify_sony_calibration(profile, &calibration)?;

        let fw = reader.feature(firmware)?;
        if reader.feature(identity)?[6] != 2
            || (profile == Profile::DualSense
                && (fw[20..24] != [3, 0, 4, 0]
                    || fw[44..46] != [0x30, 6]
                    || reader.feature(0x22)?[0] != 0x22))
            || (profile == Profile::DualShock4 && fw[17..25] != *b"07:01:12")
        {
            return Err("Sony identity/firmware compatibility response mismatch".into());
        }
        println!("PASS {profile:?}: calibration, device identity and firmware features reach the Windows HID consumer");
        let input_before = reader.snapshot()?;
        let feedback_before = pad.feedback_snapshot()?;
        for _ in 0..3 {
            reader.configure_sony_usb()?;
        }
        if reader.snapshot()? != input_before
            || pad.feedback_snapshot()?.encode() != feedback_before.encode()
            || reader.feature(identity)?[6] != 2
        {
            return Err(
                "Sony configuration unexpectedly changed input, feedback or identity".into(),
            );
        }
        println!("PASS {profile:?}: native-game SetFeature initialization succeeds repeatedly without changing input, feedback or identity");
        let mut output = if profile == Profile::DualShock4 {
            let mut r = vec![0; 32];
            r[0] = 5;
            r[1] = 0xff; // Supported rumble/LED plus ignored hardware controls.
            r[2] = 0xff;
            r[4] = 0x55;
            r[5] = 0xaa;
            r[6..9].copy_from_slice(&[17, 34, 51]);
            r
        } else {
            let mut r = vec![0; 48];
            r[0] = 2;
            r[1] = 0x0d; // Gears-style mixed output, including trigger flags.
            r[2] = 0x17;
            r[3] = 0x55;
            r[4] = 0xaa;
            r[45..48].copy_from_slice(&[17, 34, 51]);
            r
        };
        reader.output(&mut output).map_err(|e| {
            format!(
                "{profile:?} output failed: {e}; native diagnostics {:?}",
                pad.consumer_diagnostics()
            )
        })?;
        let expected = |f: &scx_vpad_protocol::Feedback| {
            f.left_motor == 0xaaaa && f.right_motor == 0x5555 && f.rgb == [17, 34, 51]
        };
        let mut received = feedback
            .read(2000)
            .map_err(|e| format!("Sony feedback wait: {e}"))?;
        for _ in 0..32 {
            if expected(&received) {
                break;
            }
            received = feedback.read(2000)?;
        }
        if !expected(&received)
            || !expected(
                &pad.feedback_snapshot()
                    .map_err(|e| format!("Sony feedback snapshot: {e}"))?,
            )
        {
            return Err("Sony rumble/RGB feedback mismatch".into());
        }
        println!("PASS {profile:?}: game-side HID rumble/RGB output reaches event-driven SCX feedback and snapshot");
        // No output change must block and cancel cleanly; the same reader then
        // has to receive a new stop event rather than replaying stale rumble.
        match feedback.read(40) {
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => (),
            other => return Err(format!("Feedback idle wait did not time out: {other:?}").into()),
        }
        let (right, left) = if profile == Profile::DualShock4 {
            (4, 5)
        } else {
            (3, 4)
        };
        output[right] = 0;
        output[left] = 0;
        reader.output(&mut output)?;
        let stopped = feedback.read(2000)?;
        if stopped.left_motor != 0 || stopped.right_motor != 0 {
            return Err("Feedback stop after cancelled wait mismatch".into());
        }
        println!(
            "PASS {profile:?}: idle feedback wait cancels; subsequent motor stop is delivered"
        );
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

/// Explicit offline-game diagnostic. The independent WGI/GameInput consumers
/// receive distinctive states from four owned fixtures, never physical pads.
pub fn sony_multi(wgi_probe: &str, gi_probe: &str, runtime: &str) -> Result<()> {
    struct Fixture {
        reader: GamepadReader<64>,
        pad: VirtualPad,
        profile: Profile,
        state: InputState,
    }
    fn check(f: &Fixture) -> Result<()> {
        let r = f.reader.snapshot()?;
        let face = if f.profile == Profile::DualShock4 {
            5
        } else {
            8
        };
        let trigger = if f.profile == Profile::DualShock4 {
            8
        } else {
            5
        };
        let face_bits = [0x20, 0x40, 0x10, 0x80];
        let marker = f.state.buttons.trailing_zeros() as usize;
        if r[0] != 1
            || r[face] != 8 | face_bits[marker]
            || r[1] != (32 + marker * 48) as u8
            || r[trigger] != ((marker + 1) * 51) as u8
        {
            return Err("parallel Sony HID input crossed devices or changed unexpectedly".into());
        }
        Ok(())
    }
    fn observe(fixtures: &mut [Fixture], exe: &str, args: &[&str]) -> Result<()> {
        struct OwnedChild(std::process::Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = OwnedChild(std::process::Command::new(exe).args(args).spawn()?);
        let started = Instant::now();
        loop {
            for f in fixtures.iter_mut() {
                f.pad.submit(&f.state)?;
            }
            if let Some(status) = child.0.try_wait()? {
                if !status.success() {
                    return Err(format!("independent consumer failed: {exe}: {status}").into());
                }
                break;
            }
            if started.elapsed() > Duration::from_secs(10) {
                return Err("independent consumer timed out".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        for f in fixtures {
            check(f)?;
        }
        Ok(())
    }
    for layout in ["ds5", "ds4", "mixed"] {
        let mut fixtures = Vec::new();
        let mut instances = std::collections::HashSet::new();
        let mut sony_ids = std::collections::HashSet::new();
        for slot in 0..4 {
            let profile = if layout == "ds4" || (layout == "mixed" && slot % 2 == 1) {
                Profile::DualShock4
            } else {
                Profile::DualSense
            };
            let pad = VirtualPad::create_profile(profile)?;
            if !instances.insert(pad.instance_id().to_owned()) {
                return Err("duplicate PnP instance".into());
            }
            let reader = GamepadReader::<64>::open(&pad)?;
            let id = reader.feature(if profile == Profile::DualShock4 {
                0x12
            } else {
                9
            })?;
            if !sony_ids.insert(id[1..7].to_vec()) {
                return Err("duplicate Sony hardware identity".into());
            }
            fixtures.push(Fixture {
                reader,
                pad,
                profile,
                state: InputState {
                    buttons: 1 << slot,
                    left_x: -24576 + slot * 12288,
                    left_trigger: (slot as u16 + 1) * 13107,
                    ..InputState::NEUTRAL
                },
            });
        }
        for f in &mut fixtures {
            f.pad.submit(&f.state)?;
        }
        for f in &fixtures {
            check(f)?;
        }
        println!("PASS {layout}: four unique PnP/Sony identities and independent HID states");
        observe(&mut fixtures, wgi_probe, &["5", "--multi-sony", "4"])?;
        observe(
            &mut fixtures,
            gi_probe,
            &[runtime, "2", "--expect-scx-sony", "4"],
        )?;
        // Write only to owned fixtures; no physical feedback sink is attached.
        for (i, f) in fixtures.iter_mut().enumerate() {
            let mut output = vec![
                0;
                if f.profile == Profile::DualShock4 {
                    32
                } else {
                    48
                }
            ];
            if f.profile == Profile::DualShock4 {
                output[0] = 5;
                output[1] = 1;
                output[4] = 21 + i as u8;
                output[5] = 41 + i as u8;
            } else {
                output[0] = 2;
                output[1] = 3;
                output[3] = 21 + i as u8;
                output[4] = 41 + i as u8;
            }
            f.reader.output(&mut output)?;
        }
        for (i, f) in fixtures.iter_mut().enumerate() {
            let feedback = f.pad.feedback_snapshot()?;
            if feedback.left_motor != (41 + i as u16) * 257
                || feedback.right_motor != (21 + i as u16) * 257
            {
                return Err("parallel Sony feedback crossed devices".into());
            }
        }
        println!("PASS {layout}: per-controller rumble feedback remains isolated");
        let removed = fixtures[3].pad.instance_id().to_owned();
        fixtures.pop();
        let deadline = Instant::now() + Duration::from_secs(5);
        while device_present(&removed) && Instant::now() < deadline {
            for f in &mut fixtures {
                f.pad.submit(&f.state)?;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if device_present(&removed) {
            return Err("removed Sony fixture remains present".into());
        }
        observe(&mut fixtures, wgi_probe, &["5", "--multi-sony", "3"])?;
        observe(
            &mut fixtures,
            gi_probe,
            &[runtime, "2", "--expect-scx-sony", "3"],
        )?;
        println!(
            "PASS {layout}: removal affects only one pad; three Windows consumers stay independent"
        );
        for f in &mut fixtures {
            f.pad.neutral()?;
        }
        drop(fixtures);
        let deadline = Instant::now() + Duration::from_secs(5);
        while instances.iter().any(|id| device_present(id)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        if instances.iter().any(|id| device_present(id)) {
            return Err("Sony fixture leaked after release".into());
        }
    }
    println!("SONY MULTI PASS: ds5/ds4/mixed; four independent HID/WGI/GameInput devices, isolated feedback and sibling removal.");
    Ok(())
}

#[cfg(test)]
mod calibration_tests {
    use super::*;

    #[test]
    fn native_verifier_accepts_current_driver_features_for_both_sony_profiles() {
        for (profile, id) in [(Profile::DualShock4, 2), (Profile::DualSense, 5)] {
            let (report, length) = scx_vpad_core::profiles::feature(profile, id, 1).unwrap();
            verify_sony_calibration(profile, &report[..length]).unwrap();
            verify_sony_calibration(profile, &report).unwrap();
        }
    }

    #[test]
    fn native_verifier_rejects_bad_fields_truncation_and_wrong_id() {
        for (profile, id) in [(Profile::DualShock4, 2), (Profile::DualSense, 5)] {
            let (report, length) = scx_vpad_core::profiles::feature(profile, id, 1).unwrap();
            for offset in (1..=33).step_by(2) {
                let mut corrupted = report;
                corrupted[offset] ^= 1;
                let error = verify_sony_calibration(profile, &corrupted)
                    .unwrap_err()
                    .to_string();
                assert!(error.contains(&format!("offset {offset}:")), "{error}");
            }
            assert!(verify_sony_calibration(profile, &report[..length - 1]).is_err());
            assert!(verify_sony_calibration(profile, &[]).is_err());
            let mut wrong_id = report;
            wrong_id[0] = 0xff;
            assert!(verify_sony_calibration(profile, &wrong_id).is_err());
        }
    }

    #[test]
    fn native_verifier_reports_old_reference_instead_of_generic_mismatch() {
        let (mut report, _) = scx_vpad_core::profiles::feature(Profile::DualSense, 5, 1).unwrap();
        report[7..9].copy_from_slice(&1440i16.to_le_bytes());
        let error = verify_sony_calibration(Profile::DualSense, &report)
            .unwrap_err()
            .to_string();
        assert!(error.contains("got 1440, expected 8000"), "{error}");
    }
}
