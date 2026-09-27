# Audio storage Specification

## Purpose

F8/P1/P3. Архивные чанки, восстановление, FLAC и retention.

Статус контракта: **accepted**, 2026-09-27. Это обязательное поведение,
а не заявление о безошибочности текущей сборки.
[Основание принятия и границы](../../baseline.md) ·
[Сверка каждого требования](../../evidence/coverage.md#audio-storage).

## Requirements

### Requirement: STORE-001 Публикация чанка

ChunkRecorder SHALL писать mono PCM16 16 kHz в .part, при закрытии обновлять WAV-заголовок и переименовывать в .wav; пустой файл удаляется. Метаданные содержат source, offset, duration, RMS и voice_ratio. Ротация текущего архивного ChunkRecorder основана на числе сэмплов chunk_sec, а не VAD-тишине.

Код: [StreamingWav, ChunkRecorder::feed/close_current](../../../crates/localvox-light-core/src/chunks.rs).
Существующая проверка: `streaming_wav_writes_valid_header`, `empty_wav_is_removed_on_finalize`.

#### Scenario: Пустой чанк

- **WHEN** финализируется файл без PCM
- **THEN** файл удаляется, пустой WAV не публикуется.

#### Scenario: Порог ротации

- **WHEN** число сэмплов достигло chunk_sec
- **THEN** чанк закрывается независимо от паузы в речи.

### Requirement: INV-STORE-001 Восстановление архивных part

recover_orphan_chunks SHALL обрабатывать sessions/*/audio/*.part иначе, чем legacy fast lane: чинить заголовок по размеру и переименовывать непустые файлы в WAV. Затем незарегистрированные WAV в audio/ добавляются в meta с дедупликацией; FLAC этим проходом не сканируются. Повреждённая meta не заменяется выдуманной пустой метой.

Код: [recover_orphan_chunks, recover_part, register_recovered_in_meta](../../../crates/localvox-light-core/src/chunks.rs).
Существующая проверка: `recover_orphan_patches_and_renames`, `recovered_chunk_is_registered_in_meta_after_previous_ones`.

#### Scenario: Crash до meta

- **WHEN** WAV опубликован, но meta не обновилась
- **THEN** startup пытается зарегистрировать WAV без дубля.

#### Scenario: Crash посреди чанка

- **WHEN** архивный .part содержит PCM
- **THEN** данные сохраняются восстановлением WAV, а не удалением всего файла.

### Requirement: STORE-002 FLAC с сохранением fallback

При включённом FLAC система SHALL конвертировать закрытый WAV, сохраняя исходник до проверки полного результата. Успешный exit и наличие файла сами по себе недостаточны: перед удалением WAV SHALL проверяться читаемость FLAC и соответствие числа аудиосэмплов исходнику. При отказе запуска, конвертации, проверке или превышении конечного лимита WAV SHALL сохраняться; принадлежащий конвертер SHALL управляемо завершаться.

Код: [spawn_flac_convert](../../../crates/localvox-light-core/src/chunks.rs).

#### Scenario: Нет ffmpeg

- **WHEN** конвертер не запускается
- **THEN** исходный WAV остаётся.

#### Scenario: Неполный FLAC

- **WHEN** конвертер завершился успешно, но FLAC повреждён или короче исходника
- **THEN** WAV сохраняется, преобразование не считается подтверждённым.

### Requirement: STORE-003 Retention только аудио

Система SHALL по умолчанию хранить аудио до ручного удаления (days=0).
При явно включённом retention старые WAV/FLAC непосредственно в audio/ SHALL
удаляться только после проверки подтверждённого STT, при отсутствии активной записи,
незакрытых чанков и Pending/Running задания, под исключительным владением сессией.
Повреждённая/недоступная очередь или расшифровка SHALL сохранять исходник.
Возраст определяется mtime и настроенным days; текст, summary и meta этот проход
SHALL не удалять. Существующий явно заданный срок сохраняется, новые условия
безопасности применяются и к нему. Ручное удаление не регулируется retention.

Код: [sweep_audio_retention](../../../crates/localvox-light-core/src/chunks.rs), [CLI default](../../../crates/localvox-light-core/src/cli.rs).
Проверки default/opt-in, отсутствующего/битого STT, очереди/lock/part/marker:
[evidence](../../evidence/implementation-review-2026-09-27.md), BL-020/DEC-001.

#### Scenario: Отключён retention

- **WHEN** срок не задан или days=0
- **THEN** автоматический проход не удаляет аудио.

#### Scenario: Истёк срок

- **WHEN** файл старше явно заданного срока и сессия свободна, закрыта и имеет валидное подтверждение STT
- **THEN** старый аудиофайл можно удалить, текст и прочие результаты сохраняются.

#### Scenario: Нет подтверждённого STT

- **WHEN** аудио старое, но STT не завершён, отсутствует или повреждён
- **THEN** исходник сохраняется независимо от возраста.

#### Scenario: Запись занята

- **WHEN** есть live marker, part, Pending/Running задание или execution lock занят
- **THEN** автоматическая очистка этой записи ничего не удаляет.

#### Scenario: Неизвестно состояние очереди

- **WHEN** jobs.json нельзя прочитать или разобрать
- **THEN** исходники сохраняются, отказ доступен в диагностике.

### Requirement: INV-STORE-002 Один движок на каталог

run_engine SHALL держать exclusive workspace lock до выхода и выполнять recovery после взятия lock. Очистка пустых каталогов сохраняет каталоги с ценным содержимым; чужой второй движок не должен начинать recovery этого каталога.

Код: [try_instance_lock, sweep_empty_session_dirs](../../../crates/localvox-light-core/src/session.rs), [run_engine](../../../crates/localvox-light-core/src/engine.rs).

#### Scenario: Второй процесс

- **WHEN** каталог занят первым движком
- **THEN** второй не получает владение каталогом.

## Implementation status

STORE-002 не соответствует контракту проверки FLAC и управляемого timeout (RV-005). STORE-003 исправлен и проверен; установленная release-сборка не обновлялась.

[Матрица по требованиям](../../evidence/coverage.md#audio-storage) ·
[Расхождения RV](../../review.md) ·
[Выполненные проверки](../../evidence/implementation-review-2026-09-27.md).
Ниже сохранены дополнительные наблюдения о реализации, не исключения из требований.

## Observed limitations

FLAC запускается прямым subprocess и не наследует гарантии нового phase-supervisor автоматически (RV-005). Retention исправлен по DEC-001 и проверен на временных каталогах; работающая release-сборка этим не обновлена. Ошибки записи ChunkRecorder логируются; обещания «звук не теряется при любом отказе диска» в baseline нет.
