# Rust SDK quick start

## Source Guard in the .37 preview

`SourceGuard::acquire_if_installed(hid_path)` validates the physical collection
and acquires a short-lived lease only when the exact device filter is installed.
Authorize every SCX-owned overlapped HID handle with
`guard.access().authorize_handle(handle)` before starting reads, feature writes
or feedback. Keep the guard alive until these workers have stopped. Check lease
health during processing; do not silently continue emulation after lease loss.
No filter returns `None`, not a claim that the source is protected. Failed
authentication on a present filter is an error. SDL-owned and XInput-only
handles are not covered by this SDK integration.

Read-only model discovery: `cargo run --release -p scx-vpad-client --example
source_models`. It emits model IDs, not device serials or instance paths.
The bounded `isolation/SourceModels.ps1` generator accepts exact qualified
hardware IDs and rejects wildcard, class-wide, revision-less USB and injected
INF entries. Model installation needs elevation once; normal source leases do
not. New hardware is not automatically installed at runtime.

The release's selected model list is `isolation/models-qualified.txt`. To build
the same coverage, pass `-IncludeSourceGuard -SourceModelFile
isolation/models-qualified.txt` to `scripts/build-package.ps1`. Other generic
HID models require their own exact model entry and installed-device validation.

## Virtual outputs

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
profiles are `GenericHid`, `DualShock4`, and `DualSense`; native revision .33 adds
`Wheel`, `FlightStick`, and `ArcadeStick`. Xbox and Sony compatibility
paths have the documented compatibility scope; Sony profiles do not implement every physical-controller
feature. The generic
HID profile is not an XInput controller. See [validation](VALIDATION.md) for the
tested compatibility limits and [security](SECURITY.md) for the lease boundary.

## Generic device classes (native .33)

The new classes use the same bounded `InputState` and authenticated generic HID
transport. They expose 32 independent buttons, one eight-way hat (8 = neutral),
four signed axes and two unsigned axes. Device class and axis usages are fixed
by the Rust descriptor; the client cannot upload a descriptor. Each class has
its own SCX product ID and stable per-installation/profile/slot container.

| Profile | Windows HID class | `left_x`, `left_y`, `right_x`, `right_y` | `left_trigger`, `right_trigger` |
| --- | --- | --- | --- |
| `GenericHid` | Gamepad | X, Y, Rx, Ry | Z, Rz |
| `Wheel` | Joystick | Steering, Y, Rx, Ry | Accelerator, brake |
| `FlightStick` | Joystick | X, Y, Rz, Rx | Throttle, brake |
| `ArcadeStick` | Gamepad | X, Y, Rx, Ry | Z, Rz |

Use `VirtualPad::create_profile(Profile::Wheel)` (or the other class). Class-aware
games can bind these HID/DirectInput controls; this does not guarantee automatic
classification as `Windows.Gaming.Input.RacingWheel`, XInput support, or support
by every game. These input-only profiles do not implement force feedback,
rumble, a keyboard/mouse endpoint, physical-device drivers, or legacy adapters.
Their capability flags and feedback API reject unsupported output feedback.
Physical USB/Bluetooth/adapter detection and remapping belong in the SCX app.

With games closed, the explicit `scx-vpadctl self-test-generic-classes` command
creates four classes at once, verifies Windows HID usage parsing, button/axis
independence and watchdog neutralization, then removes its own devices. Run
`scx-vpadctl security-check wheel` (and `flight`/`arcade`) separately to verify
the installed capability and token boundary. These checks require .33 installed;
building them does not certify runtime compatibility.

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


Native .32 restores a unique, stable per-installation/profile/slot Windows
container for Sony devices as well. Xbox/generic identities retain their existing
values; the new .33 classes use separate profile IDs. Use the lease/device path
for live ownership, never the public container ID as an authorization token.
Four-controller tests verify independent identity,
input, feedback and removing one device while its three siblings remain active.
