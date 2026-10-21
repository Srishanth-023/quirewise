<#
.SYNOPSIS
  Collects what the installer carries besides the app: the worker program and the PDF engine files.
  Run it before `tauri build --config src-tauri/tauri.bundle.conf.json` (run from apps/desktop; the path is relative to it).
.DESCRIPTION
  Builds the worker in release mode (unless -SkipBuild), then fills apps/desktop/src-tauri/staging:
    quirewise-worker-<target>.exe   the sidecar Tauri renames to quirewise-worker.exe on install
    native/pdfium/                  pdfium.dll and its licenses
    native/qpdf/                    qpdf.exe, its library and the Visual C++ runtime it needs
  The folders come from vendor/, which the fetch scripts fill with pinned, hash-checked downloads.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target,
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$arch = if ($Target -like 'aarch64*') { 'arm64' } else { 'x64' }
$stage = Join-Path $root 'apps/desktop/src-tauri/staging'

if (-not $SkipBuild) {
    cargo build --release --locked -p quirewise-worker --target $Target
    if ($LASTEXITCODE -ne 0) { throw 'Building the worker failed.' }
}
$worker = Join-Path $root "target/$Target/release/quirewise-worker.exe"
if (-not (Test-Path $worker)) { throw "Worker not found: $worker" }

$pdfium = Join-Path $root "vendor/pdfium/$arch"
$qpdfVersions = Get-ChildItem (Join-Path $root "vendor/qpdf/$arch") -Directory -ErrorAction SilentlyContinue
if (-not (Test-Path (Join-Path $pdfium 'bin/pdfium.dll'))) { throw "pdfium is missing for $arch. Run scripts/fetch-pdfium.ps1." }
if (-not $qpdfVersions) { throw "qpdf is missing for $arch. Run scripts/fetch-qpdf.ps1." }
$qpdfBin = Join-Path ($qpdfVersions | Select-Object -First 1).FullName 'bin'

if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
$null = New-Item -ItemType Directory -Force $stage, "$stage/native/pdfium", "$stage/native/qpdf"

Copy-Item $worker (Join-Path $stage "quirewise-worker-$Target.exe")
Copy-Item (Join-Path $pdfium 'bin/pdfium.dll') "$stage/native/pdfium/"
foreach ($license in 'LICENSE', 'licenses') {
    $path = Join-Path $pdfium $license
    if (Test-Path $path) { Copy-Item $path "$stage/native/pdfium/" -Recurse }
}
# fix-qdf and zlib-flate are tools of qpdf that Quirewise does not use.
Get-ChildItem $qpdfBin -File |
    Where-Object { $_.Name -notin 'fix-qdf.exe', 'zlib-flate.exe' } |
    Copy-Item -Destination "$stage/native/qpdf/"

$bytes = (Get-ChildItem $stage -Recurse -File | Measure-Object Length -Sum).Sum
Write-Host ("Staged {0:N1} MB for {1} in {2}" -f ($bytes / 1MB), $Target, $stage)
