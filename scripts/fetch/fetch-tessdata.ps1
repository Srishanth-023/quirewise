<#
.SYNOPSIS
  Fetches the pinned language data (tessdata_fast) into vendor/tesseract/<arch>/tessdata after
  verifying SHA-256. Network use is explicit: this runs on a developer machine or in CI, never in
  the app.
#>
[CmdletBinding()]
param(
    [string[]]$Language = @('eng'),
    [ValidateSet('x64', 'arm64')][string[]]$Arch = @('x64', 'arm64')
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$pins = (Get-Content (Join-Path $root 'scripts/native-pins.json') -Raw | ConvertFrom-Json).tessdata
foreach ($code in $Language) {
    $pin = $pins.$code
    if (-not $pin) { throw "No pin for the language $code in scripts/native-pins.json" }
    foreach ($a in $Arch) {
        $dir = Join-Path $root "vendor/tesseract/$a/tessdata"
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        $file = Join-Path $dir "$code.traineddata"
        Invoke-WebRequest -Uri $pin.url -OutFile $file -UseBasicParsing
        $actual = (Get-FileHash -Algorithm SHA256 -Path $file).Hash.ToLowerInvariant()
        if ($actual -ne $pin.sha256.ToLowerInvariant()) {
            Remove-Item -Force $file
            throw "SHA-256 mismatch for $code. Expected $($pin.sha256), got $actual."
        }
        Write-Host "$code ($a) installed to $file"
    }
}
