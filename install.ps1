$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repository = "lassejlv/paper-cli"
$installDir = if ($env:PAPER_INSTALL_DIR) {
    $env:PAPER_INSTALL_DIR
} else {
    Join-Path $HOME ".local\bin"
}
$version = if ($env:PAPER_VERSION) {
    $env:PAPER_VERSION
} else {
    "latest"
}

function Write-Message {
    param([Parameter(Mandatory = $true)][string]$Message)

    Write-Host $Message
}

function Stop-Install {
    param([Parameter(Mandatory = $true)][string]$Message)

    throw "paper installer: $Message"
}

if ($env:OS -ne "Windows_NT") {
    Stop-Install "this installer supports Windows only; use install.sh on macOS or Linux"
}

[System.Net.ServicePointManager]::SecurityProtocol =
    [System.Net.ServicePointManager]::SecurityProtocol -bor [System.Net.SecurityProtocolType]::Tls12

$architecture = switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture) {
    "X64" { "x86_64" }
    "Arm64" { "aarch64" }
    default {
        Stop-Install "unsupported CPU architecture: $([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture)"
    }
}

$target = "$architecture-pc-windows-msvc"
$archive = "paper-$target.zip"

if ($version -eq "latest") {
    $downloadRoot = "https://github.com/$repository/releases/latest/download"
} elseif ($version -match "^v[0-9]") {
    $downloadRoot = "https://github.com/$repository/releases/download/$version"
} elseif ($version -match "^[0-9]") {
    $version = "v$version"
    $downloadRoot = "https://github.com/$repository/releases/download/$version"
} else {
    Stop-Install "invalid PAPER_VERSION '$version'; expected 'latest' or a version such as 'v0.1.0'"
}

$temporaryDir = Join-Path ([System.IO.Path]::GetTempPath()) "paper-cli-$([System.Guid]::NewGuid())"
$stagedBinary = $null

try {
    New-Item -ItemType Directory -Path $temporaryDir | Out-Null
    $archivePath = Join-Path $temporaryDir $archive
    $checksumsPath = Join-Path $temporaryDir "SHA256SUMS"

    Write-Message "Downloading paper for $target..."
    Invoke-WebRequest -UseBasicParsing -Uri "$downloadRoot/$archive" -OutFile $archivePath
    Invoke-WebRequest -UseBasicParsing -Uri "$downloadRoot/SHA256SUMS" -OutFile $checksumsPath

    $matchingChecksums = @(
        Get-Content -LiteralPath $checksumsPath | ForEach-Object {
            if ($_ -match "^([0-9A-Fa-f]{64})\s+\*?(.+)$" -and $Matches[2] -eq $archive) {
                $Matches[1].ToLowerInvariant()
            }
        }
    )
    if ($matchingChecksums.Count -ne 1) {
        Stop-Install "release checksums do not contain exactly one entry for $archive"
    }

    $actualChecksum = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualChecksum -ne $matchingChecksums[0]) {
        Stop-Install "checksum verification failed for $archive"
    }

    Expand-Archive -LiteralPath $archivePath -DestinationPath $temporaryDir
    $downloadedBinary = Join-Path $temporaryDir "paper.exe"
    if (-not (Test-Path -LiteralPath $downloadedBinary -PathType Leaf)) {
        Stop-Install "release archive does not contain paper.exe"
    }

    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    $installDir = [System.IO.Path]::GetFullPath($installDir)
    $installedBinary = Join-Path $installDir "paper.exe"
    $stagedBinary = Join-Path $installDir ".paper-install.$PID.exe"
    Copy-Item -LiteralPath $downloadedBinary -Destination $stagedBinary
    if (Test-Path -LiteralPath $installedBinary -PathType Leaf) {
        [System.IO.File]::Replace($stagedBinary, $installedBinary, $null)
    } else {
        [System.IO.File]::Move($stagedBinary, $installedBinary)
    }
    $stagedBinary = $null

    Write-Message "Installed paper to $installedBinary"
    if (-not (($env:PATH -split ";") -contains $installDir)) {
        Write-Message "Add $installDir to PATH to run paper from any directory."
    }
} finally {
    if ($stagedBinary -and (Test-Path -LiteralPath $stagedBinary)) {
        Remove-Item -LiteralPath $stagedBinary -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $temporaryDir) {
        Remove-Item -LiteralPath $temporaryDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
