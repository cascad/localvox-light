# One-command bootstrap. The native wizard owns the reviewed installation plan.
[CmdletBinding()]
param(
    [string]$InstallDir = '',
    [string]$Tag = 'latest',
    [string]$ReleaseBase = 'https://github.com/cascad/localvox-light',
    [switch]$Update,
    [switch]$Check,
    [switch]$NoPathUpdate
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$PSNativeCommandUseErrorActionPreference = $false
$invokedAsFile = $MyInvocation.MyCommand -is [System.Management.Automation.ExternalScriptInfo]
if (-not $InstallDir) {
    $location = Join-Path $env:USERPROFILE '.localvox/install-location'
    if (Test-Path -LiteralPath $location -PathType Leaf) {
        $saved = [IO.File]::ReadAllText($location).TrimEnd("`r","`n")
        if ([IO.Path]::IsPathRooted($saved) -and $saved -notmatch '[\x00-\x1f\x7f]') { $InstallDir=$saved }
    }
    if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA 'Programs/LocalVox' }
}
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
$ReleaseBase = $ReleaseBase.TrimEnd('/')
$uri = [Uri]$ReleaseBase
if (-not $uri.IsAbsoluteUri -or ($uri.Scheme -ne 'https' -and -not ($uri.Scheme -eq 'http' -and $uri.IsLoopback))) { throw 'ReleaseBase must be HTTPS (HTTP is allowed only for a local test server)' }
$architecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if ($architecture -ne 'AMD64') { throw "Unsupported Windows architecture: $architecture. An x64 installation is required." }
$options = @('--install-dir', $InstallDir)
if ($NoPathUpdate) { $options += '--no-path-update' }
$installed = Join-Path $InstallDir 'localvox-install.exe'

# Scope native error dialogs to this bootstrap and its children, never a system setting.
if (-not ('LocalVoxBootstrap.NativeErrors' -as [type])) {
    Add-Type -Namespace LocalVoxBootstrap -Name NativeErrors -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern uint SetErrorMode(uint mode);'
}
$previousErrorMode = [LocalVoxBootstrap.NativeErrors]::SetErrorMode(3)
$scratch = $null
try {
    if ($Check) {
        if (-not (Test-Path -LiteralPath $installed -PathType Leaf)) { Write-Host 'LocalVox is not installed.'; if ($invokedAsFile) { exit 2 }; $global:LASTEXITCODE=2; return }
        & $installed @options --check
        if ($LASTEXITCODE -notin @(0,1,2)) { throw "LocalVox doctor could not start ($LASTEXITCODE); run setup again to repair the bundle" }
        if ($invokedAsFile) { exit $LASTEXITCODE }
        return
    }
    if (-not $Update -and $Tag -eq 'latest' -and (Test-Path -LiteralPath $installed -PathType Leaf)) {
        $intact = $false
        try { & $installed --bundle-check --install-dir $InstallDir 2>$null; $intact = $LASTEXITCODE -eq 0 } catch { $intact = $false }
        if ($intact) {
            Write-Host 'LocalVox files are intact; download skipped.'
            & $installed @options
            if ($LASTEXITCODE -ne 0) { throw "LocalVox setup failed ($LASTEXITCODE)" }
            return
        }
    }
    if ($Tag -eq 'latest') {
        $response = Invoke-WebRequest -Uri ($ReleaseBase + '/releases/latest') -Method Head -MaximumRedirection 8 -UseBasicParsing
        $base = $response.BaseResponse
        $resolved = if ($base.PSObject.Properties['ResponseUri']) { $base.ResponseUri.AbsoluteUri } else { $base.RequestMessage.RequestUri.AbsoluteUri }
        $prefix = $ReleaseBase + '/releases/tag/'
        if (-not $resolved.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Could not resolve the latest release tag' }
        $Tag = [Uri]::UnescapeDataString($resolved.Substring($prefix.Length))
    }
    if ($Tag -notmatch '^v[0-9][A-Za-z0-9._-]*$') { throw 'Invalid release tag' }
    $name = 'localvox-x86_64-pc-windows-msvc.zip'
    $url = $ReleaseBase + '/releases/download/' + [Uri]::EscapeDataString($Tag) + '/' + $name
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\','/')
    $scratch = Join-Path $tempRoot ('localvox-bootstrap-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $scratch | Out-Null
    $checksum = Join-Path $scratch ($name + '.sha256')
    $archive = Join-Path $scratch $name
    Write-Host "Downloading LocalVox $Tag..."
    Invoke-WebRequest -Uri ($url + '.sha256') -OutFile $checksum -UseBasicParsing
    $line = [IO.File]::ReadAllText($checksum).Trim()
    if ($line -notmatch ('^([a-fA-F0-9]{64})[ \t]+\*?' + [regex]::Escape($name) + '$')) { throw 'Invalid release checksum file' }
    $expected = $Matches[1].ToLowerInvariant()
    Invoke-WebRequest -Uri $url -OutFile ($archive + '.part') -UseBasicParsing
    $sha = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($archive + '.part')
    try { $actual = [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','').ToLowerInvariant() }
    finally { $stream.Dispose(); $sha.Dispose() }
    if ($actual -ne $expected) { throw 'Release SHA-256 mismatch; no installation was performed' }
    Move-Item -LiteralPath ($archive + '.part') -Destination $archive
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $reader = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        foreach ($entry in $reader.Entries) {
            if ([IO.Path]::IsPathRooted($entry.FullName) -or $entry.FullName -match '(^|[/\\])\.\.([/\\]|$)|:' -or ($entry.ExternalAttributes -shr 16 -band 0xF000) -eq 0xA000) { throw 'Unsafe release archive member' }
        }
    } finally { $reader.Dispose() }
    $bundle = Join-Path $scratch 'bundle'
    [IO.Compression.ZipFile]::ExtractToDirectory($archive, $bundle)
    foreach ($file in @('localvox-install.exe','bundle.json','libvosk.dll','libgcc_s_seh-1.dll','libstdc++-6.dll','libwinpthread-1.dll')) {
        if (-not (Test-Path -LiteralPath (Join-Path $bundle $file) -PathType Leaf)) { throw "Incomplete release: $file. Select a complete newer release." }
    }
    & (Join-Path $bundle 'localvox-install.exe') @options --staging-root $scratch
    if ($LASTEXITCODE -ne 0) { throw "LocalVox setup failed ($LASTEXITCODE). Completed models are preserved; run this command again." }
} finally {
    [void][LocalVoxBootstrap.NativeErrors]::SetErrorMode($previousErrorMode)
    if ($scratch -and (Test-Path -LiteralPath $scratch)) {
        $resolvedScratch = [IO.Path]::GetFullPath($scratch)
        if (-not $resolvedScratch.StartsWith($tempRoot + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($resolvedScratch) -notmatch '^localvox-bootstrap-[a-f0-9]{32}$') { throw 'Refusing cleanup outside the private bootstrap directory' }
        Remove-Item -LiteralPath $resolvedScratch -Recurse -Force
    }
}
