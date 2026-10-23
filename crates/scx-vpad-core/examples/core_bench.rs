//! CPU-only measurement. Does not measure USB/Bluetooth, scheduling, WDF,
//! end-to-end latency, or a comparison with other controller drivers.
use scx_vpad_core::hid_state::HidState;
use scx_vpad_protocol::{InputState, Profile};
use std::{hint::black_box, time::Instant};

fn main() {
    const DEVICES: usize = 16;
    const ROUNDS: u32 = 32768;
    const SAMPLES: usize = 9;
    println!("{{\"scope\":\"CPU-only decode, queue and report encoding; not driver latency\",\"devices\":{DEVICES},\"samples\":[");
    for (profile_index, profile) in [Profile::GenericHid, Profile::DualShock4, Profile::DualSense]
        .into_iter()
        .enumerate()
    {
        let mut times = [0f64; SAMPLES];
        for sample in &mut times {
            let mut queues: [HidState; DEVICES] =
                core::array::from_fn(|_| HidState::new(profile).unwrap());
            for queue in &mut queues {
                queue.consume();
            }
            let start = Instant::now();
            for sequence in 0..ROUNDS {
                for (index, queue) in queues.iter_mut().enumerate() {
                    let state = InputState {
                        sequence,
                        buttons: (sequence & 1) << (index % 15),
                        left_x: sequence as i16,
                        right_trigger: (sequence as u16).wrapping_mul(17),
                        timestamp_us: u64::from(sequence) * 1000,
                        ..InputState::NEUTRAL
                    };
                    let packet = black_box(state.encode());
                    queue
                        .submit(black_box(&packet))
                        .expect("valid independent device");
                    black_box(queue.current());
                    if queue.front().is_some() {
                        queue.consume();
                    }
                }
            }
            *sample = start.elapsed().as_nanos() as f64 / (f64::from(ROUNDS) * DEVICES as f64);
        }
        times.sort_by(f64::total_cmp);
        println!("{}{{\"profile\":\"{profile:?}\",\"reports_per_sample\":{},\"nanoseconds_per_report\":{:?},\"median_nanoseconds\":{:.2}}}",
            if profile_index==0{""}else{","},ROUNDS as usize*DEVICES,times,times[SAMPLES/2]);
    }
    println!("]}}");
}
