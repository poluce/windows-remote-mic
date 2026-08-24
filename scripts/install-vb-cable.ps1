# One-click VB-CABLE install helper (Windows).
# Downloads the official VB-Audio driver pack, extracts it, and launches the
# official setup (UAC prompt appears). NOTE: full SHA-256 pinning to be added
# before release; only use this on machines you trust.
param(
    [string]$VbCableUrl = "https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip"
)

$ErrorActionPreference = "Stop"
$local = Join-Path $env:LOCALAPPDATA "RemoteMic\RC003"
New-Item -ItemType Directory -Force -Path $local | Out-Null

$zip = Join-Path $local "VBCABLE_Driver_Pack45.zip"
$extract = Join-Path $local "vbcable"

Write-Output "downloading $VbCableUrl ..."
Invoke-WebRequest -Uri $VbCableUrl -OutFile $zip

if (Test-Path $extract) { Remove-Item -Recurse -Force $extract }
Expand-Archive -Path $zip -DestinationPath $extract -Force

$setup = Get-ChildItem -Path $extract -Recurse -Filter "VBCABLE_Setup_x64.exe" | Select-Object -First 1
if (-not $setup) { throw "VBCABLE_Setup_x64.exe not found in archive" }

Write-Output "starting official installer (confirm UAC)..."
Start-Process -FilePath $setup.FullName -ArgumentList "/S" -Verb RunAs
Write-Output "launched installer; once finished, click 重新检测 in the app."
