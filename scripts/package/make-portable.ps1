<#
.SYNOPSIS
  Builds the portable zip: the app, the worker and the PDF engine files in one folder with a
  portable.flag next to the exe. In that folder Quirewise keeps settings, logs, components and
  scratch files under .\data and writes nothing to the registry or to the user profile.
.DESCRIPTION
  Run scripts/package/stage-installer.ps1 first (it builds the worker and collects the engine files) and
  build the app: cargo build --release --locked -p quirewise-desktop --target <target>.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target,
    [string]$OutDir = 'release'
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$stage = Join-Path $root 'apps/desktop/src-tauri/staging'
$app = Join-Path $root "target/$Target/release/quirewise-desktop.exe"
$worker = Join-Path $stage "quirewise-worker-$Target.exe"
if (-not (Test-Path $app)) { throw "App not found: $app. Build it first." }
if (-not (Test-Path $worker)) { throw 'Run scripts/package/stage-installer.ps1 first.' }

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$name = "Quirewise-$version-portable-$($Target.Split('-')[0])"
$folder = Join-Path (Join-Path $root $OutDir) $name
if (Test-Path $folder) { Remove-Item -Recurse -Force $folder }
$null = New-Item -ItemType Directory -Force $folder

Copy-Item $app (Join-Path $folder 'quirewise.exe')
Copy-Item $worker (Join-Path $folder 'quirewise-worker.exe')
Copy-Item (Join-Path $stage 'native') (Join-Path $folder 'native') -Recurse
foreach ($doc in 'LICENSE', 'docs/legal/THIRD_PARTY_NOTICES.md', 'docs/legal/PRIVACY.md', 'docs/legal/TERMS.md') {
    Copy-Item (Join-Path $root $doc) $folder
}
# The flag switches on portable mode: see AppPaths in the infrastructure crate.
Set-Content -Path (Join-Path $folder 'portable.flag') -Value 'Delete this file to keep settings in the user profile instead.' -Encoding ascii

$zip = "$folder.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $folder -DestinationPath $zip -CompressionLevel Optimal
$mb = [math]::Round((Get-Item $zip).Length / 1MB, 2)
Write-Host "Portable build: $zip ($mb MB)"
