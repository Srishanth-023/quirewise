<#
.SYNOPSIS
  Fetches the pinned llama.cpp server into vendor/llama-cpp/<arch>, and optionally a model file
  into vendor/models/<id>, after verifying SHA-256.
  Network use is explicit: this runs on a developer machine or in CI, never in the app.
.DESCRIPTION
  The product ships neither: the llama.cpp server and each model are on-demand components
  (docs/components.md), and the person sees the license of a model before it is downloaded.
  This script prepares a development tree, where the tests of the AI tools look for them.
.EXAMPLE
  scripts/fetch-llama.ps1 -Arch x64 -Model smollm2-360m-instruct
#>
[CmdletBinding()]
param(
    [ValidateSet('x64', 'arm64')][string[]]$Arch = @('x64'),
    [string]$Model = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'lib/Fetch-Pinned.ps1')
foreach ($a in $Arch) { Invoke-PinnedFetch -Name 'llama-cpp' -Arch $a -Root $root }

if ($Model) {
    $pins = Get-Content (Join-Path $root 'scripts/native-pins.json') -Raw | ConvertFrom-Json
    $pin = $pins.models.$Model
    if (-not $pin) { throw "No model $Model in scripts/native-pins.json" }
    $dest = Join-Path $root "vendor/models/$Model"
    New-Item -ItemType Directory -Path $dest -Force | Out-Null
    $target = Join-Path $dest $pin.file
    $partial = "$target.part"
    Write-Host "Model $Model ($($pin.license), $([math]::Round($pin.size / 1MB)) MB) from $($pin.url)"
    Invoke-WebRequest -Uri $pin.url -OutFile $partial -UseBasicParsing
    $actual = (Get-FileHash -Algorithm SHA256 -Path $partial).Hash.ToLowerInvariant()
    if ($actual -ne $pin.sha256.ToLowerInvariant()) {
        Remove-Item -Force $partial
        throw "SHA-256 mismatch for model $Model. Expected $($pin.sha256), got $actual."
    }
    Move-Item -Force -Path $partial -Destination $target
    Write-Host "model $Model installed to $target"
}
