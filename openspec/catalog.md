# Каталог возможностей

Все 15 capability — **accepted contracts** с 2026-09-27 по [baseline](baseline.md).
Таблица перечисляет обязательное поведение в границах каждого spec. Это не статус
«всё реализовано»: [матрица](evidence/coverage.md) связывает каждое требование с
кодом и проверками, [review](review.md) хранит известные расхождения.
Принятые DEC-001–008 встроены в требования; их карта находится в baseline.md.

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
| [runtime-settings](specs/runtime-settings/spec.md) | F3/F6/P7 | Демон/окно, комплект сборки, .env, автостарт | Есть расхождения Live vs snapshot; Windows-only автостарт |
| [installation-models](specs/installation-models/spec.md) | Инфраструктура | Скрипты моделей, hashes, пути, doctor | Проверка наличия не заменяет испытание модели |

## Предложения

Конкретные работы ведутся в штатных change-пакетах. Таблица — навигация;
актуальную готовность документов показывает `openspec status --change <name>`,
выполнение задач — `openspec instructions apply --change <name> --json`.
Семь changes имеют полный комплект документов, семь — только proposal.
BL-001/007/020 реализованы и проверены в исходниках; BL-002/018/019 выполнены частично.
[Сверка всех capability и выполненные проверки](evidence/implementation-review-2026-09-27.md)
показывает границы результатов. Исторический архив и установленная release-сборка
не обновлялись; наличие полного плана не означает завершённую работу.

| Изменение | Основание | Тема |
|---|---|---|
| [fix-ingest-ack](changes/fix-ingest-ack/proposal.md) | [BL-001](backlog.md#bl-001) | Подтверждать приём ссылки сохранением meta и очереди |
| [serialize-session-mutations](changes/serialize-session-mutations/proposal.md) | [BL-002](backlog.md#bl-002) | Согласовать владение сессией для изменяющих операций |
| [benchmark-processing-batch](changes/benchmark-processing-batch/proposal.md) | [BL-003](backlog.md#bl-003) | Измерить release-обработку пачки и предложить бюджет |
| [recover-corrupt-processing-artifacts](changes/recover-corrupt-processing-artifacts/proposal.md) | [BL-004](backlog.md#bl-004) | Восстанавливать повреждённые готовые артефакты |
| [verify-background-indexing](changes/verify-background-indexing/proposal.md) | [BL-005](backlog.md#bl-005) | Проверить индексатор вместе с очередью и поиском |
| [unify-manual-and-queued-import](changes/unify-manual-and-queued-import/proposal.md) | [BL-006](backlog.md#bl-006) | Объединить ручной импорт и фоновый Prepare |
| [allow-postprocess-without-audio](changes/allow-postprocess-without-audio/proposal.md) | [BL-007](backlog.md#bl-007) | Повторять Text/Summary после retention аудио |
| [explore-stt-worker-lifecycle](changes/explore-stt-worker-lifecycle/proposal.md) | [BL-008](backlog.md#bl-008) | Исследовать память импорта и постоянный STT-worker |
| [review-code-inventory](changes/review-code-inventory/proposal.md) | [BL-009](backlog.md#bl-009) | Завершённый аудит и принятие baseline с известными gaps |
| [automate-spec-evidence-checks](changes/automate-spec-evidence-checks/proposal.md) | [BL-010](backlog.md#bl-010) | Проверять связь спецификаций с кодом и тестами в CI |
| [fix-unresponsive-shutdown](changes/fix-unresponsive-shutdown/proposal.md) | [BL-018](backlog.md#bl-018) | Найти и устранить корневую причину зависания при остановке |
| [disable-cross-session-speaker-recognition](changes/disable-cross-session-speaker-recognition/proposal.md) | [BL-019](backlog.md#bl-019) | Отключить автоматическое узнавание людей между записями |
| [protect-retained-audio](changes/protect-retained-audio/proposal.md) | [BL-020](backlog.md#bl-020) | Хранить аудио по умолчанию и проверять STT перед opt-in очисткой |
| [persist-notes-before-delivery](changes/persist-notes-before-delivery/proposal.md) | [BL-021](backlog.md#bl-021) | Сохранить локальную заметку до доставки и подтверждать фактический результат |

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
P6: параллельны фазы разных сессий; бюджет «час за пять минут» не измерен.
P7: UI — клиент; история про обязательную общую шину не доказывает WS.
P8: явный выбор обязателен по DEC-006; неявный API default Claude остаётся дефектом RV-009.
P9: источники — файлы, индексы производные; обязательная meta.sqlite не обнаружена.

Список следующих идей сохранён в BL-011–017. Уже имеющиеся элементы (clipboard
команда, doctor, models.json, голосовые профили) не считаются отсутствующими только
потому, что были записаны в старый пул хотелок. Расширение этих возможностей —
отдельное предложение после сверки существующего кода.
