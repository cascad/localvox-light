# Tasks

## 1. Shared client fixture

- [ ] 1.1 Воспроизвести сбой, сохранить точную причину и native/helper/browser диагностику в evidence; подтвердить её regression-проверкой.
- [ ] 1.2 Исправить причину без ослабления assertions; выполнить настоящий `shared_workspace` с HTTP, Chromium и ConPTY, записать результат.
- [x] 1.3 Сохранить helper status/stdout/stderr, прежний panic hook и browser startup errors; проверить реальным сценарием и отдельным missing-browser regression тестом.

## 2. Integration checks

- [x] 2.1 Выполнить затронутые тесты, оставшиеся interface проверки, строгие OpenSpec/check-openspec/diff проверки; обновить evidence/coverage и создать локальный commit, отделив hosted результаты от локальных.
