// Read-only identity inventory using Microsoft's public GameInput v3 API.
// SDK dependency is local/test-only; no game modules are modified or injected.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <GameInput-3.5.262.h>
#include <cstdio>
#include <cwchar>
#include <mutex>
#include <map>
#include <set>
#include <string>
#include <algorithm>
#include <cctype>
using namespace GameInput::v3;
static std::mutex output;
static std::map<std::string,std::string> sonyDevices;
static const char* Label(GameInputLabel label) {
    switch(label) {
    case GameInputLabelIconCross: return "Cross";
    case GameInputLabelIconCircle: return "Circle";
    case GameInputLabelIconSquare: return "Square";
    case GameInputLabelIconTriangle: return "Triangle";
    case GameInputLabelXboxA: return "XboxA";
    case GameInputLabelXboxB: return "XboxB";
    case GameInputLabelXboxX: return "XboxX";
    case GameInputLabelXboxY: return "XboxY";
    default: return "other";
    }
}
static void CALLBACK DeviceChanged(GameInputCallbackToken,void*,IGameInputDevice* device,
    uint64_t timestamp,GameInputDeviceStatus status,GameInputDeviceStatus previous) {
    const GameInputDeviceInfo* i=nullptr;
    const HRESULT hr=device->GetDeviceInfo(&i);
    if(FAILED(hr) || !i) return;
    if(!(i->supportedInput&GameInputKindGamepad) &&
       !(i->usage.page==1 && (i->usage.id==4 || i->usage.id==5))) return;
    std::lock_guard<std::mutex> lock(output);
    std::string path=i->pnpPath?i->pnpPath:"";
    std::transform(path.begin(),path.end(),path.begin(),[](unsigned char c){return static_cast<char>(std::tolower(c));});
    if(path.find("hid#scxvpad#")!=std::string::npos) {
        if(status&GameInputDeviceConnected) {
            const auto* bytes=reinterpret_cast<const unsigned char*>(&i->deviceId);
            sonyDevices[path]=std::string(reinterpret_cast<const char*>(bytes),sizeof(i->deviceId));
        } else sonyDevices.erase(path);
    }
    printf("GI3 timestamp=%llu uptime-ms=%llu status=%x previous=%x vid=%04x pid=%04x rev=%04x family=%d usage=%x:%x kinds=%x rumble=%x system-buttons=%x\n",
        timestamp,GetTickCount64(),status,previous,i->vendorId,i->productId,i->revisionNumber,i->deviceFamily,
        i->usage.page,i->usage.id,i->supportedInput,i->supportedRumbleMotors,i->supportedSystemButtons);
    printf(" name=%s\n path=%s\n",i->displayName?i->displayName:"",i->pnpPath?i->pnpPath:"");
    const GUID& c=i->containerId;
    printf(" container=%08lx-%04x-%04x-%02x%02x-%02x%02x%02x%02x%02x%02x\n",
        c.Data1,c.Data2,c.Data3,c.Data4[0],c.Data4[1],c.Data4[2],c.Data4[3],c.Data4[4],c.Data4[5],c.Data4[6],c.Data4[7]);
    printf(" firmware=%u.%u.%u.%u hardware=%u.%u.%u.%u reports=%u/%u ffb=%u\n",
        i->firmwareVersion.major,i->firmwareVersion.minor,i->firmwareVersion.build,i->firmwareVersion.revision,
        i->hardwareVersion.major,i->hardwareVersion.minor,i->hardwareVersion.build,i->hardwareVersion.revision,
        i->inputReportCount,i->outputReportCount,i->forceFeedbackMotorCount);
    if(i->controllerInfo) printf(" controller axes=%u buttons=%u switches=%u\n",
        i->controllerInfo->controllerAxisCount,i->controllerInfo->controllerButtonCount,i->controllerInfo->controllerSwitchCount);
    if(i->sensorsInfo) printf(" sensors=%x\n",i->sensorsInfo->supportedSensors);
    if(i->gamepadInfo) {
        const auto* g=i->gamepadInfo;
        printf(" gamepad layout=%x labels=%d/%d/%d/%d (%s/%s/%s/%s) extra=%u/%u\n",
            g->supportedLayout,g->aButtonLabel,g->bButtonLabel,g->xButtonLabel,g->yButtonLabel,
            Label(g->aButtonLabel),Label(g->bButtonLabel),Label(g->xButtonLabel),Label(g->yButtonLabel),
            g->extraButtonCount,g->extraAxisCount);
    }
    for(uint32_t n=0;n<i->inputReportCount && n<128;++n) printf(" input id=%u size=%u kind=%d\n",i->inputReportInfo[n].id,i->inputReportInfo[n].size,i->inputReportInfo[n].kind);
    for(uint32_t n=0;n<i->outputReportCount && n<128;++n) printf(" output id=%u size=%u kind=%d\n",i->outputReportInfo[n].id,i->outputReportInfo[n].size,i->outputReportInfo[n].kind);
}
int wmain(int argc,wchar_t** argv) {
    if((argc!=2 && argc!=3 && argc!=5) || wcslen(argv[1])<4 || argv[1][1]!=L':') {
        puts("gameinput3_probe ABSOLUTE_RUNTIME_DLL [seconds 1..600] [--expect-scx-sony count]");return 2;
    }
    const int seconds=argc>=3?_wtoi(argv[2]):5;
    if(seconds<1 || seconds>600) return 2;
    const int expected=argc==5?_wtoi(argv[4]):-1;
    if(argc==5 && (wcscmp(argv[3],L"--expect-scx-sony") || expected<1 || expected>4)) return 2;
    setvbuf(stdout,nullptr,_IONBF,0);
    HMODULE library=LoadLibraryExW(argv[1],nullptr,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_SYSTEM32);
    if(!library) {printf("LoadLibrary error=%lu\n",GetLastError());return 1;}
    using Initialize=HRESULT (WINAPI*)(REFIID,void**);
    auto initialize=reinterpret_cast<Initialize>(reinterpret_cast<uintptr_t>(GetProcAddress(library,"GameInputInitialize")));
    if(!initialize) {printf("GameInputInitialize export missing error=%lu\n",GetLastError());FreeLibrary(library);return 1;}
    IGameInput* input=nullptr;
    HRESULT hr=initialize(IID_IGameInput,reinterpret_cast<void**>(&input));
    wprintf(L"runtime=%ls\n",argv[1]);printf("GameInputInitialize(v3)=%08lx\n",hr);
    if(FAILED(hr)) {FreeLibrary(library);return 1;}
    GameInputCallbackToken token=0;
    hr=input->RegisterDeviceCallback(nullptr,static_cast<GameInputKind>(0xffffffffu),GameInputDeviceAnyStatus,
        GameInputBlockingEnumeration,nullptr,DeviceChanged,&token);
    printf("RegisterDeviceCallback=%08lx\n",hr);
    if(SUCCEEDED(hr)) {Sleep(static_cast<DWORD>(seconds)*1000);input->UnregisterCallback(token);}
    bool matched=true;
    if(expected>=0) {
        std::lock_guard<std::mutex> lock(output);
        std::set<std::string> ids;
        for(const auto& entry:sonyDevices) ids.insert(entry.second);
        matched=sonyDevices.size()==static_cast<size_t>(expected) && ids.size()==static_cast<size_t>(expected);
        printf("%s GameInput v3: expected=%d distinct-paths=%zu distinct-device-ids=%zu\n",
            matched?"PASS":"FAIL",expected,sonyDevices.size(),ids.size());
    }
    input->Release();FreeLibrary(library);return FAILED(hr)||!matched?1:0;
}
