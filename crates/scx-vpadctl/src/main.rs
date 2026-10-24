#[cfg(windows)]
mod verify;
#[cfg(windows)]
fn main() {
    if let Err(e) = run() {
        eprintln!("SCX VPad: {e}");
        std::process::exit(1)
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("SCX VPad requires Windows");
    std::process::exit(1)
}

#[cfg(windows)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use scx_vpad_client::{service_status, VirtualPad};
    use scx_vpad_protocol::InputState;
    use std::{
        io::{self, BufRead},
        time::{Duration, Instant},
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("probe-hid-routing") if args.len() == 1 => {
            for (game, zero) in [(false, true), (true, true), (true, false)] {
                let pad = VirtualPad::create()?;
                println!("{}", pad.probe_hid_route(game, zero)?);
            }
        }
        Some("security-check") if args.len() <= 2 => {
            let mut pad = VirtualPad::create_profile(profile(args.get(1))?)?;
            if let Err(error) = pad.neutral() {
                if let Ok(diagnostic) = pad.authorization_diagnostic() {
                    eprintln!("{diagnostic}");
                }
                return Err(error.into());
            }
            println!("PASS: normal installed owner submits an actual neutral input packet.");
            match pad.control_security() {
                Ok(sddl) => println!("Actual producer endpoint ACL: {sddl}"),
                Err(e) => println!(
                    "Actual producer ACL query unavailable: {e}; testing effective access instead."
                ),
            }
            pad.verify_restricted_writer_denied()?;
            println!("PASS: restricted broad-user token cannot obtain a lease or submit input without a capability.");
            pad.verify_low_integrity_writer_denied()?;
            println!("PASS: same-owner low-integrity token cannot obtain a lease or submit input without a capability.");
            if profile(args.get(1))? != scx_vpad_protocol::Profile::Xbox360 {
                pad.verify_bootstrap_unreadable()?;
                pad.verify_capability_rejections()?;
                println!("PASS: protected bootstrap unreadable; zero/wrong/cross-device/retired capabilities, zero-access producer, game-collection bypass, legacy packets and replays rejected.");
            }
            println!("PASS: installed owner can use the producer; restricted broad-user token is denied at handle open or input submission. This verifies user isolation, not isolation between processes running as the same owner.");
        }
        Some("self-test") if args.len() == 1 => verify::self_test()?,
        Some("self-test-generic-classes") if args.len() == 1 => verify::generic_classes()?,
        Some("self-test-sony") if args.len() == 1 => verify::sony()?,
        Some("self-test-sony-multi") if args.len() == 4 => {
            verify::sony_multi(&args[1], &args[2], &args[3])?
        }
        Some("self-test-xbox-feedback") if args.len() == 1 => verify::xbox_feedback()?,
        Some("benchmark-hid") if args.len() <= 2 => {
            verify::benchmark(number(args.get(1), 1000, 100, 100000)? as usize)?
        }
        Some("status") | Some("list") if args.len() == 1 => {
            service_status()?;
            println!("SCX VPad service available. Supported classes depend on the installed driver version; revision .33 adds experimental wheel, flight and arcade HID to generic gamepad, DS4/DS5 and Xbox/XInput. Steam persona is not implemented.");
        }
        Some("create") | Some("demo") if args.len() <= 4 => {
            let seconds = number(args.get(1), 10, 1, 3600)?;
            let count = number(args.get(2), 1, 1, 4)?;
            let profile = profile(args.get(3))?;
            if profile.is_experimental() {
                println!("EXPERIMENTAL {profile:?}: Windows/game compatibility is still under validation.");
            }
            let mut pads = Vec::new();
            for _ in 0..count {
                let pad = VirtualPad::create_profile(profile)?;
                println!("Created {}", pad.instance_id());
                pads.push(pad);
            }
            println!("Holding {count} controller(s) for {seconds} seconds; closing this process removes only these devices.");
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(seconds) {
                for (index, pad) in pads.iter_mut().enumerate() {
                    let mut state = InputState::NEUTRAL;
                    if args[0] == "demo" {
                        // Explicit opt-in demo, bounded values. Do not run while
                        // a game is focused unless these test inputs are wanted.
                        state.buttons = if (start.elapsed().as_millis() / 500 + index as u128)
                            .is_multiple_of(2)
                        {
                            1
                        } else {
                            0
                        };
                        state.left_x = ((start.elapsed().as_secs_f64() * 2.0 + index as f64).sin()
                            * 16000.0) as i16;
                    }
                    pad.submit(&state)?;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            for pad in &mut pads {
                pad.neutral()?;
                if let Ok(counters) = pad.consumer_diagnostics() {
                    println!("I/O counters {:x?}", counters);
                }
            }
            println!("Released all demo devices.");
        }
        Some("stream") if args.len() <= 2 => {
            println!("Read 128 hex digits (one validated 64-byte SCX packet) per line from stdin; EOF removes device. Resend held state within 250 ms. No arbitrary descriptors accepted.");
            let mut pad = VirtualPad::create_profile(profile(args.get(1))?)?;
            let mut line = String::with_capacity(132);
            let mut input = io::stdin().lock();
            loop {
                line.clear();
                // take() caps attacker-controlled stdin allocation.
                use std::io::Read;
                if (&mut input).take(132).read_line(&mut line)? == 0 {
                    break;
                }
                let bytes = decode_packet(line.trim())?;
                pad.submit(&InputState::decode(&bytes)?)?;
            }
            pad.neutral()?;
        }
        Some("benchmark") if args.len() <= 3 => {
            let count = number(args.get(1), 10000, 100, 1_000_000)? as usize;
            let mut pad = VirtualPad::create_profile(profile(args.get(2))?)?;
            let neutral = InputState::NEUTRAL;
            for _ in 0..200 {
                pad.submit(&neutral)?;
            }
            let mut samples = Vec::with_capacity(count);
            let total = Instant::now();
            for _ in 0..count {
                let start = Instant::now();
                pad.submit(&neutral)?;
                samples.push(start.elapsed().as_nanos());
            }
            let elapsed = total.elapsed();
            samples.sort_unstable();
            println!("Neutral-report API completion only; not hardware/controller-to-game latency. One device, direct synchronous driver submission, {count} samples.");
            println!(
                "p50={:.3} us p95={:.3} us p99={:.3} us max={:.3} us total={:.3} s",
                samples[count / 2] as f64 / 1000.0,
                samples[(count - 1) * 95 / 100] as f64 / 1000.0,
                samples[(count - 1) * 99 / 100] as f64 / 1000.0,
                samples[count - 1] as f64 / 1000.0,
                elapsed.as_secs_f64()
            );
        }
        _ => {
            println!("scx-vpadctl status\nscx-vpadctl create [seconds 1..3600] [controllers 1..4] [generic|wheel|flight|arcade|ds4|ds5|xbox]\nscx-vpadctl demo [seconds] [controllers] [generic|wheel|flight|arcade|ds4|ds5|xbox]\nscx-vpadctl stream [generic|wheel|flight|arcade|ds4|ds5|xbox]\nscx-vpadctl self-test\nscx-vpadctl self-test-generic-classes\nscx-vpadctl self-test-sony\nscx-vpadctl self-test-sony-multi <wgi_probe.exe> <gameinput3_probe.exe> <GameInputRedist.dll>\nscx-vpadctl self-test-xbox-feedback\nscx-vpadctl security-check [generic|wheel|flight|arcade|ds4|ds5|xbox]\nscx-vpadctl benchmark [samples 100..1000000] [generic|wheel|flight|arcade|ds4|ds5|xbox]\nscx-vpadctl benchmark-hid [samples 100..100000]\n\nDS4/DS5 and Xbox are experimental. The service and locally signed UMDF2 driver must be installed once by an administrator. Normal commands never elevate. Demo, self-tests and benchmark-hid generate actual button/stick input; close games first.");
        }
    }
    Ok(())
}
#[cfg(any(windows, test))]
fn profile(
    value: Option<&String>,
) -> Result<scx_vpad_protocol::Profile, Box<dyn std::error::Error>> {
    use scx_vpad_protocol::Profile;
    match value.map(String::as_str) {
        None | Some("generic") => Ok(Profile::GenericHid),
        Some("ds4") => Ok(Profile::DualShock4),
        Some("ds5") => Ok(Profile::DualSense),
        Some("xbox") => Ok(Profile::Xbox360),
        Some("wheel") => Ok(Profile::Wheel),
        Some("flight") => Ok(Profile::FlightStick),
        Some("arcade") => Ok(Profile::ArcadeStick),
        _ => Err("profile must be generic, wheel, flight, arcade, ds4, ds5 or xbox".into()),
    }
}
#[cfg(any(windows, test))]
fn number(
    value: Option<&String>,
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64, Box<dyn std::error::Error>> {
    let n = match value {
        Some(v) => v.parse()?,
        None => default,
    };
    if !(min..=max).contains(&n) {
        return Err(format!("value must be {min}..{max}").into());
    }
    Ok(n)
}
#[cfg(any(windows, test))]
fn decode_packet(line: &str) -> Result<[u8; 64], Box<dyn std::error::Error>> {
    if line.len() != 128 || !line.is_ascii() {
        return Err("expected exactly 128 ASCII hex digits".into());
    }
    let mut bytes = [0u8; 64];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&line[i * 2..i * 2 + 2], 16)?;
    }
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_selection_only_accepts_fixed_implemented_personas() {
        use scx_vpad_protocol::Profile;
        for (name, expected) in [
            ("generic", Profile::GenericHid),
            ("wheel", Profile::Wheel),
            ("flight", Profile::FlightStick),
            ("arcade", Profile::ArcadeStick),
            ("ds4", Profile::DualShock4),
            ("ds5", Profile::DualSense),
            ("xbox", Profile::Xbox360),
        ] {
            assert_eq!(profile(Some(&name.to_owned())).unwrap(), expected);
        }
        for name in ["steam", "5", "keyboard", "mouse", "../wheel", "wheel\0", ""] {
            assert!(profile(Some(&name.to_owned())).is_err());
        }
    }
    #[test]
    fn stream_has_strict_bounds() {
        assert_eq!(decode_packet(&"00".repeat(64)).unwrap(), [0; 64]);
        assert!(decode_packet(&"00".repeat(65)).is_err());
        assert!(decode_packet(&"é".repeat(64)).is_err());
        assert!(decode_packet(&"zz".repeat(64)).is_err());
    }
    #[test]
    fn command_limits() {
        assert_eq!(number(None, 10, 1, 100).unwrap(), 10);
        assert!(number(Some(&"0".into()), 10, 1, 100).is_err());
        assert!(number(Some(&"101".into()), 10, 1, 100).is_err());
    }
}
