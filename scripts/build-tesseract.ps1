<#
.SYNOPSIS
  Builds Tesseract 5 and Leptonica with vcpkg for x64-windows and arm64-windows.
  Requires VCPKG_ROOT. Output is staged in vendor/tesseract/<arch>.
#>
[CmdletBinding()]
param([ValidateSet('x64', 'arm64')][string[]]$Arch = @('x64', 'arm64'))
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $env:VCPKG_ROOT) { throw 'Set VCPKG_ROOT to a vcpkg checkout first.' }
foreach ($a in $Arch) {
    $triplet = "$a-windows-release"
    & "$env:VCPKG_ROOT/vcpkg.exe" install "tesseract:$triplet" "leptonica:$triplet" --classic
    if ($LASTEXITCODE -ne 0) { throw "vcpkg install failed for $triplet" }
    $dest = Join-Path $root "vendor/tesseract/$a"
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    $installed = Join-Path $env:VCPKG_ROOT "installed/$triplet"
    Copy-Item -Recurse -Force (Join-Path $installed 'bin/*') $dest
    Write-Host "Tesseract ($a) staged in $dest"
}
