# Tasks

## 1. Runner prerequisites

- [x] 1.1 Добавить ранний Python/tool/version preflight, отдельный shell step и достоверный blocked/not-run; проверить runner contracts для Python 3.10, missing shells и старого установленного клиента, обновить US-067/инструкцию.

## 2. Archive и изолированные fixtures

- [x] 2.1 Исправить ARCH-001 для обеих форм разделителей/prefix и нормализуемых путей; выполнить архивные регрессы с корректными Unicode IDs, связать delta/evidence/coverage.
- [x] 2.2 Канонизировать ownership проверок copied settings/cross-client fixtures, сохранять отказ child в диагностике; выполнить настоящий settings HTTP и shared UI/TUI tests локально, Mac повтор оставить отдельным not-run.
- [x] 2.3 Изолировать model root installation fixture, сохранив проверку reuse; выполнить install tests и регресс с существующим альтернативным model root.

## 3. Native gates и итог

- [x] 3.1 Включить archive/settings API regressions в существующие macOS build jobs и проверить delivery workflow contracts.
- [x] 3.2 Зафиксировать разбор исходного Mac отчёта, локальные проверки и точную команду повторного Mac прогона; strict specs/change, check-openspec, diff, коммит. Не выдавать Windows/статическое ревью за Mac test-run.
- [x] 3.3 По повторному Mac отчёту убрать Windows-only gate shared UI/TUI/API fixture, проверить реальное обнаружение и выполнение теста, добавить Unix CI-покрытие и записать новые результаты без переписывания прежних. Windows test-run, пользовательский Mac PASS (20261126T130946-76010) и неподтверждённый Linux прогон разделены в evidence/shared-registration-followup.md.
