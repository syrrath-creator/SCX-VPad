// Bounded DirectInput regression probe for one physical DualSense Edge instance.
// The supervisor terminates only its own child on timeout. No input/output or
// feature reports are written and no virtual devices or leases are created.
#define WIN32_LEAN_AND_MEAN
#define COBJMACROS
#define DIRECTINPUT_VERSION 0x0800
#include <windows.h>
#include <dinput.h>
#include <objbase.h>
#include <stdio.h>
#include <wchar.h>
#include <stdlib.h>
#include <string.h>

typedef struct FIND_DEVICE {
    GUID target;
    GUID instance;
    DWORD product;
    unsigned found;
} FIND_DEVICE;

static BOOL CALLBACK found_device(const DIDEVICEINSTANCEW* info, void* raw) {
    FIND_DEVICE* match = (FIND_DEVICE*)raw;
    if (!IsEqualGUID(&info->guidInstance, &match->target)) return DIENUM_CONTINUE;
    match->instance = info->guidInstance;
    match->product = info->guidProduct.Data1;
    ++match->found;
    wprintf(L"MATCH product=%ls instance=%ls vid=%04x pid=%04x\n",
        info->tszProductName, info->tszInstanceName,
        LOWORD(info->guidProduct.Data1), HIWORD(info->guidProduct.Data1));
    return DIENUM_CONTINUE;
}

static void stage(const char* label, ULONGLONG began) {
    printf("STAGE %s elapsed_ms=%llu\n", label, GetTickCount64() - began);
    fflush(stdout);
}

static void result(const char* label, HRESULT hr, ULONGLONG began) {
    printf("CALL %s hr=%08lx elapsed_ms=%llu\n", label, (unsigned long)hr,
        GetTickCount64() - began);
    fflush(stdout);
}

// Use the known baseline path directly: a guarded device may intentionally
// disappear from DirectInput enumeration, which cannot prove raw-open denial.
static int probe_foreign_path(const wchar_t* path) {
    const ULONGLONG began = GetTickCount64();
    HANDLE raw;
    DWORD error;
    stage("ForeignReadWriteOpenKnownPath", began);
    raw = CreateFileW(path, GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING,
        FILE_FLAG_OVERLAPPED, NULL);
    error = raw == INVALID_HANDLE_VALUE ? GetLastError() : ERROR_SUCCESS;
    printf("FOREIGN_OPEN allowed=%d win32=%lu elapsed_ms=%llu\n",
        raw != INVALID_HANDLE_VALUE, error, GetTickCount64() - began);
    if (raw != INVALID_HANDLE_VALUE) { CloseHandle(raw); return 31; }
    return error == ERROR_ACCESS_DENIED ? 0 : 32;
}

static int probe(const GUID* target, BOOL preopen, BOOL foreign_open) {
    const ULONGLONG began = GetTickCount64();
    IDirectInput8W* input = NULL;
    IDirectInputDevice8W* device = NULL;
    HWND window = NULL;
    FIND_DEVICE match = {0};
    DIPROPGUIDANDPATH path = {0};
    DIJOYSTATE2 state = {0};
    HRESULT hr;
    int code = 0;
    BOOL acquired = FALSE;
    unsigned acquire_failures = 0, state_failures = 0, successes = 0;
    ULONGLONG max_acquire_ms = 0;
    HRESULT first_error = S_OK;
    match.target = *target;
    stage("DirectInput8Create", began);
    hr = DirectInput8Create(GetModuleHandleW(NULL), DIRECTINPUT_VERSION,
        &IID_IDirectInput8W, (void**)&input, NULL);
    result("DirectInput8Create", hr, began);
    if (FAILED(hr)) return 19;
    stage("EnumDevices", began);
    hr = IDirectInput8_EnumDevices(input, DI8DEVCLASS_GAMECTRL, found_device,
        &match, DIEDFL_ATTACHEDONLY);
    result("EnumDevices", hr, began);
    printf("ENUM_MATCHES count=%u\n", match.found);
    if (FAILED(hr) || match.found != 1) { code = 10; goto cleanup; }
    if (match.product != 0x0df2054c) { code = 11; goto cleanup; }
    stage("CreateDevice", began);
    hr = IDirectInput8_CreateDevice(input, &match.instance, &device, NULL);
    result("CreateDevice", hr, began);
    if (FAILED(hr)) {
        if (hr == E_ACCESSDENIED) {
            printf("BLOCKED stage=CreateDevice hr=%08lx\n", (unsigned long)hr);
            code = 26;
        } else code = 20;
        goto cleanup;
    }
    path.diph.dwSize = sizeof(path);
    path.diph.dwHeaderSize = sizeof(path.diph);
    path.diph.dwHow = DIPH_DEVICE;
    stage("GetPropertyPath", began);
    hr = IDirectInputDevice8_GetProperty(device, DIPROP_GUIDANDPATH, &path.diph);
    result("GetPropertyPath", hr, began);
    if (FAILED(hr)) { code = 21; goto cleanup; }
    wprintf(L"HID_PATH=%ls\n", path.wszPath);
    fflush(stdout);
    if (foreign_open) {
        HANDLE raw;
        DWORD error;
        stage("ForeignReadWriteOpen", began);
        raw = CreateFileW(path.wszPath, GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED, NULL);
        error = raw == INVALID_HANDLE_VALUE ? GetLastError() : ERROR_SUCCESS;
        printf("FOREIGN_OPEN allowed=%d win32=%lu elapsed_ms=%llu\n",
            raw != INVALID_HANDLE_VALUE, error, GetTickCount64() - began);
        if (raw != INVALID_HANDLE_VALUE) { CloseHandle(raw); code = 31; }
        else code = error == ERROR_ACCESS_DENIED ? 0 : 32;
        goto cleanup;
    }
    window = CreateWindowExW(0, L"STATIC", L"SCX bounded DirectInput probe", 0,
        0, 0, 0, 0, NULL, NULL, GetModuleHandleW(NULL), NULL);
    if (!window) { code = 23; goto cleanup; }
    stage("SetDataFormat", began);
    hr = IDirectInputDevice8_SetDataFormat(device, &c_dfDIJoystick2);
    result("SetDataFormat", hr, began);
    if (FAILED(hr)) { code = 22; goto cleanup; }
    stage("SetCooperativeLevel", began);
    hr = IDirectInputDevice8_SetCooperativeLevel(device, window,
        DISCL_BACKGROUND | DISCL_NONEXCLUSIVE);
    result("SetCooperativeLevel", hr, began);
    if (FAILED(hr)) { code = 23; goto cleanup; }
    if (preopen) {
        char signal[16] = {0};
        stage("PreopenAcquire", began);
        hr = IDirectInputDevice8_Acquire(device);
        result("PreopenAcquire", hr, began);
        if (FAILED(hr)) { code = 24; goto cleanup; }
        acquired = TRUE;
        printf("READY waiting_for_lease\n");
        fflush(stdout);
        if (!fgets(signal, sizeof(signal), stdin) || strcmp(signal, "GO\n")) {
            code = 27; goto cleanup;
        }
        stage("PreopenGetDeviceState", began);
        hr = IDirectInputDevice8_GetDeviceState(device, sizeof(state), &state);
        result("PreopenGetDeviceState", hr, began);
    }
    stage("RepeatedUnacquireAcquirePollState", began);
    for (unsigned cycle = 0; cycle < 100; ++cycle) {
        ULONGLONG acquire_began;
        if (acquired) {
            IDirectInputDevice8_Unacquire(device);
            acquired = FALSE;
        }
        acquire_began = GetTickCount64();
        hr = IDirectInputDevice8_Acquire(device);
        ULONGLONG acquire_ms = GetTickCount64() - acquire_began;
        if (acquire_ms > max_acquire_ms) max_acquire_ms = acquire_ms;
        if (FAILED(hr)) {
            ++acquire_failures;
            code = 24;
        } else {
            acquired = TRUE;
            // Give the asynchronous HID completion time to report failure;
            // querying immediately after Acquire can return cached neutral data.
            Sleep(10);
            IDirectInputDevice8_Poll(device);
            hr = IDirectInputDevice8_GetDeviceState(device, sizeof(state), &state);
            if (FAILED(hr)) { ++state_failures; code = 25; }
            else ++successes;
        }
        if (FAILED(hr) && SUCCEEDED(first_error)) {
            first_error = hr;
            printf("FIRST_ERROR cycle=%u hr=%08lx elapsed_ms=%llu\n",
                cycle, (unsigned long)hr, GetTickCount64() - began);
            fflush(stdout);
        }
        if (!acquired) Sleep(10);
    }
    printf("CYCLES count=100 success=%u acquire_fail=%u state_fail=%u max_acquire_ms=%llu elapsed_ms=%llu\n",
        successes, acquire_failures, state_failures, max_acquire_ms, GetTickCount64() - began);
cleanup:
    if (device) {
        if (acquired) {
            stage("Unacquire", began);
            hr = IDirectInputDevice8_Unacquire(device);
            result("Unacquire", hr, began);
        }
        stage("DeviceRelease", began);
        IDirectInputDevice8_Release(device);
    }
    if (window) DestroyWindow(window);
    if (input) {
        stage("InputRelease", began);
        IDirectInput8_Release(input);
    }
    printf("CHILD_RESULT code=%d elapsed_ms=%llu\n", code, GetTickCount64() - began);
    return code;
}

int wmain(int argc, wchar_t** argv) {
    GUID target;
    DWORD timeout = 5000;
    BOOL child = FALSE;
    BOOL preopen = FALSE;
    BOOL foreign_open = FALSE;
    BOOL foreign_path = FALSE;
    wchar_t exe[32768], command[33000];
    STARTUPINFOW startup = {0};
    PROCESS_INFORMATION process = {0};
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits = {0};
    HANDLE job = NULL;
    ULONGLONG began;
    DWORD wait, code;
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc < 2 || argc > 5) {
        fwprintf(stderr, L"Usage: dinput-probe.exe {instance-guid}|HID-path [timeout-ms] [cold|preopen|foreign-open|foreign-path] [--child]\n");
        return 2;
    }
    if (argc >= 3) {
        wchar_t* end = NULL;
        timeout = wcstoul(argv[2], &end, 10);
        if (!end || *end || timeout < 500 || timeout > 15000) return 2;
    }
    if (argc >= 4) {
        if (wcscmp(argv[3], L"cold") && wcscmp(argv[3], L"preopen")
            && wcscmp(argv[3], L"foreign-open") && wcscmp(argv[3], L"foreign-path")) return 2;
        preopen = !wcscmp(argv[3], L"preopen");
        foreign_open = !wcscmp(argv[3], L"foreign-open");
        foreign_path = !wcscmp(argv[3], L"foreign-path");
    }
    if (argc == 5) {
        if (wcscmp(argv[4], L"--child")) return 2;
        child = TRUE;
    }
    if (foreign_path) {
        wchar_t lowered[32768];
        if (wcschr(argv[1], L'"') || _wcsnicmp(argv[1], L"\\\\?\\hid#", 8)
            || wcsncpy_s(lowered, ARRAYSIZE(lowered), argv[1], _TRUNCATE)) return 2;
        if (_wcslwr_s(lowered, ARRAYSIZE(lowered))) return 2;
        if (!wcsstr(lowered, L"vid&0002054c_pid&0df2")
            && !wcsstr(lowered, L"vid_054c&pid_0df2")) return 2;
        if (wcsstr(lowered, L"scxvpad")) return 2;
    } else if (FAILED(CLSIDFromString(argv[1], &target))) return 2;
    if (child) return foreign_path ? probe_foreign_path(argv[1]) : probe(&target, preopen, foreign_open);
    if (!GetModuleFileNameW(NULL, exe, ARRAYSIZE(exe))) return 3;
    if (_snwprintf_s(command, ARRAYSIZE(command), _TRUNCATE,
        L"\"%ls\" \"%ls\" %lu %ls --child", exe, argv[1], timeout,
        preopen ? L"preopen" : (foreign_path ? L"foreign-path" : (foreign_open ? L"foreign-open" : L"cold"))) < 0) return 3;
    startup.cb = sizeof(startup);
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    startup.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
    startup.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    job = CreateJobObjectW(NULL, NULL);
    if (!job) return 3;
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if (!SetInformationJobObject(job, JobObjectExtendedLimitInformation, &limits, sizeof(limits))) {
        CloseHandle(job); return 3;
    }
    began = GetTickCount64();
    if (!CreateProcessW(exe, command, NULL, NULL, TRUE, CREATE_NO_WINDOW | CREATE_SUSPENDED,
        NULL, NULL, &startup, &process)) {
        printf("SUPERVISOR_ERROR CreateProcess win32=%lu\n", GetLastError());
        CloseHandle(job);
        return 3;
    }
    if (!AssignProcessToJobObject(job, process.hProcess) || ResumeThread(process.hThread) == (DWORD)-1) {
        TerminateProcess(process.hProcess, 126);
        CloseHandle(process.hThread); CloseHandle(process.hProcess); CloseHandle(job);
        return 126;
    }
    CloseHandle(process.hThread);
    wait = WaitForSingleObject(process.hProcess, timeout);
    if (wait == WAIT_TIMEOUT) {
        printf("TIMEOUT limit_ms=%lu elapsed_ms=%llu\n", timeout, GetTickCount64() - began);
        if (!TerminateProcess(process.hProcess, 124)) {
            printf("SUPERVISOR_ERROR TerminateProcess win32=%lu\n", GetLastError());
            CloseHandle(process.hProcess);
            CloseHandle(job);
            return 125;
        }
        WaitForSingleObject(process.hProcess, 2000);
        code = 124;
    } else if (wait != WAIT_OBJECT_0 || !GetExitCodeProcess(process.hProcess, &code)) {
        TerminateProcess(process.hProcess, 126);
        code = 126;
    }
    CloseHandle(process.hProcess);
    CloseHandle(job);
    printf("RESULT code=%lu elapsed_ms=%llu\n", code, GetTickCount64() - began);
    return (int)code;
}
