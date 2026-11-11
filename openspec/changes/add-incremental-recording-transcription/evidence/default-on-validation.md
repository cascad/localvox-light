# Ранняя расшифровка: исправление кеша и default on · 2026-11-11

Продолжение 7.3 по прямому поручению владельца. После первого неуспешного замера
владелец выбрал «Сначала исправить задержки, затем включить». Исходные лимиты,
окна, модель, рецепт и проверки совпадения не ослаблялись. Main specs не sync/archive.

## Причина и исправление

В [R1](default-on-native-r1.json) одна пара дала +68.75%, суммарно +25.64% — fail.
В самой медленной порции ASR занял 4.017 s, кеш — 13.607 s. Изолированный journal
probe подтвердил стоимость синхронизации после каждого небольшого результата.
Это измеренная задержка storage; не вся разница wall сводится к одному syscall.

Теперь prefix и full cook используют общий [ResultBatch](../../../../crates/localvox-light-core/src/incremental/batch.rs):
до 8 outputs / 1 MiB, без удержания PCM; большой одиночный record ограничен прежним
8 MiB. [WAL](../../../../crates/localvox-light-core/src/incremental/wal.rs) сохраняет
прежние frames/checksums, синхронизирует группу, затем [cache](../../../../crates/localvox-light-core/src/incremental/cache.rs)
перечитывает и проверяет результаты. Только после этого выдаётся Accepted и
разрешены checkpoint, текст, версия и receipts. Orphan frames после отказа sync
повторно синхронизируются перед принятием, включая восстановление checkpoint.
При crash допускается повтор неподтверждённой группы, сохранённые окна не теряются.
Свежие PCM/model hashes, границы окон, worker protocol и legacy JSON сохранены.

Сопоставимый journal probe после исправления: 1608 frames / 1 014 648 bytes;
single **6.471 s / 1608 sync**, batch8 **0.919 s / 201 sync**. Собственно sync:
1.368 → 0.185 s. Первый отдельный probe 19.005 s нельзя считать чистым A/B:
между сериями изменилось свободное место на D:. Детали, исходные попытки и
зафиксированный до запуска повтор — в [repair plan](default-on-repair-plan.md).

## Полный native R3

Публичный PCM 7146.527375 s / 114344438 samples, дорожка 1 — настоящий zero PCM.
GigaAM int8 / ru / r7 / 15/8/500; LLM и diarization выключены. Два последовательных
контроля, каждый O1/I1 steady300, O2/I2 burst900, O3/I3 steady300; новые кеши,
без ограничения длины, без параллельной сборки/другого нашего ASR. Один release
worker, SHA `a24a091078a2a1f6711e9fb549b10442350c105707a9b369c39c08d5ce354cb5`.
SHA входа/весов/vocab проверены profiler до и после. Plan/report SHA, CPU, logical
I/O, peak memory, active wall и отдельные prefix timers сохранены в JSON.

| Контроль | Пара 1 | Пара 2 | Пара 3 | Суммарно | Gate |
|---|---:|---:|---:|---:|---|
| [Исходный legacy offline](default-on-native-r3-legacy.json) | −4.67% | +12.46% | +14.01% | +6.25% | 3/3 pass |
| [Текущий одинаковый capture/planner](default-on-native-r3-current.json) | +4.43% | +8.12% | +0.09% | +4.03% | 3/3 pass |

Порог **≤20% в каждой паре и суммарно**. Все 12 полных runs: одинаковые
1608 windows, 704 lines, 714 ASR calls, один model load и worker start. В режиме I
после Stop распознаётся только хвост: 24 calls, 1552 cache hits; принятые окна не
распознаются повторно. Current Stop→ready **5.657 / 12.440 / 6.418 s**, offline
**78.461 / 78.039 / 84.308 s**. Работа с кешем во всех ранних порциях одной записи:
current **0.993 / 0.946 / 1.237 s** (включая проверки, не только запись).

R1 fail сохранён; legacy R2 прошёл, current R2 прерван до завершения ради исправления
recovery и не засчитан. Оба полных R3 повторены после этой правки. Старые отчёты
+48.12%, resident legacy fail и current +2.95% остаются историческими результатами.
Изменение состояния диска не позволяет приписать весь выигрыш R1→R3 только коду;
сопоставимый journal probe подтверждает отдельно эффект группировки.

## Default и локальная traceability

В [.env.defaults](../../../../.env.defaults) `LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE=on`.
Без override режим включён, явный сохранённый off сохраняется. Поле видно в обычных
UI/TUI настройках и применяется live; сброс возвращает on. Выключение завершает
текущую порцию и оставляет оставшуюся работу до Stop; включение возобновляет ранние
порции. Нужна включённая автоварка. US-050 обновлён в [справочнике](../../../../docs/user-guide.md).

| Requirement | Код | Новое доказательство / граница |
|---|---|---|
| PIPE-016 (delta) | incremental/{batch,cache,wal}, cook и cook/prefix | bounded pending/drop/duplicate, write/sync refusal, orphan recovery, actual child-worker crash; native no-repeat |
| PIPE-018 (delta), CFG-001/002 | presets/settings, coordinator, HTTP settings, UI/TUI | default on/off/reset/live; real coordinator и общие регрессии ниже |
| PIPE-002/008/010/011/012 | прежние worker/queue/workflow и новые cache consumers | полные R3 correctness + performance; worker protocol 3 без изменения |
| ARCH-001/002, ARCH-006 (delta) | прежние UI/TUI/partial API | browser/ConPTY/shared HTTP; visual baselines не пересоздаются |

Это дополнительное свидетельство принятых требований и локальная карта новых IDs,
не принятие delta. Ранняя LLM-правка PIPE-017 остаётся за границей выпуска.
Аппаратные microphone/loopback, слышимый звук и слабый Mac — **not-run** (7.4).

## Регрессии

`TEMP/TMP=target/tmp`, Cargo locked / jobs=2 / test threads=2, `INSTA_UPDATE=no`;
личный архив и устройства не используются. Логи — `target/tmp/incremental-default-*`.

| Проверка | Фактический результат |
|---|---|
| Общий Rust (core/asr/application/API/process/TUI/LLM/ingest) | **915 passed / 36 ignored**, плюс custom resident_io harness exit 0 |
| В том числе core ONNX / API / TUI library | 467 / 81 / 199 passed |
| Actual child-worker fixtures | 6 passed в общем прогоне: до group sync, после durable result, checkpoint/final crash, no-repeat, legacy/force/resident |
| TUI normal integration / real ConPTY / run script | 2 / 5 / 2 passed; прежние snapshots сохранены |
| Explicit real coordinator | 1 passed / 35.15 s; 8 режимов, включая отсутствие override, growth/Stop/disable/cancel/Retry и fault |
| Explicit shared HTTP + реальные UI/TUI | 1 passed / 8.47 s; отдельная временная fixture без speech engine |
| UI build / unit / browser | build passed / 14 unit / 81 browser passed, 33.4 s |

В browser прогоне три намеренных symbol/color/geometry несовпадения — ожидаемые
отрицательные проверки visual detector. Остальные snapshots не пересоздавались.
Settings HTTP проверяет не только DTO, но и настоящий live scheduler flag после
off/on/reset, отсутствие restart и запись в отдельную `.env` fixture. TUI и browser
проверяют те же действия; отмена несохранённых правок не меняет сохранённый режим.
В локальном пользовательском `.env` override отсутствует; файл не изменялся.
Работающего пользовательского демона при проверке нет, устройства не запускались.

Воспроизведение общего прогона:

```powershell
cargo test --locked -p localvox-light-core -p localvox-light-asr -p localvox-light -p localvox-light-api -p localvox-light-process -p localvox-light-terminal -p localvox-light-llm -p localvox-light-ingest --features localvox-light-asr/asr-fixtures --jobs 2 -- --test-threads=2
npm.cmd --prefix ui run build
npm.cmd --prefix ui test
npm.cmd --prefix ui run test:e2e -- --workers=2
```

Для explicit coordinator предварительно собрать fixture worker/TUI с
`--features localvox-light-asr/asr-fixtures`, задать `LOCALVOX_TEST_ASR_WORKER` и
запустить через Cargo `--ignored --exact autocook::prefix_tests::finite_prefix_coordinator_handles_growth_stop_disable_cancel_and_retry`.
Для shared HTTP — `cargo test --locked -p localvox-light-api --lib --jobs 2 -- --ignored --exact http::tests::cross_clients::shared_workspace --test-threads=1`.

## Итоговый комплект

Release daemon + worker + TUI + profiler собраны вместе за 1m24s; в daemon встроен
актуальный UI, в preset — on. Комплект `target/review/incremental-default-on`
содержит три runtime EXE, Vosk DLL/dependencies, DirectML и `.env.defaults`;
пользовательская `.env` не копируется. Копии проверены SHA-256, полный manifest и
результат smoke — [bundle evidence](default-on-bundle.json). Worker protocol probe:
`localvox-worker/3`, TUI version probe и daemon `--help` завершились exit 0.

Именно скопированный worker с новым baked preset прошёл отдельную 330 s native
пару: 75 windows / 33 lines / 33 ASR calls совпали, load/start=1. Stop→ready
0.877 s, wall −1.21%, report pass=true / **reference=false**. Это проверка
собранного комплекта, не замена двух полных R3 gates: явные worker-phase запросы
profiler не зависят от boolean default. Автоматическое использование default
проверено настоящим coordinator выше. Аппаратные сценарии этим не выполнены.

Запуск и короткая ручная приёмка — [инструкция](../../../../docs/incremental-acceptance.md).

Финальные проверки документации: strict main **15 passed / 0 failed**, strict
change valid; check-openspec **15 capabilities / 88 requirements / 188 scenarios /
88 coverage rows / 1894 local links PASS**. Scoped rustfmt `--check` и
`git diff --check` прошли. Baseline не синхронизируется, 7.4 not-run; push не выполняется.
