//! Opt-in driver-to-Windows regression probe; creates/removes only its own pad.
#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use scx_vpad_client::{GamepadReader, VirtualPad};
    use scx_vpad_protocol::*;
    use std::{
        io, thread,
        time::{Duration, Instant},
    };
    let args: Vec<_> = std::env::args().collect();
    if !args.iter().any(|s| s == "--run-hardware-test") {
        println!("Opt-in: --run-hardware-test ds4|ds5 [--observe-only]");
        return Ok(());
    }
    let profile = if args.iter().any(|s| s == "ds4") {
        Profile::DualShock4
    } else {
        Profile::DualSense
    };
    let observe = args.iter().any(|s| s == "--observe-only");
    let mut pad = VirtualPad::create_profile(profile)?;
    let reader = GamepadReader::<64>::open(&pad)?;
    println!("PROFILE {profile:?} AXIS_ORDER X Y Z Rx Ry Rz Hat");
    let mut cases = vec![(
        "neutral",
        InputState::NEUTRAL,
        [128, 128, 128, 0, 0, 128, 8],
        vec![],
    )];
    for (name, button, usage) in [
        ("square", BUTTON_WEST, 1),
        ("cross", BUTTON_SOUTH, 2),
        ("circle", BUTTON_EAST, 3),
        ("triangle", BUTTON_NORTH, 4),
        ("L1", BUTTON_LEFT_SHOULDER, 5),
        ("R1", BUTTON_RIGHT_SHOULDER, 6),
        ("share", BUTTON_SELECT, 9),
        ("options", BUTTON_START, 10),
        ("L3", BUTTON_LEFT_STICK, 11),
        ("R3", BUTTON_RIGHT_STICK, 12),
        ("guide", BUTTON_GUIDE, 13),
        ("touch-click", BUTTON_TOUCHPAD, 14),
    ] {
        cases.push((
            name,
            InputState {
                buttons: button,
                ..InputState::NEUTRAL
            },
            [128, 128, 128, 0, 0, 128, 8],
            vec![usage],
        ));
    }
    cases.push((
        "L2",
        InputState {
            left_trigger: 65535,
            ..InputState::NEUTRAL
        },
        [128, 128, 128, 255, 0, 128, 8],
        vec![7],
    ));
    cases.push((
        "R2",
        InputState {
            right_trigger: 65535,
            ..InputState::NEUTRAL
        },
        [128, 128, 128, 0, 255, 128, 8],
        vec![8],
    ));
    cases.push((
        "right-stick",
        InputState {
            right_x: i16::MAX,
            right_y: i16::MIN,
            ..InputState::NEUTRAL
        },
        [128, 128, 255, 0, 0, 0, 8],
        vec![],
    ));
    for (name, state, axes) in [
        (
            "left-stick-up-right",
            InputState {
                left_x: i16::MAX,
                left_y: i16::MIN,
                ..InputState::NEUTRAL
            },
            [255, 0, 128, 0, 0, 128, 8],
        ),
        (
            "left-stick-down-left",
            InputState {
                left_x: i16::MIN,
                left_y: i16::MAX,
                ..InputState::NEUTRAL
            },
            [0, 255, 128, 0, 0, 128, 8],
        ),
        (
            "right-stick-down-left",
            InputState {
                right_x: i16::MIN,
                right_y: i16::MAX,
                ..InputState::NEUTRAL
            },
            [128, 128, 0, 0, 0, 255, 8],
        ),
    ] {
        cases.push((name, state, axes, vec![]));
    }
    for hat in 0..8 {
        let mut axes = [128, 128, 128, 0, 0, 128, 8];
        axes[6] = u32::from(hat);
        cases.push((
            "dpad",
            InputState {
                hat,
                ..InputState::NEUTRAL
            },
            axes,
            vec![],
        ));
    }
    cases.push((
        "combined",
        InputState {
            buttons: BUTTON_SOUTH | BUTTON_EAST,
            left_trigger: 65535,
            right_trigger: 65535,
            right_x: i16::MAX,
            ..InputState::NEUTRAL
        },
        [128, 128, 255, 255, 255, 128, 8],
        vec![2, 3, 7, 8],
    ));
    cases.push((
        "released",
        InputState::NEUTRAL,
        [128, 128, 128, 0, 0, 128, 8],
        vec![],
    ));
    let mut failures = 0;
    for (name, state, expected_axes, expected_buttons) in cases {
        println!("BEGIN {name}");
        let start = Instant::now();
        // Hold with keepalive for the independent WinRT observer; no busy loop.
        while start.elapsed() < Duration::from_millis(if name == "neutral" { 2500 } else { 1000 }) {
            pad.submit(&state)?;
            thread::sleep(Duration::from_millis(20));
        }
        let report = reader.snapshot()?;
        let (axes, buttons) = reader.windows_controls(&report)?;
        let pass = axes == expected_axes && buttons == expected_buttons;
        if !pass {
            failures += 1;
        }
        println!(
            "{name}: axes={axes:?} buttons={buttons:?} {}",
            if pass { "PASS" } else { "FAIL" }
        );
    }
    pad.neutral()?;
    println!("RESULT failures={failures}");
    if failures > 0 && !observe {
        return Err(io::Error::other(
            "Windows interpreted the controller controls incorrectly",
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This diagnostic requires Windows and an installed SCX VPad service.");
}
