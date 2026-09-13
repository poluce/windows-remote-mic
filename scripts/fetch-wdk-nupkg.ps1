$ErrorActionPreference = "Stop"
$dir = "D:\Document\windows-remote-mic\third_party\wdk-nupkg"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$nupkg = Join-Path $dir "wdk.nupkg"
Write-Output "Downloading WDK nupkg..."
Invoke-WebRequest -Uri "https://api.nuget.org/v3-flatcontainer/microsoft.windows.wdk.x64/10.0.26100.6584/microsoft.windows.wdk.x64.10.0.26100.6584.nupkg" -OutFile $nupkg -UseBasicParsing
Write-Output ("size=" + (Get-Item $nupkg).Length)
Add-Type -AssemblyName System.IO.Compression.FileSystem
$extract = Join-Path $dir "extract"
if (Test-Path $extract) { Remove-Item -Recurse -Force $extract }
[System.IO.Compression.ZipFile]::ExtractToDirectory($nupkg, $extract)
Write-Output "Searching VhfUm.lib / wdf.h / wudfddi.h"
Get-ChildItem $extract -Recurse -Include "VhfUm.lib","wdf.h","wudfddi.h","WdfDriverStubUm.lib" |
  ForEach-Object { $_.FullName }
