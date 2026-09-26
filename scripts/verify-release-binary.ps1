# Verifies a Windows release binary's PE architecture before publication.
#
# Usage: pwsh -NoProfile -File scripts/verify-release-binary.ps1 `
#            <x86_64|aarch64> <path-to-labeldeck.exe>
param(
    [Parameter(Mandatory = $true)][ValidateSet('x86_64', 'aarch64')][string]$Arch,
    [Parameter(Mandatory = $true)][string]$Binary
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Die([string]$Message) {
    Write-Error $Message -ErrorAction Stop
    exit 1
}

if (-not (Test-Path $Binary)) { Die "binary not found: $Binary" }

$bytes = [System.IO.File]::ReadAllBytes((Resolve-Path $Binary).Path)
if ($bytes.Length -lt 64) { Die 'file is too small to be a PE binary' }
if ($bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) { Die 'binary is not a PE image (no MZ signature)' }

$peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
if ($peOffset -lt 0 -or ($peOffset + 6) -gt $bytes.Length) {
    Die 'PE header offset is out of bounds'
}
if ($bytes[$peOffset] -ne 0x50 -or $bytes[$peOffset + 1] -ne 0x45 -or
    $bytes[$peOffset + 2] -ne 0 -or $bytes[$peOffset + 3] -ne 0) {
    Die 'binary does not carry a PE signature'
}

$machine = [BitConverter]::ToUInt16($bytes, $peOffset + 4)
$expected = switch ($Arch) {
    'x86_64' { 0x8664; break }
    'aarch64' { 0xAA64; break }
}

if ($machine -ne $expected) {
    Die "expected $Arch PE (machine 0x$($expected.ToString('X4'))), found machine 0x$($machine.ToString('X4'))"
}

Write-Host "verified $Arch PE binary: $Binary"
