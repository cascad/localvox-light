# Tasks

## 1. Длинные native прогоны

- [x] 1.1 Собрать текущие release worker/profiler; сверить source/binary/model fingerprints и выполнить Gemma → Qwen → Gemma → Qwen на неизменённой публичной STT; проверить все source bytes, версии, receipts, edits и сохранить численные результаты.
- [x] 1.2 Проверить прежние рискованные строки и заданную в design выборку новых результатов с контекстом; сохранить content-free agent-review с hashes, отдельно описать полезность, риски и пределы вывода.

## 2. Evidence и завершение

- [x] 2.1 Обновить evidence LLM-003/PIPE-006, каталог и краткий вывод о моделях без смены поведения; выполнить evaluator tests, strict main/change, check-openspec и diff; закоммитить проверенный этап.
