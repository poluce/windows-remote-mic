$ErrorActionPreference = "Stop"
$destDir = Join-Path $env:LOCALAPPDATA "RemoteMic\WinUHid\driver"
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
$cer = Join-Path $destDir "RemoteMicWinUHid.cer"
$cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq "CN=RemoteMicWinUHid" } | Select-Object -First 1
if (-not $cert) { throw "CN=RemoteMicWinUHid not in CurrentUser\\My" }
Export-Certificate -Cert $cert -FilePath $cer | Out-Null
Write-Output "Exported $cer"
Write-Output ("Thumbprint=" + $cert.Thumbprint)
