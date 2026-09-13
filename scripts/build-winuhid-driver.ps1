# Compile WinUHid UMDF driver DLL using WDK NuGet headers/libs (no full WDK toolset).
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$wdk = Join-Path $root "third_party\wdk-nupkg\extract\c"
$srcDir = Join-Path $root "third_party\WinUHid\WinUHid Driver"
$outDir = Join-Path $root "third_party\WinUHid\WinUHid Driver\build\x64"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$vsdev = Join-Path $vs "Common7\Tools\VsDevCmd.bat"
if (-not (Test-Path $vsdev)) { throw "VsDevCmd.bat not found" }

$incUmdf = Join-Path $wdk "Include\wdf\umdf\2.15"
$libVhf = Join-Path $wdk "Lib\10.0.26100.0\um\x64\VhfUm.lib"
$libWdf = Join-Path $wdk "Lib\wdf\umdf\x64\2.15\WdfDriverStubUm.lib"
$cfile = Join-Path $srcDir "WinUHid.c"
$obj = Join-Path $outDir "WinUHid.obj"
$dll = Join-Path $outDir "WinUHidDriver.dll"

# Fail loudly with the offending path instead of letting the compiler emit a
# bare C1083 for wdf.h later on.
foreach ($required in @((Join-Path $incUmdf "wdf.h"), $libVhf, $libWdf, $cfile)) {
    if (-not (Test-Path $required)) {
        throw "Missing build input: $required (run scripts\fetch-wdk-nupkg.ps1 first)"
    }
}

# WinUHid.c includes the WPP-generated WinUHid.tmh, which only exists when the
# full WDK toolset runs the WPP preprocessor. We compile with plain cl.exe, so
# install the stubbed header shipped in the repo when a fresh clone lacks it.
$tmh = Join-Path $srcDir "WinUHid.tmh"
if (-not (Test-Path $tmh)) {
    $stub = Join-Path $PSScriptRoot "winuhid-driver\WinUHid.tmh"
    if (-not (Test-Path $stub)) { throw "Missing WPP stub: $stub" }
    Copy-Item $stub $tmh
    Write-Output "Installed WinUHid.tmh stub (WPP/ETW tracing from the driver disabled)."
}

$cmd = @"
call `"$vsdev`" -arch=amd64 -host_arch=amd64
cl /nologo /c /W3 /O2 /MD /utf-8 ^
  /DUNICODE /D_UNICODE /DWIN32 /D_WINDOWS /D_WINDLL /D_USRDLL ^
  /DUMDF_VERSION_MAJOR=2 /DUMDF_VERSION_MINOR=15 /D_WIN32_WINNT=0x0A00 ^
  /I`"$srcDir`" /I`"$incUmdf`" ^
  /Fo`"$obj`" `"$cfile`"
link /nologo /DLL /MACHINE:X64 /SUBSYSTEM:WINDOWS ^
  /OUT:`"$dll`" `"$obj`" `"$libWdf`" `"$libVhf`" onecoreuap.lib ntdll.lib
"@

$bat = Join-Path $outDir "build.bat"
Set-Content -Path $bat -Value $cmd -Encoding ASCII
Write-Output "Compiling WinUHidDriver.dll..."
cmd.exe /C $bat
if ($LASTEXITCODE -ne 0) { throw "Driver compile/link failed: $LASTEXITCODE" }
Write-Output "Built $dll"
