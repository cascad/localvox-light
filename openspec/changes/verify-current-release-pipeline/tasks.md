# Tasks

## 1. Подготовить согласованную сборку

- [x] 1.1 Собрать UI и пройти существующие unit/browser e2e с одним worker; зафиксировать итог и fingerprints dist, не обновлять snapshots автоматически.
- [x] 1.2 Собрать release daemon/worker/TUI/desktop/profile/queue-profile, пройти client/terminal/process/coordinator/profiler/core/LLM и explicit release queue failure fixture; сохранить build/test logs и binary hashes, не запускать пользовательский daemon.

## 2. Проверить реальные модели

- [x] 2.1 На прежнем публичном PCM выполнить fresh STT-only и STT+summary через production coordinator с текущими GigaAM/GLiNER/Ollama qwen3.5:9b; проверить receipts, переходы/отсутствие лишних стадий, clocks/operation trace, complete и SHA, сохранить numeric evidence с границами качества/ресурсов.
- [x] 2.2 Обновить coverage PIPE-004/007/008/009/010/011, LLM-004/005, catalog и готовность свежей сборки; strict main/change, check-openspec и diff проходят; закоммитить только проверенный этап, ручную аппаратную приёмку оставить not-run.
