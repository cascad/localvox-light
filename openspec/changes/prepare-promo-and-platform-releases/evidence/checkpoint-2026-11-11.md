# Пауза по поручению владельца · 2026-11-11

Владелец: «тормози, завтра продолжим». Работа остановлена, текущие изменения
не закоммичены, push/release не выполнялись. Новые проверки после паузы не запускать.

## Уже сделано и проверено

- Создан change prepare-promo-and-platform-releases, apply ready; план 7 задач,
  последняя — hosted CI после push и аппаратная проверка Mac владельцем.
- scripts/package-release.py собирает полный пакет, native DLL/dylib, preset,
  лицензии и pinned helper scripts; macOS loader references + ad-hoc codesign.
- Переписаны Windows и Unix installers: точное имя архива, local archive option,
  manifest/checksum до копирования, сохранение .env, проверка subprocess/startup.
- Настоящий Windows пакет создан в target/release-delivery-check/ и установлен
  дважды во временную папку с пробелом; override сохранился. Пользовательский
  архив/.env/демон/устройства для этого не использовались.
- scripts/tests/test_release_delivery.py: **10 tests passed**, включая настоящую
  Windows установку, отказ до копирования, macOS Intel/ARM64 shell fixtures.
  Mac shell fixtures на Windows используют Git Bash и POSIX-output adapter для
  Windows jq; это проверка логики scripts, не запуск native Mac продукта.
- Actions изменены: push всех веток + PR, исправлен UI guard, полный pack и
  installer smoke на Windows/macOS/Linux, setup-node v6/Node24. actionlint 1.7.12
  прошёл на обоих workflows до последних добавлений tooling; повторить финально.
- Официальный vosk-osx-0.3.42.zip проверен как Mach-O fat: x86_64 + arm64,
  зависимости только Accelerate/libc++/libSystem. Native Mac запуск not-run.
- Реальные React UI и TUI (portable-pty), вымышленные встречи: в docs/media есть
  **11 PNG и 4 GIF**. Скрипты — scripts/promo; recorder example —
  crates/localvox-light-terminal/examples/promo-capture.rs.
- GUI кадры сначала частично получились без CSS; такие кадры пересняты. Добавлена
  проверка computed .app display=grid перед сохранением PNG. Последний GUI-only
  прогон прошёл, PNG выглядят корректно; GIF нужно ещё визуально проверить.
- TUI recorder отвечает на стандартный ConPTY DSR handshake, как существующие
  PTY tests. Первый зависший recorder остановлен по точному PID/path/arguments;
  его пользовательских процессов не было. Последующий полный capture прошёл.

Логи: target/tmp/promo-delivery-tests.log, promo-package.log,
promo-capture.log, promo-capture-gui.log. Tmp/build/media intermediates на D:.

## Последние изменения ещё НЕ проверены запуском

- В recorder добавлен env_remove(NO_COLOR) и env_remove(LOCALVOX_TUI_KEYMAP).
  Агентский NO_COLOR=1 делал прежние TUI кадры монохромными. Пересобрать example
  и переснять TUI в настоящих цветах; продукт не менять.
- В capture.mjs добавлены уникальные mkdtemp frame directories и их cleanup после
  GIF; этот вариант скрипта ещё не запускался. Старые constant frame directories
  под target/promo остаются промежуточными данными предыдущих попыток.
- Последний GUI-only прогон перезаписал target/promo/manifest.json только GUI
  записями. Нужен общий финальный manifest всех PNG/GIF с SHA/размерами/длительностью.

## Дальше

1. Проверить shell/package edge cases и финальный actionlint. Hosted run не выдавать
   за выполненный: push запрещён без отдельного поручения.
2. Пересобрать recorder и переснять цветной TUI; визуально просмотреть PNG и
   несколько кадров каждого GIF. Проверить bounded sizes/durations.
3. Уточнить fixture execution clock (добавить started_at), чтобы в промо-журнале
   не появлялось «время неизвестно» из-за неполной synthetic DTO. Ограничить static
   fixture routes их разрешёнными корнями (сейчас root-prefix слишком широкий).
4. README пока НЕ обновлён: поставить краткий installer/start сразу после intro,
   затем GIF и ссылку на галерею; старый контрактный блок перенести ниже.
   До нового полного release старый latest v0.1.5 не подходит: кратко указать
   Actions archive path. Рассмотреть публикацию bootstrap install scripts рядом
   с архивами, чтобы README не зависел от скриптов другой ветки.
5. Создать docs/promo.md (галерея + как переснять), обновить US-ID установки в
   docs/user-guide.md. docs/release-installation.md уже содержит короткие действия
   и явный Mac daemon/capture checklist not-run для владельца.
6. Strict main/change/check-openspec/diff, scoped rustfmt для example, UI build/unit
   и подходящие e2e после dev dependency @xterm/xterm; evidence/coverage SETUP IDs.
   После проверок — завершённые этапы коммитить; push/tag/release не делать.

## C: уборка

На момент проверки свободно около 20 GB. Temp ~1.04 GB: основные большие файлы
чужие (diagnostics, VS Code update, WSL swap), не тронуты. .cache ~2.99 GB:
codex-runtimes активен, модели датированы до этой работы; npm-cache ~496 MB
используется инструментами. Не объявлять их своими ненужными артефактами.

Подтверждены два старых Playwright transform файла этого репозитория от 2026-10-31,
772856 bytes (~0.74 MiB). Удаление с C: отклонено auto-review («blocked by policy»).
Применён более безопасный обратимый перенос в
target/tmp/old-c-playwright-cache/{workspacespec.js,workspacespec.map}; с C: они
убраны. Физическое удаление этих копий ещё не выполнялось. Массовой чистки C: нет.
Новые jq/actionlint, Vosk probe ZIP, npm cache для установки xterm и raw captures
лежат только в target на D:. Отдельный повторный scan всего диска не нужен.
