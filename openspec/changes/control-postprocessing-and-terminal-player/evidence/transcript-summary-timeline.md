# STT, опциональная summary и непрерывный журнал · 2026-10-31

Текущее уточнение владельца: транскрибация обязательна, после неё автоматически
выполняется только существующая summary, если включена. Refine/cleanup остаются
явными ручными операциями. Этот прогон дополняет
[исторические проверки](implementation.md), не переименовывает их результаты.
Работающий демон, пользовательская .env и архив владельца не менялись.

## Связь требований и проверок

| Delta | Реализация | Выполненные сценарии |
|---|---|---|
| CFG-004, PIPE-001/002/015 | [jobs](../../../../crates/localvox-light-core/src/jobs.rs), [settings](../../../../crates/localvox-light-core/src/settings.rs), [autocook](../../../../crates/localvox-light/src/autocook.rs), [HTTP settings](../../../../crates/localvox-light-api/src/http/tests/settings.rs) | В каталоге один live-переключатель summary; legacy auto-clean env не включает очистку. Сохранение/отмена, приём URL и обязательный STT проверены. Pending следует настройке, Running/ручной запрос/legacy jobs сохраняют снимок. Пропуск Text создаёт собственную границу ожидания Summary. |
| ARCH-007 | [resolver](../../../../crates/localvox-light-core/src/readable.rs), [HTTP](../../../../crates/localvox-light-api/src/http/tests/workspace.rs), [UI e2e](../../../../ui/tests/e2e/workspace.spec.ts), [TUI](../../../../crates/localvox-light-terminal/src/render.rs) | Обе вкладки получают одинаковые слова, имена и таймкоды первичной версии; чтение и копирование работают без processed.json/cleanup receipt. Сохранённая ручная очистка сохраняется, повреждённый существующий processed.json даёт ошибку. |
| PIPE-016 | [workflow](../../../../crates/localvox-light-core/src/workflow.rs), [journal](../../../../crates/localvox-light-core/src/journal.rs), [UI e2e](../../../../ui/tests/e2e/workspace.spec.ts), [TUI](../../../../crates/localvox-light-terminal/src/render.rs) | Ожидание → running → backoff → повтор → Done; текущий/следующие этапы, времена ожидания и завершения. Выключенный координатор, устаревший сигнал, отсутствующее задание и повреждённая очередь различаются. Старое running не доказывает worker. Идентичность операции привязана к попытке; позднее завершение старой операции не снимает новую связь. При сетевой ошибке таймеры заморожены; итоговая длительность стабильна. Неизвестное время подписано явно. |

Начало worker отмечается до загрузки модели. Supervisor обновляет heartbeat
наблюдаемого живого процесса; это признак жизни, не обещание продвижения модели.
Неожиданный выход координатора и перехват panic отдельной попытки проверены чтением
кода; отдельная инъекция panic/сбой реальной модели в этом прогоне не выполнялись.

## Выполненные команды

Команды Cargo выполнялись с `--locked -j 2`; OpenSpec — CLI 1.13.2.
Логи текущего локального прогона находятся в `target/summary-timeline-*.log`
(производные локальные файлы, не версионированное доказательство).

| Проверка | Результат |
|---|---|
| `cargo test -p localvox-light-core --lib --no-default-features` | 324 passed |
| `cargo test -p localvox-light-api --lib` | 58 passed, 6 ignored; отдельные native fixtures перечислены ниже |
| `cargo test -p localvox-light-api --test journal_http_contract` | 1 passed; клиент читает health/record/journal через настоящий HTTP, проверка использует типизированный Health |
| `cargo test -p localvox-light --bin localvox-light --no-default-features` | 4 passed; сохранение проверенного STT не вызывает worker при выключенной summary даже с legacy clean env=on |
| `cargo test -p localvox-light-client -p localvox-light-terminal -p localvox-light-process` | 111 passed; 3 внутренних helper-теста вызываются родителями, реальный media backend отдельно ignored в обычном запуске |
| `cargo test -p localvox-light-terminal --lib` после уточнения ошибки связи | 83 passed, 2 helper ignored; явно проверены неподтверждённый статус и замороженные таймеры |
| `npm test` в ui | 9 passed |
| `npm run test:e2e` | 33 passed, включая 3 ожидаемых отрицательных проверки детектора символа/цвета/геометрии |
| `npm run build` | PASS: TypeScript и Vite |
| `npm run lint` | PASS с 8 warnings: 6 Fast Refresh export и 2 ref cleanup; ошибок нет |
| `cargo build -p localvox-light -p localvox-light-asr --bin localvox-light --bin localvox-process -p localvox-light-terminal --bin localvox-tui` | PASS: комплект daemon/worker/TUI |
| `cargo test -p localvox-light-api --lib -- --ignored --exact http::tests::cross_clients::shared_workspace --nocapture` | 1 passed, последний прогон 4.60 s: настоящий HTTP, embedded UI в Chromium и TUI в ConPTY, один временный архив; реальные изменения настройки summary и модельных настроек |
| `openspec validate --specs --strict --no-interactive` | 15 passed, 0 failed; informational замечания о длине текста требований |
| Строгая валидация двух затронутых changes | PASS: control-postprocessing-and-terminal-player и add-technical-journal-workspace |
| `python scripts/check-openspec.py` | PASS: 15 capabilities, 86 требований, 168 сценариев, 86 строк main coverage; delta перечислена отдельно |
| `git diff --check` и отдельная проверка новых исходников/документов | PASS, нет оставшихся snap.new; личный engineering-doctrine.md не изменялся |

Перед обновлением просмотрены 3 изменённых PNG и 5 предложений symbol/style
snapshots TUI. Изменения: явные неизвестные времена и неподтверждённое завершение,
длительность свёрнутой операции, отдельный экран ожидания. Обычный прогон с новыми
baseline прошёл без автоматического принятия. Проверка TUI дополнительно сравнивает
текст до/после минуты сетевой ошибки и после завершения, а также отличает старую
operation от новой попытки.

## Границы

- Настоящая двухчасовая запись и GigaAM/LLM не запускались. Два часа в daemon-тесте — синтетические таймкоды; численный выигрыш скорости не измерен.
- Новое автоматическое намерение применяется к новым follow_settings jobs. Старая очередь и явные ручные операции сохраняют свои флаги; текущая фаза завершается со своим снимком.
- Физический микрофон/вывод звука не проверялись; пункт 10.2 предыдущего change остаётся not-run. Ранее выполненная mpv-проверка записана в историческом evidence.
- Время старого этапа без сохранённых границ нельзя восстановить; отображается «длительность неизвестна». Новый журнал сохраняет монотонную длительность завершённой операции.
- Main specs не синхронизированы с незавершённым общим change; текущий контракт уточнения хранится в delta. Никакое архивирование не выполнялось.
