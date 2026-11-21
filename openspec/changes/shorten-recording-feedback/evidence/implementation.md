# REC-006 / CFG-006 — 2026-11-21

По запросу владельца живые файлы закрываются примерно раз в 30 с вместо 300 с.
ASR recipe остался 15 с / 8 с / 500 мс, Prepare импорта по-прежнему использует
свои 300 с и весь доступный вход. Pre-roll проходит обычными capture frames:
90 секунд прошлого больше не превращаются в один большой файл.

Причина отсутствия out в прежней установленной записи: loopback не был включён,
сохранённой device config/явного override не было; manifest содержал только PCM
source 0 и пустую source 1. Это не доказательство сбоя WASAPI. Default теперь
включает оба источника. Отдельный Bool и выбор выхода видны в UI/TUI.

Defaults Capture разрешаются ниже сохранённой device config; явный env/CLI on/off
имеет приоритет. Пустое значение использует preset. Реальный settings HTTP в
отдельном процессе проверил restart без controls, затем off/on/reset и изменение
реального capture snapshot/reload generation с controls, без запуска устройств.

## Свидетельства

| Требование | Код | Выполненная проверка |
|---|---|---|
| REC-006 | `.env.defaults`, `pipeline::seed_preroll`, `chunks::planning` | `live_default_seals_both_tracks_and_offers_work_after_thirty_seconds_without_stop`: 31 с двух PCM дорожек → реальные sealed ranges/planner/horizon/раннее задание без EOF; после 61 с + Stop все границы/PCM hashes совпадают с цельным входом |
| REC-006 | `pipeline::seed_preroll` | `long_preroll_uses_short_live_rotation_without_losing_samples`: 90 с двух источников, несколько closed файлов до Stop, каждый ≤30,04 с, полное покрытие 90 с |
| CFG-006 | `cli::resolve_audio_config`, `presets::apply_missing`, `audio::reload_capture_from_env` | default on, saved off, env on/off, no-loopback, blank; настоящий settings HTTP round-trip; UI переключатель и отдельный выход |

Core **454 passed / 9 ignored**, API **94 passed / 8 ignored**; frontend **16 unit**,
**83 browser e2e passed**, production build passed. Три negative visual tests
ожидаемо обнаружили искажения; browser baselines не обновлялись.
Настоящие UI/TUI против одного временного HTTP/архива — passed.
Новый CLI helper test добавлен в существующий Interface workflow.

## Локальная поставка и границы

Release daemon/worker/TUI/vox/installer собраны с ONNX через ASR feature unification.
После штатного завершения владельцем обновлены пять exe и `.env.defaults` в F:\lv,
manifest hash/bytes актуальны, release_tag=null для локального кандидата.
Help/protocol/версии/bundle-check прошли с native diagnostic guard.
SHA-256 пользовательского `.env` и существующих progress.jsonl не изменились;
модели/архив не переписывались. Backup — ignored target/review/recording-controls-2026-11-21/original.

Владелец сообщил «оно работает». Read-only проверка новой записи показала обе
дорожки с пятью порциями по 30,016 с и последними короткими хвостами после Stop.
Наличие файлов не выдаётся за проверку качества речи/каждого звукового устройства.
Длинная аппаратная запись и Mac из пункта 7.4 раннего change остаются not-run.
Если ожидаемая дорожка вообще не поступает, общий префикс по-прежнему ждёт её/Stop.
