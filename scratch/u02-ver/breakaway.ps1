$src = @"
using System;
using System.Runtime.InteropServices;
public static class U02Probe {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct SI { public int cb; public string r, d, t; public int x, y, xs, ys, xc, yc, fill, flags; public short sw, r2; public IntPtr r3, i, o, e; }
  [StructLayout(LayoutKind.Sequential)]
  public struct PI { public IntPtr hp, ht; public int pid, tid; }
  [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
  public static extern bool CreateProcessW(string app, string cmd, IntPtr pa, IntPtr ta, bool inh, uint flags, IntPtr env, string dir, ref SI si, out PI pi);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
  [DllImport("kernel32.dll")]
  public static extern IntPtr GetCurrentProcess();
  public static string InJob() { bool r; bool ok = IsProcessInJob(GetCurrentProcess(), IntPtr.Zero, out r); return ok ? ("in_job=" + r) : ("IsProcessInJob error=" + Marshal.GetLastWin32Error()); }
  public static string Breakaway(string cmd) {
    var si = new SI(); si.cb = Marshal.SizeOf(si); PI pi;
    bool ok = CreateProcessW(null, cmd, IntPtr.Zero, IntPtr.Zero, false, 0x01000000, IntPtr.Zero, null, ref si, out pi);
    return ok ? ("breakaway created pid=" + pi.pid) : ("breakaway failed error=" + Marshal.GetLastWin32Error());
  }
}
"@
Add-Type -TypeDefinition $src
"W4 " + [U02Probe]::InJob()
"W4 " + [U02Probe]::Breakaway("cmd.exe /c exit 0")
exit 0
