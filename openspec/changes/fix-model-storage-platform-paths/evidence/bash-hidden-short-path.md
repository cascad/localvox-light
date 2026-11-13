# Bash Hidden and short HOME · 2026-11-13

## Hosted 99bd709

[Windows job](https://github.com/cascad/localvox-light/actions/runs/43477210846/job/132064029558)
собрал binaries/desktop и прошёл все delivery checks кроме Bash default Hidden.
Прежние PS Hidden и native curl Unicode failures исправлены; текущее падение —
единственный assert `store.stat().st_file_attributes & 2 == 0` в Bash subtest.

Linux x64/ARM64 jobs этого build **success**. macOS Intel прошёл Delivery contracts
и перешёл к Native vox registration (полный job на момент наблюдения ещё running).
[macOS ARM64](https://github.com/cascad/localvox-light/actions/runs/43477210846/job/132064030263)
**cancelled** до получения runner: GitHub annotation сообщает, что hosted runner
не выделен после нескольких попыток и есть capacity constraints на macOS arm64.
Это не failure тестов и не fail-fast: matrix явно содержит `fail-fast: false`.
Раннее предположение о fail-fast в coverage было неверным; оно уточнено без
изменения фактического результата cancelled.

## Root and red → green

Zip guard преобразует destination к physical path. `new_model_parent` сравнивал
его с USER_MODEL_ROOT в короткой Windows/иной эквивалентной форме. Из-за ложного
prefix check root создавался обычным mkdir без Hidden.

- Direct parent regression с equivalent `home/../home` на 99bd709: **failed**,
  Hidden=0 (`target/tmp/bash-hidden-parent-red.log`).
- Настоящий `GetShortPathNameW` probe: отдельный маленький временный HOME на томе
  с включёнными 8.3 aliases, настоящий Bash fetch tiny localhost Vosk ZIP.
  На 99bd709: `SHORT_HOME_DIFFERENT True`, `HIDDEN_BEFORE_FIX False`.
- Fix переиспользует `physical_model_path` для обеих сторон prefix check;
  затем создаёт только отсутствующий стандартный root и ставит Hidden.
  Тот же реальный short-name probe: `HIDDEN_AFTER_FIX True`.
- Tiny probe HOME на C: удалён контекстом TemporaryDirectory; проверка
  `lv-short-path-probe-*` не нашла оставшихся папок. Script/HTTP payloads/logs
  и обычный комплект/fixtures — D:. Настоящие веса/архив/демон не трогались.

## Verification

Свежий Windows bundle перепакован с новым helper, package smoke и installed
doctor/setup-check/cancel/no-op/reconcile **passed**. Model-storage suite:
**10 passed / 2 skipped** (12 tests, 80.147 s), включая native JSON bridge,
настоящий System32 curl, Check/repeat, root aliases, NTFS junction security,
ZIP и file downloads, сохранение existing/custom attributes.

Два skips: native Unix symlink integration на Windows и short-name test на D:,
где 8.3 aliases выключены. Отдельный реальный short-name probe на подходящем томе
выше выполнен, не выдан за skip/pass основной fixture. HOME junction end-to-end
scenario прошёл до и после fix; именно direct parent + native short HOME дали red.

Логи: `target/tmp/bash-hidden-{parent-red,short-red,short-green,green,package,install,models}.log`.
Runtime/форматы/контракт и user-guide прежние. Новые hosted checks этого helper
и исправленного voice-grouping clock — **not-run** до push владельца.
