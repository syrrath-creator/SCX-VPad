/* Original SCX Sony sideband. The service creates all named objects with a
 * per-lease owner/UMDF/SYSTEM ACL before publishing the random bootstrap ID.
 * All offsets are fixed; no shared pointer, length, index or allocation is used.
 * Shared words are atomic even against a hostile producer. The 97-byte packet
 * is copied once and authenticated/validated before changing device state.
 */
static LONG ScxChannelRead(SCX_DEVICE *ctx, unsigned byteOffset)
{
    return InterlockedCompareExchange(ctx->ChannelWords + byteOffset / 4, 0, 0);
}
static void ScxChannelWrite(SCX_DEVICE *ctx, unsigned byteOffset, const void *data, unsigned bytes)
{
    unsigned i;
    for (i = 0; i < bytes; i += 4) {
        LONG value;
        memcpy(&value, (const uint8_t *)data + i, 4);
        InterlockedExchange(ctx->ChannelWords + (byteOffset + i) / 4, value);
    }
}
static void ScxChannelDiagnostics(SCX_DEVICE *ctx)
{
    if (!ctx->ChannelWords) return;
    ScxChannelWrite(ctx, 224, ctx->OutputDiagnostic, sizeof(ctx->OutputDiagnostic));
    ScxChannelWrite(ctx, 256, &ctx->AuthStage, 4);
    ScxChannelWrite(ctx, 260, &ctx->AuthError, 4);
    ScxChannelWrite(ctx, 264, &ctx->AuthRequestorMode, 4);
}
static void ScxChannelPublish(SCX_DEVICE *ctx)
{
    if (!ctx->ChannelWords) return;
    InterlockedIncrement(ctx->ChannelWords + 7);
    ScxChannelWrite(ctx, 192, ctx->Feedback + 1, 32);
    InterlockedIncrement(ctx->ChannelWords + 7);
    SetEvent(ctx->ChannelFeedback);
}
static DWORD ScxChannelError(NTSTATUS status)
{
    if (NT_SUCCESS(status)) return ERROR_SUCCESS;
    if (status == STATUS_ACCESS_DENIED) return ERROR_ACCESS_DENIED;
    if (status == STATUS_DEVICE_BUSY) return ERROR_BUSY;
    if (status == STATUS_DEVICE_NOT_READY) return ERROR_NOT_READY;
    return ERROR_INVALID_PARAMETER;
}
static DWORD WINAPI ScxChannelWorker(LPVOID parameter)
{
    WDFDEVICE device = (WDFDEVICE)parameter;
    SCX_DEVICE *ctx = ScxContext(device);
    HANDLE waits[2] = { ctx->ChannelStop, ctx->ChannelInput };
    LONG completed = 0;
    for (;;) {
        uint8_t packet[100];
        unsigned i;
        LONG ticket;
        NTSTATUS status;
        DWORD error;
        if (WaitForMultipleObjects(2, waits, FALSE, INFINITE) != WAIT_OBJECT_0 + 1) break;
        ticket = ScxChannelRead(ctx, 16);
        if (ticket == completed) continue;
        for (i = 0; i < sizeof(packet); i += 4) {
            LONG word = ScxChannelRead(ctx, 64 + i);
            memcpy(packet + i, &word, 4);
        }
        WdfObjectAcquireLock(device);
        status = ScxAcceptPacket(ctx, NULL, packet);
        ScxChannelDiagnostics(ctx);
        WdfObjectReleaseLock(device);
        SecureZeroMemory(packet, sizeof(packet));
        error = ScxChannelError(status);
        InterlockedExchange(ctx->ChannelWords + 6, (LONG)error);
        completed = ticket;
        InterlockedExchange(ctx->ChannelWords + 5, completed);
        SetEvent(ctx->ChannelDone);
    }
    InterlockedExchange(ctx->ChannelWords + 3, 0);
    SetEvent(ctx->ChannelDone);
    SetEvent(ctx->ChannelFeedback);
    return 0;
}
void ScxCleanup(WDFOBJECT object)
{
    SCX_DEVICE *ctx = ScxContext((WDFDEVICE)object);
    /* Cleanup is not a serialized I/O callback. Do not hold the device lock
     * while joining a worker that can acquire that lock. No timed join/unmap:
     * the worker must be gone before its context or mapped storage is freed. */
    if (ctx->ChannelStop) SetEvent(ctx->ChannelStop);
    if (ctx->ChannelThread) {
        WaitForSingleObject(ctx->ChannelThread, INFINITE);
        CloseHandle(ctx->ChannelThread);
        ctx->ChannelThread = NULL;
    }
    if (ctx->ChannelWords) { UnmapViewOfFile((LPCVOID)ctx->ChannelWords); ctx->ChannelWords = NULL; }
    if (ctx->ChannelMap) { CloseHandle(ctx->ChannelMap); ctx->ChannelMap = NULL; }
    if (ctx->ChannelInput) { CloseHandle(ctx->ChannelInput); ctx->ChannelInput = NULL; }
    if (ctx->ChannelDone) { CloseHandle(ctx->ChannelDone); ctx->ChannelDone = NULL; }
    if (ctx->ChannelFeedback) { CloseHandle(ctx->ChannelFeedback); ctx->ChannelFeedback = NULL; }
    if (ctx->ChannelStop) { CloseHandle(ctx->ChannelStop); ctx->ChannelStop = NULL; }
}
static NTSTATUS ScxChannelStart(WDFDEVICE device, SCX_DEVICE *ctx)
{
    WCHAR id[33] = {0}, name[96];
    uint8_t info[32];
    NTSTATUS status = ScxReadCreationProperty(device, &ScxBootstrapKey, id, sizeof(id), DEVPROP_TYPE_STRING);
    unsigned n;
    if (!NT_SUCCESS(status)) return status;
    for (n = 0; n < 32; ++n)
        if (!((id[n] >= L'0' && id[n] <= L'9') || (id[n] >= L'a' && id[n] <= L'f'))) return STATUS_ACCESS_DENIED;
    if (id[32] != 0) return STATUS_ACCESS_DENIED;
    wcscpy_s(name, 96, L"Global\\SCX.VPad."); wcscat_s(name, 96, id);
    ctx->ChannelMap = OpenFileMappingW(FILE_MAP_READ | FILE_MAP_WRITE, FALSE, name);
    if (!ctx->ChannelMap) return STATUS_ACCESS_DENIED;
    ctx->ChannelWords = (volatile LONG *)MapViewOfFile(ctx->ChannelMap, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, 512);
    if (!ctx->ChannelWords) return STATUS_INSUFFICIENT_RESOURCES;
    if (ScxChannelRead(ctx, 0) != 0x53435843 || ScxChannelRead(ctx, 4) != 1 ||
        ScxChannelRead(ctx, 8) != (LONG)ctx->Profile) return STATUS_INVALID_PARAMETER;
    wcscat_s(name, 96, L".in");
    ctx->ChannelInput = OpenEventW(SYNCHRONIZE | EVENT_MODIFY_STATE, FALSE, name);
    name[wcslen(name) - 3] = 0; wcscat_s(name, 96, L".done");
    ctx->ChannelDone = OpenEventW(SYNCHRONIZE | EVENT_MODIFY_STATE, FALSE, name);
    name[wcslen(name) - 5] = 0; wcscat_s(name, 96, L".feedback");
    ctx->ChannelFeedback = OpenEventW(SYNCHRONIZE | EVENT_MODIFY_STATE, FALSE, name);
    ctx->ChannelStop = CreateEventW(NULL, TRUE, FALSE, NULL);
    if (!ctx->ChannelInput || !ctx->ChannelDone || !ctx->ChannelFeedback || !ctx->ChannelStop) return STATUS_ACCESS_DENIED;
    if (scx_vpad_get_info_for_profile(ctx->Profile, info, sizeof(info)) != 0) return STATUS_INTERNAL_ERROR;
    ScxChannelWrite(ctx, 288, info, sizeof(info));
    ScxChannelPublish(ctx);
    InterlockedExchange(ctx->ChannelWords + 3, 1);
    ctx->ChannelThread = CreateThread(NULL, 0, ScxChannelWorker, device, 0, NULL);
    if (!ctx->ChannelThread) { InterlockedExchange(ctx->ChannelWords + 3, 0); return STATUS_INSUFFICIENT_RESOURCES; }
    return STATUS_SUCCESS;
}
