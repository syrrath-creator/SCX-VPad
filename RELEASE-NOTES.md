# 0.1.0-preview.2 — Sony control mapping fix (native revision 0.1.0.8)

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

This fixes reproduced Windows API failures. Game-by-game confirmation is recorded
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
