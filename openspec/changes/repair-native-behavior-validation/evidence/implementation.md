# Разбор native Mac отчёта и исправления · 2026-11-25

## Входные данные

Архив владельца: SHA-256
`5a183820dc514e5d702d123a579255d49aba380140ac895c30175a89775df674`.
Отчёт `20261125T114827-25043`: macOS 26.6.2 ARM64, исходники
`c11c213a3a8770e295b1bb7144b331630978b248`, working tree dirty. Содержание
незакоммиченных Mac-правок неизвестно и не считается совпадающим с Windows.
Установленные vox/TUI отвечают `0.2.11`; текущая workspace version — `0.2.13`.
Из traceback подтверждён Python 3.10. Сырые логи сохранены локально в
игнорируемом `target/review/macos-behavior-20261125`, в Git не добавляются.

## Каждый отказ

| Группа | Фактический отказ | Исправление / граница |
|---|---|---|
| tui-state-and-render | 253 passed; единственный failed — нет fish для shell profile test | Runner выделяет внешний zsh/fish тест в отдельный шаг; missing shell → blocked, остальные state/render tests выполняются |
| tui-native-terminal | 7 таймаутов ожидания новых журналов, подтверждений/состояний; запускается TUI 0.2.11 | Версионный preflight блокирует несовместимый установленный клиент; старые failures не выдаются за проверку текущего TUI. Нужен повтор с совпадающей версией |
| archive-api | `C:` получает «сессия не найдена» вместо «некорректное имя» | ARCH-001: явный portable guard до Path::components; также запрещены обратный слеш, colon/NUL и нормализуемые `/`, `/.`; правильное Unicode-имя сохранено |
| asr-settings-http | copied child завершился до readiness, stdout/stderr потеряны | Обнаружен буквальный root/cwd/exe compare, неверный при `/tmp` → `/private/tmp`; пути канонизируются, Unix-тест намеренно использует symlink root. Child stdout/stderr теперь сохраняются. Первичная Mac причина без stderr остаётся неподтверждённой до native повтора |
| setup-commands | ожидался Download GigaAM, но модель уже найдена в HOME | Discovery и settings fixture задают все component paths из manifest внутри собственного temp root. Production reuse не меняется; проверки существующих внешних моделей сохранены |
| delivery-scripts | нет tomllib/hashlib.file_digest в Python 3.10 | Python 3.11+ проверяется до запуска Cargo/групп, неподдерживаемый runtime даёт blocked/not-run с отчётом; частичной поддержки 3.10 не добавлено |
| GUI (6 blocked) | отсутствует ui/node_modules/@playwright/test | В инструкции добавлены npm ci / Playwright Chromium и подготовка Python/fish. Blocked сохраняется как blocked, поведение GUI на Mac не объявлено проверенным |

Версия бинарника не доказывает равенство SHA. Отчёт сохраняет Python/workspace
version, source commit/dirty tree и фактические/ожидаемые версии клиентов раздельно.
Новый preflight не заменяет установленный клиент собранным бинарником.

## Выполненные локальные проверки — Windows

Один Cargo worker, один test thread; собственные temp/HTTP/PTY, без пользовательского
архива/демона/моделей. Логи — `target/review/macos-*.log`.

- Archive regression сначала **failed** на существующем имени с завершающим `/`
  (`macos-archive-before.log`), затем 36 archive tests **passed**, 2 ignored.
- Настоящий settings HTTP: **passed**; повтор с унаследованным loopback=on также
  **passed** после явной изоляции переменной (1.42 s). Unix symlink ветка на
  Windows не выполнялась.
- Настоящий shared UI/TUI/API: **passed**, 6.00 s. Канонизация скопированного
  executable сохранила ownership assertions и все пользовательские проверки.
- Полный API library suite: 99 **passed**, 8 ignored (9.28 s), до добавления
  отдельного diagnostic regression. После него settings suite: 2 **passed**, 1 ignored;
  намеренно не принадлежащий fixture root child падает, parent сохраняет его panic в диагностике.
- Installation suite: 13 **passed**, включая повторное использование внешних моделей.
- Python runner/workflow contracts: 22 run, 1 skipped, 0 failed. Есть real-child
  version checks, unsupported Python до Cargo/TOML import, отдельный shell step,
  blocked/failed/not-run distinction и немедленный stop при прерывании version probe.
- End-to-end runner с настоящими review vox/TUI 0.2.11: оба version probes
  **blocked**, native PTY **blocked**, source TUI 260 **passed** / 3 ignored,
  HTTP contention **passed**. Нет failed и подмены клиента; exit 1 ожидаем.
  Отчёт: `target/behavior/20261125T161802-10072`. Итоговый текст этого запуска ещё
  был FAIL; последующая правка финальной метки проверяется Python contracts.
- Новые macOS CI команды archive/settings API проверены статически workflow
  contracts; native CI-run этим локальным результатом не подменяется.
- API rustfmt, strict validation main specs/change, check-openspec и diff: **passed**.

## Что требуется на Mac

Обновить установленный клиент до версии исходников, поставить Python 3.11+,
fish и GUI dependencies по [инструкции](../../../../docs/behavior-testing.md).
Затем полный запуск `python3.13 scripts/test-behavior.py --installed-dir
"$HOME/Applications/LocalVox"`. При своих source-правках сначала сохранить их;
не сбрасывать dirty tree ради теста.

Новый полный ARM/Intel Mac прогон, Unix alias ветка, Linux source suite и
Mac GUI остаются **not-run** до фактического выполнения. Микрофон, loopback,
разрешения и реальная ASR/LLM quality/performance — также **not-run**.
