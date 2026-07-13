# Модели диаризации («кто говорит») → models/diarize
#
# Две модели, и обе обязательны:
#
#   segmentation.onnx — pyannote segmentation-3.0: размечает по кадрам, кто говорит
#                       внутри 10-секундного окна, включая ПЕРЕКРЫТИЕ речи.
#   embedding.onnx    — ERes2Net (3D-Speaker): вектор ГОЛОСА, по которому голоса
#                       сливаются в людей и узнаются в следующих сессиях.
#
# ЛИЦЕНЗИИ (проверены по первоисточникам, а не по README):
#   * segmentation-3.0 — MIT, Copyright (c) CNRS. Оригинальный репозиторий на HF
#     закрыт формой (gated: анонимное скачивание отдаёт 401), НО это сбор контактов,
#     а не лицензионное ограничение: в самой форме написано «Though this model uses
#     MIT license and will always remain open-source». MIT разрешает распространение,
#     поэтому берём ungated-реэкспорт onnx-community. Текст MIT кладём в поставку
#     сами — в зеркале файла LICENSE нет.
#   * ERes2Net — Apache-2.0 (3D-Speaker / ModelScope). Скачивается анонимно с
#     релизов k2-fsa.
#
# ЧЕГО НЕ БРАТЬ (измерено, а не предположено):
#   * int8 сегментации: расходится с fp32 на 7 % кадров, размазывает границу реплики
#     с 3.5 с до 6.6 с и ВЫДУМЫВАЕТ перекрытие речи, которого нет. Экономия — 4 МБ.
#   * fp16 сегментации: падает с segfault на CPU.
#   * int8 эмбеддера: кластеризация работает УГЛАМИ между векторами, а угол ломается
#     квантизацией первым. На CAM++ косинус fp32↔int8 падал до 0.391 — геометрия
#     разъезжается, и голоса начинают слипаться.
#   * wespeaker CAM++ (его берёт pyannote-rs): на русской речи EER 18–39 % — то есть
#     он почти не отличает людей. Не брать, несмотря на популярность.
#
#   .\scripts\setup-diarize.ps1

param(
    [string]$Dir = 'models/diarize'
)

$ErrorActionPreference = 'Stop'

# Файл, ожидаемый sha256, откуда качать. Хеш проверяется ВСЕГДА: молча получить не
# ту модель — это не «другая версия», это тихо испорченная разметка.
$models = @(
    @{
        Name = 'segmentation.onnx'
        Url  = 'https://huggingface.co/onnx-community/pyannote-segmentation-3.0/resolve/main/onnx/model.onnx'
        Size = '6 МБ'
        What = 'сегментация: кто говорит в каждом кадре (pyannote segmentation-3.0, MIT/CNRS)'
    },
    @{
        Name   = 'embedding.onnx'
        Url    = 'https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_200k_sv_zh-cn_16k-common.onnx'
        Sha256 = 'e2d2048292e055f7b61cdec3db010503f35369b245bf0b3bbad021c9a91e4053'
        Size   = '40 МБ'
        What   = 'голос: вектор тембра (ERes2Net base 200k, Apache-2.0)'
    }
)

New-Item -ItemType Directory -Force -Path $Dir | Out-Null

foreach ($m in $models) {
    $target = Join-Path $Dir $m.Name
    if (Test-Path $target) {
        Write-Host "уже на месте: $target"
        continue
    }
    Write-Host "⇣ $($m.What)"
    Write-Host "  $($m.Size) → $target"
    Invoke-WebRequest -Uri $m.Url -OutFile $target

    if ($m.Sha256) {
        $got = (Get-FileHash -Path $target -Algorithm SHA256).Hash.ToLower()
        if ($got -ne $m.Sha256) {
            Remove-Item $target -Force
            throw "sha256 не совпал у $($m.Name): ждали $($m.Sha256), получили $got. Файл удалён"
        }
        Write-Host "  sha256 совпал"
    }
}

# Лицензии кладём РЯДОМ С ВЕСАМИ. Веса распространяются вместе с продуктом, а MIT и
# Apache-2.0 требуют сохранять текст лицензии и копирайт. В зеркале onnx-community
# файла LICENSE нет — значит это наша обязанность, а не чья-то ещё.
$notice = @'
Модели в этом каталоге — сторонние. Они распространяются на своих условиях.

segmentation.onnx
  pyannote/segmentation-3.0 (реэкспорт onnx-community/pyannote-segmentation-3.0)
  MIT License, Copyright (c) CNRS
  https://huggingface.co/pyannote/segmentation-3.0

  Permission is hereby granted, free of charge, to any person obtaining a copy
  of this software and associated documentation files (the "Software"), to deal
  in the Software without restriction, including without limitation the rights
  to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
  copies of the Software, and to permit persons to whom the Software is
  furnished to do so, subject to the following conditions:

  The above copyright notice and this permission notice shall be included in all
  copies or substantial portions of the Software.

  THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
  IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
  FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
  AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
  LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
  OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
  SOFTWARE.

embedding.onnx
  3D-Speaker ERes2Net base 200k (iic/speech_eres2net_base_200k_sv_zh-cn_16k-common)
  Apache License 2.0
  https://github.com/modelscope/3D-Speaker
  Полный текст: https://www.apache.org/licenses/LICENSE-2.0
'@
Set-Content -Path (Join-Path $Dir 'LICENSES.txt') -Value $notice -Encoding UTF8

Write-Host ''
Write-Host "готово: $Dir"
Write-Host 'Диаризация включится сама при следующей варке: она входит в рецепт,'
Write-Host 'и архив доразметится тем же механизмом, что и при смене языка.'
Write-Host 'Выключить, не удаляя модели: LOCALVOX_DIARIZE=off'
