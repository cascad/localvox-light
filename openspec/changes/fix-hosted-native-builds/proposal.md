# Proposal

## Why

После push 371975d hosted Windows/ARM Linux bundles и Unicode tests прошли.
Run 43245450999 обнаружил несовместимый glibc у Linux x64 и отсутствие ORT
prebuilt для Intel Mac; 43245450996 — ошибочное сравнение short/long cwd в fixture.

## What Changes

- Проверять восстановление фактически установленного cwd, покрыть нормализуемый
  путь и отрицательный контроль, сохранив строгие проверки запуска.
- Согласовать Linux x64 runner с требуемой glibc; явно описать Ubuntu 24.04 baseline.
- Intel Mac: собрать тот же CPU ONNX Runtime 1.24.2 из pinned official source,
  кэшировать законченный runtime, link/bundle/relocate/sign его с приложением.
  Не использовать чужие неофициальные бинарники или отключение ASR.
- Проверить helpers/packaging/smoke локально и оставить native Mac/новый hosted
  run not-run до фактического запуска. Слияние main уже выполнил и push сделал владелец.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

Нет. `skip_specs: true`: исправления доставки/проверок SETUP-002/003, без новых
пользовательских действий, моделей, ASR API, настроек или изменения контракта.

## Impact

Workflow, изолированный runtime build helper, packaging tests и launch fixture,
docs/evidence. Runtime version не обновляется и не понижается. Source build
исполняется на Intel Mac runner; большие native сборки на Windows не нужны.
Новый commit локально, без push; прежние результаты сохраняются как история.
