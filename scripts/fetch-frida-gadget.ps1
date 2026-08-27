# Fetch the official Frida Gadget used by the optional RC003 HID tap.
# ASCII-only. Does not inject anything; the app starts the tap after ATVV is up.
param(
    [string]$Version = "17.15.3"
)

$ErrorActionPreference = "Stop"

$archiveName = "frida-gadget-$Version-windows-x86_64.dll.xz"
$url = "https://github.com/frida/frida/releases/download/$Version/$archiveName"
# Official GitHub release digest for 17.15.3 windows-x86_64 gadget xz.
$expectedArchiveSha256 = "b566d70189b6d551ad8f4e0bea24de08a3d4c0f559bb35b2bdb67d45182240c2"

$dest = Join-Path $env:LOCALAPPDATA "RemoteMic\RC003\hid-tap"
New-Item -ItemType Directory -Force -Path $dest | Out-Null

$archivePath = Join-Path $dest $archiveName
$dllPath = Join-Path $dest "frida-gadget.dll"

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

if (-not (Test-Path $archivePath)) {
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $archivePath -UseBasicParsing
}

$got = Get-Sha256Hex $archivePath
if ($got -ne $expectedArchiveSha256) {
    Remove-Item -Force $archivePath
    throw "Frida Gadget archive SHA-256 mismatch (got $got). File deleted."
}

$extractedName = "frida-gadget-$Version-windows-x86_64.dll"
$extractedPath = Join-Path $dest $extractedName
if (Test-Path $extractedPath) {
    Remove-Item -Force $extractedPath
}

Push-Location $dest
try {
    & tar.exe -xf $archiveName
    if ($LASTEXITCODE -ne 0) {
        throw "tar.exe failed to extract $archiveName (exit $LASTEXITCODE)"
    }
} finally {
    Pop-Location
}

if (-not (Test-Path $extractedPath)) {
    throw "Extracted DLL not found: $extractedPath"
}

Copy-Item -Force $extractedPath $dllPath
Write-Host "Gadget ready: $dllPath"
Write-Host "Restart Remote Mic, connect the remote, then allow the UAC prompt if asked."
