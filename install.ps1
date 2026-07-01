$ErrorActionPreference = "Stop"

$Repo = if ($env:RUNX_REPO) { $env:RUNX_REPO } else { "your-org/runx" }
$Version = if ($env:RUNX_VERSION) { $env:RUNX_VERSION } else { "latest" }
$InstallDir = if ($env:RUNX_INSTALL_DIR) { $env:RUNX_INSTALL_DIR } else { Join-Path $HOME ".runx\bin" }

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "x64" }
    "ARM64" { "arm64" }
    default { throw "Unsupported architecture: $env:PROCESSOR_ARCHITECTURE" }
}

$asset = "runx-windows-$arch.zip"
if ($Version -eq "latest") {
    $url = "https://github.com/$Repo/releases/latest/download/$asset"
} else {
    $url = "https://github.com/$Repo/releases/download/$Version/$asset"
}

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tmp | Out-Null
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

try {
    $archive = Join-Path $tmp $asset
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $archive
    Expand-Archive -Path $archive -DestinationPath $tmp -Force
    Copy-Item -Path (Join-Path $tmp "runx.exe") -Destination (Join-Path $InstallDir "runx.exe") -Force
    Write-Host "Installed runx to $(Join-Path $InstallDir "runx.exe")"
    if (($env:PATH -split ';') -notcontains $InstallDir) {
        Write-Host "Add $InstallDir to PATH to run runx from any directory."
    }
}
finally {
    Remove-Item -Recurse -Force $tmp
}
