# Схема сервиса `localvox-light` (звездообразная топология)

Плоская звезда: в центре — основной процесс `localvox-light`. Вокруг — конкретные
ресурсы и соседи (база, KV, топики очереди, внешние сервисы, триггеры, worker).
Отдельные CLI-процессы (`localvox-youtube`, `localvox-asr`) нарисованы как
самостоятельные процессы рядом и показано, куда они ходят. Расстояние до центра
≈ сила связи: топики и хранилище — близко, внешние сервисы — дальше.

```mermaid
flowchart LR
    %% ================= ЦЕНТР =================
    SVC(("localvox-light<br/>ОСНОВНОЙ ПРОЦЕСС<br/>realtime + TUI"))

    %% ============ ТРИГГЕРЫ / WORKER (очень близко к центру) ============
    SIG["СИГНАЛ-ТРИГГЕР (не shutdown)<br/>F2 reload / x reset / r pause"]
    WRK["WORKER queue-stats<br/>тикер 1с"]

    %% ============ ОЧЕРЕДЬ: ДВА ТОПИКА (близко) ============
    T1[["ТОПИК 1: pcm<br/>in-proc bounded(1024)"]]
    T2[["ТОПИК 2: segments/WAV<br/>durable disk queue"]]

    %% ============ ХРАНИЛИЩЕ + KV (близко) ============
    DB[("transcript.jsonl<br/>DB / WAL")]
    KV[("config.json<br/>KV")]

    %% ============ ОТДЕЛЬНЫЕ CLI-ПРОЦЕССЫ (дальше) ============
    YTCLI(["localvox-youtube<br/>CLI-процесс"])
    ASRCLI(["localvox-asr<br/>CLI-процесс"])
    OUT[("transcript.txt<br/>выходные файлы")]

    %% ============ ВНЕШНИЕ СЕРВИСЫ (дальше всего) ============
    YTDLP{{"yt-dlp<br/>внешний сервис"}}
    FFMPEG{{"ffmpeg<br/>внешний сервис"}}

    %% ---- связи центра ----
    SIG -->|"F2: PUT config + reload_gen+1 → ПЕРЕСЧЁТ устройств"| SVC
    SVC -->|"спавнит"| WRK
    WRK -->|"GET скан O(n)+чтение O(m), раз в 1с"| DB

    SVC -->|"produce PCM / consume"| T1
    T1 -->|"consume → VAD"| SVC
    SVC -->|"produce WAV (PUT O(1))"| T2
    T2 -->|"consume сегмент (competing consumers asr-0..N)"| SVC

    SVC -->|"PUT append O(1)+fsync"| DB
    SVC -->|"GET history/dedup O(m)"| DB
    SVC -->|"GET старт O(1)"| KV
    SIG -.->|"PUT по F2"| KV

    %% ---- отдельные CLI-процессы и куда они ходят ----
    YTCLI -->|"exec: скачать аудио"| YTDLP
    ASRCLI -->|"exec: скачать аудио"| YTDLP
    YTCLI -->|"exec: транскод → PCM"| FFMPEG
    ASRCLI -->|"exec: транскод → PCM"| FFMPEG
    YTCLI -->|"PUT результат"| OUT
    ASRCLI -->|"PUT результат"| OUT

    classDef center fill:#1f2937,stroke:#f59e0b,stroke-width:3px,color:#fff;
    classDef store fill:#065f46,stroke:#34d399,color:#fff;
    classDef topic fill:#1e3a8a,stroke:#60a5fa,color:#fff;
    classDef ext fill:#7c2d12,stroke:#fb923c,color:#fff;
    classDef proc fill:#374151,stroke:#9ca3af,color:#fff;
    class SVC center;
    class DB,KV,OUT store;
    class T1,T2 topic;
    class YTDLP,FFMPEG ext;
    class YTCLI,ASRCLI,SIG,WRK proc;
```

## Узлы звезды

| Узел | Что это в коде | Обращение / объём |
|------|----------------|-------------------|
| **Центр — `localvox-light`** | `engine::run_engine`, оркестрирует потоки mic/loopback/pipeline/asr-pool/TUI | — |
| **СИГНАЛ-ТРИГГЕР** | `F2` → сохранить устройства + `reload_gen.fetch_add(1)`; `x` → reset транскрипта; `r` → pause записи (`tui.rs:357,414,418`) | пересчёт устройства захвата без рестарта |
| **WORKER queue-stats** | поток-тикер раз в 1с (`engine.rs:51-64`) | `GET` скан каталога **O(n)** + чтение журнала **O(m)** |
| **ТОПИК 1 — pcm** | `crossbeam bounded(1024)`, produce mic+loopback → consume pipeline (`engine.rs:95`) | produce/consume, backpressure |
| **ТОПИК 2 — segments/WAV** | `seg`-канал + WAV на диске как очередь (`engine.rs:96`, `session.rs:94`) | produce/consume, durable, at-least-once |
| **DB — `transcript.jsonl`** | append-only журнал с fsync (`transcript.rs:50-92`) | `PUT` **O(1)**+fsync; `GET` всего файла **O(m)**; рост **O(n)** |
| **KV — `config.json`** | устройства mic/loopback (`light_config.rs`) | `GET` старт **O(1)**; `PUT` по F2 |
| **CLI `localvox-youtube`** | отдельный процесс: URL/файл → транскрипт (`youtube/src/main.rs`) | ходит в yt-dlp, ffmpeg; `PUT` transcript.txt |
| **CLI `localvox-asr`** | отдельный процесс: URL/файл → ONNX GigaAM (`asr/src/main.rs`) | ходит в yt-dlp, ffmpeg; `PUT` transcript.txt |
| **Внешний сервис `yt-dlp`** | `std::process` exec для скачивания | вызов CLI-процессами |
| **Внешний сервис `ffmpeg`** | `std::process` exec, транскод → PCM s16le 16kHz | вызов CLI-процессами |

## Распознанные механизмы и паттерны

- **WAL / durable append-only log** → компонент **DB `transcript.jsonl`**. Каждая строка — самодостаточный JSON, запись с `flush`+`fsync` (на unix), устойчиво к частичной записи (`transcript.rs:50-60`).
- **At-least-once delivery + crash recovery (redelivery)** → компонент **ТОПИК 2 (WAV-очередь)**. Необработанные WAV переотправляются на старте через `recover_unprocessed` (`session.rs:94-135`).
- **Idempotent / dedup consumer** → пара **ТОПИК 2 ↔ DB**. Множество `processed_seg_ids` (seg_id из журнала) не даёт обработать сегмент повторно — эффект, близкий к exactly-once (`transcript.rs:79-92`).
- **Cleaner старых/битых данных** → компонент **ТОПИК 2**. `remove_orphan_part_files` чистит незавершённые `.part` при старте; обработанный WAV удаляется после `PUT` в журнал (`session.rs:11-25`, `engine.rs:545`).
- **Backpressure** → компонент **ТОПИК 1 (pcm)**. Ограниченный `bounded(1024)` тормозит захват, если pipeline не успевает (`engine.rs:95`).
- **Competing consumers / worker pool** → компонент **ТОПИК 2 → asr-0..N**. N воркеров тянут из одного канала сегментов (`engine.rs:441-470`).
- **Hot-reload через generation counter (epoch)** → пара **СИГНАЛ → Центр**. `reload_gen: AtomicU64`: потоки захвата сравнивают поколение и переоткрывают устройство без рестарта (`engine.rs:267,312`).
- **Ticker / polling worker** → компонент **WORKER queue-stats**. Периодический опрос каталога вместо событийных уведомлений (`engine.rs:51-64`).
- **VAD-сегментация + noise gate** → внутри **Центра** перед записью в ТОПИК 2 (`pipeline.rs`, `engine.rs:607`).
- **Shared-core, multi-binary** → **CLI-процессы** переиспользуют `localvox-light-core` (Vosk/ONNX, ingest), общий код, разные точки входа и внешние сервисы.
