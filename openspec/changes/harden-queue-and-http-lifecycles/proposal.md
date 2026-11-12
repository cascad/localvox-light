# Proposal

## Why

После общего модульного рефакторинга остались два конкретных риска ручного
bookkeeping: несколько queue transitions не откатывают память при отказе записи,
а HTTP inflight освобождается только после нормального возврата обработчика.
Новая запись на forced recook ещё и сохраняется дважды. Эти различия влияют на
надёжность очереди и доступность UI/TUI, а не только на удобство чтения кода.

## What Changes

- Общий checked update одного Job поверх существующего commit_jobs; сохранить
  индивидуальные правила recook/retry/stale/artifact repair и прежние wrappers.
- Подтверждать удаление jobs только при сохранённой очереди; force нового recook
  задавать до единственной публикации, с прежним rollback jobs/next_id.
- Передавать HTTP inflight slot вместе с обработчиком как RAII guard; отказ
  spawn, normal return и unwind освобождают slot. Сохранить 32/4 и 503/body/auth.
- Проверить storage failures/retry/JSON compatibility и настоящий HTTP overload/
  recovery; без ASR/LLM inference и записи в пользовательские данные.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

Нет. `skip_specs: true`: bug fixes и внутренний refactor исполнения PIPE-002/008/
010/011 и API-003, без новых действий, форматов, лимитов или политик повторов.

## Impact

core/jobs, HTTP request budget, соответствующие fault/integration tests, evidence.
Два ограниченных этапа с проверенными коммитами. BL-006, streaming decode,
массовое дробление файлов и новые функции не входят в этот срез.
