# labeldeck installer for Windows (PowerShell 5.1 and 7+).
#
# Downloads the correct release archive for this machine's architecture,
# verifies its SHA-256, runs the candidate binary's --version as a smoke
# test, and only then replaces any existing installation. A failure at
# any point leaves an existing installation untouched.
#
# Environment variables:
#   LABELDECK_VERSION     install a specific release tag (e.g. v0.1.0)
#   LABELDECK_INSTALL_DIR override the installation directory
#   LABELDECK_BASE_URL    override the release base: an https:// URL, or
#                         a local directory acting as a release mirror
#                         (used by the installer tests and offline setups)
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Die([string]$Message) {
    Write-Error $Message -ErrorAction Stop
    exit 1
}

if ($PSVersionTable.PSVersion.Major -lt 6) {
    # Windows PowerShell: make sure TLS 1.2 is enabled for GitHub.
    try {
        [Net.ServicePointManager]::SecurityProtocol = `
            [Net.ServicePointManager]::SecurityProtocol -bor `
            [Net.SecurityProtocolType]::Tls12
    } catch { }
}

$script:ReleasesBase = if ($env:LABELDECK_BASE_URL) {
    $env:LABELDECK_BASE_URL
} else {
    'https://github.com/seapagan/labeldeck/releases'
}

function Test-IsHttp([string]$Url) {
    $Url -match '^https?://'
}

function Get-Target {
    $arch = switch ($env:PROCESSOR_ARCHITECTURE) {
        'AMD64' { 'x86_64'; break }
        'ARM64' { 'aarch64'; break }
        default { Die "unsupported architecture: $($env:PROCESSOR_ARCHITECTURE)" }
    }
    "$arch-pc-windows-msvc"
}

function New-HttpClient([bool]$AllowRedirect) {
    Add-Type -AssemblyName System.Net.Http -ErrorAction SilentlyContinue
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.AllowAutoRedirect = $AllowRedirect
    [System.Net.Http.HttpClient]::new($handler)
}

function Resolve-Version {
    if ($env:LABELDECK_VERSION) { return $env:LABELDECK_VERSION }

    if (-not (Test-IsHttp $script:ReleasesBase)) {
        # Local mirror mode: the tag is read from a "latest" marker file.
        $marker = Join-Path $script:ReleasesBase 'latest'
        if (-not (Test-Path $marker)) {
            Die 'no LABELDECK_VERSION set and the local mirror has no "latest" marker'
        }
        $tag = (Get-Content $marker -TotalCount 1).Trim()
        if ($tag -notmatch '^(v[0-9]+\.[0-9]+\.[0-9]+)$') {
            Die "the local latest marker did not resolve to a usable release tag: $tag"
        }
        return $tag
    }

    $client = New-HttpClient -AllowRedirect:$false
    try {
        $response = $client.GetAsync("$script:ReleasesBase/latest").GetAwaiter().GetResult()
        $code = [int]$response.StatusCode
        if ($code -notin 301, 302, 303, 307, 308) {
            Die "could not resolve the latest release (HTTP $code)"
        }
        $location = $response.Headers.Location
        if (-not $location) {
            Die 'the latest-release endpoint did not redirect to a tag'
        }
    } finally { $client.Dispose() }

    $tag = $location.ToString()
    if ($tag -notmatch '/tag/(v[0-9]+\.[0-9]+\.[0-9]+)$') {
        Die "the latest release redirect did not resolve to a usable release tag: $tag"
    }
    $Matches[1]
}

function Save-Asset([string]$Url, [string]$Destination) {
    if (-not (Test-IsHttp $script:ReleasesBase)) {
        # Local mirror mode: assets are plain files under download/<tag>/.
        $relative = $Url.Substring($script:ReleasesBase.Length).TrimStart('/', '\')
        $source = Join-Path $script:ReleasesBase $relative
        if (-not (Test-Path $source)) {
            Die "could not download ${Url}: asset not found in the local mirror"
        }
        Copy-Item $source $Destination -Force
        return
    }
    $client = New-HttpClient -AllowRedirect:$true
    try {
        $bytes = $client.GetByteArrayAsync($Url).GetAwaiter().GetResult()
        [System.IO.File]::WriteAllBytes($Destination, $bytes)
    } catch {
        Die "could not download ${Url}: $($_.Exception.Message)"
    } finally { $client.Dispose() }
}

function Assert-Checksum([string]$Archive, [string]$Sidecar) {
    if (-not (Test-Path $Sidecar)) { Die 'checksum sidecar is missing' }
    $expected = (Get-Content $Sidecar -TotalCount 1).Split(' ')[0]
    if ($expected -notmatch '^[0-9a-fA-F]{64}$') { Die 'checksum verification failed' }
    $actual = (Get-FileHash $Archive -Algorithm SHA256).Hash
    if (-not $actual.Equals($expected, [System.StringComparison]::OrdinalIgnoreCase)) {
        Die 'checksum verification failed'
    }
}

function Assert-CandidateVersion([string]$Exe, [string]$Version) {
    $reported = $null
    try { $reported = (& $Exe --version) 2>$null } catch { }
    $exitOk = $false
    if (Get-Variable -Name LASTEXITCODE -ErrorAction SilentlyContinue) {
        $exitOk = ($LASTEXITCODE -eq 0)
    }
    if (-not $exitOk -or -not $reported) {
        Die 'candidate failed --version validation'
    }
    $expected = "labeldeck $($Version -replace '^v', '')"
    if ($reported -ne $expected) {
        Die "reported version does not match ${Version}: $reported"
    }
}

$target = Get-Target
$version = Resolve-Version
$asset = "labeldeck-$version-$target.zip"

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("labeldeck-install-" + [System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $archive = Join-Path $tmp $asset
    $sidecar = "$archive.sha256"
    Save-Asset "$script:ReleasesBase/download/$version/$asset" $archive
    Save-Asset "$script:ReleasesBase/download/$version/$asset.sha256" $sidecar
    Assert-Checksum $archive $sidecar

    $extracted = Join-Path $tmp 'extracted'
    Expand-Archive -Path $archive -DestinationPath $extracted
    $candidate = Join-Path $extracted 'labeldeck.exe'
    if (-not (Test-Path $candidate)) { Die 'release archive is missing labeldeck.exe' }
    Assert-CandidateVersion $candidate $version

    if ($env:LABELDECK_INSTALL_DIR) {
        $installDir = $env:LABELDECK_INSTALL_DIR
    } elseif ($env:LOCALAPPDATA) {
        $installDir = Join-Path $env:LOCALAPPDATA 'Programs\labeldeck'
    } else {
        Die 'no install directory could be determined; set LABELDECK_INSTALL_DIR'
    }
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null

    # Stage inside the destination, then move over any existing binary.
    $staged = Join-Path $installDir '.labeldeck-new.exe'
    Copy-Item $candidate $staged -Force
    try {
        Move-Item $staged (Join-Path $installDir 'labeldeck.exe') -Force
    } catch {
        Remove-Item $staged -Force -ErrorAction SilentlyContinue
        Die "could not replace the installed binary: $($_.Exception.Message)"
    }
} finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "Installed labeldeck $version to $installDir."
$pathComponents = ($env:Path -split ';') | Where-Object { $_ }
if ($pathComponents -notcontains $installDir) {
    Write-Warning "$installDir is not on PATH; add it to PATH to use labeldeck."
}
