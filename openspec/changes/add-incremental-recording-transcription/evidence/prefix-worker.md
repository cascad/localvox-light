# R1 · Worker и финализация, 2026-11-10

Реализован конечный запуск `localvox-process --worker-prefix`: один снимок готовых
окон, только suffix после сохранённого checkpoint, lazy ASR, отдельный cache.
Нет записи в versions/best/processing и нет запуска LLM/индекса. Это внутренний
контракт, пока не подключённый к автоматической очереди или UI/TUI (R2/R3).

Код: [prefix](../../../../crates/localvox-light-core/src/cook/prefix.rs),
[bounded assembler](../../../../crates/localvox-light-core/src/incremental/audio.rs),
[model identity](../../../../crates/localvox-light-core/src/cook/model.rs),
[final cook](../../../../crates/localvox-light-core/src/cook.rs),
[worker](../../../../crates/localvox-light-asr/src/bin/localvox-process.rs).

Assembler собирает одно planned окно из последовательных sealed ranges; каждый
нужный контейнер читается/декодируется один раз за horizon под общим chunk lease.
Размер PCM ограничен речевым окном, decoder буфер — 64 KiB, FLAC decode — supervised
temporary file на диске аудио. Проверяются count, conversion receipt и SHA окна.
Предыдущие закрытые окна не перечитываются при очередном prefix запуске.

При Stop обычный cook проверяет terminal input и один раз воспроизводит общий
VAD по полному PCM. Точные совпавшие descriptors/PCM/model/recipe принимаются из
того же cache, EOF tail распознаётся; diarization получает весь поток в прежний
финальный путь. Полная версия, best и transcript receipt публикуются штатно.
`--force` оставляет смысл свежего распознавания; legacy без capture ledger работает.

## Recovery / rollback

При обрыве до checkpoint сохранённый result не объявляет prefix готовым: следующий
worker заново доказывает PCM и принимает result. Битый derived suffix можно
пересчитать из первичного аудио; cache не заменяет seals. После transcript file,
но до manifest, повтор использует cache. После manifest повтор проверяет JSONL,
восстанавливает best и receipt без нового ASR. После receipt повтор идемпотентен.
Читатели финального результата по-прежнему проверяют artifact receipt; частичный
снимок остаётся отдельным namespace. Не удалять capture ledger ради отката:
отключение раннего режима/обычный Stop сохраняют full-cook fallback.

## Выполнено

- `cargo test --locked --offline -j2 -p localvox-light-asr --features asr-fixtures --test incremental_fixture -- --nocapture`:
  **3 passed**. Настоящие дочерние worker, capture recorder/ledger/native VAD,
  synthetic ASR с явным test-only feature/CLI. Crash процесса после первого result,
  checkpoint, transcript file, manifest, best и receipt; повтор не дублирует ASR,
  строки или версии, проверяет source/parents/times/receipt. Пустой ASR явно завершает
  покрытие; force и независимый legacy archive работают. Физическая потеря питания
  не эмулируется. Обычная сборка не содержит fixture CLI/engine.
- `cargo test --locked --offline -j2 -p localvox-light-core --features onnx --lib`:
  **444 passed, 8 ignored**. Включая прежние cook/windowing/receipts и новые assembler
  seam/suffix/count/SHA/missing/cancel tests. Отдельно real FFmpeg assembler — **1 passed**.
- `cargo check --locked --offline -j2 -p localvox-light-asr --features asr-fixtures` passed.
- Native worker test [incremental_worker.rs](../../../../crates/localvox-light-asr/tests/incremental_worker.rs):
  **1 passed**, GigaAM `v3_e2e_ctc.int8.onnx`, публичный PCM от 600 до 750 s,
  temporary sessions, две порции 90+60 s и отдельная дорожка настоящей тишины.
  Первая порция — 9 ASR calls, вторая — 6, Stop — **1**, финал — **33 cache hits**.
  Свежий offline — **16 calls**; 16 строк совпали по text/source/start/end точно.
  Повтор неизменного prefix: 0 calls/0 chunks/0 windows, модель не загружалась.
  Debug elapsed всей проверки 31.28 s; не release benchmark. Воспроизводится с
  `LOCALVOX_TEST_PUBLIC_PCM`, `LOCALVOX_TEST_ASR_MODEL` и `--test incremental_worker -- --ignored`.
  Receipt этого запуска — ignored `target/prefix-native-smoke-20261110.json`.

Native smoke сначала выявил допущение codec о 44-byte WAV header у публичного
ffmpeg WAV с metadata. Исправлено чтение фактического RIFF data offset через hound;
отдельный test проверяет JUNK/LIST и отказ truncated PCM. Неуспешный первый запуск
не считается успешным. Повтор native smoke прошёл после исправления.

Нативная diarization, двухчасовой performance/reference gate, coordinator/API/UI/TUI,
микрофон/loopback/Mac **этим этапом не проверены**. Действующий демон и пользовательский
архив не менялись. Новые PIPE-015/016 — delta, не sync в принятые main specs.
