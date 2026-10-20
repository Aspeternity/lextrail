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

It sends keys: leave the machine alone while it runs. Exits with 1 if a key was stuck.

.EXAMPLE
pwsh tools/e2e/stuck-keys.ps1 -App target\debug\lextrail-desktop.exe
#>
param(
  [string]$App = "target\debug\lextrail-desktop.exe",
  [int[]]$GapsMs = @(0, 0, 1, 1, 2, 3, 5, 8, 13, 20, 40, 80),
  [int]$Rounds = 2,
  [string]$OutDir = (Join-Path $env:TEMP "lextrail-e2e")
)
$ErrorActionPreference = "Stop"
$App = (Resolve-Path $App).Path
New-Item -ItemType Directory -Force $OutDir | Out-Null

Add-Type @"
using System; using System.Runtime.InteropServices; using System.Threading;
public static class LtKeys {
  [StructLayout(LayoutKind.Sequential)] struct KI { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Explicit, Size = 40)] struct IN { [FieldOffset(0)] public uint type; [FieldOffset(8)] public KI ki; }
  [DllImport("user32.dll")] static extern uint SendInput(uint n, IN[] i, int size);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
  [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vk);
  static void Key(ushort vk, bool up) {
    var i = new IN[1];
    i[0].type = 1; i[0].ki.vk = vk; i[0].ki.scan = (ushort)MapVirtualKey(vk, 0);
    i[0].ki.flags = up ? 2u : 0u;
    SendInput(1, i, Marshal.SizeOf(typeof(IN)));
  }
  // Left Ctrl + left Alt + X down; after gapMs, X, Alt and Ctrl up.
  public static void Hotkey(int gapMs) {
    Key(0xA2, false); Thread.Sleep(15); Key(0xA4, false); Thread.Sleep(15); Key(0x58, false);
    if (gapMs > 0) Thread.Sleep(gapMs);
    Key(0x58, true); Key(0xA4, true); Key(0xA2, true);
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

Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
$log = Join-Path $OutDir "stuck-keys-app.log"
# Scratch settings and history, never the user's own (debug builds only).
$env:LEXTRAIL_DEBUG_CONFIG_DIR = Join-Path $OutDir ("config-" + [guid]::NewGuid())
$env:LEXTRAIL_DEBUG_DATA_DIR = Join-Path $OutDir ("data-" + [guid]::NewGuid())
Start-Process $App -RedirectStandardError $log -WindowStyle Hidden
Remove-Item Env:LEXTRAIL_DEBUG_CONFIG_DIR, Env:LEXTRAIL_DEBUG_DATA_DIR -ErrorAction SilentlyContinue
Start-Sleep 4
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
  if ($target.MainWindowHandle -eq 0) { throw "test window not found" }
  for ($round = 1; $round -le $Rounds; $round++) {
    foreach ($gap in $GapsMs) {
      $n++
      [void]$ws.AppActivate($target.Id); Start-Sleep -Milliseconds 300
      $lines = @(Get-Content $log).Count
      [LtKeys]::Hotkey($gap); Start-Sleep -Milliseconds 700
      $held = [LtKeys]::Held()
      if ($held) { $stuck++ }
      "press ${n} (gap ${gap} ms): " + $(if ($held) { "STUCK: $held" } else { "ok" })
      # This press's mask decision (debug builds).
      Get-Content $log | Select-Object -Skip $lines | Select-String "alt mask|released before|not come through" |
        ForEach-Object { "    " + $_.Line }
      [LtKeys]::Esc(); Start-Sleep -Milliseconds 300
      # Release whatever is stuck, so the next press starts clean.
      if ($held) { foreach ($vk in $held.Split(" ")) { [LtKeys]::Up([Convert]::ToInt32($vk, 16)) } }
    }
  }
} finally {
  Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
  Stop-Process -Id $target.Id -Force -ErrorAction SilentlyContinue
}
"app log: $log"
if ($stuck) { "FAILED: a modifier stayed down after $stuck of $n presses"; exit 1 }
"no stuck keys in $n presses"
