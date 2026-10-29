//! Explicit owned-device measurement. Pure matching/statistics tests need no driver.
use scx_vpad_protocol::{InputState, Profile, BUTTON_SOUTH};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub const HELP: &str = "scx-vpadctl benchmark-latency [--profiles generic,ds4,ds5] [--count 10000] [--rate 250] [--warmup 100] [--format text|json|csv] [--priority normal|above-normal] [--spin-us 0..50] [--legacy-barriers]\n  Creates 1..4 owned virtual HID devices and generates button/axis transitions. Close games first. Xbox is unsupported. Nonzero spin and legacy barriers require the latency-probe build feature and Sony-only profiles.";
const WARMUP: u32 = 1 << 23;
const MAX_COUNT: usize = 100_000;

#[derive(Clone, Debug)]
struct Config {
    profiles: Vec<Profile>,
    count: usize,
    warmup: usize,
    rate: u32,
    format: String,
    above_normal: bool,
    spin_us: u32,
    legacy_barriers: bool,
}
impl Config {
    fn parse(args: &[String]) -> Result<Self> {
        let mut c = Self {
            profiles: vec![Profile::GenericHid],
            count: 10_000,
            warmup: 100,
            rate: 250,
            format: "text".into(),
            above_normal: false,
            spin_us: 0,
            legacy_barriers: false,
        };
        let mut seen = std::collections::HashSet::new();
        let mut i = 0;
        while i < args.len() {
            let key = args[i].as_str();
            if !seen.insert(key) {
                return Err(format!("duplicate option {key}").into());
            }
            if key == "--legacy-barriers" {
                c.legacy_barriers = true;
                i += 1;
                continue;
            }
            let value = args
                .get(i + 1)
                .ok_or_else(|| format!("missing value for {key}"))?;
            match key {
                "--profiles" => {
                    c.profiles = value
                        .split(',')
                        .map(|name| crate::profile(Some(&name.to_owned())))
                        .collect::<Result<_>>()?;
                    if !(1..=4).contains(&c.profiles.len())
                        || c.profiles.iter().any(|p| p.report_size() == 0)
                    {
                        return Err(
                            "choose 1..4 HID profiles; Xbox has no HID observation path".into()
                        );
                    }
                }
                "--count" => c.count = crate::number(Some(value), 0, 1, MAX_COUNT as u64)? as usize,
                "--warmup" => c.warmup = crate::number(Some(value), 0, 0, 1000)? as usize,
                "--rate" => c.rate = crate::number(Some(value), 0, 10, 8000)? as u32,
                "--format" if ["text", "json", "csv"].contains(&value.as_str()) => {
                    c.format = value.clone()
                }
                "--priority" if value == "normal" => c.above_normal = false,
                "--priority" if value == "above-normal" => c.above_normal = true,
                "--spin-us" => c.spin_us = crate::number(Some(value), 0, 0, 50)? as u32,
                _ => return Err(format!("unknown option or invalid value: {key} {value}").into()),
            }
            i += 2;
        }
        if c.count + c.warmup > c.rate as usize * 600 {
            return Err("planned duration including warmup must be at most 600 seconds".into());
        }
        if c.spin_us != 0 || c.legacy_barriers {
            if !cfg!(feature = "latency-probe") {
                return Err(
                    "spin/barrier experiments require --features latency-probe at build time"
                        .into(),
                );
            }
            if c.profiles.iter().any(|p| p.is_generic_hid()) {
                return Err("spin/barrier experiments require Sony-only profiles".into());
            }
        }
        Ok(c)
    }
    fn period(&self) -> Duration {
        Duration::from_nanos(1_000_000_000 / u64::from(self.rate))
    }
}

fn profile_name(p: Profile) -> &'static str {
    match p {
        Profile::GenericHid => "generic",
        Profile::Wheel => "wheel",
        Profile::FlightStick => "flight",
        Profile::ArcadeStick => "arcade",
        Profile::DualShock4 => "ds4",
        Profile::DualSense => "ds5",
        _ => "unsupported",
    }
}

/// The four exactly representable axis bytes contain device and sample identity.
/// Sony counters are deliberately not used: the SDK owns those sequences.
fn state(device: usize, sample: u32) -> InputState {
    let marker = (((device + 1) as u32) << 24 | sample).to_le_bytes();
    let axis = |b: u8| (i32::from(b) * 256 - 32768) as i16;
    InputState {
        buttons: if sample & 1 != 0 { BUTTON_SOUTH } else { 0 },
        hat: (sample % 9) as u8,
        left_x: axis(marker[0]),
        left_y: axis(marker[1]),
        right_x: axis(marker[2]),
        right_y: axis(marker[3]),
        left_trigger: ((sample.wrapping_mul(37) & 255) * 257) as u16,
        right_trigger: ((sample.wrapping_mul(73) & 255) * 257) as u16,
        ..InputState::NEUTRAL
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Match {
    Sample(usize),
    Warmup,
    Background,
    Foreign,
    Invalid,
}
fn identify(profile: Profile, device: usize, report: &[u8], count: usize, warmup: usize) -> Match {
    if report.len() != profile.report_size() || report.first() != Some(&1) {
        return Match::Invalid;
    }
    let mut axes = [0; 4];
    if profile.is_generic_hid() {
        for (i, b) in axes.iter_mut().enumerate() {
            let axis = i16::from_le_bytes([report[6 + i * 2], report[7 + i * 2]]);
            *b = ((i32::from(axis) + 32768) / 256) as u8;
        }
    } else {
        axes.copy_from_slice(&report[1..5]);
    }
    let marker = u32::from_le_bytes(axes);
    // Initial/watchdog neutral has no benchmark device marker.
    if axes == [128; 4] {
        return Match::Background;
    }
    if marker >> 24 != (device + 1) as u32 {
        return Match::Foreign;
    }
    let sample = marker & 0x00ff_ffff;
    let is_warmup = sample & WARMUP != 0;
    let index = (sample & !WARMUP) as usize;
    if index == 0 || index > if is_warmup { warmup } else { count } {
        return Match::Invalid;
    }
    let s = state(device, sample);
    let valid = if profile.is_generic_hid() {
        let words = [
            s.left_x as u16,
            s.left_y as u16,
            s.right_x as u16,
            s.right_y as u16,
            s.left_trigger,
            s.right_trigger,
        ];
        u32::from_le_bytes(report[1..5].try_into().unwrap()) == s.buttons
            && report[5] == s.hat
            && words
                .iter()
                .enumerate()
                .all(|(i, w)| report[6 + i * 2..8 + i * 2] == w.to_le_bytes())
    } else {
        let (face, trigger) = if profile == Profile::DualShock4 {
            (5, 8)
        } else {
            (8, 5)
        };
        let shoulders = (u8::from(s.left_trigger != 0) * 4) | (u8::from(s.right_trigger != 0) * 8);
        report[face] == (s.hat | if s.buttons != 0 { 0x20 } else { 0 })
            && report[face + 1] == shoulders
            && report[face + 2]
                & if profile == Profile::DualShock4 {
                    3
                } else {
                    255
                }
                == 0
            && report[trigger] == (s.left_trigger / 257) as u8
            && report[trigger + 1] == (s.right_trigger / 257) as u8
    };
    if !valid {
        Match::Invalid
    } else if is_warmup {
        Match::Warmup
    } else {
        Match::Sample(index - 1)
    }
}

struct Observations {
    first: Vec<Option<Instant>>,
    duplicates: usize,
    out_of_order: usize,
    invalid: usize,
    foreign: usize,
    background: usize,
    warmup: usize,
    highest: Option<usize>,
    error: Option<String>,
}
impl Observations {
    fn new(count: usize) -> Self {
        Self {
            first: vec![None; count],
            duplicates: 0,
            out_of_order: 0,
            invalid: 0,
            foreign: 0,
            background: 0,
            warmup: 0,
            highest: None,
            error: None,
        }
    }
    fn record(&mut self, found: Match, at: Instant) {
        match found {
            Match::Sample(i) => {
                if self.first[i].is_some() {
                    self.duplicates += 1;
                    return;
                }
                if self.highest.is_some_and(|h| i < h) {
                    self.out_of_order += 1;
                }
                self.highest = Some(self.highest.map_or(i, |h| h.max(i)));
                self.first[i] = Some(at);
            }
            Match::Warmup => self.warmup += 1,
            Match::Background => self.background += 1,
            Match::Foreign => self.foreign += 1,
            Match::Invalid => self.invalid += 1,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Stats {
    count: usize,
    p50: u128,
    p95: u128,
    p99: u128,
    p999: u128,
    max: u128,
}
impl Stats {
    fn from(mut ns: Vec<u128>) -> Option<Self> {
        if ns.is_empty() {
            return None;
        }
        ns.sort_unstable();
        let q = |parts: usize| ns[(ns.len() * parts).div_ceil(1000).saturating_sub(1)];
        Some(Self {
            count: ns.len(),
            p50: q(500),
            p95: q(950),
            p99: q(990),
            p999: q(999),
            max: *ns.last().unwrap(),
        })
    }
}

#[cfg(windows)]
pub use native::run;
#[cfg(windows)]
mod native {
    use super::*;
    use scx_vpad_client::{win::Handle, GamepadReader, VirtualPad};
    use std::{
        io::{self, Write},
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        },
        thread,
    };
    use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::{
        Console::*, Threading::*, WindowsProgramming::THREAD_PRIORITY_ERROR_RETURN,
    };

    static INTERRUPTED: AtomicBool = AtomicBool::new(false);
    unsafe extern "system" fn interrupt(kind: u32) -> i32 {
        if kind == CTRL_C_EVENT || kind == CTRL_BREAK_EVENT {
            INTERRUPTED.store(true, Ordering::Relaxed);
            1
        } else {
            0
        }
    }
    struct ConsoleGuard;
    impl ConsoleGuard {
        fn install() -> io::Result<Self> {
            INTERRUPTED.store(false, Ordering::Relaxed);
            if unsafe { SetConsoleCtrlHandler(Some(interrupt), 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self)
        }
    }
    impl Drop for ConsoleGuard {
        fn drop(&mut self) {
            unsafe {
                SetConsoleCtrlHandler(Some(interrupt), 0);
            }
        }
    }
    struct PriorityGuard(i32);
    impl PriorityGuard {
        fn set(above: bool) -> io::Result<Self> {
            let thread = unsafe { GetCurrentThread() };
            let previous = unsafe { GetThreadPriority(thread) };
            if previous == THREAD_PRIORITY_ERROR_RETURN as i32 {
                return Err(io::Error::last_os_error());
            }
            let priority = if above {
                THREAD_PRIORITY_ABOVE_NORMAL
            } else {
                THREAD_PRIORITY_NORMAL
            };
            if unsafe { SetThreadPriority(thread, priority) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(previous))
        }
    }
    impl Drop for PriorityGuard {
        fn drop(&mut self) {
            unsafe {
                SetThreadPriority(GetCurrentThread(), self.0);
            }
        }
    }
    struct Readers {
        stop: Arc<AtomicBool>,
        handles: Vec<thread::JoinHandle<Observations>>,
    }
    impl Readers {
        fn new() -> Self {
            Self {
                stop: Arc::new(AtomicBool::new(false)),
                handles: Vec::new(),
            }
        }
        fn finish(&mut self) -> Result<Vec<Observations>> {
            self.stop.store(true, Ordering::Relaxed);
            let mut results = Vec::new();
            let mut panicked = false;
            for h in self.handles.drain(..) {
                match h.join() {
                    Ok(r) => results.push(r),
                    Err(_) => panicked = true,
                }
            }
            if panicked {
                Err("HID observer panicked; all observers joined".into())
            } else {
                Ok(results)
            }
        }
    }
    impl Drop for Readers {
        fn drop(&mut self) {
            let _ = self.finish();
        }
    }

    fn launch<const N: usize>(
        mut reader: GamepadReader<N>,
        profile: Profile,
        device: usize,
        c: &Config,
        readers: &mut Readers,
    ) -> Result<()> {
        let stop = readers.stop.clone();
        let (count, warmup) = (c.count, c.warmup);
        let (ready, signal) = mpsc::sync_channel(1);
        readers.handles.push(
            thread::Builder::new()
                .name(format!("vpad-hid-{device}"))
                .spawn(move || {
                    let mut result = Observations::new(count);
                    let _priority = match PriorityGuard::set(false) {
                        Ok(p) => p,
                        Err(e) => {
                            result.error = Some(format!("observer priority: {e}"));
                            return result;
                        }
                    };
                    let mut ready = Some(ready);
                    let mut received = 0;
                    while !stop.load(Ordering::Relaxed) {
                        let report = reader.read_after(50, || {
                            if let Some(ready) = ready.take() {
                                let _ = ready.send(());
                            }
                            Ok(())
                        });
                        // Independent observation timestamp precedes decoding/all bookkeeping.
                        let at = Instant::now();
                        match report {
                            Ok(report) => {
                                received += 1;
                                if received > 3 * (count + warmup) + 256 {
                                    result.error = Some("observer report budget exhausted".into());
                                    break;
                                }
                                result
                                    .record(identify(profile, device, &report, count, warmup), at);
                            }
                            Err(e) if e.kind() == io::ErrorKind::TimedOut => (),
                            Err(e) => {
                                result.error = Some(e.to_string());
                                break;
                            }
                        }
                    }
                    result
                })?,
        );
        signal
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "observer did not post its first HID read within 5 seconds")?;
        Ok(())
    }

    struct Submission {
        start: Instant,
        ack: Instant,
        late: Duration,
        accepted: bool,
        error: Option<i32>,
    }
    struct Summary {
        attempted: usize,
        accepted: usize,
        matched: usize,
        missing: usize,
        observed_failed: usize,
        pre_submit: usize,
        unsubmitted: usize,
        achieved_rate_hz: Option<f64>,
        ack: Option<Stats>,
        hid: Option<Stats>,
        lateness: Option<Stats>,
    }
    fn summarize(sent: &[Submission], seen: &Observations) -> Summary {
        let accepted = sent.iter().filter(|s| s.accepted).count();
        let mut latencies = Vec::new();
        let mut missing = 0;
        let mut observed_failed = 0;
        let mut pre_submit = 0;
        for (i, s) in sent.iter().enumerate() {
            match seen.first[i] {
                Some(at) if at < s.start => {
                    pre_submit += 1;
                    if s.accepted {
                        missing += 1;
                    }
                }
                Some(at) if s.accepted => latencies.push(at.duration_since(s.start).as_nanos()),
                Some(_) => observed_failed += 1,
                None if s.accepted => missing += 1,
                None => (),
            }
        }
        Summary {
            attempted: sent.len(),
            accepted,
            matched: latencies.len(),
            missing,
            observed_failed,
            pre_submit,
            unsubmitted: seen.first[sent.len()..]
                .iter()
                .filter(|t| t.is_some())
                .count(),
            achieved_rate_hz: sent.first().zip(sent.last()).and_then(|(a, b)| {
                let span = b.start.duration_since(a.start).as_secs_f64();
                (span > 0.0).then(|| (sent.len() - 1) as f64 / span)
            }),
            ack: Stats::from(
                sent.iter()
                    .filter(|s| s.accepted)
                    .map(|s| s.ack.duration_since(s.start).as_nanos())
                    .collect(),
            ),
            hid: Stats::from(latencies),
            lateness: Stats::from(sent.iter().map(|s| s.late.as_nanos()).collect()),
        }
    }
    struct Pacer(Handle);
    impl Pacer {
        fn new() -> io::Result<Self> {
            // Per-handle high resolution; no global timer-resolution/affinity change.
            Handle::new(unsafe {
                CreateWaitableTimerExW(
                    std::ptr::null(),
                    std::ptr::null(),
                    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    TIMER_ALL_ACCESS,
                )
            })
            .map(Self)
        }
        fn wait_until(&self, at: Instant, deadline: Instant) -> io::Result<bool> {
            while Instant::now() < at {
                if INTERRUPTED.load(Ordering::Relaxed) || Instant::now() >= deadline {
                    return Ok(false);
                }
                let left = at
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(10));
                let due = -(left.as_nanos().div_ceil(100).max(1) as i64);
                if unsafe {
                    SetWaitableTimerEx(
                        self.0 .0,
                        &due,
                        0,
                        None,
                        std::ptr::null(),
                        std::ptr::null(),
                        0,
                    )
                } == 0
                {
                    return Err(io::Error::last_os_error());
                }
                match unsafe { WaitForSingleObject(self.0 .0, 10) } {
                    WAIT_OBJECT_0 | WAIT_TIMEOUT => (),
                    _ => return Err(io::Error::last_os_error()),
                }
            }
            Ok(!INTERRUPTED.load(Ordering::Relaxed) && Instant::now() < deadline)
        }
    }

    pub fn run(args: &[String]) -> Result<()> {
        if args == ["--help"] {
            println!("{HELP}");
            return Ok(());
        }
        let c = Config::parse(args)?;
        let pacer = Pacer::new()?;
        let _console = ConsoleGuard::install()?;
        eprintln!("Owned virtual HID fixtures only; deterministic button/axis transitions. Physical input, game polling and display are excluded. Independent normal-priority observers; producer priority {}.", if c.above_normal { "above-normal" } else { "normal" });
        // Drop order keeps leased devices alive until every observer has stopped.
        let mut pads = Vec::new();
        let mut readers = Readers::new();
        for (device, &profile) in c.profiles.iter().enumerate() {
            let pad = VirtualPad::create_profile(profile)?;
            #[cfg(feature = "latency-probe")]
            let pad = {
                let mut pad = pad;
                pad.set_submit_tuning(c.spin_us, c.legacy_barriers)?;
                pad
            };
            if profile.is_generic_hid() {
                launch(
                    GamepadReader::<18>::open(&pad)?,
                    profile,
                    device,
                    &c,
                    &mut readers,
                )?;
            } else {
                let reader = GamepadReader::<64>::open(&pad)?;
                reader.configure_sony_usb()?;
                launch(reader, profile, device, &c, &mut readers)?;
            }
            pads.push(pad);
        }
        let _priority = PriorityGuard::set(c.above_normal)?;
        let epoch = Instant::now();
        let planned = c.period() * (c.count + c.warmup) as u32;
        let deadline = epoch + planned + Duration::from_secs(10);
        let mut next = epoch;
        let mut sent: Vec<Vec<Submission>> = c
            .profiles
            .iter()
            .map(|_| Vec::with_capacity(c.count))
            .collect();
        let mut stopped = "completed";
        let mut warmup_failed = false;
        'produce: for step in 0..c.warmup + c.count {
            if !pacer.wait_until(next, deadline)? {
                stopped = if INTERRUPTED.load(Ordering::Relaxed) {
                    "interrupted"
                } else {
                    "deadline"
                };
                break;
            }
            let sample = if step < c.warmup {
                WARMUP | (step + 1) as u32
            } else {
                (step - c.warmup + 1) as u32
            };
            let round = Instant::now();
            for (device, pad) in pads.iter_mut().enumerate() {
                if INTERRUPTED.load(Ordering::Relaxed) || Instant::now() >= deadline {
                    stopped = if INTERRUPTED.load(Ordering::Relaxed) {
                        "interrupted"
                    } else {
                        "deadline"
                    };
                    break 'produce;
                }
                let input = state(device, sample);
                let start = Instant::now();
                let result = pad.submit(&input);
                let ack = Instant::now();
                let accepted = result.is_ok();
                if step >= c.warmup {
                    sent[device].push(Submission {
                        start,
                        ack,
                        late: start.saturating_duration_since(next),
                        accepted,
                        error: result.as_ref().err().and_then(io::Error::raw_os_error),
                    });
                } else if !accepted {
                    warmup_failed = true;
                }
                if let Err(e) = result {
                    eprintln!(
                        "device {device} {} submit failed: {e}",
                        profile_name(c.profiles[device])
                    );
                    // Backpressure is evidence; avoid turning a failed run into a queue flood.
                    stopped = "submit_error";
                    break 'produce;
                }
            }
            // No catch-up burst after an oversleep or slow submit. Record lateness.
            next = (next + c.period()).max(round + c.period());
        }
        // A fixed, bounded drain window exposes missing reports, including tail loss.
        if !INTERRUPTED.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(250));
        }
        let seen = readers.finish()?;
        let elapsed = epoch.elapsed();
        drop(pads); // Lease close removes only fixtures; no physical output sink exists.
        let summaries: Vec<_> = sent
            .iter()
            .zip(&seen)
            .map(|(s, o)| summarize(s, o))
            .collect();
        let valid = stopped == "completed"
            && !warmup_failed
            && summaries.iter().zip(&seen).all(|(s, o)| {
                s.attempted == c.count
                    && s.accepted == c.count
                    && s.missing == 0
                    && s.pre_submit == 0
                    && s.unsubmitted == 0
                    && o.duplicates == 0
                    && o.out_of_order == 0
                    && o.invalid == 0
                    && o.foreign == 0
                    && o.error.is_none()
            });
        emit(&c, epoch, elapsed, stopped, valid, &sent, &seen, &summaries)?;
        if valid {
            Ok(())
        } else {
            Err(
                "benchmark incomplete or report integrity check failed; inspect emitted counters"
                    .into(),
            )
        }
    }

    fn json_string(s: &str) -> String {
        let mut out = String::from("\"");
        for ch in s.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                ch if ch < ' ' => out.push_str(&format!("\\u{:04x}", ch as u32)),
                ch => out.push(ch),
            }
        }
        out.push('"');
        out
    }
    fn stats_json(s: Option<Stats>) -> String {
        s.map_or("null".into(), |s| {
            format!(
                "{{\"count\":{},\"p50\":{},\"p95\":{},\"p99\":{},\"p99_9\":{},\"max\":{}}}",
                s.count, s.p50, s.p95, s.p99, s.p999, s.max
            )
        })
    }
    fn signed_ns(at: Instant, since: Instant) -> String {
        if let Some(elapsed) = at.checked_duration_since(since) {
            elapsed.as_nanos().to_string()
        } else {
            format!("-{}", since.duration_since(at).as_nanos())
        }
    }
    fn text_stats(out: &mut dyn Write, name: &str, stats: Option<Stats>) -> io::Result<()> {
        if let Some(s) = stats {
            writeln!(
                out,
                "  {name}: n={} p50={:.3} p95={:.3} p99={:.3} p99.9={:.3} max={:.3} us",
                s.count,
                s.p50 as f64 / 1000.,
                s.p95 as f64 / 1000.,
                s.p99 as f64 / 1000.,
                s.p999 as f64 / 1000.,
                s.max as f64 / 1000.
            )
        } else {
            writeln!(out, "  {name}: no valid samples")
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn emit(
        c: &Config,
        epoch: Instant,
        elapsed: Duration,
        stopped: &str,
        valid: bool,
        sent: &[Vec<Submission>],
        seen: &[Observations],
        summaries: &[Summary],
    ) -> io::Result<()> {
        let mut stdout = io::BufWriter::new(io::stdout().lock());
        if c.format == "json" {
            write!(stdout,"{{\"schema\":1,\"scope\":\"submit_start_to_api_ack_and_independent_matching_HID_read_return\",\"units\":\"ns\",\"producer_mode\":\"single_thread_round_robin\",\"pacer\":\"high_resolution_waitable_timer\",\"driver_worker_priority\":\"unchanged\",\"valid\":{valid},\"stop_reason\":{},\"requested_count_per_device\":{},\"warmup_per_device\":{},\"requested_rate_hz_per_device\":{},\"producer_priority\":{},\"observer_priority\":\"normal\",\"spin_us\":{},\"legacy_payload_barriers\":{},\"elapsed_ns\":{},\"devices\":[",json_string(stopped),c.count,c.warmup,c.rate,json_string(if c.above_normal {"above-normal"}else{"normal"}),c.spin_us,c.legacy_barriers,elapsed.as_nanos())?;
            for (i, (s, o)) in summaries.iter().zip(seen).enumerate() {
                if i != 0 {
                    write!(stdout, ",")?;
                }
                write!(
                    stdout,
                    "{{\"device\":{i},\"profile\":{},\"achieved_rate_hz\":{},",
                    json_string(profile_name(c.profiles[i])),
                    s.achieved_rate_hz
                        .map_or("null".into(), |v| format!("{v:.6}"))
                )?;
                write!(stdout,"\"attempted\":{},\"accepted\":{},\"matched\":{},\"missing_accepted\":{},\"duplicates\":{},\"out_of_order\":{},\"invalid\":{},\"foreign\":{},\"background\":{},\"warmup_observed\":{},\"observed_failed\":{},\"pre_submit\":{},\"unsubmitted\":{},\"observer_error\":{},\"api_ack_ns\":{},\"hid_observed_ns\":{},\"schedule_lateness_ns\":{}}}",s.attempted,s.accepted,s.matched,s.missing,o.duplicates,o.out_of_order,o.invalid,o.foreign,o.background,o.warmup,s.observed_failed,s.pre_submit,s.unsubmitted,o.error.as_ref().map_or("null".into(),|e|json_string(e)),stats_json(s.ack),stats_json(s.hid),stats_json(s.lateness))?;
            }
            writeln!(stdout, "]}}")?;
        } else {
            let mut stderr = io::stderr().lock();
            let output: &mut dyn Write = if c.format == "csv" {
                &mut stderr
            } else {
                &mut stdout
            };
            writeln!(output,"valid={valid} stop={stopped} requested={} Hz/device count={} warmup={} spin={} us legacy_barriers={} elapsed={:.3} s",c.rate,c.count,c.warmup,c.spin_us,c.legacy_barriers,elapsed.as_secs_f64())?;
            writeln!(output,"API acknowledgement and independent HID read-return timing; excludes physical input/game/display. Quantiles use nearest rank; high quantiles need sufficient samples.")?;
            for (i, (s, o)) in summaries.iter().zip(seen).enumerate() {
                writeln!(output,"device={i} profile={} attempted={} accepted={} matched={} missing={} duplicate={} out_of_order={} invalid={} foreign={} background={} warmup={} observed_failed={} pre_submit={} unsubmitted={} observer_error={:?}",profile_name(c.profiles[i]),s.attempted,s.accepted,s.matched,s.missing,o.duplicates,o.out_of_order,o.invalid,o.foreign,o.background,o.warmup,s.observed_failed,s.pre_submit,s.unsubmitted,o.error)?;
                writeln!(
                    output,
                    "  achieved rate: {} Hz/device",
                    s.achieved_rate_hz
                        .map_or("unavailable".into(), |v| format!("{v:.3}"))
                )?;
                text_stats(output, "API ack", s.ack)?;
                text_stats(output, "HID observed", s.hid)?;
                text_stats(output, "schedule lateness", s.lateness)?;
            }
            if c.format == "csv" {
                writeln!(stdout,"device,profile,sample,accepted,submit_start_ns,api_ack_ns,hid_observed_ns,hid_observed_at_ns,schedule_lateness_ns,win32_error")?;
                for (i, samples) in sent.iter().enumerate() {
                    for (j, s) in samples.iter().enumerate() {
                        let observed = seen[i].first[j]
                            .map(|t| signed_ns(t, s.start))
                            .unwrap_or_default();
                        let observed_at = seen[i].first[j]
                            .map(|t| signed_ns(t, epoch))
                            .unwrap_or_default();
                        writeln!(
                            stdout,
                            "{i},{},{},{},{},{},{},{},{},{}",
                            profile_name(c.profiles[i]),
                            j + 1,
                            s.accepted,
                            s.start.duration_since(epoch).as_nanos(),
                            s.ack.duration_since(s.start).as_nanos(),
                            observed,
                            observed_at,
                            s.late.as_nanos(),
                            s.error.map(|e| e.to_string()).unwrap_or_default()
                        )?;
                    }
                }
            }
        }
        stdout.flush()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn independent_observation_is_not_clamped_to_acknowledgement_and_losses_stay_visible() {
            let t = Instant::now();
            let make = |offset, accepted| Submission {
                start: t + Duration::from_micros(offset),
                ack: t + Duration::from_micros(offset + 100),
                late: Duration::ZERO,
                accepted,
                error: None,
            };
            let sent = vec![
                make(0, true),
                make(1000, true),
                make(2000, false),
                make(3000, true),
            ];
            let mut observed = Observations::new(5);
            observed.record(Match::Sample(0), t + Duration::from_micros(30));
            observed.record(Match::Sample(2), t + Duration::from_micros(2030));
            observed.record(Match::Sample(3), t + Duration::from_micros(2999));
            observed.record(Match::Sample(4), t + Duration::from_micros(4000));
            let s = summarize(&sent, &observed);
            assert_eq!(
                (s.attempted, s.accepted, s.matched, s.missing),
                (4, 3, 1, 2)
            );
            assert_eq!((s.observed_failed, s.pre_submit, s.unsubmitted), (1, 1, 1));
            assert_eq!(s.hid.unwrap().p50, 30_000);
            assert_eq!(s.ack.unwrap().p50, 100_000);
            assert_eq!(s.achieved_rate_hz, Some(1000.));
            assert_eq!(signed_ns(t, t + Duration::from_micros(1)), "-1000");
        }
        #[test]
        fn json_strings_escape_control_characters_and_preserve_unicode() {
            assert_eq!(
                json_string("line\n\"\\\u{1}ö"),
                "\"line\\n\\\"\\\\\\u0001ö\""
            );
            assert_eq!(stats_json(None), "null");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }
    #[test]
    fn configuration_rejects_unbounded_unsupported_and_ambiguous_requests() {
        assert!(Config::parse(&[]).is_ok());
        for s in [
            "--count 0",
            "--count 100001",
            "--warmup 1001",
            "--rate 0",
            "--rate 8001",
            "--count 100000 --rate 10",
            "--profiles xbox",
            "--profiles generic,ds4,ds5,wheel,flight",
            "--count",
            "--count 3 --count 4",
            "--format yaml",
            "--priority realtime",
            "--spin-us 51",
            "--unknown 1",
        ] {
            assert!(Config::parse(&args(s)).is_err(), "{s}");
        }
        assert!(Config::parse(&args(
            "--profiles generic,ds4,ds5 --count 1000 --rate 1000 --format json"
        ))
        .is_ok());
        assert!(Config::parse(&args("--profiles generic --spin-us 1")).is_err());
        assert!(
            Config::parse(&args("--profiles ds4 --spin-us 5 --legacy-barriers")).is_ok()
                == cfg!(feature = "latency-probe")
        );
    }
    #[test]
    fn deterministic_reports_match_unique_device_and_sample_after_native_encoding() {
        for profile in [
            Profile::GenericHid,
            Profile::Wheel,
            Profile::FlightStick,
            Profile::ArcadeStick,
            Profile::DualShock4,
            Profile::DualSense,
        ] {
            for device in 0..4 {
                for sample in [
                    1,
                    2,
                    255,
                    256,
                    65535,
                    65536,
                    MAX_COUNT as u32,
                    WARMUP | 1,
                    WARMUP | 1000,
                ] {
                    let mut input = state(device, sample);
                    input.sequence = 429;
                    input.validate().unwrap();
                    let report = scx_vpad_core::profiles::encode(profile, &input).unwrap();
                    let report = &report[..profile.report_size()];
                    assert_eq!(
                        identify(profile, device, report, MAX_COUNT, 1000),
                        if sample & WARMUP != 0 {
                            Match::Warmup
                        } else {
                            Match::Sample(sample as usize - 1)
                        }
                    );
                    assert_eq!(
                        identify(profile, (device + 1) % 4, report, MAX_COUNT, 1000),
                        Match::Foreign
                    );
                    let mut damaged = report.to_vec();
                    damaged[if profile.is_generic_hid() {
                        1
                    } else if profile == Profile::DualShock4 {
                        5
                    } else {
                        8
                    }] ^= 0x10;
                    assert_eq!(
                        identify(profile, device, &damaged, MAX_COUNT, 1000),
                        Match::Invalid
                    );
                    assert_eq!(
                        identify(
                            profile,
                            device,
                            &report[..report.len() - 1],
                            MAX_COUNT,
                            1000
                        ),
                        Match::Invalid
                    );
                }
            }
        }
    }
    #[test]
    fn generic_low_axis_bits_cannot_alias_a_sample_and_neutral_never_matches() {
        for profile in [Profile::GenericHid, Profile::DualShock4, Profile::DualSense] {
            let neutral = scx_vpad_core::profiles::encode(profile, &InputState::NEUTRAL).unwrap();
            assert_eq!(
                identify(profile, 0, &neutral[..profile.report_size()], 10, 0),
                Match::Background
            );
        }
        let mut r = scx_vpad_core::profiles::encode(Profile::GenericHid, &state(0, 1)).unwrap();
        r[6] ^= 1;
        assert_eq!(
            identify(Profile::GenericHid, 0, &r[..18], 10, 0),
            Match::Invalid
        );
    }
    #[test]
    fn duplicate_and_reordered_reports_do_not_replace_first_timestamp_or_hide_losses() {
        let now = Instant::now();
        let mut o = Observations::new(4);
        o.record(Match::Sample(2), now);
        o.record(Match::Sample(0), now + Duration::from_millis(1));
        o.record(Match::Sample(2), now + Duration::from_millis(2));
        o.record(Match::Invalid, now);
        assert_eq!(o.first[2], Some(now));
        assert_eq!(o.duplicates, 1);
        assert_eq!(o.out_of_order, 1);
        assert_eq!(o.first.iter().filter(|t| t.is_none()).count(), 2);
        assert_eq!(o.invalid, 1);
    }
    #[test]
    fn nearest_rank_quantiles_include_long_tail_and_empty_sets_are_explicit() {
        assert!(Stats::from(vec![]).is_none());
        let s = Stats::from((1..=1000).rev().collect()).unwrap();
        assert_eq!(
            (s.count, s.p50, s.p95, s.p99, s.p999, s.max),
            (1000, 500, 950, 990, 999, 1000)
        );
        let s = Stats::from(vec![7]).unwrap();
        assert_eq!((s.p50, s.p999, s.max), (7, 7, 7));
    }
}
