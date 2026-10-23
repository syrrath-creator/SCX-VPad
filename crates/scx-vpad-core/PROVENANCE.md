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
the attributed Maestro profiles. Sony has no private SCX HID collection since revision .24. Its authenticated
input and feedback use a service-created sideband channel, so feedback is not
a second input controller. Generic HID alone retains the SCX control collection. Calibration remains synthesized from our normalized units and
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

On 2026-10-23 the DualSense input encoder was checked again against SDL's
PS5StatePacket_t: the USB packet also includes a 32-bit packet sequence at
bytes 12-15, a second sampling timer at 49-52 and USB-present bit 0x08 at 54
(all including report ID). SCX now supplies both counters, derives the two
simultaneous synthetic sampling times from InputState.timestamp_us, and marks
the virtual USB link present even while controls are neutral. A real neutral
USB packet with these populated fields is independently documented at
<https://github.com/nondebug/dualsense/blob/main/README.md>.
These are wire-format facts, not copied implementation code. This correction
alone is not evidence of Tekken compatibility; that still requires a game test.
2026-10-23 native calibration comparison:
A live, isolated Maestro 1.10.0.1457 DualSense capture returned +/-8000 raw
gyro references at +/-500 degrees/second; SCX .30 returned +/-1440 at +/-90.
Both represent 16 raw units per degree/second. Revision .31 uses the observed
reference range for Sony USB calibration, without changing motion encoding,
bias or accelerometer units. A regression compares the captured feature bytes
and checks unchanged scaling for positive/negative motion on all three axes.
This is an isolated compatibility experiment, not a confirmed Tekken fix.
Evidence: sony-protocol-20261023-145408-617-maestro-protocol.txt in the local
Tekken diagnostic directory. HIDMaestro's calibration source is already
covered by the MIT reference above; no private Sony SDK code is used.