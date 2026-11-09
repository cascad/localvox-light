# Tasks

## 1. Устранить ненужные role inference

- [x] 1.1 Реализовать общий collector прежних окон и индекс пересечений без смены контекста/порогов; model-free tests подтверждают Unicode/границы/coverage, точные role decisions и propagation ошибок; добавить skipped counters и документировать доказательство/API.
- [x] 1.2 Сохранить full-scan reference на общей реализации и добавить explicit native parity/численный длинный paired test, обновить CI/docs; targeted core/probe/LLM, rustfmt и release build проходят без смены настроек/рабочего worker.

## 2. Проверить эффект на прежнем материале

- [x] 2.1 Выполнить два fresh full/relevant source сравнения с обратным порядком, затем два обычных grounding profile на прежней публичной паре без LLM; подтвердить exact Entity/verdict parity, skipped/calls/cache и SHA до/после, сохранить numeric evidence и пределы ускорения.
- [x] 2.2 Обновить текущий статус/coverage LLM-004/PIPE-007/catalog, зафиксировать правило владельца об устойчивости; strict main/change, check-openspec и diff проходят, закоммитить только проверенный этап.
