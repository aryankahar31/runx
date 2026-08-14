param(
    [switch]$NoModifyPath
)

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

function Write-PathInstructions {
    param([string]$PathToAdd)

    Write-Host "Runx is installed but not on your User PATH."
    Write-Host "Add this directory to the User Path in Windows Environment Variables:"
    Write-Host ""
    Write-Host "  $PathToAdd"
    Write-Host ""
    Write-Host "Open a new PowerShell window after updating it."
}

function Test-UserPathContains {
    param([string]$PathToFind, [string]$UserPath)

    $normalisedTarget = $PathToFind.Trim().TrimEnd('\', '/')
    return @($UserPath -split ';' | Where-Object {
        $_.Trim().TrimEnd('\', '/') -ieq $normalisedTarget
    }).Count -gt 0
}

function Update-UserPath {
    param([string]$PathToAdd)

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (Test-UserPathContains -PathToFind $PathToAdd -UserPath $userPath) {
        return
    }

    $canPrompt = $false
    try {
        $null = $Host.UI.RawUI
        $canPrompt = -not [Console]::IsInputRedirected
    } catch {
        $canPrompt = $false
    }

    if ($NoModifyPath -or -not $canPrompt) {
        Write-PathInstructions -PathToAdd $PathToAdd
        return
    }

    $reply = Read-Host "Runx is installed but not on your User PATH. Add it automatically? [Y/n]"
    if ($reply -match '^(|y|yes)$') {
        $newUserPath = if ([string]::IsNullOrWhiteSpace($userPath)) {
            $PathToAdd
        } else {
            "$($userPath.TrimEnd(';'));$PathToAdd"
        }
        [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
        Write-Host "Added runx to your User PATH. Open a new PowerShell window to use it."
    } else {
        Write-PathInstructions -PathToAdd $PathToAdd
    }
}

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

    # -----------------------------------------------------------------------
    # Sigstore/cosign signature verification (graceful)
    #
    # Since v0.4.2 every release asset carries a Sigstore bundle signed
    # keylessly by the release workflow. cosign is not a hard dependency: a
    # missing cosign (or a pre-signature release) prints a warning and the
    # install continues — checksum verification above already failed closed.
    # RUNX_REQUIRE_SIGNATURE=1 makes that a hard error instead. A signature
    # that is present but FAILS verification is always fatal.
    # -----------------------------------------------------------------------
    $SigstoreIdentity = '^https://github\.com/aryankahar31/runx/\.github/workflows/release\.yml@refs/(tags/v[0-9]+\.[0-9]+\.[0-9]+|heads/main)$'
    $cosign = Get-Command cosign -ErrorAction SilentlyContinue
    if ($cosign) {
        $bundleFile = Join-Path $tmp "$asset.sigstore.json"
        try {
            Invoke-WebRequest -Uri "$baseUrl/$asset.sigstore.json" -OutFile $bundleFile -UseBasicParsing
        } catch {
            if ($env:RUNX_REQUIRE_SIGNATURE -eq "1") {
                Write-Error "RUNX_REQUIRE_SIGNATURE=1 but no signature bundle was published for this release."
                exit 1
            }
            Write-Warning "No signature bundle published for this release; signature verification skipped (checksum still verified)."
            $bundleFile = $null
        }
        if ($bundleFile) {
            & cosign verify-blob $archive `
                --bundle $bundleFile `
                --certificate-identity-regexp $SigstoreIdentity `
                --certificate-oidc-issuer https://token.actions.githubusercontent.com
            if ($LASTEXITCODE -ne 0) {
                Write-Error @"
Error: signature verification FAILED for the downloaded archive.
The Sigstore signature does not match the release workflow's identity or the
artifact has been tampered with. Aborting.
"@
                exit 1
            }
            Write-Host "Signature verified."
        }
    } else {
        if ($env:RUNX_REQUIRE_SIGNATURE -eq "1") {
            Write-Error "RUNX_REQUIRE_SIGNATURE=1 but cosign is not installed."
            exit 1
        }
        Write-Warning "cosign not found - release signature verification skipped. The SHA-256 checksum was still verified and enforced. Install cosign (https://docs.sigstore.dev/cosign/installation/) to verify release signatures."
    }

    Expand-Archive -Path $archive -DestinationPath $tmp -Force

    $extracted = Join-Path $tmp "runx.exe"
    if (-not (Test-Path $extracted)) {
        Write-Error "Error: archive did not contain the expected runx.exe binary."
        exit 1
    }

    Copy-Item -Path $extracted -Destination (Join-Path $InstallDir "runx.exe") -Force
    Write-Host "Installed runx to $(Join-Path $InstallDir "runx.exe")"
    Update-UserPath -PathToAdd $InstallDir
}
finally {
    Remove-Item -Recurse -Force $tmp
}
