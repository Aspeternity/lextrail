<#
.SYNOPSIS
Re-tests the lookup hotkey by modifier class (docs/specs/m1-hotkey-settings.md 8.2; D014,
R2-1, R2-2, manual test 5.1).

.DESCRIPTION
For each hotkey in -Hotkeys (default: one per class: Ctrl+Alt, Shift+Alt, Alt only,
Ctrl+Shift, a function key without modifiers) the given LexTrail debug build is started
with that lookup hotkey in fresh scratch settings and data directories, then:

  Windows Terminal: a new window (its own title) echoes a sentence; the word "brown" of
  the output is double-clicked with the real mouse, the hotkey is pressed, and
    - the popup must show "brown";
    - the terminal's selection must still be "brown" afterwards (UI Automation). With
      only Alt held the Alt mask falls back to 0xE8, which clears it (D014 supplement):
      for "Alt+..." the selection is expected to be gone.
  VS Code: a new instance with its own scratch profile (--user-data-dir, --extensions-dir;
  never the user's own) opens a scratch file; Ctrl+Home, Ctrl+Shift+Right select "The",
  the hotkey is pressed, the popup must show the word; after Esc the menu bar must not
  be active: nothing focused in it, and Down must not open a menu (R2-1).

Then, unless -NoStuckKeys, stuck-keys.ps1 runs with the same hotkey (-StuckRounds rounds of
its 12 gaps) and must report no stuck key.

It moves the mouse and sends keys: leave the machine alone while it runs. Keys are only
sent while the window they are meant for is in the foreground; otherwise the script
stops. Screenshots cover only the test windows and the popup. Exits with 1 if any check
fails.

.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File tools/e2e/hotkey-classes-e2e.ps1 -App target\debug\lextrail-desktop.exe
powershell -NoProfile -ExecutionPolicy Bypass -File tools/e2e/hotkey-classes-e2e.ps1 -App target\debug\lextrail-desktop.exe -Hotkeys "Alt+X" -NoStuckKeys
#>
param(
  [string]$App = "target\debug\lextrail-desktop.exe",
  [string[]]$Hotkeys = @("Ctrl+Alt+X", "Shift+Alt+X", "Alt+X", "Ctrl+Shift+F1", "F8"),
  [string]$OutDir = (Join-Path $env:TEMP "lextrail-classes-e2e"),
  [switch]$NoTerminal,
  [switch]$NoVsCode,
  [switch]$NoStuckKeys,
  [int]$StuckRounds = 10
)
$ErrorActionPreference = "Stop"
$App = (Resolve-Path $App).Path
if ($App -like "*\release\*") { throw "use a debug build: release builds would use the real settings" }
New-Item -ItemType Directory -Force $OutDir | Out-Null
. (Join-Path $PSScriptRoot "hotkey-keys.ps1")

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
Add-Type @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class LtCls {
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Ri, B; }
  [StructLayout(LayoutKind.Sequential)] public struct Pt { public int X, Y; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out R r, int size);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out R r);
  public static R Visible(IntPtr h) { R r; if (DwmGetWindowAttribute(h, 9, out r, 16) != 0) GetWindowRect(h, out r); return r; }
  [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
  public static void Key(byte vk, bool up) {
    // Arrows, Home, End are extended keys.
    uint ext = (vk >= 0x21 && vk <= 0x28) ? 1u : 0u;
    keybd_event(vk, (byte)MapVirtualKey(vk, 0), (up ? 2u : 0u) | ext, IntPtr.Zero);
  }
  [DllImport("user32.dll")] static extern short GetAsyncKeyState(int vk);
  public static string ReleaseHeld() {
    string s = "";
    int[] vks = { 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C };
    foreach (int vk in vks) if (GetAsyncKeyState(vk) < 0) { Key((byte)vk, true); s += vk.ToString("X") + " "; }
    return s.Trim();
  }
  [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Pt p);
  [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint flags);
  [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e);
  // Clicks (clicks = 1 or 2) at (x, y) only if that point shows window h.
  public static bool ClickAt(IntPtr h, int x, int y, int clicks) {
    var p = new Pt { X = x, Y = y };
    if (GetAncestor(WindowFromPoint(p), 2) != h) return false;
    SetCursorPos(x, y);
    for (int i = 0; i < clicks; i++) { mouse_event(2, 0, 0, 0, IntPtr.Zero); mouse_event(4, 0, 0, 0, IntPtr.Zero); System.Threading.Thread.Sleep(60); }
    return true;
  }
  // The first visible top-level window whose title contains the text.
  public static IntPtr WindowTitled(string part) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      if (!IsWindowVisible(h)) return true;
      var sb = new StringBuilder(512); GetWindowText(h, sb, 512);
      if (sb.ToString().Contains(part)) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
"@
[void][LtCls]::SetProcessDPIAware()
$UIA = [System.Windows.Automation.AutomationElement]
$TreeScope = [System.Windows.Automation.TreeScope]
$ControlType = [System.Windows.Automation.ControlType]

$failures = 0
function Check([bool]$Ok, [string]$What) {
  if ($Ok) { Write-Host "  ok   $What" } else { Write-Host "  FAIL $What"; $script:failures++ }
}

function Save-Screenshot([IntPtr]$Hwnd, [string]$Name) {
  $r = [LtCls]::Visible($Hwnd)
  $bmp = New-Object System.Drawing.Bitmap ($r.Ri - $r.L), ($r.B - $r.T)
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $OutDir "$Name.png"), [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}

function Assert-Front([IntPtr]$Hwnd, [string]$What) {
  $fg = [LtCls]::GetForegroundWindow()
  if ($fg -ne $Hwnd) { throw "the $What ($Hwnd) is not in the foreground ($fg); not sending keys" }
}

function Send-Keys([IntPtr]$To, [string]$What, [int[]]$Vks) {
  Assert-Front $To $What
  $last = $Vks.Count - 1
  for ($i = 0; $i -lt $last; $i++) { [LtCls]::Key([byte]$Vks[$i], $false); Start-Sleep -Milliseconds 15 }
  [LtCls]::Key([byte]$Vks[$last], $false); Start-Sleep -Milliseconds 40
  [LtCls]::Key([byte]$Vks[$last], $true); Start-Sleep -Milliseconds 30
  for ($i = $last - 1; $i -ge 0; $i--) { [LtCls]::Key([byte]$Vks[$i], $true); Start-Sleep -Milliseconds 20 }
}

function Wait-Window([scriptblock]$Find, [int]$Seconds) {
  for ($i = 0; $i -lt $Seconds * 5; $i++) {
    $h = & $Find
    if ($h -ne [IntPtr]::Zero) { return $h }
    Start-Sleep -Milliseconds 200
  }
  [IntPtr]::Zero
}

function Popup { [LtCls]::FindWindow("Tauri Window", "LexTrail") }

# The popup's text through UI Automation, once it shows the wanted word.
function Read-Popup([IntPtr]$Hwnd, [string]$Want) {
  $root = $UIA::FromHandle($Hwnd)
  $names = @()
  for ($try = 0; $try -lt 10; $try++) {
    $names = @($root.FindAll($TreeScope::Descendants, [System.Windows.Automation.Condition]::TrueCondition) |
        ForEach-Object { $_.Current.Name } | Where-Object { $_ })
    if ($names | Where-Object { $_ -ieq $Want }) { break }
    Start-Sleep -Milliseconds 200
  }
  $names
}

# Waits for the popup, checks it shows the word, closes it with Esc.
function Check-Popup([string]$Word, [string]$Name, [IntPtr]$Back) {
  $popup = Wait-Window { $p = Popup; if ($p -ne [IntPtr]::Zero -and [LtCls]::IsWindowVisible($p)) { $p } else { [IntPtr]::Zero } } 3
  Check ($popup -ne [IntPtr]::Zero) "$Name`: the popup shows"
  if ($popup -eq [IntPtr]::Zero) { return }
  Start-Sleep -Milliseconds 300
  $names = Read-Popup $popup $Word
  Check ([bool]($names | Where-Object { $_ -ieq $Word })) "$Name`: the popup shows '$Word'"
  Save-Screenshot $popup "$Name-popup"
  Start-Sleep -Milliseconds 200
  $fg = [LtCls]::GetForegroundWindow()
  if ($fg -eq $popup) { Send-Keys $popup "popup" @(0x1B) } else { Write-Host "  (popup without focus; Esc not sent)" }
  Start-Sleep -Milliseconds 500
  Check ([LtCls]::GetForegroundWindow() -eq $Back) "$Name`: Esc gives the foreground back"
}

function Test-Terminal([string]$Hotkey, [string]$Tag) {
  $title = "lextrail-e2e-wt-" + [guid]::NewGuid().ToString().Substring(0, 8)
  Start-Process wt.exe -ArgumentList "-w", "new", "--title", $title, "--suppressApplicationTitle",
    "cmd", "/k", "echo The quick brown fox jumps over the lazy dog."
  $wt = Wait-Window { [LtCls]::WindowTitled($title) } 10
  Check ($wt -ne [IntPtr]::Zero) "$Tag terminal: window opened"
  if ($wt -eq [IntPtr]::Zero) { return }
  try {
    Start-Sleep 2
    $root = $UIA::FromHandle($wt)
    $textCond = New-Object System.Windows.Automation.PropertyCondition($UIA::IsTextPatternAvailableProperty, $true)
    $term = $root.FindFirst($TreeScope::Descendants, $textCond)
    if (-not $term) { throw "no text control in the terminal" }
    $text = $term.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern)
    # The last "brown": the output line, not the command line.
    $range = $text.DocumentRange.FindText("brown", $true, $false)
    $rects = $range.GetBoundingRectangles()
    if ($rects.Count -eq 0) { throw "'brown' is not on screen" }
    $x = [int]($rects[0].X + $rects[0].Width / 2); $y = [int]($rects[0].Y + $rects[0].Height / 2)
    if (-not [LtCls]::ClickAt($wt, $x, $y, 2)) { throw "the terminal is covered; not clicking" }
    Start-Sleep -Milliseconds 400
    $selected = ($text.GetSelection() | ForEach-Object { $_.GetText(-1) }) -join ""
    Check ($selected.Trim() -eq "brown") "$Tag terminal: 'brown' selected before the hotkey ('$selected')"
    Send-Keys $wt "terminal" (Get-HotkeyVks $Hotkey)
    Check-Popup "brown" "$Tag-terminal" $wt
    $after = ($text.GetSelection() | ForEach-Object { $_.GetText(-1) }) -join ""
    $altOnly = $Hotkey -match "^Alt\+"
    if ($altOnly) {
      Check ($after.Trim() -ne "brown") "$Tag terminal: selection gone, as expected with only Alt ('$after')"
    } else {
      Check ($after.Trim() -eq "brown") "$Tag terminal: selection kept ('$after')"
    }
    Save-Screenshot $wt "$Tag-terminal"
  } finally {
    [void][LtCls]::PostMessage($wt, 0x10, [IntPtr]::Zero, [IntPtr]::Zero)
    Start-Sleep 1
  }
}

function Test-VsCode([string]$Hotkey, [string]$Tag) {
  $dir = Join-Path $OutDir ("vscode-" + [guid]::NewGuid())
  New-Item -ItemType Directory -Force "$dir\user", "$dir\ext" | Out-Null
  $file = Join-Path $dir "lextrail-e2e-sample.txt"
  Set-Content -Path $file -Value "The quick brown fox jumps over the lazy dog." -Encoding ASCII
  Start-Process code.cmd -WindowStyle Hidden -ArgumentList "--new-window", "--user-data-dir", "`"$dir\user`"",
    "--extensions-dir", "`"$dir\ext`"", "--disable-workspace-trust", "--skip-welcome",
    "--skip-release-notes", "`"$file`""
  $code = Wait-Window { [LtCls]::WindowTitled("lextrail-e2e-sample.txt") } 20
  Check ($code -ne [IntPtr]::Zero) "$Tag VS Code: window opened"
  if ($code -eq [IntPtr]::Zero) { return }
  try {
    Start-Sleep 3
    $r = [LtCls]::Visible($code)
    # Into the editor: a click a little below the tab bar, left of the middle.
    if (-not [LtCls]::ClickAt($code, [int]($r.L + ($r.Ri - $r.L) * 0.4), [int]($r.T + ($r.B - $r.T) * 0.3), 1)) {
      throw "VS Code is covered; not clicking"
    }
    Start-Sleep -Milliseconds 500
    Send-Keys $code "VS Code" @(0xA2, 0x24)          # Ctrl+Home
    Send-Keys $code "VS Code" @(0xA2, 0xA0, 0x27)    # Ctrl+Shift+Right: "The"
    Start-Sleep -Milliseconds 300
    Send-Keys $code "VS Code" (Get-HotkeyVks $Hotkey)
    Check-Popup "The" "$Tag-vscode" $code
    $focused = $UIA::FocusedElement
    $type = if ($focused) { $focused.Current.ControlType.ProgrammaticName } else { "none" }
    Check ($type -notmatch "MenuItem|MenuBar") "$Tag VS Code: menu bar not active after Esc (focus: $type)"
    Send-Keys $code "VS Code" @(0x28)                # Down
    Start-Sleep -Milliseconds 600
    $menuCond = New-Object System.Windows.Automation.PropertyCondition($UIA::ControlTypeProperty, $ControlType::Menu)
    $menu = $UIA::FromHandle($code).FindFirst($TreeScope::Descendants, $menuCond)
    Check ($null -eq $menu) "$Tag VS Code: Down opens no menu"
    Save-Screenshot $code "$Tag-vscode"
  } finally {
    [void][LtCls]::PostMessage($code, 0x10, [IntPtr]::Zero, [IntPtr]::Zero)
    Start-Sleep 2
    # Only the instance with this scratch profile.
    Get-CimInstance Win32_Process -Filter "Name = 'Code.exe'" |
      Where-Object { $_.CommandLine -like "*$dir*" } |
      ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  }
}

try {
  foreach ($hotkey in $Hotkeys) {
    $tag = $hotkey.Replace("+", "")
    Write-Host "Hotkey $hotkey"
    Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
    $configDir = Join-Path $OutDir ("config-" + [guid]::NewGuid())
    Write-HotkeySettings $configDir $hotkey
    $env:LEXTRAIL_DEBUG_CONFIG_DIR = $configDir
    $env:LEXTRAIL_DEBUG_DATA_DIR = Join-Path $OutDir ("data-" + [guid]::NewGuid())
    $log = Join-Path $OutDir "$tag-app.log"
    $lt = Start-Process $App -RedirectStandardError $log -PassThru
    Remove-Item Env:LEXTRAIL_DEBUG_CONFIG_DIR, Env:LEXTRAIL_DEBUG_DATA_DIR
    Start-Sleep 4
    try {
      if (-not $NoTerminal) { Test-Terminal $hotkey $tag }
      if (-not $NoVsCode) { Test-VsCode $hotkey $tag }
    } finally {
      Stop-Process -Id $lt.Id -Force -ErrorAction SilentlyContinue
    }
    if (-not $NoStuckKeys) {
      & (Join-Path $PSScriptRoot "stuck-keys.ps1") -App $App -Hotkey $hotkey -Rounds $StuckRounds -OutDir $OutDir |
        Select-Object -Last 3 | ForEach-Object { Write-Host "  stuck-keys: $_" }
      Check ($LASTEXITCODE -eq 0) "$hotkey stuck-keys: no stuck key"
    }
  }
} finally {
  $held = [LtCls]::ReleaseHeld()
  if ($held) { Write-Host "released stuck keys: $held" }
  Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
}

if ($failures -gt 0) { Write-Host "$failures check(s) failed"; exit 1 }
Write-Host "all checks passed"
