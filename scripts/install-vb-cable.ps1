# VB-CABLE 一键安装辅助脚本（Windows）。
# 下载官方 VB-Audio 驱动包，校验其 SHA-256，解压
# 并启动官方安装程序（会弹出 UAC 提示）。
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

# Get-FileHash / Expand-Archive 位于自动加载模块中，但在某些 GUI 启动的
# powershell.exe 会话中缺失（IWR 仍可用：它是 DLL cmdlet）。
# 通过 .NET 计算哈希和解压，使应用内安装按钮不依赖它们。
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
