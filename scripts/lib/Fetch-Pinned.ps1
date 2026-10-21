<#
.SYNOPSIS
  Shared helper: download a pinned archive, verify SHA-256, extract into vendor/<name>/<arch>.
  Network use is explicit: the calling script is run by a developer or CI, never by the app.
#>
function Invoke-PinnedFetch {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][ValidateSet('x64', 'arm64')][string]$Arch,
        [Parameter(Mandatory)][string]$Root
    )
    $pins = Get-Content (Join-Path $Root 'scripts/native-pins.json') -Raw | ConvertFrom-Json
    $pin = $pins.$Name.$Arch
    if (-not $pin) { throw "No pin for $Name/$Arch in scripts/native-pins.json" }
    if ([string]::IsNullOrWhiteSpace($pin.sha256)) {
        throw "Pin for $Name/$Arch has no sha256. Review the download, then record the hash (see docs/project/open-questions.md)."
    }
    $dest = Join-Path $Root "vendor/$Name/$Arch"
    # A marker written after a verified extraction lets a repeat run skip the download.
    $marker = Join-Path $dest '.pin-sha256'
    if ((Test-Path $marker) -and ((Get-Content $marker -Raw).Trim() -eq $pin.sha256.ToLowerInvariant())) {
        Write-Host "$Name $($pins.$Name.version) ($Arch) is up to date in $dest"
        return
    }
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("quirewise-fetch-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $archive = Join-Path $tmp ([IO.Path]::GetFileName(([uri]$pin.url).AbsolutePath))
        # The progress bar of Windows PowerShell 5.1 slows a download down by a factor of ten or more.
        $ProgressPreference = 'SilentlyContinue'
        Invoke-WebRequest -Uri $pin.url -OutFile $archive -UseBasicParsing
        $actual = (Get-FileHash -Algorithm SHA256 -Path $archive).Hash.ToLowerInvariant()
        if ($actual -ne $pin.sha256.ToLowerInvariant()) {
            throw "SHA-256 mismatch for $Name/$Arch. Expected $($pin.sha256), got $actual."
        }
        if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
        New-Item -ItemType Directory -Path $dest | Out-Null
        if ($archive -like '*.zip') { Expand-Archive -Path $archive -DestinationPath $dest -Force }
        else {
            # Use the Windows tar explicitly: a Git for Windows tar earlier on PATH misreads drive letters.
            & (Join-Path $env:SystemRoot 'System32\tar.exe') -xzf $archive -C $dest
            if ($LASTEXITCODE -ne 0) { throw 'tar extraction failed' }
        }
        Set-Content -Path $marker -Value $pin.sha256.ToLowerInvariant() -Encoding ascii
        Write-Host "$Name $($pins.$Name.version) ($Arch) installed to $dest"
    }
    finally { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
}
