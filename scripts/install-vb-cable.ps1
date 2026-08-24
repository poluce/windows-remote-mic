# One-click VB-CABLE install helper (Windows).
# Downloads the official VB-Audio driver pack, verifies its SHA-256, extracts
# it, and launches the official setup (UAC prompt appears).
param(
    [string]$VbCableUrl = "https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip"
)

$ErrorActionPreference = "Stop"

$hashFile = Join-Path $PSScriptRoot "vb-cable.sha256"
$expectedHash = ""
if (Test-Path $hashFile) {
    $expectedHash = (
        Get-Content -Path $hashFile |
            Where-Object { $_ -notmatch '^\s*#' -and $_.Trim() -ne '' } |
            Join-String
    ).Trim()
}

if (-not $expectedHash) {
    throw "VB-CABLE SHA-256 is not pinned (scripts/vb-cable.sha256 is empty). Refusing to install."
}

$local = Join-Path $env:LOCALAPPDATA "RemoteMic\RC003"
New-Item -ItemType Directory -Force -Path $local | Out-Null

$zip = Join-Path $local "VBCABLE_Driver_Pack45.zip"
$extract = Join-Path $local "vbcable"

Write-Output "Downloading $VbCableUrl ..."
Invoke-WebRequest -Uri $VbCableUrl -OutFile $zip

Write-Output "Verifying SHA-256 ..."
$actualHash = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
if ($actualHash -ne $expectedHash.ToLowerInvariant()) {
    Remove-Item -Path $zip -Force -ErrorAction SilentlyContinue
    throw "SHA-256 mismatch. Actual=$actualHash Expected=$expectedHash. Deleted downloaded file."
}
Write-Output "Hash OK."

if (Test-Path $extract) { Remove-Item -Recurse -Force $extract }
Expand-Archive -Path $zip -DestinationPath $extract -Force

$setup = Get-ChildItem -Path $extract -Recurse -Filter "VBCABLE_Setup_x64.exe" | Select-Object -First 1
if (-not $setup) { throw "VBCABLE_Setup_x64.exe not found in archive" }

Write-Output "Starting official installer (confirm UAC)..."
Start-Process -FilePath $setup.FullName -ArgumentList "/S" -Verb RunAs
Write-Output "Installer launched. When it finishes, click Re-detect in the app."
