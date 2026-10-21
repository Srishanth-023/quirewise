<#
.SYNOPSIS
  Regenerates golden fingerprints in fixtures/golden (one "WxH | text" line per output page).
  Review the diff before committing (docs/AGENTS.md section 8). Golden files change only through this
  script.
#>
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
$env:QW_UPDATE_GOLDEN = '1'
try {
    cargo test -p quirewise-worker --test organize --locked
    if ($LASTEXITCODE -ne 0) { throw 'Golden regeneration failed.' }
}
finally { Remove-Item Env:QW_UPDATE_GOLDEN -ErrorAction SilentlyContinue }
git status --short fixtures/golden
Write-Host 'Review the fixtures/golden diff before committing.'
