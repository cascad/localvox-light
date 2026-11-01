# Реализация нового рабочего пространства

2026-10-31, Windows. Пользователь разрешил начать apply. Проверки используют синтетические данные; работающий пользовательский демон/архив не меняются.

## Клиент и новый TUI

- `cargo check -p localvox-light-client -p localvox-light-terminal` — PASS.
- `cargo tree -p localvox-light-terminal --depth 1` — PASS. Клиент/terminal не зависят от core, API-server, capture, Vosk или ONNX; PTY входит только в dev-dependencies.
- `cargo test -p localvox-light-client -p localvox-light-terminal -- --nocapture` — PASS: 2 client + 6 terminal unit/render tests. Проверены encoding URL, недопустимые токены/управляющий текст, поздний ответ старой сессии, вставка Unicode, отмена удаления, ожидание подтверждения записи, stop/выход при 160×48/120×36/80×24/30×8.
- Это первые проверки каркаса, не полный паритет P01–P30. PTY, полный HTTP-контракт, плеер и visual snapshots ещё не приняты; соответствующие задачи открыты.

## Инфраструктура UI

- Установлен локальный Playwright 1.64.1; `npm run build` — PASS до изменения визуала.
- Эти команды не проверяют внешний микрофон, физическое воспроизведение или качество моделей.

## Последующие проверки реализации — 2026-10-31

Рабочее дерево, Windows x64, временные каталоги/loopback/синтетические документы.
Работающий пользовательский демон не запускался повторно, архив не менялся.
Все новые требования остаются delta; полный P01–P30 пока не принят.

| Команда | Фактический результат | Граница |
|---|---|---|
| `cargo test -p localvox-light-client -p localvox-light-terminal` | PASS: 2 client unit, 7 transport, 38 terminal unit/style/snapshot, 1 I/O responsiveness, 1 ConPTY | Настоящий binary и ОС-терминал; модель и микрофон не используются. Media acceptance в обычном запуске явно ignored. |
| `cargo test -p localvox-light-core journal:: --lib -- --nocapture` | PASS: 12 тестов | Cursor replay, partial tail, before, reset, giant/bad строки, allowlist/управляющие символы, квота с сохранением ошибок, append fault, concurrent writers, принадлежность операции исходному run. |
| `cargo test -p localvox-light-process --test lifecycle` | PASS: 12, 1 ignored helper | Настоящие процессы: output limit, partial/live spool, полный sink, timeout/cancel, nested/abrupt owner/descendants, managed tool ownership. |
| `npm --prefix ui run build` | PASS после подключения локального IBM Plex Mono | Сборка; не e2e. Шрифт и лицензия поставляются локально, APP_CSP разрешает только self fonts. |
| `npm --prefix ui run test` | PASS: 6 | Идентичность страниц, run, ровно 35 строк, item/byte cache budgets и сохранение запрошенной прошлой страницы. |
| `npm --prefix ui run test:e2e` | PASS: 8, включая 3 ожидаемых отказа визуального сравнения | Chromium/Windows, фиксированные clock/font/data; 1440/800/480, stop, журнал, API error. Намеренно испорченные символ/цвет/геометрия действительно дают diff. Это ещё не полный browser-паритет. |
| `npm --prefix ui run setup:mpv` | PASS: официальный first-party build, SHA-256 проверен | Только `target/tools/mpv`, без изменения PATH и установки моделей. |
| `cargo test -p localvox-light-terminal --test media_playback -- --ignored --nocapture` | PASS: 1 настоящий mpv + временный HTTP + generated PCM | Header auth, far Range seek, pause, двукратный тот же fragment и остановка в его конце. `--ao=null`: слышимость на устройстве не проверяется. Сервер синтетический; не объявляется сквозной приёмкой настоящего daemon/API-архива. |

Новый TUI содержит отдельные app/update/render, формы с Unicode/caret/paste,
палитру и ограниченный I/O. Проверены stale A→B для чтения **и mutation ack**,
401/403/503, timeout/lost-ack без повторного POST, сохранность последних данных,
восстановление черновика после ошибки, независимый от model-запросов stop.
Если во время ошибки открыта новая форма, прежняя сохраняется в отдельной очереди.
Копия документа исключает original/provenance; экспорт не перезаписывает файл.

Settings edits отправляют только изменённые ключи и остаются при ошибке.
Ответ need_restart виден; серверная метка применения не объявляется фактом обновления
snapshot. RV-001/010 остаются серверными зависимостями; unknown settings values
новый клиент отбрасывает. Паритет настроек/participants требует ещё полного API/e2e.

Ratatui: шесть просмотренных и принятых style snapshots 160×48, 120×36, 80×24,
оба theme. Отдельный meta-test проверяет чувствительность сериализации Buffer к
символу, цвету и размеру. Проверены ASCII borders, 16-color и monochrome.
Playwright `.png` просмотрены после подключения локального шрифта; обычный запуск
имеет `updateSnapshots: none`. Workflow `.github/workflows/interface-tests.yml`
добавлен с сохранением diff/trace/snap.new; удалённый CI ещё не запускался.

### Измеренные пределы

`large_history_poll_and_payload_are_bounded`: файл **34 120 000 bytes**, один хвост
основного progress читает **1 048 609 bytes**, JSON ответа **257 727 bytes**,
idle continuation читает **33 bytes**. Производная метаинформация читается отдельно,
до 4 KiB. Кеши обоих клиентов проверены на 5000 событий / 4 MiB. Это измерение
нового cursor-reader, а не benchmark ASR/LLM или всех legacy endpoint.

Real media: first-party `mpv-v0.41.0-dev-g9e2ed65eb-41103006513-x86_64-w64-mingw32-full.zip`,
archive SHA-256 `24a5ac969219767c617f5d109761d4e24db8ba05dee30db485ea756415dc63b4`.
WAV генерируется потоком: час mono/16 kHz/16 bit, 115 200 044 bytes.
В последнем прогоне трёх загрузок сервер отдал **38 092 800 bytes**; Range offsets
`[0, 57600044, 0, 383020, 0, 383020]`. Проверяется старт до загрузки половины файла
и суммарное чтение трёх seek меньше одного полного файла. Буферизация TCP/демультиплексора
не выдаётся за прочитанный PCM; точный byte count зависит от ОС.
Перед header auth Windows backend проверяет реальный server PID и owner-only DACL;
тесты также отвергают чужой endpoint; отдельно проверено удаление LOCALVOX_API_TOKEN
из унаследованного окружения backend.

Один одновременный запуск media/process stress-suite дал startup/timeout failures.
По отдельности оба прогона прошли; CI выполняет их последовательно. Слишком короткий
3s mpv preflight заменён на ограниченные 10s с обычным приоритетом и cancellation
при закрытии клиента. Не заявляется аппаратная приёмка или проверка других ОС.

Руководства: [journal](../../../../docs/technical-journal.md),
[новый TUI](../../../../docs/terminal-workspace.md).

### HTTP и чтение журнала: дополнительный срез

- `cargo test -p localvox-light-api journal_http_ --lib -- --nocapture` — PASS: 2.
  Настоящий `handle_request`, loopback socket и temp archive: after/before/limit,
  одинаковое время, повтор страницы, partial newline, reset, giant/bad record,
  401 и Host/Origin 403, совместимость `/progress`.
- `npm --prefix ui run test:e2e` — PASS: 10 после просмотра новых светлых PNG:
  dark/light/system, reduced-motion, keyboard toggle, 480px sidebar сохраняет
  видимую запись архива и stop. Три намеренные визуальные мутации дают ожидаемые отказы.
- `npm --prefix ui run test:e2e -- --grep "arrivals|previous journal"` — PASS: 2.
  Новые строки при чтении, сохранение раскрытия/позиции, фильтр, 503/recovery,
  предыдущая страница и инертный HTML. Якорь привязан к читаемой строке/ID,
  поскольку run/stage headings переходят к началу предыдущей страницы.

Новые проверки ещё не означают полный UI-паритет P01–P30 или аппаратную приёмку.

### Продолжение 2026-10-31: чтение, формы, ConPTY и настоящий media API

- `cargo test -p localvox-light-core journal:: --lib -- --nocapture` — PASS: **15**.
  Дополнительно: первая partial-запись не меняет поколение при дописывании;
  замена файла с тем же содержимым даёт reset; giant legacy at/run не зацикливает cursor.
  Файл 34 120 000 bytes: tail-read **1 048 609**, JSON **257 761**, idle-read **33**.
  Изменение размера JSON относительно предыдущего прогона — отпечатки cursor.
- `cargo test -p localvox-light-client -p localvox-light-terminal` — PASS:
  **2** client unit, **8** transport, **60** terminal unit/style/snapshot,
  **2** responsiveness HTTP, **1** ConPTY. Один ignored helper реально запущен
  вложенным ConPTY-тестом; media в этой команде отдельно ignored.
- ConPTY исполняет настоящий binary против synthetic HTTP: открывает журнал,
  раскрывает 35 строк, вставляет русский URL с q/r/i, меняет размер до 80×24,
  посылает F8 внутри редактора, закрывает клиент при новой серверной записи.
  Повторного stop/shutdown нет. Отдельный helper вызывает ошибку после инициализации
  терминала и подтверждает восстановление raw mode/paste/alternate screen.
- Просмотрены и приняты **10** style snapshots: 6 документов (оба theme, три размера)
  и 4 журнала (collapsed dark/light, реальные раскрытые строки ASCII16 на 80×24,
  видимая ошибка при свёрнутом monochrome). Нормальный прогон прошёл без обновления.
- `npm --prefix ui run test` — PASS: **7**. Rust/TS/browser используют общий journal fixture.
- `npm --prefix ui run test:e2e` — PASS: **17**, включая три ожидаемых отказа
  намеренных визуальных мутаций. Проверены late before/after при reset, reading anchor,
  последовательный slow poll, очередь с несколькими running, порядок/stuck/error,
  переход activity→queue→journal, все состояния этапов, неизменный процент при heartbeat,
  готовый STT при failed Summary. Последние строки сохраняются после 503.
- `npm --prefix ui run build` — PASS после выделения `usePolling`.

Нативный I/O разделён на пять bounded workers; два настоящих HTTP-теста удерживают
модельные запросы и чтение документов заблокированными, пока stop/save остаются доступны.
Формы поиска/чата и backpressure сохраняют ввод. Проверены фильтр/локальная отмена
settings, отсутствие override от неизменённого поля, повтор peaks после stale context
и backoff ошибки, инвалидирование старых чтений после подтверждения документа.
Состояние/формы/текст документов выделены в отдельные модули нового TUI.

### Потоковое аудио и backend

- `cargo test -p localvox-light-api --lib` — PASS: **46**, **3** ignored
  (исторический архив и две явно запускаемые реальные media-проверки).
  Сохранены проверки clip, saturation mix, failed Summary, локальных имён,
  worker lease и Text/Summary без аудио. HTTP journal auth/cursor — также PASS.
- `cargo test -p localvox-light-api media_http_range_plays --lib -- --ignored --nocapture`
  — PASS: **1**. Настоящий `handle_request`, temp archive с часовым WAV
  **115 200 044 bytes**, Bearer, 401 без токена, Range `57600044–57600143`,
  настоящий mpv: seek 1800s, длительность, остановка fragment, настоящие peaks.
- `cargo test -p localvox-light-api flac_range_is_sample_accurate --lib -- --ignored --nocapture`
  — PASS: **1**. Настоящий ffmpeg создаёт FLAC из synthetic PCM; интервалы
  `[1,19000)`, `[8001,9601)`, `[17999,20001)` совпадают побайтно с исходными сэмплами.
- `cargo test -p localvox-light-terminal --test media_playback -- --ignored --nocapture`
  — PASS: **1**, расширен до настоящего worker и смены сессии. HTTP/IPC auth,
  far seek, pause, одна реплика дважды, смена `path` на другую сессию, 404 audio,
  сброс ошибки через stop, worker demo→other→idle→stop. Последний прогон:
  **48 660 480 bytes**, Range offsets `[0,57600044,0,383020,0,383020,0,0,3199020,0,639020]`.
- `cargo test -p localvox-light-terminal --lib player::` — PASS: **4**.
  Отсутствующий backend показывает ошибку именно выбранной B; stop восстанавливает
  idle. Остальные проверки — restricted IPC, bounded frame и удаление token env.

Найденный media-дефект исправлен в `archive/media.rs`: HTTP больше не собирает весь
открытый диапазон перед ответом. Unit проверяет ленивый start и PCM-окна **≤1 MiB**,
нечётные byte ranges и small reads через границу чанков. Это предел одного PCM-окна,
не всего RSS: mixing, metadata, HTTP, supervisor и ffmpeg имеют собственные расходы.
RIFF больше допустимого u32 размера отклоняется до выделения тела. FLAC decode ограничен
интервалом, 30s и output budget, процесс принадлежит supervisor.

Все media-прогоны используют null audio. Микрофон, слышимость, LAN, другие ОС,
удалённый CI, полный P01–P30 и установка release остаются непроверенными.
Известные RV/BL не закрываются этими тестами. Задачи 9.1–9.3 ещё не выполнены.

### Реальный контракт действий и подтверждение

`cargo test -p localvox-light-api http::tests::workspace --lib -- --nocapture`
запущен для первых трёх HTTP-сценариев — PASS: **3**. Затем отдельно выполнены
`confirmation_http` и `session_deletion_requires` — PASS по **1** каждый.
Ни работающий демон, ни пользовательский архив, ни модели не используются.

- Настоящие versions/best/summary/readable/transcript; best не создаёт LLM job.
  Локальные имена source/voice сохраняют речь/таймкоды и readable artifact;
  worker lease даёт отказ до изменения. Summary/Text работают без аудио,
  All и смена языка при недоступном исходнике отказывают без инвалидирования.
  Import возвращает ID; очередь сохраняет порядок.
- Обнаружено и исправлено несовпадение jobs.running: сервер возвращает **число**,
  старые UI type/fixture и новый клиент считали boolean. Fixture исправлен,
  typed-клиент читает настоящий API; browser проверяет 2 running + 1 pending +
  1 failed = badge 4. `npm --prefix ui run test:e2e` — повторно PASS **17**.
- Default note не отправляет пустую строку как имя слота. Реальный HTTP проверил
  default destination, Unicode, чтение и удаление по raw+source+slot.
- File ask: отсутствующий/не UTF-8/пустой/>1 MiB/>400000 chars отвергаются до POST;
  UTF-8 BOM снимается, имя файла и provider сохраняются. Синтетические обновления
  сохранённого ask читаются как running/done/failed; это не запуск модели.
- Дополнительный просмотренный style snapshot `ask_failed_source` фиксирует
  failed с символом/цветом и доступный сохранённый материал. Enter в документе
  раскрывает источник; возврат в раздел читает прежний ID.

Полный client/terminal-прогон сначала прошёл **64** unit/style/snapshot (и 2+8 client),
но обнаружил timeout contention HTTP-теста; общий запуск тогда **не был успешным**.
Диагностика показала, что пятый keep-alive запрос control не попадал в обработчик,
пока другой socket не закрылся. В новом клиенте установлено `Connection: close`.
Сервер fixture читает тело запроса; он не подменяет этот эффект ответным close.
После исправления `cargo test -p localvox-light-terminal --test io_responsiveness`
— PASS **2**, оба дешёвых действия завершаются до release долгих запросов.
Это проверка конкретного HTTP/client contention, не обещание решения всех перегрузок API.

**RV-016 / LLM-006 исправлен в исходниках:** processing::confirm возвращает
`Result<bool>`; API различает «нет сомнения» и отказ checksum/read/write.
Session worker lease проверяется до изменения. Windows test открывает ledger с
FILE_SHARE_READ, запрещая atomic replacement; ошибка сохраняет прежнюю пометку.
После снятия запрета подтверждение действительно сохраняется. Пройдены:

- `cargo test -p localvox-light-core processing:: --lib` — PASS **8**.
- `cargo test -p localvox-light-core artifacts:: --lib` — PASS **9**.
- `cargo test -p localvox-light-api confirmation_http --lib -- --nocapture` — PASS **1**.
- `cargo test -p localvox-light-api session_deletion_requires --lib` — PASS **1**.
- `cargo check -p localvox-light -p localvox-light-api -p localvox-light-terminal`
  — PASS перед последним confirm-изменением; окончательная сборка фиксируется отдельным прогоном.

Исторические RV-016 наблюдения не выданы за новые тесты. Серверные RV-001/010,
RV-008/009 и остальные известные gaps остаются. Рабочий демон не перезапускался.
Текущая сверка всех строк — в [parity](../parity.md#текущая-сверка-2026-10-31).
## Дополнение: поиск, заметки и совместимость API, 2026-10-31

Последующий прогон `cargo test -p localvox-light-api http::tests::workspace --lib -- --nocapture`:
**8 passed**, 49 filtered. Настоящий временный HTTP/архив проверил lexical/semantic/hybrid,
ошибки обоих каналов, сохранение lexical при недоступном semantic, citations и отсутствие
LLM-вызова без фрагментов. Endpoint модели/embedding синтетический, inference не выполнялся.
RV-008 исправлен в `search_mode`: обе ошибки больше не становятся пустым результатом.
Исторические записи выше/ниже о найденном дефекте сохраняют статус своего прогона.

Заметки: default/явный слот, raw/source/slot delete, читаемый пустой слот,
отказ записи (родитель — файл) и ошибка чтения не-UTF-8 файла. Первый вариант
fault fixture ошибочно считал каталог отказом (каталоги поддерживаются как назначения);
второй ошибочно ожидал read error у отсутствующего файла. Эти два прогона не прошли;
последний использует независимые, воспроизводимые write/read отказы и прошёл.
TUI app/render: pending/empty/error поиска, Enter к timecode, сохранение query/draft,
ошибка доставки с сохранением slot/text, нечитаемый/error/empty slot и отбрасывание
route для изменённого текста. `cargo test -p localvox-light-terminal --lib -- --skip snapshot_`:
**57 passed**, 1 ignored helper, 12 filtered (до добавления launcher).
Руководство: [terminal workspace](../../../../docs/terminal-workspace.md).

`cargo test -p localvox-light-terminal launcher:: --lib -- --nocapture`:
**5 passed**, 1 ignored child helper, 70 filtered, 10.07 s. Проверены совместимый уже
работающий API, старый/чужой/неполный protocol, 401/403, remote/HTTPS без autostart,
одна попытка запуска собственного fixture process, успешная готовность, ранний exit,
ограниченное ожидание и жизнь daemon fixture после drop клиента. Рабочий демон не трогали.
Первый прогон обнаружил неверный TCP timeout 150 ms на Windows; итог использует причину
ConnectionRefused из HTTP transport и общий deadline. Workspace requests разрешаются
только после health service/protocol/capabilities; авторизация health сохранена.
Полный ConPTY/reconnect после интеграции launcher ещё не прогнан.

## Последующий прогон: UI, настройки и миграция запуска

Это последующие результаты 2026-10-31; прежние not-run и неудачные прогоны выше
сохраняют историю. Рабочий пользовательский демон, .env и архив не изменялись.

- `npm run build` — PASS после новых controls/notes/recorder/player.
- `npm run test:e2e` — **31 passed**, 10.5 s. Включены три expected failures
  визуального detector (symbol/color/geometry); обычные PNG не обновлялись.
  [Тесты](../../../../ui/tests/e2e/workspace.spec.ts) покрывают P01–P30 через mock HTTP:
  версии с фактическим ID, язык/confirm, локальные имена/сброс источника, три scope,
  принятый import ID, notes raw/source/slot/delete/error/draft/stale route,
  asks Unicode file/provider/history/pending/error/copy/delete, поиск трёх режимов,
  чат/источники, clean copy/provenance/checksum error, settings типы/unset/restart,
  browser 401/masked local token/clear, поздний документ A→B, recording accepted/confirmed
  и stop до подтверждения. Нативный Chromium действительно декодирует синтетический
  PCM/WAV: play/pause/keyboard bounds/15s/repeated fragment. HTTP перехвачен Playwright,
  timeupdate конца фрагмента подан тестом; это не проверка физического звука или моделей.
- `npm run test` (эквивалент ранее выполненного npm test) — **8 passed**:
  общий workspace/journal fixture и reducers/bounds.
- `cargo test -p localvox-light-api --lib` — **55 passed, 4 ignored**, 1.31 s:
  реальные временные HTTP/архив, поиск/grounding с synthetic model, confirm I/O,
  settings. Ignored — real FLAC, real mpv, historical archive copy и child settings helper;
  helper запускается своим родительским тестом, не пропускается фактически.

RV-001 исправлен узко по существующему CFG-002: каталог больше не обещает Live
для startup Config/child_env; unknown key также сообщает need_restart. `cargo test
-p localvox-light-core settings:: --lib` — **4 passed**. `cargo test -p localvox-light
--bin localvox-light` — **3 passed**: старый coordinator/команда сохраняют старую модель,
новый Config читает новую; server/maintenance/no-tui не выбирают клиент. Реальный
model job не запускался: проверены сам Config и передаваемое command environment.

RV-010 исправлен на сервере: extra только key/set. Новый [settings HTTP test](../../../../crates/localvox-light-api/src/http/tests/settings.rs)
копирует test executable в tempdir и запускает его с собственным .env/cwd; GET/POST
используют настоящий router/client. Проверены secret null/set, отсутствие unknown value
на wire, все типы каталога, bool/number/null, forbidden PATH, сохранение комментария,
unknown restart и отказ записи до apply (model остаётся `after`). Production .env
не переопределяется тестовым параметром. UI дополнительно отбрасывает extra.value
старого демона; TUI уже не декодировал его. Local-first/outbox RV-007 и ASK RV-009 не закрыты.

`cargo test -p localvox-light-terminal --test pty_workspace -- --nocapture` —
**2 passed**, 1.50 s. Реальный ConPTY проверил 401→masked token→reconnect,
отсутствие токена в capture и отсутствие replay stop; исходный paste/resize/quit тоже PASS.
Первый новый прогон ошибочно ожидал ASCII `*`, renderer маскирует `•`; исправлен selector.

`cargo test -p localvox-light-terminal --test run_script` — **2 passed**, 1.60 s:
8 изолированных PowerShell случаев (Cargo/exe fixtures), включая BuildOnly, dev/release,
Tui/Desktop/default, conflict до Cargo, build failure, missing artifact и разные каталоги.
Actual localvox-tui.exe запускает --version из пустого temp bundle без core/models/DLL.
Первый прогон выявил имя CLI package вместо binary; имя закреплено как localvox-tui.
Совместный `cargo build --locked --profile dev -p localvox-light -p localvox-light-asr
-p localvox-light-terminal --bin localvox-light --bin localvox-process --bin localvox-tui`
прошёл (до удаления старого crate); полный обновлённый комплект ещё перепроверяется.

Новый `--tui` делегирует соседнему клиенту до модели/capture/engine. Старые четыре
файла crate удалены из workspace; feature tui сохранён как пустой compatibility switch.
`cargo test -p localvox-light --test launcher_modes -- --ignored --nocapture` —
**1 passed**, 0.88 s: actual main→actual new TUI в собственном copied bundle/ConPTY,
явный и интерактивный запуск при заведомо отсутствующей модели, готовый private API,
quit без POST/stop/shutdown и без создания audio-dir. Это не второй реальный engine.
`cargo check -p localvox-light --no-default-features` — PASS.

`cargo test -p localvox-light-terminal render::tests --lib` — **21 passed**:
дата/длительность/loading/empty/error/Unicode при 160×48, 120×36, 80×24 и все style snapshots.
8 ожидаемых diff были просмотрены: дата во второй строке, расширение selection background,
приглушённая загрузка пустой ещё не прочитанной панели. Затем .snap.new приняты явно;
автоматическое обновление normal/CI не включалось. Mixed catalogue validation TUI —
отдельный **1 passed** (number/bool/options/secret/default/changed keys с подтверждением).

CI дополнен этими проверками; удалённый runner, live LAN/macOS/Linux, физические устройства
и единый UI+TUI shared HTTP сценарий здесь ещё не выполнялись. Требования 10.1/10.2
остаются открытыми. Рабочий release не заменён.

## Общий настоящий API: UI и TUI одновременно

Последующий `cargo test -p localvox-light-api --lib -- --ignored --exact
http::tests::cross_clients::shared_workspace --nocapture` — **1 passed**, 4.31 s
(после расширения отказами/reconnect). [Rust harness](../../../../crates/localvox-light-api/src/http/tests/cross_clients.rs)
и [browser script](../../../../ui/tests/integration/actual-api.mjs). Комплект запускается
из copied executable с private cwd/.env/архивом и своим loopback-портом; browser и
ConPTY читают один настоящий HTTP API. Дерево Node/Chromium/fixtures под supervisor.

Пройдены: фактический принятый import ID → 35 journal details → сохранённые версии,
STT/readable/summary с receipts → lexical search и реальный browser WAV decoder →
file note, видимая одновременно в TUI → asks/provider/input/сохранённый ответ →
settings/restart → отказ PATH без записи в .env → 401 после очистки browser token и
повторная авторизация → TUI reconnect/120×36→80×24/quit, после которого API и архив доступны.
Обработка/ответ asks сделаны **явным synthetic producer**, без ASR/LLM и без capture.
Это проверка клиентов/HTTP/сохранённых артефактов, не benchmark, модельное качество
или физический звук. 10.1 закрыт в этих заявленных границах; 10.2 остаётся not-run.

Были исправлены ошибки harness: exact text selector включал citation в summary,
неверный исходный focus TUI и отсутствие gate для сохранённого ask. Они не выдаются
за найденные дефекты продукта. Дополнительно screenshot ожидал окончание загрузки;
артефакт с промежуточной загрузкой не является принятым visual baseline.
Итоговые captures/trace находятся в `ui/test-results/cross-real-api/`, CI их загружает.
Сценарий добавлен как `npm --prefix ui run test:integration`; remote CI не запускался.

Полный последующий клиентский прогон `cargo test -p localvox-light-client
-p localvox-light-terminal -p localvox-light-process` — **104 passed** суммарно:
client 10, process lifecycle 12, terminal lib 76, I/O 2, ConPTY 2, run_script 2.
Ignored — три subprocess helper и отдельный реальный mpv smoke; helper выполняются
родительскими тестами. Все 12 terminal snapshot/style cases прошли без auto-accept.

## Завершающая сверка

После удаления старого crate итоговая совместная dev-сборка daemon + worker +
localvox-tui с `--locked` — PASS, 8.41 s. Физические устройства не запускались.
Повторный общий UI+TUI smoke после очистки старых failure-артефактов и проверки
graceful закрытия Chromium — **1 passed**, 4.80 s; готовый browser screenshot и
terminal captures просмотрены. Источник URL сохранён в meta.source, согласно wire-схеме.

При завершающем чтении settings найден ещё один отказ CFG-001: `env_file::save_to`
подавлял ошибку чтения как пустую строку. Теперь пустой исходник допустим только при
NotFound, другой read error возвращается до записи/apply. `cargo test
-p localvox-light-core env_file:: --lib` — **15 passed**, включая Windows файл,
которому разрешена запись, но запрещено чтение новым handle: исходный файл сохранён.

Проверки формы: main specs strict — **15 passed**, change strict — PASS;
`python scripts/check-openspec.py` — PASS: 15 capabilities, 86 requirements,
168 scenarios, 86 coverage rows, 960 local links. `git diff --check` — PASS.
76 новых файлов проверены `git diff --no-index --check` с `cr-at-eol`, 0 issues;
исходный пользовательский engineering-doctrine.md исключён и не менялся. Найденные
две пустые строки EOF в новых документах убраны. Временный запуск с принудительным
autocrlf=false без cr-at-eol ошибочно трактовал CRLF как whitespace; это не результат
обычного repository diff check.

Не sync/archive: единственный оставшийся пункт реализации — **10.2 not-run**,
реальный микрофон/слышимость и оба клиента при фактической записи. ASR/LLM benchmark
не выполнен и остаётся отдельным BL; процессные/медийные проверки не подменяют его.
Рабочий демон/архив и установленный release не изменялись.

## Подготовка аппаратной приёмки — 2026-11-01

Подготовлен [чек-лист текущей сборки](../../../../docs/manual-acceptance.md):
сборка полного комплекта, отдельный D:-архив, реальные источники звука, UI/TUI,
воспроизведение, выход клиента при записи, отмена/продолжение и длинный материал.
Команды/клавиши сверены с run.ps1, CLI и обработчиками; исправлена подсказка US-012
(`y` вместо Ctrl+Y). Это документация, аппаратный прогон не выполнялся: задача 10.2
остаётся открытой, M01–M11 имеют статус not-run. Прежние результаты выше не изменены.

## Доработки по замечаниям владельца — 2026-11-01

Новый срез и его проверки записаны [отдельно](usability-2026-11-01.md). Console с группировкой повторов, сведения, настройки, seek, waveform retry и отсутствие summary реализованы; справочник обновлён. Прежний вывод об одной оставшейся 10.2 относится к состоянию до замечаний. Сейчас открыты аппаратная 10.2 и исходный невоспроизведённый отказ mpv в 11.3; старые результаты не переименованы.
