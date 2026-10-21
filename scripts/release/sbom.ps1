<#
.SYNOPSIS
  Generates CycloneDX SBOMs for Rust and npm into sbom/ (published as CI artifacts).
#>
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
New-Item -ItemType Directory -Force -Path sbom | Out-Null
cargo cyclonedx --format json --all --override-filename rust-sbom
if ($LASTEXITCODE -ne 0) { throw 'cargo cyclonedx failed (install: cargo install cargo-cyclonedx --locked)' }
$rust = Get-ChildItem -Recurse -Filter 'rust-sbom.json' -File | Select-Object -First 1
Move-Item -Force -Path $rust.FullName -Destination sbom/rust-sbom.json
pnpm -C apps/desktop/ui dlx @cyclonedx/cyclonedx-npm --output-file ../../../sbom/npm-sbom.json --output-format JSON
if ($LASTEXITCODE -ne 0) { throw 'npm SBOM generation failed' }
Write-Host 'SBOMs written to sbom/'
