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
- После hosted 873ffa2 и native reproduction: исправить общий Windows audio host,
  чтобы CPAL не создавал process-wide enumerator в заканчивающемся STA-потоке.
  Это исправление нарушения существующих CFG-003/ARCH-001, не новое поведение.
- Проверить затронутые тесты и оставшиеся interface шаги, записать границы
  локальной и hosted проверки отдельно.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

Нет новых или изменённых требований: `skip_specs: true`. Контракты CFG-003,
ARCH-001 и API-003 сохраняются. Исправление native crash восстанавливает
уже требуемую доступность настроек и архива; новых пользовательских действий нет.

## Impact

Shared-client fixture, общий Windows COM/audio host, процессная диагностика тестов,
interface workflow и evidence. CPAL/модели не обновляются.
Модели, пользовательский архив, настройки и работающий демон не затрагиваются.
