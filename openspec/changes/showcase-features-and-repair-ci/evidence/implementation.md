# README и CI установки — 2026-11-12

## Основание и границы

Завершённые локальные проверки ниже — test-run, не результат нового hosted run.
Существующие SETUP-001/002/003, INV-SETUP-001, SETUP-005/006 delta и ARCH-014 /
TERM-007 delta сохраняются. Новых действий/форматов нет; main specs не менялись.
Пользовательские архив, `.env`, модели и работающий демон для проверки не нужны.

## Воспроизведённые сбои GitHub

Через GitHub connector прочитаны jobs/logs последнего pushed `8a3a7dc`:

- [Build binaries 43195753328](https://github.com/cascad/localvox-light/actions/runs/43195753328):
  все пять платформ остановились на `npm ci`. Логи Windows job 131106153681 и
  Intel Mac 131106154064: отсутствуют `@emnapi/core@1.11.3` и
  `@emnapi/runtime@1.11.3` в lock. npm в runners — 11.19.1 / 11.17.0.
- [Interface 43195753300](https://github.com/cascad/localvox-light/actions/runs/43195753300):
  Windows job 131106153244 продолжил UI block после npm failure; последняя
  успешная команда скрыла nonzero. Позже native paste test потерял `🦀`:
  фактическое `https://example.org/q?i=Русский`, ожидалось то же с emoji.

Локальный npm 11.6.1 не обнаруживал неполный lock даже при regeneration. С npm
11.19.1 исходный `ci --ignore-scripts --dry-run` воспроизвёл обе missing entries.
Lock regenerated через npm 11.19.1 в чистой папке без node_modules: добавлены
два optional peer узла и исправлены peer flags, версии остальных пакетов сохранены.

Native ConPTY на локальном Windows **проходил также до fix**: представление
символов в hosted console пока не доказано. Отдельный regression воспроизвёл
реальную потерю committed Alt+Numpad Unicode на `VK_MENU` key-up; после fix он
проходит. Обычные key-up остаются проигнорированы. Это соответствует стандартному
Windows input: [Microsoft Terminal implementation](https://github.com/microsoft/terminal/blob/main/src/terminal/input/terminalInput.cpp).
Исходная emoji assertion не ослаблена; test-only bounded trace теперь приложит
сырые KEY_EVENT при повторном hosted failure. Нужен новый GitHub run, чтобы
подтвердить устранение именно наблюдавшегося там сбоя.

## Реализация

- [README](../../../../README.md): GUI GIF → идея → установка → возможности с
  GUI/TUI PNG/GIF. Текущая установка — полный Actions bundle, поскольку v0.1.5
  содержит отдельные бинарники. Старые installation anchors сохранены.
- [Developer guide](../../../../docs/development.md) сохраняет инструкции сборки;
  [promo index](../../../../promo/README.md) ссылается на единые originals.
  Telegram post не создавался. Обновлены ссылки пользовательского справочника.
- [Workflows](../../../../.github/workflows/build-binaries.yml): fail-fast UI
  Bash и PowerShell native commands. Ранний exit 7 проверен настоящим subprocess:
  следующая успешная команда не выполняется. Installer локально интерпретирует
  doctor 0/1/2 и проверяет native exit codes, в том числе под строгим PS7 parent.
- [Native input](../../../../crates/localvox-light-terminal/src/runtime/input/windows.rs)
  сохраняет committed text; [regression](../../../../crates/localvox-light-terminal/src/runtime/input/windows/tests.rs)
  проверяет UTF-16 через границы batches и отсутствие дублирования обычных release.
- [Reusable installed smoke](../../../../scripts/check-release-install.py) вызывается
  после упаковки на всех пяти targets. Свой install с пробелами, закрытый local
  endpoint и отсутствующие ASR; no inference/download. Проверяет JSON schema/IDs,
  expected exit 2, setup cancel/no-op напрямую и через reconcile, SHA/mtime и
  дерево файлов, fetch check без создания model/archive dirs. Reconcile получает
  несуществующий архив: готовый комплект обязан использоваться повторно.

## Выполнено локально

Windows, release/offline Cargo, jobs 2 и test threads 2; временные данные и npm
cache в `target/` на D:. Визуальные baselines не обновлялись (`INSTA_UPDATE=no`).

| Проверка | Результат |
|---|---|
| npm 11.19.1 clean `ci --no-audit --no-fund` | pass; до fix воспроизводимый EUSAGE |
| npm 11.19.1 `ci --dry-run --os=darwin --cpu=arm64/x64` | pass; это lock resolution, не native Mac build |
| UI production build / unit / Playwright, workers 2 | pass / 14 passed / 81 passed; detector negative controls тоже проходят |
| TUI/client/process release suites | TUI 200 passed, 3 fixture-only ignored; client 3 + 8 passed; process 12 passed, 1 fixture-only ignored + resident IO executable |
| TUI actual workspace ConPTY / IO / run-script | 5 / 2 / 2 passed; input ConPTY и restoration также в lib suite |
| Final TUI lib after diagnostic addition | 200 passed, 3 fixture-only ignored; unchanged snapshots |
| Portable delivery fixtures + native failure propagation | 18 passed, 2 explicit archive tests pending next row; Mac shell fixtures выполнялись в Git Bash с jq, не на Mac |
| Complete native Windows bundle + both explicit archive tests | **20 delivery passed, 0 skipped**, 50.56 s; новые daemon/worker/TUI/desktop собраны release, packaged startup passed; wizard напрямую и через installer, private `.env` и mtimes сохранены |
| Script quality fixtures | 22 passed |
| Workflow / scripts syntax | actionlint both workflows passed; Bash `-n` installer/fetch/Ollama/Vosk; PowerShell parser installer/fetch/Ollama/Vosk/run; все rustfmt checks из interface workflow и API cargo fmt passed |
| Existing mpv setup | pass: verified executable reused, no download |
| README preview | headless Chromium, 1280 and 390 px; 7 images loaded, no horizontal overflow; desktop/mobile inspected visually |
| Media / links | 15 original hashes, sizes, dimensions, GIF frames/durations verified; 60 current documentation links/anchors passed |

Windows archive: `target/setup-delivery-check/localvox-x86_64-pc-windows-msvc.zip`,
57,763,170 bytes; temporary local build, not uploaded/published. Full source build
занял 7m10s с jobs 2; packaging проверил старты daemon/worker/TUI без моделей.

`openspec validate --specs --strict --no-interactive`, strict validation этого
change, `python scripts/check-openspec.py` и `git diff --check` — passed.
Информационные замечания о длине прежних требований не стали ошибками; незавершённые
proposal-only changes через `validate --all` не объявлялись проверенными.

Ignored logs and preview PNG are in `target/tmp/showcase-*` / `target/tmp/readme-*`;
they are temporary verification output, not additional promo assets.

## Осталось вне локальной проверки

- Новый hosted Windows/Mac/Linux run после push — **not-run**. Push не выполнялся.
- Настоящие Mac binaries, capture permissions, microphone/loopback, audio output
  и first-run Ollama/pull/GGUF — **not-run**; [чеклист владельца](../../../../docs/release-installation.md#проверка-mac-владельцем).
- Отдельные Windows Terminal/WezTerm/VS Code host UI — **not-run**;
  native ConPTY не заменяет эту приёмку. Качество ASR/LLM не измерялось.
