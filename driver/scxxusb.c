/* Original SCX UMDF2 XUSB-compatible function-device binding.
 * Private Windows protocol compatibility is experimental until live tested.
 * Copyright (c) 2026 SCX. See LICENSE.
 */
#define WIN32_NO_STATUS
#include <windows.h>
#undef WIN32_NO_STATUS
#pragma warning(push)
#pragma warning(disable: 4324 5104 6553)
#include <wdf.h>
#pragma warning(pop)
#include <stdint.h>
#include <string.h>
#include "scx_vpad.h"

#define SCX_X_SUBMIT 0x0022a000u
#define SCX_X_INFO 0x00226004u
#define SCX_X_WAIT_FEEDBACK 0x00226008u
#define SCX_X_GET_FEEDBACK 0x0022600cu
#define XUSB_SET_STATE 0x8000a010u

static const GUID XusbInterface = {0xec87f1e3,0xc13b,0x4100,{0xb5,0xf7,0x8b,0x84,0xd5,0x42,0x60,0xcb}};
static const GUID ScxControlInterface = {0x582a87fd,0xa2dd,0x43f1,{0x92,0x24,0xd7,0xc0,0xdf,0xdc,0xf4,0x5d}};
static const DEVPROPKEY ScxPlayerKey = {{0x19f81049,0x3679,0x40f2,{0x96,0x89,0x2a,0x2f,0x61,0xf8,0xbd,0x29}},4};

typedef struct _SCX_XUSB {
    uint8_t Input[64];
    uint8_t Feedback[32];
    uint32_t Packet;
    uint32_t LastSequence;
    uint8_t PlayerIndex;
    uint8_t Led;
    BOOLEAN HasSequence;
    BOOLEAN Online;
    BOOLEAN WatchdogArmed;
    ULONGLONG LastSubmitMs;
    WDFTIMER Watchdog;
    WDFQUEUE FeedbackWaiters;
} SCX_XUSB;
typedef struct _SCX_FILE { BOOLEAN IsControl; } SCX_FILE;
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(SCX_XUSB, ScxXContext)
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(SCX_FILE, ScxFileContext)

EVT_WDF_DEVICE_FILE_CREATE ScxXFileCreate;
EVT_WDF_IO_QUEUE_IO_DEVICE_CONTROL ScxXControl;
EVT_WDF_IO_QUEUE_IO_CANCELED_ON_QUEUE ScxXCancel;
EVT_WDF_TIMER ScxXWatchdog;
EVT_WDF_DEVICE_SURPRISE_REMOVAL ScxXRemoved;

static NTSTATUS ScxXCopy(WDFREQUEST request, const void *bytes, size_t length)
{
    WDFMEMORY memory;
    size_t capacity;
    NTSTATUS status;
    if (length == 0 || bytes == NULL) return STATUS_INVALID_PARAMETER;
    status = WdfRequestRetrieveOutputMemory(request, &memory);
    if (!NT_SUCCESS(status)) return status;
    (void)WdfMemoryGetBuffer(memory, &capacity);
    if (capacity < length) return STATUS_BUFFER_TOO_SMALL;
    status = WdfMemoryCopyFromBuffer(memory, 0, (PVOID)bytes, length);
    if (NT_SUCCESS(status)) WdfRequestSetInformation(request, length);
    return status;
}

static void ScxXNeutral(uint8_t input[64])
{
    memset(input, 0, 64);
    input[0] = 1; input[2] = 64; input[24] = 8;
}

void ScxXFileCreate(WDFDEVICE device, WDFREQUEST request, WDFFILEOBJECT file)
{
    PCUNICODE_STRING name = WdfFileObjectGetFileName(file);
    UNREFERENCED_PARAMETER(device);
    ScxFileContext(file)->IsControl = name != NULL && name->Length != 0 &&
        CompareStringOrdinal(name->Buffer, name->Length / sizeof(WCHAR),
                             L"\\SCXControl", -1, TRUE) == CSTR_EQUAL;
    WdfRequestComplete(request, STATUS_SUCCESS);
}

void ScxXWatchdog(WDFTIMER timer)
{
    SCX_XUSB *ctx = ScxXContext((WDFDEVICE)WdfTimerGetParentObject(timer));
    if (!ctx->Online || !ctx->WatchdogArmed) { ctx->WatchdogArmed = FALSE; return; }
    if (GetTickCount64() - ctx->LastSubmitMs >= 500ull) {
        ScxXNeutral(ctx->Input);
        ++ctx->Packet;
        ctx->WatchdogArmed = FALSE;
        return;
    }
    (void)WdfTimerStart(timer, WDF_REL_TIMEOUT_IN_MS(250));
}

static NTSTATUS ScxXSubmit(SCX_XUSB *ctx, const uint8_t *input, size_t length)
{
    uint32_t sequence;
    uint8_t before[29], after[29];
    const uint8_t neutralControls[12] = {0};
    BOOLEAN neutral;
    if (scx_vpad_validate_input(input, length) != 0) return STATUS_INVALID_PARAMETER;
    memcpy(&sequence, input + 4, sizeof(sequence));
    if (ctx->HasSequence && (int32_t)(sequence - ctx->LastSequence) <= 0)
        return STATUS_INVALID_PARAMETER;
    if (scx_vpad_xusb_state_report(ctx->Input, 64, 0, 0, before, sizeof(before)) != 0 ||
        scx_vpad_xusb_state_report(input, length, 0, 0, after, sizeof(after)) != 0)
        return STATUS_INVALID_PARAMETER;
    /* Unsupported sensors/buttons must neither wake a watchdog nor manufacture
     * an XInput packet-number change while the represented controls are idle. */
    neutral = memcmp(after + 11, neutralControls, sizeof(neutralControls)) == 0;
    if (memcmp(before, after, sizeof(before)) != 0) ++ctx->Packet;
    memcpy(ctx->Input, input, sizeof(ctx->Input));
    ctx->LastSequence = sequence;
    ctx->HasSequence = TRUE;
    ctx->LastSubmitMs = GetTickCount64();
    if (!ctx->WatchdogArmed && !neutral) {
        ctx->WatchdogArmed = TRUE;
        (void)WdfTimerStart(ctx->Watchdog, WDF_REL_TIMEOUT_IN_MS(250));
    } else if (ctx->WatchdogArmed && neutral) {
        (void)WdfTimerStop(ctx->Watchdog, FALSE);
        ctx->WatchdogArmed = FALSE;
    }
    return STATUS_SUCCESS;
}

static NTSTATUS ScxXOutput(SCX_XUSB *ctx, const uint8_t *input, size_t length)
{
    uint8_t decoded[32], next[32];
    uint32_t fields, previousFields, sequence;
    WDFREQUEST waiter;
    if (scx_vpad_xusb_decode_output(input, length, decoded, sizeof(decoded)) != 0)
        return STATUS_INVALID_PARAMETER;
    memcpy(next, ctx->Feedback, sizeof(next));
    memcpy(&fields, decoded + 8, sizeof(fields));
    memcpy(&previousFields, next + 8, sizeof(previousFields));
    previousFields |= fields;
    memcpy(next + 8, &previousFields, sizeof(previousFields));
    if (fields & 1u) memcpy(next + 12, decoded + 12, 4);
    if (fields & 4u) { next[19] = decoded[19]; ctx->Led = input[1]; }
    if (memcmp(next, ctx->Feedback, sizeof(next)) == 0) return STATUS_SUCCESS;
    memcpy(&sequence, next + 4, sizeof(sequence)); ++sequence;
    memcpy(next + 4, &sequence, sizeof(sequence));
    memcpy(ctx->Feedback, next, sizeof(next));
    while (NT_SUCCESS(WdfIoQueueRetrieveNextRequest(ctx->FeedbackWaiters, &waiter))) {
        NTSTATUS status = ScxXCopy(waiter, ctx->Feedback, sizeof(ctx->Feedback));
        WdfRequestComplete(waiter, status);
    }
    return STATUS_SUCCESS;
}

void ScxXControl(WDFQUEUE queue, WDFREQUEST request, size_t outLength,
                 size_t inLength, ULONG code)
{
    SCX_XUSB *ctx = ScxXContext(WdfIoQueueGetDevice(queue));
    WDFFILEOBJECT file = WdfRequestGetFileObject(request);
    WDFMEMORY memory;
    const uint8_t *input = NULL;
    size_t actual = 0, outputLength = 0;
    uint8_t output[36];
    NTSTATUS status = STATUS_NOT_SUPPORTED;
    UNREFERENCED_PARAMETER(outLength);
    if (!ctx->Online) { WdfRequestComplete(request, STATUS_DEVICE_NOT_READY); return; }
    if (inLength != 0) {
        status = WdfRequestRetrieveInputMemory(request, &memory);
        if (!NT_SUCCESS(status)) { WdfRequestComplete(request, status); return; }
        input = (const uint8_t *)WdfMemoryGetBuffer(memory, &actual);
        if (actual != inLength) { WdfRequestComplete(request, STATUS_INVALID_BUFFER_SIZE); return; }
    }
    if ((code & 0xffff0000u) == 0x00220000u &&
        (file == NULL || !ScxFileContext(file)->IsControl)) {
        WdfRequestComplete(request, STATUS_ACCESS_DENIED); return;
    }
    switch (code) {
    case SCX_X_SUBMIT:
        status = ScxXSubmit(ctx, input, actual); break;
    case SCX_X_INFO:
        if (actual != 0) { status = STATUS_INVALID_PARAMETER; break; }
        if (scx_vpad_get_info_for_profile(2, output, sizeof(output)) != 0) {
            status = STATUS_NOT_SUPPORTED; break;
        }
        status = ScxXCopy(request, output, 32); break;
    case SCX_X_GET_FEEDBACK:
        status = actual == 0 ? ScxXCopy(request, ctx->Feedback, 32) : STATUS_INVALID_PARAMETER;
        break;
    case SCX_X_WAIT_FEEDBACK:
        if (actual == 4) {
            uint32_t seen, current;
            ULONG waiting;
            memcpy(&seen, input, 4); memcpy(&current, ctx->Feedback + 4, 4);
            if (seen != current) { status = ScxXCopy(request, ctx->Feedback, 32); break; }
            (void)WdfIoQueueGetState(ctx->FeedbackWaiters, &waiting, NULL);
            if (waiting >= 64) { status = STATUS_DEVICE_BUSY; break; }
            status = WdfRequestForwardToIoQueue(request, ctx->FeedbackWaiters);
            if (NT_SUCCESS(status)) return;
        } else status = STATUS_INVALID_PARAMETER;
        break;
    case XUSB_SET_STATE:
        status = ScxXOutput(ctx, input, actual); break;
    default:
        if (scx_vpad_xusb_query(code, input, actual, ctx->Input, 64, ctx->Packet,
                ctx->PlayerIndex, ctx->Led, output, sizeof(output), &outputLength) == 0)
            status = ScxXCopy(request, output, outputLength);
        else status = STATUS_NOT_SUPPORTED;
        break;
    }
    WdfRequestComplete(request, status);
}

void ScxXCancel(WDFQUEUE queue, WDFREQUEST request)
{
    UNREFERENCED_PARAMETER(queue);
    WdfRequestComplete(request, STATUS_CANCELLED);
}

void ScxXRemoved(WDFDEVICE device)
{
    SCX_XUSB *ctx = ScxXContext(device);
    WdfObjectAcquireLock(device);
    ctx->Online = FALSE;
    (void)WdfTimerStop(ctx->Watchdog, FALSE);
    ctx->WatchdogArmed = FALSE;
    WdfObjectReleaseLock(device);
    WdfIoQueuePurge(ctx->FeedbackWaiters, NULL, NULL);
}

NTSTATUS ScxXboxDeviceAdd(WDFDRIVER driver, PWDFDEVICE_INIT init)
{
    WDFDEVICE device;
    SCX_XUSB *ctx;
    WDF_OBJECT_ATTRIBUTES attributes;
    WDF_FILEOBJECT_CONFIG files;
    WDF_PNPPOWER_EVENT_CALLBACKS power;
    WDF_IO_QUEUE_CONFIG queue;
    WDF_TIMER_CONFIG timer;
    WDF_DEVICE_PROPERTY_DATA property;
    DEVPROPTYPE propertyType;
    ULONG required, player = 0;
    NTSTATUS status;
    UNICODE_STRING controlReference;
    UNREFERENCED_PARAMETER(driver);
    RtlInitUnicodeString(&controlReference, L"SCXControl");
    WDF_FILEOBJECT_CONFIG_INIT(&files, ScxXFileCreate, WDF_NO_EVENT_CALLBACK, WDF_NO_EVENT_CALLBACK);
    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, SCX_FILE);
    WdfDeviceInitSetFileObjectConfig(init, &files, &attributes);
    WDF_PNPPOWER_EVENT_CALLBACKS_INIT(&power);
    power.EvtDeviceSurpriseRemoval = ScxXRemoved;
    WdfDeviceInitSetPnpPowerEventCallbacks(init, &power);
    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, SCX_XUSB);
    attributes.SynchronizationScope = WdfSynchronizationScopeDevice;
    attributes.ExecutionLevel = WdfExecutionLevelPassive;
    status = WdfDeviceCreate(&init, &attributes, &device);
    if (!NT_SUCCESS(status)) return status;
    ctx = ScxXContext(device);
    ScxXNeutral(ctx->Input);
    ctx->Feedback[0] = 1; ctx->Feedback[2] = 32; ctx->Feedback[24] = 2;
    WDF_DEVICE_PROPERTY_DATA_INIT(&property, &ScxPlayerKey);
    status = WdfDeviceQueryPropertyEx(device, &property, sizeof(player), &player, &required, &propertyType);
    if (NT_SUCCESS(status) && (propertyType != DEVPROP_TYPE_UINT32 || required != sizeof(player) || player > 3))
        return STATUS_INVALID_PARAMETER;
    if (!NT_SUCCESS(status) && status != STATUS_NOT_FOUND && status != STATUS_OBJECT_NAME_NOT_FOUND &&
        status != STATUS_OBJECT_PATH_NOT_FOUND) return status;
    ctx->PlayerIndex = (uint8_t)player;
    ctx->Led = (uint8_t)(6u + player);
    WDF_IO_QUEUE_CONFIG_INIT(&queue, WdfIoQueueDispatchManual);
    queue.PowerManaged = WdfFalse;
    queue.EvtIoCanceledOnQueue = ScxXCancel;
    status = WdfIoQueueCreate(device, &queue, WDF_NO_OBJECT_ATTRIBUTES, &ctx->FeedbackWaiters);
    if (!NT_SUCCESS(status)) return status;
    WDF_TIMER_CONFIG_INIT(&timer, ScxXWatchdog);
    timer.AutomaticSerialization = TRUE;
    WDF_OBJECT_ATTRIBUTES_INIT(&attributes);
    attributes.ParentObject = device;
    attributes.ExecutionLevel = WdfExecutionLevelPassive;
    status = WdfTimerCreate(&timer, &attributes, &ctx->Watchdog);
    if (!NT_SUCCESS(status)) return status;
    WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(&queue, WdfIoQueueDispatchSequential);
    queue.PowerManaged = WdfFalse;
    queue.EvtIoDeviceControl = ScxXControl;
    status = WdfIoQueueCreate(device, &queue, WDF_NO_OBJECT_ATTRIBUTES, WDF_NO_HANDLE);
    if (!NT_SUCCESS(status)) return status;
    status = WdfDeviceCreateDeviceInterface(device, &ScxControlInterface, &controlReference);
    if (!NT_SUCCESS(status)) return status;
    status = WdfDeviceCreateDeviceInterface(device, &XusbInterface, NULL);
    if (!NT_SUCCESS(status)) return status;
    ctx->Online = TRUE;
    return STATUS_SUCCESS;
}
