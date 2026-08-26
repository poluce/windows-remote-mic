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
    $lines = Get-Content -Path $hashFile
    $expectedHash = (($lines | Where-Object { $_ -notmatch '^\s*#' -and $_.Trim() -ne '' }) -join '').Trim()
}

if (-not $expectedHash) {
    throw "VB-CABLE SHA-256 is not pinned (scripts/vb-cable.sha256 is empty). Refusing to install."
}

$local = Join-Path $env:LOCALAPPDATA "RemoteMic\RC003"
New-Item -ItemType Directory -Force -Path $local | Out-Null

$zip = Join-Path $local "VBCABLE_Driver_Pack45.zip"
$extract = Join-Path $local "vbcable"

# Get-FileHash / Expand-Archive live in autoloaded modules and are missing in
# some GUI-spawned powershell.exe sessions (IWR still works: it is a DLL cmdlet).
# Hash and unzip via .NET so the in-app install button does not depend on them.
function Get-Sha256Hex([string]$Path) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $fs = [System.IO.File]::OpenRead($Path)
        try {
            $bytes = $sha.ComputeHash($fs)
            return ([BitConverter]::ToString($bytes) -replace '-', '').ToLowerInvariant()
        } finally {
            $fs.Dispose()
        }
    } finally {
        $sha.Dispose()
    }
}

Write-Output "Downloading $VbCableUrl ..."
Invoke-WebRequest -Uri $VbCableUrl -OutFile $zip -UseBasicParsing

Write-Output "Verifying SHA-256 ..."
$actualHash = Get-Sha256Hex $zip
if ($actualHash -ne $expectedHash.ToLowerInvariant()) {
    Remove-Item -Path $zip -Force -ErrorAction SilentlyContinue
    throw "SHA-256 mismatch. Actual=$actualHash Expected=$expectedHash. Deleted downloaded file."
}
Write-Output "Hash OK."

if (Test-Path $extract) { Remove-Item -Recurse -Force $extract }
Add-Type -AssemblyName System.IO.Compression.FileSystem
[System.IO.Compression.ZipFile]::ExtractToDirectory($zip, $extract)

$setup = Get-ChildItem -Path $extract -Recurse -Filter "VBCABLE_Setup_x64.exe" | Select-Object -First 1
if (-not $setup) { throw "VBCABLE_Setup_x64.exe not found in archive" }

Write-Output "Starting official installer (confirm UAC)..."
Start-Process -FilePath $setup.FullName -ArgumentList "/S" -Verb RunAs
Write-Output "Installer launched. When it finishes, click Re-detect in the app."
