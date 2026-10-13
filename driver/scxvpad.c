/* Copyright (c) 2026 SCX. See LICENSE at the repository root.
 * Original UMDF2 binding. Windows owns PnP, HID collection routing and request
 * cancellation; Rust owns the byte protocol and the HID descriptor/encoder.
 */
#define WIN32_NO_STATUS
#pragma warning(push)
#pragma warning(disable: 6553) /* Windows SDK RegOpenKeyEx SAL annotation. */
#include <windows.h>
#pragma warning(pop)
#undef WIN32_NO_STATUS
#pragma warning(push)
/* WDK 2.25 intentionally uses aligned structures and its legacy assert macro. */
#pragma warning(disable: 4324 5104 6553)
#include <wdf.h>
#include <hidport.h>
#pragma warning(pop)
#include <stdint.h>
#include <string.h>
#include "scx_vpad.h"

#define SCX_REPORT_BYTES 64u
#define SCX_FRAME_BYTES 64u
#define SCX_RING_SIZE 64u
#define SCX_FEATURE_SUBMIT 0x7au
#define SCX_FEATURE_INFO 0x7fu
#define SCX_FEEDBACK_REPORT 0x7du
#define SCX_STALE_MS 500ull

static const DEVPROPKEY ScxProfileKey = {
    {0x19f81049,0x3679,0x40f2,{0x96,0x89,0x2a,0x2f,0x61,0xf8,0xbd,0x29}}, 2
};
static const DEVPROPKEY ScxIdentityKey = {
    {0x19f81049,0x3679,0x40f2,{0x96,0x89,0x2a,0x2f,0x61,0xf8,0xbd,0x29}}, 3
};
static const DEVPROPKEY ScxBootstrapKey = {
    {0x19f81049,0x3679,0x40f2,{0x96,0x89,0x2a,0x2f,0x61,0xf8,0xbd,0x29}}, 6
};

typedef struct _SCX_DEVICE {
    WDFQUEUE Reads;
    WDFTIMER Watchdog;
    HID_DESCRIPTOR Hid;
    const uint8_t *Descriptor;
    size_t DescriptorLength;
    size_t ReportLength;
    uint32_t Profile;
    uint64_t InstanceIdentity;
    USHORT VendorId;
    USHORT ProductId;
    USHORT ProductVersion;
    uint8_t Reports[SCX_RING_SIZE][SCX_REPORT_BYTES];
    size_t ReportSizes[SCX_RING_SIZE];
    uint8_t Current[SCX_REPORT_BYTES];
    uint8_t Neutral[SCX_REPORT_BYTES];
    uint8_t Feedback[33];
    ULONG Head;
    ULONG Count;
    uint32_t LastSequence;
    ULONGLONG LastSubmitMs;
    BOOLEAN HasSequence;
    BOOLEAN WatchdogArmed;
    BOOLEAN Online;
    uint8_t Capability[32];
    BOOLEAN CapabilityLoaded;
    uint32_t AuthStage;
    uint32_t AuthError;
    uint32_t AuthRequestorMode;
    uint32_t OutputDiagnostic[8]; /* Counts/length/flags only; no report payload. */
} SCX_DEVICE;
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(SCX_DEVICE, ScxContext)

DRIVER_INITIALIZE DriverEntry;
EVT_WDF_DRIVER_DEVICE_ADD ScxDeviceAdd;
EVT_WDF_IO_QUEUE_IO_DEVICE_CONTROL ScxIoControl;
EVT_WDF_IO_QUEUE_IO_CANCELED_ON_QUEUE ScxCanceled;
EVT_WDF_TIMER ScxWatchdog;
EVT_WDF_DEVICE_SURPRISE_REMOVAL ScxRemoved;
EVT_WDF_OBJECT_CONTEXT_DESTROY ScxDestroy;
NTSTATUS ScxXboxDeviceAdd(WDFDRIVER driver, PWDFDEVICE_INIT init);

/* HIDClass does not preserve the original producer token. The trusted service
 * provisions a separate per-lease 256-bit capability under a volatile registry
 * key readable only by SYSTEM and the UMDF driver group. The public device
 * property is an opaque lookup ID, never the capability itself. */
static NTSTATUS ScxReadProducerCapability(WDFDEVICE device, SCX_DEVICE *ctx)
{
    WDF_DEVICE_PROPERTY_DATA property;
    DEVPROPTYPE propertyType;
    ULONG required;
    WCHAR bootId[33] = {0}, path[128];
    static const WCHAR prefix[] = L"SOFTWARE\\SCX\\VirtualPad\\Bootstrap\\";
    HKEY key = NULL;
    DWORD valueType, valueLength = sizeof(ctx->Capability);
    unsigned index;
    uint8_t nonzero = 0;
    LSTATUS registryStatus;
    NTSTATUS status;
    WDF_DEVICE_PROPERTY_DATA_INIT(&property, &ScxBootstrapKey);
    status = WdfDeviceQueryPropertyEx(device, &property, sizeof(bootId), bootId, &required, &propertyType);
    if (!NT_SUCCESS(status) || propertyType != DEVPROP_TYPE_STRING ||
        required != sizeof(bootId) || bootId[32] != L'\0') return STATUS_ACCESS_DENIED;
    for (index = 0; index < 32; ++index)
        if (!((bootId[index] >= L'0' && bootId[index] <= L'9') ||
              (bootId[index] >= L'a' && bootId[index] <= L'f'))) return STATUS_ACCESS_DENIED;
    _Static_assert(sizeof(prefix) + sizeof(bootId) - sizeof(WCHAR) <= sizeof(path),
                   "Bootstrap path has fixed sufficient capacity");
    memcpy(path, prefix, sizeof(prefix) - sizeof(WCHAR));
    memcpy(path + sizeof(prefix) / sizeof(WCHAR) - 1, bootId, sizeof(bootId));
    registryStatus = RegOpenKeyExW(HKEY_LOCAL_MACHINE, path, 0,
                                   KEY_QUERY_VALUE | KEY_WOW64_64KEY, &key);
    if (registryStatus != ERROR_SUCCESS) return STATUS_ACCESS_DENIED;
    registryStatus = RegQueryValueExW(key, L"Capability", NULL, &valueType,
                                     ctx->Capability, &valueLength);
    RegCloseKey(key);
    if (registryStatus != ERROR_SUCCESS || valueType != REG_BINARY ||
        valueLength != sizeof(ctx->Capability)) {
        SecureZeroMemory(ctx->Capability, sizeof(ctx->Capability));
        return STATUS_ACCESS_DENIED;
    }
    for (index = 0; index < sizeof(ctx->Capability); ++index) nonzero |= ctx->Capability[index];
    if (nonzero == 0) return STATUS_ACCESS_DENIED;
    ctx->CapabilityLoaded = TRUE;
    return STATUS_SUCCESS;
}

static NTSTATUS ScxAuthorizeProducer(SCX_DEVICE *ctx, WDFREQUEST request, const uint8_t *capability)
{
    volatile uint8_t difference = 0;
    unsigned index;
    /* Volatile accumulation prevents an early-exit comparison; all 32 bytes
     * are visited before any sequence/report/heartbeat state can be changed. */
    for (index = 0; index < sizeof(ctx->Capability); ++index)
        difference |= capability[index] ^ ctx->Capability[index];
    ctx->AuthRequestorMode = (uint32_t)WdfRequestGetRequestorMode(request);
    ctx->AuthStage = ctx->CapabilityLoaded && difference == 0 ? 9u : 8u;
    ctx->AuthError = ctx->AuthStage == 9u ? 0 : ERROR_ACCESS_DENIED;
    return ctx->AuthStage == 9u ? STATUS_SUCCESS : STATUS_ACCESS_DENIED;
}

void ScxDestroy(WDFOBJECT object)
{
    SCX_DEVICE *ctx = ScxContext((WDFDEVICE)object);
    SecureZeroMemory(ctx->Capability, sizeof(ctx->Capability));
    ctx->CapabilityLoaded = FALSE;
}
static NTSTATUS ScxReadCreationProperty(WDFDEVICE device, const DEVPROPKEY *key,
                                        void *value, ULONG size, DEVPROPTYPE expected)
{
    WDF_DEVICE_PROPERTY_DATA property;
    DEVPROPTYPE actual;
    ULONG required;
    NTSTATUS status;
    WDF_DEVICE_PROPERTY_DATA_INIT(&property, key);
    status = WdfDeviceQueryPropertyEx(device, &property, size, value, &required, &actual);
    /* Backward-compatible generic creation without custom properties only. */
    if (status == STATUS_OBJECT_NAME_NOT_FOUND || status == STATUS_NOT_FOUND ||
        status == STATUS_OBJECT_PATH_NOT_FOUND) return STATUS_SUCCESS;
    if (!NT_SUCCESS(status)) return status;
    if (actual != expected || required != size) return STATUS_INVALID_PARAMETER;
    return STATUS_SUCCESS;
}

/* Device synchronization scope serializes all queue/timer callbacks for this
 * device only. No global lock, allocation or cross-device state in the hot path.
 */
static NTSTATUS ScxCopy(WDFREQUEST request, const void *source, size_t length)
{
    WDFMEMORY output;
    size_t capacity;
    if (length == 0 || source == NULL) return STATUS_INVALID_PARAMETER;
    NTSTATUS status = WdfRequestRetrieveOutputMemory(request, &output);
    if (!NT_SUCCESS(status)) return status;
    (void)WdfMemoryGetBuffer(output, &capacity);
    if (capacity < length) return STATUS_BUFFER_TOO_SMALL;
    status = WdfMemoryCopyFromBuffer(output, 0, (PVOID)source, length);
    if (NT_SUCCESS(status)) WdfRequestSetInformation(request, length);
    return status;
}

static void ScxDrain(SCX_DEVICE *ctx)
{
    WDFREQUEST read;
    while (ctx->Count != 0 &&
           NT_SUCCESS(WdfIoQueueRetrieveNextRequest(ctx->Reads, &read))) {
        NTSTATUS status = ScxCopy(read, ctx->Reports[ctx->Head], ctx->ReportSizes[ctx->Head]);
        /* A malformed reader must not consume a queued button transition. */
        if (NT_SUCCESS(status)) {
            ctx->Head = (ctx->Head + 1u) % SCX_RING_SIZE;
            --ctx->Count;
        }
        WdfRequestComplete(read, status);
    }
}

static void ScxNeutralize(SCX_DEVICE *ctx)
{
    /* Safety takes precedence over stale queued presses after producer loss. */
    ctx->Head = 0;
    ctx->Count = 1;
    memcpy(ctx->Current, ctx->Neutral, ctx->ReportLength);
    memcpy(ctx->Reports[0], ctx->Neutral, ctx->ReportLength);
    ctx->ReportSizes[0] = ctx->ReportLength;
    ScxDrain(ctx);
}

void ScxWatchdog(WDFTIMER timer)
{
    SCX_DEVICE *ctx = ScxContext((WDFDEVICE)WdfTimerGetParentObject(timer));
    if (!ctx->Online || !ctx->WatchdogArmed) {
        ctx->WatchdogArmed = FALSE;
        return;
    }
    if (GetTickCount64() - ctx->LastSubmitMs >= SCX_STALE_MS) {
        ScxNeutralize(ctx);
        ctx->WatchdogArmed = FALSE;
        return;
    }
    /* This one-shot wake-up runs only while a non-neutral state is held.
     * Submits update a timestamp, never restart a timer at input frequency. */
    (void)WdfTimerStart(timer, WDF_REL_TIMEOUT_IN_MS(250));
}

static NTSTATUS ScxSubmit(SCX_DEVICE *ctx, WDFREQUEST request, size_t reportId)
{
    WDFMEMORY memory;
    size_t length;
    const uint8_t *bytes;
    uint8_t report[SCX_REPORT_BYTES];
    uint8_t neutral;
    uint32_t sequence;
    NTSTATUS status;
    if (!ctx->Online) return STATUS_DEVICE_NOT_READY;
    if (reportId != SCX_FEATURE_SUBMIT) return STATUS_ACCESS_DENIED;
    status = WdfRequestRetrieveInputMemory(request, &memory);
    if (!NT_SUCCESS(status)) return status;
    bytes = (const uint8_t *)WdfMemoryGetBuffer(memory, &length);
    if (length != SCX_FRAME_BYTES + 33u || bytes[0] != SCX_FEATURE_SUBMIT)
        return STATUS_INVALID_BUFFER_SIZE;
    status = ScxAuthorizeProducer(ctx, request, bytes + 1);
    if (!NT_SUCCESS(status)) return status;
    if (scx_vpad_encode_report_for_profile(ctx->Profile, bytes + 33, SCX_FRAME_BYTES,
                                          report, sizeof(report)) != 0)
        return STATUS_INVALID_PARAMETER;
    if (scx_vpad_input_is_neutral(bytes + 33, SCX_FRAME_BYTES, &neutral) != 0)
        return STATUS_INVALID_PARAMETER;
    memcpy(&sequence, bytes + 37, sizeof(sequence));
    /* Wrapping sequence arithmetic; reject duplicates and old/replayed frames. */
    if (ctx->HasSequence && (int32_t)(sequence - ctx->LastSequence) <= 0)
        return STATUS_INVALID_PARAMETER;
    if (memcmp(report, ctx->Current, ctx->ReportLength) != 0) {
        ScxDrain(ctx);
        if (ctx->Count == SCX_RING_SIZE) return STATUS_DEVICE_BUSY;
        memcpy(ctx->Reports[(ctx->Head + ctx->Count) % SCX_RING_SIZE],
               report, ctx->ReportLength);
        ctx->ReportSizes[(ctx->Head + ctx->Count) % SCX_RING_SIZE] = ctx->ReportLength;
        ++ctx->Count;
        memcpy(ctx->Current, report, ctx->ReportLength);
    }
    ctx->LastSequence = sequence;
    ctx->HasSequence = TRUE;
    ctx->LastSubmitMs = GetTickCount64();
    if (!ctx->WatchdogArmed && !neutral) {
        ctx->WatchdogArmed = TRUE;
        (void)WdfTimerStart(ctx->Watchdog, WDF_REL_TIMEOUT_IN_MS(250));
    }
    if (ctx->WatchdogArmed && neutral) {
        (void)WdfTimerStop(ctx->Watchdog, FALSE);
        ctx->WatchdogArmed = FALSE;
    }
    ScxDrain(ctx);
    WdfRequestSetInformation(request, length);
    return STATUS_SUCCESS;
}

static NTSTATUS ScxGetFeature(SCX_DEVICE *ctx, WDFREQUEST request, BOOLEAN input)
{
    WDFMEMORY memory;
    size_t length;
    const uint8_t *id;
    uint8_t info[33] = {SCX_FEATURE_INFO};
    uint8_t feature[SCX_REPORT_BYTES];
    size_t featureLength;
    NTSTATUS status = WdfRequestRetrieveInputMemory(request, &memory);
    if (!NT_SUCCESS(status)) return status;
    id = (const uint8_t *)WdfMemoryGetBuffer(memory, &length);
    if (length < 1) return STATUS_INVALID_BUFFER_SIZE;
    if (!input && id[0] == 0x79) {
        uint8_t diagnostic[33] = {0x79};
        memcpy(diagnostic + 1, ctx->OutputDiagnostic, sizeof(ctx->OutputDiagnostic));
        return ScxCopy(request, diagnostic, sizeof(diagnostic));
    }
    if (!input && id[0] < 0x79) { ++ctx->OutputDiagnostic[6]; ctx->OutputDiagnostic[7] = id[0]; }
    if (!input && id[0] == 0x7c) {
        uint8_t diagnostic[17] = {0x7c,1,0,16,0};
        memcpy(diagnostic + 5, &ctx->AuthStage, 4);
        memcpy(diagnostic + 9, &ctx->AuthError, 4);
        memcpy(diagnostic + 13, &ctx->AuthRequestorMode, 4);
        return ScxCopy(request, diagnostic, sizeof(diagnostic));
    }
    if (input && id[0] == 1)
        return ScxCopy(request, ctx->Current, ctx->ReportLength);
    if (!input && id[0] == SCX_FEATURE_INFO) {
        if (scx_vpad_get_info_for_profile(ctx->Profile, info + 1, 32) != 0)
            return STATUS_INTERNAL_ERROR;
        return ScxCopy(request, info, sizeof(info));
    }
    if (id[0] == SCX_FEEDBACK_REPORT && ctx->Profile != 1)
        return ScxCopy(request, ctx->Feedback, sizeof(ctx->Feedback));
    if (!input && scx_vpad_feature_report_for_profile(ctx->Profile, id[0],
                ctx->InstanceIdentity, feature, sizeof(feature), &featureLength) == 0)
        return ScxCopy(request, feature, featureLength);
    return STATUS_NOT_SUPPORTED;
}

static NTSTATUS ScxOutput(SCX_DEVICE *ctx, WDFREQUEST request, size_t reportId)
{
    WDFMEMORY memory;
    size_t length;
    const uint8_t *bytes;
    uint8_t decoded[32];
    uint8_t next[33];
    uint32_t incomingFields, combinedFields, sequence;
    NTSTATUS status;
    uint32_t decodedStatus;
    if (!ctx->Online) return STATUS_DEVICE_NOT_READY;
    status = WdfRequestRetrieveInputMemory(request, &memory);
    if (!NT_SUCCESS(status)) return status;
    bytes = (const uint8_t *)WdfMemoryGetBuffer(memory, &length);
    ++ctx->OutputDiagnostic[0];
    ctx->OutputDiagnostic[2] = (uint32_t)length;
    ctx->OutputDiagnostic[3] = (uint32_t)reportId;
    ctx->OutputDiagnostic[4] = (length > 1 ? bytes[1] : 0) | (length > 2 ? ((uint32_t)bytes[2] << 8) : 0) | (length > 39 ? ((uint32_t)bytes[39] << 16) : 0);
    if (length == 0 || reportId > 255 || bytes[0] != (uint8_t)reportId)
        return STATUS_INVALID_PARAMETER;
    decodedStatus = scx_vpad_parse_output_for_profile(ctx->Profile, bytes, length,
                                          decoded, sizeof(decoded));
    ctx->OutputDiagnostic[5] = decodedStatus;
    if (decodedStatus != 0)
        return STATUS_NOT_SUPPORTED;
    ++ctx->OutputDiagnostic[1];
    memcpy(next, ctx->Feedback, sizeof(next));
    memcpy(&incomingFields, decoded + 8, sizeof(incomingFields));
    memcpy(&combinedFields, next + 9, sizeof(combinedFields));
    combinedFields |= incomingFields;
    memcpy(next + 9, &combinedFields, sizeof(combinedFields));
    /* Valid-field merging preserves rumble across unrelated LED updates. */
    if (incomingFields & 1u) memcpy(next + 13, decoded + 12, 4);
    if (incomingFields & 2u) memcpy(next + 17, decoded + 16, 3);
    if (incomingFields & 4u) next[20] = decoded[19];
    if (incomingFields & 8u) next[21] = decoded[20];
    if (incomingFields & 16u) memcpy(next + 22, decoded + 21, 2);
    if (memcmp(next, ctx->Feedback, sizeof(next)) != 0) {
        ScxDrain(ctx);
        if (ctx->Count == SCX_RING_SIZE) return STATUS_DEVICE_BUSY;
        memcpy(&sequence, next + 5, sizeof(sequence));
        ++sequence;
        memcpy(next + 5, &sequence, sizeof(sequence));
        memcpy(ctx->Feedback, next, sizeof(next));
        memcpy(ctx->Reports[(ctx->Head + ctx->Count) % SCX_RING_SIZE], next, sizeof(next));
        ctx->ReportSizes[(ctx->Head + ctx->Count) % SCX_RING_SIZE] = sizeof(next);
        ++ctx->Count;
        ScxDrain(ctx);
    }
    WdfRequestSetInformation(request, length);
    return STATUS_SUCCESS;
}

static NTSTATUS ScxString(SCX_DEVICE *ctx, WDFREQUEST request)
{
    WDFMEMORY memory;
    size_t length;
    const void *selectorBuffer;
    ULONG selector;
    const WCHAR *text;
    NTSTATUS status = WdfRequestRetrieveInputMemory(request, &memory);
    if (!NT_SUCCESS(status)) return status;
    selectorBuffer = WdfMemoryGetBuffer(memory, &length);
    if (length < sizeof(ULONG)) return STATUS_INVALID_BUFFER_SIZE;
    memcpy(&selector, selectorBuffer, sizeof(selector));
    switch (selector & 0xffffu) {
    case HID_STRING_ID_IMANUFACTURER: text = L"SCX"; break;
    case HID_STRING_ID_IPRODUCT:
        text = ctx->Profile == 3 ? L"SCX DualShock 4" : ctx->Profile == 4 ? L"SCX DualSense" : L"SCX Virtual Gamepad";
        break;
    /* Instance identity is supplied by the service's unique PnP instance ID;
     * no misleading shared serial number across simultaneous controllers. */
    case HID_STRING_ID_ISERIALNUMBER: text = L""; break;
    default: return STATUS_NOT_SUPPORTED;
    }
    return ScxCopy(request, text, (wcslen(text) + 1u) * sizeof(WCHAR));
}

void ScxIoControl(WDFQUEUE queue, WDFREQUEST request, size_t outLength,
                  size_t inLength, ULONG code)
{
    SCX_DEVICE *ctx = ScxContext(WdfIoQueueGetDevice(queue));
    NTSTATUS status;
    HID_DEVICE_ATTRIBUTES attributes;
    UNREFERENCED_PARAMETER(inLength);
    switch (code) {
    case IOCTL_HID_GET_DEVICE_DESCRIPTOR:
        status = ScxCopy(request, &ctx->Hid, sizeof(ctx->Hid)); break;
    case IOCTL_HID_GET_REPORT_DESCRIPTOR:
        status = ScxCopy(request, ctx->Descriptor, ctx->DescriptorLength); break;
    case IOCTL_HID_GET_DEVICE_ATTRIBUTES:
        memset(&attributes, 0, sizeof(attributes));
        attributes.Size = sizeof(attributes);
        /* Generic uses an unassigned software-HID identity; experimental
         * compatibility profiles use the IDs required by their consumers. */
        attributes.VendorID = ctx->VendorId;
        attributes.ProductID = ctx->ProductId;
        attributes.VersionNumber = ctx->ProductVersion;
        status = ScxCopy(request, &attributes, sizeof(attributes)); break;
    case IOCTL_HID_READ_REPORT:
        if (!ctx->Online) { status = STATUS_DEVICE_NOT_READY; break; }
        {
            ULONG waiting;
            (void)WdfIoQueueGetState(ctx->Reads, &waiting, NULL);
            if (waiting >= SCX_RING_SIZE) { status = STATUS_DEVICE_BUSY; break; }
        }
        status = WdfRequestForwardToIoQueue(request, ctx->Reads);
        if (NT_SUCCESS(status)) { ScxDrain(ctx); return; }
        break;
    case IOCTL_UMDF_HID_SET_FEATURE:
        status = ScxSubmit(ctx, request, outLength); break;
    case IOCTL_UMDF_HID_GET_FEATURE:
        status = ScxGetFeature(ctx, request, FALSE); break;
    case IOCTL_UMDF_HID_GET_INPUT_REPORT:
        status = ScxGetFeature(ctx, request, TRUE); break;
    case IOCTL_HID_WRITE_REPORT:
    case IOCTL_UMDF_HID_SET_OUTPUT_REPORT:
        status = ScxOutput(ctx, request, outLength); break;
    case IOCTL_HID_GET_STRING:
        status = ScxString(ctx, request); break;
    case IOCTL_HID_ACTIVATE_DEVICE:
        ctx->Online = TRUE;
        WdfIoQueueStart(ctx->Reads);
        status = STATUS_SUCCESS; break;
    case IOCTL_HID_DEACTIVATE_DEVICE:
        ctx->Online = FALSE;
        (void)WdfTimerStop(ctx->Watchdog, FALSE);
        ctx->WatchdogArmed = FALSE;
        ScxNeutralize(ctx);
        WdfIoQueuePurge(ctx->Reads, NULL, NULL);
        status = STATUS_SUCCESS; break;
    default:
        status = STATUS_NOT_SUPPORTED; break;
    }
    WdfRequestComplete(request, status);
}

void ScxCanceled(WDFQUEUE queue, WDFREQUEST request)
{
    UNREFERENCED_PARAMETER(queue);
    WdfRequestComplete(request, STATUS_CANCELLED);
}

void ScxRemoved(WDFDEVICE device)
{
    SCX_DEVICE *ctx = ScxContext(device);
    /* PnP surprise-removal is not covered by automatic I/O synchronization.
     * Explicitly join the device callback lock before touching report state. */
    WdfObjectAcquireLock(device);
    ctx->Online = FALSE;
    (void)WdfTimerStop(ctx->Watchdog, FALSE);
    ctx->WatchdogArmed = FALSE;
    ctx->Count = 0;
    WdfObjectReleaseLock(device);
    WdfIoQueuePurge(ctx->Reads, NULL, NULL);
}

NTSTATUS ScxDeviceAdd(WDFDRIVER driver, PWDFDEVICE_INIT init)
{
    WDF_OBJECT_ATTRIBUTES attributes;
    WDF_PNPPOWER_EVENT_CALLBACKS power;
    WDF_IO_QUEUE_CONFIG queue;
    WDF_TIMER_CONFIG timer;
    WDFDEVICE device;
    SCX_DEVICE *ctx;
    WDF_DEVICE_PROPERTY_DATA creationProperty;
    DEVPROPTYPE creationType;
    ULONG creationRequired;
    uint32_t creationProfile = 1;
    NTSTATUS status;
    UNREFERENCED_PARAMETER(driver);
    WDF_DEVICE_PROPERTY_DATA_INIT(&creationProperty, &ScxProfileKey);
    status = WdfFdoInitQueryPropertyEx(init, &creationProperty, sizeof(creationProfile),
                                      &creationProfile, &creationRequired, &creationType);
    if (NT_SUCCESS(status)) {
        if (creationRequired != sizeof(creationProfile) || creationType != DEVPROP_TYPE_UINT32)
            return STATUS_INVALID_PARAMETER;
        if (creationProfile == 2) return ScxXboxDeviceAdd(driver, init);
    } else if (status != STATUS_NOT_FOUND && status != STATUS_OBJECT_NAME_NOT_FOUND &&
               status != STATUS_OBJECT_PATH_NOT_FOUND) return status;
    WdfFdoInitSetFilter(init);
    WDF_PNPPOWER_EVENT_CALLBACKS_INIT(&power);
    power.EvtDeviceSurpriseRemoval = ScxRemoved;
    WdfDeviceInitSetPnpPowerEventCallbacks(init, &power);
    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, SCX_DEVICE);
    attributes.SynchronizationScope = WdfSynchronizationScopeDevice;
    attributes.ExecutionLevel = WdfExecutionLevelPassive;
    attributes.EvtDestroyCallback = ScxDestroy;
    status = WdfDeviceCreate(&init, &attributes, &device);
    if (!NT_SUCCESS(status)) return status;
    ctx = ScxContext(device);
    status = ScxReadProducerCapability(device, ctx);
    if (!NT_SUCCESS(status)) return status;
    ctx->Profile = 1;
    status = ScxReadCreationProperty(device, &ScxProfileKey, &ctx->Profile,
                                    sizeof(ctx->Profile), DEVPROP_TYPE_UINT32);
    if (!NT_SUCCESS(status)) return status;
    ctx->ReportLength = scx_vpad_profile_report_size(ctx->Profile);
    if (ctx->ReportLength == 0 || ctx->ReportLength > SCX_REPORT_BYTES)
        return STATUS_NOT_SUPPORTED;
    status = ScxReadCreationProperty(device, &ScxIdentityKey, &ctx->InstanceIdentity,
                                    sizeof(ctx->InstanceIdentity), DEVPROP_TYPE_UINT64);
    if (!NT_SUCCESS(status)) return status;
    if (ctx->Profile != 1 && ctx->InstanceIdentity == 0)
        return STATUS_INVALID_PARAMETER;
    if (scx_vpad_get_hid_attributes(ctx->Profile, &ctx->VendorId, &ctx->ProductId,
                                     &ctx->ProductVersion) != 0)
        return STATUS_NOT_SUPPORTED;
    ctx->Feedback[0] = SCX_FEEDBACK_REPORT;
    ctx->Feedback[1] = 1;
    ctx->Feedback[3] = 32;
    memcpy(ctx->Feedback + 25, &ctx->Profile, sizeof(ctx->Profile));
    ctx->Descriptor = scx_vpad_report_descriptor_for_profile(ctx->Profile, &ctx->DescriptorLength);
    if (ctx->Descriptor == NULL || ctx->DescriptorLength == 0 || ctx->DescriptorLength > 65535u)
        return STATUS_INVALID_PARAMETER;
    if (scx_vpad_neutral_report_for_profile(ctx->Profile, ctx->Neutral, sizeof(ctx->Neutral)) != 0)
        return STATUS_INTERNAL_ERROR;
    memcpy(ctx->Current, ctx->Neutral, ctx->ReportLength);
    ctx->Hid.bLength = sizeof(HID_DESCRIPTOR);
    ctx->Hid.bDescriptorType = 0x21;
    ctx->Hid.bcdHID = 0x0111;
    ctx->Hid.bNumDescriptors = 1;
    ctx->Hid.DescriptorList[0].bReportType = 0x22;
    ctx->Hid.DescriptorList[0].wReportLength = (USHORT)ctx->DescriptorLength;
    WDF_IO_QUEUE_CONFIG_INIT(&queue, WdfIoQueueDispatchManual);
    queue.PowerManaged = WdfFalse;
    queue.EvtIoCanceledOnQueue = ScxCanceled;
    status = WdfIoQueueCreate(device, &queue, WDF_NO_OBJECT_ATTRIBUTES, &ctx->Reads);
    if (!NT_SUCCESS(status)) return status;
    WDF_TIMER_CONFIG_INIT(&timer, ScxWatchdog);
    timer.AutomaticSerialization = TRUE;
    WDF_OBJECT_ATTRIBUTES_INIT(&attributes);
    attributes.ParentObject = device;
    attributes.ExecutionLevel = WdfExecutionLevelPassive;
    status = WdfTimerCreate(&timer, &attributes, &ctx->Watchdog);
    if (!NT_SUCCESS(status)) return status;
    WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(&queue, WdfIoQueueDispatchSequential);
    queue.PowerManaged = WdfFalse;
    queue.EvtIoDeviceControl = ScxIoControl;
    status = WdfIoQueueCreate(device, &queue, WDF_NO_OBJECT_ATTRIBUTES, WDF_NO_HANDLE);
    if (!NT_SUCCESS(status)) return status;
    ctx->Online = TRUE;
    memcpy(ctx->Reports[0], ctx->Neutral, ctx->ReportLength);
    ctx->ReportSizes[0] = ctx->ReportLength;
    ctx->Count = 1;
    return STATUS_SUCCESS;
}

NTSTATUS DriverEntry(PDRIVER_OBJECT driverObject, PUNICODE_STRING registryPath)
{
    WDF_DRIVER_CONFIG config;
    WDF_DRIVER_CONFIG_INIT(&config, ScxDeviceAdd);
    return WdfDriverCreate(driverObject, registryPath, WDF_NO_OBJECT_ATTRIBUTES,
                           &config, WDF_NO_HANDLE);
}
