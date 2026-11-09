# Proposal

## Why

RV-020 исправлен в общем словарном guard, но прежние длинные native Gemma/Qwen
результаты получены до него. Новый release уже [собран и проверен](../verify-current-release-pipeline/evidence/results.md);
нужна новая оценка реальной правки, отдельно от replay сохранённых предложений.

## What Changes

- Обычный text-only profiler подключать напрямую, когда --trace-ollama не задан:
  не включать скрытый proxy `/:11434/v1`. Неизмеренные native counters оставлять null.
  Явные исторические telemetry/sparse experiments сохраняются opt-in.
- На неизменённых 72-case/24-boundary corpus выполнить по три свежих обычных
  cleanup repeats Gemma E4B и Qwen 4B. Не менять guards/prompts/ожидания ради gate.
- Два повтора каждой модели на прежней публичной 704-строчной STT; заранее
  зафиксировать review indices/RV controls и отдельно учесть новые спорные правки.
- Проверить сохранность source/version/receipts, полезность и смысловые риски;
  время/неизвестные counters отделять от качества. Не переключать модель/defaults.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

Нет. Инструментарий/измерение существующих LLM-003, PIPE-005/006; `skip_specs: true`.
Продуктовый runtime и автоматический маршрут STT + optional summary не меняются.

## Impact

Bench text profiling и его тесты/engineering docs; native evidence и coverage.
Тексты остаются в target на D:, в Git только numeric/hash/индексы/заключения.
Mac/audio/общая смысловая гарантия и внедрение новой модели не объявляются выполненными.
