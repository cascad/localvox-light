# Сохранение переходов очереди · 2026-11-01

Уточнение PIPE-002; проверки затрагивают PIPE-008/010/011. Код реализован,
существенные сценарии выполнены на временных данных. Это не принятие всех
сценариев capability или аппаратная приёмка.

## Воспроизведение

До исправления три новых теста в [jobs.rs](../../../../crates/localvox-light-core/src/jobs.rs)
завершились отказом. Несохранённый захват оставлял Pending → Running и attempts
2 → 3 в памяти; revival сообщал 2 вместо 0, recovery — 1 вместо 0. Исходный
прогон сохранён в `target/queue-commit-red.log`; исторический результат не заменён
последующим успешным прогоном.

## Реализация

- `try_start/try_start_phase`, `try_reclaim_abandoned`, `try_revive_failed`
  возвращают Result и откатывают изменённые Job при отказе save. Совместимые
  методы возвращают None/0; только сохранённый переход даёт задание или count.
  `next_id`, формат файла и успешное поведение сохранены. При load_error checked
  методы отказывают до изменений и не перезаписывают повреждённый файл.
- Recovery сохраняет очередь перед дополнением progress. Завершённые события
  и прежняя история сохраняются. Запись очереди и progress не являются общей
  crash-транзакцией: диагностическое дополнение после принятого recovery остаётся
  best effort.
- В [autocook.rs](../../../../crates/localvox-light/src/autocook.rs) startup
  повторяет checked recovery с кооперативной паузой и состоянием blocked.
  Линии создаются после успеха. Maintenance использует checked revival,
  сообщает об ошибке и повторяет проверку без ложного count/успешного лога.
  Claim сохраняет прежний Option API; None не передаёт задание worker.

## Существенные проверки

Четыре новых core-теста проверяют отказ и успешный повтор на том же объекте:
тождество снимка QueueFile и исходных байтов, бюджет/идентичность захвата,
rollback группы из двух revival, прежнюю фазу/force/attempts после recovery,
неизменный progress при отказе и сохранение префикса истории после успеха.
Отдельно проверены checked методы на повреждённом jobs.json.

Два новых Windows-теста вызывают настоящий startup helper на временной очереди.
Read handle с `share_mode(1)` запрещает замену jobs.json: helper сообщает blocked,
не возвращает успех, не меняет файл и не создаёт recovery progress. После release
сохраняется Pending Summary с прежней attempts=1; при stop helper возвращает false
и сохраняет исходные байты. Барьер перед созданием линий проверен по коду;
это не запуск всего демона с настоящими моделями/worker под файловым отказом.

## Проверки

Во всех текущих тестовых процессах TEMP/TMP направлены в
`D:\rust_repo\localvox-light\target\test-temp`; глобальное окружение не менялось.

| Команда | Результат |
|---|---|
| `cargo test --locked -j 2 -p localvox-light-core --lib jobs:: -- --test-threads=2` | PASS: 59/59 |
| `cargo test --locked -j 2 -p localvox-light-core --lib workflow:: -- --test-threads=2` | PASS: 11/11 |
| `cargo test --locked -j 2 -p localvox-light-core --lib progress:: -- --test-threads=2` | PASS: 10 passed, 1 ignored |
| `cargo test --locked -j 2 -p localvox-light --bin localvox-light -- --test-threads=2` | PASS: 8 passed, 1 ignored; включает оба Windows startup-теста |
| `cargo test --locked -j 2 -p localvox-light-api` | PASS: 64 lib + 3 integration; 6 ignored |
| `npm --prefix ui run build`, `npm --prefix ui test`, `npm --prefix ui run lint` | PASS: bundle, 9/9 unit; прежние 8 lint warnings |
| `npm --prefix ui run test:e2e -- --workers=2` | PASS: 68/68, 25.8 s; прежние PNG не обновлялись, три намеренных визуальных повреждения дают ожидаемый отказ |
| `cargo test --locked -j 2 -p localvox-light-api --lib -- --ignored --exact http::tests::cross_clients::shared_workspace --nocapture` | PASS: 1/1, 4.88 s, 69 отфильтрованы; общий временный API/UI/TUI после полного browser-прогона |
| `openspec validate --specs --strict --no-interactive` (CLI 1.13.2) | PASS: 15/15; прежние INFO о длине требований |
| `openspec validate commit-queue-execution-transitions --type change --strict --no-interactive` | PASS |
| `python scripts/check-openspec.py` | PASS: 15 capability, 86 main требований, 168 сценариев, 86 main coverage rows |
| `rustfmt --edition 2021 --config skip_children=true --check crates/localvox-light-core/src/jobs.rs crates/localvox-light/src/autocook.rs`, `git diff --check` | PASS |

Логи: `target/queue-commit-{jobs,workflow,progress,main,api,shared}.log`,
`target/queue-commit-ui-{build,test,lint,e2e}.log`.

В interface CI добавлена строгая проверка change; существующие core jobs и main
test commands включают новые fault tests. Browser и общий API/UI/TUI выполняются
последовательно, чтобы не удалять общий outputDir во время работы второго теста.

## Границы

Этот срез проверяет claim, startup recovery и automatic revival. Остальные
мутации JobQueue, orphan cleanup и общий монитор ошибок записи не подвергались
полному аудиту. Политика retry 1/2/4 часа, UI/TUI payload и успешные действия
не менялись. Игнорируемые media/historical/profiling проверки не объявляются
выполненными. Пользовательский демон, модели и архив не трогались, удалённый CI
не запускался. BL-002/003/004 и физическая приёмка 10.2 остаются открытыми.
Main specs не синхронизируются, changes не архивируются, коммиты делает пользователь.
