# Tasks

## 1. Command

- [x] 1.1 Общий native entry и binary vox с portable settings/cwd, существующим daemon bootstrap; проверить CLI/portable ConPTY и отсутствие модели/capture в клиенте.
- [x] 1.2 Реализовать read-only и идемпотентную PATH registration, конфликт чужой vox, Windows isolated registry tests и Unix temp-profile/shell tests; обновить docs/US.

## 2. Delivery

- [x] 2.1 Включить vox в package/installers и default PATH registration с opt-out; actual Windows package/install/check/reconcile и portable delivery fixtures, без глобальных изменений тестами.
- [x] 2.2 Выполнить relevant TUI/launcher/delivery/spec checks, обновить evidence/coverage и commit; native Mac/host UI оставить not-run. Зафиксировать обсуждение полного выхода отдельным proposal, не выдавать его за готовую функцию.
