# SCX original Windows binding

Revision .35 retains the independent stable Sony containers introduced in .32.
Each controller has its own PnP path, identity, lease and authenticated channel;
it is not assigned the historical shared PC/root-sentinel container from .28.
The explicit service reconnect operation preserves those identities while
recovering the tested late-start GameInput discovery case. Source Guard is a
separate device-specific UMDF2 filter for the qualified Edge Bluetooth source;
it prevents physical-plus-virtual duplicate input during an active source lease.
See VALIDATION.md for installed evidence and the scope of the GameInput workaround.

Sony diagnosis can explicitly enable the ETW provider `SCX-VPad-Diagnostics`
(`05b6b2fc-a45b-4b7c-8c94-3b2ce386b1e5`, keyword 1, informational level).
It records requestor PID, public profile/instance identity, IOCTL/selector,
buffer sizes, status and transferred length. It records no report payload,
input capability, owner SID or channel name. It is disabled by default and
does not trace per-frame READ_REPORT/GET_INPUT_REPORT. Requestor PID may refer
to a Windows forwarding process rather than the originating game; attribution
must be checked against live process inventory rather than assumed.

Native .26 accepts DS4 feature 0x14/17 bytes and DualSense feature 0x08/48 bytes
as configuration no-ops: there is no Bluetooth radio on these virtual USB
endpoints. The game trace established that blanket SET_FEATURE rejection was
aborting initialization. Report IDs and lengths are strictly checked; these
calls cannot change producer state, identity or feedback. All other public
feature writes remain denied. No command is forwarded to physical hardware.

Native .27 processes supported fields in mixed Sony output reports even when
audio or adaptive-trigger controls are also present. Unsupported fields are
ignored; they do not cause the whole rumble/light command to fail. Exact report
IDs and lengths remain checked. This does not add an audio/trigger engine.

This is a new UMDF2 HID minidriver and XUSB function driver, not a HIDMaestro fork. The Rust core owns
the fixed byte protocol, validation, HID descriptor and report encoding. This
small C binding owns the Windows framework handles, queued requests and device
lifetime. Revision .29 moves the actual 64-report FIFO, replay checking and
feedback merge into Rust. Native ABI tests exercise those exact functions;
there is no separate unconnected queue simulation. WDF keeps the per-device
lock across Rust peek, output copy and consume. It does not load a third-party driver DLL or contain a custom kernel
binary. The Windows inbox `mshidumdf` bridge and `WUDFRd` reflector are required.

## Device profiles and validation boundaries

Profile 1 is an original generic HID gamepad with 32 buttons, a hat, four signed
stick axes and two unsigned trigger axes. It is **not XInput**. Profiles 3 and 4
implement DualShock 4 and DualSense USB-compatible reports within the documented scope,
synthetic calibration/identity, motion and touch fields, rumble and supported
LED feedback. State encoders are original; public USB descriptors and firmware
metadata tables are adapted from MIT-licensed HIDMaestro with attribution in
THIRD-PARTY-NOTICES.txt and licenses/HIDMaestro-MIT.txt. They set the experimental
capability bit until device/game testing verifies compatibility. Profile 2 is
an Xbox-compatible XUSB function device whose Windows XInput discovery
and game compatibility are covered separately by installed validation. Steam output is not
implemented. Adaptive trigger programs and USB audio
are explicitly unsupported. Generic HID does not expose motion/touch or rumble.

The package currently targets x64 Windows 11, build 22000 or newer, because it
uses the modern inbox `MsHidUmdf.inf` and `WUDFRD.inf` installation sections.
The HID package binds `SCX\VirtualPad`; the separate XnaComposite-class INF binds
`SCX\VirtualXbox` for XUSB. Both INFs reference one DLL and one catalog. The HID control collection uses
the vendor-defined HID usage page `0xff53`, usage `1` for generic HID only.
Native .24 Sony profiles expose no private HID collection. Their authenticated
input, status and feedback use a service-provisioned fixed shared section and
blocking events; the driver validates and acknowledges every input packet.
Compatibility profiles
use the manufacturer/product identifiers expected by their consumers; the
generic profile uses vendor ID zero. No physical USB device is impersonated on
the USB bus. The service owns each unique PnP instance.

The service supplies immutable `DEVPROPKEY` values under GUID
`{19f81049-3679-40f2-9689-2a2f61f8bd29}`: property 2 is a `UINT32` profile,
property 3 a unique nonzero `UINT64` instance identity, and property 6 an opaque
bootstrap identifier as a 32-character lowercase hexadecimal `STRING`. They are read once at
device creation, checked for exact type/length, and never accepted over the
data channel. Missing optional profile/identity properties select the generic defaults;
unsupported profiles fail device creation. The bootstrap identifier
and its protected capability value are mandatory for all HID profiles.

The XUSB profile additionally accepts immutable `UINT32` property 4, player
index 0..3. The service reserves these four slots separately from its total
device budget, reflecting the standard XInput API's four-controller limit.

## Direct data path and multiple devices

SCX sends `HidD_SetFeature` with report `0x7a`, a 32-byte per-lease capability,
and the strict 64-byte versioned input packet: exactly 97 bytes. The obsolete
unauthenticated report `0x7e` is removed and denied. This is the documented HID control-collection route;
arbitrary private IOCTL routing through the HID class stack is not assumed.
The service is not in the per-report data path. `HidD_GetFeature` report `0x7f`
returns a 32-byte device-information block, never the secret capability.
Callers allocate the collection's maximum feature buffer size (97 bytes), and
the driver returns 33 meaningful
bytes including the report ID. Since native revision .24, Sony profiles use
the fixed, authenticated sideband channel for input, metadata and feedback.
There are no SCX feature reports or extra control collections on Sony HID.
Revision .29 deletes the unreachable .23 HID feedback handlers and unused
queue. The SDK retains legacy fallbacks only for older driver compatibility.
Output field masks are merged, so an
RGB update does not erase an existing rumble state. Feedback is per device and
has a monotonic sequence. Repeated identical outputs do not wake readers.

Each device has independent state, sequence checks, a fixed 64-report FIFO,
manual HID read queue, and a watchdog. Windows serializes callbacks within a
device; different devices have no shared SCX lock. Duplicate gamepad states
refresh liveness without waking HID readers. Different states are delivered
in order, including quick press/release transitions. Sony feedback uses its sideband event. A full input FIFO returns
`STATUS_DEVICE_BUSY` without accepting the frame or consuming its sequence.
The producer must retry/back off; silent transition loss is not success.

While a non-neutral state is held, producers must submit a fresh sequence at
least every 250 ms. After 500 ms without a valid frame, the watchdog discards
stale queued states and emits neutral. It wakes at most four times per second
while armed and does not run while neutral. There is no per-frame timer reset,
input polling thread, heap allocation, or service round trip. These are design
properties, **not measured end-to-end latency claims**.

HID reads remain framework-owned in a manual queue until data arrives. Windows
cancellation and device removal complete pending requests; the driver never
stores raw app pointers or holds a request after completing it. Queue admission
is bounded to 64 pending reads. Descriptor and report lengths are fixed, with
no registry-supplied descriptors or arbitrary memory access.

## Experimental XUSB function path

The XUSB profile runs directly under the inbox UMDF reflector, outside HIDClass.
The public XUSB interface implements versioned information, capabilities, state,
LED, battery, and rumble requests using the original Rust byte codec. Unknown
private extensions, guide waits and audio are rejected, rather than guessed.
XInput's state packet number advances only when encoded controls change; SCX
heartbeat timestamps and sequence numbers do not cause false state changes.

The separate SCX control interface `{582a87fd-a2dd-43f1-9224-d7c0dfdcf45d}` uses
the `SCXControl` reference. Private submit/info/feedback IOCTLs are rejected on
public-interface handles. `FILE_DEVICE_SECURE_OPEN` applies the device ACL to
the named reference as well. Submission takes exactly one Input64 packet;
feedback waits take the last observed sequence and complete only on change or
cancellation. The service is not in either data path. Feedback queues are bounded.

`xinput_probe.exe` is a separate test consumer linked only against Windows and
the C runtime, with no SCX core or client code. It loads Microsoft's system
`xinput1_4.dll` and reports each slot's state, capabilities and battery as JSON
lines. `--seconds 10` observes changes; `--slot 0 --rumble` explicitly requests
a 100 ms motor pulse followed by a zero-motor command. It never creates or
installs devices. A successful build alone is not proof of XInput compatibility.

## Access and installation

The service must supply a per-instance software-device security descriptor
granting access only to SYSTEM and the configured owner's SID. No Everyone,
Authenticated Users or generic interactive-user grant is introduced by the
INF. Windows HIDClass assigns its own ACL to child collections, so their
producer access is **not** secured by the parent PDO descriptor. Testing also
confirmed that HIDClass forwards submissions as kernel requests and does not
provide usable client impersonation. HID feature IOCTLs use `FILE_ANY_ACCESS`,
so a successful zero-access open is not an authorization boundary either.

The authenticated service instead generates a fresh 256-bit capability per
lease. It provides that capability to its authorized owner client through the
protected service pipe, and to the UMDF driver through a volatile registry
bootstrap value. `HKLM\SOFTWARE\SCX\VirtualPad\Bootstrap\<identifier>` grants
SYSTEM and elevated administrators full access, and the UMDF driver group read
access. Only the opaque lookup
identifier is published as a device property. The driver reads the exact 32-byte
nonzero `REG_BINARY` value once at creation; missing/malformed provisioning fails
closed. The service retains it for host restart during the lease and removes it
when the lease ends.

Every input submission compares all 32 capability bytes before touching the
input queue, sequence or watchdog. This needs no allocation, impersonation,
registry access or service round trip per input. The capability is never returned
by any HID read/feature/diagnostic and is securely cleared when the device context
is destroyed. This is a per-lease bearer capability: an authorized client must
not disclose it. Public gamepad readers do not gain producer access. Bounded
diagnostic feature `0x7c` reports status codes only, never capability data.
The driver runs in UMDF's user-mode host; device creation remains a privileged
service operation. Signing is not an authorization mechanism.

`scripts/check-wdk.ps1` locates VS2022 C++/SDK and an installed WDK or the
official Microsoft Windows WDK NuGet extraction. `scripts/build-driver.ps1`
links the Rust static library and the C binding, checks the INF with Microsoft's
InfVerif and creates the catalog with Inf2Cat. It does not install anything,
create certificates, alter boot options or add a trust anchor. The local
installer is a separate explicit administrative step.

## Design references and compatibility data

- [Microsoft: Creating WDF HID minidrivers](https://learn.microsoft.com/en-us/windows-hardware/drivers/wdf/creating-umdf-hid-minidrivers)
- [Microsoft: HID sample documentation explaining control collections](https://github.com/microsoft/Windows-driver-samples/tree/main/hid/vhidmini2)
- [Microsoft: UMDF SET_FEATURE marshalling](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidport/ni-hidport-ioctl_umdf_hid_set_feature)
- [Microsoft: UMDF GET_FEATURE marshalling](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidport/ni-hidport-ioctl_umdf_hid_get_feature)
- [Microsoft: WDF INF directives](https://learn.microsoft.com/en-us/windows-hardware/drivers/wdf/specifying-wdf-directives-in-inf-files)
- [Microsoft: UMDF client impersonation](https://learn.microsoft.com/en-us/windows-hardware/drivers/wdf/handling-client-impersonation-in-umdf-drivers)
- [Microsoft: Volatile registry keys and access control](https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regcreatekeyexw)

The Windows binding and private transport were written for SCX. Sony USB
compatibility tables retain the upstream MIT license. Standard WDF API usage
and inbox INF section names follow Microsoft's documented contracts. The WDK
headers and stub library are external Microsoft build dependencies and retain
their own terms; they are not relicensed by the SCX source license.

## Revision .29 diagnostic and ABI cleanup

The public generic HID descriptor no longer declares auth-stage report 0x7c.
Reports 0x78/0x79/0x7c/0x7d are not implemented by the current HID binding.
Xbox diagnostic IOCTL 0x00226010 now returns NOT_SUPPORTED. Legacy SDK diagnostic
helpers are for older revisions, not reliable counters for this revision.
Sony channel bytes 224..267 are reserved/unused; all functional offsets and the
512-byte channel version remain unchanged. No payload/flag snapshots or auth
stage counters are retained. ETW above remains opt-in and payload-free.

scx_device.h declares the private device context. scx_channel.c and scx_trace.c
are independent implementation units with guarded declaration headers. The
wire protocol continues explicit byte encoding (no packed-struct reinterpretation).
The Rust-owned in-process HidState has a separately checked C ABI layout and is
never exposed to producers. UMDF HID set requests carry their report ID in the
callback's output-length argument, as documented by Microsoft:
https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidport/ni-hidport-ioctl_umdf_hid_set_feature
