#include "scx_device.h"
#include "scx_trace.h"
/* Opt-in ETW metadata for diagnosing native Sony consumers. Disabled unless
 * an administrator explicitly enables this provider. No input/output payload,
 * capability, user SID, or shared-channel name is recorded. No per-frame input
 * tracing. One successful GET_INPUT_REPORT and failed reads are included; no
 * report payload is logged. This path never changes the request's result.
 */
#include <TraceLoggingProvider.h>
#include <winmeta.h>
TRACELOGGING_DEFINE_PROVIDER(ScxTraceProvider, "SCX-VPad-Diagnostics",
    (0x05b6b2fc,0xa45b,0x4b7c,0x8c,0x94,0x3b,0x2c,0xe3,0x86,0xb1,0xe5));

void ScxTraceRequest(SCX_DEVICE *ctx, WDFREQUEST request,
                           ULONG code, size_t inLength, size_t outLength, NTSTATUS status)
{
    ULONG selector = 0xffffffffu;
    WDFMEMORY memory;
    const uint8_t *data;
    size_t length;
    if ((ctx->Profile != SCX_PROFILE_DUALSHOCK4 && ctx->Profile != SCX_PROFILE_DUALSENSE) ||
        !TraceLoggingProviderEnabled(ScxTraceProvider, WINEVENT_LEVEL_INFO, 1)) return;
    if (code == IOCTL_HID_READ_REPORT && NT_SUCCESS(status)) return;
    if (code == IOCTL_UMDF_HID_GET_INPUT_REPORT && NT_SUCCESS(status)) {
        if (ctx->InputReportTraceSeen) return;
        ctx->InputReportTraceSeen = TRUE;
    }
    if (code == IOCTL_UMDF_HID_GET_INPUT_REPORT || code == IOCTL_UMDF_HID_GET_FEATURE || code == IOCTL_UMDF_HID_SET_FEATURE ||
        code == IOCTL_HID_WRITE_REPORT || code == IOCTL_UMDF_HID_SET_OUTPUT_REPORT ||
        code == IOCTL_HID_GET_STRING) {
        if (NT_SUCCESS(WdfRequestRetrieveInputMemory(request, &memory))) {
            data = (const uint8_t *)WdfMemoryGetBuffer(memory, &length);
            if (length >= 1) selector = data[0];
            if (code == IOCTL_HID_GET_STRING && length >= sizeof(ULONG))
                memcpy(&selector, data, sizeof(ULONG));
        }
    }
    TraceLoggingWrite(ScxTraceProvider, "SonyRequest",
        TraceLoggingLevel(WINEVENT_LEVEL_INFO), TraceLoggingKeyword(1),
        TraceLoggingUInt32(ctx->Profile, "Profile"),
        TraceLoggingUInt64(ctx->InstanceIdentity, "PublicInstance"),
        TraceLoggingUInt32(WdfRequestGetRequestorProcessId(request), "RequestorPid"),
        TraceLoggingHexUInt32(code, "Ioctl"),
        TraceLoggingUInt32(selector, "Selector"),
        TraceLoggingUInt64((UINT64)inLength, "InputLength"),
        TraceLoggingUInt64((UINT64)outLength, "OutputLength"),
        TraceLoggingHexInt32(status, "Status"),
        TraceLoggingUInt64((UINT64)WdfRequestGetInformation(request), "Transferred"));
}

void ScxTraceUnload(WDFDRIVER driver)
{
    UNREFERENCED_PARAMETER(driver);
    TraceLoggingUnregister(ScxTraceProvider);
}

void ScxTraceInitialize(void) { (void)TraceLoggingRegister(ScxTraceProvider); }
void ScxTraceShutdown(void) { TraceLoggingUnregister(ScxTraceProvider); }
