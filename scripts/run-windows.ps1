param(
    [Parameter(Mandatory = $false)]
    [string]$Action = "dev"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host "[run-windows] root=$root action=$Action"

if (Test-Path "node_modules") {
    Write-Host "[run-windows] removing node_modules for a clean Windows install..."
    Remove-Item -Recurse -Force "node_modules"
}

Write-Host "[run-windows] npm install"
npm install
if ($LASTEXITCODE -ne 0) {
    throw "npm install failed (exit $LASTEXITCODE)"
}

if ($Action -eq "build") {
    Write-Host "[run-windows] npm run tauri build"
    npm run tauri build
}
else {
    Write-Host "[run-windows] npm run tauri dev"
    npm run tauri dev
}
