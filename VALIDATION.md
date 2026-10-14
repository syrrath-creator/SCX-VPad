# Windows validation record

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
