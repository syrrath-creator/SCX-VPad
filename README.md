# SCX VPad

An independently implemented Windows virtual-controller driver with a Rust core,
a small UMDF2 binding, and a restricted Rust service for device management.

**Development preview 2 — native driver 0.1.0.28.** Independently implemented architecture, with
attributed MIT Sony compatibility tables from HIDMaestro. This is an experimental
driver and SDK, not a production-certified
drop-in replacement. Its Windows validation and remaining compatibility gaps
are recorded separately from protocol unit tests.

## Architecture

The administrator installs the driver and service once. The installer creates a
nonexportable machine-local signing key and trusts its public certificate locally.
It does not enable test signing or turn off Secure Boot or Memory Integrity.
Normal applications communicate with the service without elevation. The service
retains privileges only to create and remove the allowlisted SCX devices.

Controller state travels directly from the native client to the UMDF2 driver,
using an authenticated per-device shared section for Sony, a vendor HID
collection for generic HID, or the restricted Xbox control interface. The
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
SCX calibration and per-device identities, plus attributed Sony USB compatibility
descriptors and firmware-information tables from HIDMaestro (see notices).
Native USB audio, adaptive trigger effects, Bluetooth radio emulation, console
pairing and Steam-controller output are not implemented. The generic profile has
no rumble protocol. Revision 0.1.0.28 includes Windows Gaming Input and GameInput
compatibility corrections, Sony initialization and mixed-output handling, plus
ROOT-compatible Sony container metadata with distinct per-device identities.
The tested Gears of War: E-Day setup recognizes PlayStation input and game rumble.
Four Sony controllers (DS4, DS5 and mixed) pass independent WGI/GameInput v3 input,
identity, rumble-isolation and partial-removal tests. See VALIDATION for scope and
the single earlier transient that could not be reproduced in the recorded run.
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

### Recommended: Windows Setup EXE

Download **[SCX-VPad-0.1.0.28-Setup-x64.exe](https://github.com/syrrath-creator/SCX-VPad/releases/download/v0.1.0-preview.2/SCX-VPad-0.1.0.28-Setup-x64.exe)**
from the existing Preview 2 release. Its separate `Setup-SHA256SUMS.txt` asset
contains the EXE checksum. No ZIP extraction or terminal commands are needed.

1. Close SCX and games. Start the EXE by double-clicking it from the Windows
   account that will use SCX.
2. Read the license and local driver/certificate notice, confirm consent and
   select **Install**. Approve the Windows administrator prompt.
3. Wait for the success message, then open SCX, select **SCX VPad** and connect.
   Restart Windows first if the installer requests it.

The EXE installs the driver **and** its service using the unchanged, verified
Preview 2 package. It includes German and English interfaces and works offline.
If already installed, **Reinstall** replaces this revision and retains the
authorized Windows account. Driver updates still require administrator approval;
ordinary controller connections do not. SCX application profiles are not changed.

For removal, run the EXE again and choose **Uninstall**. Setup is not registered
as a separate entry in Windows Installed apps. **Copy log** copies diagnostics;
failed operations retain a protected setup log at the location shown in the UI.

The Setup EXE has no public publisher signature, so Windows may display an
unknown-publisher warning. Its local driver certificate does not give the EXE a
globally trusted signature. This remains a development preview, not WHQL-certified.
Neither the EXE nor its driver is included in the SCX Store package.

### Advanced: manual ZIP installation

Download the Windows x64 ZIP from [Releases](https://github.com/syrrath-creator/SCX-VPad/releases)
and compare its SHA256 with the accompanying checksum file. For a downloaded
ZIP, review its source and use Windows **Properties → Unblock** before extracting
if Windows marks the unsigned development scripts as downloaded. Do not weaken
machine-wide execution policy. Open 64-bit Windows PowerShell in the extracted
package and run:

```powershell
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File .\installer\Install.ps1
```

Run this as the intended interactive user. The script requests administrative
elevation once and carries
that user's SID into the service configuration. Initial setup authorizes one
local/domain/MS-account Windows user; changing that owner is administrative.
It installs only SCX VPad under
`Program Files\SCX VPad`, records its own driver package and certificate, and
does not change existing controller-driver installations.

Sony Gamepad and UINavigation mappings are installed for USB IDs 054C:09CC and
054C:0CE6. Windows scopes these mappings to VID/PID/usage, so they also affect
matching physical USB controllers. Their layout follows native Sony axes/buttons;
DualSense Edge and Bluetooth identities are not modified. Previous values are
journaled in the protected installation record. Uninstall restores prior values
only while they still match SCX's installed values, preserving later external edits.

The signing key is local, nonexportable and restricted to administrators and
SYSTEM. Installation failures are recorded under `Program Files\SCX VPad\logs`.
Copy a failure log before uninstalling; removal deletes that installation folder.
Alternatively, use the packaged `installer/Uninstall.ps1` for removal. Driver/service upgrades
remain administrative operations; ordinary controller connections do not.
The installed removal script is also retained under
`C:\Program Files\SCX VPad\installer\Uninstall.ps1`.

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
SCX 2.1.2 recommends this backend. Installation does not modify application
profiles. Update the driver, service and client together; old Sony clients from
before native .24 are incompatible with the current private producer channel.
Recorded results and remaining test coverage are in [VALIDATION.md](VALIDATION.md).

### Build the Setup EXE

The small WinForms shell uses the .NET Framework included with Windows 11. It
wraps the Rust driver/service package; it does not rebuild or change them.
Build from an already validated package and supply its independently verified hash:

```powershell
./scripts/build-setup.ps1 -PackageZip ./SCX-VPad-0.1.0.28-preview.2-windows-x64.zip -ExpectedSha256 ad7e32670e7110fe1a71a7b8601c2389d66b9b65b06b3197639801b006604cd2 -OutputDirectory ./artifacts/setup
./scripts/test-setup.ps1 -PackageZip ./SCX-VPad-0.1.0.28-preview.2-windows-x64.zip -BuildDirectory ./artifacts/setup
```

The published Setup is an additional Preview 2 asset. The original ZIP and tag
are unchanged; see the release notes for the Setup source commit.

## License and commercial distribution

Copyright 2026 **Tim Haller**. Source is available under the [SCX Source Available
Noncommercial Distribution License](LICENSE). Use and modification are allowed,
including internal organizational use. **Commercial distribution, paid bundling
and commercial hosted access require Tim Haller's prior written permission.**
Contact **tim.haller@gmail.com**.

This commercial-distribution restriction means the project is source available,
not OSI-approved open source. Third-party components keep their own licenses.
The architecture is independently implemented; no HIDMaestro driver or SDK
is bundled. Attributed MIT Sony compatibility tables are included. See [security boundaries](SECURITY.md),
[protocol provenance](crates/scx-vpad-core/PROVENANCE.md),
[XUSB contract](crates/scx-vpad-core/XUSB-CONTRACT.md) and third-party notices.

