//! CPU-only comparison of the old/new XUSB submit preparation paths, using
//! the actual exported validation/encoding functions. Not driver latency.
use scx_vpad_core::{scx_vpad_validate_input, xusb::scx_vpad_xusb_state_report};
use scx_vpad_protocol::InputState;
use std::{hint::black_box, time::Instant};

fn sample(cached: bool, packets: &[[u8; 64]]) -> f64 {
    const ROUNDS: usize = 262_144;
    let mut previous = InputState::NEUTRAL.encode();
    let mut controls = [0u8; 12];
    let started = Instant::now();
    for i in 0..ROUNDS {
        let input = black_box(&packets[i % packets.len()]);
        let mut after = [0; 29];
        let changed;
        // SAFETY: all FFI arrays are local, disjoint and correctly sized.
        unsafe {
            if cached {
                assert_eq!(
                    scx_vpad_xusb_state_report(input.as_ptr(), 64, 0, 0, after.as_mut_ptr(), 29),
                    0
                );
                changed = controls != after[11..23];
            } else {
                let mut before = [0; 29];
                assert_eq!(scx_vpad_validate_input(input.as_ptr(), 64), 0);
                assert_eq!(
                    scx_vpad_xusb_state_report(
                        black_box(previous.as_ptr()),
                        64,
                        0,
                        0,
                        before.as_mut_ptr(),
                        29
                    ),
                    0
                );
                assert_eq!(
                    scx_vpad_xusb_state_report(input.as_ptr(), 64, 0, 0, after.as_mut_ptr(), 29),
                    0
                );
                changed = before != after;
            }
        }
        let neutral = after[11..23] == [0; 12];
        previous = *input;
        if cached {
            controls.copy_from_slice(&after[11..23]);
        }
        black_box((&previous, &controls, changed, neutral));
    }
    started.elapsed().as_nanos() as f64 / ROUNDS as f64
}

fn main() {
    let packets: Vec<[u8; 64]> = (0..1024u32)
        .map(|i| {
            InputState {
                sequence: i,
                buttons: i.wrapping_mul(101),
                left_x: i.wrapping_mul(1019) as i16,
                left_y: i.wrapping_mul(773) as i16,
                right_trigger: i.wrapping_mul(257) as u16,
                hat: (i % 9) as u8,
                ..InputState::NEUTRAL
            }
            .encode()
        })
        .collect();
    let mut old = [0.0; 11];
    let mut new = [0.0; 11];
    // Warm both paths; alternate order to reduce scheduling/clock bias.
    black_box((sample(false, &packets), sample(true, &packets)));
    for i in 0..old.len() {
        if i % 2 == 0 {
            old[i] = sample(false, &packets);
            new[i] = sample(true, &packets);
        } else {
            new[i] = sample(true, &packets);
            old[i] = sample(false, &packets);
        }
    }
    old.sort_by(f64::total_cmp);
    new.sort_by(f64::total_cmp);
    println!("{{\"scope\":\"CPU-only XUSB submit preparation via actual FFI; excludes WDF, OS scheduling, device I/O and end-to-end latency\",\"samples_per_path\":11,\"reports_per_sample\":262144,\"old_ns\":{old:?},\"cached_ns\":{new:?},\"old_median_ns\":{},\"cached_median_ns\":{}}}", old[5], new[5]);
}
