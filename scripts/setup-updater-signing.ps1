<#
.SYNOPSIS
Create (or rotate) the Tauri updater signing key and publish it to GitHub Actions secrets.

.DESCRIPTION
The Tauri v2 updater signs every release artifact with a minisign keypair that is
independent from Authenticode code signing. This script:

  1. generates the keypair with `tauri signer generate` (private key stays on this machine),
  2. stores the password next to the private key,
  3. uploads both values to the repository secrets used by the release workflow
     (TAURI_SIGNING_PRIVATE_KEY / TAURI_SIGNING_PRIVATE_KEY_PASSWORD),
  4. prints the public key that has to be pasted into
     src-tauri/tauri.conf.json -> plugins.updater.pubkey.

Keep the private key and its password backed up: losing either one means future releases
cannot be signed, and installed apps can no longer auto-update.

.PARAMETER Repo
GitHub repository that hosts the releases, "owner/repo".

.PARAMETER KeyPath
Where to write the private key (a .pub file and .password.txt are written next to it).

.PARAMETER Password
Password for the private key. Generated randomly when omitted.

.PARAMETER Rotate
Overwrite an existing keypair. Rotation breaks auto-update for already installed versions:
they embed the old public key, so users must install the new build manually once.

.PARAMETER SkipSecrets
Only generate the keypair; do not touch GitHub secrets.
#>
[CmdletBinding()]
param
(
    [string]$Repo = "poluce/windows-remote-mic",
    [string]$KeyPath = (Join-Path $env:USERPROFILE ".tauri\remote-mic-updater.key"),
    [string]$Password = "",
    [switch]$Rotate,
    [switch]$SkipSecrets
)

$ErrorActionPreference = "Stop"

function Write-Step([string]$Text) {
    Write-Output ""
    Write-Output "==> $Text"
}

if (Test-Path -LiteralPath $KeyPath) {
    if (-not $Rotate) {
        throw "Key already exists: $KeyPath. Use -Rotate to replace it (this breaks auto-update for installed apps)."
    }
    Remove-Item -LiteralPath $KeyPath -Force
    if (Test-Path -LiteralPath "$KeyPath.pub") { Remove-Item -LiteralPath "$KeyPath.pub" -Force }
    if (Test-Path -LiteralPath "$KeyPath.password.txt") { Remove-Item -LiteralPath "$KeyPath.password.txt" -Force }
    Write-Warning "Rotating the updater key: already installed apps will not be able to auto-update."
}

$keyDir = Split-Path -Parent $KeyPath
if (-not [string]::IsNullOrWhiteSpace($keyDir) -and -not (Test-Path -LiteralPath $keyDir)) {
    New-Item -ItemType Directory -Force -Path $keyDir | Out-Null
}

if ([string]::IsNullOrWhiteSpace($Password)) {
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    $bytes = New-Object byte[] 24
    $rng.GetBytes($bytes)
    $Password = ($bytes | ForEach-Object { $_.ToString("x2") }) -join ""
}

Write-Step "Generating updater keypair"
$cli = Join-Path $PSScriptRoot "..\node_modules\@tauri-apps\cli\tauri.js"
if (-not (Test-Path -LiteralPath $cli)) {
    throw "Tauri CLI not found at $cli. Run 'npm ci' in the repository root first."
}
& node $cli signer generate --ci -w $KeyPath -p $Password
if ($LASTEXITCODE -ne 0) { throw "tauri signer generate failed with exit code $LASTEXITCODE" }

# The private key must stay a single line without a trailing newline: the CLI feeds the
# secret content straight into base64 decoding, where any extra whitespace is fatal.
[System.IO.File]::WriteAllText("$KeyPath.password.txt", $Password, [System.Text.UTF8Encoding]::new($false))

$publicKey = ([System.IO.File]::ReadAllText("$KeyPath.pub")).Trim()
Write-Step "Public key (paste into src-tauri/tauri.conf.json -> plugins.updater.pubkey)"
Write-Output $publicKey

if (-not $SkipSecrets) {
    Write-Step "Uploading GitHub Actions secrets for $Repo"
    & gh auth status 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "gh is not authenticated. Run 'gh auth login' first." }

    $privateKey = [System.IO.File]::ReadAllText($KeyPath).Trim()
    & gh secret set TAURI_SIGNING_PRIVATE_KEY --repo $Repo --body $privateKey
    if ($LASTEXITCODE -ne 0) { throw "Failed to set TAURI_SIGNING_PRIVATE_KEY" }

    & gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo $Repo --body $Password
    if ($LASTEXITCODE -ne 0) { throw "Failed to set TAURI_SIGNING_PRIVATE_KEY_PASSWORD" }

    Write-Output "Secrets updated: TAURI_SIGNING_PRIVATE_KEY, TAURI_SIGNING_PRIVATE_KEY_PASSWORD"
}

Write-Step "Done"
Write-Output "Private key : $KeyPath"
Write-Output "Password    : $KeyPath.password.txt"
Write-Output "Public key  : $KeyPath.pub"
Write-Output ""
Write-Output "Back up the private key and password in a safe place. Never commit them."
