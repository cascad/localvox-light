# Отказ чтения очереди · 2026-11-01

Продолжение проверки фоновой обработки по поручению владельца. Требования:
PIPE-002/011. Пользовательские jobs.json, архив, .env, устройства и работающий
демон не изменялись. TEMP/TMP тестовых процессов находятся в target/test-temp на D:.

## Исходные отказы

Два новых API-теста выполнены до исправления: **0 passed, 2 failed**.
Настоящий GET /api/queue при повреждённом JSON вернул 200 вместо 503; respond
для удалённого клиента и каталога вместо jobs.json также вернул 200.
Лог: target/queue-read-red.log. Причина установлена чтением кода: load_error
сохраняется в JobQueue, но Archive queue/jobs_status игнорировали его.

Отдельно воспроизведён stale status TUI: после read error постоянный App.status
содержал «Ошибка», даже когда ресурс затем восстанавливался. Новая проверка
`queue_read_errors_retain_snapshots_and_clear_only_on_their_own_recovery`
с ожиданием отсутствия постоянной копии ошибки дала **FAIL** до исправления
(target/queue-read-tui-red.log).

## Реализация и существенные проверки

- [Archive](../../../../crates/localvox-light-api/src/archive.rs): queue/jobs_status
  возвращают Result через проверенный загрузчик. QueueUnavailable сохраняет причину
  и тип ошибки; [HTTP](../../../../crates/localvox-light-api/src/http.rs) распознаёт
  её через downcast до эвристик по тексту и возвращает 503. Удалённый клиент получает
  прежнюю общую формулировку недоступности без внутренних деталей.
- [HTTP/client tests](../../../../crates/localvox-light-api/src/http/tests/workspace.rs):
  missing queue без создания файла, два невалидных JSON без замены байтов, I/O-отказ
  при каталоге вместо файла без его удаления, реальное восстановление running/pending/
  failed в прежнем порядке и правильные счётчики, затем валидная пустая очередь.
  Отдельно проверено редактирование причины для удалённого клиента. **2 passed**
  после исправления (target/queue-read-http.log).
- [UI](../../../../ui/src/App.tsx), [RecordBar](../../../../ui/src/RecordBar.tsx):
  jobs error не теряется, индикатор открывает прежнюю очередь, прошлые счётчики
  подписаны «последние данные», начальный отказ не рисует выдуманные нули.
  У nav виден `!` также при ширине 480, кнопка остановки записи остаётся доступна.
  Новые browser cases проверяют начальные queue/jobs ошибки, stale counts/recovery;
  старый queue-error case дополнен восстановлением строк без повторного действия.
- [TUI apply](../../../../crates/localvox-light-terminal/src/app.rs) хранит read error
  только у ресурса, Action/Control/Route сохраняют постоянный результат. Успешный
  GET не стирает отказ команды. [App tests](../../../../crates/localvox-light-terminal/src/app/tests.rs)
  проверяют сохранение snapshot и независимое снятие Jobs/Queue errors.
- [TUI render](../../../../crates/localvox-light-terminal/src/render.rs): при jobs
  error верхняя строка сообщает недоступность вместо нулей; существующий список
  сохраняет строки с отметкой ошибки. Новый render test проверяет пустой/заполненный
  случай, текст и цвет header, отсутствие «Пока пусто», возврат счётчиков после успеха.

## Визуал и тестовый ритм

Новый эталон `queue-unavailable-dark-win32.png` создан только для намеренно нового
error badge и просмотрен. После просмотра убрано случайное разбиение заголовка:
badge содержит заголовок и обрезаемую строку прошлых данных, полный текст доступен
в title. Прежние PNG не обновлялись.

Первый полный browser-прогон: 60 passed, 2 failed. Отказы — старый native audio
test ожидал точные 20 секунд сразу после click Pause, не дождавшись native pause/
обновления кнопки; light keyboard screenshot зависел от ещё не выполненного
initial follow-to-tail requestAnimationFrame. Исправлена синхронизация fixtures:
явное подтверждение паузы и выполнение initial virtual frame до раскрытия журнала.
Это не изменение прежних визуальных эталонов или ослабление сравнения.

## Проверки

| Команда | Результат |
|---|---|
| `npm --prefix ui run build` | PASS: TypeScript и production bundle |
| `npm --prefix ui test` | PASS: 9/9 |
| `npm --prefix ui run lint` | PASS: прежние 8 warnings, новых нет |
| `cargo test --locked -j 2 -p localvox-light-api -p localvox-light-terminal -- --test-threads=2` | API: 61 lib + 3 integration passed, 6 ignored. TUI до уточнения постоянного status: 91 lib + 6 integration passed, 3 ignored; последующий прогон фиксируется отдельно |
| `cargo test --locked -j 2 -p localvox-light-terminal -- --test-threads=2` после исправления status | PASS: 91 lib + 6 integration (launcher delegation, ConPTY reconnect/workspace, clean bundle/run script), 3 ignored; обычные символы/стили/snapshots сохранены, INSTA_UPDATE=no |
| Целевая повторная проверка native audio и light keyboard | PASS: 2/2; прежние PNG не обновлены |
| `npm --prefix ui run test:e2e -- --workers=2` итоговый | PASS: 62/62, 33.3 s. Три намеренных визуальных повреждения по-прежнему дают ожидаемые отказы. Единственный новый PNG — просмотренный queue-unavailable-dark |
| `cargo test --locked -j 2 -p localvox-light-api --lib -- --ignored --exact http::tests::cross_clients::shared_workspace --nocapture` | PASS: 1/1, 7.43 s, 66 отфильтрованы; новый production UI и текущий TUI с настоящим временным API/архивом |
| `openspec validate --specs --strict --no-interactive` | PASS: 15/15; прежние информационные замечания о длине требований |
| `openspec validate fix-queue-read-failures --type change --strict --no-interactive` | PASS: CLI 1.13.2 |
| `python scripts/check-openspec.py` | PASS: 15 capability, 86 main требований, 168 сценариев, 86 main coverage rows |
| `cargo fmt -p localvox-light-api -p localvox-light-terminal -- --check`, `git diff --check`, новые текстовые файлы | PASS |

API ignored: аппаратные/нативные media, историческая копия, совместный сценарий и
его helper, settings helper. Совместный сценарий выполнен отдельной командой выше;
остальные ignored не объявляются пройденными этим package-запуском. TUI ignored:
два helpers и реальный media backend. Старые аппаратные evidence не переименованы
в новые проверки. Локальные логи: target/queue-read-{rust,tui,e2e-final,shared}.log.

В [interface CI](../../../../.github/workflows/interface-tests.yml) добавлена строгая
валидация change; новые Rust/browser cases входят в прежние команды. Удалённый
GitHub Actions не запускался. Документация и coverage связывают отказ/восстановление
с PIPE-002/011, не изменяя исторические измерения или main требования.

## Границы

Проверки используют временный API/архив, синтетические данные и ConPTY. Они не
являются замером скорости моделей, аппаратной приёмкой или подтверждением
controlled cancel/delete и ремонта повреждённых Done. BL-002/003/004 не закрываются.
Пункт 10.2 общего журнала остаётся not-run. Файлы очереди не ремонтируются этим
изменением: отказ виден до восстановления доступности. Main specs не синхронизированы,
changes не архивированы; коммит и индекс под управлением пользователя.
