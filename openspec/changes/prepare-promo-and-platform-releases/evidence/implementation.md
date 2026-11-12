# Пакеты, установка и промо · 2026-11-12

Продолжение после паузы владельца. Исходная точка и промежуточные результаты
сохранены в [checkpoint](checkpoint-2026-11-11.md); они не переписаны как новые проверки.
Push, tag и release не выполнялись. Задача 3.2 остаётся not-run.

## Доставка

- [package-release.py](../../../../scripts/package-release.py) собирает daemon со
  встроенным UI, worker, TUI, desktop при соответствующем флаге, native libraries,
  preset, helper scripts и лицензии. Пользовательская `.env` и модели не копируются.
  `bundle.json` фиксирует target, имена, размеры и SHA-256 каждого файла.
- Windows DLL собираются явно из native/source directories, включая DirectML,
  если он находится рядом с worker. Ошибочная native target directory build hook
  не используется как единственное доказательство комплекта.
- На Mac dylib ID/dependency references заменяются на относительные, после
  изменения Mach-O применяется ad-hoc signature. Несистемная зависимость без
  bundled library останавливает сборку. Unit проверяет команды; native Mac run
  остаётся not-run. Официальный Vosk OSX архив отдельно прочитан как fat Mach-O
  с x86_64 и arm64, зависимостями Accelerate/libc++/libSystem.
- [Windows](../../../../scripts/install-release.ps1) и [Unix](../../../../scripts/install-release.sh)
  выбирают точное имя полного архива, проверяют platform/manifest/hash до копирования,
  отвергают path traversal/links, сохраняют `.env` и проверяют startup/внешние
  exit codes. Local archive использует тот же путь. Existing models loader и его
  manifest сохранены; RequiredOnly передаётся ему, failures не подавляются.
  Явный success exit Windows installer отдельно проверен при прямом вызове из
  другого PowerShell script, как в CI: допустимый Doctor=2 при SkipModels не
  протекает в exit code всей установки, управление возвращается вызывающему script.
- [CI](../../../../.github/workflows/build-binaries.yml) запускается также на рабочих
  ветках/PR. Matrix: Windows x64, macOS Intel/Apple Silicon, прежние Linux x64/arm64.
  UI placeholder guard теперь действительно завершает job ошибкой; package/startup
  и установка во временную папку предшествуют upload. Bootstrap installers публикуются
  рядом с архивами. Release/tag должен указывать на тот же commit, что собран;
  несовпадающий существующий tag отвергается. Удалённая публикация не запускалась.

Настоящий Windows архив собран из проверенного runtime комплекта предыдущего
7e35377 (runtime код этой работой не меняется), с новыми scripts/docs:
`target/release-delivery-check/localvox-x86_64-pc-windows-msvc.zip`.
[Состав и SHA проверенного Windows архива](windows-bundle.json).
Packaged daemon `--help`, worker `--worker-protocol` (localvox-worker/3), TUI
`--version` прошли из staging. Реальная установка и повторное обновление в папку
с пробелами прошли, owner override сохранился. Полная повторная загрузка моделей,
запись устройств и remote release download не выполнялись.

## Промо

[Галерея](../../../../docs/promo.md) содержит **11 PNG + 4 GIF**, 1440 × 930.
Итого после окончательной пересъёмки **2 153 785 bytes**;
актуальные bytes/SHA/dimensions/durations находятся в
[manifest](../../../../docs/media/manifest.json). GIF: GUI reading 9 s / 45 frames,
GUI journal 10 s / 50 frames, TUI navigation 12 s / 60 frames, TUI search
9.6 s / 48 frames. Ограничения инструмента: ≤3 MiB/asset, ≤8 MiB в сумме.

React UI снят из настоящего production build обычными кликами. TUI — настоящий
процесс в portable-pty; ANSI воспроизведён в xterm. Сценарии реально нажимают
j/k, l/Enter, вкладки, Slash и Esc. Подписи клавиш/рамка вне продукта. Для child
удалены унаследованные NO_COLOR и custom keymap, что не меняет пользовательские
настройки. DSR host reply такой же, как в существующих ConPTY tests.

Синтетические данные, отдельный loopback fixture server, статические файлы
ограничены их roots. Личная `.env`, архив, демон, microphone/loopback не нужны.
Предыдущие GUI кадры без CSS пересняты; перед PNG проверяется активный grid style.
В GIF чтения съёмка ожидает содержимое новой вкладки, чтобы preview не начинался
с загрузки. PNG и first/middle/last GIF frames просмотрены; baselines продукта
не менялись. Уникальные промежуточные PNG directories удаляются после GIF.

README теперь начинает с краткого описания, установки и запуска, затем показывает
GUI/TUI GIF. Состояние старого release явно отмечено до команд. Добавлены US-057/058,
прежние 56 US IDs сохранены. [Mac checklist](../../../../docs/release-installation.md#проверка-mac-владельцем)
остаётся за владельцем, все его аппаратные пункты not-run.

## Выполненные проверки

Логи `target/tmp/promo-final-*`, TEMP/TMP на D:, jobs=2, PYTHONDONTWRITEBYTECODE=1.

| Проверка | Результат |
|---|---|
| Delivery unittest | **12 passed**, включая real Windows install/update, отказ старого release/network, неверный target/checksum/paths/links, обе Mac shell fixtures |
| Workflow actionlint 1.7.12 | Оба workflow passed; hosted job этим не объявляется пройденным |
| Shell / PowerShell parser | Bash syntax и PowerShell parse passed |
| Rust promo recorder | Locked build и настоящий ConPTY capture passed |
| Promo fixture | 1 Node test passed: данные/clock/static confinement |
| UI build / unit / browser | build passed, **14 unit**, **81 browser**, 32.2 s; прежние visual baselines сохранены |
| Media integrity | 15 файлов, SHA-256/размеры/1440×930 проверены; GIF frame counts/durations получены ffprobe |

Три ожидаемых negative visual tests symbol/color/geometry входят в 81 passed и
подтверждают обнаружение изменений. Speech models/quality не прогонялись: runtime
обработки не менялся. Mac-native/Linux-native package smoke и hosted CI остаются
до запуска jobs; не заменены shell fixtures/Windows loader проверкой.

## Traceability и оставшиеся границы

| Requirement | Реализация | Доказательство / предел |
|---|---|---|
| SETUP-001, INV-SETUP-001 | прежние models.json/fetch-models, pinned helpers | RequiredOnly forwarding в fixture; model checksum/inference заново не запускались |
| SETUP-002 | package/native staging + installers | Windows native startup/полный состав; Mac relocation unit и реальный fat library audit; Mac runtime not-run |
| SETUP-003 | прежний doctor, installer exit handling | Doctor вызывается из отдельной установленной папки; SkipModels явно отделён от готовых моделей |
| SETUP-004 (delta) | package/install/workflows | Local archive и отказы test-run; remote/hosted/native Mac acceptance not-run |
| ARCH-001/002, PIPE-011 (прежний контракт) | UI/TUI код сохранён | Production UI e2e, real TUI media capture; это показ интерфейса, не новая аппаратная приёмка |

Main specs не sync/archive. Автономные пункты закрываются после strict checks;
3.2 остаётся открытым для владельца. Для установки по README нужен новый полный
release; до него — Actions archive + installer из этой ветки.

## Уборка

Крупных ненужных артефактов агента на C: не обнаружено; два старых transform файла
репозитория (772856 bytes) ранее перенесены с C: в target/tmp на D:. Попытка удалить
эти копии и два Python cache файла (~0.8 MB суммарно) в завершении отклонена
automatic review: `blocked by policy`. Повторных обходов удаления нет; файлы
остаются локально и не входят в Git. Новые сборки, media intermediates и tools на D:.


Финальные проверки документации 2026-11-12: strict main **15 passed / 0 failed**,
strict change valid; check-openspec **15 capabilities / 88 requirements /
188 scenarios / 88 coverage rows / 1919 links PASS**. Scoped rustfmt и diff check
прошли. 6/7 автономных/приёмочных задач закрыты; остаётся только 3.2 владельца.
