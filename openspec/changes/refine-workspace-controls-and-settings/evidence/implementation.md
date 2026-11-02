# Предустановки и LLM-серверы — 2026-11-02

Реализованы CFG-001 delta и CFG-005; CFG-002 сохраняет Live/Capture/Restart.
Трассировка — в [матрице](coverage.md). Связанные визуальные поправки журнала/TUI —
в [отдельном отчёте](../../add-technical-journal-workspace/evidence/manual-feedback-2026-11-02.md).

## Поведение

`.env.defaults` содержит все предустановки каталога. Ищется рядом с пользовательским
`.env`, затем в cwd; если файла нет, используется сборочная копия того же env-файла.
Частичный внешний файл перекрывает сборочные значения; ошибки чтения/синтаксиса
не считаются отсутствием файла. Секреты в presets запрещены. Bootstrap после dotenv
добавляет только отсутствующие непустые значения, CLI сохраняет приоритет.
Contained worker сохраняет snapshot координатора. Основные точки bootstrap:
демон, standalone API и не-contained `localvox-process`.

Пустые устройства означают автоматический выбор и не создают явный CLI device:
прежний выбор из legacy capture config сохраняется. Сброс снимает override в `.env`
и возвращает preset с прежним сроком применения. `value/set` API описывают сохранённый
override; отдельный `default_value` не возвращается для секретов. Старые ответы без
новой metadata продолжают читаться. Открытие и закрытие редактора не создаёт override.

`LOCALVOX_LLM_SERVER_URLS` — JSON-список до 32 HTTP(S) адресов без credentials/query/
fragment, с нормализацией и дедупликацией. Выбор и добавление в UI/TUI меняют черновик,
POST отправляется по Save. Неизвестный активный URL остаётся доступен. Палитра TUI
ждёт загрузки существующего списка, чтобы не затереть его. Список имеет Live только
как список вариантов; активный `LOCALVOX_LLM_BASE_URL` остаётся Restart. Model/auth
задаются отдельно. API-подключение TUI этот список не меняет.

## Фактические проверки

Windows, CLI OpenSpec 1.13.2; Cargo `--locked --offline -j 2`, snapshots без обновления
при обычном прогоне. Временные Rust-файлы — в `target/tmp`, реальный архив не менялся.

| Команда / набор | Результат |
|---|---|
| `cargo test -p localvox-light-core --lib presets::` | 3 passed: полнота env-данных, приоритет файлов/fallback/I/O, запрет секретов, URL |
| core `settings::`, `cli::`, `env_file::` | 4 / 11 / 15 passed |
| `cargo test -p localvox-light --lib -- --test-threads=2` | 14 passed, 9 ignored: coordinator snapshots, recovery, завершение без postprocessing worker |
| `cargo test -p localvox-light-api --lib` | 76 passed, 6 ignored; settings HTTP повторён после окончательного bootstrap: 1 passed, 1 private fixture ignored |
| `cargo test -p localvox-light-client -p localvox-light-terminal` | client 3 unit + 8 transport; TUI 124 unit, 2 ignored; integration/ConPTY без ошибок; native media ignored |
| финальный `cargo test -p localvox-light-terminal --lib -- --test-threads=2` | 125 passed, 2 ignored, включая новый palette-load guard; 43 render tests включены |
| `npm --prefix ui test` | 10 passed |
| `npm --prefix ui run test:e2e` | 76 passed, включая 3 ожидаемых срабатывания visual detector; новый golden настроек, старые browser goldens сохранены |
| `npm --prefix ui run lint` | exit 0, 8 предупреждений существующих правил exports/ref cleanup |
| `npm --prefix ui run build` | TypeScript/Vite PASS |
| `scripts/run.ps1 -Tui -BuildOnly` | release PASS: демон, worker, TUI; приложение не запускалось |
| `rustfmt --check` для изменённых Rust-файлов; PowerShell Parser / `bash -n` installers | PASS |
| OpenSpec main specs / оба затронутых change, `--strict --no-interactive` | PASS; после sync CLI информирует, что CFG-005 уже есть в main, повторный archive требует skip-specs |
| `python scripts/check-openspec.py` | PASS: 15 capabilities, 88 requirements, 178 scenarios, 88 coverage rows |
| `git diff --check` | PASS |

Screenshot новых настроек просмотрен; TUI snapshots обновлены для намеренных
изменений и повторно проверены обычным прогоном. Сборочная поставка теперь включает
`.env.defaults`; install templates не закрепляют отдельные копии model/base URL.
Удалённый release CI и запуск installers не выполнялись. Сборка не является аппаратной
приёмкой. Подключение к реальному LLM, слышимость mpv, микрофон и одновременная запись
в UI/TUI этим этапом не проверялись. Пользователь перезапускает установленный комплект
самостоятельно; рабочий демон агентом не перезапускался.

CFG-001/005 синхронизированы в main specs 2026-11-02. Change остаётся активным;
связанный add-technical-journal-workspace не синхронизирован/не архивирован как
завершённый: его аппаратные 10.2/11.3 открыты.
