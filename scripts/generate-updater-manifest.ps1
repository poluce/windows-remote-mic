<#
.SYNOPSIS
Generate the Tauri v2 updater manifest (latest.json) for a Windows release.

.DESCRIPTION
The Tauri v2 updater plugin fetches a static JSON manifest from the URL configured in
`plugins.updater.endpoints` (`src-tauri/tauri.conf.json`). This script builds that JSON from
the signed NSIS installer produced by `tauri build` (`bundle.createUpdaterArtifacts = true`),
the matching `.exe.sig` signature file, and the release notes taken from CHANGELOG.md.

The JSON is written as UTF-8 without a BOM: a BOM would make the manifest fail to parse.

.PARAMETER Version
Version announced by the manifest, for example "0.7.0" (no leading "v").

.PARAMETER Tag
Git tag used in the asset download URL, for example "v0.7.0".

.PARAMETER InstallerPath
Path to the signed NSIS installer (`*_x64-setup.exe`).

.PARAMETER SignaturePath
Path to the updater signature file (the installer path plus ".sig").

.PARAMETER OutputPath
Where to write latest.json.

.PARAMETER Repository
GitHub "owner/repo" used to build the asset URL.

.PARAMETER ChangelogPath
CHANGELOG.md used to fill the "notes" field (falls back to the version string).
#>
[CmdletBinding()]
param
(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Tag,
    [Parameter(Mandatory = $true)][string]$InstallerPath,
    [Parameter(Mandatory = $true)][string]$SignaturePath,
    [Parameter(Mandatory = $true)][string]$OutputPath,
    [string]$Repository = "poluce/windows-remote-mic",
    [string]$ChangelogPath = "CHANGELOG.md"
)

$ErrorActionPreference = "Stop"

$versionClean = $Version.Trim().TrimStart("v")
if ($versionClean -notmatch '^\d+\.\d+\.\d+([.-][0-9A-Za-z.-]+)?$') {
    throw "Invalid version '$Version'"
}
$tagClean = $Tag.Trim()
if ([string]::IsNullOrWhiteSpace($tagClean)) {
    throw "Tag must not be empty"
}

$installer = Get-Item -LiteralPath $InstallerPath -ErrorAction Stop
$signature = Get-Item -LiteralPath $SignaturePath -ErrorAction Stop
$signatureText = (Get-Content -LiteralPath $signature.FullName -Raw).Trim()
if ([string]::IsNullOrWhiteSpace($signatureText)) {
    throw "Signature file '$($signature.FullName)' is empty"
}

# Tauri's updater expects the minisign signature exactly as written by `tauri build`.
$url = "https://github.com/$Repository/releases/download/$tagClean/$([uri]::EscapeDataString($installer.Name))"

# Release notes: the CHANGELOG section for this version, if present.
# -Encoding UTF8 keeps this working on Windows PowerShell 5.1, where the default
# is the ANSI code page and would corrupt the Chinese text.
$notes = "Remote Mic $versionClean"
if (Test-Path -LiteralPath $ChangelogPath) {
    $lines = Get-Content -LiteralPath $ChangelogPath -Encoding UTF8
    $heading = "## [$versionClean]"
    $match = $lines | Select-String -SimpleMatch $heading | Select-Object -First 1
    if ($match) {
        $start = $match.LineNumber
        $section = @()
        for ($i = $start; $i -lt $lines.Count; $i++) {
            if ($i -gt $start -and $lines[$i] -match '^## \[') { break }
            $section += $lines[$i]
        }
        $body = (($section -join "`n").Trim() -replace '^## \[[^\]]+\][^\n]*\n?', '').Trim()
        if (-not [string]::IsNullOrWhiteSpace($body)) { $notes = $body }
    }
}

$platform = [ordered]@{
    signature = $signatureText
    url       = $url
}

$manifest = [ordered]@{
    version   = $versionClean
    notes     = $notes
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
        # The updater looks for "windows-x86_64-nsis" first and falls back to
        # "windows-x86_64"; publishing both keeps older/newer plugin versions happy.
        "windows-x86_64-nsis" = $platform
        "windows-x86_64"      = $platform
    }
}

$json = $manifest | ConvertTo-Json -Depth 6
$outputDir = Split-Path -Parent $OutputPath
if (-not [string]::IsNullOrWhiteSpace($outputDir) -and -not (Test-Path -LiteralPath $outputDir)) {
    New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

# UTF-8 without BOM.
[System.IO.File]::WriteAllText(
    (Join-Path (Get-Location) $OutputPath),
    $json,
    [System.Text.UTF8Encoding]::new($false)
)

Write-Output "updater manifest: $OutputPath"
Write-Output "  version:   $versionClean"
Write-Output "  installer: $($installer.Name)"
Write-Output "  url:       $url"
