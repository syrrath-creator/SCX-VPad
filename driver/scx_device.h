#ifndef SCX_DEVICE_H
#define SCX_DEVICE_H
#include "scx_windows.h"
typedef struct _SCX_DEVICE {
    HANDLE ChannelMap, ChannelInput, ChannelDone, ChannelFeedback, ChannelStop, ChannelThread;
    volatile LONG *ChannelWords;
    WDFQUEUE Reads;
    WDFTIMER Watchdog;
    HID_DESCRIPTOR Hid;
    const uint8_t *Descriptor;
    size_t DescriptorLength, ReportLength;
    uint32_t Profile;
    uint64_t InstanceIdentity;
    USHORT VendorId, ProductId, ProductVersion;
    SCX_VPAD_HID_STATE Input;
    uint8_t Feedback[SCX_VPAD_FEEDBACK_PACKET_SIZE];
    ULONGLONG LastSubmitMs;
    BOOLEAN WatchdogArmed, Online;
    uint8_t Capability[SCX_VPAD_AUTH_CAPABILITY_SIZE];
    BOOLEAN CapabilityLoaded;
    BOOLEAN InputReportTraceSeen;
} SCX_DEVICE;
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(SCX_DEVICE, ScxContext)

extern const DEVPROPKEY ScxBootstrapKey;
NTSTATUS ScxReadCreationProperty(WDFDEVICE device, const DEVPROPKEY *key,
    void *value, ULONG size, DEVPROPTYPE expected);
NTSTATUS ScxAcceptPacket(SCX_DEVICE *ctx, const uint8_t *bytes);
#endif
