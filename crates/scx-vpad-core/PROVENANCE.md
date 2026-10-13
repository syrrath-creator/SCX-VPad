# Original implementation and protocol references

All Rust code and descriptor-generation logic in this crate was newly written
for SCX VPad. No HIDMaestro, ViGEmBus, Sony firmware, captured HID descriptor,
or previously written SCX source was copied into this crate.

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

Those projects retain their own licenses. We have not imported their functions,
structures, control flow, comments, descriptor arrays, or firmware data.
The experimental profiles synthesize fresh HID descriptors from the required
report field layouts, and calibration values from our explicitly defined
normalized units. Identity and firmware responses are synthetic SCX values,
not extracted device data. Interoperability IDs identify the selected protocol;
they do not imply that this software is Sony hardware or endorsed by Sony.

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
