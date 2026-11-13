# Platform paths follow-up · 2026-11-13

Исправление installer tooling по SETUP-001/002, INV-SETUP-001 и SETUP-008 в
[исходной delta](../../use-user-model-storage/specs/installation-models/spec.md).
Новые пользовательские действия/требования не вводились, справочник не менялся.

## Hosted fee3103

- [Interface contracts and visuals](https://github.com/cascad/localvox-light/actions/runs/43463289610)
  **success**: весь Windows job, включая UI/TUI/shared native и spec checks, зелёный.
- [Build binaries](https://github.com/cascad/localvox-light/actions/runs/43463289640):
  Linux x64/ARM64 **success**, Windows/macOS Intel **failure** в Delivery contracts
  после успешной компиляции бинарников/desktop. macOS ARM64 **cancelled**, publish
  **skipped**. Это не полностью зелёный CI и не аппаратная приёмка.
- macOS Intel: `Refusing model replacement through a symlink: /var` — прежний
  guard отвергал штатный ancestor alias /var → /private/var.
- Windows: assert Hidden=0. Исходный user-root сравнивался с уже нормализованным
  component path; воспроизведено на эквивалентном `profile/../profile` пути.
  Hosted temp использует short-name RUNNER~1. Следующее Bash Check=0 — каскад после
  failed PS subtest, оставившего готовый store, а не отдельная причина.
- Отдельный Windows Bash download: native curl exit 23, Unicode output filename
  превратился в underscores. Выбор используемого curl зависел от PATH runner.

## Исправление

- [PS parent](../../../../scripts/model-paths.ps1): одинаковый GetFullPath для
  Target/UserRoot перед prefix check; новые standard roots получают Hidden,
  existing/custom attributes сохраняются.
- [Bash guard](../../../../scripts/model-paths.sh): physical parent через cd/pwd -P
  плюс отсутствующий suffix. Operation использует возвращённый physical destination;
  final model symlink/junction и несовпадающий destination отвергаются. Нет hardcode
  /var и отключения защиты. MSYS mount aliases /d/... и /tmp приведены к одной форме.
- [Fetch](../../../../scripts/fetch-models.sh): .part открывает shell redirection,
  curl пишет байты в stdout. Hash/rename/ZIP marker checks сохранены; native curl
  не получает Unicode filename для перекодирования.

## Local red → green

Все временные файлы под D:, tiny localhost payloads, никакого user daemon/архива,
моделей/inference или настоящего скачивания весов.

Новые [регрессии](../../../../scripts/tests/test_model_storage_delivery.py) исполнены
также с прежними helpers из `git show fee3103:...`, в отдельном приватном fixture:
PS 5.1 и 7.7.0 дали Hidden failure, Bash отверг ancestor alias (**3 ожидаемых failed
assertions**). С исправлением обе проверки **passed**, включая NTFS junction без
symlink privileges, physical destination parity и отказ менять link самой модели
или соседний каталог. Windows fixtures принудительно используют настоящий System32
curl, а не удобный вариант из PATH. Unix native symlink integration на Windows
отмечается skipped и выполняется на Linux/Mac runner.

Полный текущий `test_*delivery.py` с freshly packaged Windows release archive:
**38 passed / 1 skipped** (39 tests, 364.257 s). Пропущен только native Unix
symlink integration; NTFS ancestor/final junction regression прошёл отдельно.
Package smoke, установленный model-path JSON bridge, doctor/setup-check,
cancel/no-op/reconcile и fetch Check **passed**. Бинарники переиспользованы из
проверенного fee3103 release build: Rust не менялся, перепакованы новые helpers.
После уточнения regression входа `logical == selected` обе targeted проверки
повторно **passed** (PS 5.1/7.x и Bash, 6.101 s).

Strict main **15 passed**, исходная и новая change validation, check-openspec
(15 capabilities / 88 requirements / 188 scenarios), actionlint, обе Bash syntax
проверки и scoped diff **passed**. Полный inference/GUI прогон для правки scripts
не повторялся; hosted fee3103 Interface результат приведён выше.

Логи: `target/tmp/model-platform-{red,green,fixtures,package,install,delivery,specs}.log`.
Исходные требования и исторические результаты не переписывались.

## Граница

Новый hosted прогон и native Mac после исправления — **not-run** до следующего push
владельца. Не выдавать локальный Windows/junction тест за настоящий Mac /var прогон.
