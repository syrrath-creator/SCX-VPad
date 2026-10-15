using System;
using System.Linq;
using System.Runtime.InteropServices;
namespace ScxSetup {
internal static class SetupTests {
    [DllImport("shell32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CommandLineToArgvW(string command, out int count);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr pointer);
    static int checks;
    static void Check(bool ok, string label) { if (!ok) throw new Exception("FAIL: " + label); checks++; Console.WriteLine("PASS: " + label); }
    [STAThread] static int Main() {
        try {
            var files = Payload.ReadVerified();
            Check(files.Count > 20, "embedded ZIP hash and every manifest member verified");
            Check(files.ContainsKey("installer/Install.ps1") && files.ContainsKey("installer/Uninstall.ps1"), "install and uninstall engines present");
            Check(Payload.Resource("SCX.Desktop.ps1").Length > 500, "fixed operation wrapper embedded");
            foreach (string input in new[] { "plain", "C:\\Program Files\\SCX VPad\\", "a b", "a\"b", "a\\\"b", "", "x & $() ; ' y", "C:\\测试\\日本語\\" }) {
                int count; IntPtr p = CommandLineToArgvW("setup.exe " + Program.Quote(input), out count);
                try { Check(count == 2 && Marshal.PtrToStringUni(Marshal.ReadIntPtr(p, IntPtr.Size)) == input, "Windows argument quoting: " + input); }
                finally { LocalFree(p); }
            }
            foreach (string bad in new[] { "../escape", "a/../../b", "C:\\escape", "\\\\server\\file", "/absolute", "a//b", "a/./b", "a./file", "a /file", "file:stream", "CON.txt", "x/NUL", "x/LPT1.exe" }) {
                bool rejected = false; try { Payload.SafePath(@"C:\SCX-Test", bad); } catch (System.IO.InvalidDataException) { rejected = true; }
                Check(rejected, "reject unsafe package path: " + bad);
            }
            Check(Payload.SafePath(@"C:\SCX-Test", "installer/Install.ps1") == @"C:\SCX-Test\installer\Install.ps1", "valid extraction path");
            foreach (string bad in new[] { "S-1-5-18", "S-1-5-32-544", "S-1-5-21-1-2-3-4\n", "S-1-5-21-1-2-3-4;calc" }) Check(!Program.ValidSid(bad), "reject invalid owner SID");
            Check(Program.ValidSid("S-1-5-21-1-2-3-1001"), "accept interactive owner SID");
            Check(new[] {"install","replace","remove"}.All(Program.ValidAction) && !Program.ValidAction("command") && !Program.ValidAction("install -x"), "operation allowlist");
            Check(Program.PlatformError() == null, "current Windows platform supported");
            Console.WriteLine("Setup tests passed: " + checks + ". No driver/service/trust changes performed.");
            return 0;
        } catch (Exception e) { Console.Error.WriteLine(e); return 1; }
    }
    }
}
