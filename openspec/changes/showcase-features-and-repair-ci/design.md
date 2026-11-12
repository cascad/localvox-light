# Design

## Context

См. proposal. Есть 11 PNG + 4 GIF в docs/media с manifest; кадры настоящих
интерфейсов на демонстрационном архиве. Последний release v0.1.5 неполный,
новые workflows не дошли до сборки Rust из-за npm ci. Push владельца нужен,
чтобы GitHub исполнил изменённый workflow; публикацию локальные tests не заменяют.

## Goals / Non-Goals

Понятный README и воспроизводимая CI проверка setup. Не добавлять функции,
AI mockup картинки, личные встречи, новый рекламный пост или молчаливые test skips.

## Decisions

- Сначала существующий GUI GIF, затем краткая идея и установка из актуального
  Actions bundle. Новый полный release installer остаётся отдельной инструкцией,
  без неработающего latest-download как главного quick start.
- Feature sections сопоставляются user-guide и accepted specs/deltas. Медиа там,
  где показывают взаимодействие; не выдавать GIF list filter за semantic search
  или synthetic встречи за пример качества ASR.
- Developer build details в docs/development.md; исправить относительные ссылки.
  promo/README — указатель на originals/gallery, без дублирования image blobs.
- npm lock исправить через npm, не обходить ci командой install в workflow.
  Multi-command PowerShell blocks останавливаются на native error; Bash блоки
  используют fail-fast. Проверить ранний nonzero настоящим subprocess test.
- Native ConPTY failure исследовать по стандартным KEY_EVENT и regression:
  не удалять emoji/строгое сравнение, не исключать host из CI. Поддержка committed
  Alt+Numpad text на key-up не должна включать обычные release events как команды.
- Один reusable Python delivery smoke для установленного bundle на Windows/Mac/
  Linux: expected doctor status + JSON IDs, check, reconcile и snapshot hashes/mtime.
  Без скачивания весов и вызова LLM inference; только свой temporary install/env.

## Risks / Trade-offs

- README promise drift → сверка с user-guide/contracts и просмотр PNG/GIF metadata.
- Optional peer dependencies lock drift → чистый npm ci, inspect diff без update
  всех пакетов; проверка source build/unit/browser.
- CI может остановиться на следующем ранее скрытом дефекте → локально выполнить
  существенные workflows/script suites, hosted outcome оставить not-run до push.
- Unicode fix может дублировать key-up → targeted records, actual ConPTY и
  unchanged TUI snapshots; добавить native diagnostics к failing fixture.

## Migration Plan

Docs/media review и CI repro → ограниченные fixes/tests → README preview и clean
install smoke → evidence/commit без push. Публикация Telegram отдельным запросом.
