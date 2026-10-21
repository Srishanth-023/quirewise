<#
.SYNOPSIS
  Builds the Quirewise .msi installer (WiX) and copies it to .\dist.
.DESCRIPTION
  Fetches the PDF engines when vendor\ lacks them, stages the worker and engines
  (scripts/package/stage-installer.ps1), builds the UI and the app, and bundles an .msi with
  src-tauri\tauri.bundle.conf.json. Tauri downloads the WiX toolset on the first run.
  Run from any folder: .\build-msi.ps1 [-Target aarch64-pc-windows-msvc] [-SkipStage] [-Quick] [-WixZip <path>]
  Tauri downloads the WiX toolset itself, which can time out. Pass the zip you downloaded from
  https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip and it is
  unpacked for Tauri after its SHA-256 is checked.
  It uses the release profile of Cargo.toml (thin LTO, 16 code-generation units), the same as CI.
  -Quick turns LTO off for a faster test build; the program is larger and slower, so do not ship it.
  Note: the Explorer menu is written by the app per user; an .msi uninstall does not remove it
  (the NSIS installer does). Remove it in Settings, Explorer menu before uninstalling.
#>
[CmdletBinding()]
param(
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target = 'x86_64-pc-windows-msvc',
    [switch]$SkipStage,
    [switch]$Quick,
    [string]$WixZip
)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
if ($Quick) {
    # Same `release` folder, so staging and the bundler work unchanged.
    $env:CARGO_PROFILE_RELEASE_LTO = 'off'
}
if ($WixZip) {
    $wixDir = Join-Path $env:LOCALAPPDATA 'tauri/WixTools314'
    if (-not (Test-Path (Join-Path $wixDir 'light.exe'))) {
        # The hash Tauri itself pins for this archive.
        $pinned = '6ac824e1642d6f7277d0ed7ea09411a508f6116ba6fae0aa5f2c7daa2ff43d31'
        if ((Get-FileHash -Algorithm SHA256 -Path $WixZip).Hash.ToLowerInvariant() -ne $pinned) { throw "$WixZip is not the expected wix314-binaries.zip." }
        New-Item -ItemType Directory -Force $wixDir | Out-Null
        Expand-Archive -Path $WixZip -DestinationPath $wixDir -Force
        Write-Host "WiX unpacked to $wixDir"
    }
}
$arch = if ($Target -like 'aarch64*') { 'arm64' } else { 'x64' }
$desktop = Join-Path $root 'apps/desktop'
$tauri = Join-Path $desktop 'ui/node_modules/.bin/tauri.cmd'

if (-not (Test-Path $tauri)) {
    Push-Location (Join-Path $desktop 'ui')
    try { pnpm install --frozen-lockfile; if ($LASTEXITCODE -ne 0) { throw 'pnpm install failed.' } } finally { Pop-Location }
}

if (-not $SkipStage) {
    foreach ($fetch in 'fetch-pdfium.ps1', 'fetch-qpdf.ps1') {
        # PowerShell scripts throw on failure; $LASTEXITCODE would only hold a stale value from an earlier command.
        & (Join-Path $root "scripts/fetch/$fetch") -Arch $arch
    }
    & (Join-Path $root 'scripts/package/stage-installer.ps1') -Target $Target
}

# The Tauri CLI finds the project only when started from apps/desktop.
Push-Location $desktop
try {
    & $tauri build --target $Target --bundles msi --config src-tauri/tauri.bundle.conf.json
    if ($LASTEXITCODE -ne 0) { throw 'tauri build failed.' }
} finally { Pop-Location }

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$msi = Get-ChildItem (Join-Path $root "target/$Target/release/bundle/msi") -Filter *.msi
if (-not $msi) { throw 'No .msi was produced.' }
$msi | Copy-Item -Destination $dist -Force
$msi | ForEach-Object { '{0}  {1:N1} MB' -f (Join-Path $dist $_.Name), ($_.Length / 1MB) }
