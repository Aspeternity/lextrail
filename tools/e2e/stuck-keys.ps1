<#
.SYNOPSIS
Checks that the hotkey's Alt mask never leaves a modifier logically held down.

.DESCRIPTION
The Alt mask sends one more key-down of a held Ctrl (or Shift) and no key-up (D014,
R3-P1). If the user releases that key just before the repeat arrives, the repeat would
leave it logically down; a low-level hook detects that and sends the key-up.

This script starts the given LexTrail build and a small window of its own (a text box;
Windows 11 Notepad is avoided because it reuses the user's open Notepad process), then
presses Ctrl+Alt+X with
SendInput many times, varying the time between the hotkey's key-down and the releases
(0 ms upwards, so the release races the repeat). After each press it checks with
GetAsyncKeyState that no Ctrl, Alt or Shift key is still down, closes the popup with
Esc, and goes on. It also prints the app's "alt mask" log lines (debug builds).

It sends keys: leave the machine alone while it runs. The hotkey is only sent while the
test window is in the foreground, and Esc only while the popup (or the test window) is;
otherwise the script stops, so no key lands in another program. Exits with 1 if a key was
stuck.

-Observe also runs key-observer.ps1 (every key event with its injected flag and time) and,
whenever a key is stuck, prints that press's events and the modifiers' GetAsyncKeyState
and GetKeyState (as seen by this script's thread, and with its input attached to the test
window's thread and to the foreground window's thread).

-Hotkey sets another lookup hotkey (scratch settings) and presses that instead, e.g.
"Shift+Alt+X", "Alt+X", "Ctrl+Shift+F1" or "F8" (docs/specs/m1-hotkey-settings.md 8.2).
.EXAMPLE
pwsh tools/e2e/stuck-keys.ps1 -App target\debug\lextrail-desktop.exe
pwsh tools/e2e/stuck-keys.ps1 -App target\debug\lextrail-desktop.exe -Hotkey "Shift+Alt+X"
#>
param(
  [string]$App = "target\debug\lextrail-desktop.exe",
  [int[]]$GapsMs = @(0, 0, 1, 1, 2, 3, 5, 8, 13, 20, 40, 80),
  [int]$Rounds = 2,
  [switch]$Observe,
  [string]$OutDir = (Join-Path $env:TEMP "lextrail-e2e"),
  [string]$Hotkey = "Ctrl+Alt+X"
)
$ErrorActionPreference = "Stop"
$App = (Resolve-Path $App).Path
. (Join-Path $PSScriptRoot "hotkey-keys.ps1")
$hotkeyVks = Get-HotkeyVks $Hotkey
New-Item -ItemType Directory -Force $OutDir | Out-Null

Add-Type @"
using System; using System.Runtime.InteropServices; using System.Threading;
public static class LtKeys {
  [StructLayout(LayoutKind.Sequential)] struct KI { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Explicit, Size = 40)] struct IN { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KI ki; }
  [DllImport("user32.dll")] static extern uint SendInput(uint n, IN[] i, int size);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
  [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vk);
  [DllImport("user32.dll")] static extern short GetKeyState(int vk);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  // The clock of KBDLLHOOKSTRUCT.time (Environment.TickCount turns negative after 24.8 days).
  [DllImport("kernel32.dll")] public static extern uint GetTickCount();
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool attach);
  // Ctrl, Alt and their left/right keys: "vk=async/state" in hex (high bit = down).
  static string States() {
    string s = "";
    int[] vks = { 0x11, 0xA2, 0xA3, 0x12, 0xA4, 0xA5 };
    foreach (int vk in vks) s += String.Format("{0:X2}={1:X4}/{2:X4} ", vk, (ushort)GetAsyncKeyState(vk), (ushort)GetKeyState(vk));
    return s.Trim();
  }
  // Key states from this thread, then with this thread's input attached to each window's.
  public static string KeyStates(IntPtr testWindow) {
    string s = "own: " + States();
    var fg = GetForegroundWindow();
    if (fg == testWindow) s += "; foreground is the test window";
    foreach (var w in fg == testWindow ? new[] { testWindow } : new[] { testWindow, fg }) {
      uint tid = GetWindowThreadProcessId(w, IntPtr.Zero);
      string name = w == testWindow ? "test window" : "foreground " + w;
      if (tid == 0 || tid == GetCurrentThreadId()) continue;
      if (!AttachThreadInput(GetCurrentThreadId(), tid, true)) { s += "; " + name + ": attach failed"; continue; }
      s += "; " + name + ": " + States();
      AttachThreadInput(GetCurrentThreadId(), tid, false);
    }
    return s;
  }
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Ri, B; }
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e);
  [StructLayout(LayoutKind.Sequential)] public struct Pt { public int X, Y; }
  [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Pt p);
  [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint flags);
  // Clicks the middle of the window, only if that point shows this window (not one on top).
  public static bool ClickCenter(IntPtr h) {
    R r; GetWindowRect(h, out r);
    var p = new Pt { X = (r.L + r.Ri) / 2, Y = (r.T + r.B) / 2 };
    if (GetAncestor(WindowFromPoint(p), 2) != h) return false;
    SetCursorPos(p.X, p.Y); mouse_event(2, 0, 0, 0, IntPtr.Zero); mouse_event(4, 0, 0, 0, IntPtr.Zero);
    return true;
  }
  static void Key(ushort vk, bool up) {
    var i = new IN[1];
    i[0].type = 1; i[0].ki.vk = vk; i[0].ki.scan = (ushort)MapVirtualKey(vk, 0);
    i[0].ki.flags = up ? 2u : 0u;
    SendInput(1, i, Marshal.SizeOf(typeof(IN)));
  }
  // The hotkey's keys down (modifiers 15 ms apart, main key last); after gapMs, all up
  // in reverse order.
  public static void Hotkey(int[] vks, int gapMs) {
    for (int i = 0; i < vks.Length; i++) { Key((ushort)vks[i], false); if (i < vks.Length - 1) Thread.Sleep(15); }
    if (gapMs > 0) Thread.Sleep(gapMs);
    for (int i = vks.Length - 1; i >= 0; i--) Key((ushort)vks[i], true);
  }
  public static void Esc() { Key(0x1B, false); Key(0x1B, true); }
  public static void Up(int vk) { Key((ushort)vk, true); }
  // Names of the modifiers that are logically down.
  public static string Held() {
    string s = "";
    int[] vks = { 0x10, 0x11, 0x12, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5 };
    foreach (int vk in vks) if (GetAsyncKeyState(vk) < 0) s += vk.ToString("X") + " ";
    return s.Trim();
  }
}
"@
# Real pixel coordinates for ClickCenter on scaled displays.
[void][LtKeys]::SetProcessDPIAware()

Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
$log = Join-Path $OutDir "stuck-keys-app.log"
# Scratch settings and history, never the user's own (debug builds only).
$env:LEXTRAIL_DEBUG_CONFIG_DIR = Join-Path $OutDir ("config-" + [guid]::NewGuid())
Write-HotkeySettings $env:LEXTRAIL_DEBUG_CONFIG_DIR $Hotkey
$env:LEXTRAIL_DEBUG_DATA_DIR = Join-Path $OutDir ("data-" + [guid]::NewGuid())
Start-Process $App -RedirectStandardError $log -WindowStyle Hidden
Remove-Item Env:LEXTRAIL_DEBUG_CONFIG_DIR, Env:LEXTRAIL_DEBUG_DATA_DIR -ErrorAction SilentlyContinue
Start-Sleep 4
# A hotkey another program holds did not register: pressing it would reach that program.
if (Select-String -Path $log -Pattern "did not register" -SimpleMatch -Quiet) {
  Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
  throw "the hotkey did not register (taken by another program?); not sending keys. Log: $log"
}
$form = @'
Add-Type -AssemblyName System.Windows.Forms
$f = New-Object System.Windows.Forms.Form
$f.Text = "lextrail-stuck-keys"
$t = New-Object System.Windows.Forms.TextBox
$t.Multiline = $true; $t.Dock = "Fill"; $t.Text = "The quick brown fox jumps over the lazy dog."
$f.Controls.Add($t)
[System.Windows.Forms.Application]::Run($f)
'@
$encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($form))
$target = Start-Process powershell -ArgumentList "-NoProfile", "-STA", "-EncodedCommand", $encoded -PassThru
for ($i = 0; $i -lt 50 -and $target.MainWindowHandle -eq 0; $i++) { Start-Sleep -Milliseconds 200; $target.Refresh() }
$ws = New-Object -ComObject WScript.Shell

$stuck = 0; $n = 0
try {
  # Inside the try, so that the finally stops it whatever happens.
  if ($Observe) {
    $keysLog = Join-Path $OutDir "stuck-keys-events.log"
    Remove-Item $keysLog -ErrorAction SilentlyContinue
    $observer = Start-Process powershell -ArgumentList "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $PSScriptRoot "key-observer.ps1"), "-Log", $keysLog, "-ParentPid", $PID -WindowStyle Hidden -PassThru
  }
  if ($target.MainWindowHandle -eq 0) { throw "test window not found" }
  for ($round = 1; $round -le $Rounds; $round++) {
    foreach ($gap in $GapsMs) {
      $n++
      [void]$ws.AppActivate($target.Id); Start-Sleep -Milliseconds 300
      $activated = "AppActivate"
      if ([LtKeys]::GetForegroundWindow() -ne $target.MainWindowHandle) {
        $activated = "SetForegroundWindow"
        [void][LtKeys]::SetForegroundWindow($target.MainWindowHandle); Start-Sleep -Milliseconds 300
      }
      # Refused while another program is in front: click the test window itself.
      if ([LtKeys]::GetForegroundWindow() -ne $target.MainWindowHandle) {
        $activated = "click"
        [void][LtKeys]::ClickCenter($target.MainWindowHandle); Start-Sleep -Milliseconds 300
      }
      $fg = [LtKeys]::GetForegroundWindow()
      if ($fg -ne $target.MainWindowHandle) { throw "test window is not in the foreground ($fg); not sending keys" }
      $lines = @(Get-Content $log).Count
      $sentAt = [int64][LtKeys]::GetTickCount()
      [LtKeys]::Hotkey([int[]]$hotkeyVks, $gap); Start-Sleep -Milliseconds 700
      $held = [LtKeys]::Held()
      if ($held) { $stuck++ }
      "press ${n} (gap ${gap} ms, tick $sentAt, $activated): " + $(if ($held) { "STUCK: $held" } else { "ok" })
      if ($held -and $Observe) {
        "    key states: " + [LtKeys]::KeyStates($target.MainWindowHandle)
        # This press's events (the observer writes every 100 ms).
        Start-Sleep -Milliseconds 200
        Get-Content $keysLog | Where-Object { $_ -match '^t=(\d+)' -and [int64]$Matches[1] -ge $sentAt - 5 -and [int64]$Matches[1] -le $sentAt + 700 } |
          ForEach-Object { "    event: $_" }
      }
      # This press's mask decision (debug builds).
      Get-Content $log | Select-Object -Skip $lines | Select-String "alt mask|released before|not come through|still down after" |
        ForEach-Object { "    " + $_.Line }
      # Esc only to the popup (or the test window, when the popup did not take focus).
      $fg = [LtKeys]::GetForegroundWindow()
      if ($fg -eq [LtKeys]::FindWindow("Tauri Window", "LexTrail") -or $fg -eq $target.MainWindowHandle) { [LtKeys]::Esc() }
      else { "    foreground is another window ($fg): Esc not sent" }
      Start-Sleep -Milliseconds 300
      # Release whatever is stuck, so the next press starts clean.
      if ($held) { foreach ($vk in $held.Split(" ")) { [LtKeys]::Up([Convert]::ToInt32($vk, 16)) } }
    }
  }
} finally {
  # Never leave a modifier down for the user's next keystrokes.
  foreach ($vk in 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C) { if ([LtKeys]::GetAsyncKeyState($vk) -lt 0) { [LtKeys]::Up($vk) } }
  Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
  Stop-Process -Id $target.Id -Force -ErrorAction SilentlyContinue
  if ($observer) { Stop-Process -Id $observer.Id -Force -ErrorAction SilentlyContinue; "key events: $keysLog" }
}
"app log: $log"
if ($stuck) { "FAILED: a modifier stayed down after $stuck of $n presses"; exit 1 }
"no stuck keys in $n presses"
