# Объяснение автоматического повтора · 2026-11-01

Продолжение проверки автоматического повтора и восстановления, PIPE-010/011,
уточнённый PIPE-016. Пользовательские jobs.json, архив, .env, работающий демон,
модели и устройства не изменялись. TEMP/TMP тестов находятся в target/test-temp на D:.

## Воспроизведение

Новый core test `automatic_retry_is_not_promised_when_the_coordinator_is_disabled`
выполнен на прежнем коде: **FAIL**, actual `retry_wait`, expected `blocked`, хотя
передан disabled coordinator с явной причиной. Лог target/retry-state-red.log.
Прежняя execution проверяла coordinator только для Pending/Running. Для Failed
отсутствующая дата также ошибочно означала требование ручного повтора, хотя
revive_failed допускает автоматическое возвращение legacy задания без failed_at.
Ни один результат не выдаётся за измерение скорости моделей или новой restart-сборки.

## Реализация

- [workflow](../../../../crates/localvox-light-core/src/workflow.rs): чистый retry_plan
  для Failed, общий для progress и queue. Сохранены MAX_REVIVALS и automatic_retry_at;
  jobs/scheduler не менялись. Fresh ready даёт retry_wait; остальные известные состояния
  координатора — blocked с исходной причиной; stale/invalid heartbeat — stalled;
  None — неизвестный координатор. Исчерпанный бюджет — failed/ручной повтор,
  независимо от координатора. Неизвестная дата с оставшимся бюджетом не означает
  исчерпание. State/message/retry_at одинаковы при одинаковом snapshot/clock.
- [Archive](../../../../crates/localvox-light-api/src/archive.rs): phase и optional
  retry plan добавлены к QueueItem, один coordinator snapshot/now на весь ответ.
  Дополнительных обходов/проверок артефактов на строку нет; порядок не меняется.
  [Rust DTO](../../../../crates/localvox-light-client/src/types.rs) принимает старые
  строки без новых полей; TypeScript поля также optional.
- [UI](../../../../ui/src/Queue.tsx): фаза и число попыток, причина, нижняя граница
  «Повтор не раньше», явная неизвестная дата/неподтверждённый план старого API.
  Кнопка ручного retry, порядок и polling сохранены. При ошибке остаётся последний
  план с прежней отметкой недоступности очереди, при восстановлении — свежие данные.
- [TUI document](../../../../crates/localvox-light-terminal/src/app/documents.rs)
  дополняет выбранную задачу теми же сведениями; дата выделена цветом аргумента,
  фаза — цветом значения, неподтверждённый/исчерпанный план — цветом ошибки.
  В [списке](../../../../crates/localvox-light-terminal/src/render.rs) `~ повтор`
  означает ожидание, `? повтор` — неподтверждённый запуск. Старые данные не получают
  выдуманную фазу/дату.

## Существенные проверки

- Core workflow: **11 passed**. Матрица ready/starting/disabled/blocked/stopped,
  stale и invalid heartbeat, None; точные 3600/7200/14400 секунд; budget exhausted,
  missing failed_at, неизменность Job и сохранение deadline после сериализации/reload.
  Существующий успешный backoff case теперь явно передаёт fresh ready; неизвестный
  координатор проверяется отдельным сценарием и не считается работоспособным.
- [Настоящий HTTP](../../../../crates/localvox-light-api/src/http/tests/workspace.rs):
  Queue/progress/client сообщают ту же фазу и план. Для всех revival rounds сохранены
  число попыток, интервалы, stuck и исходные байты jobs.json после чтения/reload.
  Legacy Failed без failed_at не получает дату или ложное исчерпание. **1 passed**.
- Новый client test читает legacy и additive JSON. Два native render tests проверяют
  phase/attempts/reason/date, цвета, exhausted и legacy/unknown date.
- Два новых browser cases прошли: blocked → retry_wait → stalled → offline → pending,
  сохранение срока/порядка/последних строк, отсутствие POST, unknown date и legacy.
  Новый `queue-retry-plan-dark-win32.png` создан отдельным целевым запуском и просмотрен;
  прежние эталоны не обновлялись.

## Проверки

| Команда | Результат |
|---|---|
| `cargo test --locked -j 2 -p localvox-light-core --lib workflow:: -- --test-threads=2` | PASS: 11/11 |
| `npm --prefix ui run build` | PASS: TypeScript и production bundle |
| `npm --prefix ui test` | PASS: 9/9 |
| `npm --prefix ui run lint` | PASS: прежние 8 warnings, новых нет |
| `cargo test --locked -j 2 -p localvox-light-core --lib jobs:: -- --test-threads=2` | PASS: 55/55, включая phase/restart recovery, bounded revival и сохранение Failed при reload |
| `cargo test --locked -j 2 -p localvox-light-core --lib progress:: -- --test-threads=2` | PASS: 10 passed, 1 ignored |
| `cargo test --locked -j 2 -p localvox-light-api -p localvox-light-client -p localvox-light-terminal -- --test-threads=2` | PASS: API 64 lib + 3 integration, 6 ignored; client 3 lib + 8 transport; TUI 96 lib + 6 integration, 3 ignored. Старые snapshots сохранены, INSTA_UPDATE=no |
| `npm --prefix ui run test:e2e -- --workers=2` | PASS: 68/68, 46.4 s; прежние PNG неизменны, новый эталон просмотрен, три намеренных визуальных повреждения дают ожидаемый отказ |
| `cargo test --locked -j 2 -p localvox-light-api --lib -- --ignored --exact http::tests::cross_clients::shared_workspace --nocapture` последовательно после e2e | PASS: 1/1, 25.99 s, 69 отфильтрованы; новый UI и TUI с настоящим временным API |
| `openspec validate --specs --strict --no-interactive` | PASS: 15/15; прежние INFO о длине требований |
| `openspec validate explain-automatic-retry-state --type change --strict --no-interactive` | PASS: CLI 1.13.2 |
| `python scripts/check-openspec.py` | PASS: 15 capability, 86 main требований, 168 сценариев, 86 main coverage rows |
| `cargo fmt -p localvox-light-api -p localvox-light-client -p localvox-light-terminal -- --check`, `rustfmt --edition 2021 --config skip_children=true --check crates/localvox-light-core/src/workflow.rs`, `git diff --check` и новые текстовые файлы | PASS |

Первый shared-прогон запущен параллельно с полным Playwright и завершился timeout
61.13 s; он не объявляется успешным. Каталог cross-real-api во время этого запуска
исчез. Общий helper пишет в ui/test-results/cross-real-api, а установленный
Playwright createRemoveOutputDirsTask очищает outputDir (lib/runner/index.js),
поэтому эти два прогона нельзя считать независимыми. Последовательный повтор
сохранил timeline/terminal/browser artifacts и прошёл без изменения продукта,
fixtures или лимитов 60/45/15 секунд. Логи: target/retry-state-shared.log и
retry-state-shared-final.log. В CI команды уже идут последовательно.

Остальные ignored media/historical/helpers не объявляются заново выполненными;
общий сценарий явно запущен отдельной командой. Новый эталон создавался только
целевым browser-тестом, обычный полный запуск не обновлял baseline. В interface CI
добавлена строгая проверка change, существующие команды включают новые cases.
Удалённый CI не запускался. Delta сохраняет все сценарии предыдущего уточнения
PIPE-011 из fix-queue-read-failures.

## Границы

Не менялись число попыток, автоматические интервалы, jobs.json и действия над
заданиями. Время «не раньше» не обещает запуск ровно в этот момент; доступность
внешней модели не доказывается heartbeat координатора. Выполненные restart/recovery
проверки jobs — программные сценарии, не аппаратный перезапуск пользователя.
BL-002/003/004 и физическая приёмка 10.2 остаются открытыми. Main specs не
синхронизируются, changes не архивируются, коммиты выполняет пользователь.
