# Native ASR · 2026-11-11

**Повторных ASR вызовов не было ни в исходном, ни в новом варианте.** Оптимизация
устраняет повторные загрузки модели, обходы cache и технические записи журнала.
Исходный R4 и новый контроль имеют разные baseline capture paths; их результаты
публикуются раздельно. Default раннего режима остаётся **off**.

Вход: весь публичный PCM **7146.527375 s / 114344438 samples** на дорожку;
источник 1 — actual zero PCM. GigaAM `v3_e2e_ctc.int8.onnx`, русский, recipe r7,
15/8/500; diarization/LLM выключены. SHA PCM/worker/model/vocab проверены до/после.
План и ресурсы — [legacy JSON](native-legacy.json), дополнительный контроль — ниже.

## Исходный legacy-offline comparator

Прежний порядок O1/I1 steady300, O2/I2 burst900, O3/I3 steady300 и **≤20% в каждой
паре и суммарно** сохранены. Все **1608 окон по source/range/kind/PCM SHA**,
**704 строки по source/time/text**, **714 ASR calls** совпали в каждой паре.
Каждая сторона теперь загружает модель **один раз и в одном процессе**. До правки
I загружал её 24/8/24 раза; это отдельный факт, не повторное распознавание.

| Пара | Active wall O / I, s | Producer O / I, s | Добавка | Stop→ready O / I, s | Gate |
|---|---|---|---|---|---|
| 1 · 300 s | 95.666 / 119.540 | 18.771 / 28.550 | +24.96% | 77.433 / 6.797 | fail |
| 2 · 900 s | 110.024 / 137.951 | 17.737 / 29.202 | +25.38% | 92.890 / 14.739 | fail |
| 3 · 300 s | 122.367 / 131.286 | 17.989 / 28.935 | +7.29% | 104.997 / 6.944 | pass |

Суммарно **+18.51%**, но итог **fail**, поскольку первые две пары превысили 20%.
`report.pass=false` не подменён успешным exit. Между сериями заметен разброс
producer/worker wall; значения не являются обещанием скорости для любого железа.

Результаты промежуточных реализаций сохранены в [experiments.json](experiments.json):
resident с прежними отдельными result-файлами дал +34.86%, с framed journal и
пооконными technical log — +23.02%. Это не контрольные A/B замеры чистого вклада
каждой правки: wall и внешняя нагрузка менялись. Exclusive prefix timers показывают
отдельно model/recognize/cache/audio-journal/commit, а не приписывают всю разницу ASR.

Старый R4 +48.12% и его неуспех остаются
[историческим результатом](../../add-incremental-recording-transcription/evidence/native-reference.md).
В его plan receipt recipe получился r7-spk из parent environment, тогда как actual
workers/versions/cache использовали r7 с явным DIARIZE=off. Новый profiler выставляет
это и в parent до fingerprint; прежние артефакты не переписаны.

## Дополнительный current-capture comparator

[План зафиксирован до замера](current-capture-plan.md). В production pipeline
capture/planner используются при обоих режимах; прежний O стенда их не создавал.
Флаг `--current-capture-baseline` даёт обеим сторонам одинаковые capture/planner,
оставляя O без ASR до Stop. Порог 20%, порядок, PCM/weights/recipe не меняются.
Этот результат не заменяет исходный gate для принятия PIPE-018/default.

Short harness 330 s / 1 pair прошёл: окна/text/calls совпали, +4.35%, Stop→ready
1.207 s. Он проверяет стенд, не заменяет целую запись.

[Машинные результаты](native-current-capture.json). Все 1608 окон, 704 строки и
714 ASR calls совпали в каждой паре; по одному process start / model load у O/I.

| Пара | Active wall O / I, s | Producer O / I, s | Добавка | Stop→ready O / I, s | Gate |
|---|---|---|---|---|---|
| 1 · 300 s | 123.634 / 125.034 | 30.108 / 28.858 | +1.13% | 94.755 / 7.192 | pass |
| 2 · 900 s | 120.079 / 121.786 | 30.008 / 29.684 | +1.42% | 91.130 / 17.670 | pass |
| 3 · 300 s | 144.066 / 152.389 | 30.600 / 30.521 | +5.78% | 114.537 / 7.270 | pass |

Суммарная добавка **+2.95%**; дополнительный контроль **pass**.
Это сравнение нынешнего capture с ранним ASR и без него, при неизменном лимите.
Исходный legacy gate остаётся fail; default off и аппаратная приёмка не меняются.

| Comparator / режим | CPU, s (3 runs) | Logical reads worker, GB | Logical writes worker, MB | Max worker high-water, MiB |
|---|---|---|---|---|
| Legacy / O | 106.281 | 2.049 | 1.711 | 331.3 |
| Legacy / I | 97.766 | 16.768 | 37.319 | 335.4 |
| Current capture / O | 104.078 | 3.402 | 5.899 | 331.3 |
| Current capture / I | 101.141 | 16.768 | 37.319 | 333.6 |

## Воспроизведение и ограничения

Новые marked outputs под `target/`, TEMP/TMP на D:, Cargo `--jobs 2`. Во время
серий нет параллельных сборок/другого нашего ASR; caches/службы/личный архив не менялись.

```powershell
.\target\release\localvox-incremental-profile.exe --input target/profile-current-release-20261109-stt/sessions/podlodka-109-r1/audio/src0_chunk000000.wav --model models/gigaam-v3-e2e-ctc --worker target/release/localvox-process.exe --output target/incremental-resident-reference-repeat
# Дополнительный контроль с тем же capture у O/I: новый output + --current-capture-baseline
```

Старый CLI без нового флага сохраняет legacy comparator. Полные WAV/cache/windows/
stderr/per-request reports лежат в ignored `target/incremental-resident-reference-20261111`
и `target/incremental-current-capture-reference-20261111`; ресурсы/таймеры и SHA
сохранены в JSON этого evidence. Active wall исключает ожидание будущего звука;
logical OS I/O не равен физическому чтению, high-water не является суммой процессов.
Hardware microphone/loopback, физический mpv, слабый Mac, diarization и LLM **not-run**.
