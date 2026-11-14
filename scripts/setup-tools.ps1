# Invoked only for components in a confirmed installation plan.
[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$Only, [Parameter(Mandatory=$true)][string]$InstallDir)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$PSNativeCommandUseErrorActionPreference = $false
$tools = @($Only.Split(','))
foreach ($tool in $tools) { if ($tool -notin @('ffmpeg','yt-dlp','mpv')) { throw "Unsupported Windows tool: $tool" } }
$cache = Join-Path $InstallDir 'cache/installers'
New-Item -ItemType Directory -Path $cache -Force | Out-Null

if (@($tools | Where-Object { $_ -ne 'mpv' }).Count -gt 0) {
    if (-not (Get-Command winget.exe -ErrorAction SilentlyContinue)) {
        Install-PackageProvider -Name NuGet -Scope CurrentUser -Force -Confirm:$false | Out-Null
        Install-Module -Name Microsoft.WinGet.Client -Scope CurrentUser -Force -Repository PSGallery -Confirm:$false
        Repair-WinGetPackageManager -Force -Latest
    }
    foreach ($tool in $tools) {
        if ($tool -eq 'mpv') { continue }
        $id = if ($tool -eq 'ffmpeg') { 'Gyan.FFmpeg' } else { 'yt-dlp.yt-dlp' }
        & winget.exe install --exact --id $id --source winget --scope user --silent --accept-source-agreements --accept-package-agreements --disable-interactivity
        if ($LASTEXITCODE -ne 0) { throw "Could not install $id ($LASTEXITCODE)" }
    }
}
if ($tools -contains 'mpv') {
    $headers = @{'User-Agent'='localvox-install'}
    if ($env:GH_TOKEN) { $headers.Authorization = 'Bearer ' + $env:GH_TOKEN }
    $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/mpv-player/mpv/releases/tags/git-release' -Headers $headers
    $assets = @($release.assets | Where-Object { $_.name -match '^mpv-.*-x86_64-w64-mingw32-full\.zip$' })
    if ($assets.Count -ne 1 -or $assets[0].digest -notmatch '^sha256:([a-f0-9]{64})$') { throw 'Official mpv archive has no unique verifiable SHA-256 digest' }
    $expected = $Matches[1]
    if ($assets[0].size -gt 150MB) { throw 'Unexpected mpv archive size' }
    $archive = Join-Path $cache 'mpv.zip'
    if (-not (Test-Path -LiteralPath $archive) -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) {
        $part = $archive + '.part'
        Invoke-WebRequest -Uri $assets[0].browser_download_url -OutFile $part -UseBasicParsing
        if ((Get-Item -LiteralPath $part).Length -gt 150MB -or (Get-FileHash -LiteralPath $part -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) { Remove-Item -LiteralPath $part; throw 'mpv archive SHA-256 mismatch' }
        Move-Item -LiteralPath $part -Destination $archive -Force
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $reader = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        foreach ($entry in $reader.Entries) {
            if ([IO.Path]::IsPathRooted($entry.FullName) -or $entry.FullName -match '(^|[/\\])\.\.([/\\]|$)|:' -or ($entry.ExternalAttributes -shr 16 -band 0xF000) -eq 0xA000) { throw 'Unsafe mpv archive member' }
        }
    } finally { $reader.Dispose() }
    $folder = Join-Path $InstallDir 'tools/mpv'
    Expand-Archive -LiteralPath $archive -DestinationPath $folder -Force
    if (-not (Test-Path -LiteralPath (Join-Path $folder 'mpv.exe') -PathType Leaf)) { throw 'mpv.exe missing in official archive' }
    $receipt = @{source=$assets[0].browser_download_url;sha256=$expected;executable_sha256=(Get-FileHash -LiteralPath (Join-Path $folder 'mpv.exe')).Hash.ToLowerInvariant();installed_at=[DateTime]::UtcNow.ToString('o')}
    [IO.File]::WriteAllText((Join-Path $folder 'installed.json'),($receipt | ConvertTo-Json),[Text.UTF8Encoding]::new($false))
}
