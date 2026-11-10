# R4 · Кандидат для просмотра, 2026-11-10

UI/TUI готовой части реализованы, комплект собран. **Это точка передачи владельцу**,
дальше — баги по просмотру. Рефакторинг/новые фичи не начаты. Change не завершён:
7.3 default-on заблокирован reference budget, 7.4 hardware not-run. Требование
PIPE-018 и лимит 20% сохранены; режим включается явно для просмотра кандидата.

[Инструкция запуска и проверки](../../../../docs/incremental-acceptance.md),
[SHA собранного комплекта](candidate.json), [короткий справочник](../../../../docs/user-guide.md).
Актуальный `npm --prefix ui run build` выполнен **до** совместной release-сборки
daemon/worker/TUI. Обычный worker отвечает `localvox-worker/2` и отвергает fixture
CLI. Действующий daemon и личный архив не запускались/не менялись; push не выполнялся.

## Финальный автономный прогон

TEMP/TMP на D:, cargo locked/offline, `-j2`, `RUST_TEST_THREADS=2`, baselines не
пересоздаются. Существующие compiler warnings не исправлялись в этом этапе.

| Проверка | Фактический результат |
|---|---|
| Core ONNX, `--lib` | 448 passed / 8 ignored, 17.16 s |
| API, `--lib` | 80 passed / 8 ignored, 17.05 s |
| Application, `--lib` | 14 passed / 10 ignored, 6.65 s |
| Client | 3 + 8 passed |
| Process | 12 passed / 1 ignored |
| TUI library | 127 passed / 2 ignored |
| TUI normal integration / ConPTY | 2 + 3 + 2 passed; real mpv ignored в этой общей команде |
| Coordinator fault fixture | 7 режимов, 1 passed, 30.58 s; сохранены входы/receipts |
| Child-worker crash fixtures | 3 passed, 3.74 s |
| UI build / unit / browser | build passed; 10 unit; 77 browser passed, 27.7 s |
| Новый native profiler | cargo check passed; короткий probe и полная native серия выполнены |
| Совместная normal release-сборка | daemon + worker + TUI passed, 65 s; актуальный UI встроен |

У reference worker и совместно пересобранного комплекта отдельные binary SHA,
оба сохранены. Именно кандидатный worker дополнительно прошёл 330 s native probe:
окна/text/calls совпали, Stop→ready 1.644 s, wall +45.19%; это стендовый smoke,
не новая длинная reference-серия и не успешный budget gate. Полный report —
`target/incremental-candidate-probe-20261110/`, SHA/исход в candidate JSON.

Browser включает три намеренно отрицательных visual detector сценария: ожидаемые
несовпадения symbol/color/geometry не являются скрытыми сбоями остальных тестов.
Игнорируемые codec/mpv сценарии действительно запускались отдельно в
[R3](partial-clients.md); этот общий прогон сам их не выполнил.

Обычные команды регрессий: `cargo test --locked --offline -j2 -p localvox-light-core
--features onnx --lib`, аналогично API/application `--lib`; `cargo test --locked
--offline -j2 -p localvox-light-client -p localvox-light-terminal -p localvox-light-process`;
`npm.cmd --prefix ui test`, `npm.cmd --prefix ui run test:e2e -- --workers=2`.
Сборка: `npm.cmd --prefix ui run build`, затем `scripts/run.ps1 -Tui -BuildOnly`.

## Локальная traceability delta

IDs здесь относятся к этому change, не принимаются в main specs этой таблицей.
Старые отчёты этапов не переписаны как новые результаты.

| Requirement | Код | Проверка / оставшаяся граница |
|---|---|---|
| STORE-004 | [capture](../../../../crates/localvox-light-core/src/chunks/capture.rs), [planner](../../../../crates/localvox-light-core/src/chunks/planning.rs) | [capture/codec](capture-input.md), [planner](capture-planner.md), [fault series](fault-series.md); hardware/power-loss not-run |
| PIPE-015 | [shared windows](../../../../crates/localvox-light-core/src/windowing.rs), [prefix](../../../../crates/localvox-light-core/src/cook/prefix.rs) | [native reference](native-reference.md): все PCM windows/text/calls совпали; native diarization отдельно not-run |
| PIPE-016 | [cache](../../../../crates/localvox-light-core/src/incremental/cache.rs), [full cook](../../../../crates/localvox-light-core/src/cook.rs) | [cache](asr-cache.md), [crash/reuse](prefix-worker.md), [fault](fault-series.md), native coverage/no duplicate ASR |
| PIPE-017 | — | Вне выпуска; ранняя LLM не реализована |
| PIPE-018 | [queue](../../../../crates/localvox-light-core/src/jobs/prefix.rs), [coordinator](../../../../crates/localvox-light/src/autocook.rs), [settings](../../../../crates/localvox-light-core/src/settings.rs) | [R2](prefix-coordinator.md), [fault](fault-series.md); **default on не выполнен**, [reference budget fail](native-reference.md) |
| PIPE-002, PIPE-011 | [jobs](../../../../crates/localvox-light-core/src/jobs.rs), [workflow](../../../../crates/localvox-light-core/src/workflow.rs) | protocol 2, finite/coalesced intent, clocks/WaitingInput, Cancel/Retry; [R2](prefix-coordinator.md), [R3](partial-clients.md) |
| ARCH-006 | [API](../../../../crates/localvox-light-api/src/archive/partial.rs), [UI](../../../../ui/src/DocumentPane.tsx), [TUI](../../../../crates/localvox-light-terminal/src/app.rs) | [R3 HTTP/PCM/player/оба клиента](partial-clients.md), финальные browser/ConPTY выше; реальные устройства/Mac not-run |

Пользовательское ограничение: при полностью не поступающей объявленной дорожке
общая публикация может ждать Stop. Отсутствие пакетов не считается подтверждённой
тишиной/EOF. Безопасность стыков проверена по sample ranges, а не текстовым эвристикам.

Документационные проверки после оформления: strict main **15 passed / 0 failed**,
strict change valid; `check-openspec.py` — **88 requirements, 188 scenarios,
88 coverage rows, 1709 local links PASS**. Scoped rustfmt и `git diff --check`
прошли. Перед коммитом отдельно сверяются staged diff/файлы; секреты и generated
target/архивы не входят в коммит. Настройка Windows CI дополнена форматированием
и сборкой нового profiler; запуск удалённого CI этим не заявлен.
