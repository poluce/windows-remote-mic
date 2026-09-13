# Remove the Root\WinUHid device node and the WinUHid driver package.
#
# Called from the NSIS uninstall hook (before files are deleted) or manually.
# Reads the state written by install-winuhid.ps1 so the DriverStore entry can be
# removed by its published name (oemNN.inf), which is not otherwise recoverable.
param(
    [string]$DriverDir = (Join-Path $env:LOCALAPPDATA "RemoteMic\WinUHid\driver"),
    [switch]$NoRelaunch
)

$ErrorActionPreference = "Stop"
$stateKey = "HKLM:\SOFTWARE\RemoteMic\VirtualHid"

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    if ($NoRelaunch) { throw "Administrator privileges are required." }
    Write-Output "Relaunching as administrator..."
    $self = $MyInvocation.MyCommand.Path
    $argList = @(
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "`"$self`"",
        "-DriverDir", "`"$DriverDir`""
    )
    Start-Process -FilePath "powershell.exe" -ArgumentList $argList -Verb RunAs -Wait
    exit $LASTEXITCODE
}

$published = $null
$dir = $DriverDir
if (Test-Path $stateKey) {
    $state = Get-ItemProperty -Path $stateKey -ErrorAction SilentlyContinue
    if ($state) {
        if ($state.PublishedInf) { $published = $state.PublishedInf }
        if ($state.DriverDir -and (Test-Path (Join-Path $state.DriverDir "devcon.exe"))) {
            $dir = $state.DriverDir
        }
    }
}

$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"

$devcon = Join-Path $dir "devcon.exe"
if (-not (Test-Path $devcon)) { $devcon = Join-Path $DriverDir "devcon.exe" }
if (Test-Path $devcon) {
    & $devcon remove "Root\WinUHid"
    Write-Output ("devcon remove exit=" + $LASTEXITCODE)
} else {
    & pnputil.exe /remove-device "Root\WinUHid"
    Write-Output ("pnputil remove-device exit=" + $LASTEXITCODE)
}

if ($published) {
    & pnputil.exe /delete-driver $published /uninstall /force
    Write-Output ("pnputil delete-driver exit=" + $LASTEXITCODE)
} else {
    Write-Output "No published driver name recorded; leaving the DriverStore entry in place."
}

$ErrorActionPreference = $prevEap
Remove-Item -Path $stateKey -Recurse -Force -ErrorAction SilentlyContinue
Write-Output "Virtual HID driver removed."
