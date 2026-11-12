# Tasks

## 1. Diagnosis

- [x] 1.1 Добавить стабильные doctor component IDs и JSON/read-only setup report с отдельными CLI/API/tag состояниями Ollama; проверить unit/CLI fixtures без записей и downloads, описать границы.

## 2. Setup workflow

- [x] 2.1 Реализовать общий typed planner и терминальный выбор ASR paths/downloads, optional components, existing/pull/GGUF/custom LLM; проверить cancel/no-op/невалидные пути/выбор и план без побочных действий.
- [x] 2.2 Подключить executor с workspace ownership, manifest loaders и platform Ollama handoff, recheck/retry/no-op и сохранением только изменённых .env keys; проверить настоящими subprocess/HTTP fixtures, отсутствие повторного скачивания/import и сохранение настроек.

## 3. Installer integration

- [x] 3.1 Добавить check/wizard/reconcile в оба install scripts и package; проверить отсутствие записей в Check, reuse bundle, старые Archive/SkipModels и Windows/Mac shell fixtures; обновить README и US-ID в этой работе.

## 4. Acceptance

- [x] 4.1 Прогнать подходящие Rust/API/script checks и real temporary bundle, strict main/change/check-openspec/diff; evidence SETUP IDs и коммит без push; живую установку Ollama/моделей разработчика не менять.
- [ ] 4.2 Владелец: новая Windows/Mac установка Ollama, настоящий pull/GGUF/разрешения и долгие downloads; до фактической проверки not-run.
