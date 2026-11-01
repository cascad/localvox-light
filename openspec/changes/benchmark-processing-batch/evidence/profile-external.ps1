#requires -Version 7.4
# Measurement recipe, Windows/local Ollama: run from the repository root.
# Only the benchmark process is owned; the existing Ollama service is never stopped.
param([Parameter(Mandatory)][string]$Manifest,[Parameter(Mandatory)][string]$Output,[int]$Repeats=2,[int]$TimeoutSec=900,[switch]$TraceOllama)
$ErrorActionPreference='Stop'
if (-not $IsWindows) { throw 'This resource recipe requires Windows' }
$repoRoot=(Resolve-Path -LiteralPath .).Path
if (-not (Test-Path -LiteralPath (Join-Path $repoRoot 'Cargo.toml'))) { throw 'Run from the repository root' }
$env:TEMP=Join-Path $repoRoot 'target/test-temp'
$env:TMP=$env:TEMP
$reportPath="$Output.external.json"
foreach ($path in @($Output,$reportPath,"$Output.stdout.log","$Output.stderr.log")) {
    if (Test-Path -LiteralPath $path) { throw 'Output or companion report already exists' }
}
$started=[DateTime]::UtcNow
$rows=[Collections.Generic.List[object]]::new()
$counters=@{}
$modelsBefore=Invoke-RestMethod http://127.0.0.1:11434/api/ps -TimeoutSec 5
$cpu=Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors
$memory=(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
$listen=Get-NetTCPConnection -LocalPort 11434 -State Listen | Select-Object -ExpandProperty OwningProcess -Unique
$version=Invoke-RestMethod http://127.0.0.1:11434/api/version -TimeoutSec 5
function Sample {
    $processRows=@()
    $snapshot=Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId
    $liveSet=[Collections.Generic.HashSet[int]]::new()
    foreach ($rootId in $listen) { [void]$liveSet.Add([int]$rootId) }
    do {
        $count=$liveSet.Count
        foreach ($entry in $snapshot) {
            if ($liveSet.Contains([int]$entry.ParentProcessId)) { [void]$liveSet.Add([int]$entry.ProcessId) }
        }
    } while ($count -ne $liveSet.Count)
    foreach ($observedId in $liveSet) {
        $item=$null
        try {
            $item=[Diagnostics.Process]::GetProcessById($observedId)
            $key=[string]$item.Id
            $seconds=$item.TotalProcessorTime.TotalSeconds
            if (-not $counters.ContainsKey($key)) {
                $baseline=if ($item.StartTime.ToUniversalTime() -ge $started) { 0.0 } else { $seconds }
                $counters[$key]=[ordered]@{pid=$item.Id;process_name=$item.ProcessName;baseline_cpu_sec=$baseline;last_cpu_sec=$seconds;started_before_monitor=($item.StartTime.ToUniversalTime() -lt $started)}
            }
            $counters[$key].last_cpu_sec=$seconds
            $processRows+=@{pid=$item.Id;cpu_sec=$seconds;working_set_bytes=$item.WorkingSet64;private_bytes=$item.PrivateMemorySize64}
        } catch { }
        if ($null -ne $item) { $item.Dispose() }
    }
    $model=$null
    try { $model=(Invoke-RestMethod http://127.0.0.1:11434/api/ps -TimeoutSec 3).models | Select-Object name,digest,size,size_vram,context_length,expires_at } catch { }
    $gpu=$null
    try {
        $line=& (Join-Path $env:SystemRoot 'System32/nvidia-smi.exe') --query-gpu=memory.used,memory.total,utilization.gpu --format=csv,noheader,nounits 2>$null
        $parts=($line | Select-Object -First 1) -split ','
        if ($parts.Count -eq 3) { $gpu=@{memory_used_mib=[int]$parts[0].Trim();memory_total_mib=[int]$parts[1].Trim();utilization_percent=[int]$parts[2].Trim()} }
    } catch { }
    $rows.Add(@{at=[DateTime]::UtcNow.ToString('o');elapsed_sec=([DateTime]::UtcNow-$started).TotalSeconds;processes=$processRows;models=@($model);gpu_global=$gpu})
}
Sample
$info=[Diagnostics.ProcessStartInfo]::new((Join-Path $repoRoot 'target/release/localvox-profile.exe'))
$info.UseShellExecute=$false
$info.CreateNoWindow=$true
$info.RedirectStandardOutput=$true
$info.RedirectStandardError=$true
$info.WorkingDirectory=$repoRoot
$arguments=@('--manifest',$Manifest,'--output',$Output,'--repeats',[string]$Repeats,'--timeout-sec',[string]$TimeoutSec,'--worker',(Join-Path $repoRoot 'target/release/localvox-process.exe'),'--model-dir',(Join-Path $repoRoot 'models/gigaam-v3-e2e-ctc'),'--queue-runner',(Join-Path $repoRoot 'target/release/localvox-queue-profile.exe'))
if ($TraceOllama) { $arguments+='--trace-ollama' }
foreach ($argument in $arguments) { $info.ArgumentList.Add($argument) }
$process=[Diagnostics.Process]::Start($info)
$stdout=$process.StandardOutput.ReadToEndAsync()
$stderr=$process.StandardError.ReadToEndAsync()
try {
    while (-not $process.HasExited) {
        if (([DateTime]::UtcNow-$started).TotalSeconds -gt ($TimeoutSec+45)) { throw 'Owned benchmark exceeded outer deadline' }
        Start-Sleep -Milliseconds 1800
        Sample
    }
} catch {
    if (-not $process.HasExited) { $process.Kill($true) }
    throw
}
$process.WaitForExit()
Sample
$outText=$stdout.GetAwaiter().GetResult()
$errText=$stderr.GetAwaiter().GetResult()
[IO.File]::WriteAllText("$Output.stdout.log",$outText)
[IO.File]::WriteAllText("$Output.stderr.log",$errText)
$counterRows=@($counters.Values | ForEach-Object { $_.observed_cpu_delta_sec=$_.last_cpu_sec-$_.baseline_cpu_sec; $_ })
$result=[ordered]@{version=2;scope='local_Ollama_listener_and_observed_descendants_and_global_GPU';server_listener_pid=@($listen);ollama_version=$version.version;cpu=$cpu;physical_ram_bytes=$memory;models_before=$modelsBefore.models;monitor_started_at=$started.ToString('o');elapsed_sec=([DateTime]::UtcNow-$started).TotalSeconds;profile_exit_code=$process.ExitCode;arguments=$arguments;cpu_counters=$counterRows;samples=@($rows);limits='Shared external service; no forced unload or cancellation. Process CPU deltas are observed counters; short-lived descendants may be missed. Global GPU includes desktop and other applications; model size_vram is allocation, not utilization. Sample interval includes CIM/query overhead, approximately 2 s.'}
[IO.File]::WriteAllText($reportPath,($result | ConvertTo-Json -Depth 12))
Write-Output "exit=$($process.ExitCode) samples=$($rows.Count) external=$reportPath"
$exitCode=$process.ExitCode
$process.Dispose()
exit $exitCode
