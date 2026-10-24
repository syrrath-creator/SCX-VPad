# Source Guard .37 preview validation — 24 October 2026

Windows 11 x64, ordinary-user test clients after one administrative installation.
This section is for the .37 preview; historical Stable evidence below remains
labelled with its original revision. No end-to-end latency claim is made.

- Installed DLLs verified against the candidate; authorized user and device
  identity seed preserved. Generic input/classes, Sony input, Xbox feedback and
  security gates for generic/wheel/flight/arcade/DS4/DS5/Xbox: PASS.
- Two consecutive 120-second parallel runs, Steam Controller 2026 via USB puck
  to Xbox output and DualSense Edge BT to DualSense output: PASS. Source Guard
  active on both throughout. No reported input/feedback failures, competing
  source claim rejected, second route continues after first disconnect, both
  stop cleanly. The targeted run observed 31,564 Steam and 57,603 Edge reports;
  both R2 maxima 32,767 and 237/92 sampled m:n combinations. Edge touch: 677
  reports. User confirmed Steam trackpad mouse AIM and rolling haptics.
- Live virtual Sony output bound to input.inf, not the Source Guard INF.
  The unreleased .36 candidate's broad USB matches also attached to virtual
  outputs and its prolonged parallel test failed with ERROR_BUSY. Those matches
  were removed; the .37 tests above passed. This failed candidate was not shipped.
- Client unit tests: 25 passed, 2 opt-in installed tests ignored. SCX input tests:
  48 passed, 3 diagnostic/hardware tests ignored. Rust policy, package checks,
  Clippy, native /W4 /WX /analyze, InfVerif and catalogue generation passed.

Coverage is deliberately bounded: physical hardware tested here is Edge BT and
Steam Controller 2026 puck. Generic HID support is an installed-model/SDK path,
not a test of every controller. SDL-owned and XInput-only sources are not covered.
Tekken/Gears were not manually replayed during this preview run; their existing
container/layout behavior is unchanged. The late GameInput discovery limitation
remains and still requires the documented explicit reconnect workaround.

# Windows validation record

## Exact .35 installed release gate — 24 October 2026: PASS

The following immutable release files were installed and checked on Windows 11
x64. This is a new run against the actual .35 payload, not a relabelling of
historical .33/.34 results:

- `SCX-VPad-0.1.0.35-Setup-x64.exe` SHA-256:
  `3ecd395ccc3a9faf3952b2565da39c0a93359dd0004a1a3eb81dd6831dc57971`
- Embedded `SCX-VPad-0.1.0.35-windows-x64.zip` SHA-256:
  `28fbb1695cf33ee69d7a2f62cf0a44d341e516faf9c673a2090783d7404a1633`
- Installed `ScxVPad.dll`:
  `f6aebb3db031703ce2a76b41925c08141e35c6faee632442645d61b949eb76ef`
- Installed `ScxSource.dll`:
  `4c759d5e14da699d7811fe7b96029f479d83f7424720f2e07af838b68abd3973`

The EXE's own embedded payload and replacement script were used for the
administrative installation. The old separately journaled local development
filter was removed first; the new installer now owns all three INF packages.
The original authorized Windows account and complete identity seed were
preserved. The service is running. Normal-user checks then passed:

- Generic input, 32-button wheel/flight/arcade profiles, Sony input/initialization/
  calibration/touch-capable reports, HID rumble/RGB, Xbox XInput input/rumble,
  neutral/watchdog behavior and owned-device cleanup.
- Security checks for all seven output profiles: effective access with restricted
  and low-integrity tokens, capability and bootstrap rejection where applicable.
- 80 undersized HID reads and 80 undersized Xbox feedback waits: immediate
  rejection, no output-buffer modification, followed by valid reads and an idle
  feedback wait whose cancellation was drained. HIDClass rejects the small HID
  buffer before UMDF (Win32 1784); the Xbox path returns Win32 122. This does not
  misrepresent the HID result as direct execution of the UMDF-internal branch.
- GameInput 4.0.302: seven create/reconnect/remove cycles across six layouts:
  four DualSense (twice), DualSense/DS4/Xbox/wheel, generic/wheel/flight/arcade,
  two DS4 plus two DualSense, DS4/DualSense/two Xbox, and four Xbox. Each had an
  early and a late independent consumer. Both saw all four distinct streams
  after explicit reconnect; each slot kept its independent container/identity.
- Separate four-DS5, four-DS4 and two-plus-two Sony tests: distinct public HID
  states, isolated motor pairs and RGB, RGB-only merging, individual motor stops,
  and removal of each controller without disrupting its siblings.
- Physical Bluetooth Edge Source Guard: a normal-user authenticated handle read
  5,570 reports in 12 seconds while unauthorized reads and duplicate ownership
  were denied. Release restored ordinary access. Killing the owning test process
  also restored access: 40 reports without errors after a 500-ms cleanup pause.
- The unchanged SCX 2.1.5 release app forwarded 9,022 physical Edge reports over
  20 seconds to a virtual DualSense, with clean stop and no input/output/haptic
  error. A concurrent unauthenticated reader received access denied; ordinary
  access returned after the app stopped. User profiles were not changed.

The prior manual Tekken/Gears success remains evidence for the unchanged
independent-container/Source Guard design. Those games were not freshly played
for this .35 gate. No claim of universal game or anti-cheat compatibility is made.
The upstream late-start GameInput defect still requires the documented explicit
reconnect; the gate tests the workaround, not disappearance of that Windows bug.

Build gates passed: 98 workspace Rust tests (two opt-in hardware tests skipped
in that unit run), 11 isolated Rust source-policy tests, strict Clippy/formatting,
no_std/native ABI/static analysis, three INF and two catalogue checks; 46 Setup,
23 package, 11 installer-recovery and 21 identity checks. PowerShell validation
also ran under Windows PowerShell 5.1. Local diagnostic tokens, real account
SIDs, device-instance paths and signing keys are excluded from public artifacts.

CPU-only Xbox preparation measured 40.59 to 18.99 ns/report (11 alternating
samples): reduced work in that segment, not controller-to-game latency. Separate
Windows API-completion runs used the identical client executable before/after,
three 10,000-report runs per profile on the active development machine. Median
ranges (microseconds): generic 20.3–34.1 / 26.8–33.5; DS4 45.3–46.1 / 51.2–53.5;
DS5 46.0–46.5 / 49.4–49.9; Xbox 18.7–29.0 / 23.0–32.1. These sequential noisy
runs do not demonstrate an overall latency improvement; the Sony timings were
higher in this run. No marketing latency/CPU/RAM percentage is derived from them.

The ZIP/EXE remain byte-for-byte unchanged after this qualification. Their bundled
validation document describes the release gate before execution; this repository
record and the accompanying qualification asset supply the final measured result.

## Native .35 release qualification

The v0.1.0 release combines the Source Guard/reconnect behavior documented below
with the audited .35 Rust service/client and UMDF changes. The current release
must be identified by its Setup/ZIP checksums and native .35 payload hashes;
historical .33/.34 installed observations do not establish exact-.35 validation.
Artifact-specific final installation and mixed-controller results accompany the
release qualification record. Publication requires that installed gate to pass.

The .35 isolated review covers real no_std/C ABI builds, INF/catalog checks,
Xbox controls-cache equivalence and watchdog tests, undersized-request handling,
cancellable Source Guard renewal, teardown revocation and process-exit cleanup.
Package/Setup tests exercise the actual manifest reader with corrupt lengths,
hashes, metadata, missing filter/catalogue files, archive traversal and duplicate
entries. Synthetic tests perform no driver, registry or certificate changes.

Performance statements are intentionally narrow: the Xbox preparation
microbenchmark compares repeated old/new encoding work in-process. It excludes
WDF, Windows scheduling, physical transport, game sampling and display delay.
Renewal event/buffer reuse is a removed allocation, not a measured claim that
whole-app CPU, RAM or controller-to-screen latency improved by that percentage.

Live qualification must retain individual Sony containers and cover four Sony
devices, Sony/DS4/Xbox/generic mixes, early/late GameInput consumers with explicit
reconnect, independent inputs/feedback, Source Guard expiry/owner exit and normal
unelevated SCX operation. The manual Tekken/Gears observations below establish
the preceding behavior; they are not silently relabelled as a new test run.

Stable channel status is limited to the documented feature set. It does not
mean WHQL, automatic correction of the upstream GameInput bug, universal source
isolation or all-game/anti-cheat compatibility.

## Source Guard and explicit reconnect — 24 October 2026

The local source-guard2 service/app build adds an original device-specific UMDF2
upper filter with allocation-free Rust authorization policy. It matches only the
physical DualSense Edge Bluetooth gamepad collection. It is not HidHide, a HID
class filter, a keyboard/mouse filter, or a universal physical-device hiding claim.
Windows may still enumerate the physical device; unauthorized input/output
requests on that device are denied while a source lease is active.

Installed Windows evidence under `isolation/`:

- Inactive filter: 40 physical reports, no errors; normal access retained.
- Wrong capability and second unauthenticated handle: access denied (5).
  The explicitly authorized handle continues reading. Expired capabilities
  cannot authenticate a handle. A 3-second lease expires without renewal.
- Normal, non-elevated SCX client: 3023 physical reports over six seconds;
  duplicate ownership rejected; renewal outlives the first TTL; disconnect
  restores the same independent reader. The account cannot read protected keys.
- Killing only the owned test client restores physical access: lease zero and
  40 reports with no errors 500 ms later. No service restart or UAC required.
- Real SCX engine: Edge Bluetooth to virtual DualSense, 20 seconds, 9888 reports,
  no input/output warnings, errors or haptic errors, clean shutdown. Dispatch
  timing in this log starts after physical read and is NOT end-to-end latency.
- Active source protection during four-device GameInput testing: DualSense,
  DualShock 4, Xbox and wheel. Early/late consumers receive all four independent
  input streams after explicit reconnection; device/container identities are
  distinct and unchanged. Sony/Sony/DS4/DS4 and Sony/DS4/Xbox/Xbox also pass.
- User confirms Tekken 8 and Gears of War: E-Day with Edge Bluetooth to DualSense:
  normal single menu steps, PlayStation prompts and game rumble, including
  Alt+Tab and the SCX "re-register controllers" operation. This answers the
  earlier reproducible physical-source duplicate-input case below.

Evidence: `isolation/normal-user-smoke.txt`, `client-token.json`,
`registry-acl.txt`, `isolation-read.txt`, `after-owner-crash.txt`,
`engine-source-guard.txt`, `mixed-active-source.txt`, and the three
`validation/gameinput34/reconnect-*-20261024-16*` directories. No physical IDs,
capabilities or account SIDs are embedded in release binaries or defaults.

The .34 package integrates signing, journaled ownership and safe removal of the
third (source-filter) INF. Eleven installer ownership/recovery cases and 21
identity-preservation cases pass. Per-client/global source quotas, frame bounds,
secret rotation, replay rejection and all four independent policies are tested.
Final .34 package installation is recorded separately when completed; these
game observations are from the source-guard2 local build, not a WHQL result.
The Microsoft late-start discovery bug remains upstream; the explicit reconnect
workaround is tested without changing the separate-container policy.

## Explicit reconnect candidate: discovery pass, physical-source duplication unresolved

Historical investigation before Source Guard; see the newer results above.

24 October 2026, local `reconnect-v1` service/app candidate; native .33 is
unchanged. Early and late independent GameInput consumers receive four distinct
connected objects, IDs and input streams after explicit reconnection. Additional
Sony-mixed, Sony/Xbox-mixed and four-Xbox cases pass, as does concurrent reconnect
through four real SCX output/feedback workers. These checks do not establish that
a game ignores an independently visible physical source.

The user reports double D-pad menu navigation in Gears with Edge/Bluetooth ->
SCX DualSense, after explicit reconnection. A full disconnect, a pause and normal
connect do not remove it. Closing Steam does not remove it. With SCX disconnected,
the physical Edge alone operates the menu once per press. External GameInput/WGI
observations show one virtual device and one press/release transition, rather
than two simultaneously connected virtual objects. XInput observations in the
same WGI-initialized observer are process-local projection evidence, not proof
of a second globally installed virtual device.

Controlled virtual-only comparison `gears-isolated-20261024-151619`: physical
Edge off, SCX closed, one synthetic virtual DualSense, foreground-gated F7/F8/F9
inputs, 120-ms pulse per key press. The user confirms single menu steps both
before and after F6 performs the same service reconnection. The fixture records
12 presses (5 directional before reconnection, 6 directional plus Cross after),
retains the same identity and removes its lease on F10. This separates the
physical-plus-virtual case from the tested virtual-only reconnection case.

The normal-account `sony_access_probe` then tests the physical Edge while SCX,
Gears, Steam and DSX are closed. Both exclusive read/write and exclusive read
opens return ERROR_SHARING_VIOLATION (32). Allowing other writers while denying
other readers also fails with 32. Shared read/write opens succeed. All handles
are closed immediately; no reports are read/written and no machine settings
are changed. The two diagnostic binaries pass targeted Clippy with warnings
denied. Do not claim that merely changing SCX's CreateFile share flags fixes
the physical duplication on this machine.

HidHide was downloaded/signature-checked but **not installed or configured**;
the user explicitly declined it. No physical device is disabled, no system
handle is forcibly closed, and no GameInput service is stopped for this test.
The old system/shared-container policy is not restored: it fails the required
Tekken or mixed-device cases. GameInput aggregation is a consumer API, not an
established way for SCX to force a third-party game to ignore a source device.
Physical source isolation remains unresolved; this candidate is NOT qualified
for a stable release.

## Preview 4 qualification scope — 24 October 2026

Native 0.1.0.33 is being packaged as **v0.1.0-preview.4**, not as a stable
release. The original separate-container policy is retained. Temporary
container grouping and parent/topology diagnostic services are not included.

**Known failing case:** on installed Microsoft GameInput 4.0.302, immediate
late-start consumers see two of four existing SCX Sony controllers. Four
distinct HID/WGI inputs exist; the GameInput global reading stream confirms
the same two-device omission, including a reader that polls without first
registering a device callback. ETW explicitly records a dispatcher stall and
failed PnP listener notifications before missing devices reach HID parsing.
The exact queue occupancy was not measured. Both tested 3.x DLLs forward to
the installed 4.0.302 core, so they do not establish behavior of an independently
executed older runtime. The independently installed .32 baseline also fails.

A shared container lets four homogeneous Sony devices appear only while
GameInput rejects overlapping merges; mixed Sony/Xbox/wheel input then loses
other devices. Family grouping and a separate SWD parent also fail required
cases. These experiments are rejected, not shipped as fixes. Four ViGEmBus
DS4 reference devices are visible to the same immediate consumer; a general
all-driver failure is not claimed. No arbitrary initialization delay or relaxed
device-count assertion is used to declare success.

The exact final Preview 4 payload has offline build, unit, native ABI and
isolated installer/Setup checks. It has **not** completed a fresh full live
installation and runtime regression. Results below for earlier diagnostic .33
builds, or .32/.29, are historical evidence, not final-payload installation
attestation. Multi-controller GameInput compatibility remains unresolved.

## Revision 0.1.0.33 candidate build — 24 October 2026

The candidate adds generic wheel, flight-stick and arcade-stick classes. All
84 workspace all-target Rust tests pass, with rustfmt and all-target clippy
warnings denied. Independent descriptor tests verify class/axis usages, bit
counts, ranges, 32 buttons, neutral hat, two unsigned pedals and absent output
reports. Queue/watchdog tests exercise the actual new profiles; unsupported
feedback and reserved Steam profile 5 remain rejected.

The no_std Rust core links into the native DLL. MSVC C17 `/analyze /W4 /WX`,
the expanded native C-to-Rust ABI checks, both INF validations and catalog
generation pass. Existing Sony/Xbox descriptor and runtime regression tests
are included. The seven installer ownership/recovery checks and isolated Sony
GameInput registry restore tests pass under Windows PowerShell 5.1.

The first .33 candidate was installed with the original authorized account and
all 64 bits of its device identity seed preserved. Eleven installed suites pass:
the four generic classes together, generic lifecycle, Sony input/features and
feedback, Xbox input/feedback, and security for generic, wheel, flight, arcade,
DS4, DS5 and Xbox. The generic verifier initially retried a state already held
and waited for a duplicate report that the driver correctly coalesced; the
verifier now submits neutral before testing that transition again. No production
coalescing behavior was weakened. Independent application-side HidP tests also
read the actual wheel/flight descriptors and snapshots: 32 buttons, six axes,
one hat, 18-byte input reports, and correct decoding of locally modified report
buffers without injecting those modifications into Windows.

**Release blocker, still under investigation:** the Sony multi-device suite
passes independent HID and Windows.Gaming.Input checks for four distinct input
states, but a GameInput v3 consumer started afterward sees only two of four.
A controlled actual-installation .33 -> .32 -> .33 comparison reproduces the
same result on both revisions and restores the installed account/seed. An early
GameInput consumer sees all four; a second consumer started while the first and
the same devices remain alive sees only a subset. Continuous changing input,
neutral input, longer observation, service restart, asynchronous callbacks,
explicit dispatch, and v4 API probes do not reliably restore the immediate
late-start case. Generic HID and Xbox devices reproduce incomplete discovery as
well. Delaying the first callback registration allows four-device discovery,
but an arbitrary delay is not accepted as a driver fix.

An independent installed ViGEmBus reference with four DS4 devices is immediately
enumerated as four distinct paths/IDs by the same consumer; per-path lookups and
GameInput readings succeed. Thus this is not dismissed as a general probe or
system failure. A bounded SCX ETW trace records successful descriptor/string
requests on all four devices without failed native requests. Container and
initial-discovery behavior remain under investigation. No new stable release,
universal-game claim, or completed Sony multi-device pass is claimed here.

A diagnostic service with four fully different SHA-derived RFC-format container
GUIDs changes the effective parent/child container metadata but still reproduces
two-device discovery. The Windows-assigned-container variant reuses cached PnP
containers, so that variant is not treated as a valid contrast. A separate
global GetCurrentReading/GetNextReading stream collects 3038 valid readings from
only the same two visible devices; the omission is not merely a callback display
problem. Both comparisons restore the original service and remove the fixtures.

The installer now calls CreateServiceW/QueryServiceConfigW directly rather than
repairing an sc.exe command line through registry writes. The new read-only SCM
and path-rejection checks pass under Windows PowerShell 5.1, as do the seven
ownership/recovery checks. Actual create/start/process verification will be
covered by the next candidate installation, not claimed from read-only tests.

The subsequent Rust audit removes duplicate encoding/validation work, ensures
bootstrap cleanup on channel-creation failure, monitors the authenticated
producer process lifetime, and tolerates transient feedback seqlock contention.
These source changes have focused tests and Clippy coverage; their newly rebuilt
payload still requires complete installed regression validation.

The clean audit candidate removes the temporary container/ROOT experiment
features from production source and passes all 89 workspace all-target Rust
tests, rustfmt and Clippy with warnings denied. Its new SCM installer helper
passes the Windows PowerShell 5.1 read-only API/path checks; seven ownership
recovery tests, 21 account/identity replacement tests and Sony mapping recovery
checks also pass. These are build and isolated validation results, not a claim
that the new candidate has been installed or the GameInput blocker resolved.

No physical racing wheel, force-feedback, or universal-game compatibility claim
follows from these software-device and parser checks.

## Revision 0.1.0.32 installed validation — 23 October 2026

The successful Sony container policy below is now the default service path for
all profiles. Xbox and Generic retain their previous container values. The
experimental unique-container feature was removed; the separate ROOT diagnostic
feature remains opt-in. Independent review found no change to authentication,
public Sony identity, quotas, leases or transport. All 78 workspace all-target
Rust tests pass, with rustfmt and clippy on all targets and warnings denied.

- The release build passes MSVC C17 `/analyze /W4 /WX`, native C-to-Rust ABI
  checks, both INF validations and catalog generation. The installed .32 DLL
  and service match the release payload after accounting for local signing.
  Upgrade verification confirms the service is running and the authorized
  Windows account and existing device identity seed are preserved.
- All eight installed integration suites pass from the normal Windows account:
  Sony input/feedback, generic lifecycle, Xbox input/feedback, the four profile
  security suites, and Sony multi-controller validation. The first seven passed
  before the diagnostic-probe correction described below; the unchanged .32
  driver, service and command-line client then passed the final multi test.
- DS4 and DualSense pass HID input, native initialization/calibration and
  identity checks, rumble/RGB feedback, feedback cancellation and motor stop.
  Four generic devices have independent state, neutralization and lease cleanup.
  The independent XInput consumer verifies Xbox input and rumble/stop.
  The applicable ownership, capability, replay, bootstrap, cross-device and
  restricted-client security checks pass.
- Four DS5, four DS4, and mixed two-plus-two devices remain distinct in Windows
  HID, Windows.Gaming.Input and GameInput v3. HID/WGI verify independent button,
  stick and trigger markers; feedback stays isolated. Removing one device leaves
  exactly three distinct peers and does not disrupt their state. The full multi
  suite passed twice after correcting the external GameInput probe, including a
  final run with all temporary diagnostics removed. All test devices were removed.
- 32 Setup payload/argument/path/platform checks and 21 isolated replacement-
  identity checks pass. The identity tests cover all 64 seed bits, invalid or
  missing registry values, owner/path validation, service stop/write/verify/start
  order, and failure without restarting on an unverified seed. They pass under
  Windows PowerShell 5.1 and PowerShell 7. These are mocked installer checks,
  not a separate fresh-PC GUI installation claim.

During the original .32 investigation, replacing an all-bits input-kind mask
with the documented `GameInputKindGamepad` coincided with two complete passing
multi-device runs. That was initially attributed to the probe mask. The 24
October installed comparison above reproduces incomplete immediate discovery
even with that corrected mask, so the earlier attribution is not an established
root cause and the compatibility question is reopened. The two recorded passes
remain evidence for those specific runs, not proof of reliable initialization.
GameInput's original check concerned distinct enumerated identity, not game
input processing; the newer investigation also checks actual readings.

Local evidence is retained in
`scx-rust/validation/tekken-20261023/final32-regression/`: the seven individual
successful suite logs, the original failed summary, and
`sony-multi-exact-gamepad-kind.txt` / `sony-multi-final-clean.txt`. The final clean
run uses the installed Gears GameInput v3 runtime without modifying the game.
The controlled Tekken and subsequent physical-controller checks below preceded
packaging and used the same successful container policy. No new end-to-end
latency, universal game compatibility or long-duration stability claim follows
from these tests.

## Tekken container comparison — 23 October 2026

On the installed 0.1.0.31 driver, the isolated service feature
`sony-unique-container-test` restores the existing stable per-installation,
profile and slot container scheme for Sony devices. It does not change the HID
reports, namespace, public Sony identity, authorization or driver binary.

The user confirmed that Tekken 8's menu accepted up/down/Cross in this variant,
after the preceding system-container variant was detected but ignored input.
The physical controller was off, DSX/SCX were closed and Steam Input remained
off. The synthetic SCX fixture recorded 24 foreground-gated button pulses.
Both the SWD parent and HID child were independently read back with the same
unique per-device container, distinct from the former system-container sentinel.

Evidence: `scx-rust/validation/tekken-20261023/ultra-unique-20261023-160941-823-*`
and `ultra-investigation.json`. The service candidate SHA-256 was
`76726A66D2E5637FA03C31C74DB2E368AC3FD7B0AE441B34063D78C66B12BC65`.
The original service was restored and verified byte-for-byte after the test;
the driver, owner and identity seed stayed unchanged, and no test devices
remained. This is a positive controlled Tekken result, not a universal game
compatibility claim.

The subsequent SCX 2.1.2 Edge/Bluetooth to DualSense route ran for 600.014
seconds and forwarded 294,147 samples with no route error, warning or haptic
error. Two independent HID readers verified reports; shutdown was clean and
the original service was restored. The user confirmed actual Tekken input and
then reported that Gears also worked. Evidence:
`ultra-unique-20261023-162139-750-route.json` and its sibling logs.
The physical Edge remained present in this run, so the Gears report does not
independently exclude direct physical selection or establish fresh rumble/touch
results. No end-to-end latency result follows from these forwarding metrics.

Earlier negative Maestro trials with SCX, legacy SCX and observed G2-shaped
input packets did not establish a USB transport requirement. The successful
SCX UMDF comparison now demonstrates that no new USB bus is required for this
Tekken menu test. Do not infer that the historical Sony system-container
workaround disabled WGI/GameInput: the records below show those APIs remained
available under that workaround.

## Revision 0.1.0.29 cleanup candidate — 15 October 2026

Build and installed Windows validation completed on the development PC:

- 68 workspace Rust tests pass, together with rustfmt and clippy on all targets
  with warnings denied. The actual HID queue replaces the old unconnected queue
  model; tests cover press/release order, failed-reader retention, queue-full
  retry, replay/wraparound, timeout neutrality and four independent instances.
- The no_std driver core links through the real native binding. MSVC C17
  `/analyze /W4 /WX`, both INF validations and catalog generation pass.
- Native C-to-Rust ABI checks exercise the actual storage layout, four queues
  for each HID profile, bounds, replay, neutralization and feedback merging for
  Xbox/DS4/DS5, including identical updates, unrelated LEDs, motor stop and
  malformed/cross-profile updates. These ABI checks do not install the driver.
- 32 Setup checks pass for the .29 payload. The seven Windows PowerShell 5.1
  installer recovery tests and isolated GameInput mapping/recovery tests pass.
- The public Sony descriptors/identities, container policy and functional
  shared-channel offsets are retained. Generic auth-stage HID report 0x7c and
  all permanent bring-up diagnostic writes are retired. Legacy diagnostic
  helpers are not evidence of successful operation on this revision.

- A coordinated UAC-approved upgrade from .28 to .29 completed successfully.
  The installed DLL reports 0.1.0.29, the service is running, and the authorized
  Windows account and device identity seed are preserved. The rollback package
  was verified beforehand; automatic rollback was not exercised in this upgrade.
- All eight installed integration suites pass from an unelevated client:
  generic lifecycle, Sony input/feedback, Xbox input/feedback, four profile
  security suites, and Sony multi-controller WGI/GameInput validation.
- Four generic devices retain independent buttons, axes and triggers. Watchdog
  neutralization, lease removal and continued input on sibling devices pass.
  DS4/DualSense HID input, native configuration, identity/calibration, rumble/RGB
  feedback, waiter cancellation and motor-stop delivery pass. Microsoft's
  XInput consumer receives the Xbox state and returns rumble/stop correctly.
- Restricted broad-user and same-owner low-integrity clients are denied.
  Capability, bootstrap, cross-device, retired-capability and replay checks pass
  on the applicable interfaces. Xbox's effective access-denial checks pass;
  its interface does not support querying the ACL through the diagnostic API.
- Four DS5, four DS4, and mixed two-plus-two fixtures each remain distinct in
  Windows HID, Windows.Gaming.Input and GameInput v3. Buttons/axes/triggers and
  feedback remain independent; removing one leaves exactly three working peers.
  All owned test devices are removed afterward. The GameInput consumer uses
  Gears' installed runtime, but this is an API test, not an in-game test.

Physical controller forwarding, in-game behavior, long-duration stability and
end-to-end latency have not been remeasured for .29. No performance percentage
or new latency claim follows from the removed diagnostic stores. The published,
game-tested .28 release remains unchanged and available for recovery.

## Preview 2 desktop installer — 15 October 2026

The separate Setup EXE embeds the unchanged published 0.1.0.28 ZIP, SHA-256
`ad7e32670e7110fe1a71a7b8601c2389d66b9b65b06b3197639801b006604cd2`.

- 32 installer checks pass: complete embedded manifest verification, required
  payloads, Windows argument round-trips, unsafe ZIP paths, owner SID and
  operation allowlists, and the Windows platform guard. These tests do not
  install a driver or modify trust.
- The fixed PowerShell wrapper passes syntax validation. The seven existing
  installer recovery cases and isolated GameInput mapping tests still pass.
- An interactive **Reinstall** completed on the Windows 11 x64 development PC.
  The user approved UAC; the service was running afterward and the original
  Windows account remained authorized. Setup's post-install verification and
  the underlying installation transcript both reported success.
- The revised GUI was visually checked at 200% Windows scaling after correcting
  clipped title, wrapped text and action buttons. German and English text are
  included; this is not an all-DPI/all-locale certification claim.

Fresh installation and standalone removal through the GUI have not been
separately exercised on a clean PC. Reinstall exercised removal and installation
of the existing revision. The EXE has no public publisher signature; the local
driver signing mechanism and the driver's preview status are unchanged.

## Revision 0.1.0.28 — Sony container policy and multi-controller validation

The successful diagnostic policy below is now the default Sony service policy.
The native HID/report/authorization code is unchanged from .27; the native
resource/INF version advances to keep the installable package identifiable.
Xbox/generic containers and all private ownership boundaries remain unchanged.

The completed stability capture lasted 458.667 seconds, with 223 valid device/
focus snapshots, 42,809 provider events, zero rejected captured requests and
zero dropped events. Gears sent 42,792 accepted Sony output writes.
Multiple focus transitions and the intentional Sony -> Xbox -> Sony switch
are visible; sampled device changes match that explicit user action. Game
rumble is user-confirmed and a nonzero feedback sample was recorded. The user
could not reproduce the earlier transient despite trying. Per-frame reads are
not traced, and this finite clean run cannot establish the old transient's cause.

The installed final .28 and earlier .27 + identical container policy pass self-test-sony-multi:
four DS5, four DS4, and two of each. All have distinct PnP and Sony identities,
distinct WGI controllers with independent buttons/axes/triggers and Sony labels,
and distinct GameInput v3 paths/device IDs (Gears' 3.5.262 runtime). Rumble
feedback remains per-device. Removing one fixture leaves exactly three working
WGI/GameInput devices; all owned PnP devices disappear when released.

Final installed .28 also passes generic input/lifecycle, Sony initialization,
mixed Sony rumble/light fields, cancelled feedback wait recovery, Xbox feedback
and motor stop, plus effective security checks for generic/DS4/DS5/Xbox. Restricted
broad-user and same-owner low-integrity tokens are denied. Producer authorization
and stale/replay protections remain intact. All tests release their fixtures.
The local installation preserves the prior device identity seed.

## Sony container A/B — initial result and transient report

2026-10-14: native .27 and the same SCX app/profile stayed unchanged. The
diagnostic service changed only Sony `pContainerId` to the ROOT sentinel
`{00000000-0000-0000-FFFF-FFFFFFFFFFFF}` used by the working Maestro device.
PnP parent, HID child and GameInput v3 all confirm the requested container.
The user reports PlayStation symbols, single menu steps and working game
rumble. However Xbox symbols and duplicate navigation briefly returned and
disconnect/reconnect restored correct behavior. The user recalls possibly
using Alt-Tab, cannot reproduce the recurrence, and reports stable play now.
Focus change is a hypothesis, not a proven trigger. The subsequent recorded
stability and parallel-controller checks above support adopting the policy in
the next local validation build. The earlier transient remains recorded rather
than being reclassified as reproduced or conclusively fixed.

## Revision 0.1.0.27 — mixed Sony output commands

.26's real-game trace (GoWEDay PID 6928, zero lost events) confirms the first
fix: feature 0x08 succeeds and Gears proceeds to calibration feature 0x05,
then WRITE_REPORT 0x02. That 48-byte write fails STATUS_NOT_SUPPORTED 63 times.
The independent channel diagnostics show flags 0x0d/0x17 and decoder status 3:
the parser rejects the entire packet because it includes hardware-only controls.

.27 keeps strict report ID/length validation, but ignores unsupported audio/
adaptive-trigger fields in otherwise valid Sony output reports. Supported
rumble/LED fields still apply, with existing field-wise merge and stop semantics.
No trigger/audio support is claimed or added. All 67 Rust tests, Clippy and
native /analyze /W4 /WX, ABI, INF/catalog checks pass. Installed .27 passes
both Sony initialization and mixed-output Windows tests, including input,
rumble/RGB feedback, cancelled-wait recovery, motor stop and release. The .27
Gears trace (PID 19412, zero lost events) shows one successful initialization
instead of a 500-ms retry loop, followed by 2,608 successful 48-byte output
writes and no failed game requests. Nevertheless the user still sees Xbox
symbols and reports apparent double input on downward analog-stick movement.
This revision is NOT a confirmed Gears compatibility fix. Read-only inventory
shows one HID/Raw Input device, one WGI Gamepad and no XInput slot. Duplicate
consumption across APIs remains a hypothesis, not an established cause. The
same-profile Maestro comparison now confirms PlayStation symbols AND no double
menu movement with Maestro. DS4 remains pending. Producer authorization is
unchanged from .26. A service-only diagnostic build can isolate Sony container
assignment (`diagnostic-sony-root-container`); this is not enabled in normal
builds or established as a fix, and must be rolled back after the comparison.

## Revision 0.1.0.26 — Sony configuration writes

The .25 trace captured 477 SCX events without loss. Requestor PID 31536 is the
live GoWEDay process. It repeatedly reads Sony identity/firmware successfully,
then receives STATUS_ACCESS_DENIED on SET_FEATURE: DS5 0x08 (48 bytes, 49
attempts), DS4 0x14 (17 bytes, 22 attempts). The DS5 loop repeats every ~500 ms.
This is a directly observed initialization failure, not an enumeration guess.
Maestro's SET_FEATURE handler accepts these public configuration writes.

.26 separates public Sony configuration from private input submission. Only
the two observed descriptor-sized radio-configuration reports are acknowledged
as no-ops on the virtual USB device. They cannot mutate input/identity/feedback,
reach the physical source or access the authenticated producer channel. All
other feature writes remain denied. All 66 Rust tests, Clippy and native
/analyze /W4 /WX, ABI, INF and catalog checks pass. Installed .26 passes both
Sony input/feature/feedback tests, including repeated public configuration
writes with unchanged input/identity/feedback. Both installed security suites
pass (restricted/low-integrity, wrong/cross-device/retired capabilities, replay
and public-HID injection). The real Gears trace shows the initialization now
proceeds past SET_FEATURE, but fails on a mixed output command (see .27).
This is still not a confirmed in-game compatibility fix.

## Revision 0.1.0.25 — opt-in Sony request diagnostics

2026-10-14: independent v3 probe built against the public Microsoft.GameInput
3.5.262 header and explicitly loaded Gears' bundled GameInputRedist 3.5.262.0.
SCX .24 and the actual Maestro backend both expose PS5 Controller, matching
Cross/Circle/Square/Triangle labels, sensors, controller counts and raw report
sizes. Both report VID/PID zero through this API; both report the correct IDs
through HID/Raw Input. The different runtime alone does not explain the game.

.25 adds disabled-by-default ETW request metadata to the Sony dispatch path.
It does not change the Sony protocol, device enumeration or output behavior.
No state payload, authorization capability, SID or channel name is logged.
High-rate READ_REPORT/GET_INPUT_REPORT calls are excluded. Native /analyze,
/W4 /WX, ABI, INF and catalog validation pass. Installed Sony input/features/
feedback tests pass. The user confirms Gears still shows Xbox symbols. The
completed process-attributed trace identifies the rejected writes described
above; .25 itself is diagnostics only.

## Revision 0.1.0.24 — separate Sony transport candidate

2026-10-13: live Gears/Maestro positive control confirms PlayStation symbols.
The actual firmware 0x20/patch 0x22 bytes and public HidP fields match .23.
Both have matching Sony WGI labels. .23 nevertheless still shows Xbox symbols
and repeatedly receives Sony feature reads without output writes. Its private
feature-only collection still appears in DirectInput on the live app's instance.
Removing input reports in .23 did NOT reliably eliminate the extra device.

.24 removes that entire collection and separates private traffic into an
authenticated fixed shared-memory channel. 65 Rust tests pass in the real host
context; Clippy and native static analysis/ABI/INF checks pass. Installed .24
passes both Sony Windows input/features/rumble/RGB/feedback/timeout/stop/release
tests and both security suites. A first client startup check ran before PnP had
finished starting the driver; the updated SDK now waits (bounded to 10 seconds)
for the channel to become ready. No driver reinstall was needed for that fix.
Parallel independent HidP probes pass all 58 cases. Live enumeration now shows
exactly one public HID collection and one DirectInput entry per Sony profile.
The user subsequently confirmed that Gears still shows Xbox symbols, including
after restarting the game. Revision .24 does NOT resolve the in-game Sony
recognition. Feature-read counters alone do not identify the requesting process;
SCX's SDL enumeration may issue these requests too.

## Revision 0.1.0.23 — source-guided Sony compatibility candidate

2026-10-13: user confirmed revision .20 still shows Xbox glyphs in Gears while
Maestro shows PlayStation glyphs. A direct comparison with HIDMaestro source
0c85d866cdd8e70acb63d968173b9201b2cb8bb1 identified simplified/nonmatching
public descriptors, invalid synthesized firmware metadata and missing HID
serials. The source documents games abandoning invalid Sony firmware responses;
this is evidence for a compatibility correction, not proof of Gears' exact cause.
The app's working Maestro port does not set USB bus-type overrides, so those
overrides are not part of this candidate.

62 Rust tests pass in the real Windows user context. The service ACL test
cannot run under the restricted filesystem sandbox. Native revision .23 is
installed: both Sony profiles pass actual Windows HID input, identity, firmware,
game-side rumble/RGB output, asynchronous feedback, idle wait cancellation,
subsequent stop delivery and neutral/release. Revision .22 exposed a feature
marshalling mistake: UMDF receives the report ID without an application cursor.
Revision .23 owns pending feedback state per device and broadcasts to current
waiters; cancellation does not consume a change. The user subsequently confirmed
that Gears still shows Xbox symbols with .23.

Independent HidP checks pass all 29 button/axis/hat/release cases for each of
DS4 and DualSense while both are present. The HID inventory now has one input
collection per Sony device, but a later live app inventory still lists the
feature-only DS5 supplemental DirectInput device. Public descriptors expose the canonical vendor
usages and unique nonempty serials. DS4's existing DirectInput cache still
labels 054C:09CC as SCX Virtual Gamepad; that global cache was not overwritten.
Both Sony security checks pass: restricted and low-integrity clients, wrong,
cross-device and retired capabilities, legacy envelopes, replays and public
game-collection injection are rejected. Final 62 tests and Clippy pass.

## Revision 0.1.0.18 — DualSense rumble stop

The independent SDL consumer exposed an additional bug: SDL restores audio
haptics using an empty DualSense effects packet. The previous parser accepted
it without clearing saved emulated-rumble amplitudes. Empty motor/effect flags
now explicitly stop emulated rumble; updates selecting other fields retain
their field-merge behavior. A new regression test covers start -> empty stop.

59 Rust tests, Clippy (warnings denied), native static analysis/ABI, both INFs
and catalog validation pass. On the installed .18 driver, SDL 3.4.18 produces
nonzero feedback and a subsequent 0/0 stop for both Sony profiles. Five DualSense
output reports and three DS4 reports are received and all accepted. The Xbox
changes are unchanged from .17; the foreground WGI rumble evidence below applies
to that unchanged path. The final Gears retest is still required.

## Revision 0.1.0.17 — WGI discovery/input and output compatibility

Follow-up on the same Windows development PC, 2026-10-13. User confirmed the
0.1.0.8 Sony input fix in Gears, but reported Xbox labels for Sony, missing Sony
game rumble, and no button input when selecting Xbox output. Steam was closed.

Reproduced and isolated at the Windows API boundary:

- System-class Xbox devices were absent from WGI despite working via XInput.
  XnaComposite registration admits exactly one WGI Gamepad/RawGameController.
- Windows issues four-byte WAIT_INPUT requests: 02 01 00 02 then 02 01 00 03.
  Rejecting them stopped input. The corrected path receives changing button A
  states and nonzero, advancing WGI timestamps. Public async regression checks
  pass for separate readers, release, cancellation, bounds, denied producer
  access, idle, watchdog, removal, and the neutral mode-2-to-mode-3 transition.
- The mode transition must deliver a fresh snapshot for Windows' active sink;
  without it, a neutral-only fixture never receives WGI rumble requests.
- WGI's SET_STATE envelope has nine bytes, versus XInput's five. The previously
  rejected nine-byte command is now bounded and decoded using the common
  five-byte motor/LED prefix; the extension is not advertised as extra support.
- Sony child hardware IDs now contain the complete VID_054C&PID_09CC or
  VID_054C&PID_0CE6 identity. WGI reports PlayStation button labels 43/44/45/46
  (Cross/Circle/Square/Triangle), independently from logical Gamepad A/B/X/Y.

58 Rust tests pass, including the new extended-output test. Native static
analysis, ABI, both INF validations and catalog signability pass. These checks
do not replace the outstanding in-game identification/rumble retest. The direct
HID Sony rumble path and WGI Gamepad.Vibration are separate test cases: API
acceptance alone is never counted as successful feedback delivery.

Installed 0.1.0.17 regression results:

- 58 Windows HidP semantic cases pass; the independent WGI Sony verifier passes
  both profiles, with 1,976 samples and complete button/hat/axis coverage.
- Generic four-device isolation, watchdog/release/removal, Sony HID feedback and
  XInput feedback/stop checks pass. Mapping recovery and all seven installer
  recovery/security cases pass under Windows PowerShell 5.1.
- Foreground WGI Gamepad.Vibration(0.25, 0.5) produces three Xbox feedback pairs
  16448/32896, each followed by 0/0. These are the expected rounded 8-bit motor
  values expanded to 16 bits. This is driver feedback verification, not a
  measurement of a physical motor or end-to-end input latency.
- SDL 3.4.18 identifies the owned virtual devices as PS4 and DualSense, opens
  their native HID paths, and its rumble requests produce nonzero SCX feedback.
- WGI's generic Sony Gamepad.Vibration calls still produce no HID output writes.
  Native Sony HID/SDL feedback is a different, verified path. Whether Gears
  now selects that path and presents Sony glyphs remains a user/game retest.

## Revision 0.1.0.8 — Sony mapping regression

Reported with Steam Controller -> DualShock 4 / DualSense, SCX VPad, Steam
closed, Gears of War Windows Store edition. Camera drift, trigger/camera cross-talk
and incorrect face-button behavior were reported by the user.

Reproduced before the fix independently of the app/physical controller:

- Windows HidP decoded Sony neutral axes as X/Y/Z/Rx/Ry/Rz =
  128/128/0/128/128/0. L2 changed Z and R2 changed Rz. Both Sony profiles failed
  all eight initial semantic cases, although raw-byte and SDL tests had passed.
- Windows Gaming Input exposed no Gamepad mapping for the DS4 identity on this
  PC. The existing DualSense registry mapping used generic button/axis indices:
  Cross became B, Square became A, L2/R2 became View/Menu, and the right stick
  and trigger values were mixed. The installer now journals these prior values.

Build verification: 56 Rust tests pass; Clippy with warnings denied passes;
native ABI check, both INF validations and catalog signability pass. The mapping
installer runs under Windows PowerShell 5.1 and has tested existing-value
restoration, external-edit preservation, duplicate-install refusal and cleanup.

Installed-driver verification completed on 2026-10-13:

- Fresh install of native revision 0.1.0.8, service running, locally signed;
  no elevation for any test client. Previous Sony mappings are journaled.
- 58 semantic Windows HidP cases pass (29 each for DS4 and DualSense): neutral,
  all 14 HID buttons, both independent triggers, full stick extremes, eight
  D-pad directions, combined buttons/triggers/stick and final release.
- Independent C++/WinRT Windows Gaming Input consumer: both profiles pass,
  1,978 observations total with no persistent mismatch. Coverage includes all
  14 buttons, all nine switch states and minimum/maximum for all six axes.
  Cross maps to A, Circle to B, Square to X, Triangle to Y; neutral triggers
  read exactly zero, and neutral sticks read the normal 8-bit midpoint
  (+/-0.00392157), rather than a full deflection.
- Generic HID four-controller isolation, watchdog neutralization, lease removal,
  Sony feature/rumble/RGB feedback, and Microsoft XInput state/rumble/stop
  regression checks all pass.

The user subsequently confirmed corrected DS4/DualSense inputs in Gears; the
remaining identity/rumble/Xbox issues are tracked in the newer record above.
No universal game compatibility claim is inferred from these Windows API tests.

Development validation on 2026-10-13, Windows 11 x64 build 26300. Secure Boot
remained enabled. No test-signing, signature-enforcement, Memory Integrity or
anti-cheat settings were changed. This is not Microsoft certification.

## Live checks completed

- Fresh locally signed UMDF installation, then scoped uninstall/reinstall.
- An ordinary, unelevated Windows client can reach the installed service and
  create/remove virtual devices. Administrative rights are used only for setup.
- Four generic HID controllers read independently through Windows HID:
  buttons, hat, four signed axes, independent triggers, press/release isolation,
  stale-input neutralization, removal of one device and continued sibling input.
- Sixteen generic HID controllers across four client processes. A seventeenth
  creation is rejected. Forcibly terminating one test client reclaims its four
  slots while the three other owners remain running; four new devices can then
  be created. All test processes are cleaned up afterward.
- Four Xbox-compatible controllers discovered by Microsoft's installed
  `xinput1_4.dll`, using a separately built consumer that does not link SCX code.
  Each slot shows independent changing buttons/axes. Capability and battery
  requests succeed. Rumble and rumble-stop API calls succeed. After the owners
  exit, all four slots return `ERROR_DEVICE_NOT_CONNECTED`.
- DualShock 4 and DualSense profiles: actual Windows HID consumer receives
  buttons/hat/sticks/triggers; calibration, identity and firmware feature
  queries succeed. Game-side rumble/RGB output reaches SCX feedback, both as
  an event and as a snapshot. Neutral/release state is checked.
- Microsoft's `XInputSetState` reaches SCX's event-driven feedback and snapshot,
  including rumble stop. The verification accounts for XUSB's 8-bit motor
  quantization; normalized 16-bit feedback cannot restore precision lost there.
- Independent SDL 3.4.18 recognizes the Xbox, DualShock 4 and DualSense profiles
  and receives changing buttons, sticks and triggers. Both Sony profiles expose
  touch, gyro and acceleration. Known gyro values of 10/-20/30 degrees per second
  arrive as 0.17453/-0.34907/0.52360 radians per second; 1000 milli-g arrives as
  9.80665 m/s². Touch press/release works; normalized coordinates differ slightly
  with SDL's profile-specific dimensions (a nominal 50% Y is about 51.1% on DS4
  and 50.4% on DualSense).
- Driver revision 0.1.0.7: normal-user input succeeds for all profiles. Generic,
  DS4 and DualSense reject zero, incorrect, cross-device and retired HID input
  capabilities, zero-access-handle attempts, game-collection bypass, legacy input
  packets and replay. The ordinary owner cannot read the protected bootstrap.
  Restricted broad-user and same-owner low-integrity tokens cannot obtain leases
  or inject without an issued capability. Xbox's distinct handle access boundary
  also rejects those tokens. This is account isolation, not a sandbox between
  mutually distrustful applications running as the authorized owner.
- Seven installer recovery/security regression cases execute in real Windows
  PowerShell 5.1.26100.9549, including partial-staging journal recovery.
- A forced termination of the verified SCX VPad service removes four live
  generic devices. Four orphan bootstrap keys remain after the crash and are
  removed on service restart. Subsequent normal-user generic self-test and
  generic/Xbox security checks pass; no test devices remain.
- GitHub-hosted Windows and Linux Rust checks pass, including formatting,
  tests and Clippy. The separate GitHub Windows native driver build passes
  static analysis, ABI checks, both INF validations and catalog generation.

Protocol tests, native C/Rust ABI checks, MSVC static analysis, Microsoft's
InfVerif and Inf2Cat are separate checks, not substitutes for these live tests.

## Timing sample with authenticated input

Generic HID driver revision 0.1.0.7, 20,000 samples on the development computer:

| Measured interval | Median | 95th percentile | 99th percentile | Maximum |
|---|---:|---:|---:|---:|
| SCX submission to Windows HID-reader completion | 68.7 us | 87.0 us | 98.5 us | 699.8 us |
| Neutral-report submission API completion only | 21.2 us | 43.8 us | 54.3 us | 124.9 us |

These are single-machine development measurements with background activity.
They include scheduling and the tested Windows path but exclude physical input,
USB/Bluetooth transport, game polling and display latency. They do not establish
that SCX is faster than another driver. Different machines, workload, producer
patterns and later revisions require fresh measurements.

## Scope of the evidence

The initial target is Windows 11 x64. Windows 10, ARM64, console compatibility,
GameInput SDK, USB audio and native adaptive triggers are not validated. The
original preview also lacked Windows Gaming Input validation; see the revision
0.1.0.8 regression record above. Reboot persistence, long-duration soak testing, broad game coverage
and independent security review remain production-readiness work. A locally
trusted signature does not guarantee acceptance by anti-cheat software.
