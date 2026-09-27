# localvox-light

Локальный голосовой секретарь: запись микрофона и системного звука, расшифровка,
обработка текста, поиск и вопросы к архиву. Окно Tauri и браузер используют один демон.
Есть локальные модели и выбираемые удалённые провайдеры; раздел «Спросить» сейчас
по умолчанию предлагает Claude.

Контракт поведения — [OpenSpec](openspec/README.md), принятый после сверки кода:
[каталог возможностей](openspec/catalog.md), [полный пайплайн](openspec/specs/processing-pipeline/spec.md),
[покрытие требований](openspec/evidence/coverage.md), [известные расхождения](openspec/review.md).
[Принятие baseline](openspec/baseline.md) не означает, что все дефекты уже исправлены.
Этот README описывает установку и запуск; требования функций живут в OpenSpec.

- [Запись, pre-roll, пауза и остановка](openspec/specs/recording/spec.md).
- [Импорт и обработка](openspec/specs/processing-pipeline/spec.md), [версии и языки](openspec/specs/transcript-versions/spec.md), [LLM](openspec/specs/llm-processing/spec.md).
- [Голосовые заметки](openspec/specs/voice-notes/spec.md) и [слоты](openspec/specs/note-integrations/spec.md).
- [Поиск и чат](openspec/specs/search-chat/spec.md), [плеер и экспорт](openspec/specs/archive-playback/spec.md), [говорящие](openspec/specs/speakers/spec.md).
- [Доступ с телефона и API](openspec/specs/api-access/spec.md), [настройки и процессы](openspec/specs/runtime-settings/spec.md).

## Установка (готовая сборка)

Один скрипт: качает бинари под вашу платформу, нативную библиотеку и все модели, пишет `.env`
с абсолютными путями и **проверяет установку** в конце.

```powershell
# Windows
cd $HOME\Desktop
$u='https://raw.githubusercontent.com/cascad/localvox-light/main/scripts/install-release.ps1'
$p="$env:TEMP\lv-install.ps1"; iwr -useb $u -OutFile $p; & $p
```
```bash
# macOS / Linux   (нужны curl, unzip, tar, jq)
curl -fsSL https://raw.githubusercontent.com/cascad/localvox-light/main/scripts/install-release.sh | bash
```

Скачивается ~2.6 ГБ моделей. `-RequiredOnly` / `--required-only` — только то, без чего продукт
не работает (~2 ГБ): без разметки говорящих и без проверки имён.

Запуск и проверка:

```bash
./localvox-light --daemon    # запись + варка + интерфейс на http://127.0.0.1:3017/
./localvox-light --doctor    # что стоит, чего не хватает и что с этим делать
```

### `--doctor`

Проверка компонентов и путей выполняется самим продуктом. Логика и ограничения —
[installation-models](openspec/specs/installation-models/spec.md). Пример вывода:

```
  [OK  ] Vosk (живое распознавание)     F:/vosk_models/vosk-model-ru-0.42
  [OK  ] GigaAM (расшифровка архива)    models/gigaam-v3-e2e-ctc
  [НЕТ ] Повар (localvox-process)       нет рядом с демоном
         → без него автоварка выключается и архив не расшифровывается НИКОГДА
  [ЖДЁТ] LLM (сводка и чистовик)        localhost:11434 не отвечает
         → ollama pull qwen3.5:9b
```

Код возврата: `0` — работает, `1` — работает не всё, `2` — не заработает. Годится для скрипта.

---

## Что нужно на машине

| Что | Зачем | Обязательно? |
|---|---|---|
| **Rust** (edition 2021) | собрать всё | да |
| **Node 20+ / npm** | собрать интерфейс (`ui/`, Vite 8) | да, если собираешь из исходников |
| **Ollama** | LLM и эмбеддинги (причёсывание, сводка, поиск по смыслу, «спросить») | да для LLM-части; без неё расшифровка работает, сводки нет |
| **ffmpeg** | извлечь дорожку при ингесте по ссылке; пережать чанки в FLAC | для ингеста и FLAC |
| **yt-dlp** | скачать звук с YouTube и подобных | только для ингеста по ссылке |

Модели (веса) в репозиторий не входят — их качают скрипты, см. ниже.

---

## Развернуть на новой машине (в т.ч. Mac) — по шагам

### 1. Тулчейны

```bash
# Rust — https://rustup.rs
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Node (пример через brew на Mac) и Ollama
brew install node ollama ffmpeg yt-dlp
```

### 2. Ollama и её модели

```bash
ollama serve            # если не поднялась сама
ollama pull qwen3.5:9b  # LLM: читаемый текст, сводка, «спросить у архива»
ollama pull bge-m3      # эмбеддинги: поиск по смыслу и гибридный
```

Почему именно эти две модели — замерено на живом архиве, объяснение в `.env.example` (рядом
с `LOCALVOX_LLM_MODEL` и `LOCALVOX_EMBED_MODEL`). Обе крутятся на `http://localhost:11434`.

### 3. Нативная библиотека Vosk

Нужна **всегда**, даже если распознаёшь GigaAM: `libvosk` линкуется в бинарь, и системный
загрузчик требует её **до** `main()`. Без неё не стартует ничего.

```bash
# Linux / macOS — сам определит ОС и архитектуру
./scripts/setup-vosk.sh
# явные пресеты, если uname не подошёл:
./scripts/setup-vosk-mac.sh
./scripts/setup-vosk-linux-amd64.sh
./scripts/setup-vosk-linux-arm64.sh
```
```powershell
# Windows
.\scripts\setup-vosk.ps1
```

**Скачивает:** только нативную библиотеку Vosk (`vosk-lib/`, с релизов alphacep/vosk-api, тег
`v0.3.42`). Модели — все разом, следующим шагом: их адреса лежат в одном месте
(`scripts/models.json`), а не размазаны по скриптам.

> **macOS:** `libvosk.dylib` должна лежать **рядом с бинарником** (dyld грузит её до `main()`,
> переменные из кода процесса не помогают). Скрипты релиза это делают; при ручном запуске из
> `target/` — `build.rs` копирует библиотеку в `target/<profile>/`, так что `cargo run` её
> находит.
>
> **Linux:** при необходимости `export LD_LIBRARY_PATH="$PWD/vosk-lib:$LD_LIBRARY_PATH"`.

### 4. Модели

Один скрипт на платформу, один список для обеих — `scripts/models.json`:

```bash
# Linux / macOS
./scripts/fetch-models.sh                 # всё
./scripts/fetch-models.sh --only gigaam   # выборочно
./scripts/fetch-models.sh --required-only # без диаризации и проверки имён
./scripts/fetch-models.sh --check         # ничего не качать, сказать чего нет
```
```powershell
# Windows — те же ключи
.\scripts\fetch-models.ps1
.\scripts\fetch-models.ps1 -Only gigaam
.\scripts\fetch-models.ps1 -RequiredOnly
.\scripts\fetch-models.ps1 -Check
```

| Каталог | Что | Размер | Обязательно? |
|---|---|---|---|
| `models/vosk-model-ru-0.42/` | Vosk — **живое** распознавание: голосовые триггеры и строки в TUI | ~1.8 ГБ | **да**: демон без него не стартует |
| `models/gigaam-v3-e2e-ctc/` | GigaAM v3 e2e CTC — расшифровка архива, со знаками препинания | ~215 МБ | **да**: без него записи не расшифровываются |
| `models/diarize/` | pyannote segmentation + ERes2Net — разметка «кто говорит» | ~46 МБ | нет; без неё расшифровка есть, разметки говорящих нет |
| `models/ner-gliner/` | GLiNER — проверка имён в ответах LLM | ~580 МБ | нет; без неё имена не проверяются (числа проверяются всегда) |

Роли моделей, optional-компоненты, проверка хешей и поведение при оборванной
загрузке описаны в [installation-models](openspec/specs/installation-models/spec.md).
Быстрое распознавание и архивный STT имеют разные пути исполнения.

### 5. Конфиг

```bash
cp .env.example .env       # при необходимости; дефолты рабочие
```

### 6. Собрать и запустить — см. следующий раздел.

---

## Как запустить

Интерфейс один и тот же в любом варианте — он живёт в демоне и открывается либо нативным окном,
либо в браузере на **http://127.0.0.1:3017/** (туда же можно зайти с телефона по локальной сети).
Запустить можно готовым бинарём или прямо из исходников через cargo.

### Вариант 1. Готовым бинарём

Для обычного запуска откройте `localvox-desktop`; для работы в фоне —
`localvox-light --daemon`. Закрытие окна оставляет демон работающим.
Подробный жизненный цикл — [runtime-settings](openspec/specs/runtime-settings/spec.md).

После установщика (`install-release`) или своей `--release`-сборки бинари лежат рядом друг с
другом (на Windows — `.exe`):

```bash
./localvox-desktop            # нативное окно (само поднимет демон) — обычный запуск
./localvox-light --daemon     # только демон, без окна: запись + автоварка + API + трей (фон / LAN)
./localvox-light --doctor     # что стоит, чего не хватает (см. раздел выше)
```

- В трее — пункт **«Открыть интерфейс»**; второй клик фокусирует уже открытое окно, а не плодит второе.
- Без окна и трея интерфейс всё равно доступен: открой **http://127.0.0.1:3017/** в браузере.

### Вариант 2. Из исходников через cargo

Интерфейс вшивается в бинарь при сборке, поэтому сначала фронт, потом Rust:

```bash
npm --prefix ui ci          # один раз: зависимости фронта
npm --prefix ui run build   # собрать интерфейс в ui/dist
cargo build --release --workspace
```

На Windows запускайте через скрипт, который **каждый раз сначала собирает демон и
обработчик одним вызовом Cargo**, затем запускает полученный бинарник. При ошибке
сборки приложение не запускается. По умолчанию профиль release:

```powershell
.\scripts\run.ps1                 # демон, API и трей
.\scripts\run.ps1 -Desktop        # собрать также нативное окно и запустить его
.\scripts\run.ps1 -Profile dev    # debug: демон и обработчик из одного профиля
.\scripts\run.ps1 -BuildOnly      # только собрать оба бинарника, без запуска
```

Скрипт работает из корня репозитория, даже если вызван из другой папки. Зависимости,
модели и `ui/dist` должны быть подготовлены заранее, как описано выше.

При ручном запуске **после каждого изменения Rust сначала пересоберите комплект**:

```bash
cargo build --release --workspace
cargo run --release -p localvox-desktop    # нативное окно (найдёт демон рядом в target/ и поднимет)
cargo run --release -- --daemon            # только демон, без окна
```

- **`--` обязательно** в команде демона: он отделяет аргументы cargo от аргументов программы,
  иначе `--daemon` уедет в cargo.
- **Debug** (быстрее компиляция, медленнее работа) — те же команды без `--release`. Но запускай из
  **того же профиля, что собрал**: окно и трей ищут соседний бинарь в `target/<profile>/`, и если
  собрать `--release`, а запустить debug — подхватится старый debug-бинарь (и наоборот).
- Дождись в логе `HTTP API: http://127.0.0.1:3017` — дальше браузер или трей → «Открыть интерфейс».

### Пересборка после правок (release vs debug — это важно)

Три части устаревают по отдельности: **интерфейс** (`ui/dist`), **демон** (`localvox-light`) и
**повар** (`localvox-process`, крейт `localvox-light-asr`). Как они обновляются — зависит от профиля.

**release** (`--release`) — интерфейс **ВШИТ в бинарь на этапе компиляции**. После любой правки UI
нужно пересобрать `ui/dist` И пересобрать бинарь, иначе release-демон отдаёт старый интерфейс, что
бы ты ни клал в `ui/dist`. Полный цикл:

```bash
npm --prefix ui run build          # интерфейс → ui/dist
cargo build --release --workspace  # ВШИВАЕТ свежий ui/dist + пересобирает демон И повара
```

**debug** (без `--release`) — rust-embed читает `ui/dist` **с диска в рантайме**. Правку UI видно
после `npm run build` + перезагрузки страницы, БЕЗ пересборки Rust; правку Rust —
`cargo build --workspace` или `.\scripts\run.ps1 -Profile dev`.

Грабли (все — реальные из этой работы):

- **Повар — отдельный бинарь.** Обычные `cargo run` и `cargo build -p localvox-light`
  НЕ обновляют `localvox-process`. Используй `scripts/run.ps1` или сборку `--workspace`.
  Несовместимый worker блокирует автоварку; лог содержит путь, ожидаемый/полученный
  протокол и способ исправления. После обновления worker перезапусти демон:
  неудачный preflight сам повторно не запускается.
- **Закрой приложение перед сборкой.** Запущенные демон/окно держат свои `.exe` залоченными →
  `cargo build` упадёт с «Access is denied». Останови через трей «Выход» (или `taskkill`).
- **Запускай из того же профиля, что собрал.** Собрал `--release`, запустил из `target/debug` —
  подхватится старый бинарь: окно и трей ищут соседа в `target/<profile>/`.
- **Демон single-instance: перезапуск ОКНА ≠ перезапуск демона.** Если демон уже слушает порт, новое
  окно просто подцепится к нему. Правки в `.env` и новый код демона подхватятся, только когда
  перезапущен САМ демон (закрыть/убить его процесс, потом поднять заново).

### LLM: Ollama и подписка Claude

Для локальной LLM запустите Ollama и скачайте выбранную модель. Для Claude нужен
установленный и авторизованный Claude Code (`claude` → `/login`). Модель и прокси
задаются через `LOCALVOX_CLAUDE_*`. Приложение удаляет `ANTHROPIC_API_KEY` и
`ANTHROPIC_AUTH_TOKEN` из окружения дочернего Claude CLI.

Выбор провайдера, ошибки и ограничения — в [llm-processing](openspec/specs/llm-processing/spec.md).
«Спросить» о материале — [отдельный контур](openspec/specs/ad-hoc-asks/spec.md).

### Правишь интерфейс — не пересобирай Rust на каждый чих

```powershell
.\scripts\run.ps1 -Profile dev     # собрать демон и обработчик, запустить API на 3017
npm --prefix ui run dev             # во втором терминале: Vite на 5173, живая перезагрузка
```
Открывай **http://localhost:5173** — `/api` проксируется в демон. Правки в `ui/src/*` видны
сразу. Чтобы увидеть их **вшитыми** в бинарь — `npm --prefix ui run build` и пересобрать демон.

---

## Что где лежит

```
crates/localvox-light        демон: захват, запись, трей, автоварка, HTTP API
crates/localvox-light-core   ядро: конвейер, чанки, очередь заданий, ссылки, этапы
crates/localvox-light-asr    localvox-process: расшифровка + LLM-обработка сессии
crates/localvox-light-llm    LLM-клиент, проверка обоснованности, шаблоны
crates/localvox-light-api    HTTP API + встроенный фронт (rust-embed)
crates/localvox-light-voice  голосовые команды (запись, заметки, «возьми ссылку»)
crates/localvox-light-tui    старый терминальный интерфейс
src-tauri                    оболочка рабочего стола (окно — вид на демон)
ui                           фронт (Vite + React + TS), собирается в ui/dist
models/  vosk-lib/           модели и нативная библиотека (в .gitignore, ставятся скриптами)
```

Записи лежат в `<AUDIO_DIR>/sessions/<дата_время>/`; по умолчанию `localvox-audio`,
путь задаётся `LOCALVOX_LIGHT_AUDIO_DIR`. Поведение файлов, восстановления и retention —
[audio-storage](openspec/specs/audio-storage/spec.md). После удаления исходного аудио
сохранённый текст нельзя считать произвольно восстанавливаемым из него.

## Оговорка про платформы

Разработка и предыдущие живые проверки ориентированы на Windows. Наличие веток
macOS/Linux и прежних результатов сборки не подтверждает аппаратный захват на текущей
версии. В этой миграции живые прогоны на этих ОС не выполнялись. Автостарт через
HKCU Run реализован для Windows. Детали и ограничения —
[runtime-settings](openspec/specs/runtime-settings/spec.md).

### Проверять не-Windows сборку, не имея не-Windows

Кросс-проверка с Windows не работает: `alsa-sys` требует sysroot целевой системы. Настоящий
компилятор — в контейнере, и это дёшево:

```bash
docker run --rm -v "$PWD:/src" -w /src -e CARGO_TARGET_DIR=/tmp/t rust:1-bookworm \
  bash -c "apt-get update -qq && apt-get install -y -qq libasound2-dev pkg-config && \
           cargo check -p localvox-light -p localvox-light-asr"
```

Оболочка окна (Tauri) на каждой ОС использует системный вебвью, поэтому её стоит **проверять
сборкой на самой ОС**, а не полагаться на «раз в Windows работает — работает везде».
