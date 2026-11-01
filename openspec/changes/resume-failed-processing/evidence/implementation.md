# Возобновление упавшего этапа · 2026-11-01

Продолжение проверки ручного повтора по поручению владельца, PIPE-010/011/012,
DEC-003. Пользовательский архив, работающий демон, .env, модели и устройства не
изменялись. TEMP/TMP тестовых процессов направлены в target/test-temp на D:.

## Исходное расхождение

Новый browser case `queue retry resumes its failed phase without requesting a
full recook` выполнен до исправления: **FAIL**, фактический POST
`/api/sessions/20261031_demo/recook`, ожидался `/retry` (target/resume-red.log).
Чтение QueuePane/Archive подтвердило отсутствие scope и выбор All, проверку
обязательного audio, Prepare/force и инвалидирование производных. Таким образом
повтор summary был полным повтором, а Prepare ingest без audio отклонялся.

## Реализация и существенные проверки

- [JobQueue](../../../../crates/localvox-light-core/src/jobs.rs): retry_failed
  возвращает только Failed в Pending той же фазы/id/kind с прежними параметрами.
  Новые часы очереди, attempts/revivals=0, force=false, прежние execution/error/
  started/finished сброшены. Не-Failed и missing не меняются. Save failure
  восстанавливает snapshot в памяти. Два новых теста покрывают четыре фазы,
  ingest/cook, follow_settings/явные запросы, fresh budget, duplicate/Running/Done
  и ошибку сохранения. **2 passed**, полный jobs namespace **55 passed**.
- [Archive](../../../../crates/localvox-light-api/src/archive.rs) и [HTTP](../../../../crates/localvox-light-api/src/http.rs):
  отдельный POST /api/sessions/{name}/retry под session lease и mutex, проверка
  свежей очереди/Failed, затем сохранение и новая граница progress. Старые строки
  истории не удаляются. Recook и его scope/default остаются прежними.
- [Real HTTP tests](../../../../crates/localvox-light-api/src/http/tests/workspace.rs):
  Summary без audio сохраняет байты двух версий транскрипта, versions/meta,
  processed/summary и настоящих processing receipts с исходной v2. Проверены
  Pending Summary, queued progress, сохранение префикса истории/новая граница,
  duplicate POST без изменения очереди/истории. Ingest Prepare возобновляется
  через настоящий [TUI IO](../../../../crates/localvox-light-terminal/src/io.rs)
  и Rust client, исходная ссылка неизменна. Второй тест проверяет held lease,
  recording, Pending/Running/Done, broken queue и Windows deny-replace jobs.json;
  отказ не создаёт progress и не удаляет файлы. **2 passed** после исправления
  setup receipts (исходная версия указывается явно, ошибки record проверяются).
- [UI](../../../../ui/src/Queue.tsx): отдельный api.retry, блокировка по session
  до ответа, явное обновление queue/jobs после успеха, восстановление кнопки
  после ошибки. Четыре browser cases: правильный endpoint/новая строка Pending,
  удерживаемый POST/одно нажатие, 503 и 404 без перехода к full recook и без
  автоматической отправки действий. **4 passed**.
- [TUI](../../../../crates/localvox-light-terminal/src/app/commands.rs): R вне
  форм и именованная команда палитры; выбранная Failed-строка очереди или текущая
  архивная сессия. Pending Action блокирует дубли, ошибка не отправляет recook.
  В форме R остаётся текстом. Отдельная pending_retry отметка меняет epoch после
  acknowledgement, ограждает старый progress и сохраняет документы. Control reply
  не забирает её, stop остаётся независимым. Три новых App tests входят в полный
  native прогон.

## Проверки

| Команда | Результат |
|---|---|
| `cargo test --locked -j 2 -p localvox-light-core --lib jobs:: -- --test-threads=2` | PASS: 55/55, 4.18 s |
| `cargo test --locked -j 2 -p localvox-light-core --lib workflow:: -- --test-threads=2` | PASS: 8/8 |
| `cargo test --locked -j 2 -p localvox-light-core --lib progress:: -- --test-threads=2` | PASS: 10 passed, 1 ignored |
| `cargo test --locked -j 2 -p localvox-light-api -p localvox-light-client -p localvox-light-terminal -- --test-threads=2` | PASS: API 63 lib + 3 integration, 6 ignored; client 2 lib + 8 integration; TUI 94 lib + 6 integration, 3 ignored. Старые символы/стили/snapshots сохранены, INSTA_UPDATE=no |
| `npm --prefix ui run build` | PASS: TypeScript и production bundle |
| `npm --prefix ui test` | PASS: 9/9 |
| `npm --prefix ui run lint` | PASS: прежние 8 warnings, новых нет |
| `npm --prefix ui run test:e2e -- --workers=2` итоговый | PASS: 66/66, 28.7 s; прежние PNG не изменены, три намеренных визуальных повреждения дают ожидаемый отказ |
| `cargo test --locked -j 2 -p localvox-light-api --lib -- --ignored --exact http::tests::cross_clients::shared_workspace --nocapture` | PASS: 1/1, 5.52 s; свежие UI/TUI с настоящим временным API, 68 tests отфильтрованы |
| `openspec validate --specs --strict --no-interactive` | PASS: main specs 15/15, прежние INFO о длине требований |
| `openspec validate resume-failed-processing --type change --strict --no-interactive` | PASS: CLI 1.13.2 |
| `python scripts/check-openspec.py` | PASS: 15 capability, 86 main требований, 168 сценариев, 86 main coverage rows |
| `cargo fmt -p localvox-light-api -p localvox-light-client -p localvox-light-terminal -- --check` | PASS |
| `rustfmt --edition 2021 --config skip_children=true --check crates/localvox-light-core/src/jobs.rs` | PASS: изменённый core файл без обхода остальных модулей |
| `git diff --check`, новые текстовые файлы | PASS |

Первый полный browser-прогон дал 65 passed / 1 failed: ожидание начального
heading в workspace setup теста notes, до выполнения его действий. Причина
единичного timeout не установлена; целевая проверка неизменённого notes case
прошла (1/1). Результат первого прогона не объявляется успешным, лимиты ожидания
и эталоны ради него не менялись. Логи: target/resume-e2e.log и resume-notes-recheck.log.

Итоговый полный browser-прогон после native проверок прошёл без изменения notes
case. API/TUI ignored включают собственные helpers и отдельные media/historical
сценарии; они не объявляются заново выполненными package-прогоном. Общий сценарий
явно выполнен отдельной командой выше. Один ignored в progress не объявляется
отдельно выполненным этим прогоном.

В существующий interface CI добавлена строгая валидация change. Новые cases входят
в штатные jobs/workspace/client/TUI/browser команды. Удалённый CI не запускался.
Документация и coverage связаны с PIPE-010/011/012; кроме добавленных сценариев
возобновления прежние обязанности PIPE-010 сохранены полностью, известные gaps
не переименованы в выполненные задачи.

## Границы

Новые запросы управляют очередью; speech/LLM в тестах не запускаются. Это не
benchmark двухчасовой записи и не обещание exactly-once внешней модели. Сброс
force позволяет переиспользовать подтверждённый результат упавшей фазы; следующее
автоматическое действие продолжает учитывать live summary policy. Общая
crash-транзакция queue/progress не заявляется. BL-002/003/004 и физическая приёмка
10.2 общего журнала остаются открытыми. Main specs не синхронизируются, changes
не архивируются, коммиты выполняет пользователь.
