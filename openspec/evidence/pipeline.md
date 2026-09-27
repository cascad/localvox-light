> Исторический отчёт этапа до принятия baseline. Актуальный статус контракта — в [baseline](../baseline.md), соответствие кода — в [матрице](coverage.md). Результаты ниже сохраняют исходную дату и объём.

> Миграция по коду: draft / awaiting-review. Предложения и ожидаемые исправления ниже не являются реализованными гарантиями. Наличие теста не означает его прогон при миграции.

# Сценарии приёмки PIPE

Спецификация: [полный пайплайн](../specs/processing-pipeline/spec.md). Таблица связывает ожидаемое поведение
с существующими проверками и показывает пробелы. Она не является исполняемым тестом.
`Есть автопроверка` относится к описанному сценарию, а не ко всем предложениям правила.

## Матрица сценариев

| ID / правило | Дано → действие → ожидаемый результат | Проверка / граница |
|---|---|---|
| SC-PIPE-01 / PIPE-001 | Корректная ссылка → добавить → сразу есть сессия с источником и задание, аудио ещё не требуется | Есть автопроверка: [ingest.rs](../../crates/localvox-light-core/src/ingest.rs), `a_link_becomes_an_empty_session_with_a_queued_job` |
| SC-PIPE-02 / PIPE-001 | Сохранение очереди невозможно → добавить ссылку → вызывающий получает ошибку, а не ложный успех | **Не выполнено:** найдено расхождение чтением кода, [BL-001](../backlog.md#bl-001); отказ диска на реальном архиве не воспроизводился |
| SC-PIPE-03 / PIPE-002 | Сессия ещё записывается или есть живой `.part` → сканирование → она не попадает в фоновый STT | Есть автопроверки: [jobs.rs](../../crates/localvox-light-core/src/jobs.rs), `discovery_skips_active_recording_session`, `discovery_skips_session_with_live_part_chunk` |
| SC-PIPE-04 / PIPE-003 | Сохранён только первый чанк видео → процесс прерван → Prepare не считается готовым; после полного commit готов | Есть автопроверка: [artifacts.rs](../../crates/localvox-light-core/src/artifacts.rs), `partial_ingest_is_not_ready_until_complete_audio_is_committed`; реальный обрыв yt-dlp отдельно не прогонялся |
| SC-PIPE-05 / PIPE-004, PIPE-008 | Обработчик завершился, результата нет либо его подтверждение не записалось → принять фазу → фаза не завершена | Есть автопроверки: [jobs.rs](../../crates/localvox-light-core/src/jobs.rs), `process_exit_cannot_finish_a_phase_without_verified_artifacts`, `failed_queue_commit_keeps_the_current_phase` |
| SC-PIPE-06 / PIPE-002, PIPE-010 | Прервана Summary, предыдущие результаты сохранены → восстановление → продолжение этой фазы без стирания предыдущего | Есть автопроверки: [jobs.rs](../../crates/localvox-light-core/src/jobs.rs), `summary_recovery_preserves_phase_and_rejects_stale_completion`, `an_interrupted_recook_does_not_wipe_the_archive_again_on_every_restart`; это проверка очереди, не полный аварийный e2e |
| SC-PIPE-07 / PIPE-009 | Несколько ожидающих сессий → захват разными линиями → разные фазы могут перекрываться, одна сессия не захватывается дважды | Есть автопроверка: [jobs.rs](../../crates/localvox-light-core/src/jobs.rs), `pipeline_overlaps_sessions_but_never_claims_one_twice`; реальная загрузка ресурсов — SC-PIPE-15 |
| SC-PIPE-08 / PIPE-005, PIPE-006, PIPE-014 | Есть исходный текст и подставная LLM → текстовая фаза, затем повтор → нет STT/сводки; повтор не делает новых запросов и версий | Есть интеграционная проверка: [check-pipeline.py](../../scripts/check-pipeline.py) |
| SC-PIPE-09 / PIPE-008 | Файл изменён с сохранением размера или указан чужой источник → проверка → `invalid`, а не готово | Есть автопроверки: [artifacts.rs](../../crates/localvox-light-core/src/artifacts.rs), `hashes_detect_same_length_output_and_input_changes`, `cleanup_must_reference_the_correct_version_and_lines` |
| SC-PIPE-10 / PIPE-011 | Задание ждало очереди; длительность видео другая → отображение → очередь и обработка разделены, неизвестный старт не даёт ноль | Есть автопроверки: [workflow.rs](../../crates/localvox-light-core/src/workflow.rs), `actual_video_wait_and_work_are_separate_and_media_length_is_irrelevant`, `queued_work_has_no_fake_processing_time` |
| SC-PIPE-11 / PIPE-011 | LLM отвечает дольше пяти секунд → ожидание и завершение → heartbeat есть, начало не сбрасывается, поздний heartbeat не затирает Done | Есть проверки: [progress.rs](../../crates/localvox-light-core/src/progress.rs), `heartbeats_preserve_start_and_retry_drops_old_progress`; [check-pipeline.py](../../scripts/check-pipeline.py) |
| SC-PIPE-12 / PIPE-010 | Обработчик создал потомков → отмена, таймаут или аварийная смерть владельца Windows → потомки прекращают работу | Есть OS-проверки: [lifecycle.rs](../../crates/localvox-light-process/tests/lifecycle.rs), `cancellation_stops_the_entire_tree`, `timeout_stops_the_entire_tree`, `nested_supervisors_remain_owned_by_the_outer_tree`, `abrupt_supervisor_death_closes_the_windows_job`; последняя только Windows |
| SC-PIPE-13 / PIPE-012 | Живой обработчик держит сессию → API повторной обработки → отказ без удаления результата | Есть автопроверка: [archive.rs](../../crates/localvox-light-api/src/archive.rs), `recook_does_not_invalidate_artifacts_owned_by_a_live_worker`; не распространять вывод на остальные изменяющие API |
| SC-PIPE-14 / PIPE-013 | Индексация работает → добавить запись → индексация останавливается, результаты записи остаются доступны | **Ручной прогон не выполнен**; точки кода `warm_indexes` в [autocook.rs](../../crates/localvox-light/src/autocook.rs) и `--worker-index` в [CLI](../../crates/localvox-light-asr/src/bin/localvox-process.rs) |
| SC-PIPE-15 / PIPE-009, PIPE-011 | Пять известных видео → обработка release-сборкой → измерены перекрытие фаз, ожидание, фактическое время и ресурсы | **Не выполнено**, [BL-003](../backlog.md#bl-003). Порог производительности не выводится из факта успешной сборки |
| SC-PIPE-16 / PIPE-010 | Процесс выводит большой ответ до чтения большого stdin или оставляет потомка с stdout → ожидание → нет deadlock/бесконечного ожидания EOF | Есть OS-проверки: [lifecycle.rs](../../crates/localvox-light-process/tests/lifecycle.rs), `simultaneous_large_input_and_output_cannot_deadlock`, `normal_parent_exit_cleans_descendants_without_waiting_for_pipes`, `output_is_bounded_even_if_child_already_exited` |
| SC-PIPE-17 / PIPE-002, PIPE-008 | Повреждён `jobs.json` или `processing.json` → чтение/изменение → ошибка видна, исходный повреждённый файл не заменён пустым | Есть проверки: [jobs.rs](../../crates/localvox-light-core/src/jobs.rs), `corrupted_queue_cannot_be_overwritten_or_claimed`; [artifacts.rs](../../crates/localvox-light-core/src/artifacts.rs), `corrupt_ledger_is_never_overwritten_by_record_forget_or_confirm` |
| SC-PIPE-18 / PIPE-004 | Проверяемая пустая расшифровка → следующий запуск → подтверждённый пустой результат не превращается в вечную генерацию | Есть автопроверка: [processing.rs](../../crates/localvox-light-core/src/processing.rs), `nothing_to_do_is_done_and_never_loops`; живой STT тишины — отдельная приёмка |

## Последнее известное свидетельство

2026-09-27, Windows, локальное рабочее дерево `new_course` без отдельного коммита,
debug. Результаты получены на предыдущем этапе этой работы, при реализации контура:

| Команда | Результат и ограничение |
|---|---|
| `cargo test -p localvox-light-core -p localvox-light-api -p localvox-light-llm --lib` | Запущенные тесты прошли; не является e2e с реальными моделями/устройствами |
| `cargo test -p localvox-light-process` | 8 OS-тестов прошли; один служебный fixture вызывается дочерними процессами и помечен ignored в обычном запуске |
| `cargo test -p localvox-light --bin localvox-light` | Проверка обработки диагностики прошла |
| `cargo build -p localvox-light -p localvox-light-asr --bin localvox-light --bin localvox-process` | Debug-бинарники собраны; release и установка не подтверждены |
| `python scripts/check-pipeline.py` | Протокол, изоляция фазы, heartbeat, подтверждения, повтор без LLM-запросов и ошибка отсутствующего результата прошли на временном архиве с локальной подставной LLM |

Это историческая отметка, не автоматически актуальный зелёный статус. При изменении
поведения обновить её или добавить новое свидетельство. В изменении только этих
документов runtime-тесты повторно не запускались; проверялись ссылки и привязки к коду.

## Ручная приёмка нового релиза

Использовать отдельный тестовый архив. Записать ревизию/dirty, хеши бинарников,
Windows/CPU/GPU/RAM, модели и провайдеры, настройки, перечень источников и их длительность.
Провести SC-PIPE-14/15 и аварийное возобновление на копии данных; собрать `jobs.json`,
`progress.jsonl`, `processing.json`, результаты и журнал процессов.

Для каждого видео сравнить очередь, время фаз, общее время и наличие подтверждённых
результатов; для пачки — перекрытие фаз и общее время пачки. При повторе без изменения
входов проверить отсутствие новых версий/генерации. Зафиксировать ожидание и факт,
а не только «вроде быстрее». Проверку точности текста/сводки проводить отдельно от
проверки сохранности файлов. Старый пример видео разобран в [памятке](../../docs/processing-pipeline.md).
