# Tasks

## 1. Command

- [x] 1.1 Общий native entry и binary vox с portable settings/cwd, существующим daemon bootstrap; проверить CLI/portable ConPTY и отсутствие модели/capture в клиенте.
- [x] 1.2 Реализовать read-only и идемпотентную PATH registration, конфликт чужой vox, Windows isolated registry tests и Unix temp-profile/shell tests; обновить docs/US.

## 2. Delivery

- [x] 2.1 Включить vox в package/installers и default PATH registration с opt-out; actual Windows package/install/check/reconcile и portable delivery fixtures, без глобальных изменений тестами.
- [x] 2.2 Выполнить relevant TUI/launcher/delivery/spec checks, обновить evidence/coverage и commit; native Mac/host UI оставить not-run. Зафиксировать обсуждение полного выхода отдельным proposal, не выдавать его за готовую функцию.

## 3. Регрессия запуска после установки, 2026-11-21

- [x] 3.1 Воспроизвести пустой LOCALVOX_API_TOKEN из portable presets и исправить единое определение настроенного токена для HTTP/daemon/standalone API с сохранением loopback/Host/Origin и защиты LAN; проверить настоящий HTTP и авторизованный доступ без capture. Red→green, 43 HTTP tests и оптимизированный настоящий API пройдены. [Результаты](evidence/empty-token-startup.md).
- [x] 3.2 Сохранить конкретный отказ HTTP/совместимости при ожидании запущенного демона вместо ложного startup timeout; проверить launcher/native TUI, обновить справочник/evidence и закоммитить. Рабочий пользовательский демон и архив не перезапускать ради проверок. Launcher 6 passed / 1 helper ignored, настоящий ConPTY passed; по прямому поручению владельца после его выхода через трей обновлены бинарники F:\lv с backup и проверкой комплекта. [Границы](evidence/empty-token-startup.md).

## 4. Навигация с первого кадра, 2026-11-21

- [x] 4.1 Сделать меню разделов стартовой активной областью и сохранять выбранный раздел/фокус при подключении и первом списке; подтвердить управление до health, при загрузке и после пустого/непустого ответа настоящим ConPTY, обновить справочник/evidence и существующую установку без остановки демона. 215 library tests passed / 3 helpers ignored, 9 ConPTY tests passed; клиенты F:\lv обновлены, bundle-check пройден. [Результаты](evidence/startup-navigation.md).
