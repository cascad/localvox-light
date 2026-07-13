# Модель проверки имён (NER) → models/ner-gliner
#
# ЗАЧЕМ ДВА ПУТИ. По умолчанию скачиваем ГОТОВЫЙ ONNX с зеркала onnx-community —
# это быстро и не требует Python. Но зеркало — ЧУЖАЯ конвертация, и полагаться на
# неё в поставке нельзя. Поэтому есть `-Export`: он делает ONNX САМ, из исходных
# весов (urchade/gliner_multi-v2.1, Apache-2.0), и тогда мы ни от кого не зависим.
#
# Веса, кстати, не менялись с апреля 2024 — модель это законченный артефакт, а не
# заброшенный проект: коммит от 2025-12-08 лишь добавил тот же вес в формате
# safetensors. Так что «зеркало старое» — не про качество, а про владение цепочкой.
#
# ЧТО НЕ БРАТЬ: model_int8 / model_quantized / model_uint8. У mDeBERTa int8
# разваливает калибровку — ИЗМЕРЕНО: «Роман закроет задачу» даёт 0.22 вместо 0.89,
# «Marcus will ship it» — 0.17 вместо 0.97. При любом разумном пороге модель молча
# не находит НИЧЕГО, и это неотличимо от честной пустой записи.
#
#   .\scripts\setup-ner.ps1              # готовый ONNX с зеркала (fp16, 580 МБ)
#   .\scripts\setup-ner.ps1 -Quant q4f16 # компактнее (472 МБ) и вдвое быстрее
#   .\scripts\setup-ner.ps1 -Export      # СВОЙ экспорт из исходных весов (нужен Python)

param(
    [ValidateSet('fp16', 'q4f16', 'fp32')]
    [string]$Quant = 'fp16',
    [switch]$Export,
    [string]$Dir = 'models/ner-gliner'
)

$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Dir | Out-Null

if ($Export) {
    Write-Host "Свой экспорт ONNX из исходных весов (urchade/gliner_multi-v2.1)." -ForegroundColor Cyan
    Write-Host "Нужен Python с пакетом gliner. Ставится один раз, в рантайме его нет.`n"

    python -c "import gliner" 2>$null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Ставлю gliner…" -ForegroundColor Yellow
        python -m pip install --quiet gliner onnxruntime
    }

    $py = @'
from gliner import GLiNER
import os, sys
out = sys.argv[1]
m = GLiNER.from_pretrained("urchade/gliner_multi-v2.1")
m.save_pretrained(out)                     # веса + gliner_config.json + tokenizer.json
m.onnx_export(os.path.join(out, "model.onnx"))   # fp32; fp16 — квантизацией onnxruntime
print("экспортировано в", out)
'@
    $py | python - $Dir
    Write-Host "`nГотово. Проверьте: cargo run -p localvox-light-bench --bin probe_ner" -ForegroundColor Green
    exit 0
}

$repo = 'https://huggingface.co/onnx-community/gliner_multi-v2.1/resolve/main'
$model = switch ($Quant) {
    'fp16' { 'model_fp16.onnx' }
    'q4f16' { 'model_q4f16.onnx' }
    'fp32' { 'model.onnx' }
}

Write-Host "Качаю $model + токенизатор → $Dir" -ForegroundColor Cyan
foreach ($f in @("onnx/$model", 'tokenizer.json', 'gliner_config.json')) {
    $name = Split-Path $f -Leaf
    $dest = Join-Path $Dir $name
    if (Test-Path $dest) { Write-Host "  есть: $name"; continue }
    Write-Host "  качаю: $name"
    Invoke-WebRequest -Uri "$repo/$f" -OutFile $dest
}

Write-Host "`nГотово: $Dir" -ForegroundColor Green
Write-Host "Проверить: cargo run -p localvox-light-bench --bin probe_ner"
Write-Host "Нет модели — имена не проверяются, и варка скажет об этом вслух (числа работают всегда)."
