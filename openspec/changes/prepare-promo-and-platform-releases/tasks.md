# Tasks

## 1. Platform delivery

- [x] 1.1 Исправить комплектование native dependencies и переносимость пакетов; проверить package tests и настоящий Windows loader smoke, описать состав.
- [x] 1.2 Исправить Windows/macOS installers: точный архив, staging validation, local archive, сохранение `.env`, ошибки subprocess; проверить installer fixtures и установку Windows bundle во временную папку.
- [x] 1.3 Обновить Actions matrix/triggers/UI guard/package/install smoke; проверить workflow lint и scripts, оставить новый hosted run not-run до push владельцем.

## 2. Promotion and documentation

- [x] 2.1 Сделать воспроизводимую demo fixture и съёмку реальных UI/TUI; получить разные PNG/GIF, визуально проверить кадры, продолжительность и размер, описать повторную съёмку.
- [x] 2.2 Поставить короткую установку и запуск сразу после описания README, добавить медиа/галерею и Mac daemon checklist; обновить затронутые US-ID, проверить ссылки/команды.

## 3. Integration

- [x] 3.1 Обновить evidence SETUP-001/002/003 и локальную карту SETUP-004, проверить strict main/change/check-openspec/diff, закоммитить завершённые этапы без push/публикации.
- [ ] 3.2 Владелец: после push проверить новые hosted Windows/Mac jobs и release/install на Mac, daemon/UI/TUI, permissions, microphone/loopback/Stop/restart. До фактического прогона not-run.
