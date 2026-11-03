# Измерить проверку готовой сводки

Инженерный инструмент читает JSONL и сохранённый markdown, запускает существующую
проверку имён/чисел с локальным GLiNER. LLM, STT, очередь и пользовательский архив
не запускаются. `.env` не читается; используются встроенный lexicon, summary
template выбранного языка и пустой glossary. Это замер проверки готовой пары
текстов, а не генерации map/reduce и не приёмка качества сводки.

[Reference от 2026-11-03](../openspec/changes/profile-summary-grounding-cost/evidence/results.md):
два свежих release процесса, source NER 54.632/61.785 s, загрузка около 1 s;
повторная проверка 3.400/4.643 s с существующим кешем исходника. Ускорение не вносилось.

```powershell
cargo build --release --locked -j 2 -p localvox-light-bench --bin localvox-grounding-profile
.\target\release\localvox-grounding-profile.exe --source D:\bench\source.jsonl --answer D:\bench\summary.md --ner-model-dir models\ner-gliner --output target\grounding-new.json --lang ru --passes 2
```

Output должен быть новым файлом. При отказе NER сохраняется частичный численный
отчёт с `success=false`; пустой ответ при ошибке не считается успешной проверкой.
Если входы неверны до запуска профиля, создание отчёта не гарантируется.
Проверка моделей может занимать минуту; внешний supervisor должен ограничивать
время процесса. Исходные файлы/модели не изменяются, их hashes сверяются после работы.

Из сохранённого markdown снимается заголовок provenance и citation markers,
как до grounding в pipeline. Speech состоит из текста реплик в порядке времени,
без speaker/timecode разметки. Используются текущие NER threshold/labels/model
override окружения worker; значения labels в отчёте заменены hashes.
Нужен стандартный предпочитаемый файл (`model_fp16.onnx`, затем `model_q4f16.onnx`,
затем `model.onnx`) либо явный `LOCALVOX_NER_MODEL_FILE` внутри указанного каталога.

В JSON:

- SHA входных файлов, проверяемых текстов, бинарника и файлов модели;
- отдельное время подготовки/фингерпринтов, загрузки модели и конечной проверки SHA;
- последовательные компоненты каждой проверки: source preparation, числа ответа,
  NER text preparation, source/answer requests и нормализация, entity/lexical matching;
- реальные `Ner.names` calls/errors/durations, число сомнений без самих имён/чисел;
- совпадение с результатом штатного непрофилированного API после измеренных проходов.

Время backend вложено в request, а request — в total проверки: их нельзя складывать
повторно. Fingerprinting входит в preparation. Request к source также включает
lookup/копию существующего кеша; счётчик backend показывает фактический вызов NER.
Один такой вызов содержит отдельные person/role проходы и внутренние окна — это
не счётчик отдельных ONNX inference. Кеш ограничен одним точным источником и одним
документом, ответы проверяются заново. Перезапуск программы даёт cold process,
но файловый кеш ОС не очищается. `success` инструмента не означает качество summary.

Штатные `check_parts` / `check_with_entities` используют общую реализацию с
выключенным observer: без измерений, новых логов и постоянного кеша.

Для receipt исходной сохранённой сессии есть отдельная read-only проверка:

```powershell
$env:LOCALVOX_GROUNDING_PROFILE_REFERENCE='D:\bench\session'
cargo test --locked -p localvox-light-bench --bin localvox-grounding-profile -- --ignored --exact tests::explicit_reference_receipts_verify_with_the_production_transcript_hash
```

Она вызывает штатный `artifacts::verify`; receipt hash parsed speech tuples и SHA
сырого JSONL — разные значения. Не сравнивайте их напрямую.
