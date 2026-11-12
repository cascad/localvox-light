# Only called after the terminal wizard has shown and confirmed its plan.
param([Parameter(Mandatory=$true)][string]$CacheDir)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$existing = Get-Command ollama.exe -ErrorAction SilentlyContinue
if ($existing -or (Test-Path -LiteralPath "$env:LOCALAPPDATA\Programs\Ollama\ollama.exe")) {
    Write-Host 'Ollama is already installed; open its application, then return to the wizard.'
    exit 0
}
New-Item -ItemType Directory -Path $CacheDir -Force | Out-Null
$installer = Join-Path $CacheDir 'OllamaSetup.exe'
function ValidInstaller([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $false }
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    return ($signature.Status -eq 'Valid' -and $signature.SignerCertificate.Subject -match 'Ollama')
}
if (-not (ValidInstaller $installer)) {
    $part = Join-Path $CacheDir 'OllamaSetup.part.exe'
    Invoke-WebRequest -Uri 'https://ollama.com/download/OllamaSetup.exe' -OutFile $part
    if (-not (ValidInstaller $part)) {
        throw 'The Ollama installer signature could not be verified. Download it from https://ollama.com/download/windows and retry setup.'
    }
    Move-Item -LiteralPath $part -Destination $installer -Force
}
# The official installer window is intentionally visible for install location/OS prompts.
$process = Start-Process -FilePath $installer -PassThru -Wait
if ($process.ExitCode -ne 0) { throw "Ollama installer failed ($($process.ExitCode))" }
