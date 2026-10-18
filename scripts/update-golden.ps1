<#
.SYNOPSIS
  Regenerates golden fingerprints in fixtures/golden. Review render diffs before committing
  (AGENTS.md section 8). Golden files change only through this script.
#>
[CmdletBinding()]
param([string]$Tool)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)
$env:QW_UPDATE_GOLDEN = '1'
$testArgs = @('test', '--workspace', '--locked', '--', '--ignored', 'golden')
if ($Tool) { $testArgs += $Tool }
cargo @testArgs
if ($LASTEXITCODE -ne 0) { throw 'Golden regeneration failed.' }
git status --short fixtures/golden
Write-Host 'Review fixtures/golden diffs and render diff images before committing.'
