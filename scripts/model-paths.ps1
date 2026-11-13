# Shared placement for fetch-models. Installed bundles delegate .env reading to Rust.
function Get-ModelLocations($Manifest, [string]$Install, [string]$ExplicitRoot) {
    foreach ($component in $Manifest.components) {
        if ($component.directory -notmatch '^models/[^/\\:]+$' -or (Split-Path -Leaf $component.directory) -in @('.','..')) {
            throw "Unsafe component directory: $($component.directory)"
        }
    }
    $profilePath = [Environment]::GetEnvironmentVariable('USERPROFILE')
    if (-not $profilePath) { $profilePath = [Environment]::GetFolderPath('UserProfile') }
    $userRoot = $null
    if ($profilePath -and [IO.Path]::IsPathRooted($profilePath)) {
        $userRoot = Join-Path $profilePath $Manifest.storage.user_directory
    }
    $locations = @{}
    $exe = Join-Path $Install 'localvox-light.exe'
    if (-not $ExplicitRoot -and (Test-Path -LiteralPath $exe -PathType Leaf)) {
        $start = [Diagnostics.ProcessStartInfo]::new()
        $start.FileName = $exe
        $start.Arguments = '--model-paths-json'
        $start.WorkingDirectory = $Install
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.StandardOutputEncoding = [Text.Encoding]::UTF8
        $start.StandardErrorEncoding = [Text.Encoding]::UTF8
        $process = [Diagnostics.Process]::Start($start)
        try {
            $stdout = $process.StandardOutput.ReadToEndAsync()
            $stderr = $process.StandardError.ReadToEndAsync()
            if (-not $process.WaitForExit(30000)) { $process.Kill(); throw 'Model path resolver timed out' }
            $output = $stdout.GetAwaiter().GetResult()
            $errorText = $stderr.GetAwaiter().GetResult()
            if ($process.ExitCode -ne 0) { throw "Model path resolver failed: $errorText" }
            $report = $output | ConvertFrom-Json
            if ($report.schema -ne 1) { throw 'Update the bundle: unsupported model path resolver schema' }
            foreach ($location in $report.components) { $locations[$location.id] = $location }
            $userRoot = $report.user_root
        } finally { $process.Dispose() }
    } else {
        if (-not $ExplicitRoot -and ((Test-Path -LiteralPath (Join-Path $Install '.env')) -or (Test-Path -LiteralPath '.env'))) {
            throw 'Reading .env requires the installed localvox-light resolver. Use the bundle or an explicit -Root.'
        }
        foreach ($component in $Manifest.components) {
            $path = $null; $source = 'unavailable'
            $configured = [Environment]::GetEnvironmentVariable($component.env)
            if ($ExplicitRoot) {
                $path = Join-Path ([IO.Path]::GetFullPath($ExplicitRoot)) $component.directory; $source = 'root'
            } elseif ($null -ne $configured) {
                $path = $configured.Trim(); $source = 'explicit'
                if ($path.Length -ge 2 -and (($path.StartsWith('"') -and $path.EndsWith('"')) -or ($path.StartsWith("'") -and $path.EndsWith("'")))) {
                    $path = $path.Substring(1, $path.Length - 2).Trim()
                }
                if ($path) { $path = [IO.Path]::GetFullPath($path) }
            } else {
                if ($component.sibling_of -and $locations.ContainsKey($component.sibling_of)) {
                    $parent = $locations[$component.sibling_of].path
                    if ($parent) {
                        $sibling = Join-Path (Split-Path -Parent $parent) (Split-Path -Leaf $component.directory)
                        if (Test-Path -LiteralPath $sibling -PathType Container) { $path = $sibling; $source = 'asr' }
                    }
                }
                if (-not $path) {
                    foreach ($base in @($Install, (Get-Location).Path)) {
                        $candidate = Join-Path $base $component.directory
                        if (Test-Path -LiteralPath $candidate -PathType Container) {
                            $path = $candidate
                            $source = if ($base -eq $Install) { 'portable' } else { 'cwd' }
                            break
                        }
                    }
                }
                if (-not $path -and $userRoot) { $path = Join-Path $userRoot $component.directory; $source = 'user' }
            }
            $locations[$component.id] = [pscustomobject]@{path=$path; source=$source}
        }
    }
    [pscustomobject]@{components=$locations; user_root=$userRoot}
}

function Join-ModelPayload([string]$Directory, [string]$ComponentDirectory, [string]$Payload) {
    $prefix = $ComponentDirectory.TrimEnd('/') + '/'
    if ($Payload -eq $ComponentDirectory) { return [IO.Path]::GetFullPath($Directory) }
    if (-not $Payload.StartsWith($prefix, [StringComparison]::Ordinal)) { throw "Payload outside component: $Payload" }
    $relative = $Payload.Substring($prefix.Length)
    if ([IO.Path]::IsPathRooted($relative) -or $relative -match '(^|[/\\])\.\.([/\\]|$)|:') { throw "Unsafe payload: $Payload" }
    [IO.Path]::GetFullPath((Join-Path $Directory $relative))
}

function New-ModelParent([string]$Target, [string]$UserRoot) {
    $Target = [IO.Path]::GetFullPath($Target)
    if ($UserRoot) { $UserRoot = [IO.Path]::GetFullPath($UserRoot) }
    $parent = Split-Path -Parent $Target
    # Mark only a newly created standard user root; custom/existing directories retain attributes.
    if ($UserRoot -and -not (Test-Path -LiteralPath $UserRoot) -and
        $Target.StartsWith($UserRoot.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        $created = New-Item -ItemType Directory -Path $UserRoot -Force
        if ($env:OS -eq 'Windows_NT') { $created.Attributes = $created.Attributes -bor [IO.FileAttributes]::Hidden }
    }
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
}

function Assert-ModelReplacement([string]$Target, [string]$Expected) {
    $resolved = [IO.Path]::GetFullPath($Target)
    if ($resolved -ne [IO.Path]::GetFullPath($Expected) -or (Split-Path -Leaf $resolved) -eq '' -or
        -not (Split-Path -Parent $resolved)) { throw "Unsafe model replacement: $Target" }
    $ancestor = $resolved
    while ($ancestor) {
        if ((Test-Path -LiteralPath $ancestor) -and
            ((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Refusing model replacement through a reparse point: $ancestor"
        }
        $ancestor = Split-Path -Parent $ancestor
    }
}
