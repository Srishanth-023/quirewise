<#
.SYNOPSIS
  Builds Tesseract 5 and Leptonica with vcpkg for x64-windows and arm64-windows and stages the
  program in vendor/tesseract/<arch>. Language data comes from scripts/fetch/fetch-tessdata.ps1.
  Requires VCPKG_ROOT (a vcpkg checkout). The build takes about twenty minutes.
#>
[CmdletBinding()]
param([ValidateSet('x64', 'arm64')][string[]]$Arch = @('x64', 'arm64'))
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $env:VCPKG_ROOT) { throw 'Set VCPKG_ROOT to a vcpkg checkout first.' }
foreach ($a in $Arch) {
    $triplet = "$a-windows-release"
    & "$env:VCPKG_ROOT/vcpkg.exe" install "tesseract:$triplet" --classic
    if ($LASTEXITCODE -ne 0) { throw "vcpkg install failed for $triplet" }
    $dest = Join-Path $root "vendor/tesseract/$a"
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    $installed = Join-Path $env:VCPKG_ROOT "installed/$triplet"
    # The program and every library it loads sit side by side, as the component is laid out.
    Copy-Item -Force (Join-Path $installed 'tools/tesseract/tesseract.exe') $dest
    Get-ChildItem (Join-Path $installed 'bin/*.dll') | Copy-Item -Destination $dest -Force
    Get-ChildItem (Join-Path $installed 'tools/tesseract/*.dll') -ErrorAction SilentlyContinue |
        Copy-Item -Destination $dest -Force
    Write-Host "Tesseract ($a) staged in $dest"
}
