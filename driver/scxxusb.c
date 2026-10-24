/* Original SCX UMDF2 XUSB-compatible function-device binding.
 * Private Windows protocol compatibility is experimental until live tested.
 * Copyright (c) 2026 SCX. See LICENSE.
 */
#include "scx_windows.h"
#include "scx_status.h"

#define SCX_X_SUBMIT 0x0022a000u
#define SCX_X_INFO 0x00226004u
#define SCX_X_WAIT_FEEDBACK 0x00226008u
#define SCX_X_GET_FEEDBACK 0x0022600cu
#define SCX_X_DIAGNOSTICS 0x00226010u
#define XUSB_SET_STATE 0x8000a010u
#define XUSB_WAIT_INPUT 0x8000e3acu
#define XUSB_POWER_INFO 0x80006380u

static const GUID XusbInterface = {0xec87f1e3,0xc13b,0x4100,{0xb5,0xf7,0x8b,0x84,0xd5,0x42,0x60,0xcb}};
static const GUID ScxControlInterface = {0x582a87fd,0xa2dd,0x43f1,{0x92,0x24,0xd7,0xc0,0xdf,0xdc,0xf4,0x5d}};
static const DEVPROPKEY ScxPlayerKey = {{0x19f81049,0x3679,0x40f2,{0x96,0x89,0x2a,0x2f,0x61,0xf8,0xbd,0x29}},4};

typedef struct _SCX_XUSB {
    uint8_t Input[64];
    /* Last accepted game-facing controls; no timestamp/sensor/sequence noise.
     * Kept under the same device lock as Input, including watchdog releases. */
    uint8_t Controls[SCX_VPAD_XUSB_CONTROLS_SIZE];
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
    WDFQUEUE InputWaiters;
    WDFWORKITEM InitialInput;
} SCX_XUSB;
typedef struct _SCX_FILE {
    BOOLEAN IsControl;
    BOOLEAN HasSnapshot;
    uint8_t RequestMode;
    uint32_t InputPacket;
} SCX_FILE;
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(SCX_XUSB, ScxXContext)
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(SCX_FILE, ScxFileContext)

EVT_WDF_DEVICE_FILE_CREATE ScxXFileCreate;
EVT_WDF_IO_QUEUE_IO_DEVICE_CONTROL ScxXControl;
EVT_WDF_IO_QUEUE_IO_CANCELED_ON_QUEUE ScxXCancel;
EVT_WDF_TIMER ScxXWatchdog;
EVT_WDF_DEVICE_SURPRISE_REMOVAL ScxXRemoved;
EVT_WDF_WORKITEM ScxXInitialInput;

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

/* Device serialization covers queue callbacks, submissions, the watchdog and
 * this work item. Each reader receives an initial snapshot and subsequent
 * control changes, without a polling timer or an idle completion loop. */
static void ScxXDeliverInput(SCX_XUSB *ctx)
{
    ULONG remaining = 0;
    WDFREQUEST request;
    uint8_t report[SCX_VPAD_XUSB_STATE_REPORT_SIZE];
    if (!ctx->Online) return;
    if (scx_vpad_xusb_state_report(ctx->Input, 64, ctx->Packet, 1,
                                  report, sizeof(report)) != 0) return;
    (void)WdfIoQueueGetState(ctx->InputWaiters, &remaining, NULL);
    while (remaining-- != 0 && NT_SUCCESS(WdfIoQueueRetrieveNextRequest(ctx->InputWaiters, &request))) {
        SCX_FILE *reader = ScxFileContext(WdfRequestGetFileObject(request));
        NTSTATUS status;
        if (reader->HasSnapshot && reader->InputPacket == ctx->Packet) {
            status = WdfRequestForwardToIoQueue(request, ctx->InputWaiters);
            if (!NT_SUCCESS(status)) WdfRequestComplete(request, status);
            continue;
        }
        status = ScxXCopy(request, report, sizeof(report));
        if (NT_SUCCESS(status)) {
            reader->InputPacket = ctx->Packet;
            reader->HasSnapshot = TRUE;
        }
        WdfRequestComplete(request, status);
    }
}

void ScxXInitialInput(WDFWORKITEM work)
{
    ScxXDeliverInput(ScxXContext((WDFDEVICE)WdfWorkItemGetParentObject(work)));
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
        memset(ctx->Controls, 0, sizeof(ctx->Controls));
        ++ctx->Packet;
        ctx->WatchdogArmed = FALSE;
        ScxXDeliverInput(ctx);
        return;
    }
    (void)WdfTimerStart(timer, WDF_REL_TIMEOUT_IN_MS(250));
}

static NTSTATUS ScxXSubmit(SCX_XUSB *ctx, const uint8_t *input, size_t length)
{
    uint32_t sequence;
    uint8_t after[SCX_VPAD_XUSB_STATE_REPORT_SIZE];
    const uint8_t neutralControls[SCX_VPAD_XUSB_CONTROLS_SIZE] = {0};
    BOOLEAN neutral, changed;
    /* This encoder fully validates Input64 before reading its sequence or
     * changing state. Do not decode the new packet twice or re-encode the
     * previous packet merely to detect changed controls. */
    if (scx_vpad_xusb_state_report(input, length, 0, 0, after, sizeof(after)) != 0)
        return STATUS_INVALID_PARAMETER;
    memcpy(&sequence, input + SCX_VPAD_INPUT_SEQUENCE_OFFSET, sizeof(sequence));
    if (ctx->HasSequence && !scx_vpad_sequence_after(sequence, ctx->LastSequence))
        return STATUS_INVALID_PARAMETER;
    /* Unsupported sensors/buttons must neither wake a watchdog nor manufacture
     * an XInput packet-number change while the represented controls are idle. */
    neutral = memcmp(after + SCX_VPAD_XUSB_CONTROLS_OFFSET, neutralControls, sizeof(neutralControls)) == 0;
    changed = memcmp(ctx->Controls, after + SCX_VPAD_XUSB_CONTROLS_OFFSET, sizeof(ctx->Controls)) != 0;
    if (changed) ++ctx->Packet;
    memcpy(ctx->Input, input, sizeof(ctx->Input));
    memcpy(ctx->Controls, after + SCX_VPAD_XUSB_CONTROLS_OFFSET, sizeof(ctx->Controls));
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
    if (changed) ScxXDeliverInput(ctx);
    return STATUS_SUCCESS;
}

static NTSTATUS ScxXOutput(SCX_XUSB *ctx, const uint8_t *input, size_t length)
{
    uint8_t decoded[SCX_VPAD_FEEDBACK_PACKET_SIZE], changed;
    uint32_t fields;
    WDFREQUEST waiter;
    NTSTATUS status = ScxStatus(scx_vpad_xusb_decode_output(input, length, decoded, sizeof(decoded)));
    if (!NT_SUCCESS(status)) return status;
    status = ScxStatus(scx_vpad_feedback_merge(ctx->Feedback, sizeof(ctx->Feedback), decoded, sizeof(decoded), &changed));
    if (!NT_SUCCESS(status)) return status;
    memcpy(&fields, decoded + SCX_VPAD_FEEDBACK_FIELDS_OFFSET, sizeof(fields));
    if (fields & SCX_VPAD_FEEDBACK_PLAYER_VALID) ctx->Led = input[1];
    if (!changed) return STATUS_SUCCESS;
    while (NT_SUCCESS(WdfIoQueueRetrieveNextRequest(ctx->FeedbackWaiters, &waiter))) {
        status = ScxXCopy(waiter, ctx->Feedback, sizeof(ctx->Feedback));
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
    uint8_t output[64];
    NTSTATUS status = STATUS_NOT_SUPPORTED;
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
    case XUSB_POWER_INFO:
        /* Observed Windows discovery request has no input and a four-byte
         * output capacity. This software device has no physical power data. */
        status = actual == 0 ? STATUS_SUCCESS : STATUS_INVALID_PARAMETER;
        break;
    case SCX_X_DIAGNOSTICS:
        /* Legacy bring-up counters are no longer collected in production. */
        status = STATUS_NOT_SUPPORTED;
        break;
    case XUSB_WAIT_INPUT:
        /* WGI uses {version LE16, index, flags}; observed flags 2 on startup
         * and 3 after the first input-resumed completion on Windows 26200.
         * Also retain the empty and version/index diagnostic request forms.
         * No producer data is accepted through this public interface. */
        if (file == NULL || outLength < SCX_VPAD_XUSB_STATE_REPORT_SIZE) { status = STATUS_INVALID_BUFFER_SIZE; break; }
        if (actual != 0 && ((actual != 3 && actual != 4) || input[0] < 1 || input[0] > 4 || input[1] != 1 || input[2] != 0 || (actual == 4 && (input[3] & ~3u) != 0))) {
            status = STATUS_INVALID_PARAMETER; break;
        }
        {
            ULONG waiting;
            SCX_FILE *reader = ScxFileContext(file);
            uint8_t mode = actual == 4 ? input[3] : 0;
            /* WGI changes from discovery mode 2 to active sink mode 3.
             * Replay one snapshot for that transition even when controls are
             * neutral, so the newly registered sink receives OnInputResumed.
             * Repeated requests in the same mode still sleep until a change. */
            if (reader->RequestMode != mode) {
                reader->RequestMode = mode;
                reader->HasSnapshot = FALSE;
            }
            (void)WdfIoQueueGetState(ctx->InputWaiters, &waiting, NULL);
            if (waiting >= 64) { status = STATUS_DEVICE_BUSY; break; }
            status = WdfRequestForwardToIoQueue(request, ctx->InputWaiters);
            if (NT_SUCCESS(status)) {
                /* Defer even the first neutral report: WGI starts an async
                 * input pump and must receive an input-resumed completion. */
                if (!ScxFileContext(file)->HasSnapshot || ScxFileContext(file)->InputPacket != ctx->Packet)
                    WdfWorkItemEnqueue(ctx->InitialInput);
                return;
            }
        }
        break;
    case SCX_X_SUBMIT:
        status = ScxXSubmit(ctx, input, actual); break;
    case SCX_X_INFO:
        if (actual != 0) { status = STATUS_INVALID_PARAMETER; break; }
        if (scx_vpad_get_info_for_profile(SCX_PROFILE_XBOX360, output, sizeof(output)) != 0) {
            status = STATUS_NOT_SUPPORTED; break;
        }
        status = ScxXCopy(request, output, 32); break;
    case SCX_X_GET_FEEDBACK:
        status = actual == 0 ? ScxXCopy(request, ctx->Feedback, 32) : STATUS_INVALID_PARAMETER;
        break;
    case SCX_X_WAIT_FEEDBACK:
        /* Invalid buffers must not occupy one of the bounded wait slots until
         * a game eventually sends rumble. Valid pending requests are unchanged. */
        if (outLength < sizeof(ctx->Feedback)) { status = STATUS_BUFFER_TOO_SMALL; break; }
        if (actual == 4) {
            uint32_t seen, current;
            ULONG waiting;
            memcpy(&seen, input, 4); memcpy(&current, ctx->Feedback + SCX_VPAD_FEEDBACK_SEQUENCE_OFFSET, 4);
            if (seen != current) { status = ScxXCopy(request, ctx->Feedback, 32); break; }
            (void)WdfIoQueueGetState(ctx->FeedbackWaiters, &waiting, NULL);
            if (waiting >= 64) { status = STATUS_DEVICE_BUSY; break; }
            status = WdfRequestForwardToIoQueue(request, ctx->FeedbackWaiters);
            if (NT_SUCCESS(status)) return;
        } else status = STATUS_INVALID_PARAMETER;
        break;
    case XUSB_SET_STATE:
        status = ScxXOutput(ctx, input, actual);
        break;
    default:
        status = ScxStatus(scx_vpad_xusb_query(code, input, actual, ctx->Input, sizeof(ctx->Input), ctx->Packet,
            ctx->PlayerIndex, ctx->Led, output, sizeof(output), &outputLength));
        if (NT_SUCCESS(status)) status = ScxXCopy(request, output, outputLength);
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
    WdfIoQueuePurge(ctx->InputWaiters, NULL, NULL);
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
    WDF_WORKITEM_CONFIG work;
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
    memset(ctx->Controls, 0, sizeof(ctx->Controls));
    status = ScxStatus(scx_vpad_feedback_init(SCX_PROFILE_XBOX360, ctx->Feedback, sizeof(ctx->Feedback)));
    if (!NT_SUCCESS(status)) return status;
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
    status = WdfIoQueueCreate(device, &queue, WDF_NO_OBJECT_ATTRIBUTES, &ctx->InputWaiters);
    if (!NT_SUCCESS(status)) return status;
    WDF_WORKITEM_CONFIG_INIT(&work, ScxXInitialInput);
    work.AutomaticSerialization = TRUE;
    WDF_OBJECT_ATTRIBUTES_INIT(&attributes);
    attributes.ParentObject = device;
    status = WdfWorkItemCreate(&work, &attributes, &ctx->InitialInput);
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
