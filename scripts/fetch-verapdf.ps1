<#
.SYNOPSIS
  Installs the pinned veraPDF command line into vendor/verapdf/<version> after verifying SHA-256.
  Network use is explicit: this runs on a developer machine or in CI, never in the app.
.DESCRIPTION
  veraPDF is a Java program, so Java 11 or newer must be on the path (or in JAVA_HOME). The product
  ships veraPDF with its own Java as the on-demand component `verapdf` (docs/components.md); this
  script only prepares a development tree, where the tests of the PDF/A tools look for it.
  The download address always serves the newest release, so a new release changes the hash:
  review it, then update the pin in scripts/native-pins.json.
#>
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$pin = (Get-Content (Join-Path $root 'scripts/native-pins.json') -Raw | ConvertFrom-Json).verapdf
if (-not $pin -or [string]::IsNullOrWhiteSpace($pin.sha256)) { throw 'No verapdf pin with a sha256 in scripts/native-pins.json' }

$java = $null
if ($env:JAVA_HOME -and (Test-Path (Join-Path $env:JAVA_HOME 'bin/java.exe'))) { $java = Join-Path $env:JAVA_HOME 'bin/java.exe' }
elseif (Get-Command java.exe -ErrorAction SilentlyContinue) { $java = (Get-Command java.exe).Source }
if (-not $java) { throw 'Java is needed to install and run veraPDF. Install a JRE (11 or newer) and try again.' }

$dest = Join-Path $root "vendor/verapdf/$($pin.version)"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("quirewise-fetch-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $zip = Join-Path $tmp 'verapdf-installer.zip'
    Invoke-WebRequest -Uri $pin.url -OutFile $zip -UseBasicParsing
    $actual = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
    if ($actual -ne $pin.sha256.ToLowerInvariant()) {
        throw "SHA-256 mismatch for verapdf. Expected $($pin.sha256), got $actual."
    }
    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    $installer = Get-ChildItem -Path $tmp -Recurse -Filter 'verapdf-izpack-installer-*.jar' | Select-Object -First 1
    if (-not $installer) { throw 'The installer jar is not in the download.' }
    if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
    $target = $dest.Replace('\', '/')
    # Only the command line is wanted: no window, no documentation, no sample plug-ins.
    @"
<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<AutomatedInstallation langpack="eng">
  <com.izforge.izpack.panels.htmlhello.HTMLHelloPanel id="welcome"/>
  <com.izforge.izpack.panels.target.TargetPanel id="install_dir"><installpath>$target</installpath></com.izforge.izpack.panels.target.TargetPanel>
  <com.izforge.izpack.panels.packs.PacksPanel id="sdk_pack_select">
    <pack index="0" name="veraPDF GUI" selected="false"/>
    <pack index="1" name="veraPDF CLI" selected="true"/>
    <pack index="2" name="veraPDF Documentation" selected="false"/>
    <pack index="3" name="veraPDF Sample Plugins" selected="false"/>
  </com.izforge.izpack.panels.packs.PacksPanel>
  <com.izforge.izpack.panels.install.InstallPanel id="install"/>
  <com.izforge.izpack.panels.finish.FinishPanel id="finish"/>
</AutomatedInstallation>
"@ | Set-Content -Path (Join-Path $tmp 'auto-install.xml') -Encoding UTF8
    & $java -jar $installer.FullName (Join-Path $tmp 'auto-install.xml')
    if ($LASTEXITCODE -ne 0) { throw 'The veraPDF installer failed.' }
    if (-not (Get-ChildItem -Path (Join-Path $dest 'bin') -Filter 'cli-*.jar' -ErrorAction SilentlyContinue)) {
        throw 'veraPDF was installed without its command line.'
    }
    Write-Host "verapdf $($pin.version) installed to $dest"
}
finally { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
