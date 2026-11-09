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

Добавьте `--ner-components`, чтобы разобрать сам NER: отдельно name/role passes,
разбиение на слова, сборка окон, tokenizer, masks/tensors, ожидание session mutex,
`session.run`, копирование logits, декодирование, оконные дубли и сравнение ролей.
В `checks[].ner_components` — агрегаты времени, calls/errors, начатые/завершённые
окна и tokens total/max (включают prompt и overlap), без текста/labels/entities.
Эти интервалы **вложены** в backend NER, не прибавляйте их к backend/request/total.
Остаток `unattributed_sec` включает код вне границ категорий и инструментирование.
`complete=false` сохраняет частичные данные при отказе. Нулевые inference calls
при пустом тексте и отсутствующий role pass при пустом role label не подменяются
фиктивными измерениями. Для ordinary parity core instrumentation выключается.
Без флага core clocks не включаются. Формат инженерного отчёта — version 2.

[Измерение 2026-11-09](../openspec/changes/profile-ner-inference-cost/evidence/results.md):
source NER 49.230/50.379 s, 99.46% в session.run, по 221 name/role окну.
Оба прохода стоят примерно поровну; ускорение не вносилось.

Core API `Ner::names_profiled` возвращает `(Result<Vec<Entity>>, profile::Timings)`;
результат/ошибка и численный профиль доступны отдельно. Обычные `names`/`extract_at`
используют те же вычисления с disabled observer, без clocks/логов/нового кеша.

Поиск имён обрабатывает все прежние окна. Для ролей выполняются только полные
исходные окна, которые пересекают найденные имена. Индекс объединённых byte spans
не зависит от слов, labels или scores; контекст и модель оставшихся окон прежние.
Role span всегда внутри своего окна, поэтому отброшенное окно не может изменить
сравнение с именем. `windows_skipped` отдельно от started/completed/inference calls.
При плотных именах выигрыш может отсутствовать. Ошибка необходимого вызова остаётся
ошибкой; автоматического перехода на full scan или ослабления проверки нет.

`ner::profile::names_full_scan_profiled` — явный инженерный эталон того же
decoder/comparison с выполнением всех role окон. Для сохранённой публичной STT
есть read-only exact Entity comparison (в JSON только counts/times/SHA):

```powershell
$env:LOCALVOX_NER_PROFILE_TEST_MODEL_DIR=(Resolve-Path models\ner-gliner).Path
$env:LOCALVOX_NER_ROLE_COMPARE_SOURCE='D:\bench\source.jsonl'
$env:LOCALVOX_NER_ROLE_COMPARE_OUTPUT='D:\bench\compare-new.json'
$env:LOCALVOX_NER_ROLE_COMPARE_ORDER='full-first'
cargo test --release --locked -j 2 -p localvox-light-core --features onnx --lib -- --ignored --exact ner::profile::tests::saved_source_full_and_relevant_scans_are_exactly_equal --nocapture
```

Для второго свежего процесса задайте **новый** output и `relevant-first`.
Не запускайте сборки параллельно с измерениями; внешний timeout ограничивает процесс.
Это сравнение source NER на joined speech, без генерации LLM и без изменения архива.

[Проверенный результат отбора окон](../openspec/changes/skip-irrelevant-role-windows/evidence/results.md):
два сравнения дали 22–24% ускорения source NER при exact Entity parity;
обычный grounding сохранил verdict/cache. Raw joined speech и `grounding::plain`
имеют разное число слов/окон: счётчики сравниваются с full plan того же входа.

Явная проверка точного совпадения entities, scores и byte spans на синтетической
речи с настоящим NER (включая overlap/пустой текст/отключённый role pass):

```powershell
$env:LOCALVOX_NER_PROFILE_TEST_MODEL_DIR=(Resolve-Path models\ner-gliner).Path
cargo test --release --locked -j 2 -p localvox-light-core --features onnx --lib -- --ignored --exact ner::profile::tests::real_model_profile_preserves_entities_scores_and_byte_spans --nocapture
```

Это проверка совпадения реализации; качество распознавания новых имён и summary
не оценивается. Для моделей используется та же конфигурация окружения `Ner::open`.

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
