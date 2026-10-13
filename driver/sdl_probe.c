/* Independent SDL3 consumer. No SCX protocol/client/core code is linked.
 * SDL3 is an external zlib-licensed test dependency. See ../LICENSE. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <SDL3/SDL.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct SAMPLE {
    Sint16 axes[SDL_GAMEPAD_AXIS_COUNT];
    Uint32 buttons;
    float gyro[3], accel[3], x[2], y[2], pressure[2];
    bool down[2];
} SAMPLE;

static void quoted(const char *value)
{
    const unsigned char *cursor = (const unsigned char *)(value ? value : "");
    putchar('"');
    for (; *cursor; ++cursor) {
        if (*cursor == '"' || *cursor == '\\') { putchar('\\'); putchar(*cursor); }
        else if (*cursor < 32) printf("\\u%04x", *cursor);
        else putchar(*cursor);
    }
    putchar('"');
}

int main(int argc, char **argv)
{
    SDL_Gamepad *pads[16] = {0};
    SDL_JoystickID ids[16] = {0};
    SAMPLE previous[16] = {0};
    bool first[16] = {0};
    unsigned opened = 0, seconds = 10, all = 0;
    ULONGLONG deadline;
    int count = 0, index;
    SDL_JoystickID *available;
    for (index = 1; index < argc; ++index) {
        if (!strcmp(argv[index], "--seconds") && index + 1 < argc)
            seconds = (unsigned)strtoul(argv[++index], NULL, 10);
        else if (!strcmp(argv[index], "--all")) all = 1;
        else { fprintf(stderr, "Usage: sdl_probe [--seconds 0..60] [--all]\n"); return 2; }
    }
    if (seconds > 60) return 2;
    setvbuf(stdout, NULL, _IONBF, 0);
    SDL_SetHint(SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS, "1");
    SDL_SetHint(SDL_HINT_JOYSTICK_HIDAPI, "1");
    if (!SDL_Init(SDL_INIT_GAMEPAD)) {
        fprintf(stderr, "SDL init failed: %s\n", SDL_GetError()); return 2;
    }
    SDL_SetGamepadEventsEnabled(false);
    available = SDL_GetGamepads(&count);
    for (index = 0; available && index < count; ++index) {
        SDL_JoystickID id = available[index];
        const char *name = SDL_GetGamepadNameForID(id);
        const char *path = SDL_GetGamepadPathForID(id);
        SDL_Gamepad *pad;
        printf("{\"event\":\"detected\",\"id\":%u,\"name\":", id); quoted(name);
        printf(",\"path\":"); quoted(path); puts("}");
        if ((!all && (!name || !strstr(name, "SCX"))) || opened == 16) continue;
        pad = SDL_OpenGamepad(id);
        if (!pad) { fprintf(stderr, "Open %u failed: %s\n", id, SDL_GetError()); continue; }
        pads[opened] = pad; ids[opened] = id; first[opened] = true; ++opened;
        printf("{\"event\":\"opened\",\"id\":%u,\"type\":%d,\"vendor\":%u,\"product\":%u,\"touchpads\":%d,\"gyro\":%s,\"accel\":%s,\"gyroEnabled\":%s,\"accelEnabled\":%s}\n",
               id, SDL_GetGamepadType(pad), SDL_GetGamepadVendor(pad), SDL_GetGamepadProduct(pad),
               SDL_GetNumGamepadTouchpads(pad),
               SDL_GamepadHasSensor(pad, SDL_SENSOR_GYRO) ? "true" : "false",
               SDL_GamepadHasSensor(pad, SDL_SENSOR_ACCEL) ? "true" : "false",
               SDL_SetGamepadSensorEnabled(pad, SDL_SENSOR_GYRO, true) ? "true" : "false",
               SDL_SetGamepadSensorEnabled(pad, SDL_SENSOR_ACCEL, true) ? "true" : "false");
    }
    SDL_free(available);
    deadline = GetTickCount64() + seconds * 1000ull;
    do {
        unsigned padIndex;
        SDL_UpdateGamepads();
        for (padIndex = 0; padIndex < opened; ++padIndex) {
            SAMPLE sample = {0};
            SDL_Gamepad *pad = pads[padIndex];
            int n;
            for (n = 0; n < SDL_GAMEPAD_AXIS_COUNT; ++n)
                sample.axes[n] = SDL_GetGamepadAxis(pad, (SDL_GamepadAxis)n);
            for (n = 0; n < SDL_GAMEPAD_BUTTON_COUNT && n < 32; ++n)
                if (SDL_GetGamepadButton(pad, (SDL_GamepadButton)n)) sample.buttons |= 1u << n;
            SDL_GetGamepadSensorData(pad, SDL_SENSOR_GYRO, sample.gyro, 3);
            SDL_GetGamepadSensorData(pad, SDL_SENSOR_ACCEL, sample.accel, 3);
            for (n = 0; n < 2 && SDL_GetNumGamepadTouchpads(pad) > 0; ++n)
                SDL_GetGamepadTouchpadFinger(pad, 0, n, &sample.down[n],
                                              &sample.x[n], &sample.y[n], &sample.pressure[n]);
            if (!first[padIndex] && !memcmp(&sample, &previous[padIndex], sizeof(sample))) continue;
            first[padIndex] = false; previous[padIndex] = sample;
            printf("{\"event\":\"sample\",\"id\":%u,\"buttons\":%u,\"axes\":[%d,%d,%d,%d,%d,%d],\"gyro\":[%.5f,%.5f,%.5f],\"accel\":[%.5f,%.5f,%.5f],\"touch\":[[%s,%.5f,%.5f,%.5f],[%s,%.5f,%.5f,%.5f]]}\n",
                   ids[padIndex], sample.buttons, sample.axes[0], sample.axes[1], sample.axes[2],
                   sample.axes[3], sample.axes[4], sample.axes[5], sample.gyro[0], sample.gyro[1], sample.gyro[2],
                   sample.accel[0], sample.accel[1], sample.accel[2],
                   sample.down[0] ? "true" : "false", sample.x[0], sample.y[0], sample.pressure[0],
                   sample.down[1] ? "true" : "false", sample.x[1], sample.y[1], sample.pressure[1]);
        }
        if (GetTickCount64() >= deadline) break;
        Sleep(8);
    } while (1);
    for (index = 0; index < (int)opened; ++index) SDL_CloseGamepad(pads[index]);
    SDL_Quit();
    return opened ? 0 : 1;
}
