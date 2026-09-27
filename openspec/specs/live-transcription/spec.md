# Live transcription Specification

## Purpose

F2/F8. Быстрый текст Vosk, legacy WAV и восстановление; это отдельный контур от архивного STT.

Статус контракта: **accepted**, 2026-09-27. Это обязательное поведение,
а не заявление о безошибочности текущей сборки.
[Основание принятия и границы](../../baseline.md) ·
[Сверка каждого требования](../../evidence/coverage.md#live-transcription).

## Requirements

### Requirement: LIVE-001 Сегментация по источникам

Pipeline SHALL держать раздельные VAD-состояния для source 0/1, кадр 320 сэмплов при 16 kHz; завершать сегмент по достаточной тишине с min_chunk_sec или по max_chunk_sec. По умолчанию payload находится в RAM; segments_to_disk включает legacy .part → .wav.

Код: [SourceState::feed/run_vad/finalize](../../../crates/localvox-light-core/src/pipeline.rs).

#### Scenario: Два источника

- **WHEN** микрофон и loopback присылают PCM одновременно
- **THEN** состояние VAD одной дорожки не завершает сегмент другой.

#### Scenario: Пауза fast lane

- **WHEN** незаконченный disk-сегмент ещё имеет .part
- **THEN** он отбрасывается, не отправляется ASR.

### Requirement: LIVE-002 Загрузка модели и порядок

Capture/pipeline SHALL запускаться независимо от загрузки Vosk; сегменты ожидают модель в очереди. ASR распределяется по источнику для порядка фраз в voice-hook. Live-текст сохраняется в корневой transcript.jsonl; он не является best-версией архивной сессии.

Код: [run_engine, asr_worker_pool](../../../crates/localvox-light-core/src/engine.rs).

#### Scenario: Vosk ещё грузится

- **WHEN** поступают готовые сегменты
- **THEN** они накапливаются: в RAM по умолчанию, в WAV при legacy режиме.

### Requirement: INV-LIVE-001 Commit перед удалением legacy WAV

Для успешно распознанного дискового сегмента worker SHALL сначала вызвать TranscriptWriter::append и удалять WAV только при Ok. Ошибка append оставляет файл. Append делает writeln+flush, на Unix вызывает fsync; Windows sync_all отсутствует, код возврата Unix fsync не проверяется. Поэтому строгая гарантия сохранности при потере питания пока не подтверждается.

Код: [asr_thread_loop](../../../crates/localvox-light-core/src/engine.rs), [TranscriptWriter::append](../../../crates/localvox-light-core/src/transcript.rs).

#### Scenario: Ошибка записи текста

- **WHEN** append вернул ошибку
- **THEN** распознанный WAV не удаляется этой веткой.

#### Scenario: Сбой после append

- **WHEN** валидная строка с seg_id уже читается при следующем старте
- **THEN** recovery не ставит этот сегмент повторно.

### Requirement: LIVE-003 Восстановление старых сегментов

Startup SHALL удалять .part только в корне work_dir, выбирать корневые WAV без валидной записи seg_id в transcript.jsonl и сортировать их по source/sequence. Неизвестное имя получает fallback source 0. Частично испорченные строки JSONL пропускаются. Восстановленные фразы не передаются в voice-hook.

Код: [remove_orphan_part_files, recover_unprocessed](../../../crates/localvox-light-core/src/session.rs), [asr_thread_loop](../../../crates/localvox-light-core/src/engine.rs).

#### Scenario: Вчерашняя голосовая команда

- **WHEN** WAV восстановлен со старого запуска
- **THEN** текст дописывается, но команда «запиши» повторно не исполняется.

#### Scenario: Оборванный JSONL

- **WHEN** последняя строка не парсится, соответствующий WAV есть
- **THEN** seg_id не считается обработанным.

### Requirement: LIVE-004 Просмотр и сброс live-текста

Живой интерфейс SHALL читать сохранённый transcript.jsonl; операция очистки переоткрывает его с truncate. Экспорт dump сортирует строки по timestamp и source/sequence в отдельный JSONL; пустой набор и пустой путь dump дают ошибку.

Код: [reopen_truncated, read_all_entries, export_sorted_jsonl](../../../crates/localvox-light-core/src/transcript.rs).

#### Scenario: Очистка черновика

- **WHEN** выбрана очистка live-transcript
- **THEN** корневой файл обнуляется; это не удаление versions.json архивных сессий.

## Implementation status

RV-002: flush не даёт доказанной сохранности при потере питания; память очереди и реальный Vosk требуют отдельного прогона. Контракт не обещает power-loss durability.

[Матрица по требованиям](../../evidence/coverage.md#live-transcription) ·
[Расхождения RV](../../review.md) ·
[Выполненные проверки](../../evidence/implementation-review-2026-09-27.md).
Ниже сохранены дополнительные наблюдения о реализации, не исключения из требований.

## Observed limitations

Бесконечная очередь требует контроля памяти. Не обобщать WAV recovery на архивные audio/*.part. Ошибки питания/частичные JSONL и ветки отказов ASR требуют дополнительной проверки RV-002.
