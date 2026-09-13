# Fetch the official Frida Gadget that the RC003 HOGP bypass depends on.
#
# Two modes:
#   default            install into %PROGRAMDATA%\RemoteMic\hid-tap (offline prep /
#                      diagnosing network or ACL issues)
#   -ArchiveOnly       only download + verify the .xz into -DestDir, which is how
#                      scripts\prepare-vhid-bundle.ps1 embeds it into the installer
#
# The app itself prefers a gadget shipped inside the installer and only falls back
# to downloading from GitHub, so end users normally never run this.
# ASCII-only so Windows PowerShell 5.1 can parse this file.
param(
    [string]$Version = "17.15.3",
    [string]$DestDir = (Join-Path $env:PROGRAMDATA "RemoteMic\hid-tap"),
    [switch]$ArchiveOnly
)

$ErrorActionPreference = "Stop"

$archiveName = "frida-gadget-$Version-windows-x86_64.dll.xz"
$url = "https://github.com/frida/frida/releases/download/$Version/$archiveName"
# Official GitHub Release SHA-256 for the 17.15.3 windows-x86_64 gadget xz.
$expectedArchiveSha256 = "b566d70189b6d551ad8f4e0bea24de08a3d4c0f559bb35b2bdb67d45182240c2"

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

New-Item -ItemType Directory -Force -Path $DestDir | Out-Null

# ProgramData is read-only for regular users by default, but the app has to
# update the JS/config at runtime. Grant Users Modify (idempotent); SYSTEM and
# Administrators keep full control. Not needed for -ArchiveOnly.
if (-not $ArchiveOnly) {
    & icacls.exe $DestDir /grant "*S-1-5-18:(OI)(CI)F" /grant "*S-1-5-32-544:(OI)(CI)F" /grant "*S-1-5-32-545:(OI)(CI)M" /C /Q | Out-Null
}

$archivePath = Join-Path $DestDir $archiveName

$needDownload = $true
if (Test-Path $archivePath) {
    if ((Get-Sha256Hex $archivePath) -eq $expectedArchiveSha256) {
        Write-Host "Already present and verified: $archivePath"
        $needDownload = $false
    } else {
        Write-Host "Existing archive failed verification, re-downloading."
        Remove-Item -Force $archivePath
    }
}

if ($needDownload) {
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $archivePath -UseBasicParsing
}

$got = Get-Sha256Hex $archivePath
if ($got -ne $expectedArchiveSha256) {
    Remove-Item -Force $archivePath
    throw "Frida Gadget archive SHA-256 mismatch (got $got). File deleted."
}
Write-Host "Verified SHA-256: $got"

if ($ArchiveOnly) {
    Write-Host "Archive ready for bundling: $archivePath"
    exit 0
}

$extractedName = "frida-gadget-$Version-windows-x86_64.dll"
$extractedPath = Join-Path $DestDir $extractedName
$dllPath = Join-Path $DestDir "frida-gadget.dll"
if (Test-Path $extractedPath) {
    Remove-Item -Force $extractedPath
}

# The release ships a bare .dll.xz (not tar.xz). System tar handles it on recent
# Windows; fall back to Python's lzma module otherwise.
$extracted = $false
Push-Location $DestDir
try {
    & tar.exe -xf $archiveName
    if ($LASTEXITCODE -eq 0 -and (Test-Path $extractedPath)) {
        $extracted = $true
    }
} finally {
    Pop-Location
}

if (-not $extracted) {
    $python = Get-Command python.exe -ErrorAction SilentlyContinue
    if (-not $python) {
        $python = Get-Command py.exe -ErrorAction SilentlyContinue
    }
    if (-not $python) {
        throw "tar.exe cannot extract xz; python.exe not found for lzma fallback"
    }
    & $python.Source -c "import lzma, shutil, sys; shutil.copyfileobj(lzma.open(sys.argv[1], 'rb'), open(sys.argv[2], 'wb'))" $archivePath $extractedPath
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $extractedPath)) {
        throw "python lzma extract failed for $archiveName"
    }
}

if (-not (Test-Path $extractedPath)) {
    throw "Extracted DLL not found: $extractedPath"
}

Copy-Item -Force $extractedPath $dllPath
Write-Host "Gadget ready: $dllPath"
Write-Host "Restart Remote Mic, connect the remote, then allow the UAC prompt if asked."
