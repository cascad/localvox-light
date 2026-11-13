# Proposal

## Why

На `d30727b` delivery снова прошёл на пяти платформах, но Interface run
`43429701720` остановился раньше shared-client теста: 80/81 browser tests passed.
PIPE-011 тест ожидал ровно 5 с при продолжающих идти часах. Artifact показывает
12 с ожидания; скорость загрузки runner меняет результат проверки.

## What Changes

- Зафиксировать часы только в точном pipeline timing сценарии, оставив управление
  `runFor`/`fastForward`, все assertions и существующие PNG baseline.
- Воспроизвести сбой с контролируемой задержкой до открытия пайплайна,
  проверить тот же сценарий после fix, затем выполнить полную UI suite.
- Обновить фактический hosted статус; shared-client причина остаётся открытой,
  поскольку этот шаг не выполнялся в новом run.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

Нет. PIPE-011 сохраняется, `skip_specs: true`: исправляется тестовое время,
пользовательское поведение и production код не меняются.

## Impact

UI e2e fixture/clock scenario, workflow validation, evidence.
Настройки, модели, пользовательский архив и работающий демон не затрагиваются.
