# Windows validation record

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
