//! Opt-in game compatibility diagnostic. Creates one owned Sony device.
//! Reads only F7..F10, never text, mouse events, or game memory. The foreground
//! image name gates input to the explicitly selected game. All emitted inputs
//! expire after 150 ms.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameTarget {
    Tekken,
    Gears,
}
#[cfg(any(windows, test))]
impl GameTarget {
    fn from_arguments(arguments: &[String]) -> Option<Self> {
        match arguments {
            [flag] if flag == "--tekken-test" => Some(Self::Tekken),
            [flag] if flag == "--gears-test" => Some(Self::Gears),
            _ => None,
        }
    }

    fn matches_image_path(self, path: &str) -> bool {
        let expected = match self {
            Self::Tekken => "Polaris-Win64-Shipping.exe",
            Self::Gears => "GoWEDay.exe",
        };
        path.rsplit(['\\', '/'])
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case(expected))
    }

    fn name(self) -> &'static str {
        match self {
            Self::Tekken => "Tekken",
            Self::Gears => "Gears",
        }
    }
}
#[cfg(windows)]
fn run() -> std::io::Result<()> {
    use scx_vpad_client::VirtualPad;
    use scx_vpad_protocol::{InputState, Profile, BUTTON_SOUTH};
    use std::{
        ffi::c_void,
        io::{self, Write},
        thread,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };
    #[link(name = "user32")]
    extern "system" {
        fn GetAsyncKeyState(key: i32) -> i16;
        fn GetForegroundWindow() -> *mut c_void;
        fn GetWindowThreadProcessId(window: *mut c_void, process: *mut u32) -> u32;
    }
    fn game_focused(game: GameTarget) -> bool {
        unsafe {
            let mut id = 0;
            GetWindowThreadProcessId(GetForegroundWindow(), &mut id);
            if id == 0 {
                return false;
            }
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, id);
            if process.is_null() {
                return false;
            }
            let mut path = [0u16; 32768];
            let mut count = path.len() as u32;
            let ok = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut count);
            CloseHandle(process);
            ok != 0 && game.matches_image_path(&String::from_utf16_lossy(&path[..count as usize]))
        }
    }
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.len() == 1 && arguments[0] == "--protocol-fixture" {
        // Independent neutral source for read-only protocol comparisons. No
        // keyboard polling, physical input, mouse output or game interaction.
        let mut pad = VirtualPad::create_profile(Profile::DualSense)?;
        pad.neutral()?;
        println!("PROTOCOL_READY: one neutral SCX DualSense; expires in 20 seconds.");
        io::stdout().flush()?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(20) {
            pad.submit(&InputState {
                timestamp_us: start.elapsed().as_micros() as u64,
                ..InputState::NEUTRAL
            })?;
            thread::sleep(Duration::from_millis(10));
        }
        pad.neutral()?;
        println!("PROTOCOL_DONE: neutral fixture finished.");
        return Ok(());
    }
    let Some(game) = GameTarget::from_arguments(&arguments) else {
        println!("Explicit diagnostic only: sony_game_probe --tekken-test | --gears-test | --protocol-fixture");
        return Ok(());
    };
    let mut pad = VirtualPad::create_profile(Profile::DualSense)?;
    pad.neutral()?;
    println!(
        "TESTBEREIT: One SCX DualSense, no physical source, no mouse output. Start {} now.",
        game.name()
    );
    println!(
        "In {}: F7=one D-pad UP, F8=one D-pad DOWN, F9=one CROSS. F10=stop.",
        game.name()
    );
    println!(
        "Only test the offline menu. Device expires after 10 minutes. Close this console to stop."
    );
    io::stdout().flush()?;
    let start = Instant::now();
    let mut previous = [true; 4]; // Require release before the first activation.
    let mut was_enabled = false;
    let mut pulse_end = Duration::ZERO;
    let mut pulse = 0usize;
    let mut count = 0u64;
    while start.elapsed() < Duration::from_secs(600) {
        let elapsed = start.elapsed();
        // Do not reuse a cached foreground decision for a new pulse/report.
        let enabled = game_focused(game);
        if enabled != was_enabled {
            println!(
                "Focus gate: {} {} at {} ms",
                game.name(),
                if enabled {
                    "foreground; virtual buttons enabled"
                } else {
                    "not foreground; neutral only"
                },
                elapsed.as_millis()
            );
            io::stdout().flush()?;
        }
        let keys = std::array::from_fn::<_, 4, _>(|index| unsafe {
            GetAsyncKeyState(0x76 + index as i32) < 0
        });
        if !enabled && (0..3).any(|index| keys[index] && !previous[index]) {
            println!(
                "Ignored test key: {} is not the foreground window.",
                game.name()
            );
            io::stdout().flush()?;
        }
        if keys[3] && !previous[3] {
            break;
        }
        // On focus entry all hotkeys must first be released. On focus loss any
        // pending action is immediately neutralized, never replayed on return.
        if !enabled || !was_enabled {
            pulse_end = Duration::ZERO;
        }
        if enabled && was_enabled {
            for index in 0..3 {
                if keys[index] && !previous[index] {
                    pulse = index;
                    pulse_end = elapsed + Duration::from_millis(150);
                    count += 1;
                    println!(
                        "Action {}: {} at {} ms",
                        count,
                        ["UP", "DOWN", "CROSS"][index],
                        elapsed.as_millis()
                    );
                    io::stdout().flush()?;
                }
            }
        }
        previous = keys;
        was_enabled = enabled;
        let mut state = InputState {
            timestamp_us: elapsed.as_micros() as u64,
            ..InputState::NEUTRAL
        };
        if enabled && elapsed < pulse_end {
            match pulse {
                0 => state.hat = 0,
                1 => state.hat = 4,
                _ => state.buttons = BUTTON_SOUTH,
            }
        }
        pad.submit(&state)?;
        thread::sleep(Duration::from_millis(10));
    }
    pad.neutral()?;
    println!(
        "Finished; {} requested diagnostic pulses. Owned device removed on exit.",
        count
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::GameTarget;

    #[test]
    fn game_selection_requires_exactly_one_explicit_option() {
        for (flag, game) in [
            ("--tekken-test", GameTarget::Tekken),
            ("--gears-test", GameTarget::Gears),
        ] {
            assert_eq!(GameTarget::from_arguments(&[flag.into()]), Some(game));
            assert_eq!(
                GameTarget::from_arguments(&[flag.into(), "--protocol-fixture".into()]),
                None
            );
        }
        for args in [
            vec![],
            vec!["--gears".into()],
            vec!["--protocol-fixture".into()],
        ] {
            assert_eq!(GameTarget::from_arguments(&args), None);
        }
    }

    #[test]
    fn foreground_gate_rejects_other_games_and_partial_names() {
        let tekken = r"D:\Steam\TEKKEN 8\Polaris-Win64-Shipping.exe";
        let gears = r"C:\XboxGames\Gears of War E-Day\Content\GoWEDay.exe";
        assert!(GameTarget::Tekken.matches_image_path(tekken));
        assert!(GameTarget::Gears.matches_image_path(gears));
        assert!(GameTarget::Gears.matches_image_path(&gears.to_ascii_uppercase()));
        assert!(!GameTarget::Tekken.matches_image_path(gears));
        assert!(!GameTarget::Gears.matches_image_path(tekken));
        for path in [
            "GoWEDay.exe.backup",
            "NotGoWEDay.exe",
            r"C:\GoWEDay.exe\launcher.exe",
            "steamwebhelper.exe",
            "",
        ] {
            assert!(!GameTarget::Gears.matches_image_path(path));
        }
        assert_eq!(GameTarget::Tekken.name(), "Tekken");
        assert_eq!(GameTarget::Gears.name(), "Gears");
    }
}
fn main() {
    #[cfg(windows)]
    if let Err(error) = run() {
        eprintln!("Diagnostic failed: {error}");
        std::process::exit(1);
    }
}
