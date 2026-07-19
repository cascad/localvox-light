#!/usr/bin/env bash
# Все модели, кроме Vosk → models/
#
# Vosk ставится отдельно (setup-vosk-mac.sh / setup-vosk-linux-*.sh): у него своя
# нативная библиотека под каждую платформу. Здесь — модели, одинаковые везде:
#
#   models/gigaam-v3-e2e-ctc/  — распознавание речи (русский, со знаками препинания)
#   models/diarize/            — «кто говорит»: сегментация + вектор голоса
#   models/ner-gliner/         — проверка имён в ответах LLM
#
# Скачивается ТОЛЬКО недостающее: скрипт можно гонять сколько угодно раз.
# Хеши проверяются везде, где известны: молча получить не ту модель — это не
# «другая версия», это тихо испорченная расшифровка.
#
#   ./scripts/setup-models.sh                 # всё
#   ./scripts/setup-models.sh gigaam          # только распознавание
#   ./scripts/setup-models.sh gigaam diarize  # выборочно
#
# Без diarize записи расшифруются, но без разметки говорящих.
# Без ner-gliner LLM-проверка не сможет судить об именах (числа проверяются всегда).

set -euo pipefail
cd "$(dirname "$0")/.."

want=("$@")
[ ${#want[@]} -eq 0 ] && want=(gigaam diarize ner)

has() {
    for w in "${want[@]}"; do [ "$w" = "$1" ] && return 0; done
    return 1
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1   # macOS
    fi
}

# файл, URL, ожидаемый sha256 («-» если хеша нет)
fetch() {
    local target="$1" url="$2" want_sha="$3"
    if [ -f "$target" ]; then
        echo "  уже на месте: $target"
        return
    fi
    mkdir -p "$(dirname "$target")"
    echo "  ⇣ $(basename "$target")"
    curl -fL --progress-bar "$url" -o "$target"

    if [ "$want_sha" != "-" ]; then
        local got
        got="$(sha256_of "$target")"
        if [ "$got" != "$want_sha" ]; then
            rm -f "$target"
            echo "sha256 не совпал у $(basename "$target"): ждали $want_sha, получили $got. Файл удалён" >&2
            exit 1
        fi
        echo "    sha256 совпал"
    fi
}

if has gigaam; then
    # Главный движок распознавания. Хеши сняты с файлов, на которых продукт работает
    # (14.07.2026): если зеркало перезальёт веса, скрипт упадёт громко — это лучше, чем
    # молча получить другую модель и потом искать, почему расшифровка поехала.
    echo "GigaAM v3 e2e CTC — распознавание речи (русский), ~215 МБ"
    gg='https://huggingface.co/istupakov/gigaam-v3-onnx/resolve/main'
    fetch models/gigaam-v3-e2e-ctc/v3_e2e_ctc.int8.onnx "$gg/v3_e2e_ctc.int8.onnx" \
        2e3fcb7a7b66030336fd10c2fcfb033bd1dc7e1bf238fe5cfd83b1d0cfc9d28e
    fetch models/gigaam-v3-e2e-ctc/v3_e2e_ctc.yaml "$gg/v3_e2e_ctc.yaml" \
        e67eca3a311ad7c8813d36dff6b8eeba7ad3459fd811d6faea2a26535754a358
    fetch models/gigaam-v3-e2e-ctc/v3_e2e_ctc_vocab.txt "$gg/v3_e2e_ctc_vocab.txt" \
        142de7570b3de5b3035ce111a89c228e80e6085273731d944093ddf24fa539cd
fi

if has diarize; then
    # Две модели, и обе обязательны: сегментация говорит, КТО говорит в каждом кадре,
    # эмбеддер даёт вектор голоса, по которому голоса сливаются в людей.
    #
    # Квантованные версии НЕ БРАТЬ (измерено): int8-сегментация выдумывает перекрытие
    # речи, int8-эмбеддер ломает углы между векторами — а кластеризация работает
    # именно углами, и голоса начинают слипаться.
    echo "Диаризация — «кто говорит», ~46 МБ"
    fetch models/diarize/segmentation.onnx \
        'https://huggingface.co/onnx-community/pyannote-segmentation-3.0/resolve/main/onnx/model.onnx' -
    fetch models/diarize/embedding.onnx \
        'https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_200k_sv_zh-cn_16k-common.onnx' \
        e2d2048292e055f7b61cdec3db010503f35369b245bf0b3bbad021c9a91e4053

    # Лицензии кладём РЯДОМ С ВЕСАМИ: MIT и Apache-2.0 требуют сохранять текст, а в
    # зеркале onnx-community файла LICENSE нет — значит это наша обязанность.
    cat > models/diarize/LICENSES.txt <<'EOF'
Модели в этом каталоге — сторонние, распространяются на своих условиях.

segmentation.onnx — pyannote/segmentation-3.0 (реэкспорт onnx-community)
  MIT License, Copyright (c) CNRS
  https://huggingface.co/pyannote/segmentation-3.0

embedding.onnx — 3D-Speaker ERes2Net base 200k
  Apache License 2.0
  https://github.com/modelscope/3D-Speaker
EOF
fi

if has ner; then
    # Готовый ONNX с зеркала onnx-community. НЕ БРАТЬ int8/quantized: у mDeBERTa
    # квантизация разваливает калибровку, и модель молча перестаёт находить имена —
    # что неотличимо от честной пустой записи.
    echo "GLiNER — проверка имён в ответах LLM, ~580 МБ"
    gl='https://huggingface.co/onnx-community/gliner_multi-v2.1/resolve/main'
    fetch models/ner-gliner/model_fp16.onnx "$gl/onnx/model_fp16.onnx" -
    fetch models/ner-gliner/tokenizer.json "$gl/tokenizer.json" -
    fetch models/ner-gliner/gliner_config.json "$gl/gliner_config.json" -
fi

echo
echo "готово. Что лежит:"
du -sh models/* 2>/dev/null || true
