# Tasks

## 1. Изолированное измерение правки

- [x] 1.1 Добавить text-only manifest/profiler с отдельными refine/cleanup/chain/summary, свежими копиями JSONL, fingerprints, локальным native trace, timeout и проверкой артефактов; проверить malformed input, изоляцию env, no-overwrite, ошибочный worker и численную отчётность model-free tests, описать запуск.

## 2. Нативный reference и выводы

- [x] 2.1 Собрать release profiler/worker и выполнить последовательные ограниченные прогоны четырёх путей на публичном транскрипте BL-003; сохранить wall, calls/tokens/load/prompt/generation/checks, edits и исходы, явно отделить partial/timeout от полного результата; hashes источника должны сохраниться.
- [x] 2.2 По результатам обновить evidence PIPE-005/006/007/011 и LLM-003/004, описать подтверждённое узкое место и следующий шаг без необоснованных обещаний; strict main/change, check-openspec, форматирование и diff должны пройти.
