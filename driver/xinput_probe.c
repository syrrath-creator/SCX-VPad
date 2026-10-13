/* Independent consumer: loads Microsoft's installed XInput 1.4, not SCX code.
 * No device creation, driver installation, or elevation. See ../LICENSE. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <Xinput.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef DWORD (WINAPI *GET_STATE)(DWORD, XINPUT_STATE *);
typedef DWORD (WINAPI *GET_CAPS)(DWORD, DWORD, XINPUT_CAPABILITIES *);
typedef DWORD (WINAPI *SET_STATE)(DWORD, XINPUT_VIBRATION *);
typedef DWORD (WINAPI *GET_BATTERY)(DWORD, BYTE, XINPUT_BATTERY_INFORMATION *);

int main(int argc, char **argv)
{
    HMODULE module;
    GET_STATE getState;
    GET_CAPS getCaps;
    SET_STATE setState;
    GET_BATTERY getBattery;
    DWORD packets[4] = {0}, lastResult[4] = {~0u,~0u,~0u,~0u};
    ULONGLONG deadline;
    unsigned seconds = 0, connected = 0, rumble = 0, slot = 4;
    int index;
    for (index = 1; index < argc; ++index) {
        if (!strcmp(argv[index], "--seconds") && index + 1 < argc)
            seconds = (unsigned)strtoul(argv[++index], NULL, 10);
        else if (!strcmp(argv[index], "--slot") && index + 1 < argc)
            slot = (unsigned)strtoul(argv[++index], NULL, 10);
        else if (!strcmp(argv[index], "--rumble")) rumble = 1;
        else {
            fprintf(stderr, "Usage: xinput_probe [--seconds 0..60] [--slot 0..3] [--rumble]\n");
            return 2;
        }
    }
    if (seconds > 60 || slot > 4 || (rumble && slot == 4)) {
        fprintf(stderr, "Rumble requires an explicit slot 0..3; duration must be 0..60.\n");
        return 2;
    }
    module = LoadLibraryExW(L"xinput1_4.dll", NULL, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (module == NULL) { fprintf(stderr, "XInput load failed: %lu\n", GetLastError()); return 2; }
#pragma warning(push)
#pragma warning(disable: 4191)
    getState = (GET_STATE)GetProcAddress(module, "XInputGetState");
    getCaps = (GET_CAPS)GetProcAddress(module, "XInputGetCapabilities");
    setState = (SET_STATE)GetProcAddress(module, "XInputSetState");
    getBattery = (GET_BATTERY)GetProcAddress(module, "XInputGetBatteryInformation");
#pragma warning(pop)
    if (!getState || !getCaps || !setState || !getBattery) { FreeLibrary(module); return 2; }
    setvbuf(stdout, NULL, _IONBF, 0);
    deadline = GetTickCount64() + seconds * 1000ull;
    do {
        DWORD user;
        for (user = 0; user < 4; ++user) {
            XINPUT_STATE state = {0};
            DWORD result;
            if (slot != 4 && user != slot) continue;
            result = getState(user, &state);
            if (result == ERROR_SUCCESS) {
                connected |= 1u << user;
                if (lastResult[user] != ERROR_SUCCESS) {
                    XINPUT_CAPABILITIES caps = {0};
                    XINPUT_BATTERY_INFORMATION battery = {0};
                    DWORD capResult = getCaps(user, 0, &caps);
                    DWORD batteryResult = getBattery(user, BATTERY_DEVTYPE_GAMEPAD, &battery);
                    printf("{\"event\":\"connected\",\"slot\":%lu,\"capResult\":%lu,\"type\":%u,\"subtype\":%u,\"flags\":%u,\"batteryResult\":%lu,\"batteryType\":%u,\"batteryLevel\":%u}\n",
                           user, capResult, caps.Type, caps.SubType, caps.Flags,
                           batteryResult, battery.BatteryType, battery.BatteryLevel);
                    if (rumble) {
                        XINPUT_VIBRATION vibration = {16000,9000};
                        DWORD rumbleResult = setState(user, &vibration);
                        printf("{\"event\":\"rumble\",\"slot\":%lu,\"result\":%lu,\"left\":16000,\"right\":9000}\n", user, rumbleResult);
                        Sleep(100);
                        vibration.wLeftMotorSpeed = vibration.wRightMotorSpeed = 0;
                        rumbleResult = setState(user, &vibration);
                        printf("{\"event\":\"rumble-stop\",\"slot\":%lu,\"result\":%lu}\n", user, rumbleResult);
                    }
                }
                if (lastResult[user] != ERROR_SUCCESS || packets[user] != state.dwPacketNumber) {
                    printf("{\"event\":\"state\",\"slot\":%lu,\"packet\":%lu,\"buttons\":%u,\"leftTrigger\":%u,\"rightTrigger\":%u,\"leftX\":%d,\"leftY\":%d,\"rightX\":%d,\"rightY\":%d}\n",
                           user, state.dwPacketNumber, state.Gamepad.wButtons,
                           state.Gamepad.bLeftTrigger, state.Gamepad.bRightTrigger,
                           state.Gamepad.sThumbLX, state.Gamepad.sThumbLY,
                           state.Gamepad.sThumbRX, state.Gamepad.sThumbRY);
                }
                packets[user] = state.dwPacketNumber;
            } else if (lastResult[user] != result) {
                printf("{\"event\":\"unavailable\",\"slot\":%lu,\"result\":%lu}\n", user, result);
            }
            lastResult[user] = result;
        }
        if (GetTickCount64() >= deadline) break;
        Sleep(8);
    } while (1);
    FreeLibrary(module);
    return connected ? 0 : 1;
}
