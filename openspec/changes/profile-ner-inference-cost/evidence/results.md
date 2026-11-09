# Внутренние затраты NER

2026-11-09. Измерительный срез выполнен: два fresh release процесса с настоящим
GLiNER, без LLM generation. **99.46% source NER wall занимает `session.run`**;
name и role pass стоят примерно поровну. [Численные данные](native-results.json).
Это локализация затрат; ускорение, смена модели и приёмка качества не выполнялись.

## Реализация и проверки

[Core NER](../../../../crates/localvox-light-core/src/ner.rs) использует общую
реализацию с [локальным observer](../../../../crates/localvox-light-core/src/ner/profile.rs).
Обычные `names`/`extract_at` выключают clocks; `names_profiled` возвращает исходный
Result отдельно от численных timings. В измерениях только static pass/component,
calls/errors, words/windows/tokens и времена. Нет текста, labels, имён, spans,
logits и сообщений backend. Ошибка сохраняет partial с complete=false и счётчиком
в той категории, где она произошла. Категории exclusive, агрегированы по pass;
объём отчёта не растёт с числом окон.

[Bench profiler](../../../../crates/localvox-light-bench/src/bin/localvox-grounding-profile.rs)
получил `--ner-components`; backend adapter сохраняет отдельные source/answer
profiles. Кеш source остаётся прежним и ограничен одним точным документом. Перед
ordinary parity core observer отключается. [Команда и API](../../../../docs/summary-grounding-profile.md).
Порядок name/role inference, thresholds, окна 120/20, logits/NMS/dedup и сравнение
score не менялись. Рабочие daemon/worker, `.env`, defaults и модели не обновлялись.

Выполнено:

- Core `cargo test --locked --offline -j 2 -p localvox-light-core --features onnx ner:: --lib`
  — 4 passed, 1 ignored: прежнее разбиение слов, disabled observer/result/error/order,
  finite/exclusive timing, partial/counters/privacy, отсутствие фиктивных окон/ролей.
- Bench unit — 6 passed, 1 ignored: прежние privacy/overwrite/отказы, opt-in CLI,
  profiles первого/повторного вызова и отключение перед ordinary parity.
- LLM unit — 108 passed, 0 failed. Release build probe, `--help`, targeted rustfmt
  — passed. Полный UI/TUI/e2e не повторялся: пользовательское поведение не менялось.
- Явный release native Entity parity — 1 passed, **8 случаев / 26 entities**:
  короткий текст, несколько перекрывающихся окон, пустой текст, пустой role label,
  thresholds 0.3/0.5. Совпали полные Entity, включая f32 scores и byte spans;
  положительный контроль не пуст. Это не оценка recall/precision модели.
- Strict main specs — 15 passed, 0 failed; strict выбранного change — valid
  с skip_specs/zero deltas. `python scripts/check-openspec.py` — PASS:
  15 capabilities, 88 requirements, 188 scenarios, 88 coverage rows, 1518 local links.
  Targeted rustfmt и `git diff --check` — passed. Model-free команды добавлены в CI.

## Материал и условия

Исходники от `19af3d6` плюс этот срез. SHA пяти source files и release probe
зафиксированы между запусками; binary SHA
`862e66a0133fd7736aa52eea09ebec78f5193396f24ff8ca604c299bd5045ff4`.
Прежний рабочий worker сохранил SHA
`85474d4ed43753f3f1bd51217e104825cbdf19991a5fb0b13eaa62886a6e1696`.
Cargo.lock/зависимости не менялись. Все новые сборки/отчёты размещены в target на D:.

Прежняя публичная пара [reference](../../profile-text-postprocessing-cost/evidence/native-reference.md):
Podlodka #109, 7146.527375 s, 704 реплики. Сырой JSONL SHA
`09014305b713638301d3cd4693d03e1f7f2920e75bdfc749c7164d0e10034ae8`;
summary markdown SHA `3b44981a9c17877775fbba7ffb81661addc5d93633c4d8e4faf95d4ed520f778`.
Speech с newline-разделителями 104622 chars, checked answer 8523 chars.
Built-in summary-ru template/lexicon, пустой glossary, `.env` не читается.
Сняты provenance/citations. Архив владельца, повторные STT/download не использованы.

Тот же GLiNER FP16 579717643 bytes, SHA
`9cec3c771bfa7e886e029cd013326b13ad7d18f58fac33824f43c106fbdfe920`;
tokenizer/config/прочие файлы каталога и labels hashes сохранены в JSON.
Source threshold 0.3, answer 0.5, role comparison pass threshold 0.1.
Windows AMD64, Ryzen 9 9900X3D, Windows/Python сообщают 6 logical processors.
ORT crate `2.0.0-rc.12`, прежний default CPU session/Level3, без GPU provider.
Threads оставлены auto; фактическое scheduling и отдельные ORT operators неизвестны.

Два последовательных процесса, по два profiled checks и один ordinary parity;
timeout каждого 300 s, оба штатно завершились. Сборки закончены до серии.
Файловый кеш ОС не очищался, hashes читают модель до загрузки; это свежая process
residency/document cache, не cold disk cache. Desktop-нагрузка не контролировалась.

## Source NER: категории

Время в секундах, name+role суммируются как последовательные интервалы внутри
одного NER request. Они вложены в backend, затем в grounding request/check total.
Не складывать эти уровни повторно. `session.run` включает Rust/ORT boundary и
выполнение графа, не является отдельным GPU/kernel-only измерением.

| Категория | R1, s | R2, s |
|---|---:|---:|
| Split words | 0.005759 | 0.005485 |
| Window input | 0.002766 | 0.002669 |
| Tokenization | 0.243426 | 0.246027 |
| Tensor preparation | 0.001449 | 0.001524 |
| Session lock acquisition | 0.000024 | 0.000025 |
| Tensor creation | 0.002047 | 0.002019 |
| **Inference (`session.run`)** | **48.963002** | **50.109584** |
| Logits copy | 0.000922 | 0.000921 |
| Span decode/NMS | 0.003161 | 0.003151 |
| Window dedup | 0.000059 | 0.000048 |
| Role comparison | 0.000055 | 0.000053 |
| Observer/код вне категорий | 0.007576 | 0.007514 |
| **Total source NER** | **49.230243** | **50.379020** |

Name pass (exclusive categories) 24.605134/25.221851 s, role pass
24.617478/25.149602 s. В обоих запусках 22088 words, **221 окон на каждый pass,
442 фактических session.run**. Name tokens total/max 50336/251, role 50778/253;
это window sequence lengths вместе с prompt и overlap, не уникальные tokens речи.
Все окна завершены, errors=0; ненужных искусственных inference для comparison нет.

## Проверка целиком и внешние затраты инструмента

| Операция | R1, s | R2, s |
|---|---:|---:|
| Подготовка, включая hashes | 22.277728 | 0.548624 |
| Из неё model fingerprints, вложенное | 22.262238 | 0.518568 |
| Model/tokenizer/session load | 1.053699 | 1.018117 |
| Первый grounding check (source+answer+детерминированная часть) | 52.651882 | 54.208659 |
| Повторный grounding check | 3.439404 | 3.027825 |
| Ordinary unprofiled parity check | 3.219020 | 3.087670 |
| Конечная сверка hashes | 0.521377 | 0.521952 |
| Остаток wall вне верхних интервалов | 0.206623 | 0.026956 |
| **Wall инженерного процесса** | **83.369732** | **62.439803** |

Первое чтение hashes существенно дольше второго; причину на уровне ОС/диска
этим профилем не установили. Это подготовка инженерного стенда, не стоимость
генерации сводки. Разницу с [замером 2026-11-03](../../profile-summary-grounding-cost/evidence/results.md)
нельзя выдавать за реализованное ускорение: модели/алгоритм не оптимизировались,
условия кеша и desktop-нагрузки не фиксированы.

В каждом процессе backend сделал 1 source + 3 answer calls. Второй check и
ordinary не запускали source inference; ordinary profiles пусты. Точная parity
Ungrounded подтверждена: 1 name / 2 number flags во всех проверках. Итоговая
сводка остаётся unverified; success инструмента означает завершение измерения.
Source/answer/model SHA до/после и frozen source/probe/worker SHA совпали.

## Следующее решение и границы

Оптимизация split/tokenizer/dedup не даст существенного ускорения на этом материале:
весь код вне `session.run` занимает около 0.27 s. Следующий обоснованный кандидат —
сокращение role inference: проверить, какие полные исходные окна могут пересекать
name spans, и возможность пропустить остальные с точной Entity parity. Сейчас
выполняются все 221 role windows; допустимое число пропусков ещё не измерено.
Не менять размер/контекст оставшихся окон, thresholds и модель ради результата.
В этом срезе такой пропуск и ускорение не реализованы.

Сохранённый final markdown не воспроизводит промежуточные map/reduce LLM ответы.
Нет нового времени всей генерации, приёмки смысла/аудио, Mac/слабого железа или
универсального SLA. Пользовательский справочник не меняется: это инженерный флаг.
