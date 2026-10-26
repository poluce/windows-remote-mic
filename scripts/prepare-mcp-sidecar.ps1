# Build the MCP stdio sidecar and stage it for Tauri's externalBin bundling.
#
# Tauri expects src-tauri\binaries\<name>-<target-triple>.exe. The sidecar is a
# separate console binary (remote-mic-mcp.exe) because the GUI app cannot serve
# stdio MCP itself.
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root "target\release\remote-mic-mcp.exe"
$outDir = Join-Path $root "src-tauri\binaries"
$outFile = Join-Path $outDir "remote-mic-mcp-x86_64-pc-windows-msvc.exe"

Write-Output "== Build MCP sidecar =="
Push-Location $root
try {
    & cargo build -p mcp-bridge --release
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build -p mcp-bridge --release failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

if (-not (Test-Path $target)) {
    throw "Sidecar binary not found after build: $target"
}
if (-not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
}
Copy-Item $target $outFile -Force
Write-Output "Sidecar ready: $outFile"
Get-Item $outFile | Select-Object Name, Length
