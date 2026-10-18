<#
.SYNOPSIS
  Verifies that built binaries are native arm64 PE files (machine type 0xAA64).
#>
[CmdletBinding()]
param([Parameter(Mandatory)][string[]]$Path)
$ErrorActionPreference = 'Stop'
$bad = @()
foreach ($f in Get-ChildItem -Path $Path -Recurse -Include *.exe, *.dll -File) {
    $fs = [IO.File]::OpenRead($f.FullName)
    try {
        $br = New-Object IO.BinaryReader($fs)
        $fs.Position = 0x3C
        $fs.Position = $br.ReadInt32() + 4
        $machine = $br.ReadUInt16()
    }
    finally { $fs.Dispose() }
    if ($machine -ne 0xAA64) { $bad += ('{0}: machine 0x{1:X4}' -f $f.Name, $machine) }
}
if ($bad) {
    $bad | ForEach-Object { Write-Error $_ -ErrorAction Continue }
    exit 1
}
Write-Host 'verify-arm64: all binaries are arm64'
