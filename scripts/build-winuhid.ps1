# Build WinUHid.dll (user-mode) and copy to %LOCALAPPDATA%\RemoteMic\WinUHid\
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$repo = Join-Path $root "third_party\WinUHid"
if (-not (Test-Path $repo)) {
  Write-Output "Cloning WinUHid..."
  git clone --depth 1 https://github.com/cgutman/WinUHid.git $repo
}
$src = Join-Path $repo "WinUHid\WinUHid.vcxproj"
if (-not (Test-Path $src)) {
  throw "WinUHid source missing after clone"
}

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw "Visual Studio C++ tools not found" }

$msbuild = Join-Path $vs "MSBuild\Current\Bin\MSBuild.exe"
if (-not (Test-Path $msbuild)) {
  $msbuild = Join-Path $vs "MSBuild\Current\Bin\amd64\MSBuild.exe"
}
if (-not (Test-Path $msbuild)) { throw "MSBuild.exe not found under $vs" }

Write-Output "Building WinUHid.dll (Release|x64) with $msbuild"
& $msbuild $src /p:Configuration=Release /p:Platform=x64 /m /v:minimal
if ($LASTEXITCODE -ne 0) { throw "MSBuild failed: $LASTEXITCODE" }

$built = Get-ChildItem -Path (Join-Path $root "third_party\WinUHid") -Recurse -Filter "WinUHid.dll" |
  Where-Object { $_.FullName -match "\\(x64\\Release|Release\\x64)\\" } |
  Select-Object -First 1
if (-not $built) { throw "WinUHid.dll not produced" }

$dest = Join-Path $env:LOCALAPPDATA "RemoteMic\WinUHid"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item -Force $built.FullName (Join-Path $dest "WinUHid.dll")
Write-Output "Copied $($built.FullName) -> $dest\WinUHid.dll"
Write-Output "NOTE: WinUHid UMDF driver still must be installed for the DLL to create a virtual keyboard."
