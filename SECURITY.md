# Security boundaries

SCX VPad separates controller input from privileged device management. The
Windows service may create and remove only allowlisted SCX virtual devices.
It does not execute client commands, accept paths to drivers or programs, install
drivers, change trust stores, or accept caller-supplied HID descriptors.

The administrator installs the service, the UMDF2 driver package and a local
signing certificate once. Ordinary SCX clients remain unelevated. The privileged
service is not removed from the threat model merely because UAC is no longer
shown on each connection.

The installer records an explicitly authorized interactive user's SID under
`HKLM\SOFTWARE\SCX\VirtualPad\InstallOwnerSid`. Service configuration and binaries
must be writable only by SYSTEM and Administrators. A named pipe rejects remote
clients, verifies the connecting Windows token and authorizes that configured
SID. Device ownership, bounded messages, quotas and teardown are checked by the
service. A PID supplied in a message is never authentication.

Windows HID collection ACLs do not inherit the software device's owner ACL,
and HID feature requests can be submitted through zero-access handles. The HID
bridge also does not preserve an impersonatable client token in the tested
configuration. Consequently, neither of those mechanisms authorizes SCX input.

Every HID device lease instead has a fresh, random 256-bit bearer capability.
The authenticated management pipe returns it only to an admitted owner client.
The SYSTEM service provisions a protected per-lease registry bootstrap key;
SYSTEM and elevated administrators can manage it; the Windows UMDF driver-host
identity can read it.
Ordinary users cannot read that key. The driver reads the capability at device
creation and compares all bytes before accepting any input, sequence update or
heartbeat. Legacy unauthenticated input is rejected. The capability is never
returned by a HID feature query or diagnostics. Bootstrap data remains through
the lease for driver-host restart and is removed when the lease ends.

The medium-integrity requirement applies when the service grants a lease.
A capability deliberately copied by its authorized owner delegates that lease's
input permission; it is not a per-packet assertion of the sender's token.
Other installed UMDF drivers belong to the trusted Windows driver-host boundary.
Xbox uses a separate restricted control interface and Windows handle access.

The isolation boundary is the configured Windows user account, not every process
inside that account. Another process running as the authorized user may open a
device's input interface. Lease creation/removal and quotas remain tied to the
authenticated client connection. This is not a sandbox for mutually distrustful
programs running under the same Windows identity.

Each virtual controller has independent state and bounded buffers. A lost owner
must cause device removal or neutralization; stale input must not persist.
Overflow must be reported and must never silently discard a button release.

Sony transport (.24): the service creates a 512-byte shared section and three
events before exposing the random lease identifier. Existing names are rejected.
Only SYSTEM, the UMDF group and the installed owner can access these objects;
a medium-integrity no-write-up label excludes low-integrity producers. Each
packet still carries the independent 256-bit lease capability. The driver copies
fixed atomic words, validates the local copy, and returns an explicit status.
Shared memory contains no pointers, variable lengths or queue indices. Feedback
uses atomic words under a sequence lock; consumers cannot change driver state
through the feedback area. The worker is joined before unmapping/freeing state.
Generic HID retains its authenticated feature transport; Xbox its private IOCTLs.

Native .33's wheel, flight-stick and arcade-stick classes use that same generic
HID transport and 256-bit lease capability. Their descriptor, product identity,
report size, usage pages and capabilities are fixed allowlisted Rust data.
The new classes accept no game-side output or force-feedback commands and cannot
be reconfigured into keyboard, mouse or arbitrary HID endpoints. A physical
device database or user mapping never supplies executable driver logic.

No certificate private key belongs in the repository or release archive. The
installer creates a machine-local, nonexportable signing key. It must never
disable Secure Boot, Memory Integrity, driver-signature enforcement, security
software, or test-signing policy. Locally trusted signatures do not constitute
Microsoft WHQL certification or an anti-cheat compatibility guarantee.

Source Guard is a separate, original UMDF2 upper filter. The published 0.1.0
Stable package qualifies the physical DualSense Edge Bluetooth collection.
The 0.1.0.37 preview also qualifies physical HID gamepad,
joystick, multi-axis and simulation collections, and the specific Valve
2026 input collection (VID 28DE, PID 1302–1305, usage FF00/0001). Its Rust policy
accepts bounded, authenticated per-device leases and random per-handle
capabilities; replayed or expired authorization cannot acquire source access.
The management service never proxies physical input reports. Client teardown
or lease expiry restores normal input access. The filter is not installed as a
HID class-wide filter and does not reserve keyboards, mice or arbitrary devices.
The source remains enumerable; this is input/output isolation, not device hiding.
No new kernel-mode SCX binary is installed. The Windows UMDF host/reflector is
part of the trusted platform, and administrative code remains outside this
isolation boundary.

The development candidate validates both the live HID descriptor and physical
USB/Bluetooth device ancestry in the service, independently of caller-supplied
IDs. The filter independently checks the selected device's collection usage.
Keyboard, mouse, consumer-control, virtual output and XInput compatibility
collections are excluded. Exact per-model INF entries are required; broad HID
class or reserved usage-ID matching is not used. SourceModels.ps1 accepts only
bounded, exact model IDs and rejects instance paths, wildcards and INF syntax.
Model discovery emits model IDs only, never serial numbers or instance paths.
USB model entries must retain their revision; bare Sony VID/PID matches are
excluded because those IDs can also bind virtual outputs. The filter rejects
SCX virtual hardware identities as an additional qualification check. The
unreleased .36 candidate exposed this attachment issue and was superseded by .37.
An uninstalled or unintegrated source is not reported as protected. The normal
app uses per-handle authorization for native HID, DualSense and Steam Controller
input/feedback paths. SDL-owned and XInput-only handles are not covered by this
candidate. New model installation still requires administrative setup; acquiring
or releasing an already installed source lease does not.

Release packages are explicit file allowlists with SHA-256 and byte lengths,
bounded total size and a version/channel/platform declaration. The Setup EXE
embeds the exact checked ZIP and validates its contents before protected staging.
Stable builds require the complete Source Guard payload and matching DLL/INF
versions. Removal uses journaled ownership and exact INF hashes, never a broad
search for similarly named drivers. These checks provide consistency and safe
scoping; a public checksum is not a substitute for publisher Authenticode signing.

Please report security issues privately to **Tim Haller <tim.haller@gmail.com>**,
or using GitHub private vulnerability reporting when enabled. Do not post exploitable details or private data in a
public issue. Review the current artifact-specific validation record before
deployment; stable channel status does not imply an independent security audit,
WHQL certification or an all-device/all-game compatibility guarantee.

