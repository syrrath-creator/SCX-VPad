# SCX VPad v0.1.1-preview.1 — native revision 0.1.0.37

Source Guard now supports model-independent validation for native HID gamepad,
joystick, multi-axis and simulation collections, plus the specifically qualified
Steam Controller 2026 private input collection. This is a preview, not a claim
that every device or acquisition backend is protected.

- Physically tested: DualSense Edge over Bluetooth and Steam Controller 2026
  through its USB puck; simultaneous source leases, independent disconnect and
  normal-user operation. The Steam input, mode/keepalive, rumble and trackpad
  haptic handles require the matching rebuilt SCX input implementation.
- The driver/service independently verify source ancestry and collection usage.
  Authorization remains per handle, not a process-wide bypass. Expired or
  released leases cannot regain access; physical input returns after teardown.
- Exact model INF generation rejects broad IDs, malformed entries and keyboard,
  mouse/consumer selections. The unreleased .36 test bound Sony virtual outputs
  too broadly; .37 removes these matches and preserves USB revision qualifiers.
- No change to virtual output layouts, controller containers, mappings or the
  explicit GameInput reconnect workaround. Normal connections require no UAC;
  installing/updating the driver still does.

Limitations: SDL-owned/XInput-only sources are not covered. Generic native HID
models need exact per-device setup and validation; attaching a newly encountered
model is not automatically authorized or installed. DS4 default SDL acquisition
is not newly protected. This driver update alone cannot add the new handle
integration to an older SCX binary. Keep v0.1.0 Stable if you do not need the
preview input integration. Local signing is not WHQL or publisher certification.

# SCX VPad v0.1.0 — native revision 0.1.0.35

The first stable channel for the documented Windows 11 x64 feature set: Xbox
360, DualShock 4, DualSense and input-only generic gamepad, wheel, flight-stick
and arcade-stick outputs. Stable is a release channel, not WHQL certification.

## Reliability and compatibility

- SCX Source Guard reserves the qualified physical DualSense Edge Bluetooth
  gamepad while SCX uses it, addressing physical-plus-virtual duplicate input
  without HidHide. Other physical sources retain their existing behavior.
- Normal connect/disconnect/reconnect operations need no administrator rights.
  Installation/update still requires UAC and creates a local signing certificate.
- The explicit Re-register controllers operation recovers the tested late-start
  GameInput discovery case for four Sony or mixed virtual controllers. Separate
  containers, profiles and stable IDs are preserved. This is a workaround for
  the upstream issue, not an automatic Windows patch or game-specific override.
- Rust authorization policies, bounded leases and per-handle capabilities retain
  source isolation; disconnect, process exit or lease expiry restores access.
  The existing Sony identity policy used by Tekken and Gears is unchanged.

## Performance and safety

- Xbox submit preparation validates/encodes each new packet once and compares
  cached controls, eliminating repeated work while retaining packet numbering,
  strict validation and watchdog behavior. This is not an end-to-end latency claim.
- Undersized HID-read and Xbox-feedback requests are rejected before consuming
  bounded pending-request queue slots.
- Source Guard renewals reuse serial I/O events and pre-encoded fixed responses.
  Guard teardown immediately invalidates cloned access and cancels its worker;
  delayed responses cannot revive released/expired access. Input reports still
  bypass the service management path. Client changes require a rebuilt SDK user.
- Complete package manifests enforce byte lengths, hashes, mandatory files,
  platform and channel. Stable builds require Source Guard and matching native
  DLL/INF versions. Setup verifies the embedded ZIP before protected staging.
- Added regression tests for malformed/repacked installers, identity retention,
  bounded renewal cancellation and changed/expired source leases. CI now builds
  the physical source filter and tests the real graphical installer parser.

## Installing and known scope

Use `SCX-VPad-0.1.0.35-Setup-x64.exe`, or the matching
`SCX-VPad-0.1.0.35-windows-x64.zip`, and compare `SHA256SUMS.txt`. Setup is not
publicly publisher-signed. No private certificate key is distributed, and
Secure Boot/Memory Integrity remain unchanged.

Source Guard is qualified for Edge Bluetooth, not every device or USB Edge.
Generic HID outputs have no force feedback or rumble. Sony outputs do not
implement USB audio, adaptive-trigger programs, console pairing or Bluetooth
radio emulation. An already-running game may require explicit reconnection.
See VALIDATION.md for current artifact evidence and historical comparisons.

# Historical revision 0.1.0.34 candidate

- Own SCX Source Guard for the qualified DualSense Edge Bluetooth gamepad
  collection. Physical input is reserved to authorized SCX handles while connected,
  preventing the confirmed Gears duplicate-input case. No HidHide dependency.
- Normal connect, disconnect and reconnect require no elevation. The existing
  service issues bounded leases; expiry/client exit restores ordinary input.
- Rust policy, 256-bit capabilities per handle, protected transient policy,
  independent device leases, per-client/global quotas, strict request decoding
  and rejection of unqualified source instances. No input-packet service hop.
- Explicit reconnection recovers all four mixed virtual devices for the tested
  late-start GameInput consumer. Profiles, stable IDs and separate containers
  remain intact; no game-specific or shared-PC-container workaround.
- Matching SCX app shows source protection state in all 15 UI languages.
- Source filter is included in package verification, local catalog signing,
  installation ownership journal and scoped uninstall/recovery.

The local source-guard2 build passed real Tekken/Gears user tests and mixed
four-controller Windows checks. Final .34 package installation remains a distinct
qualification step. Source Guard currently matches Edge Bluetooth only; it does
not promise filtering of every controller or hide device enumeration. Locally
signed UMDF2, not WHQL-certified. See VALIDATION.md for evidence.

# Historical v0.1.0-preview.4 — native revision 0.1.0.33

**Development preview, not stable. Known issue:** with GameInput 4.0.302, a
consumer started after four SCX Sony controllers already exist can discover
only two of them. Generic and mixed profiles also show incomplete discovery.
Initial PnP notifications are lost inside the observed GameInput startup path,
before missing devices reach its HID parser. The issue is not fixed in this
release. Shared-container experiments were rejected and are not included.
Do not rely on this preview for multi-controller GameInput use.

This package adds useful generic HID outputs and audited lifecycle/installer
improvements while retaining the original individual device identities. The
earlier installed .33 diagnostic build and the final packaged payload are
different builds: previous hardware/software-device results are not claimed
as a complete installed test of these exact final binaries. See VALIDATION.md.

## Changes

- Configure and verify the service through the Windows Service Control Manager
  API, preserving quoted paths without PowerShell native-argument ambiguity.
  Setup verifies the actual running process, and uninstall checks the live SCM
  configuration before changing mappings or removing the owned service.

- Add Rust-defined wheel, flight-stick and arcade-stick outputs alongside the
  existing generic gamepad. Each has 32 buttons, one hat and six axes; wheel
  steering/accelerator/brake and flight throttle use their HID simulation usages.
- Keep the 64-byte input ABI, authenticated per-lease producer capability,
  bounded input queue, watchdog, quotas and independent device cleanup. Profile
  5 remains reserved; profiles 1–4 keep their existing reports and identities.
- Extend Windows consumer discovery and the SDK to joystick as well as gamepad
  collections; reject feedback for the new input-only profiles. No force-feedback
  promise, arbitrary descriptors, keyboard/mouse device or vendor identity spoofing.
- Add explicit native verification of all four generic classes together,
  descriptor-based Windows axis parsing and per-class security commands.
- Preserve the installed account and device identity seed during GUI upgrades;
  this has also been verified during the installed .33/.32/.33 comparison.
- Remove duplicate Rust state validation/encoding work without bypassing checks
  at the trust boundary. Clean up bootstrap registry material if authenticated
  channel setup fails, and end a lease when its original authenticated process
  exits even if another process retains a duplicated pipe handle.
- Treat a temporarily busy feedback seqlock as contention: wait on the existing
  event within the deadline instead of disconnecting a healthy client when the
  UMDF writer is preempted during publication.
- Keep temporary container/topology diagnostic features out of production.
  Keep owner and identity seed preservation, bounded authenticated transports,
  and the .32 Sony identity policy; do not substitute shared or PC containers.
- Retain explicit development-preview and unsigned-Setup notices, and show the
  known multi-controller GameInput limitation before installation.

## Validation and installation scope

Preview preparation runs the offline Rust workspace tests, formatting and
strict Clippy checks, the no_std/native ABI build, both InfVerif checks,
catalog generation, isolated installer/recovery tests and embedded-Setup
payload verification. These are not WHQL certification, an all-games guarantee,
or proof that the final package has completed a fresh live installation test.
Earlier .33 installed tests and the unresolved late-consumer case are recorded
separately in VALIDATION.md.

Use `SCX-VPad-0.1.0.33-Setup-x64.exe` for the graphical installer, or the matching
`SCX-VPad-0.1.0.33-preview.4-windows-x64.zip`. The EXE embeds exactly that ZIP;
compare the downloaded files with `SHA256SUMS.txt`. Setup is not publicly
publisher-signed. Its installer creates a local nonexportable driver-signing
certificate with explicit administrator approval; it does not disable Secure
Boot or Memory Integrity. Normal controller connections do not elevate.

## Native revision 0.1.0.32 — Sony compatibility and reliable updates

- Give each virtual Sony controller a stable, separate Windows container.
  This corrects the tested Tekken 8 case where DualSense was recognized but
  ignored input. The same policy also received a positive Gears user test.
  No game-specific detection or switching is involved.
- Preserve the existing public Sony identity, authenticated input channel,
  per-device ownership and Xbox/Generic container scheme. Unit tests cover
  stability and separation across installation seeds, profiles and slots.
- Keep the native Sony initialization, calibration and packet corrections
  accumulated during the .30/.31 compatibility investigation. Diagnostic ROOT
  creation remains explicitly opt-in and is not included in the normal build.
- Preserve the original Windows account and all 64 bits of the device identity
  seed when the graphical Setup replaces an existing installation. This keeps
  device identities stable through updates.
- Includes the .29 Rust queue/feedback cleanup described below. No new latency
  or universal game-compatibility claim is made.

Validation: isolated virtual-only Tekken menu test and a 600-second physical
Edge forwarding run completed; the user confirms Tekken and Gears input.
The .32 upgrade preserves the authorized account and identity seed. All eight
installed Windows integration suites pass, including a clean repeated
four-controller Sony test. The external GameInput identity probe now requests
the documented gamepad kind instead of an all-bits mask; strict device-count
expectations are unchanged. Build checks, 78 Rust tests, 32 Setup checks and 21
replacement-identity tests pass. See VALIDATION.md for evidence and scope.

## Native revision 0.1.0.29 — cleanup candidate

- The real HID input queue, replay checks and valid-field feedback merging now
  run in the allocation-free Rust core, with native C-to-Rust regression tests.
  The former standalone Rust queue model and duplicated C ring/merge are removed.
- Decode each authenticated HID input once. Remove permanent bring-up counters
  and eleven atomic diagnostic stores per Sony submission. No measured latency
  improvement is claimed; the existing shared-channel ABI remains compatible.
- Remove unreachable Sony HID feedback handlers and their unused WDF queue.
  Sony feedback continues through its owned sideband channel; the active Xbox
  feedback queue remains. Sony's public HID still rejects private SCX reports.
- Remove legacy unauthenticated HID diagnostics and Xbox diagnostic counters.
  Opt-in payload-free Sony ETW remains available; no production input tracing.
- Split shared-channel and ETW implementations into independent C files;
  separate GetFeature/GetInputReport, name profiles/layout constants and use
  consistent codec error mapping. Document UMDF's report-ID length convention.
- Preserve watchdog neutralization without resetting replay protection. Only
  a fresh device/lease resets sequence state. Short press/release transitions,
  capacity/backpressure and independent device ownership remain intact.
- Package and installer versions derive from the native DLL/payload manifest.

Candidate status: build/static analysis/native ABI, 68 Rust tests and all eight
installed Windows integration suites pass. The .28-to-.29 upgrade preserves the
authorized account and device identity. Four independent Sony controllers pass
HID/WGI/GameInput checks, including isolated feedback and removal. In-game and
physical-controller forwarding have not been retested for this revision;
preview.2 (.28) remains the published release. See VALIDATION.md for scope.

# SCX VPad 0.1.0-preview.2 — native revision 0.1.0.28

- Use ROOT-compatible container metadata for DS4/DualSense while retaining
  independent device paths, Sony identities, producer capabilities and leases.
  Xbox and generic HID retain their per-controller containers.
- This policy restores PlayStation symbols and single menu navigation in the
  tested Gears setup. The user also confirms game rumble and output switching.
- A 7m39s recorded reproduction attempt included multiple focus changes and an
  intentional Sony/Xbox/Sony switch: 42,809 captured requests, no failures or
  event loss. One earlier transient recurrence was not reproduced; no universal
  compatibility or permanent resolution of that isolated episode is claimed.
- Add independent four-controller DS4, DS5 and mixed-layout validation through
  HID, Windows.Gaming.Input and GameInput v3, including feedback isolation and
  removal of one controller without disrupting the others.

Compatible with the .24+ Sony-channel client, including SCX 2.1.2. Update the
driver, service and app together when coming from preview 1. The installed final
.28 package passes generic/Xbox/Sony input, feedback, multi-controller and access
boundary tests. Source tests: 68 passing; native analysis, ABI, INF and catalogue
checks pass. See VALIDATION.md for exact evidence and remaining scope.

The separate installer requests administrator approval once. Normal controller
connections do not elevate. No private signing key is shipped and Windows Secure
Boot/Memory Integrity settings are not changed. This remains a locally signed
development driver, not WHQL-certified or universally anti-cheat compatible.

Earlier development entries below record the investigation, not unresolved
requirements of the final .28 package unless repeated in its validation limits.

# Previous local candidate — native revision 0.1.0.27

- Accept mixed Sony output commands used by native games. Unsupported audio
  and adaptive-trigger fields no longer cause the entire rumble/light packet
  to fail. Report IDs and sizes remain strictly validated.
- Preserve field-wise feedback merging and motor stop delivery. Audio and
  adaptive trigger engines remain unsupported; no physical configuration
  command is forwarded to the input controller.
- Retain .26's public Sony initialization fix. The real-game .26 trace proves
  that Gears passed that step and then encountered this second rejection.

Local candidate only; in-game verification pending.

# Previous local candidate — native revision 0.1.0.26

- Accept the public Sony configuration commands that Gears actually sends:
  DS4 feature 0x14 (17 bytes), DualSense feature 0x08 (48 bytes). Previously
  these were incorrectly routed into the private input authorization handler.
- These radio-configuration commands are no-ops for a virtual USB controller.
  Unknown/private writes remain denied; input and feedback are unaffected.
- Add a native Windows initialization regression check and Rust/ABI boundary
  coverage. Retain .25's opt-in, payload-free diagnostic provider.

Compatible with the .24 Sony-channel SDK. Local candidate only; live Gears
verification pending. Revision .25's trace confirms the rejected calls came
from Gears, with no dropped trace events.

# Previous local candidate — native revision 0.1.0.24

- Remove the entire private HID collection from Sony profiles. Each Sony device
  now exposes exactly its one canonical application collection, as Maestro does.
- Move authenticated input, feedback and diagnostics to a per-lease, 512-byte
  service-created shared section with blocking events and explicit acknowledgement.
  No polling, privileged per-report service hop or unbounded shared indices.
- Preserve capability validation, replay rejection, watchdog, independent leases
  and rumble/light feedback. An outstanding/timed-out packet cannot be overwritten.
- Private objects are restricted to SYSTEM, UMDF and the authorized owner, with
  a medium-integrity write boundary. The public Sony HID rejects SCX injection.

Driver, service and client must be updated together. This is a local candidate,
not a confirmed Gears fix. The user confirmed .23 still shows Xbox symbols.

# Previous local candidate — native revision 0.1.0.23

- Replace simplified Sony public HID collections with the matching DS4 v2 and
  DualSense USB compatibility descriptors from HIDMaestro (MIT, attribution
  included). Restore complete feature declarations and vendor usages.
- Use its documented firmware-information responses, including valid DualSense
  firmware type/version fields and the dependent Bluetooth-patch-info query.
  Correct the DS4 firmware build-time offset. This is metadata, not firmware
  flashing or added audio/adaptive-trigger support.
- Expose a unique, stable HID serial for each SCX device, matching its public
  instance identity without exposing its separate input capability.
- Make the private Sony control collection feature-only. Feedback now uses a
  bounded, cancellable feature wait instead of a second HID input collection.
  The updated SDK also supports the previous driver's feedback reader.
- Match the public DualSense 48-byte HID output length while retaining the
  previous padded 63-byte parser format.

**Requires an app rebuilt with the updated SDK for Sony feedback.** Do not
install this candidate under the old SCX app binary. Xbox is unchanged.
The user still saw Xbox symbols with revision 0.1.0.20. Revision .23 passes the
Windows input/feature/rumble/feedback tests, including timeout recovery. It is
not yet a released or game-confirmed fix. See VALIDATION.md.

# 0.1.0-preview.2 — Windows controller compatibility (native revision 0.1.0.18)

- Admit the Xbox function device through Windows' Xbox setup class. Provide
  asynchronous Windows Gaming Input reports, including discovery/active-sink
  transitions. Input wakes consumers on changes, with no continuous idle pump.
- Support the observed four-byte WGI input requests and nine-byte WGI output
  commands alongside the existing XInput formats. Keep producer access separate.
- Publish complete Sony vendor/product hardware IDs and PlayStation button
  labels, with journaled installation/removal of the label mappings.
- Stop DualSense emulated rumble when SDL sends an empty effects packet;
  verify nonzero rumble and stop through SDL for both native Sony profiles.
- Add opt-in native WGI inventory/rumble diagnostics and independent async-input
  regression tests for idle, cancellation, watchdog, multiple readers and removal.

The earlier 0.1.0.8 input correction below was confirmed in Gears by the user.
Sony identification and in-game rumble in Gears still require a new retest.
Windows API results are tracked separately in VALIDATION.md.

- Correct Sony HID semantics: X/Y and Z/Rz describe the sticks, Rx/Ry describe
  the independent triggers. Previously descriptor-based consumers interpreted
  trigger values as stick positions, causing camera rotation and cross-talk.
- Explicit Windows Gaming Input Gamepad and UI-navigation mappings for the
  DS4/DualSense USB identities. Cross/A, Circle/B, Square/X, Triangle/Y,
  Options/Menu, Share/View and stick clicks use the correct indices.
- Preserve and restore pre-existing mapping values during installation/removal;
  later external edits are retained. No per-report registry work or new polling.
- Add semantic HID regression tests, a Windows HidP control probe, an independent
  Windows Gaming Input consumer and real-registry installer recovery tests.
- Existing SCX 2.1.1 clients keep working without rebuilding or changing profiles.
  Close SCX and games, uninstall the previous preview, then install this package.
  The installer needs UAC; ordinary controller connections remain unelevated.

This addresses reproduced Windows API failures. Game-by-game confirmation is recorded
in VALIDATION; it is not a claim that every game supports native Sony controllers.

# 0.1.0-preview.1

First independently implemented SCX VPad driver and Rust client SDK.

- Rust core for fixed-size input validation, controller state and protocol
  encoding, with a small native UMDF2 binding.
- Generic HID gamepad plus experimental Xbox/XInput, DualShock 4 and DualSense
  profiles. Xbox uses its own function device and XUSB contract.
- Up to 16 concurrent virtual controllers; four Xbox/XInput slots.
- Privileged installation and device management isolated from normal clients.
  Connecting a controller does not prompt for administrator rights.
- Direct per-device input submission, event-driven feedback, bounded queues,
  stale-input neutralization and cleanup when a client disconnects or exits.
- Original SDK, command-line tests, independent Microsoft XInput consumer,
  locally signed installation, scoped removal and repeatable build scripts.
- Source-available license: commercial distribution requires prior written
  permission from Tim Haller, tim.haller@gmail.com.

This is a development preview, not WHQL certification or a universal
game/anti-cheat compatibility claim. Read README, SECURITY and VALIDATION before
installation. No signing private key is shipped. Existing HIDMaestro, ViGEmBus
and SCX application installations are not replaced by this package.
