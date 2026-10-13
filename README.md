# SCX VPad

An independently implemented Windows virtual-controller driver with a Rust core,
a small UMDF2 binding, and a restricted Rust service for device management.

**Development version 0.1.0.** Independently written source, not a HIDMaestro or
ViGEmBus fork. This is an experimental driver and SDK, not a production-certified
drop-in replacement. Its Windows validation and remaining compatibility gaps
are recorded separately from protocol unit tests.

## Architecture

The administrator installs the driver and service once. The installer creates a
nonexportable machine-local signing key and trusts its public certificate locally.
It does not enable test signing or turn off Secure Boot or Memory Integrity.
Normal applications communicate with the service without elevation. The service
retains privileges only to create and remove the allowlisted SCX devices.

Controller state travels directly from the native client to the UMDF2 driver,
using a vendor HID collection or the restricted Xbox control interface. The
privileged service's pipe manages device lifetime, not each input packet. There is no
network transport or fixed input polling timer. State encoding uses fixed-size
buffers. Devices have separate queues and synchronization, bounded capacity,
explicit backpressure and stale-input neutralization.

## Current implementation

- Generic HID gamepad: 32 buttons, hat, four signed stick axes and two unsigned
  trigger axes. This profile was exercised through real Windows HID readers.
- Experimental Xbox/XInput-compatible UMDF function device, with an original
  XUSB request dispatcher. This is separate from the generic HID profile.
- Experimental DualShock 4 and DualSense USB-style HID protocols: buttons,
  axes, triggers, motion, two touch contacts, basic rumble and LED feedback.
- Up to 16 virtual devices across clients, with a per-process quota of four.
  Xbox devices have an additional global quota of four; XInput has four slots.
- Strict versioned little-endian input packets, bounds and reserved-byte checks.
- Direct native input submission, independent HID transition queues and
  separate Xbox state. Event-driven feedback has bounded, cancellable waits.
- Owner SID, session and integrity checks; local-only management pipe;
  authenticated HID input capabilities; restricted Xbox control interface;
  server-process verification, quotas and removal on lease disconnection.
- Stale non-neutral input resets after the watchdog interval; producers must
  refresh held state at least every 250 ms.
- Windows service, Rust client library and a command-line test client.
- Stable device identities, locally signed installation and scoped removal.

Generic HID does not expose Sony or Steam features. Sony profiles have synthetic
SCX calibration, identity and feature replies, not copied hardware firmware.
Native USB audio, adaptive trigger effects, Bluetooth radio emulation, console
pairing, Steam-controller output, Windows Gaming Input and GameInput compatibility
are not implemented or validated. The generic profile has no rumble protocol.
Do not assume universal game support from a protocol or enumeration test.

## Build

Initial target: **Windows 11 x64, build 22000 or newer**. ARM64 and Windows 10 are
not currently packaged or validated.

Install Rust stable for MSVC (including `rustfmt`, `clippy` and `rust-docs`),
Visual Studio C++ build tools, Windows SDK
10.0.26100.0 and the Windows Driver Kit. The build can also use the official
`Microsoft.Windows.WDK.x64` NuGet package (10.0.26100.6584) extracted locally;
set `SCX_WDK_ROOT` to its `c` directory. Microsoft tools are build dependencies
and are not redistributed in the driver package.

```powershell
cargo fetch --locked
./scripts/fetch-wdk.ps1
./scripts/cargo.ps1 -CargoArgs @('test','--workspace','--locked','--offline')
./scripts/build-package.ps1
```

The package builder runs tests, creates native release binaries, verifies the
INF using Microsoft's InfVerif, generates a catalogue using Inf2Cat and writes
a file-hash manifest. It does not install anything or modify Windows trust.

## Install and exercise a development build

Run `installer/Install.ps1` from a complete generated package as the intended
interactive user. The script requests administrative elevation once and carries
that user's SID into the service configuration. Initial setup authorizes one
local/domain/MS-account Windows user; changing that owner is administrative.
It installs only SCX VPad under
`Program Files\SCX VPad`, records its own driver package and certificate, and
does not change existing controller-driver installations.

The signing key is local, nonexportable and restricted to administrators and
SYSTEM. Installation failures are recorded under `Program Files\SCX VPad\logs`.
Copy a failure log before uninstalling; removal deletes that installation folder.
Use the packaged `installer/Uninstall.ps1` for removal. Driver/service upgrades
remain administrative operations; ordinary controller connections do not.

```powershell
./bin/scx-vpadctl.exe status
./bin/scx-vpadctl.exe --help
./bin/scx-vpadctl.exe self-test
./bin/scx-vpadctl.exe benchmark-hid 20000
```

Consult the CLI help for bounded demo and benchmark commands. A submission-time
benchmark measures the API path; it is not physical controller-to-screen latency.
Measured submission-to-HID-read completion also excludes physical transport,
game polling and display delay. This project makes no "fastest driver" claim.
See [SDK.md](SDK.md) and the Rust client example for application integration.
The existing SCX application is not switched to this backend by installation.
Recorded results and remaining test coverage are in [VALIDATION.md](VALIDATION.md).

## License and commercial distribution

Copyright 2026 **Tim Haller**. Source is available under the [SCX Source Available
Noncommercial Distribution License](LICENSE). Use and modification are allowed,
including internal organizational use. **Commercial distribution, paid bundling
and commercial hosted access require Tim Haller's prior written permission.**
Contact **tim.haller@gmail.com**.

This commercial-distribution restriction means the project is source available,
not OSI-approved open source. Third-party components keep their own licenses.
The driver is independently implemented; no HIDMaestro driver, SDK or device
profile is bundled. See [security boundaries](SECURITY.md),
[protocol provenance](crates/scx-vpad-core/PROVENANCE.md),
[XUSB contract](crates/scx-vpad-core/XUSB-CONTRACT.md) and third-party notices.

