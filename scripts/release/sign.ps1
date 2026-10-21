<#
.SYNOPSIS
  Authenticode-signs exe, dll and installer files. Uses Azure Trusted Signing when
  QW_SIGN_ENDPOINT is set, otherwise a certificate from QW_SIGN_PFX (path) and QW_SIGN_PFX_PASSWORD.
  Secrets come from the environment only and are never logged.
#>
[CmdletBinding()]
param([Parameter(Mandatory)][string[]]$Path)
$ErrorActionPreference = 'Stop'
$files = Get-ChildItem -Path $Path -Recurse -Include *.exe, *.dll, *.msi -File
if (-not $files) { throw 'No signable files found.' }
$signtool = (Get-Command signtool.exe -ErrorAction Stop).Source
foreach ($f in $files) {
    if ($env:QW_SIGN_ENDPOINT) {
        & $signtool sign /v /fd SHA256 /tr 'http://timestamp.acs.microsoft.com' /td SHA256 `
            /dlib $env:QW_SIGN_DLIB /dmdf $env:QW_SIGN_METADATA $f.FullName
    }
    elseif ($env:QW_SIGN_PFX) {
        & $signtool sign /fd SHA256 /f $env:QW_SIGN_PFX /p $env:QW_SIGN_PFX_PASSWORD `
            /tr $env:QW_SIGN_TIMESTAMP_URL /td SHA256 $f.FullName
    }
    else { throw 'No signing configuration found in the environment.' }
    if ($LASTEXITCODE -ne 0) { throw "Signing failed for $($f.Name)" }
}
