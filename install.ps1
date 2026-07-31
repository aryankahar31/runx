$ErrorActionPreference = "Stop"

$Repo = if ($env:RUNX_REPO) { $env:RUNX_REPO } else { "aryankahar31/runx" }
$Version = if ($env:RUNX_VERSION) { $env:RUNX_VERSION } else { "latest" }
$InstallDir = if ($env:RUNX_INSTALL_DIR) { $env:RUNX_INSTALL_DIR } else { Join-Path $HOME ".runx\bin" }

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "x64" }
    "ARM64" { "arm64" }
    default { throw "Unsupported architecture: $env:PROCESSOR_ARCHITECTURE" }
}

$asset = "runx-windows-$arch.zip"
if ($Version -eq "latest") {
    $baseUrl = "https://github.com/$Repo/releases/latest/download"
} else {
    $baseUrl = "https://github.com/$Repo/releases/download/$Version"
}
$url = "$baseUrl/$asset"

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tmp | Out-Null
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

# ---------------------------------------------------------------------------
# SHA-256 checksum verification
# ---------------------------------------------------------------------------

# Verify a downloaded file against a SHA256SUMS manifest.
#
# The filename field is compared exactly rather than with a substring match.
# A substring match can select the digest of a different artifact whose name
# merely contains ours (e.g. an `.asc` signature line matching a request for
# the archive itself). The digest is also required to be 64 hex characters, so
# an HTML error page served in place of the manifest cannot pass as a hash.
function Test-Checksum {
    param(
        [string]$FilePath,
        [string]$FileName,
        [string]$ChecksumFile
    )

    $expected = $null
    foreach ($line in Get-Content -Path $ChecksumFile) {
        $fields = ($line.Trim() -split '\s+')
        if ($fields.Count -lt 2) { continue }

        $hash = $fields[0]
        # Strip the coreutils binary-mode marker, then compare basenames so a
        # manifest listing `./dist/runx-...` still matches.
        $name = $fields[1] -replace '^\*', ''
        $name = $name -replace '^.*[\\/]', ''

        if ($name -eq $FileName -and $hash -match '^[0-9a-fA-F]{64}$') {
            $expected = $hash.ToLower()
            break
        }
    }

    if (-not $expected) {
        Write-Error "Error: no valid SHA-256 entry for $FileName in SHA256SUMS."
        return $false
    }

    $computed = (Get-FileHash -Path $FilePath -Algorithm SHA256).Hash.ToLower()

    if ($expected -eq $computed) {
        Write-Host "Checksum verified."
        return $true
    } else {
        Write-Error @"
Error: checksum verification failed for $FileName.
Expected: $expected
Got:      $computed
This may indicate a corrupted download or a compromised release. Aborting.
"@
        return $false
    }
}

# ---------------------------------------------------------------------------
# Download, verify, and install
# ---------------------------------------------------------------------------

try {
    # TLS 1.2 is not the default on older Windows PowerShell hosts, where
    # Invoke-WebRequest against GitHub then fails with an opaque error.
    try {
        [Net.ServicePointManager]::SecurityProtocol =
            [Net.SecurityProtocolType]::Tls12 -bor [Net.ServicePointManager]::SecurityProtocol
    } catch {
        # Newer hosts negotiate TLS automatically; ignore.
    }

    $archive = Join-Path $tmp $asset
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing

    # Download SHA256SUMS and verify
    $checksumUrl = "$baseUrl/SHA256SUMS"
    $checksumFile = Join-Path $tmp "SHA256SUMS"
    Write-Host "Downloading SHA256SUMS"
    Invoke-WebRequest -Uri $checksumUrl -OutFile $checksumFile -UseBasicParsing

    if (-not (Test-Checksum -FilePath $archive -FileName $asset -ChecksumFile $checksumFile)) {
        exit 1
    }

    Expand-Archive -Path $archive -DestinationPath $tmp -Force

    $extracted = Join-Path $tmp "runx.exe"
    if (-not (Test-Path $extracted)) {
        Write-Error "Error: archive did not contain the expected runx.exe binary."
        exit 1
    }

    Copy-Item -Path $extracted -Destination (Join-Path $InstallDir "runx.exe") -Force
    Write-Host "Installed runx to $(Join-Path $InstallDir "runx.exe")"
    if (($env:PATH -split ';') -notcontains $InstallDir) {
        Write-Host "Add $InstallDir to PATH to run runx from any directory."
    }
}
finally {
    Remove-Item -Recurse -Force $tmp
}
