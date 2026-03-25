[CmdletBinding()]
param(
    [string]$Version,
    [switch]$Nightly,
    [string]$InstallDir = $env:TT_SYNC_INSTALL_DIR,
    [string]$Repo = $(if ($env:TT_SYNC_REPO) { $env:TT_SYNC_REPO } else { 'Darkatse/TT-Sync' })
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
} catch {
}

function Fail([string]$Message) {
    throw $Message
}

function Write-Info([string]$Message) {
    Write-Host $Message
}

function Normalize-Version([string]$Value) {
    if ([string]::IsNullOrWhiteSpace($Value)) {
        return $null
    }

    if ($Value -eq 'nightly' -or $Value.StartsWith('v')) {
        return $Value
    }

    if ($Value -match '^[0-9]') {
        return "v$Value"
    }

    return $Value
}

function Resolve-AssetName {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    switch ($arch) {
        'x64' {
            return 'tt-sync-windows-x64.exe'
        }
        'arm64' {
            Write-Info 'Windows ARM64 detected. Installing the x64 build via emulation.'
            return 'tt-sync-windows-x64.exe'
        }
        default {
            Fail "Unsupported Windows architecture: $arch"
        }
    }
}

function Test-Url([string]$Url) {
    try {
        Invoke-WebRequest -Uri $Url -Method Head | Out-Null
        return $true
    } catch {
        return $false
    }
}

function Resolve-ReleaseBase([string]$AssetName, [string]$NormalizedVersion, [bool]$UseNightly) {
    $releaseRoot = "https://github.com/$Repo/releases"

    if ($NormalizedVersion) {
        return @{
            BaseUrl = "$releaseRoot/download/$NormalizedVersion"
            Label = "release $NormalizedVersion"
        }
    }

    if ($UseNightly) {
        return @{
            BaseUrl = "$releaseRoot/download/nightly"
            Label = 'nightly'
        }
    }

    $latestAssetUrl = "$releaseRoot/latest/download/$AssetName"
    $latestChecksumsUrl = "$releaseRoot/latest/download/SHA256SUMS.txt"
    if ((Test-Url $latestAssetUrl) -and (Test-Url $latestChecksumsUrl)) {
        return @{
            BaseUrl = "$releaseRoot/latest/download"
            Label = 'latest stable release'
        }
    }

    return @{
        BaseUrl = "$releaseRoot/download/nightly"
        Label = 'nightly'
    }
}

function Add-ToUserPath([string]$Dir) {
    $currentUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @()
    if (-not [string]::IsNullOrWhiteSpace($currentUserPath)) {
        $entries = $currentUserPath -split ';' | Where-Object { $_ }
    }

    if ($entries -contains $Dir) {
        return $false
    }

    $newUserPath = if ([string]::IsNullOrWhiteSpace($currentUserPath)) {
        $Dir
    } else {
        "$currentUserPath;$Dir"
    }

    [Environment]::SetEnvironmentVariable('Path', $newUserPath, 'User')

    $processEntries = $env:Path -split ';' | Where-Object { $_ }
    if (-not ($processEntries -contains $Dir)) {
        $env:Path = if ([string]::IsNullOrWhiteSpace($env:Path)) {
            $Dir
        } else {
            "$env:Path;$Dir"
        }
    }

    return $true
}

$NormalizedVersion = Normalize-Version $Version
if ($Nightly -and $NormalizedVersion) {
    Fail 'Use either -Nightly or -Version, not both.'
}

if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $LocalAppData = [Environment]::GetFolderPath('LocalApplicationData')
    $InstallDir = Join-Path $LocalAppData 'TT-Sync\bin'
}

$AssetName = Resolve-AssetName
$Release = Resolve-ReleaseBase -AssetName $AssetName -NormalizedVersion $NormalizedVersion -UseNightly $Nightly.IsPresent

$TempDir = Join-Path ([IO.Path]::GetTempPath()) ("tt-sync-install-" + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $TempDir -Force

try {
    $AssetPath = Join-Path $TempDir $AssetName
    $ChecksumsPath = Join-Path $TempDir 'SHA256SUMS.txt'
    $DestinationPath = Join-Path $InstallDir 'tt-sync.exe'

    Write-Info "Installing TT-Sync from $($Release.Label)"
    Write-Info "Downloading $AssetName"

    Invoke-WebRequest -Uri "$($Release.BaseUrl)/$AssetName" -OutFile $AssetPath
    Invoke-WebRequest -Uri "$($Release.BaseUrl)/SHA256SUMS.txt" -OutFile $ChecksumsPath

    $ChecksumPattern = [regex]::Escape("  $AssetName") + '$'
    $ExpectedLine = Get-Content $ChecksumsPath | Where-Object { $_ -match $ChecksumPattern } | Select-Object -First 1
    if (-not $ExpectedLine) {
        Fail "Checksum entry for $AssetName not found."
    }

    $ExpectedHash = ($ExpectedLine -split '\s+')[0].ToLowerInvariant()
    $ActualHash = (Get-FileHash -Path $AssetPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($ExpectedHash -ne $ActualHash) {
        Fail "Checksum mismatch for $AssetName."
    }

    $null = New-Item -ItemType Directory -Path $InstallDir -Force
    Copy-Item -Path $AssetPath -Destination $DestinationPath -Force

    $PathUpdated = Add-ToUserPath -Dir $InstallDir

    Write-Info "Installed to $DestinationPath"
    if ($PathUpdated) {
        Write-Info "Added $InstallDir to the user PATH. Open a new shell if tt-sync is not visible yet."
    } else {
        Write-Info 'The install directory is already on PATH.'
    }

    Write-Info "Run 'tt-sync.exe --help' to get started."
} finally {
    if (Test-Path $TempDir) {
        Remove-Item -LiteralPath $TempDir -Recurse -Force
    }
}
