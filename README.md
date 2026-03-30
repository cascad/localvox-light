# localvox-light

Локальная транскрипция: микрофон и/или loopback → сегментация (VAD) → WAV на диск → Vosk ASR → `transcript.jsonl`. Сервер и сеть не нужны; очередь на диске переживает падение процесса.

## Требования

- Rust toolchain (edition 2021).
- Каталог с моделью Vosk; по умолчанию **`models/vosk-model-ru-0.42`** (так кладёт `setup-vosk`). Официальный каталог моделей и описания: **[alphacephei.com/vosk/models](https://alphacephei.com/vosk/models)**. Скрипты `setup-vosk.*` по умолчанию скачивают русскую модель с Hugging Face (зеркало того же архива `vosk-model-ru-0.42.zip`); другой URL можно задать через **`LOCALVOX_SETUP_MODEL_URL`**. Переопределение пути к уже распакованной модели: `--model` / `LOCALVOX_LIGHT_MODEL`.
- Для **разработки** (сборка из исходников): нативная библиотека Vosk в **`vosk-lib/`** в корне репозитория — `build.rs` добавляет `rustc-link-search`.

## Как выпустить GitHub Release

После пуша в `main` workflow **Build binaries** собирает артефакты. Чтобы появился **Release** с пятью бинарниками:

1. **Через тег** (обычный способ): на нужном коммите в `main`:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```
   Тег должен совпадать с форматом `v*` (например `v0.1.0`). После успешной сборки всех платформ job **Publish GitHub Release** создаст релиз с файлами вроде `localvox-light-x86_64-pc-windows-msvc.exe`.

2. **Вручную в Actions**: откройте **Actions → Build binaries → Run workflow**, включите **publish_release**, укажите **release_tag** (например `v0.1.0`) и запустите. Тег создастся на текущем `main`, если его ещё нет.

Имеет смысл держать версию в `Cargo.toml` и имя тега согласованными.

## Установка без сборки (готовый бинарник + Vosk + модель)

Скрипты **`install-release.ps1`** / **`install-release.sh`** качают бинарник с **последнего GitHub Release** (и `setup-vosk` с raw). Пока релиза нет, сначала выполните шаг выше. Дополнительно артефакты по-прежнему лежат во вкладке **Actions** у каждого прогона.

**Windows (PowerShell)** — одной строкой (по умолчанию каталог **`.\localvox-light`** относительно текущей папки; внутри создаётся **`.env`** с путём к модели и подкаталог **`vosk-lib/`**; бинарник подхватывает это сам). Релиз **`latest`**:

```powershell
$u='https://raw.githubusercontent.com/cascad/localvox-light/main/scripts/install-release.ps1'; $p="$env:TEMP\lv-install.ps1"; Invoke-WebRequest -Uri $u -OutFile $p; & $p
```

Параметры **`-Tag`**, **`-InstallDir`** и остальные из шапки скрипта передаются **только** вызову `& $p …`, не `Invoke-WebRequest` (у `iwr`/`Invoke-WebRequest` нет `-Tag`).

Конкретный релиз, одной строкой:

```powershell
$u='https://raw.githubusercontent.com/cascad/localvox-light/main/scripts/install-release.ps1'; $p="$env:TEMP\lv-install.ps1"; Invoke-WebRequest -Uri $u -OutFile $p; & $p -Tag v0.1.1
```

Другой каталог: добавьте к `& $p`, например `& $p -InstallDir D:\apps\lv -Tag v0.1.0`.

**Linux / macOS:** `curl`, `unzip`, **`jq`**. Одна строка:

```bash
curl -fsSL https://raw.githubusercontent.com/cascad/localvox-light/main/scripts/install-release.sh | bash
```

Свой каталог / тег: `bash install-release.sh --install-dir=~/lv --tag=v0.1.0` (скрипт можно сначала скачать). Флаги: `--skip-vosk`, `--skip-binary`, `--repo=`, `--branch=`.

## Разработка: модель + `vosk-lib` в репозитории

**curl** и **unzip** (Linux/macOS), **PowerShell 5+** (Windows). `setup-vosk.*` кладёт **`vosk-lib/`** и **`models/`** в **корень клонированного репозитория** (каталоги в `.gitignore`). Запуск из клона: `./scripts/setup-vosk.sh` / `.\scripts\setup-vosk.ps1`. Для произвольной папки (как в `install-release`): в PowerShell **`-InstallRoot`**, в bash **`--install-root=/path`**.

```bash
# Linux / macOS — автоопределение ОС и архитектуры
chmod +x scripts/setup-vosk.sh
./scripts/setup-vosk.sh

# Явный пресет (если uname не подошёл)
./scripts/setup-vosk-linux-amd64.sh
./scripts/setup-vosk-linux-arm64.sh
./scripts/setup-vosk-mac.sh
```

```powershell
# Windows
.\scripts\setup-vosk.ps1
```

Флаги: **`--skip-model`** / **`-SkipModel`** — только нативная библиотека; **`--force`** / **`-Force`** — перекачать модель. Переменные: **`LOCALVOX_VOSK_API_TAG`** (по умолчанию `v0.3.42`), **`LOCALVOX_SETUP_MODEL_URL`**, **`LOCALVOX_SETUP_FORCE=1`**. В `setup-vosk.sh` можно задать архитектуру вручную: **`--preset=linux-x86_64`**, **`linux-aarch64`**, **`linux-x86`**, **`darwin`**.

После установки:

- В **Windows** нативная `libvosk.dll` подхватывается загрузчиком **до** `main()`: она должна лежать **в той же папке, что и `localvox-light.exe`** (или в системном `PATH` до запуска). `build.rs` копирует `*.dll` из `vosk-lib` в `target/release/`. Скрипт **`install-release.ps1`** дублирует `vosk-lib\*.dll` рядом с exe. Если копируете только exe в другую папку — скопируйте и все `*.dll` из `vosk-lib` (или из `target\release\`) в каталог с exe. Дополнительно приложение добавляет `vosk-lib` в `PATH` для прочих нативных зависимостей. Путь к модели — **`.env`** рядом с exe.
- В **Linux** при необходимости: `export LD_LIBRARY_PATH="/path/to/repo/vosk-lib:$LD_LIBRARY_PATH"`.
- В **macOS** при необходимости: `export DYLD_LIBRARY_PATH="/path/to/repo/vosk-lib:$DYLD_LIBRARY_PATH"`.

Путь к модели в `.env` нужен только если не подходит дефолт `models/vosk-model-ru-0.42` (запуск из корня репозитория).

По умолчанию нативная часть с тега **v0.3.42** (win64 / win32 / osx / linux x86_64 и др.). При 404 на другом теге задайте **`LOCALVOX_VOSK_API_TAG`** и проверьте наличие zip в Assets релиза.

## Quickstart

1. Скопировать `.env.example` в `.env` при необходимости; после `setup-vosk` модель уже в `models/vosk-model-ru-0.42` (дефолт бинарника).
2. Сборка: `cargo build --release`
3. Запуск из корня репозитория (workspace): **`cargo run`** или **`cargo run -p localvox-light`** — это бинарник приложения. Пакет **`localvox-light-tui`** — только библиотека для TUI, у неё нет `bin`, поэтому **`cargo run -p localvox-light-tui`** выдаст ошибку.
4. Запуск с TUI: в **интерактивном** терминале достаточно `./target/release/localvox-light` (или `cargo run -p localvox-light`) — TUI включается сам, если в окружении **не** задан `LOCALVOX_LIGHT_TUI`. Явно: `--tui` или `LOCALVOX_LIGHT_TUI=1`.

5. Только логи в stderr, без TUI: `--no-tui` или `LOCALVOX_LIGHT_NO_TUI=1` (в т.ч. когда в `.env` не нужен авто-TUI).

Список устройств: `--list-devices`. Устройства можно задать флагами, `.env` или `localvox-light-config.json` в cwd (см. `.env.example`).

## Как устроен пайплайн (алгоритмически)

1. **Рабочий каталог** — `--audio-dir` / `LOCALVOX_LIGHT_AUDIO_DIR` (по умолчанию `localvox-audio`). В **корне** лежат `transcript.jsonl` и сегменты `src*_*.wav`; вложенные `session_*` больше не создаются. Каждый запуск продолжает тот же каталог (recovery необработанных WAV, нумерация сегментов с максимума на диске). Обнулить текст транскрипта: `[x]` в TUI; полностью очистить данные — вручную удалить файлы или каталог. Старые данные в `localvox-audio/session_*/` приложение не подхватывает — при необходимости перенесите файлы в корень рабочего каталога.
2. **Захват** — отдельные потоки: микрофон (`source_id = 0`), опционально loopback (`1`). PCM 16 kHz mono уходит в общий bounded-канал в **pipeline**.
3. **Сегментация (webrtc-vad)** — для каждого источника свой VAD (кадры 20 ms). Накапливается буфер; при длительной тишине (`vad_silence_sec`) или при достижении `max_chunk_sec` текущий фрагмент **финализируется**: `.part` переименовывается в `.wav`, путь отправляется в канал **ASR**. Минимальная длительность сегмента до разреза по тишине — `min_chunk_sec`. Имена: `src0_000001.wav`, `src1_000001.wav`, …
4. **Пауза записи** — флаг `record_pcm`: при выключении незавершённый сегмент сбрасывается (`.part` удаляется), VAD-буфер очищается.
5. **Восстановление** — при старте все `.wav` в рабочем каталоге без строки в `transcript.jsonl` снова ставятся в очередь ASR. Сиротские `.part` с прошлого запуска удаляются.
6. **ASR** — пул из `asr_workers` потоков читает пути из очереди, для каждого WAV: чтение f32 → опционально **noise gate** (доля «речи» по VAD; при низкой — обрезка к речи или отбрасывание) → `VoskEngine::transcribe` → запись строки в `transcript.jsonl` → удаление обработанного `.wav`.
7. **Остановка** — Ctrl+C; pipeline дренирует PCM, сбрасывает хвосты в WAV, воркеры добирают очередь.

Параллельно с записью в отдельном потоке грузится модель Vosk; пока модель не готова, WAV копятся на диске.

## TUI (`--tui` / `LOCALVOX_LIGHT_TUI=1`)

Поток **движка** в фоне, основной поток — полноэкранный интерфейс (ratatui).

**Панели**

- **Статус** — индикатор записи REC/STOP (`r`), loopback вкл/выкл из текущего конфига, уровни mic/sys, текстовый статус (путь к данным, загрузка модели и т.д.), счётчики очереди: число необработанных WAV, их суммарный размер (МБ), размер всех файлов в рабочем каталоге (МБ).
- **Транскрипт** — строки с временем, префиксом `mic` / `sys` и текстом; прокрутка с учётом переносов; режим «липнуть к низу» при новых строках.
- **Debug** — структурированные сообщения этапов (`segment`, `gate`, `asr`, `load`, …). Подробные строки только с `--verbose` / `LOCALVOX_LIGHT_VERBOSE`; без этого в панели в основном ошибки и экспорт.

**Горячие клавиши (основной режим)**

| Клавиша | Действие |
|--------|----------|
| `q`, `Esc`, `Ctrl+C` | Выход (остановка движка) |
| `r` | Пауза/возобновление записи в WAV (очередь ASR не очищается) |
| `x` | Сброс UI-транскрипта и **обрезка** `transcript.jsonl` с начала (новый «лист» в файле); необработанные WAV остаются в очереди |
| `e` | Экспорт отсортированного дампа в `--transcript-dump-dir` (`transcript_dump_*.jsonl`); каталог не должен быть пустым в конфиге |
| `F2` | Экран настроек устройств |
| `Tab` | Фокус: транскрипт ↔ Debug |
| `↑` `↓` / `k` `j`, PgUp/PgDn, Home/End, колёсико мыши | Прокрутка активной панели |

**F2 — устройства**

Два списка: микрофон и loopback (первая строка loopback — «нет»). `Tab` — смена списка, `S` — сохранить в `localvox-light-config.json` (путь как при сохранении в проекте), `Esc` — назад. После сохранения захват **переключается сразу**, без перезапуска.

При старте TUI подгружает историю из существующего `transcript.jsonl` в рабочем каталоге.

**Логи в терминале**

В режиме TUI без `--debug` в stderr уходит минимум (`RUST_LOG` по умолчанию для TUI — в основном ошибки). Подробный stderr — `--debug` или свой `RUST_LOG`.
