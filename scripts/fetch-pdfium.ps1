<#
.SYNOPSIS
  Fetches the pinned PDFium build into vendor/pdfium/<arch> after verifying SHA-256.
#>
[CmdletBinding()]
param([ValidateSet('x64', 'arm64')][string[]]$Arch = @('x64', 'arm64'))
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'lib/Fetch-Pinned.ps1')
foreach ($a in $Arch) { Invoke-PinnedFetch -Name 'pdfium' -Arch $a -Root $root }
