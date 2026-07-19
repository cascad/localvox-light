# Основной движок распознавания (русский) → models/gigaam-v3-e2e-ctc
#
# GigaAM v3 e2e CTC в ONNX. Это то, чем расшифровываются записи: он ставит знаки
# препинания и нормализует числа, чего Vosk не умеет.
#
# ТРИ ФАЙЛА, И ВСЕ ТРИ ОБЯЗАТЕЛЬНЫ:
#   v3_e2e_ctc.int8.onnx   — сами веса (215 МБ)
#   v3_e2e_ctc.yaml        — конфиг мел-фильтров и модели
#   v3_e2e_ctc_vocab.txt   — словарь CTC; без него выход декодера не во что превратить
#
# ХЕШИ ПРИБИТЫ. Они сняты с файлов, на которых продукт работает (14.07.2026). Если
# зеркало перезальёт веса, скрипт упадёт громко — это лучше, чем молча получить
# другую модель и потом искать, почему расшифровка поехала.
#
# ЗАЧЕМ ЕЩЁ НУЖЕН VOSK (setup-vosk.ps1), если расшифровывает этот: `libvosk.dll`
# линкуется в бинарь, и загрузчик Windows требует её ДО main(). Без неё не
# стартует ничего — даже если Vosk-моделью вы не пользуетесь ни разу.
#
#   .\scripts\setup-gigaam.ps1

param(
    [string]$Dir = 'models/gigaam-v3-e2e-ctc'
)

$ErrorActionPreference = 'Stop'

$repo = 'https://huggingface.co/istupakov/gigaam-v3-onnx/resolve/main'
$files = @(
    @{ Name = 'v3_e2e_ctc.int8.onnx'; Size = '215 МБ'; Sha256 = '2e3fcb7a7b66030336fd10c2fcfb033bd1dc7e1bf238fe5cfd83b1d0cfc9d28e' },
    @{ Name = 'v3_e2e_ctc.yaml'; Size = '4 КБ'; Sha256 = 'e67eca3a311ad7c8813d36dff6b8eeba7ad3459fd811d6faea2a26535754a358' },
    @{ Name = 'v3_e2e_ctc_vocab.txt'; Size = '4 КБ'; Sha256 = '142de7570b3de5b3035ce111a89c228e80e6085273731d944093ddf24fa539cd' }
)

New-Item -ItemType Directory -Force -Path $Dir | Out-Null

foreach ($f in $files) {
    $target = Join-Path $Dir $f.Name
    if (Test-Path $target) {
        Write-Host "уже на месте: $target"
        continue
    }
    Write-Host "⇣ $($f.Name) ($($f.Size))"
    Invoke-WebRequest -Uri "$repo/$($f.Name)" -OutFile $target

    $got = (Get-FileHash -Path $target -Algorithm SHA256).Hash.ToLower()
    if ($got -ne $f.Sha256) {
        Remove-Item $target -Force
        throw "sha256 не совпал у $($f.Name): ждали $($f.Sha256), получили $got. Файл удалён"
    }
    Write-Host "  sha256 совпал"
}

Write-Host ''
Write-Host "готово: $Dir"
Write-Host 'Источник: https://huggingface.co/istupakov/gigaam-v3-onnx (ONNX-экспорт GigaAM от Сбера).'
Write-Host 'Лицензию смотрите в карточке модели — она принадлежит авторам, а не нам.'
