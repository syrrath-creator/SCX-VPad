// Copyright (c) 2026 Tim Haller. See ../../LICENSE.
// Standalone UI for a hash-verified, versioned SCX VPad installation package.
// Uses .NET Framework included with Windows 11; no runtime download is needed.
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Drawing;
using System.Globalization;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Reflection;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using System.Web.Script.Serialization;
using System.Windows.Forms;
using Microsoft.Win32;

namespace ScxSetup {
internal static class Program {
    internal const string ProductKey = @"SOFTWARE\SCX\VirtualPad";
    internal static bool German = CultureInfo.CurrentUICulture.TwoLetterISOLanguageName == "de";
    internal static string Text(string de, string en) { return German ? de : en; }
    internal static bool Admin { get { using (var id = WindowsIdentity.GetCurrent()) return new WindowsPrincipal(id).IsInRole(WindowsBuiltInRole.Administrator); } }
    internal static bool ValidSid(string s) { return s != null && Regex.IsMatch(s, @"\AS-1-5-21-(\d+-){3}\d+\z"); }
    internal static bool ValidAction(string s) { return s == "install" || s == "replace" || s == "remove"; }
    internal static string Quote(string s) {
        // Windows CommandLineToArgvW/C runtime quoting, including terminal slashes.
        return "\"" + Regex.Replace(s, "(\\\\*)\"", "$1$1\\\"").ReplaceTailSlashes() + "\"";
    }
    private static string ReplaceTailSlashes(this string s) { return Regex.Replace(s, @"(\\+)\z", "$1$1"); }
    internal static RegistryKey Installed() { using (var root = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64)) return root.OpenSubKey(ProductKey); }
    internal static string DriverRoot { get { return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "SCX VPad"); } }
    internal static string PlatformError() {
        using (var k = Registry.LocalMachine.OpenSubKey(@"SYSTEM\CurrentControlSet\Control\Session Manager\Environment")) {
            if (!Environment.Is64BitProcess || !String.Equals(k.GetValue("PROCESSOR_ARCHITECTURE") as string, "AMD64", StringComparison.OrdinalIgnoreCase))
                return Text("Diese Ausgabe benötigt Windows 11 auf einem x64-PC. ARM64 wird nicht unterstützt.", "This release requires Windows 11 on an x64 PC. ARM64 is not supported.");
        }
        using (var k = Registry.LocalMachine.OpenSubKey(@"SOFTWARE\Microsoft\Windows NT\CurrentVersion")) {
            int build;
            if (!Int32.TryParse(k.GetValue("CurrentBuildNumber") as string, out build) || build < 22000)
                return Text("Windows 11 (Build 22000 oder neuer) wird benötigt.", "Windows 11 (build 22000 or newer) is required.");
        }
        return null;
    }
    [STAThread] private static void Main(string[] args) {
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        try {
            string action = null, owner;
            using (var id = WindowsIdentity.GetCurrent()) owner = id.User.Value;
            if (args.Length != 0) {
                if (args.Length != 5 || args[0] != "--elevated" || !ValidAction(args[1]) || !ValidSid(args[2]) || args[3] != "--language" || (args[4] != "de" && args[4] != "en") || !Admin)
                    throw new InvalidOperationException("Invalid setup arguments or missing administrator approval.");
                action = args[1]; owner = args[2]; German = args[4] == "de";
            }
            if (!ValidSid(owner)) throw new InvalidOperationException("Run Setup from the Windows account that will use SCX.");
            string error = PlatformError();
            if (error != null) throw new InvalidOperationException(error);
            Application.Run(new SetupForm(owner, action));
        } catch (Exception e) { MessageBox.Show(e.Message, "SCX VPad Setup", MessageBoxButtons.OK, MessageBoxIcon.Error); Environment.ExitCode = 1; }
    }
}

internal sealed class PackageFile { public string path { get; set; } public long bytes { get; set; } public string sha256 { get; set; } }
internal sealed class PackageManifest { public string product { get; set; } public string driverVersion { get; set; } public PackageFile[] files { get; set; } }
internal static class Payload {
    internal static byte[] Resource(string name) {
        using (var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream(name)) {
            if (stream == null) throw new InvalidDataException("Missing embedded resource: " + name);
            using (var buffer = new MemoryStream()) { stream.CopyTo(buffer); return buffer.ToArray(); }
        }
    }
    internal static string Hash(byte[] bytes) { using (var h = SHA256.Create()) return BitConverter.ToString(h.ComputeHash(bytes)).Replace("-", "").ToLowerInvariant(); }
    internal static string SafePath(string root, string relative) {
        if (String.IsNullOrEmpty(relative) || Path.IsPathRooted(relative) || !Regex.IsMatch(relative, @"\A[A-Za-z0-9][A-Za-z0-9_./\\ -]*\z")) throw new InvalidDataException("Invalid package path.");
        foreach (string part in relative.Split('/', '\\')) {
            if (part.Length == 0 || part == "." || part == ".." || part.EndsWith(".") || part.EndsWith(" ") || Regex.IsMatch(part, @"\A(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|\z)", RegexOptions.IgnoreCase)) throw new InvalidDataException("Ambiguous package path.");
        }
        string prefix = Path.GetFullPath(root).TrimEnd('\\') + "\\";
        string result = Path.GetFullPath(Path.Combine(prefix, relative));
        if (!result.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("Package path escaped staging.");
        return result;
    }
    internal static Dictionary<string, byte[]> ReadVerified() {
        byte[] zipped = Resource("SCX.Payload.zip");
        if (Hash(zipped) != BuildInfo.PayloadSha256) throw new InvalidDataException("Embedded package checksum mismatch.");
        var files = new Dictionary<string, byte[]>(StringComparer.OrdinalIgnoreCase);
        long total = 0;
        using (var ms = new MemoryStream(zipped)) using (var zip = new ZipArchive(ms, ZipArchiveMode.Read)) {
            foreach (var entry in zip.Entries) {
                string rel = entry.FullName.Replace('\\', '/');
                if (rel.EndsWith("/")) { SafePath(@"C:\SCX-Payload-Check", rel.TrimEnd('/')); continue; }
                SafePath(@"C:\SCX-Payload-Check", rel);
                if (files.ContainsKey(rel) || entry.Length > 32 * 1024 * 1024 || (total += entry.Length) > 64 * 1024 * 1024) throw new InvalidDataException("Invalid package size or duplicate member.");
                using (var source = entry.Open()) using (var buffer = new MemoryStream()) { source.CopyTo(buffer); files.Add(rel, buffer.ToArray()); }
            }
        }
        byte[] manifestBytes;
        if (!files.TryGetValue("package-manifest.json", out manifestBytes)) throw new InvalidDataException("Missing manifest.");
        var manifest = new JavaScriptSerializer().Deserialize<PackageManifest>(Encoding.UTF8.GetString(manifestBytes).TrimStart('\uFEFF'));
        if (manifest.product != "SCX-VPad" || manifest.driverVersion != BuildInfo.DriverVersion || manifest.files == null) throw new InvalidDataException("Unexpected package identity.");
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var member in manifest.files) {
            SafePath(@"C:\SCX-Payload-Check", member.path);
            string rel = member.path.Replace('\\', '/'); byte[] bytes;
            if (!seen.Add(rel) || !files.TryGetValue(rel, out bytes) || bytes.LongLength != member.bytes || !String.Equals(Hash(bytes), member.sha256, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("Package member verification failed: " + member.path);
        }
        if (files.Count != seen.Count + 1) throw new InvalidDataException("Unlisted files in package.");
        foreach (var required in new[] { "installer/Install.ps1", "installer/Uninstall.ps1", "installer/GameInput.ps1", "installer/Service.ps1", "driver/ScxVPad.dll", "bin/scx-vpad-service.exe", "LICENSE" })
            if (!seen.Contains(required)) throw new InvalidDataException("Incomplete package: " + required);
        return files;
    }
    internal static void AssertPlainTree(string path) {
        var dir = new DirectoryInfo(path);
        while (dir != null) { if ((dir.Attributes & FileAttributes.ReparsePoint) != 0) throw new IOException("Redirected setup directory is not supported."); dir = dir.Parent; }
    }
    internal static string Stage(Dictionary<string, byte[]> files) {
        // Create with the final ACL in a single operation under protected Program Files.
        string parent = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
        AssertPlainTree(parent);
        string stage = Path.Combine(parent, ".SCX-VPad-Setup-" + Guid.NewGuid().ToString("N"));
        var acl = new DirectorySecurity();
        acl.SetSecurityDescriptorSddlForm("O:BAG:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;GRGX;;;BU)");
        if (Directory.Exists(stage) || File.Exists(stage)) throw new IOException("Staging directory already exists.");
        Directory.CreateDirectory(stage, acl);
        AssertPlainTree(stage);
        foreach (var entry in files) {
            string target = SafePath(stage, entry.Key);
            Directory.CreateDirectory(Path.GetDirectoryName(target));
            using (var f = new FileStream(target, FileMode.CreateNew, FileAccess.Write, FileShare.None)) f.Write(entry.Value, 0, entry.Value.Length);
        }
        File.WriteAllBytes(SafePath(stage, "Desktop.ps1"), Resource("SCX.Desktop.ps1"));
        return stage;
    }
    internal static void Cleanup(string stage) {
        string parent = Path.GetFullPath(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles));
        if (!String.Equals(Path.GetDirectoryName(stage), parent, StringComparison.OrdinalIgnoreCase) || !Regex.IsMatch(Path.GetFileName(stage), @"\A\.SCX-VPad-Setup-[a-f0-9]{32}\z")) throw new IOException("Unexpected staging directory.");
        AssertPlainTree(stage);
        AssertChildrenPlain(stage);
        Directory.Delete(stage, true);
    }
    static void AssertChildrenPlain(string directory) {
        foreach (string child in Directory.EnumerateFileSystemEntries(directory)) {
            FileAttributes attributes = File.GetAttributes(child);
            if ((attributes & FileAttributes.ReparsePoint) != 0) throw new IOException("Redirected staging contents.");
            if ((attributes & FileAttributes.Directory) != 0) AssertChildrenPlain(child);
        }
    }
}

internal sealed class SetupForm : Form {
    readonly string owner, queuedAction;
    readonly Label status = new Label();
    readonly TextBox log = new TextBox();
    readonly Button install = new Button(), remove = new Button(), close = new Button(), copy = new Button();
    readonly CheckBox consent = new CheckBox();
    readonly ProgressBar progress = new ProgressBar();
    bool busy;
    readonly Color accent = Color.FromArgb(247, 157, 134);
    internal SetupForm(string ownerSid, string action) {
        SuspendLayout();
        owner = ownerSid; queuedAction = action;
        Text = "SCX VPad Setup"; ClientSize = new Size(760, 630); MinimumSize = new Size(650, 580);
        StartPosition = FormStartPosition.CenterScreen;
        Font = new Font("Segoe UI", 10); BackColor = Color.FromArgb(23, 27, 28); ForeColor = Color.WhiteSmoke;
        var layout = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 9, Padding = new Padding(24), AutoScroll = true };
        layout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
        for (int i = 0; i < 9; i++) layout.RowStyles.Add(i == 6 ? new RowStyle(SizeType.Percent, 100) : new RowStyle(SizeType.AutoSize));
        Controls.Add(layout);
        layout.Controls.Add(new Label { Text = "SCX VPad", AutoSize = true, Font = new Font("Segoe UI", 27, FontStyle.Bold), ForeColor = accent, Dock = DockStyle.Top }, 0, 0);
        layout.Controls.Add(new Label { Text = Program.Text("Treiber & Dienst · ", "Driver & service · ") + BuildInfo.DriverVersion + " · Windows 11 x64", UseMnemonic = false, AutoSize = true, Dock = DockStyle.Top, Margin = new Padding(3, 3, 3, 14) }, 0, 1);
        layout.Controls.Add(new Label { Dock = DockStyle.Top, AutoSize = true, Margin = new Padding(3, 0, 3, 14), Text = Program.Text(
            "Virtuelle Xbox 360-, DualShock 4-, DualSense- und generische HID-Geräte für SCX.\r\nDie Einrichtung benötigt einmalig Administratorrechte und erstellt ein lokales Signaturzertifikat. Normale Controller-Verbindungen brauchen keine Adminrechte.\r\nEntwicklungsvorschau, nicht WHQL-zertifiziert. Secure Boot und Speicherintegrität bleiben unverändert.\r\nBekannter Fehler: GameInput 4.0.302 erkennt beim späteren Spielstart mit mehreren virtuellen Controllern möglicherweise nur einen Teil. Details stehen in den Release Notes.",
            "Virtual Xbox 360, DualShock 4, DualSense and generic HID devices for SCX.\r\nSetup needs administrator approval once and creates a local signing certificate. Normal controller connections need no administrator rights.\r\nDevelopment preview, not WHQL certified. Secure Boot and Memory Integrity stay unchanged.\r\nKnown issue: when a game starts after several virtual controllers already exist, GameInput 4.0.302 may detect only some of them. See the release notes.") }, 0, 2);
        status.Dock = DockStyle.Top; status.AutoSize = true; status.Margin = new Padding(3, 0, 3, 12); status.ForeColor = Color.FromArgb(119, 218, 197); layout.Controls.Add(status, 0, 3);
        consent.Text = Program.Text("Ich stimme der lokalen Treiber-/Zertifikateinrichtung und der Lizenz zu. SCX und Spiele sind geschlossen.", "I agree to local driver/certificate setup and the license. SCX and games are closed.");
        consent.Dock = DockStyle.Top; consent.AutoSize = true; consent.Margin = new Padding(3, 0, 3, 12); consent.CheckedChanged += delegate { RefreshButtons(); }; layout.Controls.Add(consent, 0, 4);
        var license = new LinkLabel { Text = Program.Text("Lizenz und Hinweise anzeigen", "View license and notices"), AutoSize = true, Dock = DockStyle.Top, Margin = new Padding(3, 0, 3, 12), LinkColor = accent, ActiveLinkColor = Color.White };
        license.LinkClicked += delegate { ShowLicense(); }; layout.Controls.Add(license, 0, 5);
        log.Multiline = true; log.ReadOnly = true; log.ScrollBars = ScrollBars.Vertical; log.Dock = DockStyle.Fill; log.MinimumSize = new Size(0, 100);
        log.BackColor = Color.FromArgb(31, 36, 38); log.ForeColor = Color.Gainsboro; log.BorderStyle = BorderStyle.FixedSingle;
        log.Font = new Font("Consolas", 9); layout.Controls.Add(log, 0, 6);
        var buttons = new FlowLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, FlowDirection = FlowDirection.LeftToRight, Margin = new Padding(0, 12, 0, 8) };
        SetupButton(install, 190, accent); SetupButton(remove, 160, Color.FromArgb(47, 54, 57)); SetupButton(copy, 140, Color.FromArgb(47, 54, 57)); SetupButton(close, 110, Color.FromArgb(47, 54, 57));
        copy.Text = Program.Text("Protokoll kopieren", "Copy log"); close.Text = Program.Text("Schließen", "Close"); remove.Text = Program.Text("Deinstallieren", "Uninstall");
        buttons.Controls.AddRange(new Control[] { install, remove, copy, close }); layout.Controls.Add(buttons, 0, 7);
        progress.Dock = DockStyle.Top; progress.Height = 5; layout.Controls.Add(progress, 0, 8);
        install.Click += async delegate { using (var key = Program.Installed()) await Start(key == null ? "install" : "replace"); };
        remove.Click += async delegate { await Start("remove"); };
        close.Click += delegate { Close(); };
        copy.Click += delegate { if (log.Text.Length != 0) Clipboard.SetText(log.Text); };
        FormClosing += delegate(object sender, FormClosingEventArgs e) { if (busy) e.Cancel = true; };
        RefreshState();
        Shown += async delegate {
            if (queuedAction != null) { consent.Checked = true; await Execute(queuedAction); }
        };
        // Establish the designer baseline explicitly. Without it, Windows at
        // 200% DPI scales fonts but keeps constructor-created rows at 96-DPI sizes.
        AutoScaleDimensions = new SizeF(96, 96);
        AutoScaleMode = AutoScaleMode.Dpi;
        ResumeLayout(true);
    }
    void SetupButton(Button b, int width, Color color) { b.AutoSize = true; b.MinimumSize = new Size(width, 34); b.Padding = new Padding(8, 4, 8, 4); b.FlatStyle = FlatStyle.Flat; b.FlatAppearance.BorderSize = 0; b.BackColor = color; b.ForeColor = color == accent ? Color.FromArgb(30, 25, 24) : Color.White; }
    void RefreshButtons() { using (var key = Program.Installed()) { install.Enabled = !busy && consent.Checked; remove.Enabled = !busy && key != null && consent.Checked; } close.Enabled = !busy; consent.Enabled = !busy; }
    void RefreshState() {
        using (var key = Program.Installed()) {
            bool exists = key != null;
            install.Text = exists ? Program.Text("Erneut installieren", "Reinstall / update") : Program.Text("Installieren", "Install");
            status.Text = exists ? Program.Text("SCX VPad ist bereits eingerichtet.", "SCX VPad is already installed.") : Program.Text("Bereit zur Einrichtung. Kein Terminal erforderlich.", "Ready to install. No terminal required.");
        }
        RefreshButtons();
    }
    void Append(string line) {
        if (IsDisposed || line == null) return;
        if (InvokeRequired) { BeginInvoke(new Action<string>(Append), line); return; }
        if (log.TextLength > 120000) log.Text = log.Text.Substring(log.TextLength - 60000);
        log.AppendText(line + Environment.NewLine);
    }
    void ShowLicense() {
        var files = Payload.ReadVerified();
        using (var dlg = new Form { Text = "SCX VPad · License", Size = new Size(760, 620), StartPosition = FormStartPosition.CenterParent }) {
            var t = new TextBox { Multiline = true, ReadOnly = true, ScrollBars = ScrollBars.Vertical, Dock = DockStyle.Fill, Font = new Font("Consolas", 10), Text = Encoding.UTF8.GetString(files["LICENSE"]).Replace("\n", "\r\n") + "\r\n\r\n" + Encoding.UTF8.GetString(files["THIRD-PARTY-NOTICES.txt"]).Replace("\n", "\r\n") };
            dlg.Controls.Add(t); dlg.ShowDialog(this);
        }
    }
    async Task Start(string action) {
        if (!consent.Checked || busy) return;
        if (action != "install" && MessageBox.Show(this,
            Program.Text("Vorhandene virtuelle Verbindungen werden beendet. ", "Existing virtual connections will be stopped. ") +
            (action == "remove" ? Program.Text("SCX VPad jetzt entfernen?", "Remove SCX VPad now?") : Program.Text("Treiber und Dienst jetzt durch Version " + BuildInfo.DriverVersion + " ersetzen? Das berechtigte Windows-Konto bleibt erhalten.", "Replace the driver and service with version " + BuildInfo.DriverVersion + " now? The authorized Windows account is retained.")),
            Text, MessageBoxButtons.YesNo, MessageBoxIcon.Question, MessageBoxDefaultButton.Button2) != DialogResult.Yes) return;
        if (Program.Admin) { await Execute(action); return; }
        busy = true; RefreshButtons();
        try {
            var start = new ProcessStartInfo(Assembly.GetExecutingAssembly().Location, "--elevated " + action + " " + Program.Quote(owner) + " --language " + (Program.German ? "de" : "en")) { UseShellExecute = true, Verb = "runas", WorkingDirectory = Environment.GetFolderPath(Environment.SpecialFolder.Windows) };
            Process child = Process.Start(start);
            if (child == null) throw new IOException("Could not start elevated Setup.");
            Hide();
            await Task.Run(delegate { using (child) child.WaitForExit(); });
            busy = false; Close();
        } catch (Win32Exception e) {
            busy = false; Show(); RefreshButtons();
            status.Text = e.NativeErrorCode == 1223 ? Program.Text("Administratorfreigabe abgebrochen. Nichts geändert.", "Administrator approval cancelled. Nothing changed.") : e.Message;
        } catch (Exception e) { busy = false; Show(); RefreshButtons(); status.Text = e.Message; }
    }
    async Task Execute(string action) {
        busy = true; RefreshButtons(); progress.Style = ProgressBarStyle.Marquee;
        status.Text = Program.Text("Einrichtung läuft … bitte warten.", "Setup is running … please wait.");
        try {
            await Task.Run(delegate { RunOperation(action); });
            RefreshState();
            status.Text = action == "remove" ? Program.Text("SCX VPad wurde entfernt.", "SCX VPad has been removed.") : Program.Text("Fertig! Öffne SCX und verbinde deinen Controller.", "Done! Open SCX and connect your controller.");
            Append(Program.Text("Falls Windows einen Neustart verlangt, starte den PC vor dem Verbinden neu.", "If Windows requests a restart, restart the PC before connecting."));
        } catch (Exception e) {
            status.Text = Program.Text("Einrichtung nicht abgeschlossen. Details im Protokoll.", "Setup did not complete. See the log for details.");
            Append("ERROR: " + e.Message);
            Append(Program.Text("Bei einer Teilinstallation: Protokoll kopieren, danach mit dieser EXE deinstallieren und erneut installieren.", "For a partial installation: copy the log, then use this EXE to uninstall and install again."));
        } finally { busy = false; progress.Style = ProgressBarStyle.Blocks; RefreshButtons(); }
    }
    void RunOperation(string action) {
        if (!Program.Admin || !Program.ValidAction(action)) throw new InvalidOperationException("Administrator approval required.");
        using (var mutex = new Mutex(false, @"Global\SCX.VPad.Setup.Operation")) {
            bool locked = false;
            try {
                try { locked = mutex.WaitOne(0); } catch (AbandonedMutexException) { locked = true; }
                if (!locked) throw new IOException("Another SCX VPad setup is running.");
                foreach (string name in new[] { "SCX-Rust", "SCX", "scx-cli" }) {
                    var processes = Process.GetProcessesByName(name);
                    try { if (processes.Length != 0) throw new IOException(Program.Text("Bitte SCX vollständig schließen und erneut versuchen.", "Close SCX completely and try again.")); }
                    finally { foreach (var p in processes) p.Dispose(); }
                }
                using (var key = Program.Installed()) {
                    if ((action == "install") != (key == null)) throw new IOException("Installation state changed. Close Setup and open it again.");
                    if (key != null) {
                        string root = key.GetValue("InstallDirectory") as string;
                        if (root == null || !String.Equals(Path.GetFullPath(root).TrimEnd('\\'), Program.DriverRoot, StringComparison.OrdinalIgnoreCase)) throw new IOException("Unexpected installation directory.");
                        if (Directory.Exists(root)) Payload.AssertPlainTree(root);
                        else if (File.Exists(root)) throw new IOException("Installation directory is a file.");
                        string inf = Path.Combine(root, @"driver\scxvpad.inf");
                        if (action == "replace" && File.Exists(inf)) {
                            var version = Regex.Match(File.ReadAllText(inf), @"(?im)^DriverVer\s*=\s*[^,\r\n]+,\s*(\d+\.\d+\.\d+\.\d+)");
                            if (!version.Success || new Version(version.Groups[1].Value) > new Version(BuildInfo.DriverVersion)) throw new IOException("Refusing to downgrade a newer or unknown driver. Use its matching Setup.");
                        }
                    }
                }
                Append("Verifying embedded development-preview package (SHA-256) …");
                var files = Payload.ReadVerified();
                string stage = Payload.Stage(files);
                Append("Protected setup files: " + stage);
                bool success = false;
                try {
                    string shell = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Windows), @"System32\WindowsPowerShell\v1.0\powershell.exe");
                    var start = new ProcessStartInfo(shell, "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy RemoteSigned -File " + Program.Quote(Path.Combine(stage, "Desktop.ps1")) + " -Action " + action + " -OwnerSid " + Program.Quote(owner)) {
                        UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden, WorkingDirectory = stage,
                        RedirectStandardOutput = true, RedirectStandardError = true, StandardOutputEncoding = Encoding.UTF8, StandardErrorEncoding = Encoding.UTF8
                    };
                    using (var output = new StreamWriter(Path.Combine(stage, "setup.log"), false, new UTF8Encoding(false))) {
                        object gate = new object();
                        using (var process = new Process { StartInfo = start }) {
                            DataReceivedEventHandler receive = delegate(object sender, DataReceivedEventArgs e) { if (e.Data == null) return; lock (gate) { output.WriteLine(e.Data); output.Flush(); } Append(e.Data); };
                            process.OutputDataReceived += receive; process.ErrorDataReceived += receive;
                            if (!process.Start()) throw new IOException("Could not start installation engine.");
                            process.BeginOutputReadLine(); process.BeginErrorReadLine(); process.WaitForExit();
                            if (process.ExitCode != 0) throw new IOException("Installation engine returned " + process.ExitCode + ". Log: " + Path.Combine(stage, "setup.log"));
                            success = true;
                        }
                    }
                } finally {
                    if (success) Payload.Cleanup(stage);
                    else Append("Diagnostic files retained: " + stage);
                }
            } finally { if (locked) mutex.ReleaseMutex(); }
        }
    }
}
}
