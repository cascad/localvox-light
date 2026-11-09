# Свежая сборка и полный маршрут готового аудио

2026-11-09. Собран согласованный release комплект из `4b2e059`; программная
regression и оба native маршрута завершены успешно. [Численные отчёты, build/model
fingerprints и hashes логов](native-results.json). Capture/аппаратная и смысловая
приёмка не выполнялись. Пользовательский daemon не запускался.

## Сборка и regression

UI: `npm.cmd --prefix ui run build`, `npm.cmd --prefix ui test`,
`npm.cmd --prefix ui run test:e2e -- --workers=1` — build PASS, **10 unit / 76 e2e**.
Три отрицательных visual controls ожидаемо отвергли изменённый symbol/color/geometry;
это не неожиданные отказы. Snapshots не обновлялись. Hashes всех dist файлов сохранены.

Locked/offline release с jobs=2 собрал вместе `localvox-light`, `localvox-process`,
`localvox-tui`, `localvox-desktop`, `localvox-profile`, `localvox-queue-profile`.
Native build PASS, 3m27s. Рабочий worker теперь пересобран с последними guards и
отбором role окон; все шесть executable SHA/размеры и runtime DLL — в JSON.
Это не portable installer и не проверка запуска WebView/аудиоустройств.
TEMP/TMP и новые тестовые данные находятся на D:.

| Проверка | Passed | Ignored в этой invocation |
|---|---:|---:|
| Client, terminal, ConPTY/process tests | 154 | 4 |
| Core с ONNX | 390 | 3 |
| LLM | 108 | 0 |
| Coordinator + launcher | 15 | 9 |
| Phase/queue profiler contracts | 35 | 6 |
| Explicit release queue fault fixture | 1 | 0 |
| Оба клиента против настоящего временного HTTP/API | 1 | 0 |
| **Всего Rust passed** | **704** | — |

Отдельный release fixture реально выполнил empty/timeout/failed/cancelled/
incompatible worker сценарии с новыми sibling runner/worker, без моделей/устройств.
Ignored не засчитаны за выполненные; один и тот же fixture в обычном списке был
ignored, затем запущен явно. Model-free cancel/preflight не подменяет исходное
аппаратное зависание владельца. Известные core warnings cpal/unused_mut сохранены.

Strict main specs: 15 passed, 0 failed; strict выбранного change valid при
skip_specs/zero deltas. `python scripts/check-openspec.py`: PASS — 15 capabilities,
88 requirements, 188 scenarios, 88 coverage rows, 1543 local links.
`git diff --check` passed; выбранный change добавлен в существующую CI validation.

## Настоящие модели и условия

Прежний публичный [Podlodka #109 reference](../../benchmark-processing-batch/evidence/source-podlodka-109.json):
7146.527375 s (1 ч 59 мин), mono PCM16/16 kHz, 228688954 bytes. SHA
`96e50ce45af3988b3b069c0ebd4363842eb06b3fe5b536490a8dc9d81f02cd46`.
Аудио уже подготовлено; сеть/скачивание/конвертация не входили в замер.

GigaAM v3 e2e CTC INT8 и прежний GLiNER FP16. Настроенные Ollama `0.40.14`,
`qwen3.5:9b`, digest `6488c96fa5faab64bb65cbd30d4289e20e6130ef535a93ef9a49f42eda893ea7`;
настройки модели/endpoint не переключались. Direct loopback connection,
без cloud fallback. Refine/cleanup/diarization off, как в прежнем reference.
Язык ru, glossary пустой, templates/thresholds/token caps не менялись.

Использованы существующие profiler/production coordinator и настоящие worker
phases: marker/lock, отдельный новый архив на каждый маршрут, ownership/verification
и обычные resource lanes. По одному fresh session repeat, последовательно,
timeout 900 s на маршрут, внешняя граница существующего process supervisor.
Во время замеров сборки/другие модельные тесты не запускались.

Windows x86_64, Ryzen 9 9900X3D / 6 logical processors reported, RAM 66157793280 bytes.
Post-run hardware snapshot подтвердил RTX 5080 / 16303 MiB; это не замер GPU peak.
Файлы предварительно читались для
hashes: не cold disk cache. Ollama `/api/ps` перед обоими маршрутами пуст; после
summary resident только выбранный Qwen с тем же digest. Выгрузка моделей не
запрашивалась, эксклюзивное владение GPU/сервисом и отсутствие desktop-нагрузки не заявлены.

## Фазы и результаты

Все времена в секундах. Phase wall — из сохранённых claimed/accepted Job clocks,
не из частоты polling. Подготовка копий/фингерпринты инструмента вне batch wall.

| Граница | STT-only | STT + summary |
|---|---:|---:|
| Prepare: проверка уже подготовленного PCM | 0.279 | 0.256 |
| Transcribe, включая загрузку/сохранение/приёмку | 87.711 | 78.935 |
| Summary, включая проверки/повторы/сохранение | не запрошена | 122.652 |
| Queue wait prepare / transcribe / summary | 0.169 / 4.768 / — | 0.168 / 4.759 / 1.301 |
| **Batch wall настоящего координатора** | **93.220690** | **208.275351** |
| Owned tree sampled RAM peak, MiB | 581.24 | 1485.82 |

У обоих jobs Done, attempts=1 для каждой фазы; all_success/complete=true,
coordinator joined, events_lost=0. Приняты только prepare→transcribe и
prepare→transcribe→summary соответственно: text/refine/cleanup не запускались.
Prepare проверил готовый receipt, а не скачивал/конвертировал аудио.

В обоих случаях transcript ready, **704 реплики**, raw JSONL SHA
`09014305b713638301d3cd4693d03e1f7f2920e75bdfc749c7164d0e10034ae8` — совпадает
с прежним source. Это байтовое совпадение конкретного результата, не оценка WER.
Processed/refined файлов нет, исходный текст доступен без LLM-очистителей.

STT-only: summary missing по запрошенной политике, а не failed. STT+summary:
summary ready с SHA `bd179b72659e9e981e1fcb4643e8eff551bf2f301d7e8639f856e6bae17046e1`,
**content_unverified=true**. Очередь принимает целостность файла отдельно от
сомнений в содержании. Ready не объявляется подтверждённым качеством summary.
Production verify_phase проверил receipts на переходах, включая canonical input
сводки; Python raw SHA не выдаётся за canonical transcript receipt hash.

## Operation clocks и пределы телеметрии

Journal history available, malformed_records=0, incomplete_tail=false.
Stage UI clocks согласованы с границами worker: transcribe 87.683/78.832 s,
summary 122.418 s. Небольшая разница с queue phase wall включает commit/verification.

- ASR model/adapter operation: 0.793815 / 0.587190 s, вложена в transcribe.
- NER load: 1.036498 s, вложена в summary.
- 11 grounding operations: 49.692144 s вместе; первая 45.228526 s, остальные
  4.463619 s. Это отдельные ответы внутри генерации, не replay финального markdown.
- 9 llm_request operations, 2 llm_retry operations, 1 summary_reduce operation
  присутствуют. Их elapsed записаны в JSON, это application wall, не native token/
  generation counters. Generic parent operations также вложены: суммы категорий
  нельзя добавлять к wall или принимать за полное разложение стоимости.

Старый --trace-ollama не включался: proxy URL `/:11434/v1` обходит detect_flavor
через путь адреса. Такой обход здесь не нужен. Прямой транспорт сохранён, HTTP/
token/load/prompt/generation native counters остаются null. Структурное исправление
provider/telemetry — отдельная работа, скрытая подстановка URI не добавлялась.

Owned tree RSS — одновременная сумма sampled coordinator/descendants с шагом
100 ms; partial_memory_samples=0. Короткий потомок может быть пропущен, сумма
индивидуальных peaks не используется. **Внешняя Ollama не входит в этот RAM peak**.
Её after-run API size_vram=6618370538 bytes — snapshot allocation, не peak/utilization
и не RSS всего приложения. GPU peak/полный внешний CPU/RAM здесь не измерены.

Не сравнивать напрямую batch 208 s с прежним batch 369 s на двух сессиях: состав,
параллелизм, runtime version и условия отличаются. Новый прогон подтверждает
работоспособность свежего маршрута, не причинный процент ускорения всего pipeline.

## Сохранность и оставшаяся приёмка

SHA source/audio/model/binaries/dist/Cargo.lock/npm lock до/после совпали; digest
Qwen сохранился. Fingerprints пользовательской .env хранятся только в untracked
target; в Git лишь результат проверки неизменности. Тексты/аудио/сырой stderr
не коммитятся, evidence содержит численные данные, static metadata и hashes.

Capture/API пользовательского приложения не запускались, его архив не читался.
Новые release binaries готовы для [ручной приёмки UI/TUI](../../../../docs/manual-acceptance.md),
которая остаётся not-run: реальные микрофон/системный звук, слышимость плеера,
исходный mpv/shutdown инцидент, Mac. Native качество refine/cleanup после последнего
guard и смысловая оценка новой summary в этот срез не входили. Пользовательский
справочник не менялся: новых действий/настроек нет.
