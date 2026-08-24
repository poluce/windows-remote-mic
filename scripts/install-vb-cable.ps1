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
    throw "VB-CABLE 官方包 SHA-256 尚未固定（scripts/vb-cable.sha256 为空），已拒绝安装以防止供应链攻击。"
}

$local = Join-Path $env:LOCALAPPDATA "RemoteMic\RC003"
New-Item -ItemType Directory -Force -Path $local | Out-Null

$zip = Join-Path $local "VBCABLE_Driver_Pack45.zip"
$extract = Join-Path $local "vbcable"

Write-Output "downloading $VbCableUrl ..."
Invoke-WebRequest -Uri $VbCableUrl -OutFile $zip

Write-Output "verifying SHA-256 ..."
$actualHash = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
if ($actualHash -ne $expectedHash.ToLowerInvariant()) {
    Remove-Item -Path $zip -Force -ErrorAction SilentlyContinue
    throw "SHA-256 校验失败！实际 $actualHash，期望 $expectedHash。已删除下载文件，请勿运行该包。"
}
Write-Output "hash OK."

if (Test-Path $extract) { Remove-Item -Recurse -Force $extract }
Expand-Archive -Path $zip -DestinationPath $extract -Force

$setup = Get-ChildItem -Path $extract -Recurse -Filter "VBCABLE_Setup_x64.exe" | Select-Object -First 1
if (-not $setup) { throw "VBCABLE_Setup_x64.exe not found in archive" }

Write-Output "starting official installer (confirm UAC)..."
Start-Process -FilePath $setup.FullName -ArgumentList "/S" -Verb RunAs
Write-Output "launched installer; once finished, click 重新检测 in the app."
