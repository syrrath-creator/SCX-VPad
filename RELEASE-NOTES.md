# 0.1.0 development preview

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
