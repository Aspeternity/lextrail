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

The app reads its settings and writes its lookup history in fresh directories under
-OutDir (debug builds only: LEXTRAIL_DEBUG_CONFIG_DIR, LEXTRAIL_DEBUG_DATA_DIR), never the
user's own; release builds are refused for that reason. The lookups recorded are checked
against the popup: the first is recorded, a repeat within 30 s is not.

It moves the mouse and sends keys: leave the machine alone while it runs. Exits with 1 if
any check fails.

.EXAMPLE
pwsh tools/e2e/popup-e2e.ps1 -App target\debug\lextrail-desktop.exe
pwsh tools/e2e/popup-e2e.ps1 -App target\debug\lextrail-desktop.exe -Pdf
pwsh tools/e2e/popup-e2e.ps1 -App target\debug\lextrail-desktop.exe -Pdf -HoldMs 2000
pwsh tools/e2e/popup-e2e.ps1 -App target\debug\lextrail-desktop.exe -Lookups 3 -PauseMs 7000 -ResumeDelayMs 300
#>
param(
  [string]$App = "target\debug\lextrail-desktop.exe",
  [int]$Lookups = 2,
  [string]$OutDir = (Join-Path $env:TEMP "lextrail-e2e"),
  [switch]$Pdf,
  [int]$HoldMs = 0,
  [int]$PauseMs = 600,
  [int]$ResumeDelayMs = 0
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$App = (Resolve-Path $App).Path
# Only debug builds honour the directory overrides below.
if ($App -like "*\release\*") { throw "use a debug build: release builds would use the real settings and history" }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$expected = if ($Pdf) { @("launch") } else { @("launch", "launch missiles at ships.") }
# The clipboard fallback gives up after 3 s of held keys (D043).
# Built from code points: Windows PowerShell reads a script without BOM in the ANSI code page.
function Text([int[]]$CodePoints) { -join ($CodePoints | ForEach-Object { [char]$_ }) }
# The popup texts "release the hotkey and try again" (D043, R3-P2) and "release the
# hotkey to capture..." (R3-P4).
$keysHeldTitle = Text 0x8BF7, 0x677E, 0x5F00, 0x5FEB, 0x6377, 0x952E, 0x540E, 0x518D, 0x8BD5
$holdHint = Text 0x677E, 0x5F00, 0x5FEB, 0x6377, 0x952E, 0x540E, 0x53D6, 0x8BCD, 0x2026
if ($HoldMs -gt 3000) { $expected = @($keysHeldTitle) }

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class LtE2e {
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Ri, B; }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool r);
  [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint ms, out IntPtr result);
  // WM_NULL with a timeout: false when the window's thread does not process messages.
  public static bool Responds(IntPtr h, uint ms) { IntPtr r; return SendMessageTimeout(h, 0, IntPtr.Zero, IntPtr.Zero, 2, ms, out r) != IntPtr.Zero; }
  [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
  public static void Key(byte vk, bool up) { keybd_event(vk, (byte)MapVirtualKey(vk, 0), up ? 2u : 0u, IntPtr.Zero); }
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

function Save-Screenshot([IntPtr]$Hwnd, [string]$Path) {
  $r = New-Object LtE2e+R; [void][LtE2e]::GetWindowRect($Hwnd, [ref]$r)
  $bmp = New-Object System.Drawing.Bitmap ($r.Ri - $r.L), ($r.B - $r.T)
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
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

$failures = 0
Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
$log = Join-Path $OutDir "app.log"
if ($ResumeDelayMs -gt 0) { $env:LEXTRAIL_DEBUG_RESUME_DELAY_MS = "$ResumeDelayMs" }
$configDir = Join-Path $OutDir ("config-" + [guid]::NewGuid())
$dataDir = Join-Path $OutDir ("data-" + [guid]::NewGuid())
$env:LEXTRAIL_DEBUG_CONFIG_DIR = $configDir
$env:LEXTRAIL_DEBUG_DATA_DIR = $dataDir
Start-Process $App -RedirectStandardError $log -WindowStyle Hidden
Remove-Item Env:LEXTRAIL_DEBUG_RESUME_DELAY_MS, Env:LEXTRAIL_DEBUG_CONFIG_DIR, Env:LEXTRAIL_DEBUG_DATA_DIR -ErrorAction SilentlyContinue
Start-Sleep 4

$edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
$profileDir = Join-Path $env:TEMP ("lextrail-e2e-edge-" + [guid]::NewGuid())
$url = "file:///" + (Join-Path $here "list-item.html").Replace([char]92, [char]47)
if ($Pdf) {
  $pdfPath = Join-Path $OutDir "lextrail-e2e.pdf"
  Remove-Item $pdfPath -ErrorAction SilentlyContinue
  $printProfile = Join-Path $env:TEMP ("lextrail-e2e-print-" + [guid]::NewGuid())
  Start-Process $edge -ArgumentList "--headless", "--disable-gpu", "--user-data-dir=$printProfile",
    "--print-to-pdf=$pdfPath", "--no-pdf-header-footer", $url -Wait
  if (-not (Test-Path $pdfPath)) { throw "could not print the test page to PDF" }
  $pdfUrl = "file:///" + $pdfPath.Replace([char]92, [char]47)
  Start-Process $edge -ArgumentList "--user-data-dir=$profileDir", "--no-first-run", "--inprivate", "--new-window", $pdfUrl | Out-Null
} else {
  Start-Process $edge -ArgumentList "--user-data-dir=$profileDir", "--no-first-run", "--inprivate", "--disable-features=Translate,msTranslate", "--app=$url" | Out-Null
}
Start-Sleep 6
$ws = New-Object -ComObject WScript.Shell
$page = Get-Process msedge | Where-Object { $_.MainWindowTitle -like "lextrail-e2e*" } | Select-Object -First 1
# Where "launch" is, in logical pixels from the window's top left corner.
$word = if ($Pdf) { @(283, 230) } else { @(160, 110) }
if ($Pdf -and $page) { [LtE2e]::Resize($page.MainWindowHandle, 1050, 1000); Start-Sleep 1 }

try {
  if (-not $page) { throw "Edge test window not found" }
  for ($i = 1; $i -le $Lookups; $i++) {
    [void]$ws.AppActivate($page.Id); Start-Sleep -Milliseconds 500
    # Close any Edge bubble (e.g. "translate this page?") that would hold focus.
    $ws.SendKeys("{ESC}"); Start-Sleep -Milliseconds 300
    [LtE2e]::DoubleClick($page.MainWindowHandle, $word[0], $word[1]); Start-Sleep -Milliseconds 400
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
    } else {
      $ws.SendKeys("^%x"); Start-Sleep -Milliseconds 900
    }

    $popup = [LtE2e]::FindWindow("Tauri Window", "LexTrail")
    # The popup's window belongs to the app's UI thread: if that thread hangs, so does this.
    $responds = $popup -ne [IntPtr]::Zero -and [LtE2e]::Responds($popup, 2000)
    $visible = $popup -ne [IntPtr]::Zero -and [LtE2e]::IsWindowVisible($popup)
    $focused = $visible -and ([LtE2e]::GetForegroundWindow() -eq $popup)
    $text = @(); $shot = ""
    if ($visible) {
      $shot = Join-Path $OutDir "popup-$i.png"; Save-Screenshot $popup $shot
      $text = Get-PopupText $popup
    }
    $missing = @($expected | Where-Object { $text -notcontains $_ })
    # Held for over a second (and read before the 3 s limit): the hint must be up.
    $hintOk = -not ($HoldMs -gt 1400 -and $HoldMs -le 3000) -or ($heldText -contains $holdHint)
    $ok = $responds -and $visible -and $focused -and $missing.Count -eq 0 -and $hintOk
    if (-not $ok) { $failures++ }
    "lookup ${i}: " + $(if ($ok) { "PASS" } else { "FAIL" }) + " (responds=$responds visible=$visible focused=$focused)"
    "  popup text: " + ($text -join " | ")
    if ($missing.Count) { "  missing: " + ($missing -join " | ") }
    if ($HoldMs -gt 0) { "  while held: " + ($heldText -join " | ") + $(if ($hintOk) { "" } else { "  (hint missing)" }) }
    if ($shot) { "  screenshot: $shot" }
    if (-not $responds) { "  the app's UI thread does not respond (hung); stopping"; break }
    $ws.SendKeys("{ESC}"); Start-Sleep -Milliseconds $PauseMs
  }
} finally {
  Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
  Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" |
    Where-Object { $_.CommandLine -like "*$profileDir*" } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
"app log: $log"
Get-Content $log | Select-String "capture #|bring to front|WindowReady|resume delayed|Captured|activated" | ForEach-Object { "  " + $_.Line }
# Lookup history, in the scratch data directory: the first lookup recorded, the repeats
# (same text and sentence within 30 s) not. Keys held over 3 s capture nothing.
if ($HoldMs -le 3000) {
  $history = @(Get-Content $log | Select-String "capture #\d+: history (\w+)" | ForEach-Object { $_.Matches[0].Groups[1].Value })
  $historyOk = $history.Count -gt 0 -and $history[0] -eq "recorded" -and
    @($history | Select-Object -Skip 1 | Where-Object { $_ -ne "repeat" }).Count -eq 0 -and
    (Test-Path (Join-Path $dataDir "lextrail.db"))
  "history: " + ($history -join ", ") + " in $dataDir" + $(if ($historyOk) { "" } else { "  (FAIL)" })
  if (-not $historyOk) { $failures++ }
}
if ($failures) { "FAILED: $failures lookup(s)"; exit 1 }
"all lookups passed"
