<#
.SYNOPSIS
  Fails when a workspace crate has a forbidden dependency edge (AGENTS.md section 2).
#>
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

$meta = cargo metadata --no-deps --format-version 1 --locked | ConvertFrom-Json
$members = @{}
foreach ($p in $meta.packages) { $members[$p.name] = $p }

function Get-Layer([string]$manifest) {
    $m = $manifest.Replace('\', '/')
    if ($m -match '/crates/engines/') { return 'engine' }
    if ($m -match '/crates/domain/') { return 'domain' }
    if ($m -match '/crates/application/') { return 'application' }
    if ($m -match '/crates/infrastructure/') { return 'infrastructure' }
    if ($m -match '/crates/worker/') { return 'worker' }
    if ($m -match '/crates/cli/') { return 'cli' }
    if ($m -match '/apps/desktop/src-tauri/') { return 'shell' }
    return 'other'
}

# Layers each layer may depend on (workspace edges only).
$allowed = @{
    domain         = @()
    application    = @('domain')
    infrastructure = @('domain', 'application')
    engine         = @('domain', 'application')
    worker         = @('domain', 'application', 'infrastructure', 'engine')
    cli            = @('domain', 'application', 'infrastructure', 'engine')
    shell          = @('domain', 'application', 'infrastructure')
}
# External crates allowed in `domain` (normal dependencies only).
$domainExternal = @('serde', 'schemars', 'thiserror')

$violations = @()
foreach ($p in $meta.packages) {
    $layer = Get-Layer $p.manifest_path
    if (-not $allowed.ContainsKey($layer)) { continue }
    foreach ($d in $p.dependencies) {
        if ($d.kind -eq 'dev') { continue }
        if ($members.ContainsKey($d.name)) {
            $target = $members[$d.name]
            $tl = Get-Layer $target.manifest_path
            if ($allowed[$layer] -notcontains $tl) {
                $violations += "$($p.name) ($layer) must not depend on $($d.name) ($tl)"
            }
            if ($layer -eq 'engine' -and $tl -eq 'engine') {
                $violations += "$($p.name) must not depend on another engine ($($d.name))"
            }
        }
        elseif ($layer -eq 'domain' -and $domainExternal -notcontains $d.name) {
            $violations += "domain may only use serde, schemars, thiserror; found $($d.name)"
        }
    }
    # Opt-in AGPL feature must never be enabled by default.
    if ($p.features.PSObject.Properties.Name -contains 'default') {
        if ($p.features.default -contains 'agpl-ghostscript') {
            $violations += "$($p.name) enables agpl-ghostscript by default"
        }
    }
}

if ($violations.Count -gt 0) {
    $violations | ForEach-Object { Write-Error $_ -ErrorAction Continue }
    exit 1
}
Write-Host 'check-deps: layer boundaries OK'
