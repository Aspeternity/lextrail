<#
.SYNOPSIS
End-to-end check of the settings window (docs/specs/m1-hotkey-settings.md, section 8.2).

.DESCRIPTION
Starts the given LexTrail debug build with --settings in fresh settings and data
directories under -OutDir (LEXTRAIL_DEBUG_CONFIG_DIR / LEXTRAIL_DEBUG_DATA_DIR; never the
user's own) and checks, through UI Automation only (no mouse, no keys):
  - the settings window opens at first launch with --settings;
  - "double Ctrl+C" and "do not record history" are written to settings.toml at once;
  - after a hand edit of settings.toml the app refuses to overwrite it and says so;
  - a second launch with --settings brings the open window back instead of another one.

With -Keys it also uses the real mouse and keyboard on the settings window (and on a
small window of its own), so leave the machine alone while it runs. Keys are only sent
while the window they are meant for is in the foreground; otherwise the script stops.
Whether LexTrail holds a hotkey is checked by trying to register it here
(RegisterHotKey fails while another program holds it):
  - recording turns the hotkey off; F5 is recorded as a hotkey without reloading the page;
    Ctrl+R, Ctrl+P and Ctrl+F are saved at once without reloading, printing or finding;
  - "no hotkey" saves an empty value and lets the hotkey go;
  - Ctrl+C is refused; Esc cancels and the old hotkey comes back;
  - a combination this script holds shows "no reaction, maybe taken by another program";
  - "restore default" while this script holds the default keeps the current hotkey;
  - closing the window, or switching to another window, while recording brings the old
    hotkey back;
  - F5 in the popup does not reload its page (D010).

Screenshots cover only LexTrail's windows. Exits with 1 if any check fails.

.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File tools/e2e/settings-e2e.ps1 -App target\debug\lextrail-desktop.exe
powershell -NoProfile -ExecutionPolicy Bypass -File tools/e2e/settings-e2e.ps1 -App target\debug\lextrail-desktop.exe -Keys
#>
param(
  [string]$App = "target\debug\lextrail-desktop.exe",
  [string]$OutDir = (Join-Path $env:TEMP "lextrail-settings-e2e"),
  [switch]$Keys
)
$ErrorActionPreference = "Stop"
$App = (Resolve-Path $App).Path
if ($App -like "*\release\*") { throw "use a debug build: release builds would use the real settings" }
New-Item -ItemType Directory -Force $OutDir | Out-Null

# Built from code points: Windows PowerShell reads a script without BOM in the ANSI code page.
function Text([int[]]$CodePoints) { -join ($CodePoints | ForEach-Object { [char]$_ }) }
$settingsTitle = "LexTrail " + (Text 0x8BBE, 0x7F6E)
$noReaction = Text 0x6309, 0x4E0B, 0x6CA1, 0x6709, 0x53CD, 0x5E94

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing, System.Windows.Forms
Add-Type @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class LtSet {
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Ri, B; }
  [StructLayout(LayoutKind.Sequential)] public struct Pt { public int X, Y; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out R r, int size);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out R r);
  public static R Visible(IntPtr h) { R r; if (DwmGetWindowAttribute(h, 9, out r, 16) != 0) GetWindowRect(h, out r); return r; }
  [DllImport("user32.dll")] public static extern bool RegisterHotKey(IntPtr h, int id, uint mods, uint vk);
  [DllImport("user32.dll")] public static extern bool UnregisterHotKey(IntPtr h, int id);
  [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
  public static void Key(byte vk, bool up) { keybd_event(vk, (byte)MapVirtualKey(vk, 0), up ? 2u : 0u, IntPtr.Zero); }
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
  // Clicks (x, y) only if that point shows window h (not one on top of it).
  public static bool ClickAt(IntPtr h, int x, int y) {
    var p = new Pt { X = x, Y = y };
    if (GetAncestor(WindowFromPoint(p), 2) != h) return false;
    SetCursorPos(x, y); mouse_event(2, 0, 0, 0, IntPtr.Zero); mouse_event(4, 0, 0, 0, IntPtr.Zero);
    return true;
  }
  // Visible top-level windows of a process with exactly this title.
  public static List<IntPtr> Windows(uint pid, string title) {
    var found = new List<IntPtr>();
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) {
        var sb = new StringBuilder(256); GetWindowText(h, sb, 256);
        if (sb.ToString() == title) found.Add(h);
      }
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
"@
[void][LtSet]::SetProcessDPIAware()
$UIA = [System.Windows.Automation.AutomationElement]

$failures = 0
function Check([bool]$Ok, [string]$What) {
  if ($Ok) { Write-Host "  ok   $What" } else { Write-Host "  FAIL $What"; $script:failures++ }
}

function Save-Screenshot([IntPtr]$Hwnd, [string]$Name) {
  $r = [LtSet]::Visible($Hwnd)
  $bmp = New-Object System.Drawing.Bitmap ($r.Ri - $r.L), ($r.B - $r.T)
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $OutDir "$Name.png"), [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}

# The element with this HTML id (WebView2 reports it as the AutomationId), or $null.
function Find-Id([IntPtr]$Hwnd, [string]$Id, [int]$Tries = 15) {
  $root = $UIA::FromHandle($Hwnd)
  $cond = New-Object System.Windows.Automation.PropertyCondition($UIA::AutomationIdProperty, $Id)
  for ($i = 0; $i -lt $Tries; $i++) {
    $e = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
    if ($e) { return $e }
    Start-Sleep -Milliseconds 200
  }
  $null
}

# The visible text of an element (a <p> reports its text through its children).
function Text-Of($Element) {
  if (-not $Element) { return "" }
  $parts = @($Element.Current.Name) + @($Element.FindAll([System.Windows.Automation.TreeScope]::Descendants,
      [System.Windows.Automation.Condition]::TrueCondition) | ForEach-Object { $_.Current.Name })
  ($parts | Where-Object { $_ }) -join " "
}

function Toggle([IntPtr]$Hwnd, [string]$Id) {
  (Find-Id $Hwnd $Id).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
  Start-Sleep -Milliseconds 500
}

function Toggle-State([IntPtr]$Hwnd, [string]$Id) {
  [string](Find-Id $Hwnd $Id).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState
}

function Invoke-Id([IntPtr]$Hwnd, [string]$Id) {
  (Find-Id $Hwnd $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  Start-Sleep -Milliseconds 500
}

function Settings-Window($Proc, [int]$Seconds = 8) {
  for ($i = 0; $i -lt $Seconds * 5; $i++) {
    $found = [LtSet]::Windows([uint32]$Proc.Id, $settingsTitle)
    if ($found.Count -gt 0) { return $found[0] }
    Start-Sleep -Milliseconds 200
  }
  [IntPtr]::Zero
}

$configDir = Join-Path $OutDir ("config-" + [guid]::NewGuid())
$dataDir = Join-Path $OutDir ("data-" + [guid]::NewGuid())
$settingsFile = Join-Path $configDir "settings.toml"
$log = Join-Path $OutDir "app.log"

function Start-App {
  $env:LEXTRAIL_DEBUG_CONFIG_DIR = $configDir
  $env:LEXTRAIL_DEBUG_DATA_DIR = $dataDir
  $p = Start-Process $App -ArgumentList "--settings" -RedirectStandardError $log -PassThru
  Remove-Item Env:LEXTRAIL_DEBUG_CONFIG_DIR, Env:LEXTRAIL_DEBUG_DATA_DIR
  $p
}

function Stop-App($Proc) {
  if ($Proc -and -not $Proc.HasExited) { Stop-Process -Id $Proc.Id -Force; $Proc.WaitForExit(5000) | Out-Null }
}

Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
$appProc = Start-App
try {
  Write-Host "Window and switches (UI Automation only)"
  $win = Settings-Window $appProc
  Check ($win -ne [IntPtr]::Zero) "--settings at first launch opens the settings window"
  if ($win -eq [IntPtr]::Zero) { throw "no settings window" }
  Save-Screenshot $win "settings-open"
  Check (Test-Path $settingsFile) "settings.toml created in the scratch directory"

  Toggle $win "double-ctrl-c"
  Check ((Get-Content $settingsFile -Raw) -match "double_ctrl_c = true") "double Ctrl+C on is saved"
  Toggle $win "double-ctrl-c"
  Check ((Get-Content $settingsFile -Raw) -match "double_ctrl_c = false") "double Ctrl+C off is saved"

  Invoke-Id $win "nav-privacy"
  Toggle $win "no-history"
  Check ((Get-Content $settingsFile -Raw) -match "record = false") "no history is saved"
  Toggle $win "no-history"
  Check ((Get-Content $settingsFile -Raw) -match "record = true") "history again is saved"

  # A hand edit while the app runs: the app must not overwrite it.
  Add-Content -Path $settingsFile -Value "# hand edit" -Encoding ASCII
  Toggle $win "no-history"
  $err = Find-Id $win "history-error" 10
  Check ($null -ne $err) "a hand-edited file is not overwritten: the window says so"
  Check ((Get-Content $settingsFile -Raw) -match "# hand edit") "the hand edit is still in the file"
  Check ((Toggle-State $win "no-history") -eq "Off") "the switch went back"
  Save-Screenshot $win "settings-hand-edit"

  # A second launch with --settings: same window, no new process left behind.
  $second = Start-Process $App -ArgumentList "--settings" -PassThru
  $second.WaitForExit(5000) | Out-Null
  Start-Sleep -Milliseconds 500
  Check ($second.HasExited) "a second launch exits"
  Check ([LtSet]::Windows([uint32]$appProc.Id, $settingsTitle).Count -eq 1) "still one settings window"

  if ($Keys) {
    # A fresh start: the hand edit above makes every change refused.
    Stop-App $appProc
    Remove-Item $settingsFile
    $appProc = Start-App
    $win = Settings-Window $appProc
    if ($win -eq [IntPtr]::Zero) { throw "no settings window after the restart" }
    Write-Host "Recording (real mouse and keys)"
    $CTRL = 0xA2; $ALT = 0xA4; $SHIFT = 0xA0
    $MOD_ALT = 1; $MOD_CONTROL = 2; $MOD_SHIFT = 4

    # Whether nobody holds the combination: this script can register it (and lets go).
    function Is-Free([uint32]$Mods, [uint32]$Vk) {
      if ([LtSet]::RegisterHotKey([IntPtr]::Zero, 0x4C54, $Mods -bor 0x4000, $Vk)) {
        [void][LtSet]::UnregisterHotKey([IntPtr]::Zero, 0x4C54); return $true
      }
      $false
    }
    # The default lookup hotkey (D014).
    $defaultKey = @($MOD_ALT, 0x20)

    function Assert-Front([IntPtr]$Hwnd, [string]$What) {
      $fg = [LtSet]::GetForegroundWindow()
      if ($fg -ne $Hwnd) { throw "the $What ($Hwnd) is not in the foreground ($fg); not sending keys" }
    }

    # Modifiers down, key down and up, modifiers up (in reverse), at a person's speed.
    function Send-Combo([IntPtr]$To, [string]$What, [int[]]$Mods, [int]$Vk) {
      Assert-Front $To $What
      foreach ($m in $Mods) { [LtSet]::Key([byte]$m, $false); Start-Sleep -Milliseconds 15 }
      [LtSet]::Key([byte]$Vk, $false); Start-Sleep -Milliseconds 40
      [LtSet]::Key([byte]$Vk, $true); Start-Sleep -Milliseconds 30
      for ($i = $Mods.Count - 1; $i -ge 0; $i--) { [LtSet]::Key([byte]$Mods[$i], $true); Start-Sleep -Milliseconds 15 }
      Start-Sleep -Milliseconds 700
    }

    # A real click in the middle of the element, only where the window shows it.
    function Click-Id([IntPtr]$Hwnd, [string]$Id) {
      $e = Find-Id $Hwnd $Id
      if (-not $e) { throw "element #$Id not found" }
      $r = $e.Current.BoundingRectangle
      if (-not [LtSet]::ClickAt($Hwnd, [int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2))) {
        throw "element #$Id is covered by another window; not clicking"
      }
      Start-Sleep -Milliseconds 600
    }

    function Lookup-Setting { if ((Get-Content $settingsFile -Raw) -match 'lookup = "([^"]*)"') { $Matches[1] } }

    Check (-not (Is-Free @defaultKey)) "LexTrail holds Alt+Space"

    Click-Id $win "hotkey-change"
    Check (Is-Free @defaultKey) "recording turns the hotkey off"

    # F5 is a fine hotkey: recorded, and the page did not reload (the notice is there).
    Send-Combo $win "settings window" @() 0x74
    Check ((Text-Of (Find-Id $win "hotkey-notice")) -match "F5") "F5 is recorded; the page did not reload"
    Check (-not (Is-Free 0 0x74)) "LexTrail holds F5"
    Check (Is-Free @defaultKey) "Alt+Space is let go"
    Check ((Lookup-Setting) -eq "F5") "F5 is saved"

    # Browser shortcuts while recording: recorded at once (other programs' shortcuts are
    # the user's choice, D048), no reload / print / find.
    foreach ($k in @(@(0x52, "Ctrl+R"), @(0x50, "Ctrl+P"), @(0x46, "Ctrl+F"))) {
      Click-Id $win "hotkey-change"
      Send-Combo $win "settings window" @($CTRL) $k[0]
      Check ((Lookup-Setting) -eq $k[1]) "$($k[1]) is recorded and saved without a question"
      Check ((Text-Of (Find-Id $win "hotkey-notice")) -match [regex]::Escape($k[1].Replace("+", " + "))) "$($k[1]): the page did not reload"
      Save-Screenshot $win ("recording-" + $k[1].Replace("+", ""))
      Check ([LtSet]::Windows([uint32]$appProc.Id, $settingsTitle).Count -eq 1) "$($k[1]) opened no other window"
    }
    Click-Id $win "hotkey-change"
    Click-Id $win "hotkey-cancel-recording"
    Check (-not (Is-Free $MOD_CONTROL 0x46)) "cancel brings Ctrl+F back"

    # Restore the default: no question either.
    Click-Id $win "hotkey-default"
    Check (-not (Is-Free @defaultKey)) "restore default: LexTrail holds Alt+Space"
    Check (Is-Free $MOD_CONTROL 0x46) "Ctrl+F is let go"
    Check ((Lookup-Setting) -eq "Alt+Space") "Alt+Space is saved"

    # No lookup hotkey (spec 5.4): saved as an empty value, nothing held, not a failure.
    Click-Id $win "hotkey-off"
    Check ((Lookup-Setting) -eq "") "no hotkey is saved as an empty value"
    Check (Is-Free @defaultKey) "no hotkey: Alt+Space is let go"
    Check ((Text-Of (Find-Id $win "hotkey-recorder")) -match "\S") "no hotkey: the recorder says so"
    Save-Screenshot $win "hotkey-off"
    Click-Id $win "hotkey-default"
    Check (-not (Is-Free @defaultKey)) "default again after no hotkey"

    # Ctrl+C is refused; Esc cancels.
    Click-Id $win "hotkey-change"
    Send-Combo $win "settings window" @($CTRL) 0x43
    Check ((Text-Of (Find-Id $win "hotkey-notice")) -match "Ctrl \+ C") "Ctrl+C is refused with the reason"
    Check (Is-Free @defaultKey) "still recording after Ctrl+C"
    Send-Combo $win "settings window" @() 0x1B
    Check (-not (Is-Free @defaultKey)) "Esc cancels: Alt+Space is back"

    # A combination another program holds: the window never sees the key.
    $held = [LtSet]::RegisterHotKey([IntPtr]::Zero, 0x4C55, ($MOD_CONTROL -bor $MOD_ALT -bor 0x4000), 0x59)
    Check $held "this script holds Ctrl+Alt+Y"
    Click-Id $win "hotkey-change"
    Send-Combo $win "settings window" @($CTRL, $ALT) 0x59
    $notice = Text-Of (Find-Id $win "hotkey-notice")
    Check ($notice.Contains($noReaction)) "a held combination shows 'no reaction'"
    Save-Screenshot $win "recording-taken"
    Send-Combo $win "settings window" @() 0x1B
    [void][LtSet]::UnregisterHotKey([IntPtr]::Zero, 0x4C55)
    Check (-not (Is-Free @defaultKey)) "Alt+Space is back after Esc"

    # The new one fails to register: the current one stays, the file is unchanged.
    Click-Id $win "hotkey-change"
    Send-Combo $win "settings window" @($CTRL, $SHIFT) 0x71
    Check ((Lookup-Setting) -eq "Ctrl+Shift+F2") "Ctrl+Shift+F2 is saved"
    $held = [LtSet]::RegisterHotKey([IntPtr]::Zero, 0x4C56, ($MOD_ALT -bor 0x4000), 0x20)
    Check $held "this script holds Alt+Space"
    Click-Id $win "hotkey-default"
    Check ($null -ne (Find-Id $win "hotkey-notice")) "registering a taken default reports the failure"
    Check (-not (Is-Free ($MOD_CONTROL -bor $MOD_SHIFT) 0x71)) "Ctrl+Shift+F2 still works"
    Check ((Lookup-Setting) -eq "Ctrl+Shift+F2") "the file still says Ctrl+Shift+F2"
    Save-Screenshot $win "default-taken"
    [void][LtSet]::UnregisterHotKey([IntPtr]::Zero, 0x4C56)
    Click-Id $win "hotkey-default"
    Check (-not (Is-Free @defaultKey)) "default again once it is free"

    # A window of this script, to switch to.
    $form = New-Object System.Windows.Forms.Form
    $form.Text = "lextrail-settings-e2e"
    $form.StartPosition = "Manual"
    $wr = [LtSet]::Visible($win)
    $form.Location = New-Object System.Drawing.Point ($wr.Ri + 20), $wr.T
    $form.Size = New-Object System.Drawing.Size 400, 300
    $form.Show()
    for ($i = 0; $i -lt 10; $i++) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 50 }
    function Click-Form {
      $fr = [LtSet]::Visible($form.Handle)
      if (-not [LtSet]::ClickAt($form.Handle, [int](($fr.L + $fr.Ri) / 2), [int](($fr.T + $fr.B) / 2))) {
        throw "the test form is covered; not clicking"
      }
      for ($i = 0; $i -lt 10; $i++) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 50 }
    }

    try {
      # Switching to another window while recording cancels it.
      Click-Id $win "hotkey-change"
      Check (Is-Free @defaultKey) "recording again"
      Click-Form
      Start-Sleep -Milliseconds 500
      Check (-not (Is-Free @defaultKey)) "switching away while recording brings Alt+Space back"

      # Closing the window while recording.
      Click-Id $win "hotkey-change"
      Check (Is-Free @defaultKey) "recording before closing"
      [void][LtSet]::PostMessage($win, 0x10, [IntPtr]::Zero, [IntPtr]::Zero)
      Start-Sleep -Milliseconds 1000
      Check ([LtSet]::Windows([uint32]$appProc.Id, $settingsTitle).Count -eq 0) "the settings window is closed"
      Check (-not (Is-Free @defaultKey)) "closing while recording brings Alt+Space back"

      # F5 in the popup must not reload its page (D010): no second PageReady.
      Click-Form
      Send-Combo $form.Handle "test form" @($ALT) 0x20
      Start-Sleep -Milliseconds 800
      $popup = [LtSet]::FindWindow("Tauri Window", "LexTrail")
      Check ($popup -ne [IntPtr]::Zero -and [LtSet]::IsWindowVisible($popup)) "the hotkey shows the popup"
      if ($popup -ne [IntPtr]::Zero) {
        $ready = @(Select-String -Path $log -Pattern "PageReady" -SimpleMatch).Count
        Send-Combo $popup "popup" @() 0x74
        Start-Sleep -Milliseconds 1000
        $after = @(Select-String -Path $log -Pattern "PageReady" -SimpleMatch).Count
        Check ($after -eq $ready) "F5 in the popup does not reload it ($ready -> $after PageReady)"
        Check ([LtSet]::IsWindowVisible($popup)) "the popup is still shown after F5"
        Save-Screenshot $popup "popup-after-f5"
        Send-Combo $popup "popup" @() 0x1B
      }
    } finally {
      $form.Close()
    }
  }
} finally {
  $held = [LtSet]::ReleaseHeld()
  if ($held) { Write-Host "released stuck keys: $held" }
  Stop-App $appProc
}

if ($failures -gt 0) { Write-Host "$failures check(s) failed; log: $log"; exit 1 }
Write-Host "all checks passed"
