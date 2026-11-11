# Итоговая проверка модульного рефакторинга · 2026-11-11

Рабочая точка `b3766b6`, срезы TUI `27bb05e` и UI/API `bb67f9f`.
Объём ограничен [семью задачами](../tasks.md). Новых пользовательских действий,
форматов, моделей, рецептов и defaults нет. Карта всех 56 US-сценариев и точек
замены — [modularity](../../../../docs/modularity.md).

## Test-run

| Проверка | Результат |
|---|---|
| Общий Rust, восемь затронутых пакетов, fixture feature, jobs=2 | 908 passed, 0 failed; ignored helpers/devices отдельно |
| Coordinator/worker explicit через Cargo | 8 passed, включая finite-prefix fault/cancel/retry и индексацию |
| Настоящие UI + TUI на общем временном HTTP-архиве | 1 passed, Chromium + ConPTY; processing artifacts синтетические |
| UI production build / unit / browser | build passed, 14 unit и 80 browser passed; PNG не менялись |
| После ограничения ожидания HTTP fixture | 3 HTTP adapter tests passed повторно; production logic не менялась |
| Native ASR smoke, текущий capture, 330 s, одна пара | pass, одинаковые окна/текст/ASR calls |

Подробности и пределы: [TUI](tui.md), [UI/API](ui-api.md), [processing](processing.md).
UI unit/browser выполнены при разделении views/controller, production build повторён
перед финальной сборкой; после этих прогонов UI source не менялся. Rust включает
198 TUI library, 5 ConPTY, 81 API unit/3 integration, 462 core, 113 LLM/corpus,
ASR fixtures/PCM, process lifecycle, ingest, IO/run-script и golden fbank.

Native: сохранённый публичный PCM, модель GigaAM v3 int8, ru,
`gigaam-int8/w15-c8-s500-r7`. В каждом режиме 5 280 000 input samples, **75 windows,
33 ASR calls, 33 lines, одна загрузка и один worker**. `same_windows`, `same_text`,
`same_asr_calls` = true. В этой короткой паре active overhead +0.95%, Stop→ready
5.346 s / 0.944 s. `reference=false`: это smoke/parity, не новый полный performance
gate. Старый legacy-reference fail и отдельный current-capture reference сохраняют
свои прежние результаты; default раннего режима остаётся off.

Raw native report: ignored `target/profile-modular-native-20261111/report.json`,
SHA-256 `7fbebbddaae93e252d851f4b49ee56f6ed36980f63bffd3fffcc97c5714f12f0e`.
Plan рядом содержит input/model/vocab SHA и параметры. Дневники команд находятся
в ignored `target/tmp/modular-*.log`; личный архив/устройства не использовались.

## Комплект

Первый build в обычный `target/release` не завершился: работающий пользовательский
daemon удерживал EXE. Он не останавливался. Чистая optimized сборка всех четырёх
бинарников выполнена в `target/modular-build/release`, без fixture feature.
Проверяемый runtime комплект — **`target/review/modular-boundaries`**:
daemon, worker, TUI, Vosk DLL из `vosk-lib` и generated DirectML DLL. Старый helper
build.rs определяет DLL destination через env, поэтому при CLI `--target-dir`
runtime DLL явно упакованы в отдельный комплект; старые DLL не заменялись.

| EXE | SHA-256 |
|---|---|
| localvox-light | `5441a4519817b85ab200b11865cf0eb7f39b57b12716e4316be886f4ab2ea8ec` |
| localvox-process | `ecb051d935163dee2b2a3cd005e4104096ac47e24f22cc85858096607eb96245` |
| localvox-tui | `f9b588258c885945836af1822cc8e235a82aded0412f25bbee1889909b19e11a` |

`manifest.json` рядом хранит длины и SHA всех runtime файлов. Worker handshake
`localvox-worker/3`, TUI `--version` и daemon `--help` проверяются без старта записи.
Проектные модели и настройки остаются на прежних местах; запуск из корня репозитория.

## Границы приёмки

Strict main specs **15 valid / 0 failed**, strict change valid (`skip_specs`),
`check-openspec.py` PASS (15 capabilities / 88 requirements / 188 scenarios /
88 coverage rows / 1873 local links). Scoped rustfmt и diff-check прошли.
Все **7/7 задач** этого refactor change выполнены; новые продуктовые требования
или закрытие старых gaps этим статусом не подразумеваются.

Реальные звук/микрофон/clipboard, аппаратная диаризация, Mac и качество новой LLM
**not-run**. BL-006 не закрыт. Main specs/delta существующих фич не синхронизированы
и не архивированы. Справочник не меняется: US-действия сохранены. Push не выполняется.
