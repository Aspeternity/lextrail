<#
.SYNOPSIS
End-to-end check of what the lookup popup actually shows.

.DESCRIPTION
Starts the given LexTrail build and a fresh Edge process (own InPrivate profile, app mode)
on list-item.html, double-clicks the word "launch" with the real mouse, presses the real
hotkey (Ctrl+Alt+X) and then checks the popup:
  - it is visible and has focus;
  - its text, read back through UI Automation (independent of LexTrail's own code),
    contains the word and the sentence without the list marker.
Each lookup is also saved as a screenshot for a human look.

With -PauseMs N the script waits N ms after closing each popup. Over about 6 s the hidden
popup is suspended (D009), so the next lookup resumes it. -ResumeDelayMs makes that resume
slow (debug builds, LEXTRAIL_DEBUG_RESUME_DELAY_MS), so the captured text is ready before
the resumed window: the order that hung the app in R4 (R4-P1). After every lookup the app
must still answer window messages; a hung UI thread fails the lookup.

With -Pdf the same page is printed to a PDF and opened in Edge's PDF viewer, where UIA
is not used (D041) and the word comes through the clipboard fallback (D043): this checks
that the popup still gets focus after the simulated Ctrl+C. There is no sentence then.

With -HoldMs the hotkey's Ctrl+Alt are held for that long after the X is pressed (use it
with -Pdf, where the clipboard fallback waits for the keys, D043). The popup is read once
more shortly before the keys are released: held for over a second it must say
"release the hotkey to capture..." (R3-P4); held for more than 3 s the result must be
"release the hotkey and try again"
(R3-P2), otherwise the word. (Read with UI Automation, like the rest.)

With -DoubleCopy the lookup is made with a double Ctrl+C (D012) instead of the hotkey:
the setting hotkeys.double_ctrl_c is switched on in the scratch settings, and Ctrl+C is
pressed twice, -CopyGapMs apart. Each lookup must then be started by the double copy
(debug log "double copy: lookup triggered"); the sentence comes from UI Automation
(with -Pdf there is none). -DoubleCopySetting switches the setting on but keeps using the
hotkey: then no lookup may come from a double copy, although the clipboard fallback
(-Pdf) copies and restores within the double-copy gap. -PressTwiceMs N presses the
hotkey a second time N ms after the first (R1-5: two captures overlap).

Edge InPrivate marks what it copies CanIncludeInClipboardHistory = 0, which double Ctrl+C
ignores (spec section 2, rule 7): -DoubleCopy therefore needs -NoInPrivate (a fresh
ordinary profile with sync off).

Each popup is then closed (-CloseBy Esc, the default; Click: a click on the test page
away from the popup; ClickThenEsc: Click for odd lookups, Esc for even ones) and must be gone afterwards, with the test page in the foreground
again. -ForceAttach makes the app take the foreground through its fallback
(AttachThreadInput, debug builds: LEXTRAIL_DEBUG_FORCE_ATTACH), which is what runs after
a click outside closed the last popup; every activation must then be "Attached" (or
"AlreadyInFront": after Esc, showing the popup already brings it to the front) and the
popup must keep the focus (no focus ping-pong).

The popup's height follows its content (D046 supplement): every lookup's window height
is printed, and all lookups of a run must have the same height (the first popup used to
be taller). -Word run looks up "run" (run-word.html) instead of "launch": many senses.
-Expand then unfolds every section through UI Automation (no mouse): the window must
reach its highest (min(520 logical px, 60% of the monitor's work area)) and its body
must scroll.

With -Chrome the page opens in Google Chrome instead of Edge (incognito unless
-NoInPrivate; not with -Pdf). -ExpectIgnored checks the opposite of a lookup: no popup
may show, no lookup may be started or recorded. Use it with -DoubleCopy where double
Ctrl+C must not trigger, e.g. in InPrivate / incognito windows (marked
CanIncludeInClipboardHistory = 0, D012 supplement).

The app reads its settings and writes its lookup history in fresh directories under
-OutDir (debug builds only: LEXTRAIL_DEBUG_CONFIG_DIR, LEXTRAIL_DEBUG_DATA_DIR), never the
user's own; release builds are refused for that reason. The lookups recorded are checked
against the popup: the first is recorded, a repeat within 30 s is not.

It moves the mouse and sends keys: leave the machine alone while it runs. Keys are only
sent while the window they are meant for (the test page, or the popup for Esc) is in the
foreground; otherwise the script stops, so no key lands in another program. Screenshots
cover only the test page and the popup. Exits with 1 if any check fails.

.EXAMPLE
pwsh tools/e2e/popup-e2e.ps1 -App target\debug\lextrail-desktop.exe
pwsh tools/e2e/popup-e2e.ps1 -App target\debug\lextrail-desktop.exe -Pdf
pwsh tools/e2e/popup-e2e.ps1 -App target\debug\lextrail-desktop.exe -Pdf -HoldMs 2000
pwsh tools/e2e/popup-e2e.ps1 -App target\debug\lextrail-desktop.exe -Lookups 3 -PauseMs 7000 -ResumeDelayMs 300
pwsh tools/e2e/popup-e2e.ps1 -App target\debug\lextrail-desktop.exe -DoubleCopy
pwsh tools/e2e/popup-e2e.ps1 -App target\debug\lextrail-desktop.exe -Pdf -DoubleCopySetting -PressTwiceMs 150
#>
param(
  [string]$App = "target\debug\lextrail-desktop.exe",
  [int]$Lookups = 2,
  [string]$OutDir = (Join-Path $env:TEMP "lextrail-e2e"),
  [switch]$Pdf,
  [int]$HoldMs = 0,
  [int]$PauseMs = 600,
  [int]$ResumeDelayMs = 0,
  [switch]$DoubleCopy,
  [int]$CopyGapMs = 200,
  [switch]$DoubleCopySetting,
  [int]$PressTwiceMs = 0,
  [switch]$NoInPrivate,
  [switch]$Chrome,
  [switch]$ExpectIgnored,
  [switch]$ForceAttach,
  [ValidateSet("launch", "run")][string]$Word = "launch",
  [switch]$Expand,
  [ValidateSet("Esc", "Click", "ClickThenEsc")][string]$CloseBy = "Esc"
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$App = (Resolve-Path $App).Path
# Only debug builds honour the directory overrides below.
if ($App -like "*\release\*") { throw "use a debug build: release builds would use the real settings and history" }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$expected = if ($Word -eq "run") { @("run", "run the tests again.") }
  elseif ($Pdf) { @("launch") } else { @("launch", "launch missiles at ships.") }
if ($Word -eq "run" -and $Pdf) { throw "-Word run is for the web page only" }
# The clipboard fallback gives up after 3 s of held keys (D043).
# Built from code points: Windows PowerShell reads a script without BOM in the ANSI code page.
function Text([int[]]$CodePoints) { -join ($CodePoints | ForEach-Object { [char]$_ }) }
# The popup texts "release the hotkey and try again" (D043, R3-P2) and "release the
# hotkey to capture..." (R3-P4).
$keysHeldTitle = Text 0x8BF7, 0x677E, 0x5F00, 0x5FEB, 0x6377, 0x952E, 0x540E, 0x518D, 0x8BD5
$holdHint = Text 0x677E, 0x5F00, 0x5FEB, 0x6377, 0x952E, 0x540E, 0x53D6, 0x8BCD, 0x2026
# Debug builds show where the word came from ("from msedge.exe"); release builds never
# send it (D046 supplement).
$browserProc = if ($Chrome) { "chrome" } else { "msedge" }
$expected += (Text 0x6765, 0x81EA) + " $browserProc.exe"
if ($Chrome -and $Pdf) { throw "-Chrome is for the web page only" }
if ($HoldMs -gt 3000) { $expected = @($keysHeldTitle) }

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing, System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class LtE2e {
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Ri, B; }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out R r, int size);
  // The visible frame (DWMWA_EXTENDED_FRAME_BOUNDS): GetWindowRect also covers the
  // invisible resize borders, which show whatever window is behind.
  public static R Visible(IntPtr h) { R r; if (DwmGetWindowAttribute(h, 9, out r, 16) != 0) GetWindowRect(h, out r); return r; }
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool r);
  [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint ms, out IntPtr result);
  // WM_NULL with a timeout: false when the window's thread does not process messages.
  public static bool Responds(IntPtr h, uint ms) { IntPtr r; return SendMessageTimeout(h, 0, IntPtr.Zero, IntPtr.Zero, 2, ms, out r) != IntPtr.Zero; }
  [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
  public static void Key(byte vk, bool up) { keybd_event(vk, (byte)MapVirtualKey(vk, 0), up ? 2u : 0u, IntPtr.Zero); }
  [DllImport("user32.dll")] static extern short GetAsyncKeyState(int vk);
  // Sends a key-up for every modifier still logically down; returns their codes.
  public static string ReleaseHeld() {
    string s = "";
    int[] vks = { 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C };
    foreach (int vk in vks) if (GetAsyncKeyState(vk) < 0) { Key((byte)vk, true); s += vk.ToString("X") + " "; }
    return s.Trim();
  }
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
  // Fixed size in logical pixels, so the PDF page lays out the same every time.
  public static void Resize(IntPtr h, int w, int hh) {
    double scale = GetDpiForWindow(h) / 96.0;
    MoveWindow(h, 0, 0, (int)(w * scale), (int)(hh * scale), true);
  }
  // dx, dy in logical (96 DPI) pixels from the window's top left corner.
  public static void DoubleClick(IntPtr h, int dx, int dy) {
    double scale = GetDpiForWindow(h) / 96.0;
    R r; GetWindowRect(h, out r);
    SetCursorPos(r.L + (int)(dx * scale), r.T + (int)(dy * scale));
    for (int i = 0; i < 2; i++) { mouse_event(2, 0, 0, 0, IntPtr.Zero); mouse_event(4, 0, 0, 0, IntPtr.Zero); System.Threading.Thread.Sleep(60); }
  }
}
"@
# Real pixel coordinates for screenshots on scaled displays.
[void][LtE2e]::SetProcessDPIAware()

# Only the window's own pixels (no full-screen captures: other windows are the user's).
function Save-Screenshot([IntPtr]$Hwnd, [string]$Path) {
  $r = [LtE2e]::Visible($Hwnd)
  $bmp = New-Object System.Drawing.Bitmap ($r.Ri - $r.L), ($r.B - $r.T)
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}

# Keys go to whatever window is in front: check it is the one meant, or send nothing.
function Assert-Foreground([IntPtr]$Hwnd, [string]$What) {
  $fg = [LtE2e]::GetForegroundWindow()
  if ($fg -ne $Hwnd) { throw "the $What ($Hwnd) is not in the foreground ($fg); not sending keys" }
}

# AppActivate can pick another window of the process (or Explorer's TabProxyWindow), and
# SetForegroundWindow is refused while another program is in front: then a click on the
# page itself, after checking the page is what is under that point.
function Show-TestPage($Page) {
  [void]$ws.AppActivate($Page.Id); Start-Sleep -Milliseconds 300
  if ([LtE2e]::GetForegroundWindow() -ne $Page.MainWindowHandle) {
    [void][LtE2e]::SetForegroundWindow($Page.MainWindowHandle); Start-Sleep -Milliseconds 300
  }
  if ([LtE2e]::GetForegroundWindow() -ne $Page.MainWindowHandle) {
    [void][LtE2e]::ClickCenter($Page.MainWindowHandle); Start-Sleep -Milliseconds 300
  }
  Assert-Foreground $Page.MainWindowHandle "test page"
}

# The popup is a WebView2 (Chromium) window, which builds its accessibility tree only
# after the first UIA query, so the text is read a few times before giving up.
function Get-PopupText([IntPtr]$Hwnd, [string]$Want = $expected[0]) {
  $root = [System.Windows.Automation.AutomationElement]::FromHandle($Hwnd)
  for ($try = 0; $try -lt 10; $try++) {
    $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
      [System.Windows.Automation.Condition]::TrueCondition)
    $names = @($all | ForEach-Object { $_.Current.Name } | Where-Object { $_ } | Select-Object -Unique)
    if ($names | Where-Object { $_ -eq $Want }) { return $names }
    Start-Sleep -Milliseconds 200
  }
  return $names
}

# The popup's highest (D046 supplement), in physical pixels, for the monitor it is on.
function Get-MaxHeight([IntPtr]$Hwnd) {
  $scale = [LtE2e]::GetDpiForWindow($Hwnd) / 96.0
  $area = [System.Windows.Forms.Screen]::FromHandle($Hwnd).WorkingArea.Height
  [Math]::Min([Math]::Round(520 * $scale), [Math]::Floor($area * 0.6))
}

# Unfolds every folded section ("more", "specialist senses") through UI Automation.
function Expand-All([IntPtr]$Hwnd) {
  $root = [System.Windows.Automation.AutomationElement]::FromHandle($Hwnd)
  $folded = [string][char]0x25B8
  $count = 0
  for ($n = 0; $n -lt 8; $n++) {
    $button = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
      [System.Windows.Automation.Condition]::TrueCondition) |
      Where-Object { $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -and $_.Current.Name.StartsWith($folded) } |
      Select-Object -First 1
    if (-not $button) { break }
    $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    $count++
    Start-Sleep -Milliseconds 300
  }
  $count
}

# Whether some element of the popup scrolls vertically.
function Test-Scrollable([IntPtr]$Hwnd) {
  $root = [System.Windows.Automation.AutomationElement]::FromHandle($Hwnd)
  foreach ($e in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
      [System.Windows.Automation.Condition]::TrueCondition)) {
    $pattern = $null
    if ($e.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern, [ref]$pattern) -and
        $pattern.Current.VerticallyScrollable) { return $true }
  }
  $false
}

function Get-Height([IntPtr]$Hwnd) { $r = [LtE2e]::Visible($Hwnd); $r.B - $r.T }

$failures = 0
$heights = @()
Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
$log = Join-Path $OutDir "app.log"
if ($ResumeDelayMs -gt 0) { $env:LEXTRAIL_DEBUG_RESUME_DELAY_MS = "$ResumeDelayMs" }
if ($ForceAttach) { $env:LEXTRAIL_DEBUG_FORCE_ATTACH = "1" }
$configDir = Join-Path $OutDir ("config-" + [guid]::NewGuid())
$dataDir = Join-Path $OutDir ("data-" + [guid]::NewGuid())
if ($DoubleCopy -or $DoubleCopySetting) {
  New-Item -ItemType Directory -Force $configDir | Out-Null
  Set-Content -Path (Join-Path $configDir "settings.toml") -Value "[hotkeys]`ndouble_ctrl_c = true" -Encoding ASCII
}
$env:LEXTRAIL_DEBUG_CONFIG_DIR = $configDir
$env:LEXTRAIL_DEBUG_DATA_DIR = $dataDir
Start-Process $App -RedirectStandardError $log -WindowStyle Hidden
Remove-Item Env:LEXTRAIL_DEBUG_RESUME_DELAY_MS, Env:LEXTRAIL_DEBUG_CONFIG_DIR, Env:LEXTRAIL_DEBUG_DATA_DIR, Env:LEXTRAIL_DEBUG_FORCE_ATTACH -ErrorAction SilentlyContinue
Start-Sleep 4

$edge = if ($Chrome) { "$env:ProgramFiles\Google\Chrome\Application\chrome.exe" }
  else { "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe" }
$profileDir = Join-Path $env:TEMP ("lextrail-e2e-edge-" + [guid]::NewGuid())
$pageFile = if ($Word -eq "run") { "run-word.html" } else { "list-item.html" }
$url = "file:///" + (Join-Path $here $pageFile).Replace([char]92, [char]47)
$private = if ($NoInPrivate) { @("--disable-sync", "--no-default-browser-check") }
  elseif ($Chrome) { @("--incognito", "--no-default-browser-check") } else { @("--inprivate") }
if ($Pdf) {
  $pdfPath = Join-Path $OutDir "lextrail-e2e.pdf"
  Remove-Item $pdfPath -ErrorAction SilentlyContinue
  $printProfile = Join-Path $env:TEMP ("lextrail-e2e-print-" + [guid]::NewGuid())
  Start-Process $edge -ArgumentList "--headless", "--disable-gpu", "--user-data-dir=$printProfile",
    "--print-to-pdf=$pdfPath", "--no-pdf-header-footer", $url -Wait
  if (-not (Test-Path $pdfPath)) { throw "could not print the test page to PDF" }
  $pdfUrl = "file:///" + $pdfPath.Replace([char]92, [char]47)
  Start-Process $edge -ArgumentList (@("--user-data-dir=$profileDir", "--no-first-run") + $private + @("--new-window", $pdfUrl)) | Out-Null
} else {
  Start-Process $edge -ArgumentList (@("--user-data-dir=$profileDir", "--no-first-run") + $private + @("--disable-features=Translate,msTranslate", "--app=$url")) | Out-Null
}
Start-Sleep 6
$ws = New-Object -ComObject WScript.Shell
$page = Get-Process $browserProc | Where-Object { $_.MainWindowTitle -like "lextrail-e2e*" } | Select-Object -First 1
# Where "launch" is, in logical pixels from the window's top left corner.
$word = if ($Pdf) { @(283, 230) } elseif ($Word -eq "run") { @(90, 110) } else { @(160, 110) }
if ($Pdf -and $page) { [LtE2e]::Resize($page.MainWindowHandle, 1050, 1000); Start-Sleep 1 }

try {
  if (-not $page) { throw "Edge test window not found" }
  for ($i = 1; $i -le $Lookups; $i++) {
    Show-TestPage $page; Start-Sleep -Milliseconds 200
    # Close any Edge bubble (e.g. "translate this page?") that would hold focus.
    $ws.SendKeys("{ESC}"); Start-Sleep -Milliseconds 300
    Assert-Foreground $page.MainWindowHandle "test page"
    [LtE2e]::DoubleClick($page.MainWindowHandle, $word[0], $word[1]); Start-Sleep -Milliseconds 400
    Assert-Foreground $page.MainWindowHandle "test page"
    Save-Screenshot $page.MainWindowHandle (Join-Path $OutDir "page-$i.png")
    $heldText = @()
    if ($HoldMs -gt 0) {
      # Left Ctrl, left Alt, X down and up; Ctrl+Alt stay down for $HoldMs.
      [LtE2e]::Key(0xA2, $false); Start-Sleep -Milliseconds 30; [LtE2e]::Key(0xA4, $false)
      Start-Sleep -Milliseconds 30; [LtE2e]::Key(0x58, $false); [LtE2e]::Key(0x58, $true)
      Start-Sleep -Milliseconds ([Math]::Max(0, $HoldMs - 400))
      $early = [LtE2e]::FindWindow("Tauri Window", "LexTrail")
      if ($early -ne [IntPtr]::Zero -and [LtE2e]::IsWindowVisible($early)) {
        Save-Screenshot $early (Join-Path $OutDir "held-$i.png")
        $heldText = Get-PopupText $early $(if ($HoldMs -le 3000) { $holdHint } else { $expected[0] })
      }
      Start-Sleep -Milliseconds ([Math]::Min(400, $HoldMs))
      [LtE2e]::Key(0xA4, $true); [LtE2e]::Key(0xA2, $true)
      Start-Sleep -Milliseconds 900
    } elseif ($DoubleCopy) {
      # Left Ctrl + C, released, twice.
      for ($c = 0; $c -lt 2; $c++) {
        if ($c) { Start-Sleep -Milliseconds $CopyGapMs; Assert-Foreground $page.MainWindowHandle "test page" }
        [LtE2e]::Key(0xA2, $false); [LtE2e]::Key(0x43, $false); [LtE2e]::Key(0x43, $true); [LtE2e]::Key(0xA2, $true)
      }
      Start-Sleep -Milliseconds 900
    } else {
      $ws.SendKeys("^%x")
      if ($PressTwiceMs -gt 0) {
        Start-Sleep -Milliseconds $PressTwiceMs
        # Still the page (the first press's popup does not take focus while capturing).
        $fg = [LtE2e]::GetForegroundWindow()
        if ($fg -eq $page.MainWindowHandle -or $fg -eq [LtE2e]::FindWindow("Tauri Window", "LexTrail")) { $ws.SendKeys("^%x") }
        else { "  second press not sent: foreground is another window ($fg)" }
      }
      Start-Sleep -Milliseconds 900
    }

    $popup = [LtE2e]::FindWindow("Tauri Window", "LexTrail")
    # The popup's window belongs to the app's UI thread: if that thread hangs, so does this.
    $responds = $popup -ne [IntPtr]::Zero -and [LtE2e]::Responds($popup, 2000)
    $visible = $popup -ne [IntPtr]::Zero -and [LtE2e]::IsWindowVisible($popup)
    $focused = $visible -and ([LtE2e]::GetForegroundWindow() -eq $popup)
    $text = @(); $shot = ""
    $height = 0; $expandOk = $true
    if ($visible) {
      $shot = Join-Path $OutDir "popup-$i.png"; Save-Screenshot $popup $shot
      $text = Get-PopupText $popup
      $height = Get-Height $popup; $heights += $height
      if ($Expand) {
        $unfolded = Expand-All $popup
        Start-Sleep -Milliseconds 300
        $tall = Get-Height $popup; $max = Get-MaxHeight $popup
        $scrolls = Test-Scrollable $popup
        $expandOk = $unfolded -gt 0 -and [Math]::Abs($tall - $max) -le 2 -and $scrolls
        Save-Screenshot $popup (Join-Path $OutDir "popup-$i-expanded.png")
        "  expanded $unfolded section(s): height $height -> $tall (highest $max), scrolls=$scrolls" + $(if ($expandOk) { "" } else { "  (FAIL)" })
      }
    }
    $missing = @($expected | Where-Object { $text -notcontains $_ })
    # Held for over a second (and read before the 3 s limit): the hint must be up.
    $hintOk = -not ($HoldMs -gt 1400 -and $HoldMs -le 3000) -or ($heldText -contains $holdHint)
    $ok = $responds -and $visible -and $focused -and $missing.Count -eq 0 -and $hintOk -and $expandOk
    # Nothing may show; the app must still answer if its window exists.
    if ($ExpectIgnored) { $missing = @(); $ok = -not $visible -and ($popup -eq [IntPtr]::Zero -or $responds) }
    if (-not $ok) { $failures++ }
    "lookup ${i}: " + $(if ($ok) { "PASS" } else { "FAIL" }) + " (responds=$responds visible=$visible focused=$focused height=$height)"
    "  popup text: " + ($text -join " | ")
    if ($missing.Count) { "  missing: " + ($missing -join " | ") }
    if ($ExpectIgnored -and $visible) { "  a popup showed although none was expected" }
    if ($HoldMs -gt 0) { "  while held: " + ($heldText -join " | ") + $(if ($hintOk) { "" } else { "  (hint missing)" }) }
    if ($shot) { "  screenshot: $shot" }
    if ($popup -ne [IntPtr]::Zero -and -not $responds) { "  the app's UI thread does not respond (hung); stopping"; break }
    if ($visible -and -not $ExpectIgnored) {
      # Close it, then it must be gone and the page in front again.
      $closeSent = $false
      $how = if ($CloseBy -eq "ClickThenEsc") { if ($i % 2) { "Click" } else { "Esc" } } else { $CloseBy }
      if ($how -eq "Esc") {
        # Esc only to the popup itself.
        if ([LtE2e]::GetForegroundWindow() -eq $popup) { $ws.SendKeys("{ESC}"); $closeSent = $true }
        else { "  popup not in front: Esc not sent" }
      } else {
        # A click on the page, only where the page itself shows (not the popup).
        $closeSent = [LtE2e]::ClickCenter($page.MainWindowHandle)
        if (-not $closeSent) { "  the page's centre is covered: click not sent" }
      }
      Start-Sleep -Milliseconds 600
      $stillVisible = [LtE2e]::IsWindowVisible($popup)
      $backToPage = [LtE2e]::GetForegroundWindow() -eq $page.MainWindowHandle
      $closedOk = $closeSent -and -not $stillVisible -and $backToPage
      "  closed by ${how}: " + $(if ($closedOk) { "ok" } else { "FAIL" }) + " (popup visible=$stillVisible, page in front=$backToPage)"
      if (-not $closedOk) { $failures++ }
    }
    Start-Sleep -Milliseconds $PauseMs
  }
} finally {
  # A modifier left down would act on whatever the user does next: release it, and fail.
  $held = [LtE2e]::ReleaseHeld()
  if ($held) { "modifier keys were still down at the end (released): $held"; $failures++ }
  Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
  Get-CimInstance Win32_Process -Filter "Name='$browserProc.exe'" |
    Where-Object { $_.CommandLine -like "*$profileDir*" } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
"app log: $log"
Get-Content $log | Select-String "capture #|bring to front|WindowReady|resume delayed|Captured|activated|double copy" | ForEach-Object { "  " + $_.Line }
# The same content must give the same height every time (the first popup was taller).
if ($heights.Count -gt 1) {
  $spread = ($heights | Measure-Object -Maximum -Minimum)
  $sameOk = $spread.Maximum - $spread.Minimum -le 1
  "popup heights: " + ($heights -join ", ") + $(if ($sameOk) { "" } else { "  (FAIL: not the same)" })
  if (-not $sameOk) { $failures++ }
}
# -ForceAttach: every activation took the fallback, and the focus did not bounce.
$fronts = @(Get-Content $log | Select-String "popup: bring to front: (\w+)" | ForEach-Object { $_.Matches[0].Groups[1].Value })
$refocus = @(Get-Content $log | Select-String "focusing the page").Count
"activations: " + ($fronts -join ", ") + "; page focused by the app: $refocus time(s)"
if ($ForceAttach -and -not $ExpectIgnored) {
  $attachOk = $fronts.Count -gt 0 -and $fronts[0] -eq "Attached" -and
    @($fronts | Where-Object { $_ -notin "Attached", "AlreadyInFront" }).Count -eq 0 -and $refocus -le $fronts.Count
  if (-not $attachOk) { "  (FAIL: expected only Attached activations and at most one refocus each)"; $failures++ }
}
# Lookups started by a double Ctrl+C: one per lookup with -DoubleCopy, none otherwise.
$triggered = @(Get-Content $log | Select-String "double copy: lookup triggered").Count
$wantTriggered = if ($DoubleCopy -and -not $ExpectIgnored) { $Lookups } else { 0 }
"double copy lookups: $triggered (expected $wantTriggered)" + $(if ($triggered -eq $wantTriggered) { "" } else { "  (FAIL)" })
if ($triggered -ne $wantTriggered) { $failures++ }
# Lookup history, in the scratch data directory: the first lookup recorded, the repeats
# (same text and sentence within 30 s) not. Keys held over 3 s capture nothing.
if ($ExpectIgnored) {
  $history = @(Get-Content $log | Select-String "capture #\d+: history (\w+)")
  "history: " + $history.Count + " lookup(s) recorded (expected 0)" + $(if ($history.Count) { "  (FAIL)" } else { "" })
  if ($history.Count) { $failures++ }
} elseif ($HoldMs -le 3000) {
  $history = @(Get-Content $log | Select-String "capture #\d+: history (\w+)" | ForEach-Object { $_.Matches[0].Groups[1].Value })
  $historyOk = $history.Count -gt 0 -and $history[0] -eq "recorded" -and
    @($history | Select-Object -Skip 1 | Where-Object { $_ -ne "repeat" }).Count -eq 0 -and
    (Test-Path (Join-Path $dataDir "lextrail.db"))
  "history: " + ($history -join ", ") + " in $dataDir" + $(if ($historyOk) { "" } else { "  (FAIL)" })
  if (-not $historyOk) { $failures++ }
}
if ($failures) { "FAILED: $failures lookup(s)"; exit 1 }
"all lookups passed"
