# Доработка TUI по замечаниям владельца — 2026-11-01

Реализация задач 11.1–11.7 в текущем рабочем дереве после `9118013`.
Это новые результаты; исторические проверки в [implementation](implementation.md)
не изменяются. Аппаратная задача 10.2 остаётся **not-run**.

## Поведение и существенные проверки

| Требования | Изменение | Выполненная проверка |
|---|---|---|
| TERM-002/007 | Сведения свёрнуты в начале «Ход», I раскрывает; metadata убрана из документов. y/Ctrl+Y копируют весь «Текст». | App-тест с 500 фрагментами, переключением записи и сбросом раскрытия; snapshots сведений, текста и узкого окна. |
| TERM-003/007 | i → вставка ссылки → Enter; вставка сама не отправляет запрос. Pre-roll убран из постоянной шапки. | App проверяет единственный Ingest с URL; настоящий ConPTY проверяет открытие формы, Unicode-вставку и API. |
| TERM-006/007, CFG-002 | Основные параметры и A для всего каталога, отдельные значение/подсказка/срок применения. Bool/device/provider выбираются из списка, Enter создаёт черновик, S сохраняет. | App проверяет выбор, сброс, сохранение legacy-значения при простом Enter, отсутствие произвольного bool/paste, секреты; snapshots 120×36 dark и 80×24 light плюс выбор. Общий UI/TUI smoke проверяет сохранение. |
| TERM-005/007 | g открывает числовые часы/минуты/секунды; ограничения полей и длительности. | App: клавиатура, недопустимая вставка и время за концом не создают команду; snapshot компактной формы. |
| TERM-005/008 | Пустая ранняя волна больше не кешируется навсегда. Повтор через 5 s, HTTP-отказ — через 30 s; появление длительности инвалидирует пустой результат. | App: пусто → появление аудио → повтор, без сброса backoff от неизменившихся metadata. Позиция доступна без peaks. |
| TERM-009, JRN-003/005, PIPE-011 | Нижняя консоль, unread/ack, контекст исходной операции/сессии/фазы. Повтор опроса свёрнут в +N с последним временем; Enter раскрывает. | Unit/App: просмотр, повтор без нового unread, восстановление → новый эпизод, новая явная попытка, разные фазы, поздний отказ прежней сессии, сохранность формы, ошибка очереди внутри успешного progress-ответа. |
| TERM-009 | Ограниченная история: 128 эпизодов, 32 времени повторов, 3000 символов сообщения; безопасные управляющие символы и авторизация. | Unit: переполнение, счётчик 50 отказов и последние 32 времени, Authorization/Bearer и ANSI. Snapshots folded/expanded, видимый индикатор на 80 столбцах. ConPTY: 401 в консоли → masked reconnect без повторного stop. |
| TERM-002, ARCH-001 | Отсутствующая summary — HTTP 200 с available=false, нейтральный экран UI/TUI. | Настоящий HTTP: отсутствующая сводка, неизвестная сессия, отказ чтения, готовый файл. Browser: нет глобального HTTP 400, «Текст» доступен. |
| TERM-005, ARCH-002 | Ошибки запуска/версии/IPC/медиа сохраняют причину; любой отказ больше не объявляется отсутствием mpv. | Unit missing executable; настоящий mpv с защищённым HTTP/Range: pause, seek, повтор фрагмента, смена сессии, media 404, восстановление. Вывод звука отключён. |

Код: [App](../../../../crates/localvox-light-terminal/src/app.rs),
[контексты](../../../../crates/localvox-light-terminal/src/app/diagnostics.rs),
[ограниченный журнал](../../../../crates/localvox-light-terminal/src/diagnostics.rs),
[консоль](../../../../crates/localvox-light-terminal/src/render/console.rs),
[ввод](../../../../crates/localvox-light-terminal/src/app/input.rs),
[mpv](../../../../crates/localvox-light-terminal/src/player.rs),
[summary API](../../../../crates/localvox-light-api/src/archive.rs).
Проверки: [App UX](../../../../crates/localvox-light-terminal/src/app/ux_tests.rs),
[снимки](../../../../crates/localvox-light-terminal/src/render/ux_tests.rs),
[ConPTY](../../../../crates/localvox-light-terminal/tests/pty_workspace.rs),
[native media](../../../../crates/localvox-light-terminal/tests/media_playback.rs),
[HTTP](../../../../crates/localvox-light-api/src/http/tests/workspace.rs),
[browser](../../../../ui/tests/e2e/workspace.spec.ts).

## Выполненные команды

Windows, locked dependencies, Rust `-j 2`, тестовые TEMP/TMP в `target/test-temp`
на D:. Обычные snapshot-прогоны используют `INSTA_UPDATE=no`; новые/изменённые
эталоны просмотрены как символы и стили. Проверки работают на временных данных.

| Команда из корня | Результат |
|---|---|
| `cargo test --locked -j 2 -p localvox-light-client -p localvox-light-terminal -p localvox-light-process -- --test-threads=2` | PASS: client 3 + transport 8, process 12 (1 ignored), terminal lib 119 (2 ignored), IPC 2, ConPTY cleanup 2 + workspace 2; native media по умолчанию ignored и выполнен отдельно ниже. |
| `cargo test --locked -j 2 -p localvox-light-terminal --lib -- --test-threads=2` | PASS: 120, 2 ignored; финальный повтор после защиты сохранённых legacy-значений и обновления справки. |
| `cargo test --locked -j 2 -p localvox-light-terminal --test pty_workspace -- --test-threads=2` | PASS: 2, 1.50 s; финальный повтор после переноса синхронной ошибки reconnect в console и завершения эпизода при успешном подключении. |
| `cargo test --locked -j 2 -p localvox-light-terminal --lib player:: -- --test-threads=2` | PASS: отдельная повторная проверка после упрощения классификации ошибки запуска. |
| `cargo test --locked -j 2 -p localvox-light-terminal --test media_playback -- --ignored --nocapture` | PASS: 1, 3.97 s; реальный mpv, null audio, HTTP/auth/Range, ошибка media и восстановление. |
| `cargo test --locked -j 2 -p localvox-light-api --lib -- --test-threads=2` | PASS: 76, 6 ignored; медийные и shared-client интеграции не выдаются за этот обычный прогон. |
| `npm --prefix ui run build` | PASS: TypeScript + Vite. |
| `npm --prefix ui test` | PASS: 10. |
| `npm --prefix ui run lint` | PASS: 8 прежних предупреждений, без ошибок. |
| `npm --prefix ui run test:e2e` | PASS: 74, 21.4 s; включает штатно ожидаемые отказы трёх visual-detector cases. |
| `cargo test --locked -j 2 -p localvox-light-api --lib -- --ignored --exact http::tests::cross_clients::shared_workspace --nocapture` | PASS: 1, 5.20 s; браузер и настоящий TUI с общим временным HTTP/архивом, без capture/моделей. |

Локальные логи сохранены в игнорируемых `target/tui-polish-*.log`, общий screenshot,
trace и terminal captures — в `ui/test-results/cross-real-api`. Это локальные
артефакты; таблица выше фиксирует фактические результаты без требования их коммита.

## Проверка конкретной установки

По запросу владельца прочитаны настройки/состояние локальных сервисов. Был выбран
Claude, хотя локальная Ollama доступна и `qwen3.5:9b` установлена. Через `/api/settings`
сохранены только `LOCALVOX_SUMMARY_PROVIDER=ollama` и
`LOCALVOX_LLM_BASE_URL=http://localhost:11434/v1`; имя модели сохранено.
Ответ указал **need_restart**. `.env` не включается в Git. Работающий coordinator
использует прежний startup snapshot до штатного перезапуска демона.

Установленный `target/tools/mpv/mpv.exe` сообщил v0.41.0-dev-g9e2ed65eb.
У пользовательской записи `20261101_203558_youtube` запрос peaks вернул HTTP 200,
160 точек и 53.6135 s: backend умеет строить эту волну. В клиенте исправлен
воспроизводимый дефект раннего пустого ответа, а не заявлен отказ построения на сервере.

**Первоначальный отказ слышимого воспроизведения у владельца не воспроизведён.**
Ранее любой сбой давал одинаковое «нужен mpv», поэтому точная причина не сохранена.
Новая консоль сохраняет её, native-тест проверяет работающий установленный mpv и
отказ/восстановление HTTP. Задача 11.3 остаётся открытой в части исходного инцидента;
слышимость и устройства не подтверждены. Демон не перезапускался; старый работающий
release ещё не содержит нового summary API. [Ручная приёмка](../../../../docs/manual-acceptance.md)
остаётся not-run, sync/archive change не выполняется.

## Сборка и форма документов

- `cargo check --locked -j 2 -p localvox-light --no-default-features` — PASS.
- `cargo build --locked -j 2 --release -p localvox-light-terminal --bin localvox-tui` — PASS; `target/release/localvox-tui.exe` обновлён.
- `target/release/localvox-tui.exe --help` — PASS, release binary запускается.
- `cargo fmt -p localvox-light-api -p localvox-light-client -p localvox-light-terminal -- --check` — PASS.
- Main specs strict — PASS, 15 capability; selected change strict — PASS.
- `python scripts/check-openspec.py` — PASS: 15 capabilities, 87 requirements, 175 scenarios, 87 coverage rows. Новые TERM-ID остаются в change до sync.
- `git diff --check` и whitespace новых файлов — PASS; LF/CRLF-предупреждение Git не является дефектом содержимого.

Browser screenshot и terminal captures общего прогона просмотрены. Изменённые старые
эталоны отражают шапку без pre-roll, подсказки копирования/console и перенос metadata;
новые эталоны отдельно фиксируют settings, choice, seek, folded/expanded console,
нейтральную summary, metadata и видимый unread-индикатор. Сборка TUI не обновляет
работающий daemon: полный комплект нужно собрать после его штатного закрытия.
