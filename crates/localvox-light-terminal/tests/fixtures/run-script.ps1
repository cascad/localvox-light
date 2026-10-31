param([string]$RunScript, [string]$Directory, [string]$Case)
$ErrorActionPreference = 'Stop'
$RunScript = (Resolve-Path -LiteralPath $RunScript).Path
New-Item -ItemType Directory -Path $Directory | Out-Null
$global:localvoxRunFixture = $Directory
$global:localvoxRunFixtureCase = $Case
$original = (Get-Location).Path
$repo = Split-Path -Parent (Split-Path -Parent $RunScript)
$artifacts = Join-Path $Directory 'custom-target'
New-Item -ItemType Directory -Path $artifacts | Out-Null
foreach ($name in @('localvox-light', 'localvox-process', 'localvox-desktop', 'localvox-tui')) {
    $body = @'
$entry = @{ name = [IO.Path]::GetFileNameWithoutExtension($PSCommandPath); cwd = (Get-Location).Path; arguments = @($args) }
$entry | ConvertTo-Json -Compress | Add-Content -LiteralPath (Join-Path $global:localvoxRunFixture 'executed.jsonl')
$global:LASTEXITCODE = 0
'@
    [IO.File]::WriteAllText((Join-Path $artifacts "$name.ps1"), $body)
}
function global:cargo {
    @{ arguments = @($args); cwd = (Get-Location).Path } | ConvertTo-Json -Depth 5 |
        Set-Content -LiteralPath (Join-Path $global:localvoxRunFixture 'cargo.json')
    $requested = @()
    for ($i = 0; $i -lt $args.Length; $i++) {
        if ($args[$i] -eq '--bin') { $requested += $args[$i + 1] }
    }
    foreach ($name in $requested) {
        if ($global:localvoxRunFixtureCase -eq 'missing-artifact' -and $name -eq 'localvox-tui') { continue }
        $exe = Join-Path (Join-Path $global:localvoxRunFixture 'custom-target') "$name.ps1"
        if ($global:localvoxRunFixtureCase -eq 'different-directories' -and $name -eq 'localvox-tui') {
            $exe = Join-Path $global:localvoxRunFixture 'localvox-tui.ps1'
            Copy-Item -LiteralPath (Join-Path (Join-Path $global:localvoxRunFixture 'custom-target') "$name.ps1") -Destination $exe
        }
        @{ reason = 'compiler-artifact'; target = @{ name = $name }; executable = $exe } | ConvertTo-Json -Compress
    }
    $global:LASTEXITCODE = if ($global:localvoxRunFixtureCase -eq 'build-failure') { 19 } else { 0 }
}
$options = switch ($Case) {
    'run-desktop' { @{ Desktop = $true } }
    'run-daemon' { @{} }
    'conflict' { @{ Desktop = $true; Tui = $true } }
    'build-only-tui' { @{ Tui = $true; BuildOnly = $true; Profile = 'dev' } }
    default { @{ Tui = $true; Profile = 'dev' } }
}
$failed = $null
Push-Location -LiteralPath $Directory
try {
    try { & $RunScript @options } catch { $failed = $_.Exception.Message }
    if ((Get-Location).Path -ne $Directory) { throw 'run.ps1 did not restore cwd' }
} finally { Pop-Location }
if ((Get-Location).Path -ne $original) { throw 'fixture did not restore cwd' }
$expectFailure = $Case -in @('conflict', 'build-failure', 'missing-artifact', 'different-directories')
if ($expectFailure -ne ($null -ne $failed)) { throw "Unexpected result: $Case $failed" }
$execution = Join-Path $Directory 'executed.jsonl'
if ($expectFailure -or $Case -eq 'build-only-tui') {
    if (Test-Path -LiteralPath $execution) { throw 'executed an artifact after failure/BuildOnly' }
} else {
    $entries = @(Get-Content -LiteralPath $execution | ForEach-Object { $_ | ConvertFrom-Json })
    $expected = switch ($Case) { 'run-daemon' { 'localvox-light' }; 'run-desktop' { 'localvox-desktop' }; default { 'localvox-tui' } }
    if ($entries.Length -ne 1 -or $entries[0].name -ne $expected) { throw 'wrong executable' }
    if ($entries[0].cwd -ne $repo) { throw 'runtime cwd must be the repository' }
    if ($Case -eq 'run-daemon' -and $entries[0].arguments[0] -ne '--daemon') { throw 'missing daemon flag' }
}
$cargoLog = Join-Path $Directory 'cargo.json'
if ($Case -eq 'conflict') {
    if (Test-Path -LiteralPath $cargoLog) { throw 'conflicting modes reached Cargo' }
} else {
    $invocation = Get-Content -LiteralPath $cargoLog -Raw | ConvertFrom-Json
    if ($invocation.cwd -ne $repo) { throw 'build cwd must be the repository' }
    foreach ($required in @('localvox-light', 'localvox-process', '--locked', '--message-format=json-render-diagnostics')) {
        if ($required -notin $invocation.arguments) { throw "missing build argument $required" }
    }
    if ($options.Tui -and 'localvox-tui' -notin $invocation.arguments) { throw 'Tui missing from build' }
    $profileIndex = [array]::IndexOf($invocation.arguments, '--profile')
    $expectedProfile = if ($options.Profile) { $options.Profile } else { 'release' }
    if ($invocation.arguments[$profileIndex + 1] -ne $expectedProfile) { throw 'wrong profile' }
}
Write-Output "PASS $Case"
