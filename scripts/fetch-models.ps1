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
    [string]$Root = ""
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$manifestPath = Join-Path $here 'models.json'
if (-not $Root) { $Root = Split-Path -Parent $here }

if (-not (Test-Path -LiteralPath $manifestPath)) { throw "Не найден $manifestPath" }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path -LiteralPath $Root).Path
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json

# Прогресс-бар Invoke-WebRequest режет скорость закачки в разы — на 1.8 ГБ это разница
# между парой минут и получасом.
$ProgressPreference = 'SilentlyContinue'
$missing = $false

function Test-Wanted([string]$id, [bool]$required) {
    if ($Skip -contains $id) { return $false }
    if ($Only.Count -gt 0) { return ($Only -contains $id) }
    if ($RequiredOnly -and -not $required) { return $false }
    return $true
}

# Качаем в «.part» и переименовываем только после проверки. Оборванная закачка на 1.8 ГБ
# иначе осталась бы лежать под правильным именем и при следующем запуске сошла бы за
# готовый файл — а это молча испорченная модель.
function Get-Verified([string]$url, [string]$target, $wantSha, $note) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
    $part = "$target.part"
    Write-Host "  ⇣ $(Split-Path -Leaf $target)"
    Invoke-WebRequest -Uri $url -OutFile $part -UserAgent 'localvox-fetch-models/1.0'

    if ($wantSha) {
        $got = (Get-FileHash -LiteralPath $part -Algorithm SHA256).Hash.ToLower()
        if ($got -ne $wantSha) {
            Remove-Item -LiteralPath $part -Force
            throw "sha256 НЕ СОВПАЛ у $(Split-Path -Leaf $target): ждали $wantSha, получили $got. Файл удалён."
        }
        Write-Host "    sha256 совпал"
    } elseif ($note) {
        Write-Host "    sha256 вверху не публикуют — $note"
    }
    Move-Item -LiteralPath $part -Destination $target -Force
}

foreach ($comp in $manifest.components) {
    if (-not (Test-Wanted $comp.id ([bool]$comp.required))) {
        Write-Host "— $($comp.title): пропущено"
        continue
    }
    Write-Host "$($comp.title) (~$($comp.approx))"

    foreach ($item in $comp.items) {
        switch ($item.kind) {
            'file' {
                $dest = Join-Path $Root $item.dest
                if (Test-Path -LiteralPath $dest) { Write-Host "  уже на месте: $(Split-Path -Leaf $dest)"; break }
                if ($Check) { Write-Host "  НЕТ: $($item.dest)"; $script:missing = $true; break }
                Get-Verified $item.url $dest $item.sha256 $item.sha256_note
            }
            'text' {
                if ($Check) { break }
                $dest = Join-Path $Root $item.dest
                New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dest) | Out-Null
                # Без BOM: файл читают и не-Windows инструменты.
                [System.IO.File]::WriteAllText($dest, ($item.content -join "`r`n") + "`r`n",
                    [System.Text.UTF8Encoding]::new($false))
            }
            'zip' {
                $unpackTo = Join-Path $Root $item.unpack_to
                if (Test-Path -LiteralPath (Join-Path $unpackTo $item.marker)) {
                    Write-Host "  уже на месте: $($item.unpack_to)"; break
                }
                if ($Check) { Write-Host "  НЕТ: $($item.unpack_to)"; $script:missing = $true; break }

                $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("lv-" + [Guid]::NewGuid().ToString())
                New-Item -ItemType Directory -Force -Path $tmp | Out-Null
                try {
                    $zip = Join-Path $tmp 'a.zip'
                    Get-Verified $item.url $zip $item.sha256 $item.sha256_note
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
                    if (Test-Path -LiteralPath $unpackTo) { Remove-Item -LiteralPath $unpackTo -Recurse -Force }
                    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $unpackTo) | Out-Null
                    Move-Item -LiteralPath $inner -Destination $unpackTo -Force
                    if (-not (Test-Path -LiteralPath (Join-Path $unpackTo $item.marker))) {
                        throw "После распаковки нет $($item.marker) в $unpackTo"
                    }
                } finally {
                    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
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
    Write-Host 'готово. Что лежит:'
    Get-ChildItem -LiteralPath (Join-Path $Root 'models') -Directory -ErrorAction SilentlyContinue | ForEach-Object {
        $mb = [math]::Round(((Get-ChildItem -LiteralPath $_.FullName -Recurse -File |
            Measure-Object -Property Length -Sum).Sum / 1MB), 0)
        Write-Host ("  {0,-28} {1} МБ" -f $_.Name, $mb)
    }
}
