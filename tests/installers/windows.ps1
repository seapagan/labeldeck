# Deterministic installer tests for install.ps1. All release assets come
# from a local mirror directory (LABELDECK_BASE_URL); nothing contacts
# GitHub.
#
# Failure-path cases run on any OS with PowerShell 7+. Success-path cases
# need a real Windows labeldeck.exe and run only on Windows.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$script:Passes = 0
$script:Skipped = 0

function Fail([string]$Message) {
    Write-Host "FAILED: $Message" -ForegroundColor Red
    exit 1
}

function Pass { $script:Passes++ }

function Skip([string]$Reason) {
    $script:Skipped++
    Write-Host "SKIP: $Reason"
}

$root = Join-Path ([System.IO.Path]::GetTempPath()) `
    ("labeldeck-ps1-tests-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $root | Out-Null

$installer = Join-Path $PSScriptRoot '..\..\install.ps1'
$resolvedInstaller = (Resolve-Path $installer).Path

# Synthetic versions deliberately unrelated to real releases.
$TestVersion = '99.88.77'
$TestTag = "v$TestVersion"

function New-Mirror {
    $mirror = Join-Path $root ("mirror-" + [System.IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path (Join-Path $mirror "download\$TestTag") -Force | Out-Null
    return $mirror
}

function Publish-Asset([string]$Mirror, [string]$Target, [byte[]]$Content) {
    $name = "labeldeck-$TestTag-$Target.zip"
    $path = Join-Path $Mirror "download\$TestTag\$name"
    [System.IO.File]::WriteAllBytes($path, $Content)
    $hash = [System.BitConverter]::ToString(
        [System.Security.Cryptography.SHA256]::Create().ComputeHash($Content)
    ).Replace('-', '').ToLowerInvariant()
    "$hash  $name" | Set-Content -NoNewline (Join-Path $Mirror "download\$TestTag\$name.sha256")
}

function New-ZipBytes([hashtable]$Files) {
    $stage = Join-Path $root ("zip-" + [System.IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path $stage | Out-Null
    foreach ($name in $Files.Keys) {
        $destination = Join-Path $stage $name
        New-Item -ItemType Directory -Path (Split-Path $destination) -Force | Out-Null
        [System.IO.File]::WriteAllBytes($destination, $Files[$name])
    }
    $archive = "$stage.zip"
    Compress-Archive -Path "$stage\*" -DestinationPath $archive
    return [System.IO.File]::ReadAllBytes($archive)
}

function Invoke-Installer([string]$Mirror, [string]$InstallDir, [string]$Version = $TestTag, [string]$Architecture = 'AMD64') {
    $envVars = @{
        LABELDECK_BASE_URL     = $Mirror
        LABELDECK_INSTALL_DIR  = $InstallDir
        LABELDECK_VERSION      = $Version
        PROCESSOR_ARCHITECTURE = $Architecture
    }
    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = (Get-Command pwsh -ErrorAction SilentlyContinue).Source
    if (-not $psi.FileName) { $psi.FileName = (Get-Command powershell).Source }
    $psi.Arguments = "-NoProfile -File `"$resolvedInstaller`""
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    foreach ($name in $envVars.Keys) {
        $psi.EnvironmentVariables[$name] = $envVars[$name]
    }
    $process = [System.Diagnostics.Process]::Start($psi)
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    [pscustomobject]@{
        Code = $process.ExitCode
        Output = $stdout + $stderr
    }
}

function Assert-Fails([object]$Result, [string]$Why, [string]$Needle) {
    if ($Result.Code -eq 0) { Fail "$Why succeeded" }
    if ($Needle -and ($Result.Output -notmatch [regex]::Escape($Needle))) {
        Fail "${Why}: expected output containing `"$Needle`", got: $($Result.Output)"
    }
    Pass
}

function Assert-Succeeds([object]$Result, [string]$Why) {
    if ($Result.Code -ne 0) { Fail "${Why}: $($Result.Output)" }
    Pass
}

# The candidate used by failure tests: not a real executable.
$junk = [System.Text.Encoding]::UTF8.GetBytes('this is not an executable')

# --- Architecture mapping -------------------------------------------------

foreach ($case in @(
    @{ Arch = 'AMD64'; Target = 'x86_64-pc-windows-msvc' },
    @{ Arch = 'ARM64'; Target = 'aarch64-pc-windows-msvc' }
)) {
    $mirror = New-Mirror   # empty: any asset request fails
    $result = Invoke-Installer -Mirror $mirror -InstallDir (Join-Path $root 'install') -Architecture $case.Arch
    Assert-Fails $result "architecture $($case.Arch) mapping" $case.Target
}

$mirror = New-Mirror
$result = Invoke-Installer -Mirror $mirror -InstallDir (Join-Path $root 'install') -Architecture 'MIPS'
Assert-Fails $result 'unsupported architecture' 'unsupported architecture'

# --- Checksum verification ------------------------------------------------

$mirror = New-Mirror
Publish-Asset $mirror 'x86_64-pc-windows-msvc' $junk
# Corrupt the sidecar.
$sidecar = Get-ChildItem $mirror -Recurse -Filter '*.sha256' | Select-Object -First 1
('0' * 64) + '  ' + $sidecar.Name | Set-Content -NoNewline $sidecar.FullName
$result = Invoke-Installer -Mirror $mirror -InstallDir (Join-Path $root 'install')
Assert-Fails $result 'bad checksum' 'checksum verification failed'

$mirror = New-Mirror
Publish-Asset $mirror 'x86_64-pc-windows-msvc' $junk
# Malformed sidecar (not a hex digest).
$sidecar = Get-ChildItem $mirror -Recurse -Filter '*.sha256' | Select-Object -First 1
'not-a-checksum  x' | Set-Content -NoNewline $sidecar.FullName
$result = Invoke-Installer -Mirror $mirror -InstallDir (Join-Path $root 'install')
Assert-Fails $result 'malformed checksum' 'checksum verification failed'

# --- Missing assets -------------------------------------------------------

$mirror = New-Mirror
$result = Invoke-Installer -Mirror $mirror -InstallDir (Join-Path $root 'install')
Assert-Fails $result 'missing archive' 'asset not found in the local mirror'

$mirror = New-Mirror
$name = "labeldeck-$TestTag-x86_64-pc-windows-msvc.zip"
[System.IO.File]::WriteAllBytes((Join-Path $mirror "download\$TestTag\$name"), $junk)
$result = Invoke-Installer -Mirror $mirror -InstallDir (Join-Path $root 'install')
Assert-Fails $result 'missing checksum sidecar' 'labeldeck-v99.88.77-x86_64-pc-windows-msvc.zip.sha256'

# --- Candidate validation -------------------------------------------------

$mirror = New-Mirror
Publish-Asset $mirror 'x86_64-pc-windows-msvc' (New-ZipBytes @{ 'labeldeck.exe' = $junk })
$result = Invoke-Installer -Mirror $mirror -InstallDir (Join-Path $root 'install')
Assert-Fails $result 'corrupt candidate' 'candidate failed --version validation'

$mirror = New-Mirror
$zipWithoutExe = New-ZipBytes @{ 'README.md' = [System.Text.Encoding]::UTF8.GetBytes('readme') }
Publish-Asset $mirror 'x86_64-pc-windows-msvc' $zipWithoutExe
$result = Invoke-Installer -Mirror $mirror -InstallDir (Join-Path $root 'install')
Assert-Fails $result 'archive without labeldeck.exe' 'release archive is missing labeldeck.exe'

# --- Failure must preserve an existing installation -----------------------

$preserveDir = Join-Path $root 'preserve'
New-Item -ItemType Directory -Path $preserveDir -Force | Out-Null
[System.IO.File]::WriteAllText((Join-Path $preserveDir 'labeldeck.exe'), 'old labeldeck')
$mirror = New-Mirror
$sidecarName = "labeldeck-$TestTag-x86_64-pc-windows-msvc.zip.sha256"
Publish-Asset $mirror 'x86_64-pc-windows-msvc' $junk
$sidecar = Join-Path $mirror "download\$TestTag\$sidecarName"
('0' * 64) + "  $sidecarName" | Set-Content -NoNewline $sidecar
$result = Invoke-Installer -Mirror $mirror -InstallDir $preserveDir
Assert-Fails $result 'failed install' 'checksum verification failed'
if ((Get-Content (Join-Path $preserveDir 'labeldeck.exe') -Raw) -ne 'old labeldeck') {
    Fail 'failed installation changed the existing binary'
}
if (Get-ChildItem $preserveDir -Filter '.labeldeck-*' -ErrorAction SilentlyContinue) {
    Fail 'failed installation left staging files'
}
Pass

# --- Latest via local marker ---------------------------------------------

$mirror = New-Mirror
Publish-Asset $mirror 'x86_64-pc-windows-msvc' (New-ZipBytes @{ 'labeldeck.exe' = $junk })
$TestTag | Set-Content (Join-Path $mirror 'latest')
# No LABELDECK_VERSION: the marker provides the tag. The corrupt
# candidate then fails version validation, proving the marker was used.
$psi = [System.Diagnostics.ProcessStartInfo]::new()
$psi.FileName = (Get-Command pwsh -ErrorAction SilentlyContinue).Source
if (-not $psi.FileName) { $psi.FileName = (Get-Command powershell).Source }
$psi.Arguments = "-NoProfile -File `"$resolvedInstaller`""
$psi.UseShellExecute = $false
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.EnvironmentVariables['LABELDECK_BASE_URL'] = $mirror
$psi.EnvironmentVariables['LABELDECK_INSTALL_DIR'] = (Join-Path $root 'install')
$psi.EnvironmentVariables['PROCESSOR_ARCHITECTURE'] = 'AMD64'
$psi.EnvironmentVariables.Remove('LABELDECK_VERSION')
$process = [System.Diagnostics.Process]::Start($psi)
$markerOutput = $process.StandardOutput.ReadToEnd() + $process.StandardError.ReadToEnd()
$process.WaitForExit()
if ($process.ExitCode -eq 0 -or $markerOutput -notmatch 'candidate failed') {
    Fail "local latest marker was not used: $markerOutput"
}
Pass

$mirror = New-Mirror
'not-a-version' | Set-Content (Join-Path $mirror 'latest')
$psi2 = [System.Diagnostics.ProcessStartInfo]::new()
$psi2.FileName = $psi.FileName
$psi2.Arguments = $psi.Arguments
$psi2.UseShellExecute = $false
$psi2.RedirectStandardOutput = $true
$psi2.RedirectStandardError = $true
$psi2.EnvironmentVariables['LABELDECK_BASE_URL'] = $mirror
$psi2.EnvironmentVariables['LABELDECK_INSTALL_DIR'] = (Join-Path $root 'install')
$psi2.EnvironmentVariables['PROCESSOR_ARCHITECTURE'] = 'AMD64'
$psi2.EnvironmentVariables['LABELDECK_BASE_URL'] = $mirror
$psi2.EnvironmentVariables.Remove('LABELDECK_VERSION')
$process = [System.Diagnostics.Process]::Start($psi2)
$badMarkerOutput = $process.StandardOutput.ReadToEnd() + $process.StandardError.ReadToEnd()
$process.WaitForExit()
if ($process.ExitCode -eq 0 -or $badMarkerOutput -notmatch 'usable release tag') {
    Fail "malformed local latest marker accepted: $badMarkerOutput"
}
Pass

# --- Success path: needs a real Windows binary ----------------------------

$builtExe = Join-Path $PSScriptRoot '..\..\..\target\release\labeldeck.exe'
if (-not $IsWindows) {
    Skip 'success-path cases require Windows (real labeldeck.exe)'
} elseif (-not (Test-Path $builtExe)) {
    Fail "success-path cases require a built binary at $builtExe (run cargo make release first)"
} else {
    $realVersion = (& $builtExe --version) -replace '^labeldeck ', ''
    $realTag = "v$realVersion"
    $runnerArch = $env:PROCESSOR_ARCHITECTURE

    $mirror = New-Mirror
    $zip = New-ZipBytes @{ 'labeldeck.exe' = [System.IO.File]::ReadAllBytes($builtExe) }
    $name = "labeldeck-$realTag-$runnerArch-pc-windows-msvc.zip" -replace '-AMD64-', '-x86_64-' -replace '-ARM64-', '-aarch64-'
    $dir = Join-Path $mirror "download\$realTag"
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    [System.IO.File]::WriteAllBytes((Join-Path $dir $name), $zip)
    $hash = (Get-FileHash (Join-Path $dir $name) -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $name" | Set-Content -NoNewline (Join-Path $dir "$name.sha256")

    $installDir = Join-Path $root 'success'
    $result = Invoke-Installer -Mirror $mirror -InstallDir $installDir -Version $realTag -Architecture $runnerArch
    Assert-Succeeds $result 'successful installation'
    $installed = Join-Path $installDir 'labeldeck.exe'
    if (-not (Test-Path $installed)) { Fail 'installed binary missing' }
    if ((& $installed --version) -ne "labeldeck $realVersion") { Fail 'installed binary reports the wrong version' }
    Pass

    # Replacement of an existing installation.
    $result = Invoke-Installer -Mirror $mirror -InstallDir $installDir -Version $realTag -Architecture $runnerArch
    Assert-Succeeds $result 'replacement of existing installation'
    if ((& $installed --version) -ne "labeldeck $realVersion") { Fail 'replaced binary is wrong' }
    Pass

    # Wrong advertised version must be rejected and change nothing.
    $before = (Get-Item $installed).Length
    $result = Invoke-Installer -Mirror $mirror -InstallDir $installDir -Version 'v0.0.0' -Architecture $runnerArch
    Assert-Fails $result 'wrong advertised version' 'asset not found in the local mirror'
    if ((Get-Item $installed).Length -ne $before) { Fail 'wrong-version attempt changed the installation' }
    Pass
}

Write-Host "PowerShell installer tests passed: $($script:Passes) cases ($($script:Skipped) skipped)."
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
