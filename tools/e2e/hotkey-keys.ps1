# Shared by the e2e scripts (dot-source it): a hotkey string such as "Ctrl+Alt+X" or "F8"
# as the virtual keys to press, left modifiers first, main key last. Only what the tests
# use: Ctrl, Alt, Shift and a letter, a digit, Space or F1-F24.
function Get-HotkeyVks([string]$Hotkey) {
  # @(...): a single part would otherwise be a string, and [-1] its last character.
  $parts = @($Hotkey.Split("+") | ForEach-Object { $_.Trim() })
  $vks = @()
  for ($i = 0; $i -lt $parts.Count - 1; $i++) {
    switch ($parts[$i].ToLowerInvariant()) {
      "ctrl" { $vks += 0xA2 }
      "alt" { $vks += 0xA4 }
      "shift" { $vks += 0xA0 }
      default { throw "unsupported modifier '$($parts[$i])' in $Hotkey" }
    }
  }
  $key = $parts[$parts.Count - 1].ToUpperInvariant()
  if ($key -match "^[A-Z0-9]$") { $vks += [int][char]$key }
  elseif ($key -match "^F([1-9]|1[0-9]|2[0-4])$") { $vks += 0x70 + [int]$Matches[1] - 1 }
  elseif ($key -eq "SPACE") { $vks += 0x20 }
  else { throw "unsupported key '$key' in $Hotkey" }
  , $vks
}

# Writes the scratch settings file with this lookup hotkey (and extra lines for [hotkeys]).
function Write-HotkeySettings([string]$ConfigDir, [string]$Hotkey, [string]$Extra = "") {
  New-Item -ItemType Directory -Force $ConfigDir | Out-Null
  Set-Content -Path (Join-Path $ConfigDir "settings.toml") -Encoding ASCII `
    -Value "[hotkeys]`nlookup = `"$Hotkey`"`n$Extra"
}
