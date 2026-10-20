# Stage the small, user-triggered extras that the NSIS installer embeds into the
# install directory for later use by the app:
#
#   scripts/install-vb-cable.ps1  -> downloads the official VB-CABLE package from
#                                    VB-Audio, verifies the pinned SHA-256, then
#                                    starts the official installer (admin + reboot).
#   scripts/vb-cable.sha256       -> the pinned SHA-256 used by that script.
#
# We intentionally do NOT bundle the VB-CABLE package itself: its EULA allows
# redistribution "AS IS" but forbids integrating it into another product's
# installation procedure without the author's agreement. So the app ships only
# our own helper script and the user triggers the official installer.
#
# Output goes to src-tauri\windows\vb-cable, which hooks.nsh copies to
# <install dir>\scripts. CI runs this before `npm run tauri build`.
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$out = Join-Path $root "src-tauri\windows\vb-cable"

Write-Output "== Stage VB-CABLE helper scripts =="

$files = @("install-vb-cable.ps1", "vb-cable.sha256")
foreach ($name in $files) {
    $src = Join-Path $PSScriptRoot $name
    if (-not (Test-Path $src)) {
        throw "Required file not found: $src"
    }
}

if (Test-Path $out) {
    Remove-Item -Recurse -Force $out
}
New-Item -ItemType Directory -Force -Path $out | Out-Null

foreach ($name in $files) {
    Copy-Item (Join-Path $PSScriptRoot $name) (Join-Path $out $name)
}

# Sanity check: the SHA file must contain a hex digest (comments allowed).
$lines = Get-Content (Join-Path $out "vb-cable.sha256")
$hash = (($lines | Where-Object { $_ -notmatch '^\s*#' -and $_.Trim() -ne '' }) -join "").Trim()
if ($hash.Length -ne 64 -or $hash -notmatch '^[0-9a-fA-F]{64}$') {
    throw "vb-cable.sha256 does not contain a 64-character SHA-256 digest."
}

Write-Output "Bundle ready: $out"
Get-ChildItem $out | Select-Object Name, Length
