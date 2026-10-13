//! Creates only owned neutral test devices; logs the native WGI feedback path.
#[cfg(not(windows))]
fn main() {}
#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use scx_vpad_client::VirtualPad;
    use scx_vpad_protocol::{InputState, Profile};
    use std::time::{Duration, Instant};
    let args: Vec<_> = std::env::args().collect();
    if !args.iter().any(|a| a == "--run-hardware-test") {
        println!("Opt-in: --run-hardware-test. Close controller apps and games.");
        return Ok(());
    }
    let seconds = args
        .windows(2)
        .find(|a| a[0] == "--seconds")
        .map(|a| a[1].parse::<u64>().unwrap_or(0))
        .unwrap_or(130);
    if !(1..=300).contains(&seconds) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "seconds must be 1..300",
        ));
    }
    let mut pads = Vec::new();
    for profile in [Profile::Xbox360, Profile::DualShock4, Profile::DualSense] {
        let pad = VirtualPad::create_profile(profile)?;
        println!("Created {profile:?}: {}", pad.instance_id());
        pads.push((profile, pad, u32::MAX));
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(seconds) {
        for (profile, pad, sequence) in &mut pads {
            pad.submit(&InputState::NEUTRAL)?;
            let f = pad.feedback_snapshot()?;
            if f.sequence != *sequence {
                println!(
                    "{profile:?} sequence={} valid={} left={} right={}",
                    f.sequence, f.valid, f.left_motor, f.right_motor
                );
                *sequence = f.sequence;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    for (profile, pad, _) in &mut pads {
        println!("{profile:?} diagnostics {:x?}", pad.consumer_diagnostics()?);
    }
    println!("Owned fixtures released.");
    Ok(())
}
