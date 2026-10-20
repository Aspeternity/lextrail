<#
.SYNOPSIS
Logs every keyboard event the system processes, for investigating stuck modifiers.

.DESCRIPTION
Installs a low-level keyboard hook in its own process and writes one line per event to
-Log: the event's time (KBDLLHOOKSTRUCT.time, GetTickCount clock, ms), a high-resolution
time (ms since the observer started), the virtual key, down/up, the injected and
lower-integrity-injected flags, the scan code and dwExtraInfo (LexTrail's own injected
keys carry 0x4C584D4B). It only reads events and passes them on unchanged; it sends no
keys. stuck-keys.ps1 -Observe starts and stops it.

It records every key typed on the machine, so it never outlives its test: it exits as
soon as process -ParentPid is gone, and after -MaxMinutes in any case. Delete the log
once the needed lines are taken out of it; never commit it.

An extra low-level hook adds a little latency to every key event while it runs; compare
runs made with it on, not with older runs made without it.

.EXAMPLE
powershell -NoProfile -File tools/e2e/key-observer.ps1 -Log $env:TEMP\keys.log -ParentPid $PID
#>
param(
  [Parameter(Mandatory)][string]$Log,
  [Parameter(Mandatory)][int]$ParentPid,
  [int]$MaxMinutes = 15
)
$ErrorActionPreference = "Stop"

Add-Type @"
using System; using System.IO; using System.Diagnostics; using System.Threading;
using System.Collections.Concurrent; using System.Runtime.InteropServices;
public static class LtObserver {
  [StructLayout(LayoutKind.Sequential)] struct KB { public uint vk, scan, flags, time; public UIntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr h; public uint m; public UIntPtr w; public IntPtr l; public uint t; public int x, y; }
  delegate IntPtr Proc(int code, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern IntPtr SetWindowsHookEx(int id, Proc p, IntPtr mod, uint tid);
  [DllImport("user32.dll")] static extern IntPtr CallNextHookEx(IntPtr h, int code, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern int GetMessage(out MSG m, IntPtr h, uint a, uint b);
  [DllImport("kernel32.dll")] static extern IntPtr GetModuleHandle(string name);
  static Proc keep; static IntPtr hook;
  static readonly ConcurrentQueue<string> queue = new ConcurrentQueue<string>();
  static readonly Stopwatch clock = Stopwatch.StartNew();
  static IntPtr OnKey(int code, IntPtr w, IntPtr l) {
    if (code == 0) {
      var e = (KB)Marshal.PtrToStructure(l, typeof(KB));
      // Formatting is cheap; the file is written on another thread.
      queue.Enqueue(String.Format("t={0} hr={1:F2} vk={2:X2}{3} inj={4} lowil={5} scan={6:X2} extra=0x{7:X}",
        e.time, clock.Elapsed.TotalMilliseconds, e.vk, (e.flags & 0x80) != 0 ? "^" : "v",
        (e.flags & 0x10) != 0 ? 1 : 0, (e.flags & 0x02) != 0 ? 1 : 0, e.scan, e.extra.ToUInt64()));
    }
    return CallNextHookEx(hook, code, w, l);
  }
  // Ends the process when the parent is gone or the time is up, whatever else happens.
  static void Watchdog(int parentPid, int maxMinutes) {
    Process parent = null;
    try { parent = Process.GetProcessById(parentPid); } catch (ArgumentException) { }
    var deadline = DateTime.UtcNow.AddMinutes(maxMinutes);
    while (parent != null && !parent.HasExited && DateTime.UtcNow < deadline) Thread.Sleep(500);
    Environment.Exit(0);
  }
  public static void Run(string path, int parentPid, int maxMinutes) {
    var watchdog = new Thread(() => Watchdog(parentPid, maxMinutes));
    watchdog.IsBackground = true; watchdog.Start();
    var writer = new StreamWriter(path, true); writer.AutoFlush = true;
    writer.WriteLine("observer started tick=" + Environment.TickCount);
    var flusher = new Thread(() => { while (true) { string s; while (queue.TryDequeue(out s)) writer.WriteLine(s); Thread.Sleep(100); } });
    flusher.IsBackground = true; flusher.Start();
    keep = OnKey;
    hook = SetWindowsHookEx(13, keep, GetModuleHandle(null), 0);
    if (hook == IntPtr.Zero) throw new Exception("SetWindowsHookEx failed: " + Marshal.GetLastWin32Error());
    MSG m; while (GetMessage(out m, IntPtr.Zero, 0, 0) > 0) { }
  }
}
"@
[LtObserver]::Run($Log, $ParentPid, $MaxMinutes)
