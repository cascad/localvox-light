# Зафиксированный native reference, до замера · 2026-11-10

Граница R4/7.2, **не результат**. Обычный комплект release, CPU GigaAM без LLM и
diarization; waveform/player отдельно уже проверены. Нет микрофона/реального времени.

- Весь сохранённый публичный Podlodka 109 PCM из
  `target/profile-current-release-20261109-stt/sessions/podlodka-109-r1/audio/src0_chunk000000.wav`:
  SHA256 `96e50ce45af3988b3b069c0ebd4363842eb06b3fe5b536490a8dc9d81f02cd46`.
  Длина берётся из WAV data, без искусственного дополнения до 7200 s. Источник 1 —
  настоящие нулевые сэмплы той же длины, не отсутствие/потеря пакетов.
- `v3_e2e_ctc.int8.onnx`: `2e3fcb7a7b66030336fd10c2fcfb033bd1dc7e1bf238fe5cfd83b1d0cfc9d28e`;
  vocab: `142de7570b3de5b3035ce111a89c228e80e6085273731d944093ddf24fa539cd`.
  max/min/silence = 15 s / 8 s / 500 ms, 16 kHz PCM16. Новый model/prompt не выбирается.
- Три пары в порядке O1/I1, O2/I2, O3/I3. Transport закрывает 300 s; I1/I3 запускают
  конечную порцию на каждом закрытом чанке, I2 coalesces три чанка (burst/backlog).
  Последний неполный файл закрывается только настоящим Stop, без раннего EOF.
  Каждая сессия отдельная, cache между парами не переносится. OS caches принудительно
  не очищаются; рабочий архив и системные сервисы не меняются. Сборки/посторонний ASR
  не запускаются параллельно измерению.
- Primary gate: **не более 20%** добавленной active wall работы в каждой паре и
  суммарно по трём. Active wall = чтение/подача/сохранение/planning producer + wall
  всех finite workers, включая startup/hash/load/verify/finalization. Ожидание
  будущего звука в сжатой подаче отсутствует; это не wall двухчасовой встречи.
  CPU, logical OS I/O, RAM high-water и Stop→ready публикуются отдельно. CPU ratio
  диагностический; неизвестные counters не заменяются обещанием нулевой цены.
- Сначала короткий стендовый probe harness (не reference). До длинной серии он должен
  подтвердить непрерывный sample coverage, фактические ASR counters и сохранение reports.
  Настоящий full-cook window trace записывается явным engineering env только на стенде,
  с PCM SHA каждого speech/silence окна; сверяется с incremental cache и offline.
  Source/time/text проверяются отдельно. Ошибки/различия сохраняются как неуспех.

Скрипт стенда — `localvox-incremental-profile`. Plan/worker SHA/identity записываются
в новый marked output до первой пары; отчёт сохраняется после каждого запуска.
Порог не меняется после результата. Если бюджет не пройден, default остаётся off,
следующее решение о lifecycle выносится владельцу. Hardware/слабый Mac not-run.
