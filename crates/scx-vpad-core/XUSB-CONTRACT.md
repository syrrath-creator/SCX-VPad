# Experimental XUSB contract and integration boundary

This is an original byte-oriented implementation for a separate UMDF function
device. It is not an HID descriptor renamed to Xbox. No reference project's
source, descriptor arrays, control-flow implementation, or binary is included.

## Primary evidence

- Microsoft distinguishes the XUSB interface consumed by XInput from the HID
  compatibility interface: <https://learn.microsoft.com/en-us/windows/win32/xinput/directinput-and-xusb-devices>.
- Microsoft's USB-wire specification is separate from the user-to-driver IOCTL
  boundary: <https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-xusbi/7238785a-1c68-460c-8416-b62f2b0d4aaf>.
- OpenXinput publishes packed wire layouts and a working consumer:
  <https://github.com/Nemirtingas/OpenXinput/blob/OpenXinput1_4/src/OpenXinput.cpp>.
  We consulted numeric protocol facts, not its implementation for reuse.
- Independently corroborating IOCTL identifiers:
  <https://github.com/nefarius/XInputHooker/blob/master/XInputHooker/ioctls.json>.
- HIDMaestro's independent UMDF companion demonstrates feasibility and documents
  observed Windows behavior: <https://github.com/hifihedgehog/HIDMaestro/blob/master/driver/companion.c>.
  Its behavior is reference evidence; none of its functions or reply arrays
  were copied. Its V1 capability prose conflicts with its actual bytes, so our
  offsets follow the independently published packed consumer format.

## Interface

Discovery GUID: `{EC87F1E3-C13B-4100-B5F7-8B84D54260CB}`.
Install as a separate `System` UMDF function device. HIDCLASS does not forward
arbitrary user IOCTLs to a HID minidriver. Publish a separate SCX control
interface for producer submissions and enforce the authenticated owner there.
Consumers need ordinary controller read/rumble access, not producer access.

All wire integers are little-endian and packed. Each function-device instance
contains one controller, so request device index is **zero**, independently of
the global XInput player slot. A base request is `version:u16,index:u8`.

| Operation | IOCTL | Request | Reply implemented here |
|---|---|---|---|
| Information | `80006000` | empty | 12 bytes |
| Capabilities | `8000E004` | base | 24 bytes for v0101; 36 for v0102+ |
| LED | `8000E008` | base | version:u16, pattern:u8 |
| State | `8000E00C` | base | 29 bytes; legacy one-byte index request gets 20 |
| Output | `8000A010` | five bytes | no response; decoded feedback |
| Battery | `8000E018` | base + device type | version:u16, type:u8, level:u8 |
| Wait for input | `8000E3AC` | not established here | native queue required; not synchronously faked |
| Guide wait | `8000E014` | base | unsupported in this revision |

Modern state: version at0, active status at2, packet number at5, buttons at11,
triggers at13/14, signed axes at15/17/19/21. The optional resumed encoder sets
status3 and marker0x14 at10; it is reserved for an event-driven WGI integration.

Capabilities v0101: buttons4, triggers6/7, axes8..15, motor maxima22/23.
Capabilities v0102+: buttons16, triggers18/19, axes20..27, motor maxima34/35;
VID/PID/version are6/8/10. These motor maxima are byte fields, not the public
XINPUT_VIBRATION structure.

Output: index0, LED pattern1, left/right motor2/3, validity flags4 (LED1,
rumble2). A LED-only update must preserve motor state. No audio device, power
management controller, or extended firmware capability is fabricated.

## SCX implementation decisions and required proof

The state conversion uses SCX's own normalized packet directly. Positive Y is
down in this packet; XInput's positive-up convention is converted with saturating
negation. The native per-device context increments packet number only when the
encoded twelve control bytes change. Sequence/timestamp-only updates do not
force games to process unchanged controls.

Input delivery must use the restricted SCX control interface. The service owns
device lifetime. Do not accept producer state from the public XUSB interface.
Use bounded queues for asynchronous requests and cancellation on teardown.
No polling timer is needed for synchronous XInputGetState.

Before enabling Xbox output in a release, prove discovery through Microsoft's
actual xinput1_4.dll, live button/stick/trigger state, rumble readback, four-device
isolation, process-crash neutrality and teardown. WGI and GameInput require
separate validation. Unit tests of these byte layouts are not that proof.
Standard XInput exposes four player slots; the SCX service's sixteen-device
capacity does not remove that external API limit.
