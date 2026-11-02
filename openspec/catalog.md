# Каталог возможностей

Все 15 capability — **accepted contracts** с 2026-09-27 по [baseline](baseline.md).
Таблица перечисляет обязательное поведение в границах каждого spec. Это не статус
«всё реализовано»: [матрица](evidence/coverage.md) связывает каждое требование с
кодом и проверками, [review](review.md) хранит известные расхождения.
Принятые DEC-001–008 встроены в требования; их карта находится в baseline.md.

Инженерный вход для замера очереди без устройств:
[общий headless координатор](changes/extract-headless-processing-coordinator/evidence/implementation.md).
Этот срез проверяет рефакторинг/стенд; результаты полного BL-003 приведены ниже,
аппаратная приёмка остаётся открытой.

Продолжение BL-003: [release-очередь на двухчасовой публичной речи](changes/benchmark-processing-batch/evidence/queue-benchmark.md),
reference-бюджет и отдельные model/verification/resource timings. BL-003 завершён
как измерение, не как обещание скорости/качества для любого материала. Следующий
[proposal измерения grounding](changes/profile-summary-grounding-cost/proposal.md)
не принят за реализованную оптимизацию; аппаратная приёмка остаётся открытой.

| Возможность | Связь с историей | Контракт | Граница |
|---|---|---|---|
| [recording](specs/recording/spec.md) | F3/F9 | Команда записи, pre-roll, pause/stop, детект, watchdog | Кнопки/голос → PCM и meta; аппаратная приёмка открыта |
| [live-transcription](specs/live-transcription/spec.md) | F2/F8 | Vosk fast lane, legacy WAV, live transcript, recovery | Не смешивать с архивным STT и удалением архивных чанков |
| [audio-storage](specs/audio-storage/spec.md) | F8/P1/P3 | Чанки, crash recovery, FLAC, retention | Ошибки диска/питания и незавершённая обработка требуют сверки |
| [processing-pipeline](specs/processing-pipeline/spec.md) | F1/F8 | Prepare → STT → Text → Summary, очередь, receipts, время | PIPE-001–014; прежние проверки отдельно от живого benchmark |
| [transcript-versions](specs/transcript-versions/spec.md) | F8/F1 | Версии, best, язык, повторные обработки | Исходный текст сохраняется; имена меняются во всех версиях |
| [llm-processing](specs/llm-processing/spec.md) | F1 | Глоссарий, refine/cleanup, сводки, grounding, Claude | Проверки качества эвристические; удалённые вызовы существуют |
| [archive-playback](specs/archive-playback/spec.md) | F10 | Чтение, плеер, аудиоклипы, экспорт и удаление | Нет обещания пословных таймкодов/скорости/undo |
| [search-chat](specs/search-chat/spec.md) | F4 | Поиск трёх видов и grounded-вопрос к архиву | Не заявлены все когда-либо задуманные scope и streaming |
| [speakers](specs/speakers/spec.md) | F11 | Диаризация, локальные имена, запрет межсессионного узнавания | Алгоритмический merge не равен пользовательскому merge/undo |
| [voice-notes](specs/voice-notes/spec.md) | F2 | Команды, многословная диктовка, ретро, TTS, clipboard | Ретро использует распознанный текст; приёмка микрофона открыта |
| [note-integrations](specs/note-integrations/spec.md) | F2/F5 | Слоты, file/folder/MCP, чтение/удаление, suggest routing | Полный per-slot конфиг старого F2 не объявлен реализованным |
| [api-access](specs/api-access/spec.md) | F6/F7 | HTTP, media exception, MCP archive, браузер/LAN | Native/offline mobile и WS не подтверждены |
| [ad-hoc-asks](specs/ad-hoc-asks/spec.md) | F1/F6 | Материал → отдельная фоновая очередь вопросов | Явный провайдер; текущий API нарушает это правило (RV-009), File.text() без PDF parser |
| [runtime-settings](specs/runtime-settings/spec.md) | F3/F6/P7 | Демон/окно, комплект сборки, .env, автостарт | Snapshot честно требует Restart (RV-001 исправлен); аппаратная приёмка открыта |
| [installation-models](specs/installation-models/spec.md) | Инфраструктура | Скрипты моделей, hashes, пути, doctor | Проверка наличия не заменяет испытание модели |

## Предложения

Конкретные работы ведутся в штатных change-пакетах. Таблица — навигация;
актуальную готовность документов показывает `openspec status --change <name>`,
выполнение задач — `openspec instructions apply --change <name> --json`.
Полноту документов показывает CLI; новые UI/TUI реализованы и проходят отдельные
browser/HTTP/ConPTY/media проверки в [change](changes/add-technical-journal-workspace/evidence/implementation.md).
Единый UI+TUI прогон выполнен на настоящем temp API с synthetic producer; аппаратная
приёмка и полный прогон моделей пока не объявлены выполненными.
BL-001/002/007/020 реализованы и проверены в исходниках; BL-018/019 выполнены частично.
BL-003 завершён как reference-измерение с [условным бюджетом](changes/benchmark-processing-batch/evidence/queue-benchmark.md); это не SLA.
[Сверка всех capability и выполненные проверки](evidence/implementation-review-2026-09-27.md)
показывает границы результатов. Исторический архив и установленная release-сборка
не обновлялись; наличие полного плана не означает завершённую работу.

| Изменение | Основание | Тема |
|---|---|---|
| [fix-ingest-ack](changes/fix-ingest-ack/proposal.md) | [BL-001](backlog.md#bl-001) | Подтверждать приём ссылки сохранением meta и очереди |
| [serialize-session-mutations](changes/archive/2026-11-01-serialize-session-mutations/proposal.md) | [BL-002](backlog.md#bl-002) | Реализовано/проверено: владение, controlled cancel/delete, native CLI и restart/fault; [результаты](changes/archive/2026-11-01-serialize-session-mutations/evidence/implementation.md) |
| [benchmark-processing-batch](changes/benchmark-processing-batch/proposal.md) | [BL-003](backlog.md#bl-003) | Изолированные worker-замеры: [STT](changes/benchmark-processing-batch/evidence/calibration.md), [настоящая summary/NER](changes/benchmark-processing-batch/evidence/summary-probes.md); полный координатор с длинной русской речью и repeats off/on, [бюджет и границы](changes/benchmark-processing-batch/evidence/queue-benchmark.md) проверены |
| [recover-corrupt-processing-artifacts](changes/recover-corrupt-processing-artifacts/proposal.md) | [BL-004](backlog.md#bl-004) | Восстанавливать повреждённые готовые артефакты |
| [verify-background-indexing](changes/verify-background-indexing/proposal.md) | [BL-005](backlog.md#bl-005) | Native проверка выполнена: [idle/cancel/timeout/отказ/recovery/search](changes/verify-background-indexing/evidence/implementation.md), синтетические embeddings; качество модели отдельно |
| [unify-manual-and-queued-import](changes/unify-manual-and-queued-import/proposal.md) | [BL-006](backlog.md#bl-006) | Объединить ручной импорт и фоновый Prepare |
| [allow-postprocess-without-audio](changes/allow-postprocess-without-audio/proposal.md) | [BL-007](backlog.md#bl-007) | Повторять Text/Summary после retention аудио |
| [explore-stt-worker-lifecycle](changes/explore-stt-worker-lifecycle/proposal.md) | [BL-008](backlog.md#bl-008) | Исследовать память импорта и постоянный STT-worker |
| [review-code-inventory](changes/review-code-inventory/proposal.md) | [BL-009](backlog.md#bl-009) | Завершённый аудит и принятие baseline с известными gaps |
| [automate-spec-evidence-checks](changes/automate-spec-evidence-checks/proposal.md) | [BL-010](backlog.md#bl-010) | Проверять связь спецификаций с кодом и тестами в CI |
| [fix-unresponsive-shutdown](changes/fix-unresponsive-shutdown/proposal.md) | [BL-018](backlog.md#bl-018) | Найти и устранить корневую причину зависания при остановке |
| [disable-cross-session-speaker-recognition](changes/disable-cross-session-speaker-recognition/proposal.md) | [BL-019](backlog.md#bl-019) | Отключить автоматическое узнавание людей между записями |
| [protect-retained-audio](changes/protect-retained-audio/proposal.md) | [BL-020](backlog.md#bl-020) | Хранить аудио по умолчанию и проверять STT перед opt-in очисткой |
| [persist-notes-before-delivery](changes/persist-notes-before-delivery/proposal.md) | [BL-021](backlog.md#bl-021) | Сохранить локальную заметку до доставки и подтверждать фактический результат |
| [add-technical-journal-workspace](changes/add-technical-journal-workspace/proposal.md) | Запрос владельца 2026-10-31 | Общий журнал UI/TUI и новый терминальный аналог графического приложения; [план](changes/add-technical-journal-workspace/design.md), [паритет](changes/add-technical-journal-workspace/parity.md), [реализация и проверки](changes/add-technical-journal-workspace/evidence/implementation.md) в работе |
| [control-postprocessing-and-terminal-player](changes/control-postprocessing-and-terminal-player/proposal.md) | Уточнения владельца 2026-10-31/11-01 | Обязательный STT, опциональная summary, непрерывный ход обработки, плеер и цельный текст; программные задачи 21/21, [последние проверки](changes/control-postprocessing-and-terminal-player/evidence/plain-text-and-terminal-exit.md), аппаратная приёмка общего журнала остаётся открытой |
| [refactor-terminal-command-dispatch](changes/refactor-terminal-command-dispatch/proposal.md) | Продолжение рефакторинга 2026-11-01 | Именованные действия и статическая палитра TUI без изменения поведения; [проверки](changes/refactor-terminal-command-dispatch/evidence/implementation.md) |
| [fix-browser-artifact-refresh](changes/fix-browser-artifact-refresh/proposal.md) | Проверка медленных ответов и рефакторинг UI 2026-11-01 | Последовательное обновление артефактов, объединение refresh и отмена старого чтения; [проверки](changes/fix-browser-artifact-refresh/evidence/implementation.md) |
| [fix-workspace-status-polling](changes/fix-workspace-status-polling/proposal.md) | Продолжение проверки фоновых обновлений 2026-11-01 | Независимые sessions/jobs/record, отмена старого снимка после действия; [проверки](changes/fix-workspace-status-polling/evidence/implementation.md) |
| [format-api-rust-sources](changes/format-api-rust-sources/proposal.md) | Очистка проекта 2026-11-01 | Штатное форматирование API и постоянная проверка CI без изменения контрактов; [проверки](changes/format-api-rust-sources/evidence/implementation.md) |
| [fix-browser-player-lifecycle](changes/fix-browser-player-lifecycle/proposal.md) | Продолжение проверки плеера 2026-11-01 | Сохранение позиции/паузы/фрагмента при обновлении метаданных, последовательная waveform и cleanup источника; [проверки](changes/fix-browser-player-lifecycle/evidence/implementation.md) |
| [fix-queue-read-failures](changes/fix-queue-read-failures/proposal.md) | Продолжение проверки фоновой обработки 2026-11-01 | Нечитаемая очередь даёт 503 и явный статус UI/TUI с сохранением последних данных; [проверки](changes/fix-queue-read-failures/evidence/implementation.md) |
| [resume-failed-processing](changes/resume-failed-processing/proposal.md) | Продолжение проверки ручного повтора 2026-11-01 | Возобновление Failed с прежней фазы без full recook, отдельные API/UI/TUI команды; [проверки](changes/resume-failed-processing/evidence/implementation.md) |
| [explain-automatic-retry-state](changes/explain-automatic-retry-state/proposal.md) | Продолжение проверки повторов 2026-11-01 | Единое объяснение backoff/координатора, фаза и план в очереди UI/TUI; [проверки](changes/explain-automatic-retry-state/evidence/implementation.md) |
| [commit-queue-execution-transitions](changes/commit-queue-execution-transitions/proposal.md) | Продолжение проверки очереди 2026-11-01 | Откат несохранённых claim/recovery/revival, ожидание recovery перед запуском линий; [проверки](changes/commit-queue-execution-transitions/evidence/implementation.md) |
| [persist-worker-outcomes](changes/persist-worker-outcomes/proposal.md) | Продолжение проверки исходов и справочник владельца 2026-11-01 | Ожидание сохранения результата/отказа без нового worker, rollback и порядок progress; [проверки](changes/persist-worker-outcomes/evidence/implementation.md), [пользовательский справочник](../docs/user-guide.md) |

Группы сырых идей BL-011–017 остаются во [входящем списке](backlog.md).
Их ещё нужно разделить на конкретные изменения; наличие там записи не запускает разработку.

## Карта переноса из README и прежних документов

| Тема старого документа | Место сверки по коду |
|---|---|
| README: локальный секретарь, запись по команде, pre-roll | recording, live-transcription; исключения сети — llm-processing/ad-hoc-asks |
| README: ссылка/файл, распознавание и LLM | processing-pipeline, transcript-versions, llm-processing |
| README: запуск окна/демона, повторный запуск, обновление профилей сборки | runtime-settings |
| README: модели/нативная библиотека/doctor, .part/hash | installation-models |
| README: конфиг, каталог данных, регенерация | runtime-settings, audio-storage, transcript-versions |
| README: браузер, телефон, платформы | api-access, runtime-settings; неподтверждённые платформы отмечены |
| integrations.md: команды, ретро, конец заметки | voice-notes; как настроить TOML остаётся how-to |
| integrations.md: слот/file/folder/MCP | note-integrations |
| feature-registry F1–F11, user-scenarios S1–S11, UAT | темы покрыты каталогом; желания/старые галочки не перенесены как готовность |
| Прежний docs/specs/pipeline.md | processing-pipeline; нынешний контракт принят после сверки; исторический снимок сохранён отдельно |
| Прежние acceptance/backlog | evidence/pipeline.md и backlog.md; история результатов сохранена |
| Архитектурный выбор процесса/хранилища | docs/architecture.md + ADR, не продуктовое правило навсегда |

Инструкции установки/сборки остаются в README. Старые планы и UAT сохраняются
как материалы поиска и история; не создаём вторую актуальную копию поведения.

## Что не считать уже существующим по старому реестру

P1/P2: фактические commit/flush/ошибки описаны по путям; нет общего доказанного
crash-safe поведения любого файла и побайтовой неизменности всех версий.
P3: retention исправлен по DEC-001: по умолчанию выключен, opt-in требует проверенного STT.
P4/P5: отдельные крейты/флаги существуют, не каждый subprocess изолирован одинаково.
P6: параллельны фазы разных сессий; есть условный reference-бюджет BL-003, универсальное «час за пять минут» не заявлено.
P7: UI — клиент; история про обязательную общую шину не доказывает WS.
P8: явный выбор обязателен по DEC-006; неявный API default Claude остаётся дефектом RV-009.
P9: источники — файлы, индексы производные; обязательная meta.sqlite не обнаружена.

Список следующих идей сохранён в BL-011–017. Уже имеющиеся элементы (clipboard
команда, doctor, models.json, голосовые профили) не считаются отсутствующими только
потому, что были записаны в старый пул хотелок. Расширение этих возможностей —
отдельное предложение после сверки существующего кода.

BL-005 обнаружил [RV-017 / fix-empty-semantic-index-failure](changes/fix-empty-semantic-index-failure/proposal.md): отказ embeddings при пустом индексе больше не выдаётся за отсутствие совпадений; [red → green](changes/fix-empty-semantic-index-failure/evidence/implementation.md).

Предустановки каталога и список LLM-адресов: [refine-workspace-controls-and-settings](changes/refine-workspace-controls-and-settings/proposal.md),
CFG-001/005; [проверки 2026-11-02](changes/refine-workspace-controls-and-settings/evidence/implementation.md).

Правка текста после STT: [profile-text-postprocessing-cost](changes/profile-text-postprocessing-cost/proposal.md) —
отдельно измерены refine, cleanup, их последовательность и summary на публичной
двухчасовой STT. [Нативные результаты 2026-11-02](changes/profile-text-postprocessing-cost/evidence/native-reference.md);
исторический часовой инцидент владельца не воспроизведён, prompts/качество не менялись.

Проба другого подхода к правке: [experiment-guarded-sparse-text-edits](changes/experiment-guarded-sparse-text-edits/proposal.md).
Два opt-in engineering протокола, 4 локальные модели, corpus/длинные прогоны;
[итог — без production rollout](changes/experiment-guarded-sparse-text-edits/evidence/results.md), обнаружен открытый RV-018.

Проверка качества обычной правки: [evaluate-local-cleanup-quality](changes/evaluate-local-cleanup-quality/proposal.md).
[72 synthetic фразы × 2 модели × 3 повтора и 51 agent-review публичных строк](changes/evaluate-local-cleanup-quality/evidence/results.md).
На момент той проверки RV-018/RV-019 открыты; последующее исправление указано ниже. Модель не принята для автоматической смысловой правки, сверка с аудио not-run.

Отбор для Mac/слабого железа: [screen-portable-cleanup-models](changes/screen-portable-cleanup-models/proposal.md).
[18 Ollama worker repeats и 6 SAGE CPU repeats](changes/screen-portable-cleanup-models/evidence/results.md),
без production rollout; модель и изолированное Python-окружение SAGE на F: по поручению владельца.
Никто не прошёл полный gate; Mac/MPS/ONNX и качество summary остаются not-run.

Сохранность обычной правки: [fix-text-correction-retention](changes/fix-text-correction-retention/proposal.md).
[Воспроизведённые RV-018/RV-019 исправлены в исходниках](changes/fix-text-correction-retention/evidence/implementation.md):
общий guard refine/cleanup, unit/runtime-mock, 12 native repeats и replay.
Фильтрация консервативна и отклоняет часть полезных правок; полная смысловая/аудио
приёмка, native качество refine и обновление работающего демона не выполнены.
Выбор модели/default не менялся, SAGE в продукт не подключён.

Длинный cleanup после исправления сохранности: [verify-guarded-cleanup-on-long-transcript](changes/verify-guarded-cleanup-on-long-transcript/proposal.md).
[4 новых release запуска, 2816 строк и targeted review](changes/verify-guarded-cleanup-on-long-transcript/evidence/results.md):
Gemma быстрее на RTX, Qwen меньше по файлам; выявлен открытый RV-020 с близкими
морфологическими заменами. Полная автоматическая правка/default модели не приняты,
Mac и качество summary не проверены.
