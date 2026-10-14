// Read-only public API inventory. Optional --sony-snapshot reads only virtual
// Sony devices; no physical controller input or output effects are accessed.
#define WIN32_LEAN_AND_MEAN
#define DIRECTINPUT_VERSION 0x0800
#include <windows.h>
#include <hidsdi.h>
#include <setupapi.h>
#include <cfgmgr32.h>
#include <dinput.h>
#include <GameInput.h>
#include <aclapi.h>
#include <sddl.h>
#include <cstdio>
#include <vector>
#include <mutex>

static std::mutex outputLock;

static int RawDevices() {
    UINT count=0;
    if(GetRawInputDeviceList(nullptr,&count,sizeof(RAWINPUTDEVICELIST))==UINT(-1)) {
        printf("RAW list-error=%lu\n",GetLastError());return 1;
    }
    std::vector<RAWINPUTDEVICELIST> devices(count);
    const UINT found=GetRawInputDeviceList(devices.data(),&count,sizeof(RAWINPUTDEVICELIST));
    if(found==UINT(-1)) {printf("RAW list-error=%lu\n",GetLastError());return 1;}
    unsigned gamepads=0;
    for(UINT n=0;n<found;++n) {
        if(devices[n].dwType!=RIM_TYPEHID) continue;
        RID_DEVICE_INFO info{};info.cbSize=sizeof(info);UINT bytes=sizeof(info);
        if(GetRawInputDeviceInfoW(devices[n].hDevice,RIDI_DEVICEINFO,&info,&bytes)==UINT(-1)) continue;
        if(info.hid.usUsagePage!=1 || (info.hid.usUsage!=4 && info.hid.usUsage!=5)) continue;
        UINT chars=0;
        GetRawInputDeviceInfoW(devices[n].hDevice,RIDI_DEVICENAME,nullptr,&chars);
        if(!chars || chars>32768) continue;
        std::vector<WCHAR> name(chars+1);
        if(GetRawInputDeviceInfoW(devices[n].hDevice,RIDI_DEVICENAME,name.data(),&chars)==UINT(-1)) continue;
        ++gamepads;
        wprintf(L"RAW vid=%04lx pid=%04lx revision=%04lx usage=%04x:%04x path=%ls\n",
            info.hid.dwVendorId,info.hid.dwProductId,info.hid.dwVersionNumber,
            info.hid.usUsagePage,info.hid.usUsage,name.data());
    }
    printf("RAW game-controller-count=%u\n",gamepads);return 0;
}

static void ChannelCounters(DEVINST child,const WCHAR* path) {
    DEVINST parent=0;
    if(CM_Get_Parent(&parent,child,0)!=CR_SUCCESS) return;
    static const DEVPROPKEY key={{0x19f81049,0x3679,0x40f2,{0x96,0x89,0x2a,0x2f,0x61,0xf8,0xbd,0x29}},6};
    WCHAR id[33]{}; DEVPROPTYPE type=0; ULONG size=sizeof(id);
    if(CM_Get_DevNode_PropertyW(parent,&key,&type,reinterpret_cast<PBYTE>(id),&size,0)!=CR_SUCCESS || type!=DEVPROP_TYPE_STRING || size!=sizeof(id) || id[32]) return;
    for(unsigned n=0;n<32;++n) if(!((id[n]>=L'0' && id[n]<=L'9') || (id[n]>=L'a' && id[n]<=L'f'))) return;
    WCHAR name[96]=L"Global\\SCX.VPad.";wcscat_s(name,id);
    HANDLE section=OpenFileMappingW(FILE_MAP_READ,FALSE,name);
    if(!section) return;
    const volatile DWORD* words=static_cast<const volatile DWORD*>(MapViewOfFile(section,FILE_MAP_READ,0,0,512));
    if(words) {
        wprintf(L"CHANNEL COUNTERS path=%ls online=%lu profile=%lu",path,words[3],words[2]);
        for(unsigned n=56;n<64;++n) printf(" %lu",words[n]);
        printf("\nFEEDBACK sequence=%lu valid=%lu left-right=%08lx\n",words[49],words[50],words[51]);
        UnmapViewOfFile(const_cast<const DWORD*>(words));
    }
    CloseHandle(section);
}

static BOOL CALLBACK DiDevice(const DIDEVICEINSTANCEW* d, void* context) {
    wprintf(L"DI type=%08lx product-guid=%08lx instance=%ls product=%ls\n",
        d->dwDevType,d->guidProduct.Data1,d->tszInstanceName,d->tszProductName);
    IDirectInputDevice8W* device=nullptr;
    auto* di=static_cast<IDirectInput8W*>(context);
    if(SUCCEEDED(di->CreateDevice(d->guidInstance,&device,nullptr))) {
        DIPROPGUIDANDPATH path{};path.diph.dwSize=sizeof(path);path.diph.dwHeaderSize=sizeof(path.diph);path.diph.dwHow=DIPH_DEVICE;
        if(SUCCEEDED(device->GetProperty(DIPROP_GUIDANDPATH,&path.diph)))
            wprintf(L" DI path=%ls usage=%04x:%04x\n",path.wszPath,d->wUsagePage,d->wUsage);
        device->Release();
    }
    return DIENUM_CONTINUE;
}

static void CALLBACK GiDevice(GameInputCallbackToken, void*, IGameInputDevice* d,
    uint64_t, GameInputDeviceStatus status, GameInputDeviceStatus) {
    const auto* i=d->GetDeviceInfo();
    if(i->usage.page!=1 || (i->usage.id!=4 && i->usage.id!=5)) return;
    std::lock_guard<std::mutex> lock(outputLock);
    printf("GI vid=%04x pid=%04x revision=%04x family=%d interface=%u collection=%u status=%x kinds=%x rumble=%x caps=%x reports=%u/%u/%u axes=%u buttons=%u switches=%u touch=%u descriptorBytes=%u name=%s\n",
        i->vendorId,i->productId,i->revisionNumber,i->deviceFamily,i->interfaceNumber,
        i->collectionNumber,status,i->supportedInput,i->supportedRumbleMotors,i->capabilities,
        i->inputReportCount,i->outputReportCount,i->featureReportCount,i->controllerAxisCount,
        i->controllerButtonCount,i->controllerSwitchCount,i->touchPointCount,
        i->deviceDescriptorSize,i->displayName?i->displayName->data:"");
    for(uint32_t n=0;n<i->deviceStringCount;++n)
        printf(" GI string[%u]=%s\n",n,i->deviceStrings[n].data);
}

int main(int argc, char** argv) {
    if(argc==2 && strcmp(argv[1],"--raw")==0) return RawDevices();
    if(argc==3 && strcmp(argv[1],"--process")==0) {
        const DWORD pid=strtoul(argv[2],nullptr,10);
        HANDLE process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,FALSE,pid),token=nullptr;
        if(!process) {printf("process error=%lu\n",GetLastError());return 1;}
        if(OpenProcessToken(process,TOKEN_QUERY,&token)) {
            DWORD value=0,needed=0;
            if(GetTokenInformation(token,TokenIsAppContainer,&value,sizeof(value),&needed))
                printf("pid=%lu app-container=%lu\n",pid,value);
            if(GetTokenInformation(token,TokenElevationType,&value,sizeof(value),&needed))
                printf("pid=%lu elevation-type=%lu\n",pid,value);
            CloseHandle(token);
        } else printf("token error=%lu\n",GetLastError());
        CloseHandle(process);return 0;
    }
    const bool counters=argc==2 && strcmp(argv[1],"--scx-counters")==0;
    const bool features=argc==2 && strcmp(argv[1],"--sony-features")==0;
    const bool snapshot=argc==2 && strcmp(argv[1],"--sony-snapshot")==0;
    if(argc>1 && !counters && !features && !snapshot) return 2;
    setvbuf(stdout,nullptr,_IONBF,0);
    GUID hid; HidD_GetHidGuid(&hid);
    HDEVINFO set=SetupDiGetClassDevsW(&hid,nullptr,nullptr,DIGCF_PRESENT|DIGCF_DEVICEINTERFACE);
    if(set==INVALID_HANDLE_VALUE) return 1;
    for(DWORD n=0;;++n) {
        SP_DEVICE_INTERFACE_DATA iface{}; iface.cbSize=sizeof(iface);
        if(!SetupDiEnumDeviceInterfaces(set,nullptr,&hid,n,&iface)) break;
        DWORD length=0;
        SetupDiGetDeviceInterfaceDetailW(set,&iface,nullptr,0,&length,nullptr);
        if(length<sizeof(SP_DEVICE_INTERFACE_DETAIL_DATA_W)) continue;
        std::vector<BYTE> storage(length);
        auto* detail=reinterpret_cast<SP_DEVICE_INTERFACE_DETAIL_DATA_W*>(storage.data());
        detail->cbSize=sizeof(*detail);
        SP_DEVINFO_DATA dev{};dev.cbSize=sizeof(dev);
        if(!SetupDiGetDeviceInterfaceDetailW(set,&iface,detail,length,nullptr,&dev)) continue;
        HANDLE h=CreateFileW(detail->DevicePath,0,FILE_SHARE_READ|FILE_SHARE_WRITE,
            nullptr,OPEN_EXISTING,0,nullptr);
        if(h==INVALID_HANDLE_VALUE) continue;
        HIDD_ATTRIBUTES attr{}; attr.Size=sizeof(attr);
        PHIDP_PREPARSED_DATA data=nullptr; HIDP_CAPS caps{};
        if(!HidD_GetAttributes(h,&attr) || !HidD_GetPreparsedData(h,&data)) {CloseHandle(h);continue;}
        if(HidP_GetCaps(data,&caps)==HIDP_STATUS_SUCCESS &&
           ((caps.UsagePage==1 && (caps.Usage==4 || caps.Usage==5)) ||
            wcsstr(detail->DevicePath,L"scxvpad") || wcsstr(detail->DevicePath,L"SCXVPAD"))) {
            if(snapshot) {
                if(caps.UsagePage==1 && caps.Usage==5 && attr.VendorID==0x054c &&
                    (attr.ProductID==0x0ce6 || attr.ProductID==0x09cc || attr.ProductID==0x05c4) &&
                    (wcsstr(detail->DevicePath,L"scxvpad") || wcsstr(detail->DevicePath,L"hidclass")) &&
                    caps.InputReportByteLength==64) {
                    BYTE report[64]{};report[0]=1;
                    wprintf(L"SNAPSHOT path=%ls pid=%04x ",detail->DevicePath,attr.ProductID);
                    if(HidD_GetInputReport(h,report,sizeof(report))) {
                        printf("data=");for(auto value:report) printf("%02x",value);puts("");
                    } else printf("error=%lu\n",GetLastError());
                }
                HidD_FreePreparsedData(data);CloseHandle(h);continue;
            }
            if(features) {
                // Only our software-device paths, never a physical Sony pad.
                if(caps.UsagePage==1 && caps.Usage==5 && attr.VendorID==0x054c &&
                    (attr.ProductID==0x0ce6 || attr.ProductID==0x09cc || attr.ProductID==0x05c4) &&
                    (wcsstr(detail->DevicePath,L"scxvpad") || wcsstr(detail->DevicePath,L"hidclass"))) {
                    wprintf(L"FEATURES path=%ls vid=%04x pid=%04x\n",detail->DevicePath,attr.VendorID,attr.ProductID);
                    const BYTE ds5Ids[]={9,0x20,5,0x22};
                    const BYTE ds4Ids[]={0x12,0xa3,2};
                    const BYTE* ids=attr.ProductID==0x0ce6?ds5Ids:ds4Ids;
                    const size_t count=attr.ProductID==0x0ce6?sizeof(ds5Ids):sizeof(ds4Ids);
                    for(size_t index=0;index<count;++index) {
                        std::vector<BYTE> report(caps.FeatureReportByteLength);report[0]=ids[index];
                        if(HidD_GetFeature(h,report.data(),static_cast<ULONG>(report.size()))) {
                            printf(" feature=%02x bytes=%zu data=",ids[index],report.size());
                            for(auto value:report) printf("%02x",value);
                            puts("");
                        } else printf(" feature=%02x error=%lu\n",ids[index],GetLastError());
                    }
                }
                HidD_FreePreparsedData(data);CloseHandle(h);continue;
            }
            if(counters) {
                if(caps.UsagePage==1 && caps.Usage==5 && wcsstr(detail->DevicePath,L"scxvpad"))
                    ChannelCounters(dev.DevInst,detail->DevicePath);
                if(caps.UsagePage==0xff53 && caps.Usage==1 && caps.FeatureReportByteLength==97 &&
                    (wcsstr(detail->DevicePath,L"scxvpad") || wcsstr(detail->DevicePath,L"SCXVPAD"))) {
                    BYTE report[97]{};report[0]=0x79;
                    if(HidD_GetFeature(h,report,sizeof(report))) {
                        uint32_t values[8]{};memcpy(values,report+1,sizeof(values));
                        wprintf(L"COUNTERS path=%ls vid=%04x pid=%04x",detail->DevicePath,attr.VendorID,attr.ProductID);
                        for(auto value:values) printf(" %u",value);
                        puts("");
                    } else printf("COUNTERS error=%lu\n",GetLastError());
                }
                HidD_FreePreparsedData(data);CloseHandle(h);continue;
            }
            WCHAR product[128]{},maker[128]{},serial[128]{};
            HidD_GetProductString(h,product,sizeof(product));
            HidD_GetManufacturerString(h,maker,sizeof(maker));
            HidD_GetSerialNumberString(h,serial,sizeof(serial));
            wprintf(L"HID path=%ls\n vid=%04x pid=%04x revision=%04x usage=%04x:%04x bytes=%u/%u/%u nodes=%u\n product=%ls manufacturer=%ls serial=%ls\n",
                detail->DevicePath,attr.VendorID,attr.ProductID,attr.VersionNumber,caps.UsagePage,caps.Usage,
                caps.InputReportByteLength,caps.OutputReportByteLength,caps.FeatureReportByteLength,
                caps.NumberLinkCollectionNodes,product,maker,serial);
            HANDLE aclHandle=CreateFileW(detail->DevicePath,READ_CONTROL,FILE_SHARE_READ|FILE_SHARE_WRITE,nullptr,OPEN_EXISTING,0,nullptr);
            if(aclHandle!=INVALID_HANDLE_VALUE) {
                PSECURITY_DESCRIPTOR sd=nullptr;LPWSTR text=nullptr;
                const DWORD flags=OWNER_SECURITY_INFORMATION|GROUP_SECURITY_INFORMATION|DACL_SECURITY_INFORMATION;
                DWORD error=GetSecurityInfo(aclHandle,SE_FILE_OBJECT,flags,nullptr,nullptr,nullptr,nullptr,&sd);
                if(error==ERROR_SUCCESS && ConvertSecurityDescriptorToStringSecurityDescriptorW(sd,SDDL_REVISION_1,flags,&text,nullptr)) {
                    wprintf(L" ACL=%ls\n",text);LocalFree(text);
                } else printf(" ACL error=%lu\n",error);
                if(sd) LocalFree(sd);
                CloseHandle(aclHandle);
            } else printf(" ACL open-error=%lu\n",GetLastError());
            for(auto type:{HidP_Input,HidP_Output,HidP_Feature}) {
                USHORT count=type==HidP_Input?caps.NumberInputValueCaps:type==HidP_Output?caps.NumberOutputValueCaps:caps.NumberFeatureValueCaps;
                std::vector<HIDP_VALUE_CAPS> values(count);
                if(count && HidP_GetValueCaps(type,values.data(),&count,data)==HIDP_STATUS_SUCCESS) {
                    for(const auto& v:values)
                        printf(" field kind=%d id=%02x page=%04x usage=%04x size=%u count=%u logical=%ld..%ld\n",
                            type,v.ReportID,v.UsagePage,v.IsRange?v.Range.UsageMin:v.NotRange.Usage,
                            v.BitSize,v.ReportCount,v.LogicalMin,v.LogicalMax);
                }
            }
        }
        HidD_FreePreparsedData(data);CloseHandle(h);
    }
    SetupDiDestroyDeviceInfoList(set);
    if(counters || features || snapshot) return 0;
    IDirectInput8W* di=nullptr;
    if(SUCCEEDED(DirectInput8Create(GetModuleHandleW(nullptr),DIRECTINPUT_VERSION,IID_IDirectInput8W,reinterpret_cast<void**>(&di),nullptr))) {
        di->EnumDevices(DI8DEVCLASS_GAMECTRL,DiDevice,di,DIEDFL_ATTACHEDONLY);di->Release();
    }
    IGameInput* gi=nullptr;
    HRESULT hr=GameInputCreate(&gi);printf("GameInputCreate=%08lx\n",hr);
    if(SUCCEEDED(hr)) {
        GameInputCallbackToken token=0;
        hr=gi->RegisterDeviceCallback(nullptr,static_cast<GameInputKind>(0x1ffffff),GameInputDeviceConnected,
            GameInputBlockingEnumeration,nullptr,GiDevice,&token);
        printf("GameInputRegister=%08lx\n",hr);
        if(SUCCEEDED(hr)) {Sleep(1500);gi->UnregisterCallback(token,UINT64_MAX);}
        gi->Release();
    }
    return 0;
}
