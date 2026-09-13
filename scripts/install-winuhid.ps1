# Install test-signed WinUHid UMDF driver + create Root\WinUHid device.
# Must run elevated. Trusts the RemoteMicWinUHid test certificate.
$ErrorActionPreference = "Stop"
$pkg = Join-Path $env:LOCALAPPDATA "RemoteMic\WinUHid\driver"
$inf = Join-Path $pkg "WinUHidDriver.inf"
if (-not (Test-Path $inf)) { throw "Driver package missing: $inf. Run package-winuhid-driver.ps1 first." }

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  Write-Output "Relaunching as administrator..."
  $self = $MyInvocation.MyCommand.Path
  Start-Process -FilePath "powershell.exe" -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$self`"" -Verb RunAs -Wait
  exit $LASTEXITCODE
}

$cer = Join-Path $pkg "RemoteMicWinUHid.cer"
if (-not (Test-Path $cer)) {
  $cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq "CN=RemoteMicWinUHid" } | Select-Object -First 1
  if (-not $cert) { throw "Test cert missing. Re-run package-winuhid-driver.ps1 and export-winuhid-cert.ps1." }
  Export-Certificate -Cert $cert -FilePath $cer | Out-Null
}
Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPublisher | Out-Null
Write-Output "Certificate trusted (Root + TrustedPublisher)."

Write-Output "pnputil add-driver..."
& pnputil.exe /add-driver $inf /install
Write-Output ("pnputil add-driver exit=" + $LASTEXITCODE)

$devcon = Join-Path (Split-Path $PSScriptRoot) "third_party\wdk-nupkg\extract\c\tools\10.0.26100.0\x64\devcon.exe"
if (-not (Test-Path $devcon)) {
  $devcon = Join-Path $env:LOCALAPPDATA "RemoteMic\WinUHid\devcon.exe"
}
if (-not (Test-Path $devcon)) {
  throw "devcon.exe not found. This Windows pnputil has no /add-device; need WDK devcon."
}

Write-Output "Removing existing Root\WinUHid devices..."
& $devcon remove "Root\WinUHid"
Write-Output ("devcon remove exit=" + $LASTEXITCODE)

Write-Output "Creating Root\WinUHid device..."
& $devcon install $inf "Root\WinUHid"
Write-Output ("devcon install exit=" + $LASTEXITCODE)

Write-Output "Querying WinUHid device..."
Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
  $_.InstanceId -like "*WinUHid*" -or $_.FriendlyName -like "*WinUHid*"
} | Format-Table Status, Class, FriendlyName, InstanceId -AutoSize

$log = Join-Path $env:LOCALAPPDATA "RemoteMic\WinUHid\install.log"
"$(Get-Date -Format o) install finished" | Out-File -FilePath $log -Encoding ascii -Append
Write-Output "Done. Log: $log"
