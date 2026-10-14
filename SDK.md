# Rust SDK quick start

The SDK connects as the ordinary Windows user configured during installation.
Install the driver and service once using the repository's installer first;
creating or submitting to a controller does not request elevation.

From the repository root, run the bounded example with no game focused. It
creates one Xbox-compatible output, submits neutral state, presses A for 80 ms,
releases it, and closes its own lease:

```powershell
cargo run --release -p scx-vpad-client --example xbox_press
```

See [the complete example](crates/scx-vpad-client/examples/xbox_press.rs), including
its error path that attempts release even when a submission fails. Windows
removes the device asynchronously after lease closure. This example builds on
other platforms but only prints a Windows requirement there.

For a local integration, add both crates as path dependencies:

```toml
[dependencies]
scx-vpad-client = { path = "../SCX-VPad/crates/scx-vpad-client" }
scx-vpad-protocol = { path = "../SCX-VPad/crates/scx-vpad-protocol" }
```

Create `VirtualPad::create_profile(Profile::Xbox360)` and pass normalized
`InputState` values to `submit`. Start from `InputState::NEUTRAL`, rather than a
zero-filled structure: the neutral hat value is 8. Sticks use signed 16-bit axes
(X positive right, Y positive down); triggers span 0–65535. `BUTTON_SOUTH` maps
to Xbox A. The SDK overwrites `sequence` with its own per-device counter; callers
do not need to increment it. Invalid input returns an error before submission.

Submit when input changes. While holding a non-neutral state, resubmit that
state at least every 250 ms: a 500 ms producer watchdog neutralizes stale input.
This is a safety fallback, not an input smoothing timer. Explicitly submit
neutral state when disconnecting; dropping `VirtualPad` releases only that lease,
and a broken client pipe also causes service cleanup. Handle submission errors
and reconnect deliberately instead of assuming a failed call was applied.

There are at most 16 simultaneous devices, four leases per client process, and
four Xbox-compatible devices system-wide within this service. Other implemented
profiles are `GenericHid`, `DualShock4`, and `DualSense`. Xbox and Sony compatibility
paths remain experimental; Sony profiles do not implement every physical-controller
feature. The generic
HID profile is not an XInput controller. See [validation](VALIDATION.md) for the
tested compatibility limits and [security](SECURITY.md) for the lease boundary.

The service authenticates lease creation. Xbox submissions use the restricted
native control interface; HID profiles automatically include a private per-lease
capability. Keep the SDK lease private rather than copying its authorization
into logs or another process. Do not construct legacy HID feature frames.

The repository license applies to SDK distribution and integration; commercial
distribution requires the rights holder's written permission. See [LICENSE](LICENSE).

Native .24 requires the matching service and SDK. Sony submissions use a
per-lease shared section and an acknowledged event, separate from public HID.
The SDK does not return success until the driver validates and accepts the
packet. A submission timeout poisons that producer until reconnect; no pending
packet is overwritten. Xbox and generic HID retain their established transports.

Sony feedback: create one `FeedbackReader` per owned `VirtualPad`, wait with
`read(timeout_ms)` on a separate worker, and treat `TimedOut` as an idle wait.
Use `feedback_snapshot()` for
an independent immediate read without consuming the feedback stream. Native .24
uses a separate feedback sequence/event; a timeout does not consume a change.
The updated SDK retains the preceding drivers' HID feedback paths when the
old service has not provisioned a channel. Xbox is unchanged.


Native .28 keeps Sony devices in the ROOT-compatible Windows container. Do not
use the container GUID as a unique controller identifier: use the lease/device
path or the distinct GameInput device ID instead. Xbox/generic devices retain
their per-device containers. Four-controller tests verify independent identity,
input, feedback and removing one device while its three siblings remain active.
