# Проверка принятых решений и исправлений — 2026-09-27

Рабочее дерево `new_course`, включая прежние незакоммиченные изменения.
Основание: DEC-001–008 и поручение владельца «давай все сверять и работать».
Это накопительный журнал сверки. Принятие контрактов после завершения аудита
записано отдельно в [baseline](../baseline.md); тесты не переносятся на установленный release.

## BL-001: подтверждение приёма ссылки

Прочитаны `ingest::from_url`, `chunks::create_session_dir/save_meta_public`,
`JobQueue::mutate/enqueue_ingest/save`. Игнорирование ошибки подтверждено кодом.

Изменено: checked atomic write meta с возвратом ошибки; успех приёма только после
сохранения очереди. При отказе очереди источник остаётся в своей сессии, ошибка
называет её. Повтор создаёт новую сессию и не уничтожает предыдущую. Это не
транзакция нескольких файлов и не обещание дедупликации повторно введённого URL.
Имена сессий теперь резервируются атомарным create_dir, чтобы одновременные ссылки
не попали в один каталог. Неудачный enqueue откатывается и в памяти очереди.

Проверки на временных каталогах: успешный приём и новое чтение очереди;
meta.json недоступен для записи; очередь недоступна; безопасный повтор; восемь
одновременных ссылок создают восемь разных сессий и сохранённых заданий.

## BL-007: Text/Summary без аудио

Прочитаны `Archive::recook/ensure_recookable`, `JobQueue::requeue_derivatives`,
`verify_phase`, worker `--post-only`, проверка артефактов и versions.

Изменено: проверка предпосылок по scope под владением сессией; All требует полное
пригодное аудио, Text/Summary — строго прочитанную выбранную расшифровку. Сохранённые
receipts учитываются, legacy без receipts проверяется структурно; неподтверждённая
пустая расшифровка отклоняется. Scoped-задание начинается с Text/Summary, включая
старые ingest-задания, без скачивания и STT. Ручной post-only использует ту же
проверку источника. Отказ до постановки не удаляет прежнюю сводку/текст.

Проверки на временных каталогах: Text/Summary без audio; сохранение исходной версии;
приёмка сохранённого производного результата; отказ All; отсутствующий, битый и
неподтверждённый пустой текст; конфликт живого worker.

## Выполненные команды

`cargo test --locked -p localvox-light-core -p localvox-light-api --lib`:
**283 core + 37 API, 0 failures**. Сборка без feature `onnx`; реальные модели,
микрофон, сеть и остановка работающего пользовательского демона не запускались.
Остаются прежние предупреждения deprecated CPAL API и dead_code без ONNX.

## Границы снимка и общие проверки

HEAD: `6dfde241bbeff1736829b0bf94c8d4c1f84b1517`, ветка `new_course` плюс dirty tree.
Это не проверка одного коммита: прежние изменения сохранены, текущие исправления
перечислены ниже. Локальный подробный лог: `target/verification/inventory-tests.log`
(игнорируемый build-artifact, не постоянный источник требований).

- `cargo test --locked --workspace --exclude localvox-desktop --all-features`:
  **548 passed, 0 failed, 2 ignored**. Включён ONNX; core 318, API 39, LLM 98.
  Один ignored — subprocess fixture, вызываемый другими тестами; второй —
  проверка исторической записи, запущенная отдельно и успешно.
- После общего прогона добавлены и отдельно пройдены 3 API-теста:
  `verified_transcript_remains_readable_when_summary_fails`,
  `naming_reports_queue_failure_after_local_save_without_claiming_a_rebuild`,
  `language_is_restored_when_recook_cannot_be_queued` (каждый 1 passed).
- `npm run build`: TypeScript и Vite прошли. Browser/player UAT этим не заменяется.
- `scripts/run.ps1 -Profile dev -BuildOnly -Desktop`: собраны daemon, worker и
  desktop в `target/debug`; совместный комплект найден из Cargo artifacts.
  `localvox-process --worker-protocol` → `localvox-worker/1`.
- `python scripts/check-pipeline.py`: реальный debug worker + локальный mock HTTP;
  post-only без аудио/ASR, heartbeat во время ответа >5 с, terminal ordering,
  receipts, 0 новых запросов при resume, отказ повреждённого производного
  результата без повторного STT — PASS. Настоящий облачный провайдер не вызывался.
- `scripts/fetch-models.ps1 -Check`: GigaAM/диаризация/NER присутствуют;
  стандартный `models/vosk-model-ru-0.42` отсутствует. Это проверка стандартного
  каталога; нельзя заключать, что у работающего движка нет модели по другому пути.

Предупреждения сборки: устаревший CPAL name, unused_mut в ONNX, dead_code без ONNX,
совпадающее имя desktop PDB (lib/bin). Эти предупреждения не были ошибками сборки.

## DEC-001 / BL-020: хранение аудио

[protect-retained-audio](../changes/protect-retained-audio/proposal.md) реализован.
CLI default retention=0; явный положительный срок не переписывается. Sweep проверяет
STT через processing::inspect, состояние очереди, marker/part и execution lease.
Очередь удерживается через проверку и удаление; повреждённое состояние сохраняет
аудио. Проверены старый mtime, valid/missing/corrupt STT, Pending, чужой lease,
recording marker, part, повреждённый jobs.json; прочие артефакты сохраняются.
Проверки выполнялись только на временных каталогах. FLAC conversion не исправлен
этой работой (RV-005); power-loss и реальный отказ диска не воспроизведены.

## BL-018: зависание и управление

Подтверждены кодом и отдельными тестами две зависимости от отсутствующего события:
непрерываемый recv загрузки Vosk и опрос start/stop только после PCM.
Теперь ожидание модели проверяет running каждые 100 мс; служебный цикл записи
проверяет команды каждые 200 мс независимо от кадров. Тест держит PCM sender живым,
запускает запись без кадров, подаёт один кадр и останавливает без следующих;
проверены metadata и финализация сохранённого аудио.

Ожидания daemon/engine имеют имена и elapsed; timeout/panic возвращают ошибку.
shutdown сам устанавливает running=false, ошибка autocook не пропускает попытку
финализации engine. Unit-тест проверяет timeout, panic и обычное завершение.
В engine heartbeat ожидания называет pipeline/mic/loopback/device-watch/live-asr.
Лимит 10 с не увеличен. Сам native вызов загрузки не прерывается: coordinator
перестаёт ждать его. В process lifecycle пройдены cancellation/timeout дерева,
вложенные supervisor, duplex больших stdin/stdout и смерть Windows supervisor.

**Исходный release-инцидент не воспроизведён и BL-018 не закрыт.** Старый общий лог
не позволяет приписать его именно Vosk или autocook. Native capture/drop и реальное
железо остаются проверкой после безопасного обновления. Во время аудита работал
старый release daemon с обновляемым live transcript; он не перезапускался.

## BL-019: имена только внутри записи

Cook/names больше не читают shared profiles; imported audio не получает имя «Я».
Ручное имя не вызывает enroll, сохраняется с manually_named в roster этой записи.
Recook переносит его только при однозначном взаимном сопоставлении внутри этой
сессии (cosine >=0.90, одинаковая размерность); legacy label не считается ручным.
UI больше не обещает узнавание в других записях. Старые profiles можно прочитать
или удалить; автоматический перенос их имён отключён.

Проверены новый разговор при существующем профиле, короткая реплика, локальное имя,
неоднозначные векторы и legacy roster. При переименовании все версии сначала
строго читаются; битая строка больше не пропускается с последующей потерей при записи.
Для исправленной подписи переочередяется только Summary, без Prepare/STT/refine.
Отказ постановки возвращается как «имя сохранено, повтор не поставлен», а не успех.
Несколько файлов имени/текста/очереди ещё не образуют crash-транзакцию.

Реальная запись `20260927_112917_youtube` («Почему я бросил IT… – Фил Ранжин»)
содержала в roster «Арсен Маркарян». Источник исторического имени по legacy roster
не установлен; ошибочность сообщил владелец. Запущен opt-in тест
`historical_speaker_correction_on_temporary_copy_preserves_speech` с
`LOCALVOX_REVIEW_SESSION` на этот каталог: **PASS**. Текст и таймкоды всех версий
копии сохранены, подпись заменена на нейтральную, очередь начинается с Summary,
оригинальные файлы побайтово проверены как неизменённые. Настоящая запись и остальные
исторические подписи не переписаны; отдельная задача реального исправления открыта.

## DEC-007 / BL-002: владение сессией, частично

recook/set_best/set_lang/name_source/name_speaker/delete теперь получают общий
execution lease до изменения. Он удерживается при обращении к jobs mutex;
координатор берёт jobs mutex и делает только НЕБЛОКИРУЮЩИЙ try-lock, поэтому
обратный порядок не создаёт циклическое ожидание. set_lang при отказе recook
восстанавливает старую meta. Тест всех перечисленных мутаций с удержанным чужим
lease подтверждает отказ и побайтовую сохранность файлов/отсутствие задания.

**Управляемой отмены отдельного job пока нет.** Delete занятой сессии теперь
безопасно отказывает; требуемый DEC-007 путь stop → освобождение → delete
ещё предстоит реализовать. Нет durable cancelled/tombstone и multi-file transaction.
Остальные шаги BL-002 не помечены выполненными.

## Исходный медленный ролик

В `20260927_110841_youtube` найден «AI убил рынок IT? Как я искал работу 3 месяца,
нашёл... и уволился.». Один сохранённый запуск имеет Summary от
11:29:21.230 до 11:33:23.478 +03:00: **242.248 с**. Refine этого запуска:
11:26:08.371–11:27:44.246. Отдельных start/end Cleanup в историческом progress нет.
По соседним timestamp нельзя выдавать точную длительность Cleanup и тем более 0 с.
Этот журнал покрывает один запуск, не всё пользовательское окно 11:08–11:39.
Новый timing хранит неизвестные границы как None/null; производительность полной
пачки на release всё ещё требует BL-003, а не вывода из mock-теста.

## Сверка всех 15 capability

Таблица отделяет чтение кода и выполненные тесты от оставшейся приёмки.
«Проверено» относится только к названному сценарию, не ко всему capability.

| Capability / вход | Выполненная сверка и проверки | Открытая граница |
|---|---|---|
| [recording](../specs/recording/spec.md): pipeline/ChunkLane | start/stop без PCM, pre-roll, pause, rotation и recovery; тесты core | Физический микрофон/loopback, отсоединение устройства, заполненный диск; BL-018 |
| [live-transcription](../specs/live-transcription/spec.md): engine/asr_worker_pool/TranscriptWriter | Отменяемое ожидание модели; прочитан append/recovery; core tests | RV-002: Windows только flush, Unix игнорирует fsync result; нельзя обещать power-loss durability |
| [audio-storage](../specs/audio-storage/spec.md): chunks | DEC-001 исправлен и проверен; recovery orphan/continuity пройдены | RV-005: FLAC прямой subprocess, отсутствует полная проверка перед удалением WAV |
| [processing-pipeline](../specs/processing-pipeline/spec.md): jobs/autocook/worker | Phase/lease/receipt/recovery tests; реальный worker с mock; heartbeat, отсутствие повторных запросов; BL-001 | DEC-003/BL-004: Done дедупликация мешает автоматическому ремонту; BL-002 cancel не готов; BL-003/006/008 |
| [transcript-versions](../specs/transcript-versions/spec.md): versions/recook | BL-007; оригинал/новая версия, strict rename; копия реальной записи | Multi-file crash между version/roster/queue, реальные разные ASR-языки |
| [llm-processing](../specs/llm-processing/spec.md): LlmClient/pipeline/grounding | 98 LLM unit; корпусный тест; mock resume/failure; unverified остаётся результатом; verify перед confirm | DEC-004 требует примеров допустимого перефразирования. RV-016: confirm возвращает false и для ошибки, API ошибочно говорит «Пометки и так не было»; checksum это не обходит |
| [archive-playback](../specs/archive-playback/spec.md): Archive/Player | API: готовый STT читается при failed Summary (DEC-002), клипы/Range/mixing/path validation; UI build | Реальный browser seek длинного FLAC; delete только отказывает занятому, ещё не останавливает его |
| [search-chat](../specs/search-chat/spec.md): Archive::search_mode, search/semantic/chat | Lexical unit, hybrid RRF и semantic overload/dimension/empty-session integration прошли | RV-008 подтверждён кодом: оба error превращаются в пустые Vec. Полный background indexing BL-005 не воспроизведён |
| [speakers](../specs/speakers/spec.md): cook/names/roster/API | BL-019 code и временная копия исходного примера проверены | Исторический архив не мигрирован; качество реальной диаризации и ложные локальные совпадения не доказаны тестами векторов |
| [voice-notes](../specs/voice-notes/spec.md): Brain/spawn | 22 voice tests: source0, retro, boundaries, echo, flush-on-shutdown | DEC-008 ещё не реализован; Brain выдаёт Say/Status успеха до результата Write; hardware/TTS не прогонялись |
| [note-integrations](../specs/note-integrations/spec.md): Registry/FsIntegration/MCP | 18 integration unit: slots/file/folder/read/delete; изучен MCP timeout/Drop | RV-007: коллизии файлов, нет durable local-first/outbox, cleanup только child; [BL-021](../changes/persist-notes-before-delivery/proposal.md) |
| [api-access](../specs/api-access/spec.md): http/McpServer | Tests auth/Host helper/routes/MCP; существующие socket integration mock | RV-003: audio.wav обходит общий Host/Origin при loopback. Все варианты media по живому HTTP не проверены |
| [ad-hoc-asks](../specs/ad-hoc-asks/spec.md): asks/Archive/Ask.tsx | FIFO/reclaim/roundtrip/failed tests; UI явно показывает provider select | DEC-006 частично: API без provider выбирает Claude, неизвестный provider уходит в HTTP ветку; прямые writes, delete Running и повтор облачного вызова не урегулированы (RV-009) |
| [runtime-settings](../specs/runtime-settings/spec.md): settings/http/Daemon | BL-018 diagnostics/tests; settings/env tests; сборка desktop | RV-001: Live маркировка против startup Config/child_env; RV-010: extra .env не маскирует неизвестные secrets; реальный рестарт не выполнялся |
| [installation-models](../specs/installation-models/spec.md): run/fetch/doctor | Совместная dev сборка 3 бинарников, protocol, fetch Check, doctor unit | Нет нового установщика/release deployment, Linux/macOS hardware UAT и переустановки моделей |

## Дополнительная проверка и приоритеты

### Дополнительный прогон HTTP

`target/verification/audit_http.py` запускает собственный `localvox-api` на
свободном loopback-порту во временном cwd/workspace; затем завершает только этот
дочерний процесс. Отдельные варианты без токена и с синтетическим токеном.
Сохранены результаты в `target/verification/http-audit.json`:

- health: local 200, foreign Host 403; с token без Bearer/query-only 401,
  правильный Bearer 200.
- media несуществующей сессии: foreign Host и foreign Origin без token дают 404,
  т.е. проходят до поиска сессии вместо общей проверки 403. Это подтверждает
  исключение маршрута; фактическое чтение чужого аудио не выполнялось.
- media с token: без token 401, правильный query token 404 отсутствующей сессии.
- settings.extra возвращает синтетическое значение неизвестного ключа .env.
- POST asks без provider возвращает 200 и сохраняет provider=claude. Worker
  заметок/asks не запускался, модель/облако не вызывались.

### Приоритеты

1. BL-002: контролируемый cancel с подтверждённым освобождением и запретом revival;
   затем безопасное delete. Проверить crash между мутациями, не объявлять lock транзакцией.
2. BL-004: ремонт corrupt Done от минимальной повреждённой фазы, bounded retry без
   потери raw и без повторного скачивания при сохранившемся тексте.
3. BL-018/003: на обновлённом комплекте безопасно воспроизвести shutdown и пачку;
   записать компонент/фазу/время/ресурсы. Нельзя утверждать ускорение по unit-тестам.
4. BL-021/DEC-008: local-first заметки и статусы доставки. RV-001/003/008/009/010/016:
   оформить конкретные поведенческие changes перед исправлениями.

Все эти пробелы относятся к коду/проверкам, а не к повторному запросу владельцу
согласовать уже принятые DEC. Приёмка всех capability остаётся открытой.

## Проверка документов

`openspec validate --all --strict --no-interactive --json`: **15/15 specs valid,
11/14 changes valid**. Три proposal-only без deltas ещё не готовы:
recover-corrupt-processing-artifacts, unify-manual-and-queued-import,
persist-notes-before-delivery. Общий validate поэтому НЕ зелёный. Поведенческие
черновики не замаскированы skip_specs. У синхронизированного INV-INGEST-001 CLI
выдаёт INFO «ADDED already exists»; при будущей архивации учитывать уже выполненный sync.
Проверка существования относительных Markdown-ссылок: отсутствующих нет;
она не проверяет истинность текста и browser-рендер. `git diff --check` без ошибок
whitespace (есть обычные предупреждения Git о LF/CRLF).

## Завершение аудита заметок и принятие baseline

По последнему поручению владельца завершён аудит 2.5 и переход документации
в принятые контракты. Поведенческие gaps остаются открытыми; Rust runtime в этом
заключительном проходе не менялся, пользовательское приложение не перезапускалось.

Команды:

```powershell
cargo build --locked -p localvox-light-integrations --bin localvox-note
python scripts/check-note-delivery.py
```

Обе выполнены успешно. Скрипт запускает реальный debug localvox-note из временного
cwd с пустым .env, явным временным slots.toml и очищенными LOCALVOX-переменными.
MCP — локальный Python stdio fixture, который записывает только синтетические заметки
в TEMP. Настоящие конфиги, назначения, сеть и архив не используются.

| Сценарий | Наблюдение |
|---|---|
| Файловый append дважды | Оба текста прочитаны, первый сохранён |
| Родитель назначения — файл | Exit 1, успешное подтверждение не выдано |
| MCP isError, затем явный повтор | Exit 1, затем 0; 2 попытки, 1 копия в назначении |
| Назначение записало, но ответ потерян; повтор | Exit 1, затем 0; 2 попытки, **2 копии** |
| MCP молчит | При тестовом лимите 1 с получена ошибка timeout, exit 1 |

Это проверка адаптера и CLI, не end-to-end голосового TTS. В voice worker по коду
успешные Status/Say формируются до исхода Write и не отзываются при его ошибке.
Локального outbox с отдельным delivery status нет. Эти факты нарушают NOTE-006 /
VOICE-004; коллизии файлов NOTE-002 также остаются BL-021. Mock-повтор не доказывает
автоматического retry или exactly-once. Аудит завершён выявлением несоответствия,
исправление не объявлено выполненным.

При заключительном перечитывании всех 15 specs старые buggy формулировки заменены
контрактами и Implementation status. Матрица покрывает 86 требований и 168 сценариев;
43 строки имеют конкретные ранее выполненные тесты (частичное покрытие), остальные
опираются на чтение кода/изолированные проверки. 14 строк явно отмечают GAP.
Эти числа не являются процентом работоспособности приложения.

## Итоговая проверка документации после принятия

- `openspec validate --specs --strict --no-interactive`: **15/15 passed**.
- `openspec validate review-code-inventory --type change --strict --no-interactive`:
  passed; skip_specs относится к документационному аудиту, не реализации gaps.
- `python scripts/check-openspec.py`: **86/86 требований**, 168 сценариев,
  812 локальных ссылок/якорей, пропущенных строк матрицы и потерянных ссылок нет.
  Проверено существование функций всех указанных test:name.
- На отдельном временном репозитории проверен сам checker: валидный пример принят;
  обнаружены все 8 внесённых ошибок — пропущенная строка матрицы, дубликат ID,
  отсутствующий исходник, неверный якорь, переименованный тест, потерянный THEN,
  неизвестный RV и лишняя строка покрытия. Основной репозиторий этими проверками
  не повреждался; никаких моделей или внешних сервисов checker не вызывает.
- `git diff --check`: ошибок whitespace нет; предупреждения LF/CRLF не ошибки.
- Общий `validate --all` по-прежнему **26/29 passed**: все main specs и 11 changes
  проходят, три proposal-only без delta перечислены выше. Они намеренно не приняты
  как законченный план и не замаскированы пропуском спецификаций.

Completeness аудита: все 18 задач выполнены. Correctness документации: каждый ID
имеет сценарий, код и ограниченное свидетельство; известные нарушения остаются GAP.
Coherence: README/config/AGENTS/каталог и старые указатели согласованы с baseline.
Это итог проверки change review-code-inventory, не отчёт о закрытии остальных changes.
