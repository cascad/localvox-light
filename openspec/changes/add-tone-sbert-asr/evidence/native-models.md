# Native model adapter checks · 2026-11-24

Это проверка адаптеров, не утверждение о готовности установщика/настроек/пайплайна.
Остальные работы отражены отдельно в tasks.md.

## Источники

- T-one revision `106f3b0b32a9e107eb613312e4ebc61ff3d53926`, официальный
  `model.onnx` SHA-256 `707e4a282d5036304a0b8603ad83945840c3075f53c49911ef88c19bcc5a7d52`.
- SbertPuncCase revision `f778dc6c63bb0ec235a220488862509810e54583`, официальный
  safetensors SHA-256 `06173fe7aed01a3a58385f6e724502f634909bee0818f6a8514b9eb6eb869be8`.
- Веса/config/tokenizer проверены в `F:/project_models/localvox-dev`;
  Python reference venv/cache и результаты — в repository target на D.
  В installed runtime Python не используется.
- Аудио: публичные `audio_short.flac` и `audio_long.flac` из
  [официальных примеров](https://github.com/voicekit-team/T-one/tree/main/tone/demo/audio_examples).
  ffmpeg преобразовал их в PCM16 mono 8 kHz. Пользовательский архив/микрофон не использовались.

## Фактические результаты Windows

- `asr::tone::tests`: 3 passed — CTC collapse через frames/blank, odd/short lengths,
  NaN refusal, anti-alias 16→8k, точное число выходных сэмплов.
- `asr::tone::tests::real_model_matches_upstream_and_each_call_resets_state`:
  **passed**, 4.26 s на две реальные записи, тишину и повтор после сброса state.
  Текст совпал с independent Python ONNX greedy reference.
- `text_processing::sbert::tests::labels_only_add_casing_and_punctuation_and_keep_digits_and_existing_marks`:
  **passed**; неизвестные labels — ошибка, цифры/внутренняя точка/готовая пунктуация сохраняются.
- `real_model_matches_transformers_logits_labels_and_long_word_boundaries`:
  **passed**, 10.29 s; 5 случаев, включая 720 слов, кириллицу/ё/имена/латиницу/числа.
  Совпали token IDs, word IDs, итоговый текст; max absolute logits delta <0.005.
  Проверено только CPU, один model-test за раз, pool до 2 threads.

## Исправления в ходе проверки

Первый настоящий T-one test отказал до публикации: type hint upstream указывал
float16 output, реальные logits оказались float32 (state — float16). Адаптер
исправлен по реальным tensor types; ошибка не обходится неявным преобразованием.
Лог: `target/review/tone/tone-native-parity.log`, успешный — `tone-native-parity-2.log`.

Первый Sbert check в unoptimized dev был остановлен после нескольких минут,
без утверждения о результате. Для native tensor libraries включена оптимизация
и в dev/test, без изменения пользовательского алгоритма/допусков/эталонов.
Успешный лог: `target/review/tone/sbert-native-parity-optimized-2.log`.
Reference: `scripts/make-asr-reference.py`, Python 3.12, numpy 2.2.6,
onnxruntime 1.23.2, Transformers 4.57.1, CPU PyTorch 2.9.0.

## Границы

Ни WER против GigaAM на общем датасете, ни длинный production benchmark здесь
не измерены. 10.29 s — длительность набора проверок, не обещание скорости двухчасовой
записи. Greedy не приравнивается к beam/KenLM. Mac/Linux native и аппаратная запись
здесь not-run. GigaAM остаётся default; Sbert — отдельная выключенная опция для любого ASR.
