# R4 · Native reference, 2026-11-10

**Корректность сравнения пройдена; performance gate не пройден.** Предустановка
остаётся `off`. Это кандидат для проверки UI/TUI с явным включением режима,
не завершённое принятие change и не разрешение изменить lifecycle или порог.

[План до замера](native-reference-plan.md), [машинные результаты](native-reference.json),
[стенд](../../../../crates/localvox-light-bench/src/bin/localvox-incremental-profile.rs).
Полные входные/выходные сессии, plan, windows, stderr и reports сохранены в ignored
`target/incremental-native-reference-20261110/`. Worker, публичный PCM и модель
проверены по SHA до и после всей серии. Никаких пользовательских сессий/устройств.

## Результат

Весь PCM: **7146.527375 s**, 114344438 сэмплов на дорожку, вторая — настоящая тишина.
Во всех трёх парах совпали непрерывное покрытие обеих дорожек и **1608 фактических
speech/silence окон по source/start/end/PCM SHA**, **704 строки по source/time/text**
и **714 ASR вызовов**. Это равенство с тем же offline GigaAM, не независимая оценка
качества речи. При Stop в steady режиме остаётся 24 ASR вызова, 1552 cache hits;
в burst — 86 вызовов, 1415 hits. Принятые окна не распознавались повторно.

| Пара | Порция | Active wall O / I, s | Добавка | Stop→ready O / I, s | CPU O / I, s | Worker starts O / I |
|---|---|---|---|---|---|---|
| 1 | 300 s | 85.579 / 132.199 | +54.48% | 84.195 / 6.626 | 37.625 / 39.063 | 1 / 24 |
| 2 | 900 s, burst | 83.815 / 103.818 | +23.86% | 82.430 / 13.348 | 35.188 / 34.078 | 1 / 8 |
| 3 | 300 s | 76.542 / 128.258 | +67.57% | 75.176 / 6.550 | 32.125 / 36.359 | 1 / 24 |

Суммарная добавка active wall — **48.12%** при лимите **20%**. Не прошла ни одна
пара; порог и алгоритм после результата не менялись. CPU приведён отдельно и
не подменяет primary wall gate. Это сжатая подача PCM, не время двухчасовой встречи.

Логические чтения worker: около **0.683 GB** offline, **17.203 GB** steady,
**6.291 GB** burst. Чтения producer: 0.229 / около 0.751 GB. Записи producer:
около 0.458 / 0.463 GB; worker: 0.00057 / 0.0164 GB steady (0.00683 GB burst).
Это OS logical counters, не физическое чтение накопителя. Максимальный наблюдённый
worker high-water working set: offline 330.7–331.2 MiB, incremental 303.0–306.3 MiB;
не сумма одновременно живущих процессов. Producer high-water накопительный, есть в JSON.

Каждый finite worker с речью заново загружает модель: 24 раза steady, 8 burst против
одного offline. Это подтверждённая цена текущего lifecycle; вклад каждого фактора
в wall отдельно не изолирован. Удержание модели между порциями требует отдельного
решения о владении ресурсами/справедливости очереди, а не скрытого обхода gate.

Короткий предварительный probe на 330 s: окна/текст/calls совпали, wall +45.01%,
Stop→ready 1.61 s; `target/incremental-native-probe-20261110`. Он проверял стенд,
не заменяет длинную серию. Ошибки замера не скрыты успешной сборкой.

## Воспроизведение

Из корня с TEMP/TMP на D:, без параллельной сборки/ASR, новый output каждый раз:

```powershell
cargo build --locked --release -j2 -p localvox-light-asr --bin localvox-process
cargo build --locked --release -j2 -p localvox-light-bench --bin localvox-incremental-profile
.\target\release\localvox-incremental-profile.exe --input target/profile-current-release-20261109-stt/sessions/podlodka-109-r1/audio/src0_chunk000000.wav --model models/gigaam-v3-e2e-ctc --worker target/release/localvox-process.exe --output target/incremental-native-reference-repeat
```

Окончание инструмента означает сохранение результатов; `report.json.pass` — исход
gate (в этом запуске `false`). Native diarization, LLM, настоящий длительный capture
и слабый Mac этим прогоном не проверялись. Общий рефакторинг не начат.
