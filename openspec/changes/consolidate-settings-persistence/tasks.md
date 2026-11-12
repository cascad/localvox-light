# Tasks

- [x] 1.1 Свести чтение/snapshot verification/no-op/atomic publish в core/env_file, сохранить save/save_to и редактор; проверить read/replace failures, stale/missing snapshots, concurrent independent edits и no-op bytes/mtime.
- [x] 1.2 Перевести setup на общий snapshot writer, сохранить confirmation/ownership и recheck после долгой работы; settings HTTP и setup tests подтверждают сохранность/restart/secrets/cancel/retry.
- [x] 1.3 Прогнать relevant Rust tests, release build, strict main/change/check-openspec/scoped rustfmt/diff; обновить evidence CFG-001/002/003/SETUP-006, коммит без push; реальные настройки не трогать.
