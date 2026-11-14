# Production installer integration · 2026-11-14

Контракт: SETUP-009 остаётся delta этого change, SETUP-006 — delta
`add-install-reconciliation-wizard`; принятые SETUP-001/002/003, CFG-001/003 сохраняются.
Ни sync, ни archive не выполнялись. Владелец принял v2: «Ну круто, мне нравится 2 вариант».
Это разрешение подключить просмотренный frontend, не приёмка всех устройств/ОС.

## Реализация

- [Native controller](../../../../crates/localvox-light/src/setup/install.rs) использует
  общий UI и существующий planner/executor вместо второго numeric wizard.
  Callback валидирует и показывает именно применяемый план. Отмена/ошибка до
  подтверждения не пишет конфиг/компоненты/PATH. Отдельные готовые папки ASR
  сохраняются без копирования; обязательные модели нельзя снять галочкой.
  Обычный `localvox-light --setup` остаётся настройкой текущей программы: не требует
  `bundle.json`, не публикует binaries и пишет ту же активную `.env`, включая cwd
  fallback. Перенос программы выполняется отдельным `localvox-install`.
- [Bundle](../../../../crates/localvox-light/src/setup/bundle.rs) проверяет target,
  пути, native dependencies и SHA-256 всех членов до публикации. Файлы заменяются
  атомарно, manifest публикуется последним. Общего rollback не заявлено; неполное
  обновление обнаруживается и исправляется повторным bootstrap.
- [Checkpoint](../../../../crates/localvox-light/src/setup/pending.rs) хранит выбор
  подтверждённой незавершённой операции, отдельно от рабочей `.env`.
  Последующие choices снова проверяются и подтверждаются. Snapshot защищает более
  новые настройки; installer/model leases и archive lock сериализуют cooperating
  writers. Истинный no-op разрешён при занятом демоном архиве.
- [PowerShell](../../../../scripts/install.ps1) / [Bash](../../../../scripts/install.sh)
  выбирают один release tag и platform, проверяют hash и архив, запускают мастер из
  полного staging с DLL. Целевые папки внутри удаляемого staging запрещены.
  Повтор исправной установки обходится без сети; Check read-only, Update явный.
  Пользователю не нужны Rust/Node/Python/jq до bootstrap.
- Недостающие инструменты устанавливаются через OS managers. Windows mpv входит
  в bundle вместе с лицензией; CI проверяет официальный asset digest. Уже готовые
  инструменты и Ollama используются повторно. Подтверждённая установка Ollama
  использует официальный installer; локальный API запускается при необходимости,
  при ошибке убирается только дочерний процесс текущего запуска.
- Missing `.env` больше не выдаёт повреждение конфигурации. Parse/access errors
  сохранены, строка с секретным содержимым не печатается. Найден и исправлен
  отдельный дефект renderer: bare Windows paths/пробелы не проходили runtime
  dotenvy. Теперь literal quoting проверяется обоими читателями, включая `$` и `'`.

## Выполненные проверки

Windows x64, Rust 1.99.1. Все caches/TEMP/build находятся в `target` на D:.
Пользовательский демон/архив и модели не изменялись; настоящие inference, pull,
OS package install и регистрация PATH не выполнялись.

- `scripts/test-installer.ps1`: env_file **20 passed**; light **16 passed / 11 ignored**,
  main **2 passed**, backend setup **10 passed**. Ignored profiling/hardware/worker
  scenarios не считаются выполненными. Model storage **10 passed / 3 skipped**,
  legacy release delivery **19 passed / 1 skipped** на этом запуске.
- После последующих изменений `cargo test ... --test install`: **11 passed**,
  включая настоящий ConPTY cancel, prepare error/cancel, apply, repeat;
  не переписывает модели и `.env`, не регистрирует настоящий PATH. Отдельный native
  main subprocess проверил missing/invalid `.env`, отсутствие ancestor traversal
  и утечки секретного текста. Фикстуры лежат в private temporary roots.
  Отдельная regression fixture без bundle проверила сохранение внешней активной
  `.env` без создания затеняющей `.env` рядом с development binary.
- UI `cargo test -p localvox-setup-ui`: **11 passed**, реальные ConPTY клавиши,
  Back/retained answers, отдельные model paths, cursor restore и safe preview.
- Bootstrap localhost fixtures: **3 passed / 1 skipped**; Windows download/apply/reuse,
  digest/traversal failure до target mutation, все четыре Unix target routes без jq.
  Native Unix PTY success fixture на Windows skipped; команда владельцу —
  [локальный стенд](../../../../docs/installer-testing.md).
- Windows tool adapters **3 passed** после финальной правки: fake WinGet IDs/args,
  tiny mpv hash/license, corrupt payload не публикуется; официальный
  `O=Ollama Inc.` принимается, похожие чужие publisher subjects отклоняются до запуска.
  Unattended/Hidden flags проверены без запуска installer. Реальные managers не запускались.
  Publisher сверён с [официальным скриптом](https://github.com/ollama/ollama/blob/main/scripts/install.ps1).
- CI delivery contracts **4 passed / 1 skipped** до сборки полного архива.
- Итоговый общий `python -m unittest discover -s scripts/tests -p 'test_*delivery.py' -v`
  с `LOCALVOX_DELIVERY_ARCHIVE`: **46 passed / 3 skipped** (49 total), включая
  реальный Windows архив/doctor/reconcile и native resolver parity обоих загрузчиков.
  Skipped: Unix controlling-terminal bootstrap, Windows 8.3 alias и Unix symlink semantics.
  После нормализации shell helpers в packaging отдельно прошёл bundle/hash/LF regression;
  `.gitattributes` закрепляет LF независимо от checkout на Windows.
- Полный Windows release build и package smoke прошли: демон, worker protocol/3,
  TUI, vox, installer; desktop включён в архив. DLL рядом с программами.
  `check-release-install.py` прошёл: doctor exit 2 для намеренно отсутствующих ASR,
  bundle check, отказ pipe wizard без mutation, read-only и повтор без распаковки.
- Strict main specs **15 passed**, strict selected change passed;
  `check-openspec.py` прошёл (15 capabilities / 88 requirements / 188 scenarios),
  scoped diff и rustfmt/PowerShell/Bash syntax прошли. Clippy в изолированном toolchain
  отсутствует, не запускался. Existing dead-code/deprecation warnings не скрыты.

Начальные неудачные проверки сохранены как диагностика: первый cargo test получил
STATUS_DLL_NOT_FOUND и показал владельцу системное окно. Причина — PATH тестового
процесса, четыре DLL уже присутствовали. Скрипт проверок теперь preflights DLL,
добавляет runtime PATH и подавляет native modal dialogs только в собственном процессе
с восстановлением ErrorMode, без системных изменений. Затем тесты прошли.
Ранние regression failures также выявили потерянный вызов apply_actions при сборке
controller, неверную quoting fixture и изменившееся число packaged executables;
исправления проверены повторно, прежние failures не названы успешными запусками.

## Открытые границы

- Публичный полный release ещё не опубликован; README явно предупреждает про старый
  `v0.1.5`. One-line release assets/checksums и пять build targets подготовлены в
  workflow. Push/tag/publication требуют отдельного прямого поручения владельца.
- Новые hosted Actions **not-run** до push. Не приписывать текущим изменениям
  зелёный статус старой remote сборки.
- Native Linux/macOS stand, настоящие Ollama/OS manager/model downloads, системные
  права, физическая запись/плеер/микрофон — **not-run**. Mock и cross-platform routing
  не заменяют native аппаратную приёмку. Большие файлы не скачивались.
- Нет общего межпроцессного CAS против произвольных external editors, power-loss
  rollback или гарантии resume отдельного сетевого соединения. Готовые проверенные
  файлы переиспользуются; частичный файл не объявляется готовой моделью.
