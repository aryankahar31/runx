$ErrorActionPreference = "Stop"

$root = Join-Path $env:RUNNER_TEMP ([System.Guid]::NewGuid().ToString())
$assets = Join-Path $root "assets"
$installDir = Join-Path $root "installed"
$wrapper = Join-Path $root "run-installer.ps1"
$pwsh = (Get-Process -Id $PID).Path
$repoRoot = Split-Path -Parent $PSScriptRoot
$releaseBinary = Join-Path $repoRoot "target/release/runx.exe"
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x64" }
$asset = "runx-windows-$arch.zip"

function Invoke-Installer {
    $output = & $pwsh -NoProfile -File $wrapper 2>&1
    return @{ Code = $LASTEXITCODE; Output = ($output -join "`n") }
}

function Assert-Version {
    $installed = Join-Path $installDir "runx.exe"
    $reported = & $installed --version
    if ($LASTEXITCODE -ne 0 -or $reported -ne "runx 0.6.0") {
        throw "Installed executable reported '$reported'"
    }
}

try {
    New-Item -ItemType Directory -Path $assets, $installDir -Force | Out-Null
    if (-not (Test-Path $releaseBinary)) { throw "Missing release binary: $releaseBinary" }
    Compress-Archive -Path $releaseBinary -DestinationPath (Join-Path $assets $asset)

    $env:RUNX_TEST_ASSETS = $assets
    $env:RUNX_INSTALL_DIR = $installDir
    $env:RUNX_VERSION = "v0.6.0"
    Remove-Item Env:RUNX_REQUIRE_SIGNATURE -ErrorAction SilentlyContinue
    @'
$ErrorActionPreference = "Stop"
$env:PATH = "$env:SystemRoot\System32;$env:SystemRoot"
function Invoke-WebRequest {
    param([string]$Uri, [string]$OutFile, [switch]$UseBasicParsing)
    $name = [System.IO.Path]::GetFileName(([uri]$Uri).AbsolutePath)
    Copy-Item -Path (Join-Path $env:RUNX_TEST_ASSETS $name) -Destination $OutFile
}
. (Join-Path $env:GITHUB_WORKSPACE "install.ps1") -NoModifyPath
'@ | Set-Content -Path $wrapper

    $digest = (Get-FileHash (Join-Path $assets $asset) -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content (Join-Path $assets "SHA256SUMS") ("{0}  {1}" -f $digest, $asset)

    $fresh = Invoke-Installer
    if ($fresh.Code -ne 0) { throw "Fresh install failed: $($fresh.Output)" }
    Assert-Version

    $installed = Join-Path $installDir "runx.exe"
    Set-Content (Join-Path $assets "SHA256SUMS") ("{0}  {1}" -f ("0" * 64), $asset)
    $badChecksum = Invoke-Installer
    if ($badChecksum.Code -eq 0) { throw "Bad checksum was accepted" }
    Assert-Version

    $archive = Join-Path $assets $asset
    $savedArchive = Join-Path $root "valid.zip"
    Copy-Item $archive $savedArchive
    Set-Content $archive "not a zip archive"
    $badArchiveDigest = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content (Join-Path $assets "SHA256SUMS") ("{0}  {1}" -f $badArchiveDigest, $asset)
    $badArchive = Invoke-Installer
    if ($badArchive.Code -eq 0) { throw "Invalid archive was accepted" }
    Assert-Version

    Copy-Item $savedArchive $archive -Force
    Set-Content (Join-Path $assets "SHA256SUMS") ("{0}  {1}" -f $digest, $asset)
    $upgrade = Invoke-Installer
    if ($upgrade.Code -ne 0) { throw "Upgrade failed: $($upgrade.Output)" }
    Assert-Version

    $originalArchitecture = $env:PROCESSOR_ARCHITECTURE
    $env:PROCESSOR_ARCHITECTURE = "ARM64"
    try {
        $unsupported = Invoke-Installer
        if ($unsupported.Code -eq 0 -or $unsupported.Output -notmatch "ARM64 release assets") {
            throw "Unsupported Windows ARM64 target was not rejected clearly: $($unsupported.Output)"
        }
    }
    finally {
        $env:PROCESSOR_ARCHITECTURE = $originalArchitecture
    }
    Assert-Version
    if ((Get-ChildItem $installDir -Filter ".runx-*").Count -ne 0) {
        throw "Installer left staging files behind"
    }
}
finally {
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
}
