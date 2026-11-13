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

## Hosted follow-up 99bd709

В run 43477210896 найден тот же дефект времени во втором точном сценарии,
`voice grouping stays active...`: ожидание 0:05 пропущено, первый DOM уже 0:06.
Применить то же explicit clock управление только к нему; 0:05 → 0:07 и measured
0:03, состояние running после чтения всего аудио и assertions остаются прежними.
Исходная pipeline проверка уже исправлена и в этом run passed.
