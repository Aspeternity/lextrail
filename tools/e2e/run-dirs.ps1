# Scratch folders of one test run (the settings and history the app was started with, the
# browser and VS Code profiles) are removed when the run ends, so that they do not pile up in
# %TEMP%. Logs and screenshots in -OutDir are kept. Dot-sourced by the e2e scripts.

# Stops the processes named $Name that have $Dir in their command line (a browser or VS Code
# started with a scratch profile) and waits for them to exit, so that the folder can be removed.
function Stop-DirUsers([string]$Name, [string]$Dir, [int]$Seconds = 15) {
  if (-not $Dir) { return }
  $ids = @(Get-CimInstance Win32_Process -Filter "Name = '$Name'" |
    Where-Object { $_.CommandLine -like "*$Dir*" } | ForEach-Object { $_.ProcessId })
  foreach ($id in $ids) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
  $deadline = (Get-Date).AddSeconds($Seconds)
  while ((Get-Date) -lt $deadline -and
    @($ids | Where-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue }).Count) {
    Start-Sleep -Milliseconds 200
  }
}

# Removes the folders, trying again for a while when a file is still in use (a process that
# has only just exited). A folder that cannot be removed is reported and left in place.
function Remove-RunDirs([string[]]$Dirs, [int]$Tries = 10) {
  foreach ($dir in $Dirs) {
    if (-not $dir) { continue }
    for ($i = 0; $i -lt $Tries -and (Test-Path $dir); $i++) {
      if ($i) { Start-Sleep -Milliseconds 500 }
      Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path $dir) { Write-Host "could not remove (left in place): $dir" }
  }
}
