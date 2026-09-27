# Покрытие требований и состояние реализации

Дата сверки: 2026-09-27, `new_course`, HEAD `6dfde241bbeff1736829b0bf94c8d4c1f84b1517`
плюс рабочее дерево. Это свидетельства, а не второй набор требований. Полный текст
контракта находится по ссылке ID, дополнительные входные точки — в самом требовании.

- `code-reviewed`: путь реализации прочитан; сценарий не объявляется пройденным.
- `test-run (частично)`: указанные тесты выполнены, охватывают часть требования;
  это не обещание полной проверки каждого corner case.
- `runtime-mock`: изолированный реальный бинарник с синтетическими данными/назначением.
- `GAP`: известное нарушение принятого требования, а не разрешённое исключение.
  Остальные ограничения не обязательно означают баг: они ограничивают доказательство.

Команды и результаты — [журнал](implementation-review-2026-09-27.md),
основание принятия — [baseline](../baseline.md), расхождения и работы — [review](../review.md).
`test:name` — имя реально существующего теста, не автоматическое доказательство его
нового прогона. Полный workspace-прогон: 548 passed, затем отдельно три API-теста
и историческая копия; новый mock заметок описан в конце журнала.
Проверка полноты и ссылок: `python scripts/check-openspec.py`.

## ad-hoc-asks

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [ASK-001](../specs/ad-hoc-asks/spec.md#requirement-ask-001-приём-материала) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs), [src/asks.rs](../../crates/localvox-light-core/src/asks.rs), [src/Ask.tsx](../../ui/src/Ask.tsx) | test-run (частично): `test:effective_prompt_falls_back_to_default`, `test:save_and_load_roundtrip_and_files_split_by_mutability`; runtime-mock ([журнал](implementation-review-2026-09-27.md)) | GAP RV-009: omitted provider = Claude; unknown provider не отклоняется. |
| [ASK-002](../specs/ad-hoc-asks/spec.md#requirement-ask-002-отдельная-очередь) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs), [src/asks.rs](../../crates/localvox-light-core/src/asks.rs) | test-run (частично): `test:a_failed_ask_is_saved_and_marked`, `test:pending_is_fifo_and_reclaim_revives_running` | RV-009: прямые записи файлов, crash/неопределённый облачный повтор не проверены. |
| [ASK-003](../specs/ad-hoc-asks/spec.md#requirement-ask-003-история-и-удаление) | [src/asks.rs](../../crates/localvox-light-core/src/asks.rs), [src/Ask.tsx](../../ui/src/Ask.tsx) | test-run (частично): `test:delete_removes_the_folder_and_is_idempotent` | RV-009: удаление Running не координируется с worker. |

## api-access

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [API-001](../specs/api-access/spec.md#requirement-api-001-обычная-авторизация-http) | [src/http.rs](../../crates/localvox-light-api/src/http.rs) | code-reviewed; runtime-mock ([журнал](implementation-review-2026-09-27.md)) | Проверен изолированный loopback HTTP; LAN не проверялась. |
| [API-002](../specs/api-access/spec.md#requirement-api-002-исключение-для-media-url) | [src/http.rs](../../crates/localvox-light-api/src/http.rs), [src/Player.tsx](../../ui/src/Player.tsx) | code-reviewed; runtime-mock ([журнал](implementation-review-2026-09-27.md)) | GAP RV-003: media пропускает Host/Origin; в mock дошли до 404 отсутствующей сессии. |
| [API-003](../specs/api-access/spec.md#requirement-api-003-ограничение-http-работы) | [src/http.rs](../../crates/localvox-light-api/src/http.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |
| [API-004](../specs/api-access/spec.md#requirement-api-004-mcp-сервер-файлового-архива) | [src/lib.rs](../../crates/localvox-light-api/src/lib.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |
| [API-005](../specs/api-access/spec.md#requirement-api-005-один-web-ui-для-desktop-и-lan) | [src/http.rs](../../crates/localvox-light-api/src/http.rs), [src/api.ts](../../ui/src/api.ts), [src/lib.rs](../../src-tauri/src/lib.rs) | code-reviewed | Сборка UI/desktop пройдена, browser/mobile/offline приёмка не выполнялась. |

## archive-playback

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [ARCH-001](../specs/archive-playback/spec.md#requirement-arch-001-адресация-и-чтение) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | test-run (частично): `test:session_dir_rejects_everything_but_plain_names`, `test:verified_transcript_remains_readable_when_summary_fails` | Отдельный runtime всех сценариев не выполнен. |
| [ARCH-002](../specs/archive-playback/spec.md#requirement-arch-002-один-плеер-и-граница-реплики) | [src/Player.tsx](../../ui/src/Player.tsx) | code-reviewed | Живой browser seek/длинный FLAC не прогонялись. |
| [ARCH-003](../specs/archive-playback/spec.md#requirement-arch-003-аудио-через-чанки) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs), [src/http.rs](../../crates/localvox-light-api/src/http.rs) | test-run (частично): `test:audio_clip_extracts_sample_accurate_range`, `test:audio_clip_errors_when_overlapping_chunk_unreadable`, `test:mixing_two_loud_tracks_does_not_wrap_into_noise` | Отдельный runtime всех сценариев не выполнен. |
| [ARCH-004](../specs/archive-playback/spec.md#requirement-arch-004-экспорт-best) | [src/export.rs](../../crates/localvox-light-core/src/export.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |
| [ARCH-005](../specs/archive-playback/spec.md#requirement-arch-005-явное-удаление) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | test-run (частично): `test:delete_needs_the_session_name_and_is_final` | GAP BL-002: занятый worker пока получает отказ, controlled stop до удаления не реализован. |

## audio-storage

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [STORE-001](../specs/audio-storage/spec.md#requirement-store-001-публикация-чанка) | [src/chunks.rs](../../crates/localvox-light-core/src/chunks.rs) | test-run (частично): `test:streaming_wav_writes_valid_header`, `test:empty_wav_is_removed_on_finalize` | Отдельный runtime всех сценариев не выполнен. |
| [INV-STORE-001](../specs/audio-storage/spec.md#requirement-inv-store-001-восстановление-архивных-part) | [src/chunks.rs](../../crates/localvox-light-core/src/chunks.rs) | test-run (частично): `test:recover_orphan_patches_and_renames`, `test:recovered_chunk_is_registered_in_meta_after_previous_ones` | Отдельный runtime всех сценариев не выполнен. |
| [STORE-002](../specs/audio-storage/spec.md#requirement-store-002-flac-с-сохранением-fallback) | [src/chunks.rs](../../crates/localvox-light-core/src/chunks.rs) | code-reviewed | GAP RV-005: нет проверки полноты FLAC до удаления WAV и надзора за конвертером. |
| [STORE-003](../specs/audio-storage/spec.md#requirement-store-003-retention-только-аудио) | [src/chunks.rs](../../crates/localvox-light-core/src/chunks.rs), [src/cli.rs](../../crates/localvox-light-core/src/cli.rs) | code-reviewed | BL-020 реализован в исходниках, работающий release не обновлён. |
| [INV-STORE-002](../specs/audio-storage/spec.md#requirement-inv-store-002-один-движок-на-каталог) | [src/session.rs](../../crates/localvox-light-core/src/session.rs), [src/engine.rs](../../crates/localvox-light-core/src/engine.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |

## installation-models

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [SETUP-001](../specs/installation-models/spec.md#requirement-setup-001-манифест-загрузки) | [scripts/models.json](../../scripts/models.json), [scripts/fetch-models.ps1](../../scripts/fetch-models.ps1), [scripts/fetch-models.sh](../../scripts/fetch-models.sh) | code-reviewed | Check и сборка пройдены; новые загрузки не выполнялись. |
| [INV-SETUP-001](../specs/installation-models/spec.md#requirement-inv-setup-001-скачивание-через-временный-файл) | [scripts/fetch-models.ps1](../../scripts/fetch-models.ps1), [scripts/fetch-models.sh](../../scripts/fetch-models.sh) | code-reviewed | Скачивание/hash mismatch в этом проходе не воспроизводились. |
| [SETUP-002](../specs/installation-models/spec.md#requirement-setup-002-разрешение-зависимостей) | [localvox-light-core/build.rs](../../crates/localvox-light-core/build.rs), [src/lang.rs](../../crates/localvox-light-core/src/lang.rs), [src/doctor.rs](../../crates/localvox-light-core/src/doctor.rs) | code-reviewed | Dev bundle собран; прочие ОС и новые варианты установки не прогонялись. |
| [SETUP-003](../specs/installation-models/spec.md#requirement-setup-003-doctor-и-границы-проверки) | [src/doctor.rs](../../crates/localvox-light-core/src/doctor.rs), [src/main.rs](../../crates/localvox-light/src/main.rs) | code-reviewed | Наличие/доступность не равны качеству inference; реальный набор моделей не переустанавливался. |

## live-transcription

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [LIVE-001](../specs/live-transcription/spec.md#requirement-live-001-сегментация-по-источникам) | [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |
| [LIVE-002](../specs/live-transcription/spec.md#requirement-live-002-загрузка-модели-и-порядок) | [src/engine.rs](../../crates/localvox-light-core/src/engine.rs) | code-reviewed | Реальная загрузка Vosk/нагрузка очереди не проверена. |
| [INV-LIVE-001](../specs/live-transcription/spec.md#requirement-inv-live-001-commit-перед-удалением-legacy-wav) | [src/engine.rs](../../crates/localvox-light-core/src/engine.rs), [src/transcript.rs](../../crates/localvox-light-core/src/transcript.rs) | code-reviewed | RV-002: Windows flush без sync_all, Unix fsync error игнорируется; power-loss гарантия не принята. |
| [LIVE-003](../specs/live-transcription/spec.md#requirement-live-003-восстановление-старых-сегментов) | [src/session.rs](../../crates/localvox-light-core/src/session.rs), [src/engine.rs](../../crates/localvox-light-core/src/engine.rs) | code-reviewed | RV-002: recovery пропускает битые строки; физический отказ диска не проверен. |
| [LIVE-004](../specs/live-transcription/spec.md#requirement-live-004-просмотр-и-сброс-live-текста) | [src/transcript.rs](../../crates/localvox-light-core/src/transcript.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |

## llm-processing

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [LLM-001](../specs/llm-processing/spec.md#requirement-llm-001-отсутствие-речи-и-короткая-запись) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs) | test-run (частично): `test:babbling_has_no_words_at_all`, `test:a_short_thought_is_real_speech_and_gets_the_note_shape` | Отдельный runtime всех сценариев не выполнен. |
| [LLM-002](../specs/llm-processing/spec.md#requirement-llm-002-глоссарий-и-шаблоны) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs), [src/templates.rs](../../crates/localvox-light-llm/src/templates.rs), [src/glossary.rs](../../crates/localvox-light-llm/src/glossary.rs) | test-run (частично): `test:glossary_canon_is_grounded_in_the_final_check` | Отдельный runtime всех сценариев не выполнен. |
| [LLM-003](../specs/llm-processing/spec.md#requirement-llm-003-refine-и-cleanup-по-строкам) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs), [src/readable.rs](../../crates/localvox-light-core/src/readable.rs) | test-run (частично): `test:an_essay_instead_of_the_lines_costs_the_cleanup_not_the_text`, `test:lines_the_model_never_answered_for_keep_their_own_wording`, `test:one_oversized_line_goes_alone_rather_than_in_half` | Более сильное редакторское переформулирование не принято без примеров до/после. |
| [LLM-004](../specs/llm-processing/spec.md#requirement-llm-004-сводка-и-ограничения-проверки) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs), [src/grounding.rs](../../crates/localvox-light-llm/src/grounding.rs) | test-run (частично): `test:a_doubtful_result_is_still_written_as_the_real_document`, `test:a_citation_that_does_not_check_out_is_dropped_but_the_claim_stays`, `test:a_looping_summary_is_cut_at_its_first_repetition` | Эвристики не доказывают точность содержания; живое качество не измерено. |
| [LLM-005](../specs/llm-processing/spec.md#requirement-llm-005-выбор-провайдера-и-claude-cli) | [src/lib.rs](../../crates/localvox-light-llm/src/lib.rs), [src/claude_cli.rs](../../crates/localvox-light-llm/src/claude_cli.rs) | test-run (частично): `test:api_key_vars_are_always_stripped_from_the_child`, `test:non_json_is_reported`, `test:empty_result_fails` | Выбор клиента прочитан; реальный cloud не вызывался. Отдельный API asks имеет GAP ASK-001. |
| [LLM-006](../specs/llm-processing/spec.md#requirement-llm-006-подтверждение-сомнения) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs), [src/processing.rs](../../crates/localvox-light-core/src/processing.rs) | test-run (частично): `test:confirming_a_doubt_removes_the_plaque_for_good` | GAP RV-016: confirm скрывает checksum/I/O отказ за ответом об отсутствии пометки. |

## note-integrations

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [NOTE-001](../specs/note-integrations/spec.md#requirement-note-001-разрешение-слота) | [src/lib.rs](../../crates/localvox-light-integrations/src/lib.rs) | test-run (частично): `test:file_append_with_aliases_and_default` | Отдельный runtime всех сценариев не выполнен. |
| [NOTE-002](../specs/note-integrations/spec.md#requirement-note-002-файл-или-каталог) | [src/lib.rs](../../crates/localvox-light-integrations/src/lib.rs) | test-run (частично): `test:folder_target_creates_file_per_note`; runtime-mock ([журнал](implementation-review-2026-09-27.md)) | GAP RV-007 / BL-021: совпавшие timestamp+slug перезаписывают файл. |
| [NOTE-003](../specs/note-integrations/spec.md#requirement-note-003-чтение-и-удаление-заметок) | [src/lib.rs](../../crates/localvox-light-integrations/src/lib.rs), [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |
| [NOTE-004](../specs/note-integrations/spec.md#requirement-note-004-mcp-вызов-назначения) | [src/mcp.rs](../../crates/localvox-light-integrations/src/mcp.rs) | code-reviewed; runtime-mock ([журнал](implementation-review-2026-09-27.md)) | MCP mock: отказ/timeout/повтор проверены; Drop убивает только непосредственный процесс. |
| [NOTE-005](../specs/note-integrations/spec.md#requirement-note-005-suggest-then-confirm) | [src/routing.rs](../../crates/localvox-light-llm/src/routing.rs), [src/Notes.tsx](../../ui/src/Notes.tsx) | test-run (частично): `test:parses_and_validates_names` | Отдельный runtime всех сценариев не выполнен. |
| [NOTE-006](../specs/note-integrations/spec.md#requirement-note-006-локальная-сохранность-и-статус-доставки) | [src/lib.rs](../../crates/localvox-light-voice/src/lib.rs), [src/lib.rs](../../crates/localvox-light-integrations/src/lib.rs), [src/mcp.rs](../../crates/localvox-light-integrations/src/mcp.rs) | code-reviewed; runtime-mock ([журнал](implementation-review-2026-09-27.md)) | GAP RV-007 / BL-021: локальное хранилище с delivery status/retry не реализовано; lost-ack даёт дубль. |

## processing-pipeline

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [PIPE-001](../specs/processing-pipeline/spec.md#requirement-pipe-001-приём-источника-и-постановка) | [src/ingest.rs](../../crates/localvox-light-core/src/ingest.rs), [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs) | test-run (частично): `test:a_link_becomes_an_empty_session_with_a_queued_job` | Отдельный runtime всех сценариев не выполнен. |
| [PIPE-002](../specs/processing-pipeline/spec.md#requirement-pipe-002-захват-работы-и-восстановление) | [src/jobs.rs](../../crates/localvox-light-core/src/jobs.rs), [src/autocook.rs](../../crates/localvox-light/src/autocook.rs) | test-run (частично): `test:corrupted_queue_cannot_be_overwritten_or_claimed` | Отдельный runtime всех сценариев не выполнен. |
| [PIPE-003](../specs/processing-pipeline/spec.md#requirement-pipe-003-prepare--получение-полного-аудио) | [src/prepare.rs](../../crates/localvox-light-asr/src/prepare.rs), [src/artifacts.rs](../../crates/localvox-light-core/src/artifacts.rs) | test-run (частично): `test:partial_ingest_is_not_ready_until_complete_audio_is_committed` | Отдельный runtime всех сценариев не выполнен. |
| [PIPE-004](../specs/processing-pipeline/spec.md#requirement-pipe-004-transcribe--аудио-в-версию-текста) | [src/cook.rs](../../crates/localvox-light-core/src/cook.rs), [bin/localvox-process.rs](../../crates/localvox-light-asr/src/bin/localvox-process.rs) | test-run (частично): `test:nothing_to_do_is_done_and_never_loops` | Отдельный runtime всех сценариев не выполнен. |
| [PIPE-005](../specs/processing-pipeline/spec.md#requirement-pipe-005-textrefine--исправление-распознавания) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs) | code-reviewed | Живое качество исправлений не оценено; более сильная переформулировка требует примеров. |
| [PIPE-006](../specs/processing-pipeline/spec.md#requirement-pipe-006-textcleanup--читаемый-текст) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs), [src/readable.rs](../../crates/localvox-light-core/src/readable.rs) | test-run (частично): `test:cleanup_must_reference_the_correct_version_and_lines` | Отдельный runtime всех сценариев не выполнен. |
| [PIPE-007](../specs/processing-pipeline/spec.md#requirement-pipe-007-summary--сводка-и-её-проверка) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs) | code-reviewed | Реальная скорость/качество не приняты; известный старый Summary 242.248 с не общий benchmark. |
| [PIPE-008](../specs/processing-pipeline/spec.md#requirement-pipe-008-принятие-результата-и-переход) | [src/jobs.rs](../../crates/localvox-light-core/src/jobs.rs), [src/artifacts.rs](../../crates/localvox-light-core/src/artifacts.rs) | test-run (частично): `test:process_exit_cannot_finish_a_phase_without_verified_artifacts`, `test:failed_queue_commit_keeps_the_current_phase`, `test:hashes_detect_same_length_output_and_input_changes` | Отдельный runtime всех сценариев не выполнен. |
| [PIPE-009](../specs/processing-pipeline/spec.md#requirement-pipe-009-параллелизм-и-владение-ресурсами) | [src/autocook.rs](../../crates/localvox-light/src/autocook.rs), [src/jobs.rs](../../crates/localvox-light-core/src/jobs.rs) | code-reviewed | BL-003/008: CPU/GPU throughput и память пачки не измерены. |
| [PIPE-010](../specs/processing-pipeline/spec.md#requirement-pipe-010-ошибки-таймауты-отмена-и-повтор) | [src/lib.rs](../../crates/localvox-light-process/src/lib.rs), [src/jobs.rs](../../crates/localvox-light-core/src/jobs.rs) | test-run (частично): `test:cancellation_stops_the_entire_tree`, `test:timeout_stops_the_entire_tree` | GAP BL-002/004: нет пользовательской отмены без рестарта и автоматического ремонта Done; OS cancel проверен отдельно. |
| [PIPE-011](../specs/processing-pipeline/spec.md#requirement-pipe-011-честные-время-и-наблюдаемость) | [src/workflow.rs](../../crates/localvox-light-core/src/workflow.rs), [src/progress.rs](../../crates/localvox-light-core/src/progress.rs), [src/Progress.tsx](../../ui/src/Progress.tsx) | test-run (частично): `test:heartbeats_preserve_start_and_retry_drops_old_progress`; runtime-mock ([журнал](implementation-review-2026-09-27.md)) | Исторические missing boundaries остаются неизвестными; не выводить cleanup=0. Mock не задаёт SLA. |
| [PIPE-012](../specs/processing-pipeline/spec.md#requirement-pipe-012-повторная-обработка-и-сохранение-первичных-данных) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | test-run (частично): `test:recook_does_not_invalidate_artifacts_owned_by_a_live_worker` | BL-002: lease есть, controlled cancel и общая crash-транзакция отсутствуют. |
| [PIPE-013](../specs/processing-pipeline/spec.md#requirement-pipe-013-индексы-чтение-и-срок-хранения) | [src/autocook.rs](../../crates/localvox-light/src/autocook.rs), [src/chunks.rs](../../crates/localvox-light-core/src/chunks.rs) | code-reviewed | BL-005: живая совместная индексация/поиск/отмена не прогонялись. |
| [PIPE-014](../specs/processing-pipeline/spec.md#requirement-pipe-014-другие-входы-и-совместимость) | [bin/localvox-process.rs](../../crates/localvox-light-asr/src/bin/localvox-process.rs) | code-reviewed | BL-006: прямой CLI import пока отдельный путь; гарантию фонового Prepare не переносить. |
| [INV-INGEST-001](../specs/processing-pipeline/spec.md#requirement-inv-ingest-001-durable-ingest-acknowledgement) | [src/ingest.rs](../../crates/localvox-light-core/src/ingest.rs), [src/jobs.rs](../../crates/localvox-light-core/src/jobs.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |

## recording

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [REC-001](../specs/recording/spec.md#requirement-rec-001-явное-начало-архивной-записи) | [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs), [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | code-reviewed | RV-006: сохранение request не подтверждает аудио; честный ответ при отказе ещё нужен. |
| [REC-002](../specs/recording/spec.md#requirement-rec-002-pre-roll-и-время-начала) | [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs) | code-reviewed | Ограниченный RAM fallback допускает потерю при длительном отказе диска; физический отказ не прогонялся. |
| [REC-003](../specs/recording/spec.md#requirement-rec-003-stop-и-метки-звонка) | [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |
| [INV-REC-001](../specs/recording/spec.md#requirement-inv-rec-001-разные-границы-паузы) | [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs), [src/chunks.rs](../../crates/localvox-light-core/src/chunks.rs) | test-run (частично): `test:recorder_pause_closes_and_resumes_new_chunk` | Отдельный runtime всех сценариев не выполнен. |
| [REC-004](../specs/recording/spec.md#requirement-rec-004-потеря-устройства-и-отставание) | [src/engine.rs](../../crates/localvox-light-core/src/engine.rs), [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs) | test-run (частично): `test:a_backlog_in_our_own_queue_is_not_a_loss`, `test:a_pause_is_not_a_loss` | Устройства/длительная нагрузка не проверены; текущая unbounded очередь может расти. |
| [REC-005](../specs/recording/spec.md#requirement-rec-005-завершение-движка) | [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs), [src/engine.rs](../../crates/localvox-light-core/src/engine.rs) | code-reviewed | RV-014 / BL-018: исходное зависание release не локализовано. |
| [INV-REC-002](../specs/recording/spec.md#requirement-inv-rec-002-controls-independent-of-pcm) | [src/engine.rs](../../crates/localvox-light-core/src/engine.rs), [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs), [src/cli.rs](../../crates/localvox-light-core/src/cli.rs) | code-reviewed | BL-018: синтетический тест без устройства; native загрузка сама не прерывается. |

## runtime-settings

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [RUN-001](../specs/runtime-settings/spec.md#requirement-run-001-окно-отдельно-от-движка) | [src/lib.rs](../../src-tauri/src/lib.rs) | code-reviewed | Только сборка desktop; TCP listener не подтверждает именно LocalVox. |
| [RUN-002](../specs/runtime-settings/spec.md#requirement-run-002-комплект-бинарников-и-ui) | [src/autocook.rs](../../crates/localvox-light/src/autocook.rs), [src/http.rs](../../crates/localvox-light-api/src/http.rs), [../Cargo.toml](../../Cargo.toml) | code-reviewed | Совместный dev bundle собран; работающий release не перезапускался. |
| [CFG-001](../specs/runtime-settings/spec.md#requirement-cfg-001-сохранение-настроек) | [src/http.rs](../../crates/localvox-light-api/src/http.rs), [src/env_file.rs](../../crates/localvox-light-core/src/env_file.rs), [src/settings.rs](../../crates/localvox-light-core/src/settings.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |
| [CFG-002](../specs/runtime-settings/spec.md#requirement-cfg-002-область-действия-настроек) | [src/settings.rs](../../crates/localvox-light-core/src/settings.rs), [src/autocook.rs](../../crates/localvox-light/src/autocook.rs) | code-reviewed | GAP RV-001: Live label расходится со snapshot worker. |
| [CFG-003](../specs/runtime-settings/spec.md#requirement-cfg-003-показ-секретов-и-значений) | [src/http.rs](../../crates/localvox-light-api/src/http.rs), [src/settings.rs](../../crates/localvox-light-core/src/settings.rs) | code-reviewed; runtime-mock ([журнал](implementation-review-2026-09-27.md)) | GAP RV-010: synthetic unknown env value выдан через extra. |
| [RUN-003](../specs/runtime-settings/spec.md#requirement-run-003-windows-автостарт) | [src/autostart.rs](../../crates/localvox-light-core/src/autostart.rs) | code-reviewed | Модуль Windows; реальный HKCU не менялся, другие ОС не приняты. |
| [INV-RUN-001](../specs/runtime-settings/spec.md#requirement-inv-run-001-component-shutdown-diagnostics) | [src/engine.rs](../../crates/localvox-light-core/src/engine.rs), [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs), [src/cli.rs](../../crates/localvox-light-core/src/cli.rs) | code-reviewed | BL-018: unit timeout/panic пройдены; причина исходного release-инцидента неизвестна. |

## search-chat

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [SEARCH-001](../specs/search-chat/spec.md#requirement-search-001-индексируемые-данные) | [src/lib.rs](../../crates/localvox-light-search/src/lib.rs), [src/semantic.rs](../../crates/localvox-light-search/src/semantic.rs) | test-run (частично): `test:finds_stemmed_russian_word_with_timecode`, `test:finds_in_summary_paragraphs` | BL-005: полный живой индексатор и обновление при очереди не прогонялись. |
| [SEARCH-002](../specs/search-chat/spec.md#requirement-search-002-режимы-и-деградация) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | code-reviewed | GAP RV-008: две ошибки превращаются в пустой список. |
| [SEARCH-003](../specs/search-chat/spec.md#requirement-search-003-занятый-semantic) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | code-reviewed | BL-005: конкуренция с живым прогревом не проверена. |
| [CHAT-001](../specs/search-chat/spec.md#requirement-chat-001-ответ-только-после-поиска) | [src/chat.rs](../../crates/localvox-light-api/src/chat.rs) | test-run (частично): `test:a_hit_is_expanded_into_the_conversation_around_it` | GAP RV-008: ошибка поиска может стать «не найдено». |
| [CHAT-002](../specs/search-chat/spec.md#requirement-chat-002-ссылки-и-сомнения-ответа) | [src/chat.rs](../../crates/localvox-light-api/src/chat.rs) | test-run (частично): `test:citations_are_parsed_and_invented_ones_are_rejected`, `test:a_refusal_is_a_valid_answer_without_citations` | Живое качество grounded-ответов не проверено. |

## speakers

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [SPK-001](../specs/speakers/spec.md#requirement-spk-001-опциональная-диаризация) | [diarize/mod.rs](../../crates/localvox-light-core/src/diarize/mod.rs), [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | test-run (частично): `test:participants_come_from_the_lines_not_from_a_register` | Качество реальной диаризации не измерено. |
| [SPK-002](../specs/speakers/spec.md#requirement-spk-002-источник-и-человек--разные-подписи) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs), [diarize/roster.rs](../../crates/localvox-light-core/src/diarize/roster.rs), [diarize/mod.rs](../../crates/localvox-light-core/src/diarize/mod.rs) | test-run (частично): `test:naming_reports_queue_failure_after_local_save_without_claiming_a_rebuild` | BL-019: реализация проверена, несколько файлов не образуют crash-транзакцию. |
| [SPK-003](../specs/speakers/spec.md#requirement-spk-003-профиль-голоса) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs), [diarize/roster.rs](../../crates/localvox-light-core/src/diarize/roster.rs), [diarize/mod.rs](../../crates/localvox-light-core/src/diarize/mod.rs) | test-run (частично): `test:historical_speaker_correction_on_temporary_copy_preserves_speech` | BL-019: историческая копия проверена; настоящий архив и работающий release не изменены. |

## transcript-versions

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [VER-001](../specs/transcript-versions/spec.md#requirement-ver-001-версии-и-best) | [src/versions.rs](../../crates/localvox-light-core/src/versions.rs) | test-run (частично): `test:commit_requires_existing_file`, `test:commit_rejects_duplicate_id`, `test:best_defaults_to_latest_until_set` | Чтение legacy load может скрыть битый manifest; строгая запись отдельно, multi-file crash не проверен. |
| [VER-002](../specs/transcript-versions/spec.md#requirement-ver-002-язык-выбирает-модель) | [src/lang.rs](../../crates/localvox-light-core/src/lang.rs), [src/chunks.rs](../../crates/localvox-light-core/src/chunks.rs) | code-reviewed | Реальные ASR-модели всех языков не прогонялись. |
| [VER-003](../specs/transcript-versions/spec.md#requirement-ver-003-api-смены-языка-и-best) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | test-run (частично): `test:language_is_restored_when_recook_cannot_be_queued` | Смена best не генерирует сводку; проверяется её связь с источником. Нет общей crash-транзакции. |
| [INV-VER-001](../specs/transcript-versions/spec.md#requirement-inv-ver-001-текст-и-имена-имеют-разную-изменяемость) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs), [src/versions.rs](../../crates/localvox-light-core/src/versions.rs), [src/artifacts.rs](../../crates/localvox-light-core/src/artifacts.rs) | test-run (частично): `test:naming_a_voice_renames_it_in_every_version` | Метки speaker меняются; текст/таймкоды сохраняются. Нет общей транзакции relabel. |
| [VER-004](../specs/transcript-versions/spec.md#requirement-ver-004-повторы-по-scope) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs), [bin/localvox-process.rs](../../crates/localvox-light-asr/src/bin/localvox-process.rs) | test-run (частично): `test:text_and_summary_recook_without_audio_start_at_the_requested_phase`, `test:full_recook_without_audio_preserves_the_last_results`, `test:postprocess_rejects_missing_malformed_and_unconfirmed_empty_transcripts`, `test:recook_does_not_invalidate_artifacts_owned_by_a_live_worker` | Отдельный runtime всех сценариев не выполнен. |

## voice-notes

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [VOICE-001](../specs/voice-notes/spec.md#requirement-voice-001-команды-только-с-микрофона) | [src/lib.rs](../../crates/localvox-light-voice/src/lib.rs) | test-run (частично): `test:sys_source_never_triggers_commands`, `test:custom_commands_with_fixed_slot_and_multiword_activator` | Отдельный runtime всех сценариев не выполнен. |
| [VOICE-002](../specs/voice-notes/spec.md#requirement-voice-002-жизненный-цикл-диктовки) | [src/lib.rs](../../crates/localvox-light-voice/src/lib.rs) | test-run (частично): `test:multi_phrase_note_accumulates_and_closes_on_idle_tick`, `test:new_command_closes_previous_note`, `test:e2e_hook_flushes_open_note_on_shutdown` | Отдельный runtime всех сценариев не выполнен. |
| [VOICE-003](../specs/voice-notes/spec.md#requirement-voice-003-ретро--недавний-текст) | [src/lib.rs](../../crates/localvox-light-voice/src/lib.rs) | test-run (частично): `test:retro_captures_recent_conversation_from_both_sources`, `test:retro_on_empty_window_says_nothing_to_save` | Отдельный runtime всех сценариев не выполнен. |
| [VOICE-004](../specs/voice-notes/spec.md#requirement-voice-004-эхо-и-подтверждение) | [src/lib.rs](../../crates/localvox-light-voice/src/lib.rs), [src/tts.rs](../../crates/localvox-light-voice/src/tts.rs) | test-run (частично): `test:confirmation_echo_does_not_restart_command_loop`, `test:own_tts_echo_including_fragments_and_fuzzy` | GAP RV-007 / BL-021: успех Say/Status сформирован до результата Write; нет local-first. |
| [VOICE-005](../specs/voice-notes/spec.md#requirement-voice-005-отдельные-команды-встречи-и-ссылки) | [src/lib.rs](../../crates/localvox-light-voice/src/lib.rs) | test-run (частично): `test:a_meeting_can_be_started_by_voice_with_a_name`, `test:a_link_is_taken_from_the_clipboard_by_voice`, `test:a_note_about_a_link_is_still_a_note` | Отдельный runtime всех сценариев не выполнен. |

