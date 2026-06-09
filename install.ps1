<#
.SYNOPSIS
    molde installer for Windows — downloads the latest prebuilt binary and adds
    it to your PATH. No Rust toolchain required.

.EXAMPLE
    irm https://raw.githubusercontent.com/MAWESI-SAS/molde/main/install.ps1 | iex

.NOTES
    Environment overrides:
      MOLDE_INSTALL_DIR   target directory (default: %LOCALAPPDATA%\Programs\molde)
      MOLDE_VERSION       a specific release tag, e.g. v0.0.5 (default: latest)
#>

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$repo = 'MAWESI-SAS/molde'
$bin  = 'molde'

# Only x86_64-pc-windows-msvc is published today.
$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -ne 'AMD64') {
    throw "no prebuilt Windows binary for '$arch' yet. Build from source: see docs/install.md."
}

# Resolve the release tag.
$tag = $env:MOLDE_VERSION
if (-not $tag) {
    Write-Host "==> looking up the latest release..."
    $tag = (Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest").tag_name
    if (-not $tag) { throw "could not determine the latest version (set MOLDE_VERSION)." }
}

$asset = "$bin-$tag-x86_64-pc-windows-msvc.zip"
$url   = "https://github.com/$repo/releases/download/$tag/$asset"

# Pick an install dir.
$dir = $env:MOLDE_INSTALL_DIR
if (-not $dir) { $dir = Join-Path $env:LOCALAPPDATA 'Programs\molde' }
New-Item -ItemType Directory -Force $dir | Out-Null

# Download & extract.
$zip = Join-Path ([IO.Path]::GetTempPath()) $asset
Write-Host "==> downloading $asset"
Invoke-WebRequest -Uri $url -OutFile $zip
Expand-Archive -Path $zip -DestinationPath $dir -Force
Remove-Item $zip -Force

# Add to the user PATH if missing.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $dir) {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$dir", 'User')
    Write-Host "==> added $dir to your PATH (open a NEW terminal to pick it up)"
}

$exe = Join-Path $dir "$bin.exe"
$ver = (& $exe --version) 2>$null
if (-not $ver) { $ver = $tag }
Write-Host "==> installed $ver to $exe"
Write-Host "Run ``molde --help`` to get started (in a new terminal)."
