# Полная очередь на публичной длинной речи, 2026-11-01

Это release-замер настоящего координатора и моделей на изолированном архиве D:,
без capture/API приложения и без пользовательских данных. Обработка продукта не
ускорялась изменением prompts/лимитов/порогов. Качество распознавания и содержание
summary не объявляются принятыми по факту готового файла.

## Материал и условия

[Podlodka #109](https://podlodka.io/109), публичный разговор на русском:
7146.527375 s (1 ч 59 мин). [Источник, RSS, checksums и manifests](source-podlodka-109.json).
MP3 142934642 bytes получен из опубликованного RSS; ffmpeg mono PCM16 16 kHz,
без склейки/повтора. Аудио и обычные текстовые артефакты остаются untracked в
`target/public-profile-source-20261101` и `target/profile-queue-*` на D:.
Это реальная длинная речь, но не представительная выборка встреч владельца.

Windows x86_64, AMD Ryzen 9 9900X3D; процессу/CIM доступно 6 логических CPU,
physical RAM reported 66157793280 bytes (~61.6 GiB). RTX 5080 16303 MiB.
Ollama 0.40.13, `qwen3.5:9b`, digest `6488c96fa5faab64bb65cbd30d4289e20e6130ef535a93ef9a49f42eda893ea7`.
GigaAM v3, NER GLiNER fp16; hashes файлов в JSON. Diarization/refine/cleanup off;
summary сравнивается off/on. Input PCM уже подготовлен с проверенным receipt:
сеть и конверсия исключены. Coordinator interval 5 s, настоящие resource lanes.

Источник/модели предварительно прочитаны для hashes: **не cold disk cache**.
Перед summary-пачкой `/api/ps` пуст: модель не resident; первый ответ содержит
load=2.509 s. Второй summary-проход использует resident модель (первый запрос
load=0.00268 s), API snapshots сохраняют digest/context 32768. Это подтверждённые
условия residency, без принудительного unload и без утверждения об эксклюзивном
владении внешним сервером. Фоновая нагрузка desktop/ОС не контролировалась.

## Результаты

Два fresh-session repeats одной полной записи, все jobs Done, receipts проверены,
events_lost=0, worker/queue-runner hashes до/после совпали.

| Маршрут | STT r1 / r2, s | Summary r1 / r2, s | Batch wall, s | Owned tree sampled RAM peak |
|---|---:|---:|---:|---:|
| Только STT | 72.143 / 72.261 | не запрошена | 149.803 | 581.8 MiB |
| STT + summary | 71.853 / 101.064 | 149.324 / 134.430 | 369.350 | 2046.7 MiB |

Суммарно в каждой пачке 14293.055 s аудио. Во втором STT видна конкуренция с
параллельной summary: она не скрывается из времени. Фазы разных сессий перекрыты,
их сумму нельзя выдавать за batch wall. Summary остаются `content_unverified=true`
при ready артефактах. WER/полнота фактов/редакторское качество здесь не оценивались.

| Сохранённая граница / компонент | r1 | r2 |
|---|---:|---:|
| Очередь STT без summary, s | 4.758 | 76.645 |
| Очередь STT с summary, s | 4.749 | 76.337 |
| Очередь summary, s | 3.368 | 56.641 |
| ASR load/adapter без summary, s | 0.574 | 1.052 |
| ASR load/adapter с summary, s | 0.603 | 1.033 |
| NER model load, s | 1.403 | 2.066 |
| Все grounding operations, s | 72.608 | 63.186 |
| Первая grounding operation, s | 66.745 | 57.477 |
| Остальные grounding operations вместе, s | 5.863 | 5.709 |
| HTTP requests / HTTP wall, s | 12 / 70.374 | 13 / 64.413 |
| Ollama model load / prompt / generation, s | 2.533 / 7.347 / 59.745 | 0.030 / 1.166 / 61.961 |
| Phase wall минус HTTP wall, s | 78.950 | 70.017 |

Точные значения ожидания — в [исходных clocks](queue-podcast-stt-2026-11-01.json);
таблица округляется до миллисекунд. Model load/grounding — вложенные компоненты,
а не добавка к phase wall. Первая проверка включает source NER, lexical preparation
и matching; это **не доказательство**, что все 57–67 s потрачены только NER.
SourceEntities уже повторно использует извлечение исходных сущностей внутри документа.

HTTP однозначно отнесены к Summary по сохранённым started/ended границам:
локальная LLM lane последовательна. Все 25 запросов завершились `done_reason=stop`;
это не повтор прежнего synthetic-прогона с ответами ровно 8192 tokens.
Reduce занял 28.296/27.135 s, включая генерацию; он вложен в HTTP/phase wall.

## Ресурсы и границы измерителя

Owned tree: snapshot каждые 100 ms, одновременная сумма текущей памяти, retained
process CPU counters; короткий потомок может быть пропущен. Пик отдельных процессов
не суммируется в пик дерева. Полная summary-пачка: observed CPU 131.453 s,
sampled private commit peak 2555256832 bytes, RAM peak 2146164736 bytes.

[Внешние ресурсы](queue-external-resources-2026-11-01.json): global GPU peak 9334 MiB,
API `size_vram` 6618370538 bytes — allocation модели, не её utilization. WDDM не
предоставил per-process GPU memory: это unknown, не ноль. GPU включает desktop.

Первый внешний monitor v1 учитывал `ollama.exe`, **пропустил llama-server.exe**.
Его оригинальная область/числа оставлены неизменными. В отдельном последующем
lifetime query backend (создан внутри полного прогона) зафиксированы OS peak RAM
5197979648 bytes и peak paged memory 14388908032 bytes; окно включает idle после
batch и не является одновременным server+backend peak. Это отдельно от v1 samples.
Исправленный [monitor v2](profile-external.ps1) проходит по потомкам listener PID:
новый warm smoke поймал backend и 5344.5 MiB sampled external RAM. Его ресурсные
данные не подставляются задним числом в полный прогон.

## Воспроизводимость и проверка инструмента

Команды/ограничения — [руководство](../../../../docs/pipeline-benchmark.md).
Для полного benchmark использован release profiler `6d66dbf9…`, queue runner
`a3b5a9b8…`, worker `4edd4dcd…`; полные hashes, configuration и clocks —
[STT](queue-podcast-stt-2026-11-01.json), [summary](queue-podcast-summary-2026-11-01.json).
После измерений в profiler уточнены configuration отметка режима ASR timing и
`complete` при исчезнувшем запланированном job; затем выполнен отдельный короткий
release smoke. Worker/coordinator и успешно измеренные переходы неизменны.

- Model-free profiler: 21 passed, 1 ignored fixture; queue guards: 4 passed,
  1 ignored runtime; runtime отдельно: 1 passed (empty, timeout/stop, terminal
  Failed после трёх попыток, incompatible worker).
- Core ONNX cook: 8 passed; root lib 14 passed, 1 ignored профиль, bin 1 passed.
  Hash/receipt/source retention, mixed summary policy, cancellation/incomplete и
  настоящий root+child resource sampler проверены. Новый fixture не меняет исходник.
- [Пять коротких WAV](queue-golos-2026-11-01.json): release queue smoke, все Done,
  20 сохранённых transitions. [Warm native smoke](queue-external-warm-2026-11-01.json)
  проверяет исправленный внешний monitor; не заменяет длинный benchmark.

Strict main: 15 passed; benchmark change и следующий proposal: valid;
check-openspec: 15 capabilities, 86 requirements, 168 scenarios, 86 coverage rows;
rustfmt затронутых файлов, PowerShell parser и diff: passed.
Графический интерфейс/реальный звук в этом замере не запускались.

## Численный reference-бюджет и следующий шаг

Предлагаемый **regression budget только для этого fixture/комплекта/машины**:
STT ≤120 s на запись, summary ≤180 s на запись, две записи с summary ≤420 s
batch wall; owned sampled RAM ≤3 GiB. Это ориентир следующего native прогона,
не новый timeout, CI gate, P95 или обещание «любая встреча за пять минут».
Не переносить его на другой CPU/GPU, модели, diarization/cleanup/refine или сеть.
Прежний repeated synthetic case со summary 306.6 s — иной stress input, он не
скрывается и не объявляется соответствующим reference-бюджету.

Подтверждённый следующий участок — первая grounding-проверка; новый
[proposal](../../profile-summary-grounding-cost/proposal.md) разделяет её стоимость
до выбора оптимизации. Общая latency не исправляется бездумным снижением 8192 cap:
для этого нужна отдельная оценка полноты/качества. BL-003 закрывается как замер и
предложение бюджета, не как гарантия качества или внедрённая оптимизация.
