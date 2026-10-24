/* Copyright (c) 2026 SCX. See LICENSE at the repository root.
 * Original UMDF2 binding. Windows owns PnP, HID collection routing and request
 * cancellation; Rust owns the byte protocol and the HID descriptor/encoder.
 */
#include "scx_device.h"
#include "scx_channel.h"
#include "scx_status.h"
#include "scx_trace.h"
#define SCX_STALE_MS 500ull

/* Generic classes share the fixed Rust descriptor's private HID transport.
 * Sony retains its separate authenticated channel and never exposes it. */
static BOOLEAN ScxGenericHidProfile(uint32_t profile)
{
    return profile == SCX_PROFILE_GENERIC_HID || profile == SCX_PROFILE_WHEEL ||
           profile == SCX_PROFILE_FLIGHT_STICK || profile == SCX_PROFILE_ARCADE_STICK;
}

static const DEVPROPKEY ScxProfileKey = {
    {0x19f81049,0x3679,0x40f2,{0x96,0x89,0x2a,0x2f,0x61,0xf8,0xbd,0x29}}, 2
};
static const DEVPROPKEY ScxIdentityKey = {
    {0x19f81049,0x3679,0x40f2,{0x96,0x89,0x2a,0x2f,0x61,0xf8,0xbd,0x29}}, 3
};
const DEVPROPKEY ScxBootstrapKey = {
    {0x19f81049,0x3679,0x40f2,{0x96,0x89,0x2a,0x2f,0x61,0xf8,0xbd,0x29}}, 6
};

DRIVER_INITIALIZE DriverEntry;
EVT_WDF_DRIVER_DEVICE_ADD ScxDeviceAdd;
EVT_WDF_IO_QUEUE_IO_DEVICE_CONTROL ScxIoControl;
EVT_WDF_IO_QUEUE_IO_CANCELED_ON_QUEUE ScxCanceled;
EVT_WDF_TIMER ScxWatchdog;
EVT_WDF_DEVICE_SURPRISE_REMOVAL ScxRemoved;
EVT_WDF_OBJECT_CONTEXT_DESTROY ScxDestroy;
EVT_WDF_OBJECT_CONTEXT_CLEANUP ScxCleanup;
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

static NTSTATUS ScxAuthorizeProducer(SCX_DEVICE *ctx, const uint8_t *capability)
{
    volatile uint8_t difference = 0;
    unsigned index;
    /* Volatile accumulation prevents an early-exit comparison; all 32 bytes
     * are visited before any sequence/report/heartbeat state can be changed. */
    for (index = 0; index < sizeof(ctx->Capability); ++index)
        difference |= capability[index] ^ ctx->Capability[index];
    return ctx->CapabilityLoaded && difference == 0 ? STATUS_SUCCESS : STATUS_ACCESS_DENIED;
}

void ScxDestroy(WDFOBJECT object)
{
    SCX_DEVICE *ctx = ScxContext((WDFDEVICE)object);
    SecureZeroMemory(ctx->Capability, sizeof(ctx->Capability));
    ctx->CapabilityLoaded = FALSE;
}
NTSTATUS ScxReadCreationProperty(WDFDEVICE device, const DEVPROPKEY *key,
                                        void *value, ULONG size, DEVPROPTYPE expected)
{
    WDF_DEVICE_PROPERTY_DATA property;
    DEVPROPTYPE actual;
    ULONG required;
    NTSTATUS status;
    WDF_DEVICE_PROPERTY_DATA_INIT(&property, key);
    status = WdfDeviceQueryPropertyEx(device, &property, size, value, &required, &actual);
    /* Optional profile/identity defaults are allowed for generic HID. The
     * separately verified producer bootstrap/capability is always mandatory. */
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
    const uint8_t *report;
    while ((report = scx_vpad_hid_front(&ctx->Input)) != NULL &&
           NT_SUCCESS(WdfIoQueueRetrieveNextRequest(ctx->Reads, &read))) {
        NTSTATUS status = ScxCopy(read, report, ctx->ReportLength);
        /* A malformed reader must not consume a queued button transition. */
        if (NT_SUCCESS(status)) scx_vpad_hid_consume(&ctx->Input);
        else ScxTraceRequest(ctx, read, IOCTL_HID_READ_REPORT, 0, ctx->ReportLength, status);
        WdfRequestComplete(read, status);
    }
}

static void ScxNeutralize(SCX_DEVICE *ctx)
{
    scx_vpad_hid_neutralize(&ctx->Input);
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

NTSTATUS ScxAcceptPacket(SCX_DEVICE *ctx, const uint8_t *bytes)
{
    uint8_t neutral;
    NTSTATUS status;
    if (!ctx->Online) return STATUS_DEVICE_NOT_READY;
    if (bytes[0] != SCX_VPAD_SUBMIT_FEATURE_ID) return STATUS_INVALID_PARAMETER;
    status = ScxAuthorizeProducer(ctx, bytes + SCX_VPAD_SUBMIT_CAPABILITY_OFFSET);
    if (!NT_SUCCESS(status)) return status;
    ScxDrain(ctx);
    status = ScxStatus(scx_vpad_hid_submit(&ctx->Input,
        bytes + SCX_VPAD_SUBMIT_INPUT_OFFSET, SCX_VPAD_INPUT_PACKET_SIZE, &neutral));
    if (!NT_SUCCESS(status)) return status;
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
    return STATUS_SUCCESS;
}

static NTSTATUS ScxSubmit(SCX_DEVICE *ctx, WDFREQUEST request, size_t reportId)
{
    WDFMEMORY memory;
    size_t length;
    const uint8_t *bytes;
    NTSTATUS status;
    /* Sony game-facing HID never accepts SCX transport packets. */
    if (!ScxGenericHidProfile(ctx->Profile) || reportId != SCX_VPAD_SUBMIT_FEATURE_ID) return STATUS_ACCESS_DENIED;
    status = WdfRequestRetrieveInputMemory(request, &memory);
    if (!NT_SUCCESS(status)) return status;
    bytes = (const uint8_t *)WdfMemoryGetBuffer(memory, &length);
    if (length != SCX_VPAD_HID_FEATURE_REPORT_SIZE) return STATUS_INVALID_BUFFER_SIZE;
    status = ScxAcceptPacket(ctx, bytes);
    if (NT_SUCCESS(status)) WdfRequestSetInformation(request, length);
    return status;
}

static NTSTATUS ScxSetFeature(SCX_DEVICE *ctx, WDFREQUEST request, size_t reportId)
{
    WDFMEMORY memory;
    const uint8_t *bytes;
    size_t length;
    uint32_t result;
    NTSTATUS status;
    if (ScxGenericHidProfile(ctx->Profile)) return ScxSubmit(ctx, request, reportId);
    if (!ctx->Online) return STATUS_DEVICE_NOT_READY;
    status = WdfRequestRetrieveInputMemory(request, &memory);
    if (!NT_SUCCESS(status)) return status;
    bytes = (const uint8_t *)WdfMemoryGetBuffer(memory, &length);
    if (length == 0 || reportId > 255 || bytes[0] != (uint8_t)reportId)
        return STATUS_INVALID_PARAMETER;
    result = scx_vpad_validate_configuration_feature(ctx->Profile, bytes[0], length);
    if (result != SCX_VPAD_OK) return ScxStatus(result);
    /* Native Sony consumers configure the radio during initialization. There
     * is no radio in this virtual USB endpoint. Acknowledge only these known,
     * descriptor-sized commands; never dispatch them to the physical source,
     * change input or grant access to the authenticated producer channel. */
    WdfRequestSetInformation(request, length);
    return STATUS_SUCCESS;
}

static NTSTATUS ScxReportId(WDFREQUEST request, uint8_t *id)
{
    WDFMEMORY memory;
    size_t length;
    const uint8_t *bytes;
    NTSTATUS status = WdfRequestRetrieveInputMemory(request, &memory);
    if (!NT_SUCCESS(status)) return status;
    bytes = (const uint8_t *)WdfMemoryGetBuffer(memory, &length);
    if (length < 1) return STATUS_INVALID_BUFFER_SIZE;
    *id = bytes[0];
    return STATUS_SUCCESS;
}

static NTSTATUS ScxGetInputReport(SCX_DEVICE *ctx, WDFREQUEST request)
{
    uint8_t id;
    NTSTATUS status = ScxReportId(request, &id);
    if (!NT_SUCCESS(status)) return status;
    if (!ctx->Online) return STATUS_DEVICE_NOT_READY;
    if (id != SCX_VPAD_GAMEPAD_REPORT_ID) return STATUS_NOT_SUPPORTED;
    return ScxCopy(request, scx_vpad_hid_current(&ctx->Input), ctx->ReportLength);
}

static NTSTATUS ScxGetFeature(SCX_DEVICE *ctx, WDFREQUEST request)
{
    uint8_t id, feature[SCX_VPAD_MAX_REPORT_SIZE];
    size_t length;
    NTSTATUS status = ScxReportId(request, &id);
    if (!NT_SUCCESS(status)) return status;
    /* Keep SCX's transport/diagnostic reports off the Sony game-facing HID.
     * Feedback is exclusively delivered through the owned sideband channel. */
    if (!ScxGenericHidProfile(ctx->Profile) && id >= 0x78 && id <= 0x7f)
        return STATUS_NOT_SUPPORTED;
    if (ScxGenericHidProfile(ctx->Profile) && id == SCX_VPAD_INFO_FEATURE_ID) {
        uint8_t info[1 + SCX_VPAD_INFO_PACKET_SIZE] = {SCX_VPAD_INFO_FEATURE_ID};
        status = ScxStatus(scx_vpad_get_info_for_profile(ctx->Profile, info + 1, SCX_VPAD_INFO_PACKET_SIZE));
        return NT_SUCCESS(status) ? ScxCopy(request, info, sizeof(info)) : status;
    }
    status = ScxStatus(scx_vpad_feature_report_for_profile(ctx->Profile, id,
        ctx->InstanceIdentity, feature, sizeof(feature), &length));
    return NT_SUCCESS(status) ? ScxCopy(request, feature, length) : status;
}

static NTSTATUS ScxOutput(SCX_DEVICE *ctx, WDFREQUEST request, size_t reportId)
{
    WDFMEMORY memory;
    size_t length;
    const uint8_t *bytes;
    uint8_t decoded[SCX_VPAD_FEEDBACK_PACKET_SIZE], changed;
    NTSTATUS status;
    if (!ctx->Online) return STATUS_DEVICE_NOT_READY;
    status = WdfRequestRetrieveInputMemory(request, &memory);
    if (!NT_SUCCESS(status)) return status;
    bytes = (const uint8_t *)WdfMemoryGetBuffer(memory, &length);
    if (length == 0 || reportId > UINT8_MAX || bytes[0] != (uint8_t)reportId)
        return STATUS_INVALID_PARAMETER;
    status = ScxStatus(scx_vpad_parse_output_for_profile(ctx->Profile, bytes, length, decoded, sizeof(decoded)));
    if (!NT_SUCCESS(status)) return status;
    status = ScxStatus(scx_vpad_feedback_merge(ctx->Feedback, sizeof(ctx->Feedback), decoded, sizeof(decoded), &changed));
    if (!NT_SUCCESS(status)) return status;
    if (changed) ScxChannelPublish(ctx);
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
    WCHAR serial[21] = L"SCX-";
    unsigned index;
    NTSTATUS status = WdfRequestRetrieveInputMemory(request, &memory);
    if (!NT_SUCCESS(status)) return status;
    selectorBuffer = WdfMemoryGetBuffer(memory, &length);
    if (length < sizeof(ULONG)) return STATUS_INVALID_BUFFER_SIZE;
    memcpy(&selector, selectorBuffer, sizeof(selector));
    switch (selector & 0xffffu) {
    case HID_STRING_ID_IMANUFACTURER: text = L"SCX"; break;
    case HID_STRING_ID_IPRODUCT:
        /* Public compatibility names; PnP/service/provider remain branded SCX.
         * Games can identify Sony controllers by the HID product string. */
        switch (ctx->Profile) {
        case SCX_PROFILE_DUALSHOCK4: text = L"Wireless Controller"; break;
        case SCX_PROFILE_DUALSENSE: text = L"DualSense Wireless Controller"; break;
        case SCX_PROFILE_WHEEL: text = L"SCX Virtual Racing Wheel"; break;
        case SCX_PROFILE_FLIGHT_STICK: text = L"SCX Virtual Flight Stick"; break;
        case SCX_PROFILE_ARCADE_STICK: text = L"SCX Virtual Arcade Stick"; break;
        default: text = L"SCX Virtual Gamepad"; break;
        }
        break;
    case HID_STRING_ID_ISERIALNUMBER:
        /* HID consumers do not necessarily inspect our PnP parent identity.
         * Supply the same public, stable, per-device identity as an HID serial,
         * never the separate input authorization capability. */
        for (index = 0; index < 16; ++index)
            serial[4 + index] = L"0123456789ABCDEF"[(ctx->InstanceIdentity >> ((15 - index) * 4)) & 15];
        text = serial;
        break;
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
    /* UMDF HID SET_FEATURE/SET_OUTPUT_REPORT pass the report ID through
     * OutputBufferLength (not a buffer size). See IOCTL_UMDF_HID_SET_FEATURE. */
    const size_t reportId = outLength;
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
            WDFMEMORY output;
            size_t capacity;
            ULONG waiting;
            /* Reject undersized reads before they can occupy bounded wait
             * slots while the controller is idle. No transition is consumed. */
            status = WdfRequestRetrieveOutputMemory(request, &output);
            if (!NT_SUCCESS(status)) break;
            (void)WdfMemoryGetBuffer(output, &capacity);
            if (capacity < ctx->ReportLength) { status = STATUS_BUFFER_TOO_SMALL; break; }
            (void)WdfIoQueueGetState(ctx->Reads, &waiting, NULL);
            if (waiting >= SCX_VPAD_HID_QUEUE_CAPACITY) { status = STATUS_DEVICE_BUSY; break; }
        }
        status = WdfRequestForwardToIoQueue(request, ctx->Reads);
        if (NT_SUCCESS(status)) { ScxDrain(ctx); return; }
        break;
    case IOCTL_UMDF_HID_SET_FEATURE:
        status = ScxSetFeature(ctx, request, reportId); break;
    case IOCTL_UMDF_HID_GET_FEATURE:
        status = ScxGetFeature(ctx, request);
        break;
    case IOCTL_UMDF_HID_GET_INPUT_REPORT:
        status = ScxGetInputReport(ctx, request); break;
    case IOCTL_HID_WRITE_REPORT:
    case IOCTL_UMDF_HID_SET_OUTPUT_REPORT:
        status = ScxOutput(ctx, request, reportId); break;
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
    ScxTraceRequest(ctx, request, code, inLength, outLength, status);
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
    scx_vpad_hid_clear(&ctx->Input);
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
    uint32_t creationProfile = SCX_PROFILE_GENERIC_HID;
    NTSTATUS status;
    UNREFERENCED_PARAMETER(driver);
    WDF_DEVICE_PROPERTY_DATA_INIT(&creationProperty, &ScxProfileKey);
    status = WdfFdoInitQueryPropertyEx(init, &creationProperty, sizeof(creationProfile),
                                      &creationProfile, &creationRequired, &creationType);
    if (NT_SUCCESS(status)) {
        if (creationRequired != sizeof(creationProfile) || creationType != DEVPROP_TYPE_UINT32)
            return STATUS_INVALID_PARAMETER;
        if (creationProfile == SCX_PROFILE_XBOX360) return ScxXboxDeviceAdd(driver, init);
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
    attributes.EvtCleanupCallback = ScxCleanup;
    status = WdfDeviceCreate(&init, &attributes, &device);
    if (!NT_SUCCESS(status)) return status;
    ctx = ScxContext(device);
    status = ScxReadProducerCapability(device, ctx);
    if (!NT_SUCCESS(status)) return status;
    ctx->Profile = SCX_PROFILE_GENERIC_HID;
    status = ScxReadCreationProperty(device, &ScxProfileKey, &ctx->Profile,
                                    sizeof(ctx->Profile), DEVPROP_TYPE_UINT32);
    if (!NT_SUCCESS(status)) return status;
    ctx->ReportLength = scx_vpad_profile_report_size(ctx->Profile);
    if (ctx->ReportLength == 0 || ctx->ReportLength > SCX_VPAD_MAX_REPORT_SIZE)
        return STATUS_NOT_SUPPORTED;
    status = ScxReadCreationProperty(device, &ScxIdentityKey, &ctx->InstanceIdentity,
                                    sizeof(ctx->InstanceIdentity), DEVPROP_TYPE_UINT64);
    if (!NT_SUCCESS(status)) return status;
    if (ctx->Profile != SCX_PROFILE_GENERIC_HID && ctx->InstanceIdentity == 0)
        return STATUS_INVALID_PARAMETER;
    if (scx_vpad_get_hid_attributes(ctx->Profile, &ctx->VendorId, &ctx->ProductId,
                                     &ctx->ProductVersion) != 0)
        return STATUS_NOT_SUPPORTED;
    status = ScxStatus(scx_vpad_feedback_init(ctx->Profile, ctx->Feedback, sizeof(ctx->Feedback)));
    if (!NT_SUCCESS(status)) return status;
    ctx->Descriptor = scx_vpad_report_descriptor_for_profile(ctx->Profile, &ctx->DescriptorLength);
    if (ctx->Descriptor == NULL || ctx->DescriptorLength == 0 || ctx->DescriptorLength > 65535u)
        return STATUS_INVALID_PARAMETER;
    status = ScxStatus(scx_vpad_hid_init(&ctx->Input, sizeof(ctx->Input), ctx->Profile));
    if (!NT_SUCCESS(status)) return status;
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
    if (!ScxGenericHidProfile(ctx->Profile)) return ScxChannelStart(device, ctx);
    return STATUS_SUCCESS;
}

NTSTATUS DriverEntry(PDRIVER_OBJECT driverObject, PUNICODE_STRING registryPath)
{
    WDF_DRIVER_CONFIG config;
    NTSTATUS status;
    /* ETW registration is optional; diagnostics must never prevent operation. */
    ScxTraceInitialize();
    WDF_DRIVER_CONFIG_INIT(&config, ScxDeviceAdd);
    config.EvtDriverUnload = ScxTraceUnload;
    status = WdfDriverCreate(driverObject, registryPath, WDF_NO_OBJECT_ATTRIBUTES,
                           &config, WDF_NO_HANDLE);
    if (!NT_SUCCESS(status)) ScxTraceShutdown();
    return status;
}
