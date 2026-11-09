# Подтверждённое первичное аудио, 2026-11-09

Этапы 1.1–1.2. Контракт STORE-004 находится в delta этого change; ранний ASR,
partial API/UI/TUI и измерение выигрыша ещё не реализованы. Main specs не синхронизированы.
Ниже — выполненные проверки исходников на Windows, без рабочего демона, микрофона,
приватного архива и моделей. Установленная release-сборка этим не обновлена.

## Реализация и локальная карта

| Требование / сценарии | Вход в код | Проверка |
|---|---|---|
| STORE-004: опубликован файл, ошибка закрытия/публикации, дыра | [capture.rs](../../../../crates/localvox-light-core/src/chunks/capture.rs), [recorder](../../../../crates/localvox-light-core/src/chunks.rs) | closed_ranges; refused_open_close_pause_and_terminal_commit; seek_header_or_sync_failure; write_or_periodic_sync_failure; missing_or_unaccounted_audio; fresh_full_input_refuses_a_lost_sealed_chunk_or_unrecorded_suffix |
| STORE-004: Stop и pause | [pipeline](../../../../crates/localvox-light-core/src/pipeline.rs) | capture_manifest_covers_preroll_pause_both_sources_and_only_true_stop; pause_preserves_generation_and_sample_clock_without_eof; existing control_commands_are_serviced_without_new_pcm_and_stop_finalizes_audio |
| STORE-004: аварийный хвост | [capture tests](../../../../crates/localvox-light-core/src/chunks/capture_tests.rs) | crash_after_audio_rename_before_seal; failed_rename; recovery_never_shortens_a_known_lost_range; unpublished_empty_close; crash_after_empty_seal; malformed_missing_or_foreign_ledger; recovery_does_not_touch_an_owned_capture |
| STORE-004: FLAC одновременно с чтением; принятый STORE-002/RV-005 | [codec](../../../../crates/localvox-light-core/src/chunks/codec.rs), [tests](../../../../crates/localvox-light-core/src/chunks/codec_tests.rs) | shared/exclusive lease; zero-padding alias; fake-success/header-only; timeout/cancel/shutdown; no-tool fallback; два настоящих FFmpeg tests |
| STORE-001, INV-STORE-001, INV-REC-001 | Те же recorder/capture/pipeline | Обычные legacy WAV/recovery/pause regressions плюс новая fault/restart серия |
| ARCH-003, ARCH-005, INV-SESSION-001 | [media](../../../../crates/localvox-light-api/src/archive/media.rs), [mutations](../../../../crates/localvox-light-api/src/archive/mutations.rs), [control](../../../../crates/localvox-light-core/src/jobs/control.rs) | unreadable_or_short_declared_audio_is_not_fabricated_as_silence; deletion_refuses_a_remaining_capture_writer_after_marker_disappears; capture_owner_is_part_of_mutation_ownership_even_without_a_processing_worker; real API→mpv null audio |

Полные имена тестов находятся в связанных файлах. Эта карта описывает проверку
подтверждённого входа, а не доступность всей будущей функции.

## Артефакты и порядок публикации

`meta.json.capture_generation` объявляет UUID до начала записи. Единственный writer
держит `.capture.lock`; атомарный `capture/state.json` version=1 содержит generation,
revision, sample_rate=16000, sources 0/1, paused и terminal. У каждого источника:
непрерывные sequence/start_sample/samples/outcome и отдельно open range.
`unknown` хранит известную длину и причину; он не превращается в тишину.

Непустой файл: write → patch → sync → rename WAV → seal. Пустой: seal empty →
удаление `.part`, чтобы отказ manifest не удалил единственное доказательство нуля.
Отказ атомарной публикации сохраняет предыдущий дисковый snapshot; валидное обновление
остаётся у владельца в RAM для следующей попытки. Stop сначала закрывает обе дорожки,
затем публикует terminal с точными ends. Отказ виден отдельным capture error в журнале.
Discovery учитывает revision manifest; full cook отказывает без terminal и перед новым
ASR проверяет наличие/длину всех sealed inputs и отсутствие неучтённых файлов.
Уже подтверждённый текст может переиспользоваться после разрешённого retention аудио.

Recovery выполняется после workspace ownership и только при свободном capture lease.
Проверяет реальные сохранённые сэмплы, регистрирует meta и лишь затем interrupted EOF.
Известный потерянный диапазон не уменьшается. Odd-byte/truncated PCM сохраняется с
отказом, а не исправляется отбрасыванием сэмпла. Legacy/import без capture declaration
сохраняет прежний путь; отсутствие/повреждение нового manifest не даёт legacy fallback.

## FLAC / RV-005

Один chunk lease защищает WAV и FLAC: shared reader, exclusive converter. Читатель
выбирает представление после захвата; полная конвертация не удаляет файл у читателя.
Конвертер под process supervisor, deadline 120 s, ограниченная диагностика; не больше
одного активного и одного ожидающего задания. Capture lease живёт до выхода всех
принадлежащих конвертеров. Shutdown отменяет и join-ит их; перегрузка сохраняет WAV.

До удаления WAV: strict decode FLAC, точный count и SHA-256 PCM, sync FLAC,
атомарная квитанция `audio-codecs/<logical-key>.json` (generation/count/PCM SHA/container
SHA), затем повторная проверка WAV. Exit=0/наличие файла недостаточны. STREAMINFO
читается по [RFC 9639 §8.2](https://www.rfc-editor.org/rfc/rfc9639.html#section-8.2),
но его count проверяется декодированием. PCM потоками 64 KiB, temporary decoded PCM
на диске аудио, bounded по count/времени. Независимой жёсткой дисковой квоты нет:
ffmpeg `-fs` — ограничитель кодека, supervisor — срок/наблюдаемые лимиты.

RV-005 исправлен в проверенном пути: отсутствующий ffmpeg; успешный пустой/header-only
вывод; короткий FLAC; тот же count с другими PCM; timeout/cancel; shutdown и read race.
Исторические FLAC не получают выдуманной квитанции. Новая capture recovery принимает
FLAC только с соответствующей generation/container квитанцией. Power-loss, физический
диск и длительные microphone/loopback/Mac прогоны не проверялись. Гарантии supervisor
по ОС остаются прежними; Unix code path здесь не исполнялся.

## Команды и результаты

Из корня, TEMP/TMP=`D:\rust_repo\localvox-light\target\tmp`, Rust `--locked --offline -j 2`:

```powershell
cargo test --locked --offline -j 2 -p localvox-light-core --features onnx --lib --quiet
cargo test --locked --offline -j 2 -p localvox-light-core --features onnx --lib chunks::codec::tests::real_ -- --ignored --nocapture
cargo test --locked --offline -j 2 -p localvox-light-api --lib --quiet
cargo test --locked --offline -j 2 -p localvox-light-api media_http_range_plays --lib -- --ignored --nocapture
cargo test --locked --offline -j 2 -p localvox-light-api flac_range_is_sample_accurate --lib -- --ignored --nocapture
cargo check --locked --offline -j 2 -p localvox-light -p localvox-light-asr -p localvox-light-terminal
```

Core: 417 passed, 6 ignored. API: 78 passed,
6 ignored; отдельно HTTP→mpv 1 passed и FLAC sample range 1 passed. Настоящий
FFmpeg core: 2 passed, включая побайтовое PCM, диапазоны и ожидание reader lease.
Основное приложение/worker/TUI: cargo check passed. Игнорируемые native/model/device
тесты обычной library серии не объявляются пройденными. Новые capture/codec и real
FFmpeg проверки включены в Windows CI; это настройка CI, не свидетельство запуска CI.

Первый core regression выявил четыре legacy-no-meta discovery сбоя; восстановлен
прежний legacy fallback. Первый настоящий FFmpeg прогон выявил Windows AccessDenied
при sync read-only FLAC; файл открывается с write access перед sync. Оба исправления
проверены повторным запуском соответствующих серий. Дополнительные fault cases
закрепили порядок empty seal→cleanup при capture и recovery.

Документационные strict main: 15 passed / 0 failed; change valid;
check-openspec: 88 requirements, 188 scenarios, 88 coverage rows, 1611 links PASS.
Проверка diff выполняется перед коммитом.
Аппаратная приёмка 7.4 остаётся not-run; early ASR/performance/UI tasks остаются открытыми.
