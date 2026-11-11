# Tasks

## 1. Управляемая обработка

- [x] 1.1 Устранить повторные disk lookups одного cache batch в checkpoint/publication без process-wide trust; прогнать corruption/commit/recovery core tests и зафиксировать verification границы.
- [x] 1.2 Добавить reusable core AsrRuntime и bounded owned stdio session/server с protocol/id, deadline/cancel/kill/reap; проверить two requests/engine reuse/identity replacement, malformed/oversize/EOF/hang/error и fixture final parity.
- [x] 1.3 Подключить resident к Transcribe lane, prefix/final/force, keep-alive default/settings/0/idle и shutdown; real coordinator/worker tests проверяют fairness, lease release, cancel/retry/protocol, обновить затронутые US-ID и архитектуру.

## 2. Доказательства и поставка

- [x] 2.1 Обновить profiler для resident и выполнить three-pair whole native reference с прежним PCM/weights/recipe/gate, window/text/calls parity и CPU/I/O/RAM/load/startup/Stop timings; отдельно сообщить исход gate, не менять default раннего режима и не выдавать аппаратную приёмку за пройденную.
- [x] 2.2 Выполнить затронутые process/core/worker/coordinator/API regressions, strict main/change/check-openspec/diff; собрать согласованный release daemon/worker/UI/TUI, сохранить evidence/coverage и scoped commit без запуска личного архива. Hardware/Mac not-run.
