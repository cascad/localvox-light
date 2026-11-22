# Модели localvox → <корень>\models\ (Windows).
#
# Список качаемого лежит в scripts\models.json — это ДАННЫЕ. Здесь только механика:
# докачать недостающее, сверить sha256, распаковать. Повторный запуск ничего не
# переделывает: скрипт можно гонять сколько угодно раз, в том числе после обрыва связи.
#
#   .\scripts\fetch-models.ps1                       # всё
#   .\scripts\fetch-models.ps1 -Only gigaam          # только распознавание архива
#   .\scripts\fetch-models.ps1 -Skip ner             # без проверки имён (экономит 580 МБ)
#   .\scripts\fetch-models.ps1 -RequiredOnly         # минимум, на котором продукт работает
#   .\scripts\fetch-models.ps1 -Root D:\localvox     # не в репозиторий, а в установку
#   .\scripts\fetch-models.ps1 -Check                # ничего не качать, только сказать чего нет
#
# Нативная библиотека Vosk ставится отдельно (setup-vosk.ps1): она своя под каждую ОС.

param(
    [string[]]$Only = @(),
    [string[]]$Skip = @(),
    [switch]$RequiredOnly,
    [switch]$Check,
    [switch]$RepairZip,
    [string]$Root = "",
    [string]$ComponentDir = ""
)

$ErrorActionPreference = 'Stop'
# The native installer reads redirected output as UTF-8. Windows PowerShell 5
# otherwise writes the console's OEM code page (e.g. 866), corrupting Cyrillic.
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$manifestPath = Join-Path $here 'models.json'

if (-not (Test-Path -LiteralPath $manifestPath)) { throw "Не найден $manifestPath" }
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
. (Join-Path $here 'model-paths.ps1')
if ($ComponentDir) {
    if ($Only.Count -ne 1 -or $manifest.components.id -notcontains $Only[0]) { throw '-ComponentDir requires exactly one known -Only component' }
    $Root = [IO.Path]::GetFullPath($ComponentDir)
}
$placement = Get-ModelLocations $manifest (Split-Path -Parent $here) $Root
if ($ComponentDir) { $placement.components[$Only[0]].path = [IO.Path]::GetFullPath($ComponentDir); $placement.components[$Only[0]].source = 'root' }

# Прогресс-бар Invoke-WebRequest режет скорость закачки в разы — на 1.8 ГБ это разница
# между парой минут и получасом.
$ProgressPreference = 'SilentlyContinue'
$missing = $false
$nativeDownloader = $env:LOCALVOX_MODEL_DOWNLOADER
if (-not $nativeDownloader) {
    $candidate = Join-Path (Split-Path -Parent $here) 'localvox-install.exe'
    if (Test-Path -LiteralPath $candidate -PathType Leaf) {
        try {
            $protocol = & $candidate --download-protocol 2>$null
            if ($LASTEXITCODE -eq 0 -and "$protocol".Trim() -eq 'localvox-download-v1') { $nativeDownloader = $candidate }
        } catch { $nativeDownloader = $null }
    }
}

function Test-Wanted([string]$id, [bool]$required) {
    if ($Skip -contains $id) { return $false }
    if ($Only.Count -gt 0) { return ($Only -contains $id) }
    if ($RequiredOnly -and -not $required) { return $false }
    return $true
}

# Качаем в «.part» и переименовываем только после проверки. Оборванная закачка на 1.8 ГБ
# иначе осталась бы лежать под правильным именем и при следующем запуске сошла бы за
# готовый файл — а это молча испорченная модель.
function Get-Verified([string]$url, [string]$target, $wantSha, $note, $mirrors = @(), $size = 0) {
    New-ModelParent $target $placement.user_root
    $part = "$target.part"
    Write-Host "  ⇣ $(Split-Path -Leaf $target)"
    try {
        if ($nativeDownloader) {
            $arguments = @('--download-file', $url, '--download-output', $part)
            if ($wantSha) { $arguments += @('--download-sha256', [string]$wantSha) }
            if ($size -gt 0) { $arguments += @('--download-size', [string]$size) }
            foreach ($mirror in $mirrors) { if ($mirror) { $arguments += @('--download-fallback', [string]$mirror) } }
            & $nativeDownloader @arguments
            if ($LASTEXITCODE -ne 0) { throw 'Native model download failed' }
        } else {
            $sources = @(@($url) + @($mirrors) | Where-Object { $_ })
            for ($attempt = 0; $attempt -lt $sources.Count; $attempt++) {
                try {
                    Invoke-WebRequest -Uri $sources[$attempt] -OutFile $part -UserAgent 'localvox-fetch-models/1.0' -UseBasicParsing
                    break
                } catch { if ($attempt -eq $sources.Count - 1) { throw } }
            }
        }
    } catch {
        if (Test-Path -LiteralPath $part) { Remove-Item -LiteralPath $part -Force }
        $response = $_.Exception.Response
        $reason = if ($response) { "HTTP $([int]$response.StatusCode)" } else { $_.Exception.Message }
        throw "Не удалось скачать модель ($reason): $url. Готовые модели сохранены."
    }

    if ($size -gt 0 -and (Get-Item -LiteralPath $part).Length -ne $size) {
        Remove-Item -LiteralPath $part -Force
        throw "Размер скачанного файла не совпал с ожидаемым: $size байт"
    }
    if ($wantSha -and -not $nativeDownloader) {
        $got = (Get-FileHash -LiteralPath $part -Algorithm SHA256).Hash.ToLower()
        if ($got -ne $wantSha) {
            Remove-Item -LiteralPath $part -Force
            throw "sha256 НЕ СОВПАЛ у $(Split-Path -Leaf $target): ждали $wantSha, получили $got. Файл удалён."
        }
        Write-Host "    sha256 совпал"
    } elseif ($wantSha) {
        Write-Host '    sha256 совпал'
    } elseif ($note) {
        Write-Host "    sha256 вверху не публикуют — $note"
    }
    Move-Item -LiteralPath $part -Destination $target -Force
}

foreach ($comp in $manifest.components) {
    if (-not (Test-Wanted $comp.id ([bool]$comp.required))) {
        if ($Only.Count -eq 0) { Write-Host "— $($comp.title): Не выбрано" }
        continue
    }
    Write-Host "$($comp.title) (~$($comp.approx))"
    $location = $placement.components[$comp.id]
    if (-not $location.path) { throw "No model directory for $($comp.id). Set $($comp.env) or use -Root." }
    $componentPath = [IO.Path]::GetFullPath($location.path)
    Write-Host "  $componentPath [$($location.source)]"

    foreach ($item in $comp.items) {
        switch ($item.kind) {
            'file' {
                $dest = Join-ModelPayload $componentPath $comp.directory $item.dest
                if (Test-Path -LiteralPath $dest -PathType Leaf) { Write-Host "  уже на месте: $(Split-Path -Leaf $dest)"; break }
                if ($Check) { Write-Host "  НЕТ: $($item.dest)"; $script:missing = $true; break }
                if ($location.source -eq 'explicit') { throw "Configured model is incomplete: $componentPath. Choose download/repair in --setup or use an explicit -Root." }
                Get-Verified $item.url $dest $item.sha256 $item.sha256_note $item.mirrors $item.size
            }
            'text' {
                if ($Check -or $location.source -eq 'explicit') { break }
                $dest = Join-ModelPayload $componentPath $comp.directory $item.dest
                if (Test-Path -LiteralPath $dest -PathType Leaf) { break }
                New-ModelParent $dest $placement.user_root
                # Без BOM: файл читают и не-Windows инструменты.
                [System.IO.File]::WriteAllText($dest, ($item.content -join "`r`n") + "`r`n",
                    [System.Text.UTF8Encoding]::new($false))
            }
            'zip' {
                $unpackTo = Join-ModelPayload $componentPath $comp.directory $item.unpack_to
                if (-not $RepairZip -and (Test-Path -LiteralPath (Join-Path $unpackTo $item.marker) -PathType Leaf)) {
                    Write-Host "  уже на месте: $($item.unpack_to)"; break
                }
                if ($Check) { Write-Host "  НЕТ: $($item.unpack_to)"; $script:missing = $true; break }
                if ($location.source -eq 'explicit') { throw "Configured model needs repair: $componentPath. Choose download/repair in --setup or use an explicit -Root." }
                Assert-ModelReplacement $unpackTo $componentPath

                New-ModelParent $componentPath $placement.user_root
                $tempParent = [IO.Path]::GetFullPath((Split-Path -Parent $componentPath))
                $tmp = Join-Path $tempParent ('.localvox-download-' + [Guid]::NewGuid().ToString())
                New-Item -ItemType Directory -Force -Path $tmp | Out-Null
                try {
                    $zip = Join-Path $tmp 'a.zip'
                    Get-Verified $item.url $zip $item.sha256 $item.sha256_note $item.mirrors $item.size
                    if ($env:LOCALVOX_DOWNLOAD_PROTOCOL -eq '1') {
                        Write-Host ('@localvox-download-v1 ' + (@{name=$comp.title;phase='unpacking';bytes=0;total=$null;elapsed_ms=0} | ConvertTo-Json -Compress))
                    }
                    Write-Host "  распаковка…"
                    Expand-Archive -LiteralPath $zip -DestinationPath (Join-Path $tmp 'x') -Force
                    # Архив несёт свой корневой каталог (vosk-model-ru-0.42\…). Кладём его
                    # СОДЕРЖИМОЕ туда, куда просит манифест, а не полагаемся на совпадение имён:
                    # перезальют архив с другим корнем — и без этого мы молча получим models\models\…
                    if ($item.strip_root) {
                        $inner = Get-ChildItem -LiteralPath (Join-Path $tmp 'x') -Directory | Select-Object -First 1
                        if (-not $inner) { throw "В архиве нет корневого каталога" }
                        $inner = $inner.FullName
                    } else {
                        $inner = Join-Path $tmp 'x'
                    }
                    if (-not (Test-Path -LiteralPath (Join-Path $inner $item.marker) -PathType Leaf)) { throw "Downloaded archive has no $($item.marker)" }
                    Assert-ModelReplacement $unpackTo $componentPath
                    if (Test-Path -LiteralPath $unpackTo) { Remove-Item -LiteralPath $unpackTo -Recurse -Force }
                    New-ModelParent $unpackTo $placement.user_root
                    Move-Item -LiteralPath $inner -Destination $unpackTo -Force
                    if (-not (Test-Path -LiteralPath (Join-Path $unpackTo $item.marker))) {
                        throw "После распаковки нет $($item.marker) в $unpackTo"
                    }
                } finally {
                    $safeParent = $tempParent.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar
                    if ([IO.Path]::GetFullPath($tmp).StartsWith($safeParent, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $tmp).StartsWith('.localvox-download-')) {
                        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
                    }
                }
            }
            default { throw "Неизвестный kind: $($item.kind)" }
        }
    }
}

Write-Host ''
if ($Check) {
    if ($missing) { Write-Host 'чего-то не хватает — запусти без -Check'; exit 1 }
    Write-Host 'всё на месте'
} else {
    Write-Host 'готово'
}
