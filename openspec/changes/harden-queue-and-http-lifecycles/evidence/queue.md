# Queue transitions · 2026-11-12

## Reproduction

Существующий fixture `deny_queue_target` направляет публикацию в каталог:
настоящая atomic rename возвращает I/O error. До исправления два новых tests
падали: `requeue_stale` возвращал false, но менял flags/phase/attempts в памяти;
`mark_done` оставлял Done/finished_at/force=false, хотя файл оставался Running.
Raw log: ignored `target/tmp/lifecycle-queue-before.log` (2 failed / 8 passed
по фильтру legacy). Это отдельное воспроизведение, не приёмка.

## Implementation / code-reviewed

- `update_job` использует прежний `commit_jobs`; rollback копирует только один
  изменяемый Job. Eligible states/reset policies методов оставлены явными;
  requeue/retry/reclaim/mark_done/clear_force используют один путь публикации.
- `retain_jobs` восстанавливает удалённые задания на исходных позициях при
  отказе. forget/drop_orphans не сохраняют удаление только в памяти и не сообщают
  число удалённых до сохранения. Public signatures и void wrappers сохранены.
- New forced recook задаёт force до enqueue_new/save; второй промежуточный save
  удалён. Next_id/jobs откатываются при ошибке; kind/phase/follow_settings/flags
  после успешного enqueue совпадают с прежним конечным состоянием.
- Refresh post-processing использует undo только изменяемых jobs; не клонирует
  целиком неизменённую history на каждом проходе. Порядок Text→Summary→Done при
  отключении флагов, timing и attempt/revival policies сохранены.
- JSON/schema, обработка cancellation, execution ownership и приёмка артефактов
  не менялись. Это исполнение прежних PIPE-002/008/010/011, не новая state machine.

## Test-run

`TEMP/TMP=target/tmp` на D:, `CARGO_TARGET_DIR=target/modular-build`, jobs=2,
RUST_TEST_THREADS=2. Модели/пользовательский archive/daemon не использовались.

- Queue tests: **78 passed / 0 failed** (`cargo test --release --locked --offline
  --jobs 2 -p localvox-light-core --lib jobs::`).
- Полный core без ONNX feature: **440 passed / 0 failed / 8 ignored**.
- App lib/coordinator: **16 passed / 0 failed / 11 ignored**; штатные parents
  запускают свои subprocess fixtures. Helper/device ignored не выданы за passed.
- Новые cases: 6 bool transitions, 4 void/removal transitions, полный JSON до/после
  ошибки и успешного retry, несколько непоследовательных удалений с восстановлением
  порядка/next_id, forced new job с полным request после recovery.
- Существующие cases: cancellation/retry/legacy fields, failed phase commit,
  artifact verification, timing budgets, pending/live/manual policy, очередь при
  storage contention и coordinator outcome/startup recovery.

HTTP RAII — следующий этап этого change. Hardware/Mac/ONNX/native ASR не проверялись
этим прогоном; новый performance gate не заявлен. Нет общей multi-file транзакции
архива и очереди; существующие void wrappers не стали новым error API.
