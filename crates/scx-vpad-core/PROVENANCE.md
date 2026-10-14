# Original implementation and protocol references

The SCX transport, validation, state encoding and service are independently
implemented. As of native revision 0.1.0.23, sony_compat.rs incorporates the
Sony USB report descriptor and firmware-information response tables from
HIDMaestro (MIT), revision 0c85d866cdd8e70acb63d968173b9201b2cb8bb1:
profiles/sony/dualshock-4-v2.json, profiles/sony/dualsense.json and driver/driver.c.
The full license is retained in licenses/HIDMaestro-MIT.txt. These are USB
compatibility metadata, not executable or flashable Sony firmware. The tables
remain MIT-licensed and are excluded from SCX's own redistribution restriction.

The generic descriptor implements the USB HID 1.11 item format and Generic
Desktop/Button usages: <https://www.usb.org/document-library/device-class-definition-hid-111>.
The additional SCX vendor collection and binary transport are original designs.

Experimental DS4/DualSense USB encoding uses factual wire positions, sizes,
button assignments, sensor units, touch dimensions and feature-report IDs
documented by these primary interoperability implementations (consulted
2026-10-13):

- Sony's maintained Linux HID implementation:
  <https://github.com/torvalds/linux/blob/master/drivers/hid/hid-playstation.c>
- SDL's maintained controller interoperability implementations:
  <https://github.com/libsdl-org/SDL/blob/main/src/joystick/hidapi/SDL_hidapi_ps4.c>
  and <https://github.com/libsdl-org/SDL/blob/main/src/joystick/hidapi/SDL_hidapi_ps5.c>.

Those projects retain their own licenses. No Linux or SDL implementation code
was imported. Public Sony collections now use the byte-exact descriptors from
the attributed Maestro profiles. The private SCX collection is feature-only,
with a bounded cancellable feedback wait, so feedback is not a second input
controller. Calibration remains synthesized from our normalized units and
pairing/HID serial identities are unique per SCX device. Firmware-information
responses use the attributed compatibility tables because invalid type/version
fields can make native game parsers abandon the Sony handshake. Protocol IDs
do not imply that SCX is Sony hardware or endorsed by Sony.

Current boundaries: USB-style report encoding only; no Bluetooth radio,
firmware flashing, authentication, USB audio, console pairing, or adaptive
trigger engine is implemented. No XInput support is inferred from generic HID.
Synthetic protocol tests do not substitute for live Windows/SDL/game validation.

## Authenticated input envelope

The HID control descriptor exposes feature `0x7a` with exactly 96 payload bytes:
a 32-byte per-lease random capability followed by the original version-1 Input64.
Including the report ID, the maximum feature buffer is 97 bytes. Unauthenticated
legacy feature `0x7e` is absent and must always fail in the Windows binding.
Metadata version 2 advertises capability bit `CAP_AUTHENTICATED_INPUT` and the
96-byte submission payload. Old metadata, omitted capability bits, reserved
fields and incorrect sizes are rejected by the SDK. Xbox uses its separate
access-controlled function interface, so its submission remains Input64.

Only the authenticated, medium-integrity-or-higher owner receives a capability
through the service lease response. The service generates a fresh independent
secret for each lease; neither the public device identity nor the PnP properties
contain the secret. The driver loads it from protected registry storage granting
SYSTEM and elevated Administrators full access and the Windows UMDF-driver SID
read access, not ordinary interactive users. Administrative access also permits
recovery and uninstall after a service crash; a filtered administrator token
does not receive that grant.
The Windows binding checks all 32 bytes before any input/sequence/liveness
mutation. No getter, diagnostic report or log returns the capability.

This is bearer authorization: an authorized process can deliberately delegate
its capability. It is not isolation against a same-user process that can inspect
the authorized client's memory. Token impersonation through the HID kernel bridge
and a HID collection's zero-access handle are not treated as authorization.
Reference: <https://learn.microsoft.com/en-us/windows-hardware/drivers/wdf/controlling-device-access>.
