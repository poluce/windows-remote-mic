# Download the WDK NuGet package and extract the UMDF/VHF headers + libs that
# build-winuhid-driver.ps1 needs (so a full WDK install is not required).
#
# Paths are derived from this script's location: the release runs on a GitHub
# runner where no machine-specific path exists.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "third_party\wdk-nupkg"
$extract = Join-Path $dir "extract"

if (Test-Path (Join-Path $extract "c\Lib\10.0.26100.0\um\x64\VhfUm.lib")) {
    Write-Output "WDK NuGet already extracted: $extract"
    exit 0
}

New-Item -ItemType Directory -Force -Path $dir | Out-Null
$nupkg = Join-Path $dir "wdk.nupkg"
Write-Output "Downloading WDK nupkg to $nupkg ..."
Invoke-WebRequest -Uri "https://api.nuget.org/v3-flatcontainer/microsoft.windows.wdk.x64/10.0.26100.6584/microsoft.windows.wdk.x64.10.0.26100.6584.nupkg" -OutFile $nupkg -UseBasicParsing
Write-Output ("size=" + (Get-Item $nupkg).Length)

Add-Type -AssemblyName System.IO.Compression.FileSystem
if (Test-Path $extract) { Remove-Item -Recurse -Force $extract }
[System.IO.Compression.ZipFile]::ExtractToDirectory($nupkg, $extract)

Write-Output "Searching VhfUm.lib / wdf.h / wudfddi.h"
Get-ChildItem $extract -Recurse -Include "VhfUm.lib", "wdf.h", "wudfddi.h", "WdfDriverStubUm.lib" |
    ForEach-Object { $_.FullName }
