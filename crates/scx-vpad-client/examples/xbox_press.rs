//! One bounded Xbox-compatible button press using the unelevated SDK.
//! Requires the SCX VPad driver/service installation. Run with no game focused:
//! this intentionally emits A once on a newly created virtual controller.

#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use scx_vpad_client::VirtualPad;
    use scx_vpad_protocol::{InputState, Profile, BUTTON_SOUTH};
    use std::{thread, time::Duration};

    {
        let mut pad = VirtualPad::create_profile(Profile::Xbox360)?;
        pad.neutral()?;
        println!("Created an Xbox-compatible controller; sending A for 80 ms.");

        // The SDK assigns the per-device sequence number. Preserve neutral
        // defaults (including the centered hat) for every unused control.
        let pressed = InputState {
            buttons: BUTTON_SOUTH,
            ..InputState::NEUTRAL
        };
        let press_result = pad.submit(&pressed);
        if press_result.is_ok() {
            thread::sleep(Duration::from_millis(80));
        }

        // Attempt release even if the submit failed after reaching Windows.
        let release_result = pad.neutral();
        press_result?;
        release_result?;
    } // Drop closes this lease; the service removes only this controller.

    println!("Released A and closed the controller lease.");
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This SDK example requires Windows and an installed SCX VPad service.");
}
