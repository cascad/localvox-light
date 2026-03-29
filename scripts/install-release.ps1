# Download GitHub Release binary + run setup-vosk into the same folder (vosk-lib, models).
# Requires: PowerShell 5+, a published Release with assets (see README). Artifacts from Actions alone are not enough.
#
# Env: LOCALVOX_LIGHT_REPO (owner/name), LOCALVOX_LIGHT_TAG (or "latest"), LOCALVOX_LIGHT_INSTALL_DIR, LOCALVOX_LIGHT_BRANCH (for raw setup script)
#
# Example:
#   iwr -useb https://raw.githubusercontent.com/cascad/localvox-light/main/scripts/install-release.ps1 | iex
#   (or save file and:) .\install-release.ps1 -InstallDir D:\apps\localvox-light

param(
    [string]$InstallDir = "",
    [string]$Repo = "",
    [string]$Tag = "",
    [string]$Branch = "",
    [switch]$SkipVosk,
    [switch]$SkipBinary
)

$ErrorActionPreference = "Stop"
if (-not $Repo) { $Repo = $(if ($env:LOCALVOX_LIGHT_REPO) { $env:LOCALVOX_LIGHT_REPO } else { "cascad/localvox-light" }) }
if (-not $Tag) { $Tag = $(if ($env:LOCALVOX_LIGHT_TAG) { $env:LOCALVOX_LIGHT_TAG } else { "latest" }) }
if (-not $Branch) { $Branch = $(if ($env:LOCALVOX_LIGHT_BRANCH) { $env:LOCALVOX_LIGHT_BRANCH } else { "main" }) }
if (-not $InstallDir) {
    $InstallDir = if ($env:LOCALVOX_LIGHT_INSTALL_DIR) { $env:LOCALVOX_LIGHT_INSTALL_DIR } else { Join-Path $env:USERPROFILE "localvox-light" }
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$InstallDir = (Resolve-Path -LiteralPath $InstallDir).Path

if (-not $SkipVosk) {
    $setupUrl = "https://raw.githubusercontent.com/$Repo/$Branch/scripts/setup-vosk.ps1"
    $tmpSetup = Join-Path $env:TEMP ("lv-setup-vosk-" + [Guid]::NewGuid().ToString() + ".ps1")
    Write-Host "Fetching $setupUrl"
    Invoke-WebRequest -Uri $setupUrl -OutFile $tmpSetup -UserAgent "localvox-light-install/1.0"
    try {
        & powershell -NoProfile -ExecutionPolicy Bypass -File $tmpSetup -InstallRoot $InstallDir
    } finally {
        Remove-Item -LiteralPath $tmpSetup -Force -ErrorAction SilentlyContinue
    }
}

if (-not $SkipBinary) {
    $api = if ($Tag -eq "latest") {
        "https://api.github.com/repos/$Repo/releases/latest"
    } else {
        "https://api.github.com/repos/$Repo/releases/tags/$Tag"
    }
    Write-Host "Release API: $api"
    $rel = Invoke-RestMethod -Uri $api -Headers @{ "User-Agent" = "localvox-light-install/1.0" }

    $patterns = @()
    if ($env:OS -eq "Windows_NT") {
        if ($env:PROCESSOR_ARCHITECTURE -eq "AMD64") {
            $patterns = @("*x86_64-pc-windows-msvc*", "*windows*x86_64*", "*.exe")
        } elseif ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") {
            $patterns = @("*aarch64-pc-windows-msvc*", "*windows*arm64*", "*.exe")
        } else {
            $patterns = @("*win32*", "*.exe")
        }
    } else {
        throw "Use install-release.sh on non-Windows."
    }

    $assets = @($rel.assets)
    $asset = $null
    foreach ($pat in $patterns) {
        $asset = $assets | Where-Object { $_.name -like $pat } | Select-Object -First 1
        if ($asset) { break }
    }
    if (-not $asset) {
        Write-Host "Assets in this release:"
        $assets | ForEach-Object { Write-Host "  -" $_.name }
        throw "No matching Windows asset. Publish a GitHub Release and upload the exe (e.g. name containing x86_64-pc-windows-msvc)."
    }

    $outPath = Join-Path $InstallDir $asset.name
    Write-Host "Downloading" $asset.browser_download_url
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $outPath -UserAgent "localvox-light-install/1.0"

    if ($asset.name -like "*.zip") {
        Expand-Archive -Path $outPath -DestinationPath $InstallDir -Force
        Remove-Item -LiteralPath $outPath -Force
    }

    Write-Host ""
    Write-Host "Done. Run from (add vosk-lib to PATH if exe cannot load libvosk):"
    Write-Host "  cd `"$InstallDir`""
    Get-ChildItem -Path $InstallDir -Filter "localvox-light*.exe" | ForEach-Object { Write-Host "  .\$($_.Name) --tui" }
}
