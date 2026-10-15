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

It moves the mouse and sends keys: leave the machine alone while it runs. Exits with 1 if
any check fails.

.EXAMPLE
pwsh tools/e2e/popup-e2e.ps1 -App target\debug\lextrail-desktop.exe
#>
param(
  [string]$App = "target\debug\lextrail-desktop.exe",
  [int]$Lookups = 2,
  [string]$OutDir = (Join-Path $env:TEMP "lextrail-e2e")
)
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$App = (Resolve-Path $App).Path
New-Item -ItemType Directory -Force $OutDir | Out-Null
$expected = @("launch", "launch missiles at ships.")

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
function Get-PopupText([IntPtr]$Hwnd) {
  $root = [System.Windows.Automation.AutomationElement]::FromHandle($Hwnd)
  for ($try = 0; $try -lt 10; $try++) {
    $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
      [System.Windows.Automation.Condition]::TrueCondition)
    $names = @($all | ForEach-Object { $_.Current.Name } | Where-Object { $_ } | Select-Object -Unique)
    if ($names | Where-Object { $_ -eq $expected[0] }) { return $names }
    Start-Sleep -Milliseconds 200
  }
  return $names
}

$failures = 0
Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
$log = Join-Path $OutDir "app.log"
Start-Process $App -RedirectStandardError $log -WindowStyle Hidden
Start-Sleep 4

$edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe"
$profileDir = Join-Path $env:TEMP ("lextrail-e2e-edge-" + [guid]::NewGuid())
$url = "file:///" + (Join-Path $here "list-item.html").Replace([char]92, [char]47)
Start-Process $edge -ArgumentList "--user-data-dir=$profileDir", "--no-first-run", "--inprivate", "--disable-features=Translate,msTranslate", "--app=$url" | Out-Null
Start-Sleep 6
$ws = New-Object -ComObject WScript.Shell
$page = Get-Process msedge | Where-Object { $_.MainWindowTitle -like "lextrail-e2e*" } | Select-Object -First 1

try {
  if (-not $page) { throw "Edge test window not found" }
  for ($i = 1; $i -le $Lookups; $i++) {
    [void]$ws.AppActivate($page.Id); Start-Sleep -Milliseconds 500
    # Close any Edge bubble (e.g. "translate this page?") that would hold focus.
    $ws.SendKeys("{ESC}"); Start-Sleep -Milliseconds 300
    [LtE2e]::DoubleClick($page.MainWindowHandle, 160, 110); Start-Sleep -Milliseconds 400
    Save-Screenshot $page.MainWindowHandle (Join-Path $OutDir "page-$i.png")
    $ws.SendKeys("^%x"); Start-Sleep -Milliseconds 900

    $popup = [LtE2e]::FindWindow("Tauri Window", "LexTrail")
    $visible = $popup -ne [IntPtr]::Zero -and [LtE2e]::IsWindowVisible($popup)
    $focused = $visible -and ([LtE2e]::GetForegroundWindow() -eq $popup)
    $text = @(); $shot = ""
    if ($visible) {
      $shot = Join-Path $OutDir "popup-$i.png"; Save-Screenshot $popup $shot
      $text = Get-PopupText $popup
    }
    $missing = @($expected | Where-Object { $text -notcontains $_ })
    $ok = $visible -and $focused -and $missing.Count -eq 0
    if (-not $ok) { $failures++ }
    "lookup ${i}: " + $(if ($ok) { "PASS" } else { "FAIL" }) + " (visible=$visible focused=$focused)"
    "  popup text: " + ($text -join " | ")
    if ($missing.Count) { "  missing: " + ($missing -join " | ") }
    if ($shot) { "  screenshot: $shot" }
    $ws.SendKeys("{ESC}"); Start-Sleep -Milliseconds 600
  }
} finally {
  Get-Process lextrail-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
  Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" |
    Where-Object { $_.CommandLine -like "*$profileDir*" } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
"app log: $log"
if ($failures) { "FAILED: $failures lookup(s)"; exit 1 }
"all lookups passed"
