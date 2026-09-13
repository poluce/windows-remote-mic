# Install the WinUHid UMDF driver package and create the Root\WinUHid device.
#
# Called from three places:
#   - scripts/prepare-vhid-bundle.ps1 : developer flow, driver built locally
#   - the NSIS installer hook         : end-user flow, driver bundled in $INSTDIR\vhid
#   - the in-app repair command       : re-runs the bundled copy, elevated
#
# Must run elevated. While the package is test-signed the certificate is
# trusted into Root + TrustedPublisher first; once the package is signed with a
# real code-signing certificate that step becomes a no-op because the chain
# already terminates in a publicly trusted root.
param(
    [string]$DriverDir = (Join-Path $env:LOCALAPPDATA "RemoteMic\WinUHid\driver"),
    [switch]$NoRelaunch
)

$ErrorActionPreference = "Stop"
$stateKey = "HKLM:\SOFTWARE\RemoteMic\VirtualHid"

$inf = Join-Path $DriverDir "WinUHidDriver.inf"
if (-not (Test-Path $inf)) { throw "Driver package missing: $inf" }

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

# Native tools write progress to stderr; keep that from aborting the script.
$prevEap = $ErrorActionPreference

# 1) Trust the signing certificate when it is not publicly trusted yet.
$cer = Join-Path $DriverDir "RemoteMicWinUHid.cer"
if (Test-Path $cer) {
    Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
    Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPublisher | Out-Null
    Write-Output "Trusted signing certificate (Root + TrustedPublisher)."
}

# 2) Stage the driver package into the DriverStore and remember its published name.
Write-Output "Adding driver package..."
$ErrorActionPreference = "Continue"
$addOutput = (& pnputil.exe /add-driver $inf /install 2>&1 | Out-String)
$ErrorActionPreference = $prevEap
Write-Output $addOutput
$published = ([regex]::Match($addOutput, 'oem\d+\.inf')).Value
if ($published) { Write-Output "Published name: $published" }

# 3) (Re)create the root-enumerated device node.
#    Not every Windows build exposes `pnputil /add-device`, so devcon is the
#    reliable path and ships next to the driver in the bundle.
$devcon = Join-Path $DriverDir "devcon.exe"
$ErrorActionPreference = "Continue"
if (Test-Path $devcon) {
    & $devcon remove "Root\WinUHid" | Out-Null
    Write-Output "Creating device node with devcon..."
    & $devcon install $inf "Root\WinUHid"
    $createExit = $LASTEXITCODE
} else {
    Write-Output "devcon.exe not found, falling back to pnputil /add-device..."
    & pnputil.exe /add-device "Root\WinUHid"
    $createExit = $LASTEXITCODE
}
$ErrorActionPreference = $prevEap
Write-Output ("device create exit=" + $createExit)
if ($createExit -ne 0) { throw "Failed to create the Root\WinUHid device node." }

# 4) Remember what to clean up on uninstall.
New-Item -Path $stateKey -Force | Out-Null
Set-ItemProperty -Path $stateKey -Name "DriverDir" -Value $DriverDir
if ($published) { Set-ItemProperty -Path $stateKey -Name "PublishedInf" -Value $published }
Set-ItemProperty -Path $stateKey -Name "Installed" -Value 1 -Type DWord

Write-Output "Virtual HID driver installed."
