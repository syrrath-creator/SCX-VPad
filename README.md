# SCX VPad

**v0.1.1-preview.2 · native revision 0.1.0.38 · Windows 11 x64**

This preview improves Source Guard's internal-panic handling, reduces redundant
Sony client payload barriers and adds independently observed HID latency tooling.
It retains the qualified native HID and Steam Controller 2026 coverage from .37.
Read-only SDK discovery reports Source Guard preparation/status and validates
exact model identifiers; it does not automatically install or authorize devices.
The client optimization requires an application rebuilt with this SDK; installing
the driver alone does not update the client inside an existing SCX Store app.
No end-to-end latency win is claimed. The stable v0.1.0 / .35 release remains
available. See [RELEASE-NOTES.md](RELEASE-NOTES.md) for scope and requirements,
[VALIDATION.md](VALIDATION.md) for the measured results, and the
[benchmark guide](docs/latency-benchmark.md) for reproducible methodology.

Virtual Xbox 360, DualShock 4, DualSense and generic HID controllers for SCX.
The driver core, source-authorization policy, client SDK and device-management
service are written in Rust. Small C bindings integrate with Windows UMDF2;
the optional graphical Setup shell uses C#/.NET Framework.

The stable channel covers the documented, tested feature set below. It is not
Microsoft WHQL certification, a promise of every game/device, or a replacement
for the compatibility limits in [VALIDATION.md](VALIDATION.md).

## What is included

| Output | Controls and feedback |
| --- | --- |
| Xbox 360 compatible | XInput buttons, sticks, triggers and basic game rumble |
| DualShock 4 / DualSense | USB-style HID input, motion, two touch contacts, basic rumble and supported LED feedback |
| Generic gamepad | 32 buttons, one hat, four signed axes and two unsigned trigger axes |
| Wheel / flight stick / arcade stick | Input-only HID profiles with 32 buttons, one hat and six fixed axes; class-specific axis usages |

SCX supports up to four virtual controllers per client process. The service has
a global limit of 16 devices and a separate global limit of four Xbox outputs,
matching XInput's slot limit. Controllers retain independent input, feedback,
ownership and stable Windows identities, including in mixed-profile use.

Generic outputs do not implement force feedback or rumble. Sony compatibility
does not include adaptive-trigger programs, USB audio, Bluetooth radio
emulation, console pairing or Steam-controller output. Physical-device discovery
and remapping belong in SCX; the virtual driver does not add hardware support
for an arbitrary legacy adapter or vendor protocol.

## Four controllers and the GameInput workaround

GameInput 4.0.302 can miss devices when a consumer starts after several virtual
controllers already exist. SCX VPad supplies an explicit reconnect operation;
the matching SCX app exposes **Controller neu anmelden / Re-register controllers**.
Use it while the game is running if a controller is missing. Release buttons
and sticks during the short reconnection.

This is a tested workaround for the upstream discovery issue, not a claim to
have patched Windows. It recreates the owned device handles while retaining
profiles, stable IDs and separate Windows containers. It does not silently
restart Windows services, merge controllers into the PC container, reduce the
controller count or poll/reconnect continuously. See the installed-test record
for early/late consumer and mixed-controller coverage.

## Source Guard: prevent physical + virtual duplicate input

SCX Source Guard is an original, device-specific UMDF2 upper filter. Physically
tested sources in this preview are **DualSense Edge over Bluetooth** and
**Steam Controller 2026 through its USB puck**. During an authorized SCX
connection it reserves that physical gamepad's input/output for authenticated
SCX handles, so games consume the virtual output instead of both devices.
The matching SCX app reports when protection is active.

No HidHide dependency or new SCX kernel-mode binary is installed. The filter
uses Windows' existing UMDF infrastructure. It is not a HID class-wide filter,
a keyboard/mouse filter or a claim to hide every controller. The physical
controller may remain visible in device enumeration; access is restricted only
while its lease is active. Other sources retain their normal behavior.

A short per-device lease and random per-handle capabilities authorize access.
Disconnect, client exit or lease expiry restores ordinary access. Controller
reports travel directly to/from the native client; the service is not a
per-input-packet intermediary. Initial installation/update is administrative;
normal connections, disconnections and explicit reconnection do not elevate.

## Install

Download **[SCX-VPad-0.1.0.38-Setup-x64.exe](https://github.com/syrrath-creator/SCX-VPad/releases/download/v0.1.1-preview.2/SCX-VPad-0.1.0.38-Setup-x64.exe)**
from [the release](https://github.com/syrrath-creator/SCX-VPad/releases/tag/v0.1.1-preview.2).
Compare its checksum with `SHA256SUMS.txt`.

1. Close SCX and games. Run Setup from the Windows account that will use SCX.
2. Read the license/local certificate notice, consent, and select Install.
3. Approve Windows' administrator prompt once. Wait for success; restart Windows
   first if requested, then open SCX and connect using SCX VPad.

Setup works offline and embeds exactly the matching hash-verified driver ZIP.
Reinstall/update preserves the authorized Windows account and device identity
seed. It does not modify SCX profiles. To remove the driver and service, run the
same Setup and choose Uninstall. Setup is not separately registered in Windows
Installed apps. Copy the displayed log before uninstalling a failed installation.

The Setup EXE is **not publicly publisher-signed**; Windows may show an unknown
publisher. Installation creates a nonexportable machine-local signing key and
trusts its public certificate locally. No private key is shipped. Local signing
does not provide public Authenticode trust or WHQL certification. Setup does
not disable Secure Boot, Memory Integrity or driver-signature enforcement and
does not enable test-signing mode. The installer is separate from the SCX Store app.

Advanced ZIP installation: extract the matching Windows x64 ZIP, verify its
checksum, and run the packaged installer as the intended interactive user:

```powershell
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File .\installer\Install.ps1
```

If Windows marks a downloaded ZIP, review its source and use Properties →
Unblock before extracting. Do not weaken machine-wide execution policy. The
script requests UAC itself. Setup installs only its allowlisted driver packages
and restricted service under `Program Files\SCX VPad`, recording ownership for
scoped removal and recovery of partial imports.

Sony Gamepad/UINavigation mappings use USB IDs 054C:09CC and 054C:0CE6. Windows
scopes them by VID/PID/usage, so matching physical USB Sony controllers also see
these native-layout mappings. Previous registry values are journaled and are
restored on uninstall only if they still match SCX's installed values. This is
separate from the per-device Source Guard filter.

## Architecture and security

The Rust service authorizes one configured Windows user, authenticates local
management clients and grants bounded per-device leases. It accepts no client
commands, executable paths, arbitrary HID descriptors or network requests.
The account is the security boundary, not mutually hostile programs running as
that same account. See [SECURITY.md](SECURITY.md).

Input uses a private authenticated shared section for Sony, an authenticated
vendor HID collection for generic profiles, or the restricted Xbox interface.
Fixed-size packets, independent queues, backpressure, strict sequence and bounds
checks, short leases and stale-input neutralization keep work bounded. Refresh
held non-neutral state at least every 250 ms. Ordinary submit calls do not route
through the privileged service, allocate variable-sized state or sleep on a
fixed polling timer.

There is no claim of universal anti-cheat compatibility or the fastest physical
controller-to-screen latency. API submission benchmarks exclude physical
transport, game polling and display delay. Performance results must state what
was measured and which exact binaries were used.

## Build

Target: Windows 11 x64, build 22000 or newer. Windows 10 and ARM64 are not
packaged or validated. Install Rust stable MSVC (rustfmt, clippy, rust-docs),
Visual Studio C++ tools, Windows SDK 10.0.26100.0 and the WDK. The official
Microsoft.Windows.WDK.x64 NuGet package 10.0.26100.6584 is supported; set
`SCX_WDK_ROOT` to its extracted `c` directory. Microsoft build tools are not
redistributed in the package.

```powershell
cargo fetch --locked
./scripts/fetch-wdk.ps1
./scripts/cargo.ps1 -CargoArgs @('test','--workspace','--locked','--offline')
./scripts/build-package.ps1 -IncludeSourceGuard -SourceModelFile ./isolation/models-qualified.txt -ReleaseChannel preview -Destination ./dist/SCX-VPad-0.1.0.38-windows-x64
```

The stable package requires Source Guard and cannot skip tests. The build checks
Rust code, installer recovery, replacement identities and package validation;
it separately builds the no_std Rust driver core and Source Guard policy,
compiles both UMDF2 bindings, runs InfVerif and Inf2Cat, verifies matching native
versions and produces a file-hash manifest. Building changes no Windows trust,
drivers or services. Installed tests remain a separate release qualification.

ZIP the package contents with `package-manifest.json` at archive root, then:

```powershell
./scripts/build-setup.ps1 -PackageZip ./SCX-VPad-0.1.0.38-windows-x64.zip -ExpectedSha256 $verifiedZipSha256 -OutputDirectory ./artifacts/setup38
./scripts/test-setup.ps1 -PackageZip ./SCX-VPad-0.1.0.38-windows-x64.zip -BuildDirectory ./artifacts/setup38
```

The Setup tests exercise the real package reader against modified archives;
they do not install drivers, change certificates or access the live registry.
[SDK.md](SDK.md) documents client integration and direct control semantics.
[VALIDATION.md](VALIDATION.md) separates current evidence from historical tests.

## License and provenance

Copyright 2026 **Tim Haller**. Source is available under the [SCX Source Available
Noncommercial Distribution License](LICENSE). Use and modification are allowed,
including internal organizational use. **Commercial distribution, paid bundling
and commercial hosted access require Tim Haller's prior written permission.**
Contact **tim.haller@gmail.com**.

The commercial-distribution restriction means source available, not OSI-approved
open source. Third-party components retain their licenses. This is an independent
implementation, not a HIDMaestro driver/SDK bundle. Attributed MIT Sony USB
compatibility tables are included. See [provenance](crates/scx-vpad-core/PROVENANCE.md),
[XUSB contract](crates/scx-vpad-core/XUSB-CONTRACT.md) and third-party notices.
