// Independent Windows Gaming Input consumer. No driver SDK or controller hooks.
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Gaming.Input.h>
#include <windows.h>
#include <iostream>
#include <vector>
#include <memory>
#include <sstream>
#include <map>
#include <string>
#include <cmath>
#include <set>

static int VerifySonyMulti(unsigned seconds,unsigned expected) {
    using namespace winrt;
    using namespace Windows::Gaming::Input;
    if(expected<1 || expected>4) return 2;
    const auto start=GetTickCount64();
    unsigned consecutive=0;
    while(GetTickCount64()-start<seconds*1000ULL) {
        unsigned matches[4]{},sonyCount=0;
        std::set<std::wstring> ids;
        bool labels=true;
        for(auto gp:Gamepad::Gamepads()) {
            auto raw=RawGameController::FromGameController(gp);
            if(!raw || raw.HardwareVendorId()!=0x054c ||
               (raw.HardwareProductId()!=0x0ce6 && raw.HardwareProductId()!=0x09cc)) continue;
            ++sonyCount;
            const auto state=gp.GetCurrentReading();
            for(unsigned n=0;n<expected;++n) {
                const unsigned mask[]={4,8,16,32};
                if(static_cast<unsigned>(state.Buttons)==mask[n] &&
                   std::abs(state.LeftThumbstickX-((32.0+n*48.0)/127.5-1.0))<0.001 &&
                   std::abs(state.LeftTrigger-(n+1)*0.2)<0.001) {
                    ++matches[n]; ids.insert(std::wstring(raw.NonRoamableId()));
                    labels=labels && gp.GetButtonLabel(GamepadButtons::A)==GameControllerButtonLabel::Cross
                                  && gp.GetButtonLabel(GamepadButtons::B)==GameControllerButtonLabel::Circle;
                }
            }
        }
        bool good=sonyCount==expected && ids.size()==expected && labels;
        for(unsigned n=0;n<expected;++n) good=good && matches[n]==1;
        consecutive=good?consecutive+1:0;
        if(consecutive>=10) {
            std::cout<<"PASS WGI: "<<expected<<" distinct Sony controllers, independent button/axis/trigger markers and Sony labels"<<std::endl;
            return 0;
        }
        Sleep(20);
    }
    std::cerr<<"FAIL WGI: Sony devices merged, duplicated, mislabeled or failed to deliver distinct inputs"<<std::endl;
    return 1;
}

struct Coverage {
    unsigned buttons=0, hats=0, low=0, high=0, samples=0, badRun=0, errors=0;
};

static LRESULT CALLBACK DiagnosticWindow(HWND h, UINT m, WPARAM w, LPARAM l) {
    if(m==WM_DESTROY) { PostQuitMessage(0); return 0; }
    if(m==WM_PAINT) {
        PAINTSTRUCT p; HDC dc=BeginPaint(h,&p); RECT r; GetClientRect(h,&r);
        DrawTextW(dc,L"SCX: Windows-Rumble-Test\nDieses Fenster bitte kurz im Vordergrund lassen.\nNur der virtuelle Testcontroller wird angesprochen.",-1,&r,DT_CENTER|DT_WORDBREAK);
        EndPaint(h,&p); return 0;
    }
    return DefWindowProcW(h,m,w,l);
}

int main(int argc, char** argv) {
    using namespace winrt;
    using namespace Windows::Gaming::Input;
    init_apartment(apartment_type::multi_threaded);
    unsigned seconds = argc>1 ? static_cast<unsigned>(std::stoul(argv[1])) : 30;
    if(seconds>120) return 2;
    if(argc==4 && std::string(argv[2])=="--multi-sony") {
        try { return VerifySonyMulti(seconds,static_cast<unsigned>(std::stoul(argv[3]))); }
        catch(const hresult_error& e) { std::cerr<<"WGI multi error: "<<to_string(e.message())<<std::endl; return 1; }
    }
    const bool verify=argc>2 && std::string(argv[2])=="--verify";
    const bool inventory=argc>2 && std::string(argv[2])=="--inventory";
    const bool suite=argc>2 && std::string(argv[2])=="--vibrate-suite";
    const bool vibrate=suite || (argc>3 && std::string(argv[2])=="--vibrate");
    unsigned targetPid=suite?0x028e:(vibrate?static_cast<unsigned>(std::stoul(argv[3],nullptr,16)):0);
    unsigned suiteIndex=0;
    const unsigned suitePids[]={0x028e,0x09cc,0x0ce6};
    HWND window=nullptr;
    Gamepad vibrationPad{nullptr};
    unsigned vibrationStep=0;
    ULONGLONG vibrationStart=0;
    if(vibrate) {
        if(targetPid!=0x09cc && targetPid!=0x0ce6 && targetPid!=0x028e) return 2;
        WNDCLASSW c{}; c.lpfnWndProc=DiagnosticWindow; c.hInstance=GetModuleHandleW(nullptr); c.lpszClassName=L"ScxWgiProbe"; c.hbrBackground=(HBRUSH)(COLOR_WINDOW+1);
        RegisterClassW(&c);
        window=CreateWindowW(c.lpszClassName,L"SCX – Windows-Rumble-Test",WS_OVERLAPPEDWINDOW|WS_VISIBLE,CW_USEDEFAULT,CW_USEDEFAULT,600,180,nullptr,nullptr,c.hInstance,nullptr);
        if(!window) return 2;
        SetForegroundWindow(window);
    }
    std::map<unsigned,Coverage> coverage;
    std::map<std::wstring,std::string> last;
    const auto start=GetTickCount64();
    try {
        while(GetTickCount64()-start<seconds*1000ULL) {
            if(vibrate) {
                MSG message;
                while(PeekMessageW(&message,nullptr,0,0,PM_REMOVE)) {
                    if(message.message==WM_QUIT) { seconds=0; break; }
                    TranslateMessage(&message); DispatchMessageW(&message);
                }
                // One explicitly selected virtual fixture only; stop if ambiguous.
                if(!vibrationPad && Gamepad::Gamepads().Size()==(suite?3u:1u)) {
                  for(auto gp:Gamepad::Gamepads()) {
                    auto raw=RawGameController::FromGameController(gp);
                    if(raw && raw.HardwareProductId()==targetPid && (raw.HardwareVendorId()==0x054c || raw.HardwareVendorId()==0x045e)) {
                        vibrationPad=gp;
                        std::cout<<"WGI vibration target pid="<<std::hex<<targetPid<<std::dec<<std::endl;
                    }
                  }
                }
                if(vibrationPad && GetForegroundWindow()==window) {
                    if(vibrationStep<6 && (vibrationStart==0 || GetTickCount64()-vibrationStart>=1000)) {
                        const unsigned step=vibrationStep+1;
                        GamepadVibration v{};
                        if(step%2) {v.LeftMotor=0.25;v.RightMotor=0.5;}
                        vibrationPad.Vibration(v); vibrationStep=step; vibrationStart=GetTickCount64();
                        std::cout<<"WGI put_Vibration L="<<v.LeftMotor<<" R="<<v.RightMotor<<std::endl;
                    }
                }
                if(suite && vibrationStep==6 && GetTickCount64()-vibrationStart>=1000) {
                    ++suiteIndex;
                    if(suiteIndex==3) break;
                    vibrationPad=nullptr; vibrationStep=0; vibrationStart=0; targetPid=suitePids[suiteIndex];
                }
                Sleep(20); continue;
            }
            if(inventory) {
                std::cout<<"Gamepads="<<Gamepad::Gamepads().Size()<<" RawControllers="<<RawGameController::RawGameControllers().Size()<<std::endl;
                for(auto raw:RawGameController::RawGameControllers()) {
                    std::cout<<"RAW vid="<<std::hex<<raw.HardwareVendorId()<<" pid="<<raw.HardwareProductId()<<std::dec
                             <<" name="<<to_string(raw.DisplayName())<<" id="<<to_string(raw.NonRoamableId())
                             <<" force-feedback-motors="<<raw.ForceFeedbackMotors().Size()<<std::endl;
                }
                for(auto gp:Gamepad::Gamepads()) {
                    auto raw=RawGameController::FromGameController(gp);
                    const auto r=gp.GetCurrentReading();
                    std::cout<<"GP vid="<<std::hex<<raw.HardwareVendorId()<<" pid="<<raw.HardwareProductId()<<" buttons="<<static_cast<unsigned>(r.Buttons)<<std::dec
                             <<" name="<<to_string(raw.DisplayName())<<" labels(A,B,X,Y)="
                             <<static_cast<int>(gp.GetButtonLabel(GamepadButtons::A))<<","<<static_cast<int>(gp.GetButtonLabel(GamepadButtons::B))<<","<<static_cast<int>(gp.GetButtonLabel(GamepadButtons::X))<<","<<static_cast<int>(gp.GetButtonLabel(GamepadButtons::Y))
                             <<" timestamp="<<r.Timestamp<<std::endl;
                }
                Sleep(300);
                continue;
            }
            for(auto raw: RawGameController::RawGameControllers()) {
                if(raw.HardwareVendorId()!=0x054c) continue;
                const auto pid=raw.HardwareProductId();
                if(pid!=0x09cc && pid!=0x0ce6) continue;
                const auto count=raw.ButtonCount();
                auto buttons=std::make_unique<bool[]>(count);
                std::vector<double> axes(raw.AxisCount());
                std::vector<GameControllerSwitchPosition> hats(raw.SwitchCount());
                raw.GetCurrentReading(array_view<bool>(buttons.get(),buttons.get()+count),hats,axes);
                std::ostringstream ss;
                ss<<"pid="<<std::hex<<pid<<std::dec<<" raw-buttons=";
                for(int i=0;i<count;++i) if(buttons[i]) ss<<i<<",";
                ss<<" raw-axes=";
                for(double v:axes) ss<<v<<",";
                ss<<" hats=";
                for(auto h:hats) ss<<static_cast<unsigned>(h)<<",";
                auto gp=Gamepad::FromGameController(raw);
                bool good=false;
                if(gp) {
                    const auto r=gp.GetCurrentReading();
                    ss<<" GAMEPAD buttons="<<std::hex<<static_cast<unsigned>(r.Buttons)<<std::dec
                      <<" LS="<<r.LeftThumbstickX<<","<<r.LeftThumbstickY
                      <<" RS="<<r.RightThumbstickX<<","<<r.RightThumbstickY
                      <<" LT="<<r.LeftTrigger<<" RT="<<r.RightTrigger;
                    if(axes.size()==6 && count>=14 && hats.size()==1) {
                        // Independent semantic oracle for Sony HID controls.
                        const unsigned mask[]={0x10,0x4,0x8,0x20,0x400,0x800,0,0,0x2,0x1,0x1000,0x2000,0,0,0};
                        unsigned expected=0;
                        for(int i=0;i<count && i<15;++i) if(buttons[i]) expected|=mask[i];
                        const unsigned dpad[]={0,0x40,0x240,0x200,0x280,0x80,0x180,0x100,0x140};
                        const auto hat=static_cast<unsigned>(hats[0]);
                        if(hat<9) expected|=dpad[hat];
                        const auto approximately=[](double a,double b) {return std::abs(a-b)<0.0001;};
                        good=static_cast<unsigned>(r.Buttons)==expected
                            && approximately(r.LeftThumbstickX,axes[0]*2-1) && approximately(r.LeftThumbstickY,1-axes[1]*2)
                            && approximately(r.RightThumbstickX,axes[2]*2-1) && approximately(r.RightThumbstickY,1-axes[5]*2)
                            && approximately(r.LeftTrigger,axes[3]) && approximately(r.RightTrigger,axes[4]);
                        if(good) {
                            auto& c=coverage[pid];
                            for(int i=0;i<count && i<14;++i) if(buttons[i]) c.buttons|=1u<<i;
                            if(hat<9) c.hats|=1u<<hat;
                            for(unsigned i=0;i<6;++i) {
                                if(axes[i]<0.01) c.low|=1u<<i;
                                if(axes[i]>0.99) c.high|=1u<<i;
                            }
                        }
                    }
                } else ss<<" NO_GAMEPAD_MAPPING";
                auto& c=coverage[pid];
                ++c.samples;
                if(good) c.badRun=0;
                else if(++c.badRun==10) { ++c.errors; ss<<" PERSISTENT_MISMATCH"; }
                const auto id=std::wstring(raw.NonRoamableId());
                if(last[id]!=ss.str()) { last[id]=ss.str();std::cout<<(GetTickCount64()-start)<<"ms "<<ss.str()<<std::endl; }
            }
            Sleep(20);
        }
    } catch(const hresult_error& e) {
        if(vibrationPad) {try { vibrationPad.Vibration(GamepadVibration{}); } catch(...) {}}
        if(window) DestroyWindow(window);
        std::wcerr<<L"WGI ERROR "<<e.message().c_str()<<std::endl;return 1;
    }
    if(vibrationPad) vibrationPad.Vibration(GamepadVibration{});
    if(window && IsWindow(window)) DestroyWindow(window);
    if(vibrate) return (suite?suiteIndex==3:vibrationStep==6)?0:4;
    std::cout<<"WGI probe finished; distinct devices="<<last.size()<<std::endl;
    if(verify) {
        bool pass=true;
        for(unsigned pid:{0x09ccu,0x0ce6u}) {
            const auto& c=coverage[pid];
            const bool complete=c.samples>0 && c.errors==0 && c.buttons==0x3fff && c.hats==0x1ff && c.low==0x3f && c.high==0x3f;
            std::cout<<"VERIFY pid="<<std::hex<<pid<<" buttons="<<c.buttons<<" hats="<<c.hats<<" low="<<c.low<<" high="<<c.high<<std::dec<<" errors="<<c.errors<<" samples="<<c.samples<<" "<<(complete?"PASS":"FAIL")<<std::endl;
            pass=pass && complete;
        }
        return pass?0:3;
    }
}
