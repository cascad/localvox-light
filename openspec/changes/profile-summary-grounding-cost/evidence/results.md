# Компоненты проверки сводки

2026-11-03. Измерительный срез завершён: два свежих release процесса с настоящим
GLiNER на сохранённых публичных STT/summary. **LLM не запускался**, ускорение и
проверка качества новой модели не выполнялись. [Численные отчёты](native-results.json),
[инженерная команда](../../../../docs/summary-grounding-profile.md).

## Реализация

[Общая проверка](../../../../crates/localvox-light-llm/src/grounding.rs) получила
[opt-in observer](../../../../crates/localvox-light-llm/src/grounding/profile.rs).
Штатные API вызывают те же вычисления с выключенным observer, без чтения часов,
новых логов и изменения кеша. Порядок и результаты проверок сохранены.
[Отдельный CLI](../../../../crates/localvox-light-bench/src/bin/localvox-grounding-profile.rs)
считает реальные `Ner.names` requests под существующим `SourceEntities`, отдельно
замеряет загрузку, проверку и fingerprints. Report содержит числа/SHA, без текста,
имён, spans и ошибок NER. Штатный pipeline и пользовательский журнал не менялись.

Отказ NER сохраняется как incomplete/error, с `name_flags=null`, а не как
успешный пустой результат. Output создаётся через `create_new`; существующий файл
не перезаписывается. Отчёт обновляется после каждого завершённого прохода.
Supervisor ограничил каждый native процесс 300 s; оба завершились штатно.

## Входы и условия

Основание исходников: `2a96343`, плюс изменения этого среза; точные SHA трёх source
files и release probe сохранены в JSON. Между двумя запусками они не менялись.
Рабочий `localvox-process.exe` не пересобирался/не заменялся; его прежний SHA
`85474d4ed43753f3f1bd51217e104825cbdf19991a5fb0b13eaa62886a6e1696` сохранился.
Модели, defaults, настройки, `.env`, активный daemon и архив владельца не менялись.

Использована готовая пара из [native reference 2026-11-02](../../profile-text-postprocessing-cost/evidence/native-reference.md):
публичная Podlodka #109, 704 реплики, 7146.527375 s записи. Сырой JSONL 232493 bytes,
SHA `09014305b713638301d3cd4693d03e1f7f2920e75bdfc749c7164d0e10034ae8`.
Speech после соединения реплик через newline — 104622 Unicode chars; это прежние
103919 chars текста плюс 703 разделителя. Готовый markdown 16847 bytes,
SHA `3b44981a9c17877775fbba7ffb81661addc5d93633c4d8e4faf95d4ed520f778`;
после снятия provenance/citations для проверки остаётся 8523 chars.
Повторных STT, скачивания, чтения пользовательского архива не было.

Built-in summary-ru template/lexicon, пустой glossary, без загрузки `.env`.
Текущий GLiNER `model_fp16.onnx`, 579717643 bytes, SHA
`9cec3c771bfa7e886e029cd013326b13ad7d18f58fac33824f43c106fbdfe920`.
Source threshold 0.3, answer 0.5; name/role labels и policy не менялись.
SHA tokenizer/config/остальных файлов каталога и checked texts — в JSON.

Windows x86_64, AMD Ryzen 9 9900X3D, Windows сообщает 6 logical processors.
ORT Rust binding `2.0.0-rc.12`; текущий `Ner::open` использует default CPU session,
graph optimization Level3, без подключения GPU provider. ONNX Runtime статически
линкуется в release probe; отдельные runtime DLL в каталоге не найдены, их пустой
список в harness не является доказательством полного состава зависимостей ОС.
Новый процесс даёт свежую residency/пустой document cache, **не cold disk cache**:
файлы прочитаны для hashes до загрузки, кеш ОС не очищался. Фоновая desktop-нагрузка
не контролировалась. Во время серии сборки/другие модельные прогоны не запускались.

## Измеренные компоненты

Время в секундах. Backend вложен в request; request входит в total.
Нельзя повторно складывать backend с компонентами. Повторный request к source
включает только lookup/копию уже существующего кеша.

| Компонент | R1 первый | R1 повторный | R2 первый | R2 повторный |
|---|---:|---:|---:|---:|
| Source preparation | 0.030285 | 0.031105 | 0.030568 | 0.036456 |
| Answer numbers | 0.000640 | 0.000580 | 0.000559 | 0.000615 |
| NER text preparation | 0.000054 | 0.000052 | 0.000050 | 0.000053 |
| Source entity request | 54.632242 | 0.000011 | 61.784736 | 0.000012 |
| Source entity normalization | 0.000120 | 0.000099 | 0.000119 | 0.000113 |
| Answer entity request | 3.607184 | 3.357369 | 5.090048 | 4.594848 |
| Answer entity normalization | 0.000006 | 0.000007 | 0.000007 | 0.000007 |
| Entity matching | 0.009177 | 0.009037 | 0.012680 | 0.008803 |
| Lexical matching | 0.001172 | 0.001199 | 0.001319 | 0.001183 |
| Result assembly | 0.000001 | 0.000001 | 0.000002 | 0.000001 |
| **Total проверки** | **58.281448** | **3.399935** | **66.920652** | **4.642574** |
| Фактические source / answer backend calls | 1 / 1 | 0 / 1 | 1 / 1 | 0 / 1 |

| Вне профилированных проверок | R1, s | R2, s |
|---|---:|---:|
| Подготовка, включая hashes | 2.661391 | 0.530065 |
| Из неё fingerprints каталога модели, вложенное | 2.645973 | 0.512259 |
| Загрузка NER model/tokenizer/session | 1.062729 | 1.055442 |
| Штатная непрофилированная проверка для parity | 3.338917 | 3.427941 |
| Конечная сверка hashes | 0.511972 | 0.511304 |
| Остаток wall вне измеренных верхних интервалов | 0.006091 | 0.006497 |
| **Общий wall процесса** | **69.262484** | **77.094475** |

Каждый процесс сделал 1 source и 3 answer backend calls (включая обычную проверку),
0 ошибок. Все три проверки дали точное совпадение `Ungrounded`, в том числе
1 name / 2 number flags. `success=true` означает завершённый профиль, **не успешную
семантическую проверку сводки**: сохранённый результат остаётся unverified.
Source cache ограничен точным исходником одного документа; ответ проверяется
каждый раз, новый процесс не наследует entities первого.

## Что установлено и что остаётся неизвестным

Первый source NER занимает 54.632/61.785 s, около 92–94% первой проверки;
model load около 1.06 s. Детерминированная подготовка/сопоставление вместе —
около 0.042–0.048 s на проверку. Повторный проход по source не нужен уже сейчас:
фактический backend count равен нулю. Доказательств повторной загрузки модели
или дорогой lexical preprocessing этот замер не даёт.

Следующий обоснованный срез — внутренние затраты `Ner.names`: person/role passes,
tokenization/windows, ONNX inference и постобработка. Сейчас один backend call
объединяет их; всё время нельзя приписать только ORT. Оптимизация NER/кеша,
смена quantization/порогов, параллелизм и обрезка source здесь не реализованы.

Исторические 66.745/57.477 s и сумма дальнейших проверок 5.863/5.709 s из BL-003
остаются прежним результатом. Новая проверка использует целый готовый документ;
промежуточные map/reduce ответы не сохранены, сравнивать warm как равный workload
нельзя. Нет нового end-to-end времени генерации, оценки сводки на слух, теста Mac,
общего SLA и принятия новой модели/default.

## Выполненные проверки

- `cargo test --locked --offline -j 2 -p localvox-light-llm --lib` — 108 passed;
  новые cases проверяют exact verdict parity с/без NER, числа/пустой ответ,
  source hit/replacement/новый документ, повторные answer calls и privacy timings.
- `cargo test --locked --offline -j 2 -p localvox-light-bench --bin localvox-grounding-profile`
  — 4 passed, 1 ignored; numeric privacy/parity, отказ overwrite, NER load/request
  errors с partial/unknown, снятие provenance/citations.
- Явный ignored reference receipt check — 1 passed: штатный `artifacts::verify`
  подтвердил input/output сохранённой summary, processing.json не менялся.
  Canonical transcript receipt SHA `3bb45041173effd7bf23c8c9611d01cd3487d1fc763a5cf9d3e24af93b28b00c`
  относится к parsed speech tuples, не к байтам JSONL; проверен Rust-кодом,
  не подменён сравнением raw file hash.
- Release build probe, `--help` и targeted rustfmt — passed; рабочий worker не собирался.
- Оба native отчёта complete/success, exact parity и input/models hashes до/после
  совпали; SHA probe/source files между запусками сохранены. В evidence только
  численные данные, static metadata и hashes, без исходных реплик/сводки/entities.
- `openspec validate --specs --strict --no-interactive` — 15 passed, 0 failed;
  strict validation выбранного change — valid, zero deltas accepted при skip_specs.
- `python scripts/check-openspec.py` — PASS: 15 capabilities, 88 requirements,
  188 scenarios, 88 coverage rows, 1503 local links; `git diff --check` — passed.

CI дополнен model-free проверками нового инструмента и rustfmt. Пользовательский
сценарий не менялся, поэтому справочник не пополнен инженерными флагами.
