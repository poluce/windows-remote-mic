# Fetch the official Frida Gadget used by the optional RC003 HID tap.
# ASCII-only so Windows PowerShell 5.1 can parse this file.
# Does not inject; the app starts the tap after ATVV is ready.
param(
    [string]$Version = "17.15.3"
)

$ErrorActionPreference = "Stop"

$archiveName = "frida-gadget-$Version-windows-x86_64.dll.xz"
$url = "https://github.com/frida/frida/releases/download/$Version/$archiveName"
# Official GitHub Release SHA-256 for the 17.15.3 windows-x86_64 gadget xz.
$expectedArchiveSha256 = "b566d70189b6d551ad8f4e0bea24de08a3d4c0f559bb35b2bdb67d45182240c2"

$dest = Join-Path $env:PROGRAMDATA "RemoteMic\hid-tap"
New-Item -ItemType Directory -Force -Path $dest | Out-Null

# ProgramData 默认对普通用户只读，但应用需要在运行时更新 JS/config。
# 给 Users 添加 Modify 权限（幂等；SYSTEM/Administrators 保持完全控制）。
& icacls.exe $dest /grant "*S-1-5-18:(OI)(CI)F" /grant "*S-1-5-32-544:(OI)(CI)F" /grant "*S-1-5-32-545:(OI)(CI)M" /C /Q | Out-Null

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

$extracted = $false
Push-Location $dest
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
