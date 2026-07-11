# localvox-light-youtube — быстрый старт

Бинарник называется **`localvox-youtube`** (пакет `localvox-light-youtube`).

## Сборка и запуск из репозитория

```bash
cargo build -p localvox-light-youtube
cargo run -p localvox-light-youtube -- "https://www.youtube.com/watch?v=VIDEO_ID"
```

Готовый exe (после `cargo build`): `target/debug/localvox-youtube` (на Windows — `target\debug\localvox-youtube.exe`).

## Что нужно снаружи

- **ffmpeg** (в `PATH` или явный путь) — для любого режима.
- **yt-dlp** — только если вы передаёте **URL** (для `--file` / `-f` не нужен).
- **Модель Vosk** (каталог с `am/`, `conf/`, `graph/`).

## Локальный файл вместо YouTube

Один или несколько путей через **`--file`** / **`-f`** (mp4, mkv, wav, mp3 и т.д. — всё, что открывает ffmpeg). Порядок обработки: сначала все URL по очереди, затем все `--file`.

```bash
cargo run -p localvox-light-youtube -- --file ./video.mp4
cargo run -p localvox-light-youtube -- -f a.mp4 -f b.wav "https://www.youtube.com/watch?v=VIDEO_ID"
```

При запуске **только** с `--file` проверка `yt-dlp` не выполняется.

## Переменные окружения (часто задают в `.env`)

Приоритет: **аргументы CLI → переменные окружения →** `localvox-youtube-settings.json` / `settings.json` в текущем каталоге или рядом с exe → поиск `yt-dlp` / `ffmpeg` в cwd и каталоге exe.

| Переменная | Зачем | Если не задана |
|------------|--------|----------------|
| `LOCALVOX_LIGHT_YT_DLP` | Путь к `yt-dlp` / `yt-dlp.exe` | Команда `yt-dlp` в `PATH`; иначе файл `yt-dlp` / `yt-dlp.exe` в **cwd** или **рядом с exe** |
| `LOCALVOX_LIGHT_YT_FFMPEG` | Путь к `ffmpeg` или к **каталогу** `bin` с `ffmpeg.exe` | Команда `ffmpeg` в `PATH`; иначе `ffmpeg` / `ffmpeg.exe` в cwd или рядом с exe |
| `LOCALVOX_LIGHT_YT_JS_RUNTIME` | Среда для yt-dlp: `node`, `deno` или строка вида `node:C:/path/node.exe` | По сути **`node`** (см. резолвер в `tools.rs`) |
| `LOCALVOX_LIGHT_YT_JS_RUNTIME_PATH` | Явный путь к Node (если не в `PATH`) | Не обязательна; для автономного `yt-dlp.exe` часто не нужна |
| `LOCALVOX_LIGHT_YOUTUBE_OUTPUT_DIR` | Базовый каталог вывода | **Один источник** (один URL или один `--file`) → `transcript.txt` в **корне cwd**; **несколько** → каталог **`youtube-transcripts/`** |
| `LOCALVOX_LIGHT_MODEL` | Каталог модели Vosk | **`models/vosk-model-ru-0.42`** (относительно cwd при запуске) |

Пример фрагмента `.env` (пути подставьте свои; на Windows в `.env` удобнее слэши `/`):

```env
LOCALVOX_LIGHT_YT_DLP=bin/yt-dlp.exe
LOCALVOX_LIGHT_YT_FFMPEG=F:/ffmpeg7.1.1/bin/ffmpeg.exe
LOCALVOX_LIGHT_YT_JS_RUNTIME=node
LOCALVOX_LIGHT_YT_JS_RUNTIME_PATH=F:/nodejs/node.exe
LOCALVOX_LIGHT_YOUTUBE_OUTPUT_DIR=transcripts
# LOCALVOX_LIGHT_MODEL=models/vosk-model-ru-0.42
```

После этого из каталога, где подхватывается `.env` (корень репозитория при `cargo run`):

```bash
./target/debug/localvox-youtube "https://www.youtube.com/watch?v=052H7azDgw8"
```

На Windows из корня репозитория, например:

```powershell
.\target\debug\localvox-youtube.exe "https://www.youtube.com/watch?v=052H7azDgw8"
```

С `LOCALVOX_LIGHT_YOUTUBE_OUTPUT_DIR=transcripts` один URL пишется в **`transcripts/transcript.txt`**.

## Полезные флаги

- `-v` / `--verbose` — stderr yt-dlp и ffmpeg.
- `--debug` — логи tracing; индикаторы прогресса отключаются.

Полный список: `localvox-youtube --help`.
