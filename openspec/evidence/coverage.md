# Покрытие требований и состояние реализации

Дополнение SETUP-001/002/003, CFG-001/003 и SETUP-006/009 (delta), 2026-11-14:
[реальная интеграция мастера](../changes/add-interactive-install-wizard/evidence/integration.md).
V2 одобрено владельцем; общий frontend подключён к planner/executor, появились staged
bootstrap для пяти platform targets, сохранение незавершённого выбора и проверка
реальным dotenv parser. Исторические результаты ниже не являются статусом этого этапа.
Нативные Linux/Mac, настоящая установка пакетов/весов и hosted release отмечаются отдельно.

Дополнение SETUP-009 (delta), 2026-11-14 — отзыв владельца:
[навигация назад](../changes/add-interactive-install-wizard/evidence/back-navigation.md).
Оформление одобрено владельцем, Back добавлен после первого просмотра. 10 tests /
13 ConPTY sessions passed: сохранённые ответы, исправленные пути, новый маршрут
после отключения summary, отмена/курсор/неизменность файлов. Новая навигация владельцем
и native Mac/Linux not-run; production установка не подключена.

Дополнение SETUP-009 (delta), 2026-11-14:
[безопасное демо установщика](../changes/add-interactive-install-wizard/evidence/preview.md).
6 integration tests / 9 native ConPTY сессий прошли: default/custom, checkbox,
модель/папки, отмена, неизменность файлов и курсор. 5 PNG/GIF сняты с настоящего
frontend; просмотр владельца, production bootstrap/install и native Linux/Mac
not-run. Принятый main contract и существующие установщики не изменены.

Дополнение SETUP-001/002/003, INV-SETUP-001 от 2026-11-12:
[полные platform packages, installers и промо](../changes/prepare-promo-and-platform-releases/evidence/implementation.md).
12 delivery tests прошли, включая настоящую Windows установку/обновление,
native loader smoke, отказы до копирования и Mac shell fixtures. UI build/14 unit/
81 browser прошли; настоящие UI/TUI сняты на вымышленных данных.
Новый hosted CI, native Mac/Linux и аппаратный Mac daemon/capture not-run;
SETUP-004 остаётся delta. Старые результаты ниже не заменяются новой приёмкой.

Дополнение PIPE-002/008/010/011/012, CFG-001/002, ARCH-001/002 от 2026-11-11:
[grouped durable cache, default on и проверка переключателя](../changes/add-incremental-recording-transcription/evidence/default-on-validation.md).
Новые полные R3: legacy 3/3 (+6.25% суммарно), current capture 3/3 (+4.03%);
в каждой паре ≤20%, окна/текст/714 ASR calls совпали. Ранее failed gates сохранены
как исторические результаты ниже. Default on теперь реализован; PIPE-016/018 и
ARCH-006 остаются delta, аппаратная microphone/loopback/Mac приёмка 7.4 not-run.
915 Rust, 6 actual worker fixtures, real coordinator/shared UI/TUI HTTP и UI 14/81
прошли; подробности, границы и команды в новом отчёте.

Дополнение ARCH-001/002, PIPE-002/008/010/011/012, CFG-001/002 от 2026-11-11:
[inference/execution/import границы](../changes/refactor-modular-feature-boundaries/evidence/processing.md),
[итоговый рефакторинг и комплект](../changes/refactor-modular-feature-boundaries/evidence/validation.md).
908 Rust, 8 explicit worker, real UI/TUI shared HTTP, UI 14/80 и native ASR smoke
прошли; модели/форматы/defaults/visual baselines сохранены. Короткий native smoke
не заменяет прежние performance gates; аппаратная/Mac приёмка и BL-006 открыты.

Дополнение ARCH-001/002, PIPE-011/012, SEARCH-001/002 и CFG-001/002 от 2026-11-11:
[UI/API feature operations](../changes/refactor-modular-feature-boundaries/evidence/ui-api.md).
React document controller/views и Archive operations разделены при прежних DTO/DOM;
UI build/14 unit/80 browser и API 81 unit/integrations прошли, screenshots сохранены.
Это срез рефакторинга, не новая аппаратная/модельная приёмка.

Дополнение ARCH-001/002, CFG-001/002 от 2026-11-11:
[модульный TUI](../changes/refactor-modular-feature-boundaries/evidence/tui.md),
[карта всех US-сценариев и границ](../../docs/modularity.md).
Preferences/reconnect, responses/input и renderer разделены; 198 lib / 5 ConPTY /
IO/run-script прошли, прежние snapshots сохранены. Остальные этапы рефакторинга
открыты; аппаратная приёмка не подменяется.

Дополнение ARCH-001 от 2026-11-11:
[регулировка колонки списков TUI](../changes/resize-terminal-list-pane/evidence/implementation.md).
196 library / 5 ConPTY прошли, включая Control arrows и ширину после reconnect;
прежние visual baselines сохранены. ARCH-019 остаётся delta; внешние bindings/
ручное отображение у владельца not-run.

Дополнение ARCH-001/015 от 2026-11-11: [активная область и детали TUI](../changes/refine-terminal-focus-details/evidence/implementation.md).
190 lib / 5 ConPTY прошли; 38 style baselines обновлены после подтверждения
побайтного равенства area/content, focus/contrast проверены во всех темах.
ARCH-018 остаётся delta; эстетическая оценка владельца/hardware/Mac not-run.

Дополнение PIPE-002/008/010/011/012, CFG-001/002 от 2026-11-11:
[resident ASR и проверки кеша](../changes/reuse-asr-worker-between-horizons/evidence/implementation.md),
[native сравнения](../changes/reuse-asr-worker-between-horizons/evidence/native-reference.md).
Один engine между порциями, bounded IPC/отмена/idle, framed durable results,
crash/recovery и real coordinator проверены. Прежний legacy reference gate остаётся
непройденным: +18.51% суммарно, две пары выше 20%; ранний default остаётся off.
Дополнительный контроль с одинаковым current capture прошёл все пары, +2.95%
суммарно; он не заменяет исходный gate для принятия default.
PIPE-019 остаётся delta, main specs не синхронизированы. Аппаратные сценарии not-run.

Дополнение чтения ARCH-001 и ручного журнала PIPE-011 от 2026-11-10:
[Page Up/Down по видимой странице TUI](../changes/fix-terminal-page-navigation/evidence/implementation.md).
145 library tests/4 ConPTY прошли, прежние visual baselines сохранены; ARCH-013
пока delta. UI/сервер/модели и аппаратные gates не менялись.

Дополнение ARCH-001, PIPE-011 и доступности TUI от 2026-11-10:
[цветные состояния списка, фильтр и RU/EN команды](../changes/add-terminal-session-activity/evidence/implementation.md).
140 library tests, 4 настоящих ConPTY, style/geometry snapshots и release-сборка
прошли. Новые ARCH-010–012 остаются локальной delta, main specs не синхронизированы.
Установленный daemon/личный архив и предыдущий incremental budget gate не затронуты;
аппаратный звук/другие раскладки этим срезом не проверены.

Продолжение PIPE-002/004/008/010/011/012, VER-001/004, STORE-001/002,
ARCH-001–005, CFG-001/002 от 2026-11-10:
[R4 кандидат/регрессии и локальная карта delta](../changes/add-incremental-recording-transcription/evidence/review-candidate.md).
Сохранённая fault/restart серия и три native пары выполнены: окна/текст/ASR calls
совпали, **reference wall budget не пройден (+48.12% > 20%)**. Preset остаётся off,
PIPE-018 default on не реализован, 7.3/7.4 открыты; аппаратные сценарии not-run.
Это дополнительное свидетельство принятых требований, не принятие новых IDs и
не sync/archive change. Кандидат передаётся на просмотр UI/TUI с явным включением.

Продолжение ARCH-001–005, PIPE-011/012, VER-001, CFG-001/002 от 2026-11-10:
[R3 partial API/UI/TUI](../changes/add-incremental-recording-transcription/evidence/partial-clients.md).
UI build/unit/77 browser tests, TUI/client/ConPTY, real HTTP seam/error, real FLAC и
mpv/null audio прошли; core с CI-параллелизмом 448 passed/8 ignored. Native reference
и аппаратная приёмка остаются отдельно, новые ARCH-006/PIPE-015–018 ещё delta.

Продолжение PIPE-002/008/010/012, CFG-001/002, ARCH-005 от 2026-11-10:
[R2 finite queue/control/protocol](../changes/add-incremental-recording-transcription/evidence/prefix-coordinator.md).
Real-coordinator fixture stop/disable/cancel прошёл, ранние snapshots не становятся
финальными версиями. Default gate/клиенты ещё не приняты, новые IDs остаются delta.

Продолжение PIPE-004/012, VER-001/004, STORE-001/002 от 2026-11-10:
[R1 finite worker и восстановление финализации](../changes/add-incremental-recording-transcription/evidence/prefix-worker.md).
Core 444 passed/8 ignored; 3 настоящих child-worker fixture tests и native 150 s smoke
прошли. Полная native diarization, coordinator/клиенты и двухчасовой gate этим срезом
не проверены; новые PIPE-015/016 пока остаются delta, не принятыми main requirements.

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

Продолжение PIPE-002/008/009/010/011 от 2026-11-01:
[общий координатор и изолированный headless стенд](../changes/extract-headless-processing-coordinator/evidence/implementation.md).
Прежние coordinator tests выполняются в library, launcher — в binary; дополнительно
проверены bounded observer принятых переходов и настоящий model-free queue lifecycle.
Форматы и политика продукта сохранены; этот срез сам не измеряет throughput.
Аппаратная приёмка остаётся открытой.

Продолжение BL-003 для PIPE-004/007/009/011 от 2026-11-01:
[полная release-очередь на 1 ч 59 мин публичной речи](../changes/benchmark-processing-batch/evidence/queue-benchmark.md).
Две fresh sessions: STT-only batch 149.803 s; с настоящей summary — 369.350 s,
все receipts/исходы проверены. Отдельно measured ASR/NER/Ollama load, generation,
grounding и внешние ресурсы с documented scope/gap первого monitor. Reference-бюджет
и следующий proposal сохранены; ready не означает принятие содержания/качества.

## ad-hoc-asks

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [ASK-001](../specs/ad-hoc-asks/spec.md#requirement-ask-001-приём-материала) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs), [src/asks.rs](../../crates/localvox-light-core/src/asks.rs), [src/Ask.tsx](../../ui/src/Ask.tsx) | test-run (частично): `test:effective_prompt_falls_back_to_default`, `test:save_and_load_roundtrip_and_files_split_by_mutability`; runtime-mock ([журнал](implementation-review-2026-09-27.md)) | GAP RV-009: omitted provider = Claude; unknown provider не отклоняется. |
| [ASK-002](../specs/ad-hoc-asks/spec.md#requirement-ask-002-отдельная-очередь) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs), [src/asks.rs](../../crates/localvox-light-core/src/asks.rs) | test-run (частично): `test:a_failed_ask_is_saved_and_marked`, `test:pending_is_fifo_and_reclaim_revives_running` | RV-009: прямые записи файлов, crash/неопределённый облачный повтор не проверены. |
| [ASK-003](../specs/ad-hoc-asks/spec.md#requirement-ask-003-история-и-удаление) | [src/asks.rs](../../crates/localvox-light-core/src/asks.rs), [src/Ask.tsx](../../ui/src/Ask.tsx) | test-run (частично): `test:delete_removes_the_folder_and_is_idempotent` | RV-009: удаление Running не координируется с worker. |

## api-access

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [API-001](../specs/api-access/spec.md#requirement-api-001-обычная-авторизация-http) | [src/http.rs](../../crates/localvox-light-api/src/http.rs) | code-reviewed; runtime-mock ([журнал](implementation-review-2026-09-27.md)); test-run: journal HTTP 401/Host-Origin 403 ([journal/TUI 2026-10-31](../changes/add-technical-journal-workspace/evidence/implementation.md)) | Проверен изолированный loopback HTTP; LAN не проверялась. |
| [API-002](../specs/api-access/spec.md#requirement-api-002-исключение-для-media-url) | [src/http.rs](../../crates/localvox-light-api/src/http.rs), [src/Player.tsx](../../ui/src/Player.tsx) | code-reviewed; runtime-mock ([журнал](implementation-review-2026-09-27.md)) | GAP RV-003: media пропускает Host/Origin; в mock дошли до 404 отсутствующей сессии. |
| [API-003](../specs/api-access/spec.md#requirement-api-003-ограничение-http-работы) | [src/http.rs](../../crates/localvox-light-api/src/http.rs) | code-reviewed; test-run (частично): bounded journal read/response ([journal/TUI 2026-10-31](../changes/add-technical-journal-workspace/evidence/implementation.md)) | Отдельный runtime всех сценариев не выполнен. |
| [API-004](../specs/api-access/spec.md#requirement-api-004-mcp-сервер-файлового-архива) | [src/lib.rs](../../crates/localvox-light-api/src/lib.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |
| [API-005](../specs/api-access/spec.md#requirement-api-005-один-web-ui-для-desktop-и-lan) | [src/http.rs](../../crates/localvox-light-api/src/http.rs), [src/api.ts](../../ui/src/api.ts), [src/lib.rs](../../src-tauri/src/lib.rs) | code-reviewed | Сборка UI/desktop пройдена, browser/mobile/offline приёмка не выполнялась. |

## archive-playback

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [ARCH-001](../specs/archive-playback/spec.md#requirement-arch-001-адресация-и-чтение) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | test-run (частично): `test:session_dir_rejects_everything_but_plain_names`, `test:verified_transcript_remains_readable_when_summary_fails`  Дополнение: [TUI UX 2026-11-01](../changes/add-technical-journal-workspace/evidence/usability-2026-11-01.md). | Отдельный runtime всех сценариев не выполнен. |
| [ARCH-002](../specs/archive-playback/spec.md#requirement-arch-002-один-плеер-и-граница-реплики) | [src/Player.tsx](../../ui/src/Player.tsx) | test-run (частично): [browser 2026-11-01](../changes/fix-browser-player-lifecycle/evidence/implementation.md), один native audio, pause/keyboard seek, повтор фрагмента и until при обновлении метаданных  Дополнение: [TUI UX 2026-11-01](../changes/add-technical-journal-workspace/evidence/usability-2026-11-01.md). | Синтетический PCM/HTTP Range; реальный длинный FLAC и аппаратный звук этим не проверены. |
| [ARCH-003](../specs/archive-playback/spec.md#requirement-arch-003-аудио-через-чанки) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs), [src/http.rs](../../crates/localvox-light-api/src/http.rs), [media.rs](../../crates/localvox-light-api/src/archive/media.rs) | test-run (частично): `test:audio_clip_extracts_sample_accurate_range`, `test:audio_clip_errors_when_overlapping_chunk_unreadable`, `test:mixing_two_loud_tracks_does_not_wrap_into_noise`; test-run: потоковый WAV, byte Range, real API→mpv, точные FLAC-сэмплы ([journal/TUI 2026-10-31](../changes/add-technical-journal-workspace/evidence/implementation.md)); test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md) | Synthetic WAV/FLAC, null audio; аппаратный звук и LAN не проверялись. |
| [ARCH-004](../specs/archive-playback/spec.md#requirement-arch-004-экспорт-best) | [src/export.rs](../../crates/localvox-light-core/src/export.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |
| [ARCH-005](../specs/archive-playback/spec.md#requirement-arch-005-явное-удаление) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | test-run (частично): `test:delete_needs_the_session_name_and_is_final`; test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md); test-run 2026-11-10: [capture-owned planner/Stop/delete](../changes/add-incremental-recording-transcription/evidence/capture-planner.md) | BL-002: controlled stop/delete проверены на native worker и временном архиве ([результаты](../changes/archive/2026-11-01-serialize-session-mutations/evidence/implementation.md)); частичный remove_dir_all не откатывается. |

Дополнение 2026-11-01: [рефакторинг диспетчера TUI](../changes/refactor-terminal-command-dispatch/evidence/implementation.md).
Пройдены прежние snapshots, палитра/хоткеи, подтверждение удаления/повторов с привязкой
к исходной сессии, копирование и ConPTY F8/F9/F10. Настоящий общий API/UI/TUI-сценарий
также выполнен. Серверные gaps и аппаратные границы ARCH-001–005 этим не закрываются.

## audio-storage

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [STORE-001](../specs/audio-storage/spec.md#requirement-store-001-публикация-чанка) | [src/chunks.rs](../../crates/localvox-light-core/src/chunks.rs) | test-run (частично): `test:streaming_wav_writes_valid_header`, `test:empty_wav_is_removed_on_finalize`; test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md) | Отдельный runtime всех сценариев не выполнен. |
| [INV-STORE-001](../specs/audio-storage/spec.md#requirement-inv-store-001-восстановление-архивных-part) | [src/chunks.rs](../../crates/localvox-light-core/src/chunks.rs) | test-run (частично): `test:recover_orphan_patches_and_renames`, `test:recovered_chunk_is_registered_in_meta_after_previous_ones`; test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md) | Отдельный runtime всех сценариев не выполнен. |
| [STORE-002](../specs/audio-storage/spec.md#requirement-store-002-flac-с-сохранением-fallback) | [codec.rs](../../crates/localvox-light-core/src/chunks/codec.rs), [chunks.rs](../../crates/localvox-light-core/src/chunks.rs) | test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md) — count/PCM SHA, fake success, real FFmpeg, timeout/cancel/shutdown/read lease; test-run 2026-11-10: [capture-owned planner/Stop/delete](../changes/add-incremental-recording-transcription/evidence/capture-planner.md) | RV-005 исправлен в проверенном пути; hardware/power-loss/Unix not-run, установленный release не обновлён. |
| [STORE-003](../specs/audio-storage/spec.md#requirement-store-003-retention-только-аудио) | [src/chunks.rs](../../crates/localvox-light-core/src/chunks.rs), [src/cli.rs](../../crates/localvox-light-core/src/cli.rs) | code-reviewed; test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md) | BL-020 реализован в исходниках, работающий release не обновлён. |
| [INV-STORE-002](../specs/audio-storage/spec.md#requirement-inv-store-002-один-движок-на-каталог) | [src/session.rs](../../crates/localvox-light-core/src/session.rs), [src/engine.rs](../../crates/localvox-light-core/src/engine.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |

## installation-models

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [SETUP-001](../specs/installation-models/spec.md#requirement-setup-001-манифест-загрузки) | [scripts/models.json](../../scripts/models.json), [scripts/fetch-models.ps1](../../scripts/fetch-models.ps1), [scripts/fetch-models.sh](../../scripts/fetch-models.sh) | code-reviewed | Check и сборка пройдены; новые загрузки не выполнялись. |
| [INV-SETUP-001](../specs/installation-models/spec.md#requirement-inv-setup-001-скачивание-через-временный-файл) | [scripts/fetch-models.ps1](../../scripts/fetch-models.ps1), [scripts/fetch-models.sh](../../scripts/fetch-models.sh) | code-reviewed | Скачивание/hash mismatch в этом проходе не воспроизводились. |
| [SETUP-002](../specs/installation-models/spec.md#requirement-setup-002-разрешение-зависимостей) | [localvox-light-core/build.rs](../../crates/localvox-light-core/build.rs), [src/lang.rs](../../crates/localvox-light-core/src/lang.rs), [src/doctor.rs](../../crates/localvox-light-core/src/doctor.rs), [packager](../../scripts/package-release.py) | [test-run: Windows package](../changes/prepare-promo-and-platform-releases/evidence/implementation.md) | Native startup/состав/установка проверены; model path resolution в этой работе не меняется, native Mac/Linux not-run. |
| [SETUP-003](../specs/installation-models/spec.md#requirement-setup-003-doctor-и-границы-проверки) | [src/doctor.rs](../../crates/localvox-light-core/src/doctor.rs), [src/main.rs](../../crates/localvox-light/src/main.rs) | [test-run: installer invocation](../changes/prepare-promo-and-platform-releases/evidence/implementation.md) | Doctor вызван из установленной fixture; SkipModels явно отмечен, реальный набор моделей не переустанавливался и качество inference не проверялось. |

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
| [LLM-003](../specs/llm-processing/spec.md#requirement-llm-003-refine-и-cleanup-по-строкам) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs), [src/readable.rs](../../crates/localvox-light-core/src/readable.rs), [correction_guard.rs](../../crates/localvox-light-llm/src/correction_guard.rs), [russian.rs](../../crates/localvox-light-llm/src/correction_guard/russian.rs) | test-run (частично): `test:an_essay_instead_of_the_lines_costs_the_cleanup_not_the_text`, `test:lines_the_model_never_answered_for_keep_their_own_wording`, `test:one_oversized_line_goes_alone_rather_than_in_half`; test-run (native reference 2026-11-02): [правка текста отдельно от summary](../changes/profile-text-postprocessing-cost/evidence/native-reference.md); [sparse native experiment 2026-11-02](../changes/experiment-guarded-sparse-text-edits/evidence/results.md); test-run: 104 LLM unit, runtime-mock refine/cleanup настоящим worker; [retention 2026-11-02](../changes/fix-text-correction-retention/evidence/implementation.md); 12 native ordinary cleanup repeats (Gemma/Qwen, 72+24 случая ×3), 17 replay групп / 2200 решений; test-run: [длинный release cleanup 2026-11-02](../changes/verify-guarded-cleanup-on-long-transcript/evidence/results.md) — 4 native release запуска, 2816 строк, проверенные source/receipts; 154 targeted review row-visits, не полная точность; test-run: [RV-020 fix 2026-11-02](../changes/fix-unsupported-russian-word-corrections/evidence/implementation.md) — 106 LLM, 31 profiler, 4 runtime-mock; 4800 saved-proposal decisions, не новые LLM calls ; test-run: [fresh native после RV-020 2026-11-09](../changes/recheck-native-cleanup-after-word-guard/evidence/results.md) — 12 short native runs, 576/576 expected, useful 24/24 в каждом; 4 long runs / 2816 line-visits, 510 targeted review visits; 31 profiler + 5 runtime + 22 evaluator | Более сильное редакторское переформулирование не принято без примеров до/после. [Качество ordinary cleanup 2026-11-02](../changes/evaluate-local-cleanup-quality/evidence/results.md): 72 фразы × 2 модели × 3 repeats, 51 agent-review. Исторически воспроизведены RV-018/RV-019: потеря содержания и копирование соседних реплик. Исправлены в исходниках [retention 2026-11-02](../changes/fix-text-correction-retention/evidence/implementation.md); рабочий демон не обновлялся. Консервативная фильтрация отклоняет и часть полезных правок; полная смысловая/аудио приёмка не выполнена. [Отбор portable кандидатов](../changes/screen-portable-cleanup-models/evidence/results.md): 18 worker + 6 direct SAGE CPU repeats; в том историческом этапе guards не менялись; raw SAGE не является проверкой production. Воспроизведённый [RV-020](../review.md) исправлен в исходниках [RV-020 fix 2026-11-02](../changes/fix-unsupported-russian-word-corrections/evidence/implementation.md). Словарь не доказывает ошибку неизвестного слова; 15/61 ранее context_supported row-visits сохранены исходными. Общая смысловая/аудио приёмка и новая модель/default не приняты.  Fresh native: прежние RV-020 controls source; 5 audio_required visits (240/304/649) и 16 style_review остаются ограничением; общая смысловая/аудио и Mac приёмка не выполнены. |
| [LLM-004](../specs/llm-processing/spec.md#requirement-llm-004-сводка-и-ограничения-проверки) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs), [src/grounding.rs](../../crates/localvox-light-llm/src/grounding.rs), [grounding/profile.rs](../../crates/localvox-light-llm/src/grounding/profile.rs), [localvox-grounding-profile.rs](../../crates/localvox-light-bench/src/bin/localvox-grounding-profile.rs), [ner/profile.rs](../../crates/localvox-light-core/src/ner/profile.rs), [ner/windows.rs](../../crates/localvox-light-core/src/ner/windows.rs) | test-run (частично): `test:a_doubtful_result_is_still_written_as_the_real_document`, `test:a_citation_that_does_not_check_out_is_dropped_but_the_claim_stays`, `test:a_looping_summary_is_cut_at_its_first_repetition`; test-run (native reference 2026-11-02): [правка текста отдельно от summary](../changes/profile-text-postprocessing-cost/evidence/native-reference.md); test-run 2026-11-03: [компоненты grounding](../changes/profile-summary-grounding-cost/evidence/results.md) — 108 LLM unit, 4 profiler unit, production receipt check и 2 release GLiNER процесса на готовой паре; точная parity со штатной проверкой, cache/privacy/error cases; test-run 2026-11-09: [внутренние NER components](../changes/profile-ner-inference-cost/evidence/results.md) — 4 core/6 probe/108 LLM unit, native exact Entity parity (8 случаев/26 entities), 2 fresh release профиля без LLM; partial/privacy и ordinary без core observer; test-run 2026-11-09: [пропуск нерелевантных role окон](../changes/skip-irrelevant-role-windows/evidence/results.md) — 9 core/6 probe/108 LLM unit, native synthetic 8 случаев/26 entities, 2 full/relevant source пары с обратным порядком и exact 151 Entity/score/span, 2 обычных grounding профиля; 22–24% уменьшения source NER времени, прежние verdict/cache; test-run 2026-11-09: [свежий release/полный маршрут](../changes/verify-current-release-pipeline/evidence/results.md) — новая настоящая summary ready/unverified, 11 grounding operations и сохранённые request/retry/reduce clocks; приёмка содержания отдельно | Новая summary готова по целостности и остаётся unverified; смысловая точность не принята. Проверка NER не доказывает содержание. Native качество refine/cleanup после последнего guard, новая модель/default и Mac остаются отдельными проверками. |
| [LLM-005](../specs/llm-processing/spec.md#requirement-llm-005-выбор-провайдера-и-claude-cli) | [src/lib.rs](../../crates/localvox-light-llm/src/lib.rs), [src/claude_cli.rs](../../crates/localvox-light-llm/src/claude_cli.rs) | test-run (частично): `test:api_key_vars_are_always_stripped_from_the_child`, `test:non_json_is_reported`, `test:empty_result_fails`; test-run 2026-11-09: [свежий release/полный маршрут](../changes/verify-current-release-pipeline/evidence/results.md) — текущая Ollama/qwen3.5:9b через прямой loopback, digest/settings сохранены; URI telemetry proxy не использован | Выбор клиента прочитан; реальный cloud не вызывался. Отдельный API asks имеет GAP ASK-001. |
| [LLM-006](../specs/llm-processing/spec.md#requirement-llm-006-подтверждение-сомнения) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs), [src/processing.rs](../../crates/localvox-light-core/src/processing.rs) | test-run: `test:confirmation_distinguishes_no_doubt_from_invalid_artifact_and_read_failure`, `test:failed_confirmation_write_preserves_the_previous_doubt`, `test:confirmation_http_propagates_integrity_failures_instead_of_claiming_no_doubt` ([2026-10-31](../changes/add-technical-journal-workspace/evidence/implementation.md)); прежний test-run `test:confirming_a_doubt_removes_the_plaque_for_good` сохранён в историческом журнале. | RV-016 исправлен в исходниках; checksum/read/write fault проверены на temp archive, включая Windows deny-replace. Рабочий демон не обновлялся. |

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
| [PIPE-001](../specs/processing-pipeline/spec.md#requirement-pipe-001-приём-источника-и-постановка) | [src/ingest.rs](../../crates/localvox-light-core/src/ingest.rs), [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs) | test-run (частично): `test:a_link_becomes_an_empty_session_with_a_queued_job`; test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md) | Отдельный runtime всех сценариев не выполнен. |
| [PIPE-002](../specs/processing-pipeline/spec.md#requirement-pipe-002-захват-работы-и-восстановление) | [src/jobs.rs](../../crates/localvox-light-core/src/jobs.rs), [src/autocook.rs](../../crates/localvox-light/src/autocook.rs) | test-run (частично): `test:corrupted_queue_cannot_be_overwritten_or_claimed` | Отдельный runtime всех сценариев не выполнен. |
| [PIPE-003](../specs/processing-pipeline/spec.md#requirement-pipe-003-prepare--получение-полного-аудио) | [src/prepare.rs](../../crates/localvox-light-asr/src/prepare.rs), [src/artifacts.rs](../../crates/localvox-light-core/src/artifacts.rs) | test-run (частично): `test:partial_ingest_is_not_ready_until_complete_audio_is_committed` | Отдельный runtime всех сценариев не выполнен. |
| [PIPE-004](../specs/processing-pipeline/spec.md#requirement-pipe-004-transcribe--аудио-в-версию-текста) | [src/cook.rs](../../crates/localvox-light-core/src/cook.rs), [bin/localvox-process.rs](../../crates/localvox-light-asr/src/bin/localvox-process.rs) | test-run (частично): `test:nothing_to_do_is_done_and_never_loops`; test-run 2026-11-09: [свежий release/полный маршрут](../changes/verify-current-release-pipeline/evidence/results.md) — две fresh STT публичного PCM 7146.527 s: 87.711/78.935 s, 704 строки с прежним SHA, receipts приняты; test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md); test-run: [общий PCM window planner](../changes/add-incremental-recording-transcription/evidence/window-planner.md); test-run 2026-11-10: [ASR cache и неизменённый noise filter](../changes/add-incremental-recording-transcription/evidence/asr-cache.md) | Проверен подготовленный mono PCM без диаризации; capture/импорт с сайта, аппаратная приёмка и репрезентативная WER этим прогоном не подтверждены. |
| [PIPE-005](../specs/processing-pipeline/spec.md#requirement-pipe-005-textrefine--исправление-распознавания) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs), [correction_guard.rs](../../crates/localvox-light-llm/src/correction_guard.rs), [russian.rs](../../crates/localvox-light-llm/src/correction_guard/russian.rs) | code-reviewed; test-run (native reference 2026-11-02): [правка текста отдельно от summary](../changes/profile-text-postprocessing-cost/evidence/native-reference.md); test-run: 104 LLM unit, runtime-mock refine/cleanup настоящим worker; [retention 2026-11-02](../changes/fix-text-correction-retention/evidence/implementation.md); test-run: [RV-020 fix 2026-11-02](../changes/fix-unsupported-russian-word-corrections/evidence/implementation.md) — 106 LLM, 31 profiler, 4 runtime-mock; 4800 saved-proposal decisions, не новые LLM calls ; test-run: [fresh native после RV-020 2026-11-09](../changes/recheck-native-cleanup-after-word-guard/evidence/results.md) — 31 profiler, 5 runtime fixtures настоящим release worker (включая direct endpoint всех modes), 22 evaluator; native refine не оценивался | Native качество refine не проверено; более сильная переформулировка требует примеров. Проверены отказ опасной правки, полезная правка, исходные строки/тайминги/родители/receipts; общая смысловая корректность не доказана. Рабочий демон не обновлялся. В общем guard проверено новое словарное основание; native качество refine не оценено. |
| [PIPE-006](../specs/processing-pipeline/spec.md#requirement-pipe-006-textcleanup--читаемый-текст) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs), [src/readable.rs](../../crates/localvox-light-core/src/readable.rs), [correction_guard.rs](../../crates/localvox-light-llm/src/correction_guard.rs), [russian.rs](../../crates/localvox-light-llm/src/correction_guard/russian.rs) | test-run (частично): `test:cleanup_must_reference_the_correct_version_and_lines`; test-run (native reference 2026-11-02): [правка текста отдельно от summary](../changes/profile-text-postprocessing-cost/evidence/native-reference.md); [sparse native experiment 2026-11-02](../changes/experiment-guarded-sparse-text-edits/evidence/results.md); test-run: 104 LLM unit, runtime-mock refine/cleanup настоящим worker; [retention 2026-11-02](../changes/fix-text-correction-retention/evidence/implementation.md); 12 native ordinary cleanup repeats (Gemma/Qwen, 72+24 случая ×3), 17 replay групп / 2200 решений; test-run: [длинный release cleanup 2026-11-02](../changes/verify-guarded-cleanup-on-long-transcript/evidence/results.md) — 4 native release запуска, 2816 строк, проверенные source/receipts; 154 targeted review row-visits, не полная точность; test-run: [RV-020 fix 2026-11-02](../changes/fix-unsupported-russian-word-corrections/evidence/implementation.md) — 106 LLM, 31 profiler, 4 runtime-mock; 4800 saved-proposal decisions, не новые LLM calls ; test-run: [fresh native после RV-020 2026-11-09](../changes/recheck-native-cleanup-after-word-guard/evidence/results.md) — 12 short native runs, 576/576 expected, useful 24/24 в каждом; 4 long runs / 2816 line-visits, 510 targeted review visits; 31 profiler + 5 runtime + 22 evaluator | Отдельный runtime всех сценариев не выполнен. [Качество ordinary cleanup 2026-11-02](../changes/evaluate-local-cleanup-quality/evidence/results.md): 72 фразы × 2 модели × 3 repeats, 51 agent-review. Исторически воспроизведены RV-018/RV-019: потеря содержания и копирование соседних реплик. Исправлены в исходниках [retention 2026-11-02](../changes/fix-text-correction-retention/evidence/implementation.md); рабочий демон не обновлялся. Консервативная фильтрация отклоняет и часть полезных правок; полная смысловая/аудио приёмка не выполнена. [Отбор portable кандидатов](../changes/screen-portable-cleanup-models/evidence/results.md): 18 worker + 6 direct SAGE CPU repeats; в том историческом этапе guards не менялись; raw SAGE не является проверкой production. Воспроизведённый [RV-020](../review.md) исправлен в исходниках [RV-020 fix 2026-11-02](../changes/fix-unsupported-russian-word-corrections/evidence/implementation.md). Словарь не доказывает ошибку неизвестного слова; 15/61 ранее context_supported row-visits сохранены исходными. Общая смысловая/аудио приёмка и новая модель/default не приняты.  Fresh native: прежние RV-020 controls source; 5 audio_required visits (240/304/649) и 16 style_review остаются ограничением; общая смысловая/аудио и Mac приёмка не выполнены. |
| [PIPE-007](../specs/processing-pipeline/spec.md#requirement-pipe-007-summary--сводка-и-её-проверка) | [src/pipeline.rs](../../crates/localvox-light-llm/src/pipeline.rs), [src/grounding.rs](../../crates/localvox-light-llm/src/grounding.rs), [localvox-grounding-profile.rs](../../crates/localvox-light-bench/src/bin/localvox-grounding-profile.rs), [ner.rs](../../crates/localvox-light-core/src/ner.rs), [ner/profile.rs](../../crates/localvox-light-core/src/ner/profile.rs), [ner/windows.rs](../../crates/localvox-light-core/src/ner/windows.rs) | code-reviewed; test-run (native reference 2026-11-02): [правка текста отдельно от summary](../changes/profile-text-postprocessing-cost/evidence/native-reference.md); test-run 2026-11-03: [профиль проверки готовой сводки](../changes/profile-summary-grounding-cost/evidence/results.md) — 2 fresh release процесса с настоящим GLiNER без LLM; source NER 54.632/61.785 s, load 1.063/1.055 s, повторный source call отсутствует, exact verdict parity; 108 LLM + 4 profiler unit и 1 production receipt check; test-run 2026-11-09: [NER components](../changes/profile-ner-inference-cost/evidence/results.md) — 2 fresh release процесса, source 49.230/50.379 s, session.run 48.963/50.110 s, 221 name + 221 role окна; exact Entity parity на 8 synthetic случаях, 4 core/6 probe/108 LLM unit; test-run 2026-11-09: [пропуск нерелевантных role окон](../changes/skip-irrelevant-role-windows/evidence/results.md) — 9 core/6 probe/108 LLM unit, native synthetic 8 случаев/26 entities, 2 full/relevant source пары с обратным порядком и exact 151 Entity/score/span, 2 обычных grounding профиля; 22–24% уменьшения source NER времени, прежние verdict/cache; test-run 2026-11-09: [свежий release/полный маршрут](../changes/verify-current-release-pipeline/evidence/results.md) — STT-only 93.221 s, STT+summary 208.275 s; summary phase 122.652 s, optional off=missing/on=ready+unverified, text не запрошен | Полный маршрут готового аудио проверен, summary ready/unverified; это не приёмка качества и не новый общий SLA. Рабочий daemon не запускался. HTTP native counters/полные внешние ресурсы и Mac не измерены; старые 242.248 s не общий benchmark. |
| [PIPE-008](../specs/processing-pipeline/spec.md#requirement-pipe-008-принятие-результата-и-переход) | [src/jobs.rs](../../crates/localvox-light-core/src/jobs.rs), [src/artifacts.rs](../../crates/localvox-light-core/src/artifacts.rs) | test-run (частично): `test:process_exit_cannot_finish_a_phase_without_verified_artifacts`, `test:failed_queue_commit_keeps_the_current_phase`, `test:hashes_detect_same_length_output_and_input_changes`; test-run 2026-11-09: [свежий release/полный маршрут](../changes/verify-current-release-pipeline/evidence/results.md) — настоящие coordinator/worker, успешная проверка receipts перед всеми переходами и Done, оба архива complete/all_success | Проверены реальные успешные model routes и model-free отказы. Автоматический ремонт старых повреждённых Done остаётся BL-004; общая многофайловая crash-транзакция не заявлена. |
| [PIPE-009](../specs/processing-pipeline/spec.md#requirement-pipe-009-параллелизм-и-владение-ресурсами) | [src/autocook.rs](../../crates/localvox-light/src/autocook.rs), [src/jobs.rs](../../crates/localvox-light-core/src/jobs.rs) | test-run (native release reference): [BL-003 queue benchmark](../changes/benchmark-processing-batch/evidence/queue-benchmark.md), off/on summary, preserved transition clocks and observed resources; test-run 2026-11-09: [свежий release/полный маршрут](../changes/verify-current-release-pipeline/evidence/results.md) — настоящие production lanes и ownership на fresh сессиях, owned tree sampled RAM/сферы ресурсов записаны; последовательный single-session маршрут | Один публичный длинный fixture/repeats; условный reference-бюджет, не SLA. BL-008 и аппаратная приёмка открыты; качество не принято. |
| [PIPE-010](../specs/processing-pipeline/spec.md#requirement-pipe-010-ошибки-таймауты-отмена-и-повтор) | [src/lib.rs](../../crates/localvox-light-process/src/lib.rs), [src/jobs.rs](../../crates/localvox-light-core/src/jobs.rs) | test-run (частично): `test:cancellation_stops_the_entire_tree`, `test:timeout_stops_the_entire_tree`; test-run: lifecycle/supervised media decode ([journal/TUI 2026-10-31](../changes/add-technical-journal-workspace/evidence/implementation.md)); test-run 2026-11-09: [свежий release/полный маршрут](../changes/verify-current-release-pipeline/evidence/results.md) — явный release queue runtime fixture: empty/timeout/failed/cancelled/incompatible; 704 Rust regression включают coordinator/client/process; test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md) | BL-002: пользовательская отмена без рестарта проверена ([результаты](../changes/archive/2026-11-01-serialize-session-mutations/evidence/implementation.md)); BL-004: автоматический ремонт Done остаётся открытым. |
| [PIPE-011](../specs/processing-pipeline/spec.md#requirement-pipe-011-честные-время-и-наблюдаемость) | [src/workflow.rs](../../crates/localvox-light-core/src/workflow.rs), [src/progress.rs](../../crates/localvox-light-core/src/progress.rs), [src/Progress.tsx](../../ui/src/Progress.tsx) | test-run (частично): `test:heartbeats_preserve_start_and_retry_drops_old_progress`; runtime-mock ([журнал](implementation-review-2026-09-27.md)); test-run: journal cursor/quota/concurrency, heartbeat UI/TUI ([journal/TUI 2026-10-31](../changes/add-technical-journal-workspace/evidence/implementation.md))  Дополнение: [TUI UX 2026-11-01](../changes/add-technical-journal-workspace/evidence/usability-2026-11-01.md).; test-run (native reference 2026-11-02): [правка текста отдельно от summary](../changes/profile-text-postprocessing-cost/evidence/native-reference.md); test-run 2026-11-09: [свежий release/полный маршрут](../changes/verify-current-release-pipeline/evidence/results.md) — claimed/accepted clocks и operation journal, events_lost=0, malformed=0, incomplete_tail=false; native HTTP counters оставлены null | Исторические missing boundaries остаются неизвестными; не выводить cleanup=0. Mock не задаёт SLA. |
| [PIPE-012](../specs/processing-pipeline/spec.md#requirement-pipe-012-повторная-обработка-и-сохранение-первичных-данных) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | test-run (частично): `test:recook_does_not_invalidate_artifacts_owned_by_a_live_worker` | BL-002: controlled cancel/delete и отказ живому CLI проверены ([результаты](../changes/archive/2026-11-01-serialize-session-mutations/evidence/implementation.md)); общей многофайловой crash-транзакции нет. |
| [PIPE-013](../specs/processing-pipeline/spec.md#requirement-pipe-013-индексы-чтение-и-срок-хранения) | [src/autocook.rs](../../crates/localvox-light/src/autocook.rs), [src/chunks.rs](../../crates/localvox-light-core/src/chunks.rs) | test-run (native synthetic): [BL-005](../changes/verify-background-indexing/evidence/implementation.md), idle/cancel/timeout, исходные тексты сохранены. | Loopback embeddings, без реальных моделей/устройств; retention проверяется отдельно. |
| [PIPE-014](../specs/processing-pipeline/spec.md#requirement-pipe-014-другие-входы-и-совместимость) | [bin/localvox-process.rs](../../crates/localvox-light-asr/src/bin/localvox-process.rs) | code-reviewed | BL-006: прямой CLI import пока отдельный путь; гарантию фонового Prepare не переносить. |
| [INV-INGEST-001](../specs/processing-pipeline/spec.md#requirement-inv-ingest-001-durable-ingest-acknowledgement) | [src/ingest.rs](../../crates/localvox-light-core/src/ingest.rs), [src/jobs.rs](../../crates/localvox-light-core/src/jobs.rs) | code-reviewed | Отдельный runtime всех сценариев не выполнен. |

| [INV-SESSION-001](../specs/processing-pipeline/spec.md#requirement-inv-session-001-exclusive-session-mutation) | [control.rs](../../crates/localvox-light-core/src/jobs/control.rs), [cancellation.rs](../../crates/localvox-light-core/src/jobs/cancellation.rs), [mutations.rs](../../crates/localvox-light-api/src/archive/mutations.rs), [autocook.rs](../../crates/localvox-light/src/autocook.rs) | test-run: все 7 сценариев delta, native worker/CLI, настоящие HTTP, deny-replace/delete, восстановление; [результаты BL-002](../changes/archive/2026-11-01-serialize-session-mutations/evidence/implementation.md); test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md); test-run 2026-11-10: [capture-owned planner/Stop/delete](../changes/add-incremental-recording-transcription/evidence/capture-planner.md) | Windows, временные архивы; один daemon. Нет обещания отменить внешний LLM или общей crash-транзакции; аппаратная приёмка not-run. |

## recording

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [REC-001](../specs/recording/spec.md#requirement-rec-001-явное-начало-архивной-записи) | [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs), [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | code-reviewed | RV-006: сохранение request не подтверждает аудио; честный ответ при отказе ещё нужен. |
| [REC-002](../specs/recording/spec.md#requirement-rec-002-pre-roll-и-время-начала) | [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs) | code-reviewed | Ограниченный RAM fallback допускает потерю при длительном отказе диска; физический отказ не прогонялся. |
| [REC-003](../specs/recording/spec.md#requirement-rec-003-stop-и-метки-звонка) | [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs) | code-reviewed; test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md); test-run 2026-11-10: [capture-owned planner/Stop/delete](../changes/add-incremental-recording-transcription/evidence/capture-planner.md); test-run: [Windows atomic Stop, 64/64](../changes/add-incremental-recording-transcription/evidence/atomic-stop.md) | Отдельный runtime всех сценариев не выполнен. |
| [INV-REC-001](../specs/recording/spec.md#requirement-inv-rec-001-разные-границы-паузы) | [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs), [src/chunks.rs](../../crates/localvox-light-core/src/chunks.rs) | test-run (частично): `test:recorder_pause_closes_and_resumes_new_chunk`; test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md); test-run 2026-11-10: [capture-owned planner/Stop/delete](../changes/add-incremental-recording-transcription/evidence/capture-planner.md) | Отдельный runtime всех сценариев не выполнен. |
| [REC-004](../specs/recording/spec.md#requirement-rec-004-потеря-устройства-и-отставание) | [src/engine.rs](../../crates/localvox-light-core/src/engine.rs), [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs) | test-run (частично): `test:a_backlog_in_our_own_queue_is_not_a_loss`, `test:a_pause_is_not_a_loss` | Устройства/длительная нагрузка не проверены; текущая unbounded очередь может расти. |
| [REC-005](../specs/recording/spec.md#requirement-rec-005-завершение-движка) | [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs), [src/engine.rs](../../crates/localvox-light-core/src/engine.rs) | code-reviewed; test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md); test-run 2026-11-10: [capture-owned planner/Stop/delete](../changes/add-incremental-recording-transcription/evidence/capture-planner.md) | RV-014 / BL-018: исходное зависание release не локализовано. |
| [INV-REC-002](../specs/recording/spec.md#requirement-inv-rec-002-controls-independent-of-pcm) | [src/engine.rs](../../crates/localvox-light-core/src/engine.rs), [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs), [src/cli.rs](../../crates/localvox-light-core/src/cli.rs) | code-reviewed; test-run 2026-11-09: [подтверждённый capture/FLAC и API](../changes/add-incremental-recording-transcription/evidence/capture-input.md); test-run 2026-11-10: [capture-owned planner/Stop/delete](../changes/add-incremental-recording-transcription/evidence/capture-planner.md); test-run: [Windows atomic Stop, 64/64](../changes/add-incremental-recording-transcription/evidence/atomic-stop.md) | BL-018: синтетический тест без устройства; native загрузка сама не прерывается. |

## runtime-settings

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [RUN-001](../specs/runtime-settings/spec.md#requirement-run-001-окно-отдельно-от-движка) | [src/lib.rs](../../src-tauri/src/lib.rs) | code-reviewed | Только сборка desktop; TCP listener не подтверждает именно LocalVox. |
| [RUN-002](../specs/runtime-settings/spec.md#requirement-run-002-комплект-бинарников-и-ui) | [src/autocook.rs](../../crates/localvox-light/src/autocook.rs), [src/http.rs](../../crates/localvox-light-api/src/http.rs), [../Cargo.toml](../../Cargo.toml) | test-run (частично): [run script](../../crates/localvox-light-terminal/tests/run_script.rs), [ConPTY bundle](../../crates/localvox-light/tests/launcher_modes.rs) | Совместная dev-сборка; BuildOnly/conflicting flags/build failure/missing artifacts и переносимый TUI проверены. Release CI и аппаратная приёмка not-run. [Результаты](../changes/add-technical-journal-workspace/evidence/implementation.md). |
| [CFG-001](../specs/runtime-settings/spec.md#requirement-cfg-001-сохранение-настроек) | [src/http.rs](../../crates/localvox-light-api/src/http.rs), [src/env_file.rs](../../crates/localvox-light-core/src/env_file.rs), [src/settings.rs](../../crates/localvox-light-core/src/settings.rs) | test-run: [изолированный HTTP](../../crates/localvox-light-api/src/http/tests/settings.rs), 15 env_file tests | bool/number/null, запрещённый PATH, комментарии, ошибка чтения/записи до apply; Windows read-deny/write-allow не затирает конфиг. Настоящий temp .env отдельного процесса. [Результаты](../changes/add-technical-journal-workspace/evidence/implementation.md). Дополнение: [предустановки и LLM URL 2026-11-02](../changes/refine-workspace-controls-and-settings/evidence/implementation.md). |
| [CFG-002](../specs/runtime-settings/spec.md#requirement-cfg-002-область-действия-настроек) | [src/settings.rs](../../crates/localvox-light-core/src/settings.rs), [src/autocook.rs](../../crates/localvox-light/src/autocook.rs) | test-run: каталог, Config/child_env, [HTTP](../../crates/localvox-light-api/src/http/tests/settings.rs)  Дополнение: [TUI UX 2026-11-01](../changes/add-technical-journal-workspace/evidence/usability-2026-11-01.md). | RV-001 исправлен честным Restart: старый coordinator сохраняет снимок, новый читает сохранённое значение; unknown key возвращает need_restart. Hot reload и физический capture не заявлены. [Результаты](../changes/add-technical-journal-workspace/evidence/implementation.md). Дополнение: [предустановки и LLM URL 2026-11-02](../changes/refine-workspace-controls-and-settings/evidence/implementation.md). |
| [CFG-003](../specs/runtime-settings/spec.md#requirement-cfg-003-показ-секретов-и-значений) | [src/http.rs](../../crates/localvox-light-api/src/http.rs), [src/settings.rs](../../crates/localvox-light-core/src/settings.rs) | test-run: [изолированный HTTP](../../crates/localvox-light-api/src/http/tests/settings.rs), [browser](../../ui/tests/e2e/workspace.spec.ts), client | RV-010 исправлен: известный secret value=null/set, unknown extra только key/set; оба клиента скрывают старый extra.value. Реальные secrets не использовались. [Результаты](../changes/add-technical-journal-workspace/evidence/implementation.md). |
| [CFG-005](../specs/runtime-settings/spec.md#requirement-cfg-005-сохранённые-адреса-llm-серверов) | [presets](../../crates/localvox-light-core/src/presets.rs), [API](../../crates/localvox-light-api/src/http.rs), [UI](../../ui/src/Settings.tsx), [TUI](../../crates/localvox-light-terminal/src/app/forms.rs) | test-run: настоящий settings HTTP, browser e2e, TUI draft/palette | Список адресов и активный URL разделены; нормализация/дедупликация, credentials отклоняются, Save/reopen; активный URL требует Restart. [предустановки и LLM URL 2026-11-02](../changes/refine-workspace-controls-and-settings/evidence/implementation.md). |
| [RUN-003](../specs/runtime-settings/spec.md#requirement-run-003-windows-автостарт) | [src/autostart.rs](../../crates/localvox-light-core/src/autostart.rs) | code-reviewed | Модуль Windows; реальный HKCU не менялся, другие ОС не приняты. |
| [INV-RUN-001](../specs/runtime-settings/spec.md#requirement-inv-run-001-component-shutdown-diagnostics) | [src/engine.rs](../../crates/localvox-light-core/src/engine.rs), [src/pipeline.rs](../../crates/localvox-light-core/src/pipeline.rs), [src/cli.rs](../../crates/localvox-light-core/src/cli.rs) | code-reviewed | BL-018: unit timeout/panic пройдены; причина исходного release-инцидента неизвестна. |

## search-chat

| Требование | Реализация / вход | Свидетельство | Разрыв или граница проверки |
|---|---|---|---|
| [SEARCH-001](../specs/search-chat/spec.md#requirement-search-001-индексируемые-данные) | [src/lib.rs](../../crates/localvox-light-search/src/lib.rs), [src/semantic.rs](../../crates/localvox-light-search/src/semantic.rs) | test-run (частично): `test:finds_stemmed_russian_word_with_timecode`, `test:finds_in_summary_paragraphs`  [BL-005 native](../changes/verify-background-indexing/evidence/implementation.md): файлы до API, best, все три режима. | Синтетические 3D-векторы, качество/полнота реального retrieval не приняты. |
| [SEARCH-002](../specs/search-chat/spec.md#requirement-search-002-режимы-и-деградация) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | test-run (частично): [временный HTTP](../../crates/localvox-light-api/src/http/tests/workspace.rs)  [RV-017](../changes/fix-empty-semantic-index-failure/evidence/implementation.md): empty-index 503/refused/recovery. | RV-008 и RV-017 исправлены в исходниках; lexical/hybrid и настоящий worker проверены в BL-005. Качество реальной модели отдельно. |
| [SEARCH-003](../specs/search-chat/spec.md#requirement-search-003-занятый-semantic) | [src/archive.rs](../../crates/localvox-light-api/src/archive.rs) | test-run (native synthetic): [BL-005 concurrent warmup/search](../changes/verify-background-indexing/evidence/implementation.md), file lock + SemanticGuard. | Частный параллельный HTTP fixture, без нагрузочной приёмки production listener. |
| [CHAT-001](../specs/search-chat/spec.md#requirement-chat-001-ответ-только-после-поиска) | [src/chat.rs](../../crates/localvox-light-api/src/chat.rs) | test-run (частично): `test:a_hit_is_expanded_into_the_conversation_around_it`, [HTTP](../../crates/localvox-light-api/src/http/tests/workspace.rs) | RV-008 исправлен: ошибка поиска не становится «не найдено»; нет фрагментов — нет вызова LLM; citations проверены с mock model. [Результаты](../changes/add-technical-journal-workspace/evidence/implementation.md); настоящий inference не выполнен. |
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

## Delta · постобработка и терминальный плеер · 2026-10-31

Дополнение к принятым CFG-001/002, PIPE-001/002/008/011/012 и ARCH-001/002/003.
Новые ID находятся в активном change и не увеличивают число требований main baseline.

| ID | Delta / код | Новые свидетельства | Граница |
|---|---|---|---|
| CFG-004 | [runtime-settings delta](../changes/control-postprocessing-and-terminal-player/specs/runtime-settings/spec.md), [settings](../../crates/localvox-light-core/src/settings.rs), [UI](../../ui/src/Settings.tsx), [TUI](../../crates/localvox-light-terminal/src/app/forms.rs) | [test-run](../changes/control-postprocessing-and-terminal-player/evidence/transcript-summary-timeline.md): STT обязателен, один существующий live-флаг summary; legacy auto-clean env не включает очистку, сохранение/отмена | Старые задания сохраняют snapshot; модель/провайдер остаются Restart |
| PIPE-015 | [pipeline delta](../changes/control-postprocessing-and-terminal-player/specs/processing-pipeline/spec.md), [autocook](../../crates/localvox-light/src/autocook.rs), [workflow](../../crates/localvox-light-core/src/workflow.rs) | [test-run](../changes/control-postprocessing-and-terminal-player/evidence/transcript-summary-timeline.md): после STT только опциональная summary; ручная очистка и выключенная summary имеют разные причины skipped | Два часа — синтетические таймкоды, без замера моделей |
| ARCH-006 | [playback delta](../changes/control-postprocessing-and-terminal-player/specs/archive-playback/spec.md), [player](../../crates/localvox-light-terminal/src/player.rs), [render](../../crates/localvox-light-terminal/src/render.rs) | [test-run](../changes/control-postprocessing-and-terminal-player/evidence/implementation.md): native mpv/Range, pause/seek/fragment, F6, symbol/style snapshots | Null audio; физическая приёмка предыдущего change/10.2 остаётся not-run |
| ARCH-007 | [archive delta](../changes/control-postprocessing-and-terminal-player/specs/archive-playback/spec.md), [resolver](../../crates/localvox-light-core/src/readable.rs), [HTTP](../../crates/localvox-light-api/src/http.rs), [UI](../../ui/src/DocumentPane.tsx), [TUI](../../crates/localvox-light-terminal/src/render/documents.rs) | [resolver/API test-run](../changes/control-postprocessing-and-terminal-player/evidence/transcript-summary-timeline.md): исходные слова/имена/таймкоды сохранены в данных; [UI/TUI test-run 2026-11-01](../changes/control-postprocessing-and-terminal-player/evidence/plain-text-and-terminal-exit.md): raw Text цельный без меток, одно копирование всех/offscreen слов, формы/scroll/snapshots; processed оформление сохранено | Модель для fallback не вызывается; ложная квитанция не создаётся. Физический clipboard не проверялся |
| PIPE-016 | [pipeline delta](../changes/control-postprocessing-and-terminal-player/specs/processing-pipeline/spec.md), [workflow](../../crates/localvox-light-core/src/workflow.rs), [journal](../../crates/localvox-light-core/src/journal.rs), [UI](../../ui/src/Progress.tsx), [TUI](../../crates/localvox-light-terminal/src/render.rs) | [test-run](../changes/control-postprocessing-and-terminal-player/evidence/transcript-summary-timeline.md): переходы/ожидание/backoff/неизвестные времена, orphan running, связь попытки с операцией, заморозка при ошибке и стабильный итоговый таймер | Настоящая загрузка/скорость моделей и физическое оборудование не проверялись |

Дополнительный [прогон смешанного журнала и замер опроса](../changes/control-postprocessing-and-terminal-player/evidence/mixed-history-polling.md)
для PIPE-002/004/008/011 и delta PIPE-016, 2026-10-31: воспроизведён и исправлен
сброс StageEvent строками journal.run; настоящий HTTP сохраняет границы STT при
запуске/завершении summary, startup-reclaim замечает прерванный этап. Потоковое
чтение и исключение повторного разбора не меняют канонический SHA-256; прежние
проверки повреждений и переименований пройдены заново. Числа замера относятся
к dev-опросу синтетических файлов; BL-003 и аппаратная приёмка остаются открытыми.

Дополнение 2026-10-31 к PIPE-001/002/009/011/013:
[доступность очереди при обнаружении работ](../changes/control-postprocessing-and-terminal-player/evidence/queue-discovery.md).
Настоящий POST и worker claim завершаются при остановленном на барьере scan;
проверены свежесть Job/metadata, новый worker/retry, lease, удаление, запись,
выключение summary и битая очередь. Замер задержки claim относится к temp данным
и dev-сборке; полный throughput моделей BL-003 не заявляется.

## Delta · чтение документов браузером · 2026-11-01

| ID | Delta / код | Свидетельство | Граница |
|---|---|---|---|
| ARCH-008 | [archive delta](../changes/fix-browser-artifact-refresh/specs/archive-playback/spec.md), [useArtifact](../../ui/src/useArtifact.ts), [DocumentPane](../../ui/src/DocumentPane.tsx), [API client](../../ui/src/api.ts) | [test-run](../changes/fix-browser-artifact-refresh/evidence/implementation.md): до исправления 4 GET вместо 1 при удерживаемом ответе; после — последовательное/coalesced чтение, финальный refresh, отмена пяти GET при уходе, смене сессии/версии, восстановление после 503; полный browser и совместный API/UI/TUI | Медленные ответы искусственные; отмена клиента не доказывает остановку HTTP-сервера. Модели/аппаратная приёмка не запускались |

## Delta · опрос состояния UI · 2026-11-01

| ID | Delta / код | Свидетельство | Граница |
|---|---|---|---|
| API-006 | [API delta](../changes/fix-workspace-status-polling/specs/api-access/spec.md), [App](../../ui/src/App.tsx), [usePolling](../../ui/src/usePolling.ts), [API client](../../ui/src/api.ts) | [test-run](../changes/fix-workspace-status-polling/evidence/implementation.md): до исправления перекрывались sessions/jobs/record; после — независимые последовательные циклы, свежий снимок после start/stop/import, отмена старого GET, сохранение выбора и backoff/recovery; полный browser и общий API/UI/TUI | Mock задержки и команды записи, без физического захвата; REC-001/003 и аппаратная приёмка этим полностью не закрываются |

## Контрольные измерения worker · 2026-11-01

Дополнение к PIPE-004/008/009/011 и BL-003/008:
[инструмент и release-результаты](../changes/benchmark-processing-batch/evidence/calibration.md).
В изолированных сессиях выполнены пять публичных коротких WAV × 2 и отдельный
602-секундный WAV с настоящим GigaAM, подтверждены непустые транскрипты и receipts.
Есть fingerprint, времена и наблюдаемые Windows worker peak/CPU. Полная очередь,
двухчасовая встреча, настоящая summary и численный бюджет ещё не измерены;
BL-003/008 и аппаратная приёмка остаются открытыми.

[Продолжение с настоящими Qwen/GLiNER](../changes/benchmark-processing-batch/evidence/summary-probes.md):
измерены native HTTP stats на 602 s и синтетическом 7225.554 s; два ответа по 8192
токена заняли 174.8 s, проверки — 87.5 s. Содержание длинной summary помечено
сомнениями. Это диагностика прямых worker, не полный бюджет координатора.

## Сохранение исхода worker · 2026-11-01

Дополнение к PIPE-008/010/011:
[исправление и проверки](../changes/persist-worker-outcomes/evidence/implementation.md),
[уточнение PIPE-008](../changes/persist-worker-outcomes/specs/processing-pipeline/spec.md).
Воспроизведён rollback failure mark_failed; типизированные storage errors больше
не считаются невалидным артефактом. Windows deny/release проверяет сохранение той
же попытки без нового worker, исходную причину и публикацию Failed после очереди;
stop проверен для обоих исходов. [Короткий справочник](../../docs/user-guide.md)
сверен с пользовательскими входами; правило AGENTS.md требует его сопровождения.
Реальные модели/захват, multi-file crash, BL-002/003/004 и аппаратная приёмка не закрыты.

## Сохранение переходов очереди · 2026-11-01

Дополнение к PIPE-002/008/010/011:
[исправление и проверки](../changes/commit-queue-execution-transitions/evidence/implementation.md),
[уточнение PIPE-002](../changes/commit-queue-execution-transitions/specs/processing-pipeline/spec.md).
Три воспроизведённых отказа записи больше не оставляют ложный Running, не тратят
attempt/revival и не дописывают recovery progress до сохранения. Startup helper
проверен с настоящим Windows deny-replace, снятием запрета и остановкой; размещение
барьера перед созданием линий проверено по коду. Это не полный прогон демона с
моделями, аудит остальных мутаций или crash-транзакция queue/progress. BL-002/003/004
и физическая приёмка 10.2 остаются открытыми.

## План автоматического повтора · 2026-11-01

Дополнение к PIPE-010/011 и delta PIPE-016:
[план автоматического повтора](../changes/explain-automatic-retry-state/evidence/implementation.md),
[уточнение PIPE-011](../changes/explain-automatic-retry-state/specs/processing-pipeline/spec.md).
Воспроизведён ложный retry_wait при disabled coordinator; общий plan теперь
различает доступность/неподтверждённость и исчерпание, сохраняет deadline 1/2/4 часа
и неизвестное время. API/UI/TUI показывают фазу, попытки, причину и «не раньше»;
чтение не меняет jobs.json. Это не изменение scheduler или закрытие BL-002/003/004.

## Ручное возобновление обработки · 2026-11-01

Продолжение PIPE-010/011/012: [ручное возобновление Failed](../changes/resume-failed-processing/evidence/implementation.md),
[уточнение PIPE-010](../changes/resume-failed-processing/specs/processing-pipeline/spec.md).
Кнопка очереди больше не отправляет полный recook. Проверены сохранение фазы,
параметров и артефактов, fresh budget/force=false, отказ busy/recording/queue I/O,
повторный POST, pending/error/success UI и R/палитра/формы TUI.
Автоматический ремонт Done и controlled cancel/delete остаются BL-004/002.

## Проверка отказа чтения очереди · 2026-11-01

Дополнение к PIPE-002/011: [исправление и проверки](../changes/fix-queue-read-failures/evidence/implementation.md),
[уточнённые сценарии PIPE-011](../changes/fix-queue-read-failures/specs/processing-pipeline/spec.md).
Настоящий HTTP воспроизвёл успешную пустую queue/jobs при повреждении; после
исправления invalid JSON и I/O-отказ дают 503 без мутации хранилища, исправленный
файл возвращает правильные задания/счётчики. UI/TUI сохраняют последние данные с
отметкой недоступности, отличают начальную ошибку от пустоты и снимают её при
восстановлении. Модели, аппаратная приёмка и известные BL-002/003/004 не закрываются.

## Delta · жизненный цикл браузерного плеера · 2026-11-01

| ID | Delta / код | Свидетельство | Граница |
|---|---|---|---|
| ARCH-009 | [archive delta](../changes/fix-browser-player-lifecycle/specs/archive-playback/spec.md), [Player](../../ui/src/Player.tsx), [API client](../../ui/src/api.ts), [useArtifact](../../ui/src/useArtifact.ts) | [test-run](../changes/fix-browser-player-lifecycle/evidence/implementation.md): воспроизведён сброс 20 → 0; сохранены pause/playing/until при обновлении duration, остановка/сброс при смене источника/уходе/исчезновении аудио, без старого autoplay; последовательные/coalesced peaks, отмена/поздний ответ и восстановление после 503 | Реальный HTMLAudioElement с синтетическим PCM; длинный пользовательский FLAC, устройства и рост открытого WAV не проверялись |

## Сопровождение исходников · 2026-11-01

[Штатное форматирование API](../changes/format-api-rust-sources/evidence/implementation.md):
сверены изменения оформления build/CLI, archive, chat и HTTP; выполнены существующие
тесты API после форматирования. Требования и значения не менялись. Это дополнительная
проверка сохранения реализации API/ARCH/CHAT/CFG/PIPE, без закрытия известных RV/BL
или заявления о выполнении всех сценариев этих capability.


Продолжение PIPE-010/011/012, ARCH-005 и INV-SESSION-001 от 2026-11-01:
[BL-002: отмена и владение](../changes/archive/2026-11-01-serialize-session-mutations/evidence/implementation.md).
Остановка и отмена отделены от Failed/Done; finished clocks не растут, готовый текст
сохраняется. Это не аппаратная приёмка и не закрытие остальных RV/BL.

## Длинная запись и группировка голосов · 2026-11-10

| ID | Delta / код | Свидетельство | Граница |
|---|---|---|---|
| SPK-001 / SPK-004 | [speaker delta](../changes/fix-long-recording-diarization/specs/speakers/spec.md), [cluster](../../crates/localvox-light-core/src/diarize/cluster.rs), [cook](../../crates/localvox-light-core/src/cook.rs) | [test-run и release замер](../changes/fix-long-recording-diarization/evidence/implementation.md): 480 сравнений с эталоном, граничные/ошибочные входы, 2048/8000 векторов, native 180 s PCM; [установленный worker на 304 минутах](../changes/fix-long-recording-diarization/evidence/deployment.md) — 481.341 s, группировка 1.678 s | Новая delta не синхронизирована; ручное качество/аппаратная приёмка not-run |
| PIPE-011 | [pipeline delta](../changes/fix-long-recording-diarization/specs/processing-pipeline/spec.md), [journal](../../crates/localvox-light-core/src/journal.rs), [UI](../../ui/src/Journal.tsx), [TUI](../../crates/localvox-light-terminal/src/render.rs) | [test-run](../changes/fix-long-recording-diarization/evidence/implementation.md): отдельные grouping/error/elapsed/parent, UI/TUI и общий HTTP, полный UI e2e | Установлено и проверено на длинной записи; RV-021 (прошлые download/extract в next после recook) остаётся открыт; аппаратная приёмка not-run |

## Клавиши списка TUI · уточнение 2026-11-10

| ID | Delta / код | Свидетельство | Граница |
|---|---|---|---|
| ARCH-011 / ARCH-012 | [delta](../changes/add-terminal-session-activity/specs/archive-playback/spec.md), [input](../../crates/localvox-light-terminal/src/app/archive_list.rs), [render](../../crates/localvox-light-terminal/src/render.rs) | [test-run](../changes/add-terminal-session-activity/evidence/keyboard-search.md): j/k и RU, первый/второй Slash, Esc/Tab/Enter, refresh, snapshots, 153 lib и 4 ConPTY; установленный release TUI | Неархивированные delta уточнены по новому решению владельца, main specs не изменены. Ручной WezTerm/audio not-run; native Ctrl+V остаётся отдельным change |

## Часы этапов в журнале · 2026-11-10

| ID | Delta / код | Свидетельство | Граница |
|---|---|---|---|
| PIPE-011 | [delta](../changes/fix-journal-stage-durations/specs/processing-pipeline/spec.md), [TUI projection](../../crates/localvox-light-terminal/src/journal.rs), [UI projection](../../ui/src/journal-model.ts) | [test-run](../changes/fix-journal-stage-durations/evidence/implementation.md): текущий/завершённый этап, неполный хвост, retry/run guards, неизвестные границы, <1 с; 156 lib, 79 browser, 4 ConPTY | TUI установлен; новый embedded UI собран, установка daemon заблокирована работающим экземпляром. Ручной просмотр not-run, старые неизвестные границы не выдумываются |

## Безопасная вставка TUI · 2026-11-11

| ID | Delta / код | Свидетельство | Граница |
|---|---|---|---|
| ARCH-001; ARCH-014 и TERM-007 в delta | [paste delta](../changes/fix-windows-terminal-paste/specs/archive-playback/spec.md), [input](../../crates/localvox-light-terminal/src/runtime/input.rs), [App](../../crates/localvox-light-terminal/src/app.rs) | [test-run](../changes/fix-windows-terminal-paste/evidence/implementation.md): 162 lib, native Unicode/split paste/mode guard, 4 workspace ConPTY, IO/run-script, release TUI | Общий протокол без emulator branches; реальные Windows hosts, Unix, SSH/tmux/audio not-run. [Матрица](../../docs/terminal-compatibility.md), main specs не изменены |

## План повторного STT · 2026-11-11

| ID | Delta / код | Свидетельство | Граница |
|---|---|---|---|
| PIPE-011 / PIPE-012, RV-021 | [delta](../changes/fix-recook-next-stage-plan/specs/processing-pipeline/spec.md), [view/execution](../../crates/localvox-light-core/src/workflow.rs) | [test-run](../changes/fix-recook-next-stage-plan/evidence/implementation.md): 422 core, 81 API и 3 integration; actual WAV/HTTP, force/next/current phase, corruption/missing/Done guards; исправлена атомарная публикация test readiness | Исправлена реализация; реальная запись владельца/аппаратная приёмка not-run. Исполнение/очередь/модели и main specs не менялись |

## Vim-навигация списка и документа · 2026-11-11

| ID | Delta / код | Свидетельство | Граница |
|---|---|---|---|
| ARCH-001; ARCH-011/012 в delta | [delta](../changes/add-terminal-session-activity/specs/archive-playback/spec.md), [keys](../../crates/localvox-light-terminal/src/app.rs) | [test-run](../changes/add-terminal-session-activity/evidence/vim-navigation.md): l/д открывает запись; h/l и р/д как стрелки в документе; 164 lib, 4 ConPTY, IO/run-script, проверенный release кандидат | Обычный exe удерживает открытый TUI; кандидат готов отдельно. Ручная приёмка not-run; main specs не изменены |

Продолжение ARCH-001 / delta ARCH-011/012 от 2026-11-11:
[меню и безопасный qq](../changes/add-terminal-session-activity/evidence/menu-and-quit.md).
170 lib / 4 ConPTY / IO/run-script passed, сроки/контексты/вставка/Unicode и
auto-expiring hint проверены; 28 snapshots меняют только q → qq в footer.
Обычный release TUI и прежний review-кандидат обновлены; ручная приёмка not-run.

Продолжение ARCH-001 / delta ARCH-012 от 2026-11-11:
[фокус списка после отправки поиска](../changes/add-terminal-session-activity/evidence/search-focus.md).
171 lib / 4 ConPTY / IO/run-script passed; поздний ответ не перехватывает фокус,
плеер не запускается. Обычная/review сборки обновлены, ручной прогон not-run.

## Цвет технических подписей TUI · 2026-11-11

| ID | Delta / код | Свидетельство | Граница |
|---|---|---|---|
| ARCH-001; ARCH-015 в delta | [delta](../changes/refine-terminal-label-colors/specs/archive-playback/spec.md), [Theme/labels](../../crates/localvox-light-terminal/src/render.rs), [documents](../../crates/localvox-light-terminal/src/render/documents.rs) | [test-run](../changes/refine-terminal-label-colors/evidence/implementation.md): 174 lib, 4 ConPTY, IO/run-script; contrast/style tests 4 modes, 40 snapshots со строго прежним текстом/геометрией, release | Только TUI presentation. Внешний вид в реальном терминале владельца not-run; управление, копируемый текст, UI/сервер и main specs не менялись |

## Контекстное копирование и возврат TUI · 2026-11-11

| ID | Delta / код | Свидетельство | Граница |
|---|---|---|---|
| ARCH-001; ARCH-011/016 в delta | [delta](../changes/refine-terminal-copy-and-back/specs/archive-playback/spec.md), [navigation](../../crates/localvox-light-terminal/src/app/navigation.rs), [copy](../../crates/localvox-light-terminal/src/app/documents.rs) | [test-run](../changes/refine-terminal-copy-and-back/evidence/implementation.md): 183 lib, 4 ConPTY, IO/run-script; selected reply/whole text, back/parent/reorder/late replies/player state, 37 reviewed snapshots | Ручной clipboard/аудио и внешний вид not-run. ARCH-011 зависит от исходной delta add-terminal-session-activity; sync/archive не выполнялся, main specs не менялись |

Дополнение ARCH-016: [выбранный блок сводки](../changes/refine-terminal-copy-and-back/evidence/summary-copy.md).
y/н копирует выбранный SummaryBlock.text, Ctrl+Y — всю сводку; 185 lib,
4 ConPTY, IO/run-script passed, 12 snapshots просмотрены. Ручной clipboard not-run;
main specs и прежние свидетельства не переписаны.

## Раскладки команд UI/TUI · 2026-11-11

| ID | Delta / код | Свидетельство | Граница |
|---|---|---|---|
| PIPE-002/008/010/011/012, CFG-001/002; PIPE-019 в delta | [delta](../changes/reuse-asr-worker-between-horizons/specs/processing-pipeline/spec.md), [worker](../../crates/localvox-light-asr/src/bin/worker_server/mod.rs), [cache](../../crates/localvox-light-core/src/incremental/cache.rs) | [test-run](../changes/reuse-asr-worker-between-horizons/evidence/implementation.md): core/worker/process/coordinator/API/TUI; [прежний native](../changes/reuse-asr-worker-between-horizons/evidence/native-reference.md); [grouped cache R3](../changes/add-incremental-recording-transcription/evidence/default-on-validation.md) | Один engine, fresh identity, durable recovery; новые legacy/current R3 6/6 pass, default on; прежний fail сохранён; hardware/Mac not-run |
| ARCH-001; ARCH-019 в delta | [delta](../changes/resize-terminal-list-pane/specs/archive-playback/spec.md), [geometry](../../crates/localvox-light-terminal/src/render/geometry.rs), [actions](../../crates/localvox-light-terminal/src/app/pane_width.rs) | [test-run](../changes/resize-terminal-list-pane/evidence/implementation.md): 196 lib / 5 ConPTY, input/resize/reconnect/anchor, unchanged snapshots | Session preference; compact unchanged, external OS/terminal bindings not-run |
| ARCH-001/015; ARCH-018 в delta | [delta](../changes/refine-terminal-focus-details/specs/archive-playback/spec.md), [Theme/render](../../crates/localvox-light-terminal/src/render.rs) | [test-run](../changes/refine-terminal-focus-details/evidence/implementation.md): 190 lib / 5 ConPTY, focus/contrast, 38 style-only snapshots | Панели/текст/действия сохранены; владелец и реальные terminal palettes not-run |
| ARCH-001; ARCH-017 в delta | [delta](../changes/configure-command-keymaps/specs/archive-playback/spec.md), [UI](../../ui/src/DocumentPane.tsx), [TUI keymap](../../crates/localvox-light-terminal/src/keymap.rs) | [test-run](../changes/configure-command-keymaps/evidence/implementation.md): 189 lib / 5 ConPTY / IO/run-script; UI build, 14 unit / 2 browser e2e; прежние snapshots | Physical code для UI, явные символьные overrides для TUI. Реальные hosts/IME/Mac not-run; main specs не изменены |


Продолжение SETUP-001/002/003 от 2026-11-12:

| Requirement | Код / delta | Проверка | Граница |
|---|---|---|---|
| SETUP-001/002/003; SETUP-004 в delta | [delta](../changes/prepare-promo-and-platform-releases/specs/installation-models/spec.md), [packager](../../scripts/package-release.py), [Windows](../../scripts/install-release.ps1), [Unix](../../scripts/install-release.sh) | [test-run](../changes/prepare-promo-and-platform-releases/evidence/implementation.md): 12 delivery, real Windows startup/install/update, Mac shell fixtures, UI 14/81; 11 PNG + 4 GIF настоящих интерфейсов | Runtime обработки сохранён; remote release/hosted jobs/native Mac/Linux/устройства not-run; main specs не sync/archive |


Продолжение SETUP-001/002/003, INV-SETUP-001 от 2026-11-12:

| Requirement | Код / delta | Проверка | Граница |
|---|---|---|---|
| SETUP-001/002/003, INV-SETUP-001; SETUP-005/006 в delta | [delta](../changes/add-install-reconciliation-wizard/specs/installation-models/spec.md), [setup](../../crates/localvox-light/src/setup/mod.rs), [doctor](../../crates/localvox-light-core/src/doctor.rs), [installers](../../scripts/install-release.ps1) | [test-run](../changes/add-install-reconciliation-wizard/evidence/implementation.md): 10 setup integration / 16 lib / 1 main / 6 doctor; 16 delivery без skips, настоящий Windows bundle, Mac shell fixtures | Read-only discovery, plan/confirm, ownership, repeat no-op; настоящие Ollama installer/pull/GGUF/Mac first-run not-run; main specs не sync/archive |

Дополнение CFG-001/002/003 и SETUP-006 delta от 2026-11-12:

| Requirement | Код | Проверка | Граница |
|---|---|---|---|
| CFG-001/002/003; SETUP-006 в delta | [env_file](../../crates/localvox-light-core/src/env_file.rs), [setup](../../crates/localvox-light/src/setup/execute.rs) | [test-run](../changes/consolidate-settings-persistence/evidence/implementation.md): 20 core, copied-process HTTP, 16 lib / 1 main / 10 setup; реальные read/rename denial, concurrent edits, stale/retry/no-op | Общий snapshot/atomic writer, HTTP/CLI и apply_live policies сохранены; arbitrary cross-process CAS, power-loss/disk-full и native Unix/Mac not-run |

Дополнение PIPE-002/008/010/011 от 2026-11-12:

| Requirement | Код | Проверка | Граница |
|---|---|---|---|
| PIPE-002/008/010/011 | [queue](../../crates/localvox-light-core/src/jobs.rs) | [test-run](../changes/harden-queue-and-http-lifecycles/evidence/queue.md): исходные 2 failed воспроизведены; после fix 78 jobs / 440 core / 16 coordinator passed | Checked update/removal/forced enqueue; прежние reset/budgets/schema сохранены; hardware/Mac/performance not-run |
| API-003 | [HTTP](../../crates/localvox-light-api/src/http.rs), [budget](../../crates/localvox-light-api/src/http/budget.rs) | [test-run](../changes/harden-queue-and-http-lifecycles/evidence/http.md): 5 budget/concurrency/unwind/real HTTP; [общий прогон](../changes/harden-queue-and-http-lifecycles/evidence/final.md): 440 core / 89 API / 27 app, release/package | 32 slots, spawn failure/503/recovery; LLM limit/body/auth сохранены; process abort/OS exhaustion/native Mac not-run |

Дополнение SETUP-001/002/003, INV-SETUP-001 и terminal input от 2026-11-12:

| Requirement | Код / delta | Проверка | Граница |
|---|---|---|---|
| SETUP-001/002/003, INV-SETUP-001; SETUP-005/006 в delta | [smoke](../../scripts/check-release-install.py), [installer](../../scripts/install-release.ps1), [workflow](../../.github/workflows/build-binaries.yml) | [test-run](../changes/showcase-features-and-repair-ci/evidence/implementation.md): исходный npm EUSAGE воспроизведён на runner npm; clean ci/build/UI 14/81; 20 delivery без skips, настоящий полный Windows bundle, doctor/check/wizard/reconcile без повторной записи | Fail-fast native errors, expected doctor 0/1/2; hosted rerun/native Mac/Linux и аппаратная приёмка not-run; README использует Actions до нового полного release |
| ARCH-001; ARCH-014 / TERM-007 в delta | [paste delta](../changes/fix-windows-terminal-paste/specs/archive-playback/spec.md), [Windows input](../../crates/localvox-light-terminal/src/runtime/input/windows.rs) | [test-run](../changes/showcase-features-and-repair-ci/evidence/implementation.md): Alt-release UTF-16 regression red→green; 200 TUI lib и 5 workspace ConPTY, IO/run-script; strict emoji assertion сохранена | Локальный native paste проходил и до fix; точное представление hosted failure не установлено, добавлена bounded test-only диагностика. GitHub rerun/host UI/Mac/Unix not-run |

Продолжение hosted-проверки от 2026-11-12 после push/fast-forward владельца:

| Requirement | Код | Проверка | Граница |
|---|---|---|---|
| RUN-002, SETUP-002/003; SETUP-004/005/006 в delta | [runtime build](../../scripts/build-onnxruntime-macos.py), [packager](../../scripts/package-release.py), [launcher fixture](../../crates/localvox-light-terminal/tests/run_script.rs) | [hosted и test-run](../changes/fix-hosted-native-builds/evidence/implementation.md): Windows и Linux ARM delivery jobs passed; локально 27 delivery без skips, 3 launcher tests, 16 app / 10 setup / 6 doctor; alias regression red→green | Linux x64 glibc и Intel Mac prebuilt failures исправлены конфигурацией/SDK builder; новый hosted/native Mac run not-run. Main слит/отправлен владельцем; новый fix commit локально, без push |
| ARCH-001; ARCH-014 / TERM-007 в delta | [Windows input](../../crates/localvox-light-terminal/src/runtime/input/windows.rs), [native probe](../../crates/localvox-light-terminal/src/runtime/paste_probe.rs) | [hosted](../changes/fix-hosted-native-builds/evidence/implementation.md): 200 TUI lib, Unicode native paste и 5 workspace ConPTY passed в 43245450996 | Позднее падение run-script fixture отдельно исправлено; реальный UI терминальных hosts/Mac/Unix остаётся not-run |

Дополнение короткой команды от 2026-11-12:

| Requirement | Код / delta | Проверка | Граница |
|---|---|---|---|
| SETUP-007 / RUN-005 в delta | [setup delta](../changes/add-vox-entry-command/specs/installation-models/spec.md), [runtime delta](../changes/add-vox-entry-command/specs/runtime-settings/spec.md), [entry](../../crates/localvox-light-terminal/src/entry.rs), [registration](../../crates/localvox-light-terminal/src/command_path.rs) | [test-run](../changes/add-vox-entry-command/evidence/implementation.md): 207 lib / 6 ConPTY / 4 launcher / 2 IO, 29 delivery без skips, настоящий Windows bundle | Проверки registry/profile изолированы; native Unix/Mac, глобальный PATH и hosted нового commit not-run. Обычный выход сохраняет демон; full exit отдельный change |

Полный выход, 2026-11-12:

| Requirement | Код / delta | Проверка | Граница |
|---|---|---|---|
| RUN-006 в delta, INV-RUN-001 | [delta](../changes/add-graceful-application-exit/specs/runtime-settings/spec.md), [control](../../crates/localvox-light-api/src/application.rs), [daemon](../../crates/localvox-light/src/main.rs), [TUI](../../crates/localvox-light-terminal/src/app/commands.rs) | [test-run и первые failed](../changes/add-graceful-application-exit/evidence/implementation.md): app 16+2 / 10 setup, API 91 / 3 integration, client 3+10, TUI 211 / 8 PTY / 4 launcher / 2 IO | Отдельный confirmed exit с cleanup outcome; обычный выход client-only. Full-exit test и blank-cell assertion исправлены, final parallel suite passed; реальная запись/Mac/Linux/hosted not-run |

Подтверждение полного выхода без текста, 2026-11-12:

| Requirement | Код / delta | Проверка | Граница |
|---|---|---|---|
| RUN-006 MODIFIED в delta | [delta](../changes/simplify-terminal-shutdown-confirmation/specs/runtime-settings/spec.md), [forms](../../crates/localvox-light-terminal/src/app/forms.rs), [overlay](../../crates/localvox-light-terminal/src/render/overlays.rs) | [test-run](../changes/simplify-terminal-shutdown-confirmation/evidence/implementation.md): 212 lib / 8 PTY / 4 launcher / 2 IO, пустой диалог Ctrl+S/Ctrl+Enter, Esc и безопасный ввод/paste, 80/120 render | Lifecycle/API и прочие typed confirmations сохранены; sync после исходного RUN-006, реальные audio/Mac/hosted not-run |

Native CI environment, 2026-11-13:

| Requirement | Код | Проверка | Граница |
|---|---|---|---|
| RUN-002, SETUP-002/003; SETUP-007 в delta | [compiler pin](../../rust-toolchain.toml), [workflows](../../.github/workflows/build-binaries.yml), [shell fixture](../../crates/localvox-light-terminal/src/command_path/profiles/tests.rs) | [hosted failures и test-run](../changes/repair-native-ci-environments/evidence/implementation.md): Rust 1.99.1 Windows daemon/worker/TUI/desktop + package probes; 212 lib / 8 PTY / 4 launcher / 2 IO, 9 onnx cook, 30 delivery без skips; isolated Unix modules compiled | Общий pin вместо moving stable, shell lookup до child PATH isolation; SDK/модели не менялись. Full Linux cross-check недоступен без C compiler; новый native Mac/Linux/hosted run not-run до push |

Hosted update и диагностический этап, 2026-11-13:

| Requirement | Код | Проверка | Граница |
|---|---|---|---|
| RUN-002, SETUP-002/003; SETUP-007 в delta | [workflows](../../.github/workflows/build-binaries.yml) | [hosted 43395635060](../changes/repair-shared-client-ci-fixture/evidence/implementation.md): **все пять** Windows/Linux x64/ARM64/macOS Intel/ARM64 jobs passed на 4b90802, bundles/install/doctor/reconcile/registration | Release publish skipped для обычного push; устройства/запись на Mac не проверены |
| ARCH-001, API-003, CFG-001/002/003 | [shared fixture](../../crates/localvox-light-api/src/http/tests/cross_clients.rs), [browser runner](../../ui/tests/integration/actual-api.mjs) | [diagnostic checkpoint](../changes/repair-shared-client-ci-fixture/evidence/implementation.md): hosted Interface 43395635063 failed; локально прежний debug launch passed, новый debug 4 раза passed, release passed; API 91, media/FLAC 9, UI unit 15 passed | Исходная hosted причина не воспроизведена, остаётся [открытая работа](../changes/repair-shared-client-ci-fixture/tasks.md); сохранены status/stdout/stderr и startup errors, новый hosted run not-run до push |

PIPE-011 browser clock fixture, 2026-11-13:

| Requirement | Код | Проверка | Граница |
|---|---|---|---|
| PIPE-011 | [timing test](../../ui/tests/e2e/workspace.spec.ts), [workflow](../../.github/workflows/interface-tests.yml) | [red→green и hosted](../changes/stabilize-pipeline-clock-test/evidence/implementation.md): d30727b native build 5/5 passed; Interface UI 80/81 failed на живом clock. После fix UI build / unit 15 / e2e 81 / ещё 4 timing повтора passed, PNG и assertions сохранены | Только тестовые часы и capped polling backoff, production без изменений. Shared-client шаг на d30727b skipped; его прежняя причина открыта. Новый hosted not-run до push; connector rerun 403 |

Windows audio COM lifetime и hosted 873ffa2, 2026-11-13:

| Requirement | Код | Проверка | Граница |
|---|---|---|---|
| CFG-001/002/003, ARCH-001, API-003 | [Windows audio host](../../crates/localvox-light-core/src/audio/windows_host.rs), [gateway](../../crates/localvox-light-core/src/audio.rs), [regression](../../crates/localvox-light-core/src/audio/host_tests.rs) | [native red→green и hosted](../changes/repair-shared-client-ci-fixture/evidence/windows-audio.md): CPAL STA lifetime reproduced 0xC0000005 → four owned regression modes passed; core 441 / API 91+3 / TUI 212+8 PTY+4 launcher+2 IO / client 3+10 / process 12 passed; real shared UI/TUI and mpv passed; release product+desktop/package/install/doctor/reconcile passed | Общий MTA adapter, CPAL/модели/контракт не обновлялись. На 873ffa2 UI 81 passed, shared failed; Intel Mac compiled/tested, upload DNS ENOTFOUND. Новый hosted и аппаратная запись после fix not-run; [remote acceptance](../changes/repair-shared-client-ci-fixture/tasks.md) открыта |

Hosted 99bd709 short paths, 2026-11-13:

| Requirement | Код | Проверка | Граница |
|---|---|---|---|
| SETUP-001/002, INV-SETUP-001; SETUP-008 в delta | [Bash parent](../../scripts/model-paths.sh) | [short HOME red→green](../changes/fix-model-storage-platform-paths/evidence/bash-hidden-short-path.md): hosted Linux x64/ARM64 success, Mac Intel Delivery passed, Windows единственный Bash Hidden failure; native 8.3 HOME probe false→true, model-storage 10 passed/2 skipped, packaged doctor/setup/reconcile passed | Новые hosted pending push. ARM Mac не получил hosted runner из-за capacity; текущий cancelled не test-run |

Voice grouping clock follow-up, 2026-11-13:

| Requirement | Код | Проверка | Граница |
|---|---|---|---|
| PIPE-011 | [voice grouping timing](../../ui/tests/e2e/workspace.spec.ts) | [red→green](../changes/stabilize-pipeline-clock-test/evidence/voice-grouping-clock.md): hosted 99bd709 UI 80/81, missed live 0:05; controlled clock probe passed, full e2e 81 / unit 15 / timing repeats 8 passed | Только второй timing fixture, PNG/production прежние. Новый hosted fix not-run до push, native Build отдельно |

Model storage hosted follow-up, 2026-11-13:

| Requirement | Код / delta | Проверка | Граница |
|---|---|---|---|
| SETUP-001/002, INV-SETUP-001; SETUP-008 в delta | [PS](../../scripts/model-paths.ps1), [Bash](../../scripts/model-paths.sh), [fetch](../../scripts/fetch-models.sh) | [follow-up](../changes/fix-model-storage-platform-paths/evidence/implementation.md): fee3103 Interface success, Linux x64/ARM64 success; Windows Hidden/Unicode curl и Mac /var guard failed; red→green regression PS 5.1/7.7.0 и Git Bash/NTFS junction | На момент той сверки новый hosted/native Mac после исправления не выполнялся; macOS ARM64 тогда cancelled, причина не была подтверждена; предположение fail-fast уточнено выше |

User model storage, 2026-11-13:

| Requirement | Код / delta | Проверка | Граница |
|---|---|---|---|
| SETUP-001/002; SETUP-008 в delta | [model paths](../../crates/localvox-light-core/src/model_paths.rs), [delta](../changes/use-user-model-storage/specs/installation-models/spec.md) | [evidence](../changes/use-user-model-storage/evidence/implementation.md): core 447, setup 10, copied binary 1, PS/Bash localhost + native bridge 6 passed; explicit/portable/home и read-only/repeat | Native Mac/Linux, настоящий download весов и hosted run not-run; main SETUP-008 ещё не синхронизирован |

Delta coherence, 2026-11-13:

| Requirement | Код / delta | Проверка | Граница |
|---|---|---|---|
| LLM-003, PIPE-006 | [retention delta](../changes/fix-text-correction-retention/specs/llm-processing/spec.md) | [reconciliation](../changes/fix-text-correction-retention/evidence/delta-reconciliation-2026-11-13.md): hosted 44df41e functional checks включая shared UI/TUI passed, specification validation failed из-за трёх пропущенных main-сценариев; восстановлены дословно; strict main + все 39 workflow changes passed локально | Нормативный main, runtime и исторические результаты не менялись; весь hosted Interface ещё не green |
