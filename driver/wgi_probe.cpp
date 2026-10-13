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

struct Coverage {
    unsigned buttons=0, hats=0, low=0, high=0, samples=0, badRun=0, errors=0;
};

int main(int argc, char** argv) {
    using namespace winrt;
    using namespace Windows::Gaming::Input;
    init_apartment(apartment_type::multi_threaded);
    unsigned seconds = argc>1 ? static_cast<unsigned>(std::stoul(argv[1])) : 30;
    if(seconds>120) return 2;
    const bool verify=argc>2 && std::string(argv[2])=="--verify";
    std::map<unsigned,Coverage> coverage;
    std::map<std::wstring,std::string> last;
    const auto start=GetTickCount64();
    try {
        while(GetTickCount64()-start<seconds*1000ULL) {
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
    } catch(const hresult_error& e) { std::wcerr<<L"WGI ERROR "<<e.message().c_str()<<std::endl;return 1; }
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
