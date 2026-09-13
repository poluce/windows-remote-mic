# Stamp INF, generate catalog, test-sign WinUHid UMDF package.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$wdkBin = Join-Path $root "third_party\wdk-nupkg\extract\c\bin\10.0.26100.0"
$dll = Join-Path $root "third_party\WinUHid\WinUHid Driver\build\x64\WinUHidDriver.dll"
$infSrc = Join-Path $root "third_party\WinUHid\WinUHid Driver\WinUHidDriver.inf"
$pkg = Join-Path $root "third_party\WinUHid\WinUHid Driver\build\package"
if (-not (Test-Path $dll)) { throw "Build driver DLL first (scripts\build-winuhid-driver.ps1)" }

if (Test-Path $pkg) { Remove-Item -Recurse -Force $pkg }
New-Item -ItemType Directory -Force -Path $pkg | Out-Null
Copy-Item $dll (Join-Path $pkg "WinUHidDriver.dll")
Copy-Item $infSrc (Join-Path $pkg "WinUHidDriver.inf")

$stampinf = Join-Path $wdkBin "x64\stampinf.exe"
$inf2cat = Join-Path $wdkBin "x86\Inf2Cat.exe"
$infPkg = Join-Path $pkg "WinUHidDriver.inf"
& $stampinf -f $infPkg -d 09/12/2026 -a amd64 -v 10.0.26100.1 -k 2.15
if ($LASTEXITCODE -ne 0) { throw "stampinf failed" }
$infText = Get-Content -Raw -Encoding Unicode $infPkg
if ($infText -match '\$UMDFVERSION\$') {
  $infText = $infText.Replace('$UMDFVERSION$', '2.15.0')
  Write-Output "Patched UmdfLibraryVersion=2.15.0"
}
$oldAcl = 'HKR,,Security,,"D:P(A;;GA;;;BA)(A;;GA;;;SY)(A;;GA;;;UD)"'
$newAcl = 'HKR,,Security,,"D:P(A;;GA;;;BA)(A;;GA;;;SY)(A;;GA;;;UD)(A;;GA;;;AU)"'
if ($infText.Contains($oldAcl)) {
  $infText = $infText.Replace($oldAcl, $newAcl)
  Write-Output "Patched device ACL for Authenticated Users"
}
[System.IO.File]::WriteAllText($infPkg, $infText, [System.Text.Encoding]::Unicode)

Write-Output "Running Inf2Cat..."
& $inf2cat /driver:$pkg /os:10_X64,10_NI_X64,10_GE_X64 /verbose
if ($LASTEXITCODE -ne 0) { throw "Inf2Cat failed" }

$signtool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe" |
  Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
if (-not $signtool) { throw "signtool.exe not found" }

$cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq "CN=RemoteMicWinUHid" } | Select-Object -First 1
if (-not $cert) {
  Write-Output "Creating test code-signing certificate..."
  $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=RemoteMicWinUHid" -CertStoreLocation Cert:\CurrentUser\My -KeyExportPolicy Exportable
}

Get-ChildItem $pkg -File | Where-Object { $_.Extension -in ".dll", ".cat" } | ForEach-Object {
  Write-Output ("Signing " + $_.Name)
  & $signtool sign /fd SHA256 /s My /n RemoteMicWinUHid $_.FullName
  if ($LASTEXITCODE -ne 0) { throw "signtool failed on $($_.Name)" }
}

$dest = Join-Path $env:LOCALAPPDATA "RemoteMic\WinUHid\driver"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item -Force -Recurse "$pkg\*" $dest
Write-Output "Package ready at $dest"
Write-Output "Install (admin): pnputil /add-driver `"$dest\WinUHidDriver.inf`" /install"
Write-Output "Then create root device: pnputil /add-device `"Root\WinUHid`""
