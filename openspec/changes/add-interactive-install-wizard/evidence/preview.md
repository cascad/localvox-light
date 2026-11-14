# Safe installer preview · 2026-11-14

## Scope and status

SETUP-009 остаётся delta. Выполнен только первый этап: отдельный frontend и
безопасный демонстрационный бинарник. Production setup/bootstrap/release не менялись.
Просмотр владельца (1.2), подключение установки (2.1/2.2) и доставка (3.1) открыты.
Это не свидетельство готовности установки одной строкой или нативной приёмки Mac/Linux.

Код: [presentation](../../../../crates/localvox-setup-ui/src/lib.rs),
[preview](../../../../crates/localvox-setup-ui/src/main.rs),
[PTY tests](../../../../crates/localvox-setup-ui/tests/preview.rs),
[render](../../../../scripts/installer-preview/capture.mjs).
Сценарий просмотра: [installer-preview](../../../../docs/installer-preview.md),
US-064 в [справочнике](../../../../docs/user-guide.md).

## Implementation

- Лёгкий crate не зависит от daemon/core/audio/Vosk. Получает typed view data,
  возвращает выбор; prompt и клавиши обслуживает cliclack 0.5.6.
- Рекомендуемый набор: распознавание обязательно; голоса, сводки, ссылки, плеер.
  Проверка имён — дополнительная галочка в custom. Готовые компоненты отмечены.
- Custom: Space checkbox, arrows/Enter, Ollama tag/server/GGUF, папки и план.
  Отключённая сводка пропускает вопросы о ней. Esc/Ctrl+C/отказ завершают без изменений.
- Demo использует фиктивные fresh/existing состояния, только показывает домашний путь
  и публичный preset модели. Не читает частную .env, не вызывает probes, subprocess
  installers, daemon, сеть или запись настроек. Выполнение — явно помеченная имитация.
- Существующие версии пакетов в Cargo.lock сохранены: comparison name/version с HEAD
  показал 0 удалённых/обновлённых. Добавлены только frontend и его зависимости.
  textwrap оставлен 0.16.2, чтобы не обновлять посторонний ICU/model workspace.

## Executed checks · Windows x86_64

Rust 1.99.1, native ConPTY. Cargo home/build/TEMP и снимки — внутри `target/` на D:.
Ни установленная программа, ни пользовательский архив/модели не запускались.

```powershell
$env:RUSTUP_HOME = "$PWD/target/tools/rustup-native-ci"
$env:CARGO_HOME = "$PWD/target/tools/cargo-installer-ui"
$env:CARGO_TARGET_DIR = "$PWD/target/installer-ui-build"
$env:TEMP = "$PWD/target/tmp"
$env:TMP = $env:TEMP
$env:LOCALVOX_INSTALLER_CAPTURE_DIR = "$PWD/target/review/installer-preview/captures"
cargo test --locked -p localvox-setup-ui --target x86_64-pc-windows-msvc --jobs 2 -- --test-threads=1
cargo fmt -p localvox-setup-ui --check
node --check scripts/installer-preview/capture.mjs
node scripts/installer-preview/capture.mjs
```

6 integration tests passed (3.72 s), 9 native terminal sessions плюс pipe/help:

| Test | Проверенное поведение |
|---|---|
| recommended_fresh_and_existing_reuse_are_safe | Default без лишних вопросов, план fresh/existing, подтверждение демо |
| space_toggles_components_and_skips_disabled_summary_at_80_columns | Настоящие Space/arrows, обязательный STT, снятая сводка, NER, ASCII/80 колонок |
| custom_model_and_paths_reach_the_review_without_writing_them | Новый tag и пути с пробелами показаны в плане; папки не создаются |
| custom_server_and_gguf_are_presented_without_probing_or_importing | Свой API/модель и несуществующий GGUF только показаны, никаких probe/import |
| esc_ctrl_c_and_decline_restore_cursor_and_leave_no_changes | Esc, Ctrl+C и отказ в плане; success exit, курсор восстановлен |
| pipe_is_rejected_and_help_needs_no_terminal | Pipe не изображает TTY; help доступен без терминала |

Каждая PTY-сессия работает с отдельными HOME/USERPROFILE/LOCALAPPDATA/cwd и заведомо
невалидной sentinel .env. Снимок файлов/директорий до/после совпал побайтно.
Современный UTF-8 вид и ASCII fallback задаются возможностями test emulator;
разрешение конкретных пользовательских терминалов этим не объявляется проверенным.
Ожидания синхронизированы с экраном, PTY отвечает на cursor-position query ConPTY.

Fmt и Node syntax passed. Clippy **not-run**: компонент не установлен в изолированном
toolchain 1.99.1; скачивание ради preview не выполнялось.

Контракты и ссылки:

- `openspec validate --specs --strict --no-interactive` — 15 passed, 0 failed;
- `openspec validate add-interactive-install-wizard --type change --strict --no-interactive` — valid;
- `python scripts/check-openspec.py` — PASS: 15 capabilities, 88 requirements,
  188 scenarios, 88 coverage rows, 2054 local links;
- `git diff --check` — passed; production setup/bootstrap и личная .env не изменены.

## Visual artifacts

`target/review/installer-preview/`:

- `localvox-setup-preview.exe` — скопирован проверенный бинарник, `--help` passed;
- 5 PNG — welcome, компоненты, источник сводки, папки, итог;
- `installer-preview.gif` — 1240×930, 5 кадров / 10 секунд, ffprobe passed;
- `captures/*.ansi`, `*.txt`, `*.rendered.txt` — настоящий вывод ConPTY и replay xterm.

PNG вручную просмотрены агентом; исправлена многострочная подсказка папок,
чтобы пути шли отдельным блоком, а не выпадали из строки выбора.
Оформление владельцем **not-run**. Native Linux/macOS **not-run**.
GitHub build/push/publication не выполнялись; main specs не синхронизированы.
