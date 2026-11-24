# Локальная проверка T-one и SbertPuncCase

Обычный `scripts/test-behavior.py` не скачивает и не запускает модели. Для отдельной
проверки нужны выбранные в `vox setup` каталоги T-one/SbertPuncCase и ffmpeg.
Рабочий архив и микрофон не используются; тест создаёт собственную сессию.

```powershell
$env:LOCALVOX_TONE_MODEL_DIR = 'F:/project_models/t-one'
$env:LOCALVOX_SBERT_MODEL_DIR = 'F:/project_models/sbert-punc-case-ru'
$env:LOCALVOX_TEST_ASR_AUDIO = 'F:/fixtures/short-russian-speech.wav'
cargo test --locked --jobs 1 -p localvox-light-asr --test optional_text_processor -- --ignored --test-threads=1
```

На Mac/Linux вместо `$env:KEY = 'value'` использовать `export KEY='value'`.
Это проверяет настоящий worker: импорт → T-one → опциональная пунктуация,
отдельный оригинал и точные времена. Качество текста оценивается отдельно.

Для сравнения адаптеров с upstream есть `scripts/make-asr-reference.py`. В отдельном
Python-окружении нужны numpy, onnxruntime, CPU torch и transformers; установщик
LocalVox их не требует. Передать уже скачанные веса и WAV mono PCM16 **8 kHz**:

```sh
python scripts/make-asr-reference.py --tone-dir /models/t-one --sbert-dir /models/sbert-punc-case-ru --audio /fixtures/short-8k.wav --audio /fixtures/long-8k.wav --output target/asr-reference
```

Для ignored тестов ядра задать `LOCALVOX_TONE_REFERENCE=target/asr-reference/tone-reference.json`
и `LOCALVOX_SBERT_REFERENCE=target/asr-reference/sbert-reference.json` (абсолютные пути удобнее).
Запускать последовательно:

```sh
cargo test --locked --jobs 1 -p localvox-light-core --features onnx --lib asr::tone::tests::real_model_matches_upstream_and_each_call_resets_state -- --ignored --exact --test-threads=1
cargo test --locked --jobs 1 -p localvox-light-core --features onnx --lib text_processing::sbert::tests::real_model_matches_transformers_logits_labels_and_long_word_boundaries -- --ignored --exact --test-threads=1
```

GigaAM остаётся основным по умолчанию. T-one использует greedy CTC; 5-ГБ KenLM
не скачивается и качество beam decoder этим тестом не обещается. SbertPuncCase —
отдельный CPU-процессор после любого ASR; по умолчанию выключен.
