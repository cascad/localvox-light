# Tasks

## 1. Shared client fixture

- [x] 1.1 Воспроизвести сбой, сохранить точную причину и native/helper/browser диагностику в evidence; подтвердить её regression-проверкой.
- [x] 1.2 Исправить причину без ослабления assertions; выполнить настоящий `shared_workspace` с HTTP, Chromium и ConPTY, записать результат.
- [x] 1.3 Сохранить helper status/stdout/stderr, прежний panic hook и browser startup errors; проверить реальным сценарием и отдельным missing-browser regression тестом.

## 2. Integration checks

- [x] 2.1 Выполнить затронутые тесты, оставшиеся interface проверки, строгие OpenSpec/check-openspec/diff проверки; обновить evidence/coverage и создать локальный commit, отделив hosted результаты от локальных.
- [x] 2.2 После Windows audio fix выполнить native regression, core/API/TUI/client/process suite, настоящий shared_workspace, Windows product build/package/doctor и strict/diff проверки; сохранить новые hosted/local результаты отдельно и закоммитить этап.
- [ ] 2.3 После push владельца подтвердить весь Interface workflow и все пять delivery jobs вместе с загрузкой artifacts; исходный hosted failure закрывать только новым результатом, не локальным pass.
