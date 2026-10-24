# Use the Service Control Manager API rather than native command-line quoting or
# direct registry edits: SCM caches its configuration and owns that database.
if (-not ('Scx.ServiceConfiguration' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
namespace Scx {
    public static class ServiceConfiguration {
        [StructLayout(LayoutKind.Sequential)]
        private struct Config {
            public uint Type, Start, Error;
            public IntPtr Image, Group;
            public uint Tag;
            public IntPtr Dependencies, Account, Display;
        }
        [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        private static extern IntPtr OpenSCManagerW(string machine, string database, uint access);
        [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        private static extern IntPtr OpenServiceW(IntPtr scm, string name, uint access);
        [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        private static extern IntPtr CreateServiceW(IntPtr scm, string name, string display, uint access, uint type, uint start, uint error, string image, string group, IntPtr tag, string dependencies, string account, string password);
        [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool ChangeServiceConfigW(IntPtr service, uint type, uint start, uint error, string image, string group, IntPtr tag, string dependencies, string account, string password, string display);
        [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool QueryServiceConfigW(IntPtr service, IntPtr buffer, uint size, out uint needed);
        [DllImport("advapi32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CloseServiceHandle(IntPtr handle);
        private static IntPtr Checked(IntPtr handle) {
            if (handle == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
            return handle;
        }
        private static string Read(IntPtr service) {
            uint needed;
            if (QueryServiceConfigW(service, IntPtr.Zero, 0, out needed) || Marshal.GetLastWin32Error() != 122 || needed == 0 || needed > 65536)
                throw new Win32Exception(Marshal.GetLastWin32Error(), "Cannot size SCM configuration.");
            IntPtr buffer = Marshal.AllocHGlobal((int)needed);
            try {
                if (!QueryServiceConfigW(service, buffer, needed, out needed)) throw new Win32Exception(Marshal.GetLastWin32Error());
                Config config = (Config)Marshal.PtrToStructure(buffer, typeof(Config));
                return Marshal.PtrToStringUni(config.Image);
            } finally { Marshal.FreeHGlobal(buffer); }
        }
        public static string ReadImage(string name) {
            IntPtr scm = Checked(OpenSCManagerW(null, null, 1));
            try {
                IntPtr service = Checked(OpenServiceW(scm, name, 1));
                try { return Read(service); } finally { CloseServiceHandle(service); }
            } finally { CloseServiceHandle(scm); }
        }
        public static void Create(string name, string image) {
            IntPtr scm = Checked(OpenSCManagerW(null, null, 3));
            try {
                // SERVICE_WIN32_OWN_PROCESS, demand start, normal error control,
                // LocalSystem. Existing service names are never overwritten.
                IntPtr service = Checked(CreateServiceW(scm, name, "SCX VPad Device Manager", 1, 16, 3, 1, image, null, IntPtr.Zero, null, null, null));
                try {
                    if (!String.Equals(Read(service), image, StringComparison.Ordinal)) throw new InvalidOperationException("SCM image verification failed.");
                } finally { CloseServiceHandle(service); }
            } finally { CloseServiceHandle(scm); }
        }
        public static void ChangeImage(string name, string image) {
            IntPtr scm = Checked(OpenSCManagerW(null, null, 1));
            try {
                IntPtr service = Checked(OpenServiceW(scm, name, 3));
                try {
                    if (!ChangeServiceConfigW(service, UInt32.MaxValue, UInt32.MaxValue, UInt32.MaxValue, image, null, IntPtr.Zero, null, null, null, null)) throw new Win32Exception(Marshal.GetLastWin32Error());
                    if (!String.Equals(Read(service), image, StringComparison.Ordinal)) throw new InvalidOperationException("SCM image verification failed.");
                } finally { CloseServiceHandle(service); }
            } finally { CloseServiceHandle(scm); }
        }
    }
}
'@
}
function Get-ScxServiceCommandLine([string]$ExecutablePath) {
    if ([string]::IsNullOrWhiteSpace($ExecutablePath) -or $ExecutablePath.IndexOfAny([char[]]@('"', "`r", "`n", "`0")) -ge 0 -or $ExecutablePath -notmatch '^[A-Za-z]:[\\/]' -or $ExecutablePath.Substring(2).Contains(':') -or [IO.Path]::GetExtension($ExecutablePath) -ine '.exe') {
        throw 'Expected one absolute executable path without command-line arguments.'
    }
    '"' + [IO.Path]::GetFullPath($ExecutablePath) + '"'
}
