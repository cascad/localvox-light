# Proposal

## Why

На commit `4b90802` все пять native delivery jobs прошли, но interface run
`43395635063` оборвался в `cross_clients::shared_workspace`. UI и TUI успели
прочитать общий архив и заметку; helper завершился без sentinel и panic-отчёта.
Нужно установить причину и восстановить существенную проверку двух клиентов.

## What Changes

- Воспроизвести shared API/browser/ConPTY сценарий в изолированном архиве.
- Исправить подтверждённую причину сбоя тестового окружения и сохранить
  диагностику завершения helper, не ослабляя сценарий и assertions.
- Проверить затронутые тесты и оставшиеся interface шаги, записать границы
  локальной и hosted проверки отдельно.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

Нет: tooling/test repair, `skip_specs: true`. Контракты ARCH-001 и API-003
сохраняются; пользовательское поведение не меняется.

## Impact

Shared-client fixture, процессная диагностика тестов, interface workflow и evidence.
Модели, пользовательский архив, настройки и работающий демон не затрагиваются.
