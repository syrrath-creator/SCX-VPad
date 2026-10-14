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
