# Assemble everything the installer needs to set up the virtual HID keyboard:
# the built + signed WinUHid driver package, devcon, and the install/uninstall
# scripts. Output goes to src-tauri\windows\driver, which is what
# src-tauri\windows\hooks.nsh embeds into the NSIS installer.
#
# Run this before `npm run tauri build`. CI runs it in the release workflow.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$out = Join-Path $root "src-tauri\windows\driver"
$wdkRoot = Join-Path $root "third_party\wdk-nupkg\extract\c"

Write-Output "== 1/5 WDK headers + tools =="
if (-not (Test-Path (Join-Path $wdkRoot "bin"))) {
    & (Join-Path $PSScriptRoot "fetch-wdk-nupkg.ps1")
} else {
    Write-Output "WDK already present."
}

Write-Output "== 2/5 build user-mode WinUHid.dll =="
# The app loads this library to talk to the driver, so it has to ship with the
# app as well -- the driver package alone is not enough.
& (Join-Path $PSScriptRoot "build-winuhid.ps1")
$clientDll = Get-ChildItem -Path (Join-Path $root "third_party\WinUHid") -Recurse -Filter "WinUHid.dll" |
    Where-Object { $_.FullName -match "\\(x64\\Release|Release\\x64)\\" } |
    Select-Object -First 1
if (-not $clientDll) { throw "WinUHid.dll (user-mode client) was not produced." }

Write-Output "== 3/5 build UMDF driver =="
& (Join-Path $PSScriptRoot "build-winuhid-driver.ps1")

Write-Output "== 4/5 stamp, catalog, sign =="
& (Join-Path $PSScriptRoot "package-winuhid-driver.ps1")

Write-Output "== 5/5 assemble installer bundle =="
$pkg = Join-Path $root "third_party\WinUHid\WinUHid Driver\build\package"
$cat = Get-ChildItem $pkg -Filter "*.cat" | Select-Object -First 1
if (-not $cat) { throw "Catalog file not found in $pkg" }

if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force -Path $out | Out-Null

Copy-Item (Join-Path $pkg "WinUHidDriver.dll") $out
Copy-Item (Join-Path $pkg "WinUHidDriver.inf") $out
Copy-Item $cat.FullName $out
Copy-Item $clientDll.FullName (Join-Path $out "WinUHid.dll")
Copy-Item (Join-Path $PSScriptRoot "install-winuhid.ps1") $out
Copy-Item (Join-Path $PSScriptRoot "uninstall-winuhid.ps1") $out

# devcon is shipped so the installer can create the root-enumerated device node
# on Windows builds whose pnputil has no /add-device subcommand.
$devcon = Join-Path $wdkRoot "tools\10.0.26100.0\x64\devcon.exe"
if (-not (Test-Path $devcon)) { throw "devcon.exe missing at $devcon" }
Copy-Item $devcon $out

# Test certificate. Delete this block once the driver package is signed with a
# real code-signing certificate -- then no certificate has to be trusted.
$cert = Get-ChildItem Cert:\CurrentUser\My |
    Where-Object { $_.Subject -eq "CN=RemoteMicWinUHid" } |
    Select-Object -First 1
if ($cert) {
    Export-Certificate -Cert $cert -FilePath (Join-Path $out "RemoteMicWinUHid.cer") | Out-Null
} else {
    Write-Output "WARNING: CN=RemoteMicWinUHid not found; bundle has no .cer to trust."
}

Write-Output "Bundle ready: $out"
Get-ChildItem $out | Select-Object Name, Length
