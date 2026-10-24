using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Web.Script.Serialization;
namespace ScxSetup {
internal static class SetupTests {
    [DllImport("shell32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CommandLineToArgvW(string command, out int count);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr pointer);
    static int checks;
    static void Check(bool ok, string label) { if (!ok) throw new Exception("FAIL: " + label); checks++; Console.WriteLine("PASS: " + label); }
    static void RejectPackage(Dictionary<string, byte[]> original, string label, Action<Dictionary<string, byte[]>, Dictionary<string, object>> mutate) {
        var files = new Dictionary<string, byte[]>(original, StringComparer.OrdinalIgnoreCase);
        var serializer = new JavaScriptSerializer();
        var manifest = (Dictionary<string, object>)serializer.DeserializeObject(Encoding.UTF8.GetString(files["package-manifest.json"]).TrimStart('\uFEFF'));
        mutate(files, manifest);
        files["package-manifest.json"] = Encoding.UTF8.GetBytes(serializer.Serialize(manifest));
        byte[] payload;
        using (var memory = new MemoryStream()) {
            using (var archive = new ZipArchive(memory, ZipArchiveMode.Create, true)) {
                foreach (var file in files) using (var stream = archive.CreateEntry(file.Key).Open()) stream.Write(file.Value, 0, file.Value.Length);
            }
            payload = memory.ToArray();
        }
        bool rejected = false;
        try { Payload.ReadVerified(payload, Payload.Hash(payload), BuildInfo.DriverVersion, BuildInfo.SourceGuard, BuildInfo.Stable); }
        catch (InvalidDataException) { rejected = true; }
        Check(rejected, label);
    }
    static void RemoveMember(Dictionary<string, byte[]> files, Dictionary<string, object> manifest, string path) {
        files.Remove(path);
        manifest["files"] = ((object[])manifest["files"]).Where(member => (string)((Dictionary<string, object>)member)["path"] != path).ToArray();
    }
    [STAThread] static int Main() {
        try {
            var files = Payload.ReadVerified();
            Check(files.Count > 20, "embedded ZIP hash and every manifest member verified");
            Check(files.ContainsKey("installer/Install.ps1") && files.ContainsKey("installer/Uninstall.ps1"), "install and uninstall engines present");
            Check(new[] { "driver/ScxSource.dll", "driver/scxsource.inf", "driver/scxsource.cat" }.All(p => files.ContainsKey(p) == BuildInfo.SourceGuard), "source filter declaration agrees with verified payload");
            Check(Payload.Resource("SCX.Desktop.ps1").Length > 500, "fixed operation wrapper embedded");
            RejectPackage(files, "reject repacked wrong-platform payload", (f,m) => m["platform"] = "windows-arm64");
            RejectPackage(files, "reject repacked unsupported platform minimum", (f,m) => m["minimumBuild"] = 19041);
            RejectPackage(files, "reject repacked mismatched native revision", (f,m) => m["driverVersion"] = "0.1.0.0");
            RejectPackage(files, "reject repacked unknown release channel", (f,m) => m["releaseChannel"] = "other");
            RejectPackage(files, "reject repacked opposite release channel", (f,m) => m["releaseChannel"] = BuildInfo.Stable ? "preview" : "stable");
            RejectPackage(files, "reject omitted catalogue even with coherent manifest", (f,m) => RemoveMember(f,m,"driver/scxvpad.cat"));
            RejectPackage(files, "reject omitted validator even with coherent manifest", (f,m) => RemoveMember(f,m,"installer/Package.ps1"));
            RejectPackage(files, "reject unlisted archive entry", (f,m) => f.Add("unlisted.exe",new byte[] {1}));
            RejectPackage(files, "reject changed payload contents", (f,m) => f["LICENSE"] = new byte[] {1});
            RejectPackage(files, "reject ZIP traversal member", (f,m) => f.Add("../escape",new byte[] {1}));
            RejectPackage(files, "reject null manifest member", (f,m) => ((object[])m["files"])[0] = null);
            RejectPackage(files, "reject duplicate manifest member", (f,m) => m["files"] = ((object[])m["files"]).Concat(new[] {((object[])m["files"])[0]}).ToArray());
            if (BuildInfo.SourceGuard) RejectPackage(files, "reject missing source-filter member", (f,m) => RemoveMember(f,m,"driver/scxsource.cat"));
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
