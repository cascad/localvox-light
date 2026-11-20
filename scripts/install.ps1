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
# PS7 may inherit forced ANSI styling from a host/CI. Keep redirected failures
# plain as well as the meter; the host formats uncaught errors after this script.
if ([Console]::IsErrorRedirected -and (Get-Variable PSStyle -ErrorAction SilentlyContinue)) {
    $PSStyle.OutputRendering = 'PlainText'
}
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
    $latestRequested = $Tag -eq 'latest'
    if ($Tag -eq 'latest') {
        Write-Host 'Checking the latest LocalVox release...'
        $response = Invoke-WebRequest -Uri ($ReleaseBase + '/releases/latest') -Method Head -MaximumRedirection 8 -UseBasicParsing -TimeoutSec 60
        $base = $response.BaseResponse
        $resolved = if ($base.PSObject.Properties['ResponseUri']) { $base.ResponseUri.AbsoluteUri } else { $base.RequestMessage.RequestUri.AbsoluteUri }
        $prefix = $ReleaseBase + '/releases/tag/'
        if (-not $resolved.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Could not resolve the latest release tag' }
        $Tag = [Uri]::UnescapeDataString($resolved.Substring($prefix.Length))
    }
    if ($Tag -notmatch '^v[0-9][A-Za-z0-9._-]*$') { throw 'Invalid release tag' }
    if (-not $Update -and (Test-Path -LiteralPath $installed -PathType Leaf)) {
        $state = 2
        try { & $installed --bundle-check --release-tag $Tag --install-dir $InstallDir 2>$null; $state = $LASTEXITCODE } catch { $state = 2 }
        if ($state -eq 0 -or ($state -eq 3 -and $latestRequested)) {
            if ($state -eq 3) { Write-Host "Installed LocalVox is newer than $Tag; download skipped." }
            else { Write-Host "LocalVox $Tag is verified and current; download skipped." }
            & $installed @options
            if ($LASTEXITCODE -ne 0) { throw "LocalVox setup failed ($LASTEXITCODE)" }
            return
        }
    }
    $name = 'localvox-x86_64-pc-windows-msvc.zip'
    $url = $ReleaseBase + '/releases/download/' + [Uri]::EscapeDataString($Tag) + '/' + $name
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\','/')
    $scratch = Join-Path $tempRoot ('localvox-bootstrap-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $scratch | Out-Null
    $checksum = Join-Path $scratch ($name + '.sha256')
    $archive = Join-Path $scratch $name
    Write-Host "LocalVox $Tag"
    Invoke-WebRequest -Uri ($url + '.sha256') -OutFile $checksum -UseBasicParsing
    $line = [IO.File]::ReadAllText($checksum).Trim()
    if ($line -notmatch ('^([a-fA-F0-9]{64})[ \t]+\*?' + [regex]::Escape($name) + '$')) { throw 'Invalid release checksum file' }
    $expected = $Matches[1].ToLowerInvariant()
    # Keep this self-contained: no helper/native binary is trusted before SHA-256.
    & {
        param($Url, $Target)
        $previousEncoding = [Console]::OutputEncoding
        [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
        $interactive = -not [Console]::IsErrorRedirected -and $env:TERM -ne 'dumb'
        $color = $interactive -and $null -eq [Environment]::GetEnvironmentVariable('NO_COLOR')
        $accent = if ($color) { [string][char]27 + '[36m' } else { '' }
        $reset = if ($color) { [string][char]27 + '[0m' } else { '' }
        $branch = [string][char]0x2502
        $dot = [string][char]0x00b7
        $watch = [Diagnostics.Stopwatch]::StartNew()
        $display = @{ last = -10.0; width = 0; open = $false }
        $received = 0L; $total = -1L
        $request = [Net.HttpWebRequest]::Create($Url)
        $request.UserAgent = 'localvox-install'
        $response = $null; $inputStream = $null; $file = $null
        function Show-DownloadProgress([switch]$Done) {
            $interval = if ($interactive) { 0.15 } else { 5 }
            if (-not $Done -and $watch.Elapsed.TotalSeconds - $display.last -lt $interval) { return }
            $display.last = $watch.Elapsed.TotalSeconds
            $size = '{0:0.0}' -f ($received / 1MB)
            $seconds = [int][Math]::Floor($watch.Elapsed.TotalSeconds)
            $elapsed = '{0:00}:{1:00}' -f [int][Math]::Floor($seconds / 60), ($seconds % 60)
            if ($total -gt 0) {
                $percent = if ($Done) { 100 } else { [Math]::Min(99, [int][Math]::Floor(100.0 * $received / $total)) }
                $filled = [int][Math]::Floor(16 * $percent / 100)
                $bar = ([string][char]0x2501) * $filled + ([string][char]0x2500) * (16 - $filled)
                $line = "  $branch $accent[$bar]$reset $percent% $dot $size/$('{0:0.0}' -f ($total / 1MB)) MiB $dot $elapsed"
            } else {
                $frame = if ($Done) { 'Downloaded' } elseif ($interactive) { @('|','/','-','\')[[int]($watch.Elapsed.TotalSeconds * 8) % 4] + ' Download' } else { 'Download' }
                $line = "  $branch $accent$frame$reset $dot $size MiB $dot $elapsed"
            }
            if ($interactive) {
                [Console]::Error.Write("`r" + $line.PadRight($display.width))
                $display.width = $line.Length
                $display.open = $true
            } else { [Console]::Error.WriteLine($line) }
        }
        function Wait-DownloadTask($Task) {
            $stall = [Diagnostics.Stopwatch]::StartNew()
            while (-not $Task.IsCompleted) {
                Show-DownloadProgress
                if ($stall.Elapsed.TotalSeconds -gt 60) { throw 'Release download stalled: no response for 60 seconds' }
                Start-Sleep -Milliseconds 100
            }
            $Task.GetAwaiter().GetResult()
        }
        try {
            Show-DownloadProgress
            $response = Wait-DownloadTask ($request.GetResponseAsync())
            $total = $response.ContentLength
            $inputStream = $response.GetResponseStream()
            $file = [IO.File]::Create($Target)
            $buffer = [byte[]]::new(65536)
            while ($true) {
                $count = Wait-DownloadTask ($inputStream.ReadAsync($buffer, 0, $buffer.Length))
                if ($count -eq 0) { break }
                $file.Write($buffer, 0, $count)
                $received += $count
                Show-DownloadProgress
            }
            if ($total -ge 0 -and $received -ne $total) { throw "Incomplete release download: $received of $total bytes" }
            Show-DownloadProgress -Done
        } finally {
            $request.Abort()
            if ($file) { $file.Dispose() }
            if ($inputStream) { $inputStream.Dispose() }
            if ($response) { $response.Dispose() }
            if ($display.open) { [Console]::Error.WriteLine() }
            [Console]::OutputEncoding = $previousEncoding
        }
    } $url ($archive + '.part')
    Write-Host '  | Verifying SHA-256...'
    $sha = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($archive + '.part')
    try { $actual = [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','').ToLowerInvariant() }
    finally { $stream.Dispose(); $sha.Dispose() }
    if ($actual -ne $expected) { throw 'Release SHA-256 mismatch; no installation was performed' }
    Move-Item -LiteralPath ($archive + '.part') -Destination $archive
    Write-Host '  | Unpacking verified bundle...'
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
