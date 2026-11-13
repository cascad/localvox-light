# Install a complete Windows release, or an archive downloaded from Actions.
# .\install-release.ps1 -InstallDir D:\LocalVox [-Archive path.zip] [-RequiredOnly]
[CmdletBinding()]
param(
    [string]$InstallDir = '',
    [string]$Repo = 'cascad/localvox-light',
    [string]$Tag = 'latest',
    [string]$Branch = 'main', # Compatibility: helper scripts now come from the archive.
    [string]$Archive = '',
    [switch]$RequiredOnly,
    [switch]$SkipModels,
    [switch]$SkipBinary,
    [switch]$Check,
    [switch]$Wizard,
    [switch]$Reconcile,
    [switch]$NoPathUpdate
)
$ErrorActionPreference = 'Stop'
# Doctor/check use 0/1/2 as data; every native call below checks its exit code.
# Keep the same behavior when invoked by a fail-fast PowerShell 7 CI parent.
$PSNativeCommandUseErrorActionPreference = $false
$ProgressPreference = 'SilentlyContinue'
if ($Archive -and $SkipBinary) { throw '-Archive and -SkipBinary cannot be combined' }
if (-not $InstallDir) { $InstallDir = Join-Path (Get-Location).Path 'localvox-light' }
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
$architecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if ($architecture -ne 'AMD64') { throw "Windows package requires x64 PowerShell; detected $architecture" }
$target = 'x86_64-pc-windows-msvc'
$required = @('localvox-light.exe','localvox-process.exe','localvox-tui.exe','vox.exe','libvosk.dll',
    'libgcc_s_seh-1.dll','libstdc++-6.dll','libwinpthread-1.dll','.env.defaults',
    'scripts/models.json','scripts/fetch-models.ps1','scripts/model-paths.ps1','scripts/setup-ollama.ps1','licenses/rs-morphy4/NOTICE')
function CheckedPath([string]$Root, [string]$Name) {
    if ([IO.Path]::IsPathRooted($Name) -or $Name -match '(^|[/\\])\.\.([/\\]|$)|:') { throw "Unsafe archive path: $Name" }
    $resolved = [IO.Path]::GetFullPath((Join-Path $Root $Name))
    if (-not $resolved.StartsWith($Root.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Path outside archive: $Name" }
    return $resolved
}

function TestInstalledBundle {
    try {
        $path = Join-Path $InstallDir 'bundle.json'
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return $false }
        $manifest = Get-Content -LiteralPath $path -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($manifest.schema -ne 1 -or $manifest.target -cne $target) { return $false }
        $names = @($manifest.files | ForEach-Object { $_.name })
        foreach ($name in $required) { if ($names -cnotcontains $name) { return $false } }
        foreach ($file in $manifest.files) {
            $path = CheckedPath $InstallDir $file.name
            if ($file.name -eq '.env' -or -not (Test-Path -LiteralPath $path -PathType Leaf)) { return $false }
            if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $file.sha256) { return $false }
        }
        return $true
    } catch { return $false }
}
if ($Check) {
    if ($Wizard -or $Reconcile) { throw '-Check cannot be combined with -Wizard/-Reconcile' }
    if (-not (TestInstalledBundle)) { Write-Host "Missing or invalid complete installation: $InstallDir"; exit 2 }
    & (Join-Path $InstallDir 'localvox-light.exe') --cwd $InstallDir --setup-check
    $ready = $LASTEXITCODE
    & (Join-Path $InstallDir 'vox.exe') --check-command
    exit ([Math]::Max($ready, $LASTEXITCODE))
}
if ($Wizard -and $SkipModels) { throw '-Wizard and -SkipModels cannot be combined' }
if ($Reconcile -and (TestInstalledBundle)) { $SkipBinary = $true; Write-Host 'Installed bundle is intact; download and unpack skipped.' }
if ($Reconcile -and -not $SkipModels) { $Wizard = $true }
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$scratch = Join-Path $tempRoot ('localvox-install-' + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $scratch | Out-Null


try {
    if (-not $SkipBinary) {
        if ($Archive) {
            $zip = (Resolve-Path -LiteralPath $Archive).Path
        } else {
            $api = if ($Tag -eq 'latest') { "https://api.github.com/repos/$Repo/releases/latest" } else { "https://api.github.com/repos/$Repo/releases/tags/$Tag" }
            $release = Invoke-RestMethod -Uri $api -Headers @{'User-Agent'='localvox-install/2'}
            $name = "localvox-$target.zip"
            $assets = @($release.assets | Where-Object { $_.name -ceq $name })
            if ($assets.Count -ne 1) { throw "Release $($release.tag_name) has no complete $name package. Download a new bundle from Actions and use -Archive, or select a newer -Tag." }
            $zip = Join-Path $scratch $name
            Write-Host "Downloading $($release.tag_name) / $name"
            Invoke-WebRequest -Uri $assets[0].browser_download_url -OutFile $zip -UserAgent 'localvox-install/2'
            if ($assets[0].digest -match '^sha256:([0-9a-f]{64})$') {
                if ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Matches[1]) { throw 'Archive SHA-256 mismatch' }
            }
        }
        $staging = Join-Path $scratch 'bundle'
        New-Item -ItemType Directory -Path $staging | Out-Null
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $archiveReader = [IO.Compression.ZipFile]::OpenRead($zip)
        try {
            foreach ($entry in $archiveReader.Entries) {
                if ($entry.FullName.TrimEnd('/','\')) { $null = CheckedPath $staging $entry.FullName }
                if (($entry.ExternalAttributes -shr 16 -band 0xF000) -eq 0xA000) { throw 'Archive symlinks are not supported' }
            }
        } finally { $archiveReader.Dispose() }
        Expand-Archive -LiteralPath $zip -DestinationPath $staging
        $manifestPath = Join-Path $staging 'bundle.json'
        if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw 'Incomplete package: bundle.json missing' }
        $manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($manifest.schema -ne 1 -or $manifest.target -cne $target) { throw 'Package platform/schema mismatch' }
        $names = @($manifest.files | ForEach-Object { $_.name })
        foreach ($name in $required) { if ($names -cnotcontains $name) { throw "Incomplete package: $name" } }
        foreach ($file in $manifest.files) {
            $source = CheckedPath $staging $file.name
            if ($file.name -eq '.env' -or -not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Invalid package member: $($file.name)" }
            if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant() -cne $file.sha256) { throw "Package SHA-256 mismatch: $($file.name)" }
        }
        # Validate the complete staging tree before changing the installation.
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        foreach ($name in @($names) + @('bundle.json')) {
            $destination = CheckedPath $InstallDir $name
            New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
            Copy-Item -LiteralPath (CheckedPath $staging $name) -Destination $destination -Force
        }
    }
    foreach ($name in $required) { if (-not (Test-Path -LiteralPath (Join-Path $InstallDir $name) -PathType Leaf)) { throw "Installation missing: $name" } }
    foreach ($probe in @(@('localvox-process.exe','--worker-protocol'),@('localvox-tui.exe','--version'),@('vox.exe','--version'),@('localvox-light.exe','--help'))) {
        & (Join-Path $InstallDir $probe[0]) $probe[1] | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Packaged startup failed: $($probe[0]) ($LASTEXITCODE)" }
    }
    if (-not $NoPathUpdate) {
        & (Join-Path $InstallDir 'vox.exe') --install-command
        if ($LASTEXITCODE -gt 1 -or $LASTEXITCODE -lt 0) { throw "vox PATH registration failed ($LASTEXITCODE); use -Reconcile to retry or -NoPathUpdate for portable mode" }
    }
    if ($Wizard) {
        & (Join-Path $InstallDir 'localvox-light.exe') --cwd $InstallDir --setup
        if ($LASTEXITCODE -ne 0) { throw "Setup failed ($LASTEXITCODE); rerun with -Reconcile to keep completed work" }
        exit 0
    }
    if (-not $SkipModels) {
        $engine = Join-Path $PSHOME $(if ($PSVersionTable.PSEdition -eq 'Core') { 'pwsh.exe' } else { 'powershell.exe' })
        $fetchArgs = @('-NoProfile','-ExecutionPolicy','Bypass','-File',(Join-Path $InstallDir 'scripts/fetch-models.ps1'))
        if ($RequiredOnly) { $fetchArgs += '-RequiredOnly' }
        & $engine @fetchArgs
        if ($LASTEXITCODE -ne 0) { throw "Model installation failed ($LASTEXITCODE)" }
    }
    $envPath = Join-Path $InstallDir '.env'
    if (-not (Test-Path -LiteralPath $envPath)) {
        $directory = ($InstallDir -replace '\\','/') -replace '"','\"'
        $content = @(
            '# Created by install-release.ps1; presets are in .env.defaults.',
            "LOCALVOX_LIGHT_AUDIO_DIR=`"$directory/archive`"",''
        ) -join "`n"
        [IO.File]::WriteAllText($envPath, $content, [Text.UTF8Encoding]::new($false))
    } else { Write-Host 'Existing .env preserved.' }
    Push-Location $InstallDir
    try { & (Join-Path $InstallDir 'localvox-light.exe') --doctor; $doctor = $LASTEXITCODE } finally { Pop-Location }
    if ($doctor -gt 2 -or $doctor -lt 0 -or ($doctor -eq 2 -and -not $SkipModels)) { throw "Doctor failed ($doctor)" }
    Write-Host "Installed: $InstallDir"
    if ($SkipModels) { Write-Host 'Models skipped. Install them before recording/transcription.' }
    Write-Host "Run: cd `"$InstallDir`"; .\localvox-light.exe --daemon"
    Write-Host 'UI: http://127.0.0.1:3017/   TUI: .\localvox-tui.exe'
    # Translate the accepted doctor status into the installer CLI's success code.
    exit 0
} finally {
    $resolved = [IO.Path]::GetFullPath($scratch)
    if ($resolved.StartsWith($tempRoot.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolved).StartsWith('localvox-install-')) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
