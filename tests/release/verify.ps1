# Tests for scripts/verify-release-binary.ps1 using synthetic PE headers.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$script:Passes = 0

function Fail([string]$Message) {
    Write-Host "FAILED: $Message" -ForegroundColor Red
    exit 1
}

function Pass { $script:Passes++ }

$root = Join-Path ([System.IO.Path]::GetTempPath()) `
    ("labeldeck-pe-tests-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $root | Out-Null
$verifier = Join-Path $PSScriptRoot '..\..\scripts\verify-release-binary.ps1'
$verifier = (Resolve-Path $verifier).Path

function New-Pe([int]$Machine, [byte[]]$Prefix = @()) {
    # Minimal MZ/PE structure: DOS header with e_lfanew, PE signature,
    # and the machine field.
    $bytes = New-Object byte[] 256
    $bytes[0] = 0x4D; $bytes[1] = 0x5A
    [BitConverter]::GetBytes([int]0x80).CopyTo($bytes, 0x3C)
    $bytes[0x80] = 0x50; $bytes[0x81] = 0x45; $bytes[0x82] = 0; $bytes[0x83] = 0
    [BitConverter]::GetBytes([uint16]$Machine).CopyTo($bytes, 0x84)
    if ($Prefix.Length -gt 0) { $Prefix.CopyTo($bytes, 0) }
    $path = Join-Path $root ("pe-" + [System.IO.Path]::GetRandomFileName() + '.exe')
    [System.IO.File]::WriteAllBytes($path, $bytes)
    return $path
}

function Invoke-Verifier([string]$Arch, [string]$Binary) {
    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = (Get-Command pwsh -ErrorAction SilentlyContinue).Source
    if (-not $psi.FileName) { $psi.FileName = (Get-Command powershell).Source }
    $psi.Arguments = "-NoProfile -File `"$verifier`" $Arch `"$Binary`""
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $process = [System.Diagnostics.Process]::Start($psi)
    $output = $process.StandardOutput.ReadToEnd() + $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    [pscustomobject]@{ Code = $process.ExitCode; Output = $output }
}

function Assert-Passes([string]$Why, [string]$Arch, [string]$Binary) {
    $result = Invoke-Verifier $Arch $Binary
    if ($result.Code -ne 0) { Fail "${Why}: $($result.Output)" }
    Pass
}

function Assert-Fails([string]$Why, [string]$Arch, [string]$Binary, [string]$Needle) {
    $result = Invoke-Verifier $Arch $Binary
    if ($result.Code -eq 0) { Fail "$Why unexpectedly succeeded" }
    if ($result.Output -notmatch [regex]::Escape($Needle)) {
        Fail "${Why}: expected `"$Needle`" in: $($result.Output)"
    }
    Pass
}

$x64 = New-Pe 0x8664
$arm64 = New-Pe 0xAA64

Assert-Passes 'valid x86_64 PE' 'x86_64' $x64
Assert-Passes 'valid aarch64 PE' 'aarch64' $arm64
Assert-Fails 'wrong architecture rejected' 'x86_64' $arm64 'expected x86_64'
Assert-Fails 'arm64 mismatch rejected' 'aarch64' $x64 'expected aarch64'

$notPe = Join-Path $root 'not-pe.exe'
[System.IO.File]::WriteAllBytes($notPe, [System.Text.Encoding]::UTF8.GetBytes(('definitely not a PE file. ' * 10)))
Assert-Fails 'non-PE rejected' 'x86_64' $notPe 'MZ signature'

$short = Join-Path $root 'short.exe'
[System.IO.File]::WriteAllBytes($short, (New-Object byte[] 10))
Assert-Fails 'tiny file rejected' 'x86_64' $short 'too small'

$missing = Join-Path $root 'absent.exe'
Assert-Fails 'missing file rejected' 'x86_64' $missing 'not found'

Write-Host "PE verification tests passed: $script:Passes cases."
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
