# Independent HID observation benchmark

`benchmark-latency` creates only its own leased virtual devices. It never opens a physical controller or connects a physical feedback sink. It generates deterministic button, hat, stick and trigger transitions, so close games before running it. Ending the run removes its leased fixtures. Xbox has no matching HID reader here and is rejected; the existing `benchmark` command continues to support its API completion measurement.

```powershell
& scripts/cargo.ps1 -CargoArgs @('build','-p','scx-vpadctl','--release','--features','latency-probe','--locked','--offline')
& target/release/scx-vpadctl.exe benchmark-latency --profiles ds5 --count 10000 --rate 1000 --warmup 100 --format json > ds5.json
& target/release/scx-vpadctl.exe benchmark-latency --profiles generic,ds4,ds5 --count 10000 --rate 250 --format csv > mixed.csv 2> mixed-summary.txt
```

These examples are execution instructions, not evidence that those runs passed. Build and hardware results belong in separate dated logs with executable SHA-256, installed driver revision/hash, CPU, power plan, OS build and background workload. Keep the installed driver fixed for client-only comparisons.

## What is measured

- **API acknowledgement:** immediately before `VirtualPad::submit` to immediately after it returns successfully. Rejected submissions are counted separately and excluded from successful acknowledgement quantiles.
- **Independent HID observation:** the same submission start to the matching `GamepadReader::read_after` return on a separate observer thread. Its timestamp is taken before decoding or statistics bookkeeping. It can precede the producer's acknowledgement. It includes the HID stack, completion handling and observer scheduling; it is not a kernel-completion timestamp, game observation, physical-input measurement, or input-to-screen latency.
- **Schedule lateness:** actual submission start minus the requested round deadline. `--rate` is per device; mixed profiles are submitted sequentially within a round by one producer (`single_thread_round_robin`). A per-handle high-resolution waitable timer avoids permanent spinning, global timer-resolution changes and CPU affinity changes. The benchmark fails before creating fixtures if that timer is unavailable. Oversleeps do not cause catch-up bursts. Requested rate is a target, not a claim: achieved rate is reported from first/last measured submission starts per device. Reject comparisons whose achieved rates or scheduling distributions differ materially.

The four quantized stick bytes encode a unique device/sample marker. The reader independently verifies the sample's button, hat and trigger fields (and full-resolution generic axes). Native Sony sequence counters are excluded because the SDK assigns them. This identifies a specific submitted sample rather than the most recent arbitrary report or a repeated alternating press. Warmup uses a separate marker namespace and is excluded from latency distributions.

Every accepted measured sample must be observed exactly once during the run plus a 250 ms final drain. Missing accepted samples, duplicates, out-of-order samples, malformed controls, foreign device markers, observations preceding their submission, and reader errors are reported explicitly. A failed integrity check or incomplete run produces output with `valid=false` and exits nonzero. Neutral startup/watchdog reports and warmup observations have separate counters. A 250 ms drain bounds observation; "missing" means not observed within that window, not proof of a permanent driver drop.

All metrics report p50, p95, p99, p99.9 and maximum using nearest-rank quantiles. Empty distributions are `null`, never zero. At least 10,000 samples per device is useful for tail inspection; p99.9 from a tiny smoke run is effectively a maximum and cannot establish tail performance. Historical `benchmark`/`benchmark-hid` retain their existing percentile convention and now also print p99.9; the new command explicitly uses nearest rank.

## Output and experimental switches

`--format json` emits a single schema-1 summary with nanosecond distributions, configuration, achieved rates, validity and counters. Diagnostics go to stderr. `--format csv` emits per-attempt rows with device, profile, sample ID, acceptance, submission start relative to the run epoch, acknowledgement/return duration, independent observation duration, observation timestamp relative to the epoch, lateness and Win32 error. Missing observations have empty cells; negative observation deltas preserve pre-submission anomalies. Its complete summary and integrity counters go to stderr; retain that file and the exit code alongside the CSV. CSV rows for failed submits contain return duration, not a successful acknowledgement.

Default builds use the production submission policy. The optional `latency-probe` feature enables these controlled Sony-only comparisons in the same executable:

```powershell
# Baseline payload barriers; scheduling and zero-spin policy unchanged.
& target/release/scx-vpadctl.exe benchmark-latency --profiles ds5 --rate 1000 --count 10000 --legacy-barriers --format json > legacy.json
# Production payload barriers, event wait.
& target/release/scx-vpadctl.exe benchmark-latency --profiles ds5 --rate 1000 --count 10000 --format json > production.json
# Separate bounded-spin experiment, not the production default.
& target/release/scx-vpadctl.exe benchmark-latency --profiles ds5 --rate 1000 --count 10000 --spin-us 5 --format json > spin5.json
```

`--spin-us` accepts 0..50. Nonzero spin or `--legacy-barriers` requires the feature and all-Sony profiles. `--priority normal|above-normal` affects only the submitting benchmark thread and restores its original priority on return. Every observer explicitly uses normal priority. Driver worker priority is unchanged and recorded as such. No process-wide priority or affinity is changed. Above-normal is an experimental comparison, not a recommended production setting.

Alternate A/B order across several runs; compare successful matching observation distributions, integrity, achieved rates and externally measured process/system CPU together. Do not infer an end-to-end benefit from acknowledgement alone. The tool does not measure CPU consumption or physical latency.

## Bounds and cleanup

Arguments allow 1..4 profiles, 1..100,000 measured samples per profile, 0..1,000 warmup samples, 10..8,000 Hz, and a planned duration of at most 600 seconds. Storage is allocated before the measurement loop and bounded by those limits. Readers have finite 50 ms waits and a total report budget. Submission errors stop the run instead of flooding a full queue. The producer checks a cooperative deadline of planned duration plus ten seconds; Ctrl-C/Break requests early cleanup. Observers are joined before owned leases are released, including error returns.

The deadline is cooperative: existing synchronous Windows/device calls, particularly generic `HidD_SetFeature` and cancellation completion, cannot be forcibly bounded by this tool if the OS/driver wedges. It does not abandon a live `OVERLAPPED`, terminate a thread, or free buffers still used by Windows. Process termination still closes the leases through the existing service lifecycle. Startup is separate from the measured epoch.

Unit tests require no device or installed driver. They cover profile-aware matching against real core encoding, wrong devices, corrupt controls, truncated reports, quantization aliases, loss/duplicate/reorder accounting, observations preceding acknowledgement, rejected submissions, invalid arguments and exact percentile ranks.
