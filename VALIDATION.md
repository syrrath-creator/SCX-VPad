# Windows validation record

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
Windows Gaming Input, GameInput, USB audio and native adaptive triggers are not
validated. Reboot persistence, long-duration soak testing, broad game coverage
and independent security review remain production-readiness work. A locally
trusted signature does not guarantee acceptance by anti-cheat software.
