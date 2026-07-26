#!/usr/bin/env bash
# Установка localvox одной папкой (macOS / Linux).
#
#   curl -fsSL https://raw.githubusercontent.com/cascad/localvox-light/main/scripts/install-release.sh | bash
#
# Что получится в <каталог>:
#   localvox-light        демон: запись, варка, HTTP-интерфейс
#   localvox-process      повар: расшифровка и сводка (БЕЗ НЕГО АРХИВ НЕ РАСШИФРУЕТСЯ)
#   localvox-desktop      окно (если есть в релизе; иначе интерфейс в браузере)
#   libvosk.dylib/.so     нативная библиотека — грузится ДО main(), поэтому лежит рядом
#   models/               vosk-model-ru-0.42, gigaam-v3-e2e-ctc, diarize, ner-gliner
#   .env                  пути, прописанные абсолютно
#
# Порядок: бинари → модели → ПРОВЕРКА. Проверяет сам продукт (`localvox-light --doctor`), а не
# этот скрипт: демон знает, ГДЕ он ищет, а скрипт знает только то, что сам разложил. Эти два
# ответа уже расходились — модель на другом диске по LOCALVOX_LIGHT_MODEL скрипт считает
# отсутствующей, хотя всё работает.
#
# Ключи: --install-dir=~/lv  --tag=v0.1.0  --repo=owner/repo  --branch=main
#        --required-only (без диаризации и проверки имён, экономит ~630 МБ)
#        --skip-models  --skip-binary
#
# Нужны: curl, unzip, tar, jq.

set -euo pipefail

REPO="cascad/localvox-light"
TAG="latest"
BRANCH="main"
INSTALL_DIR="$(pwd)/localvox-light"
MODEL_ARGS=()
SKIP_MODELS=0
SKIP_BINARY=0

for a in "$@"; do
  case "$a" in
    --repo=*) REPO="${a#*=}" ;;
    --tag=*) TAG="${a#*=}" ;;
    --install-dir=*) INSTALL_DIR="${a#*=}" ;;
    --branch=*) BRANCH="${a#*=}" ;;
    --required-only) MODEL_ARGS+=(--required-only) ;;
    --skip-models) SKIP_MODELS=1 ;;
    --skip-binary) SKIP_BINARY=1 ;;
    -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
    *) echo "Неизвестный аргумент: $a" >&2; exit 2 ;;
  esac
done

for t in curl unzip tar jq; do
  command -v "$t" >/dev/null || { echo "Нужен $t. Поставить: brew install $t / apt install $t" >&2; exit 1; }
done

mkdir -p "$INSTALL_DIR"
INSTALL_DIR="$(cd "$INSTALL_DIR" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

RAW="https://raw.githubusercontent.com/$REPO/$BRANCH/scripts"
say() { printf '\n\033[1m%s\033[0m\n' "$*"; }

# ── Целевая тройка. Ошибиться тут — скачать чужой бинарь и получить «Exec format error»
#    вместо внятного сообщения.
case "$(uname -s)" in
  Darwin) case "$(uname -m)" in
            arm64) TARGET="aarch64-apple-darwin" ;;
            x86_64) TARGET="x86_64-apple-darwin" ;;
            *) echo "Неизвестная архитектура macOS: $(uname -m)" >&2; exit 1 ;;
          esac ;;
  Linux)  case "$(uname -m)" in
            x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
            aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
            *) echo "Неизвестная архитектура Linux: $(uname -m)" >&2; exit 1 ;;
          esac ;;
  *) echo "Эта ОС не поддерживается: $(uname -s). Для Windows — install-release.ps1" >&2; exit 1 ;;
esac

# ── Нативная библиотека Vosk. Её грузит загрузчик ДО main(), поэтому она обязана лежать
#    рядом с бинарём: переменные среды уже запущенного процесса тут не помогают.
say "Нативная библиотека Vosk"
curl -fsSL -A "localvox-install/1.0" "$RAW/setup-vosk.sh" -o "$TMP/setup-vosk.sh"
bash "$TMP/setup-vosk.sh" --install-root="$INSTALL_DIR"
if [ -d "$INSTALL_DIR/vosk-lib" ]; then
  find "$INSTALL_DIR/vosk-lib" -maxdepth 1 -name 'libvosk.*' -exec cp -f {} "$INSTALL_DIR/" \;
fi

# ── Бинари. ОДНИМ архивом на платформу: демон, повар и окно приезжают вместе. Раздельные
#    файлы уже привели к установке без повара — продукт писал звук и не расшифровывал ничего.
if [ "$SKIP_BINARY" = 0 ]; then
  say "Бинари ($TARGET)"
  if [ "$TAG" = "latest" ]; then API="https://api.github.com/repos/$REPO/releases/latest"
  else API="https://api.github.com/repos/$REPO/releases/tags/$TAG"; fi

  rel="$(curl -fsSL -H 'Accept: application/vnd.github+json' -A 'localvox-install/1.0' "$API")"
  url="$(jq -r --arg t "$TARGET" '.assets[] | select(.name | contains($t)) | .browser_download_url' <<<"$rel" | head -1)"
  if [ -z "$url" ] || [ "$url" = "null" ]; then
    echo "В релизе нет сборки под $TARGET. Что есть:" >&2
    jq -r '.assets[].name' <<<"$rel" 2>/dev/null | sed 's/^/  - /' >&2 || true
    exit 1
  fi
  echo "  ⇣ $(basename "$url")"
  curl -fL --progress-bar -A "localvox-install/1.0" "$url" -o "$TMP/bin.tar.gz"
  tar -xzf "$TMP/bin.tar.gz" -C "$INSTALL_DIR"

  for f in localvox-light localvox-process localvox-desktop; do
    [ -f "$INSTALL_DIR/$f" ] && chmod +x "$INSTALL_DIR/$f"
  done
  # macOS вешает карантин на всё скачанное, и Gatekeeper убивает бинарь без объяснений
  # («не удаётся проверить разработчика»). Снимаем метку явно: мы только что сами это
  # скачали и знаем откуда. Настоящая подпись — отдельный разговор с Apple Developer ID.
  if [ "$(uname -s)" = "Darwin" ]; then
    xattr -dr com.apple.quarantine "$INSTALL_DIR" 2>/dev/null || true
  fi
  [ -f "$INSTALL_DIR/localvox-process" ] ||
    echo "  ВНИМАНИЕ: в архиве нет localvox-process — архив не будет расшифровываться" >&2
fi

# ── Модели. Список — в models.json, механика — в fetch-models.sh. Оба скачиваются рядом,
#    потому что скрипт ищет манифест возле себя.
if [ "$SKIP_MODELS" = 0 ]; then
  say "Модели"
  mkdir -p "$TMP/s"
  curl -fsSL -A "localvox-install/1.0" "$RAW/models.json" -o "$TMP/s/models.json"
  curl -fsSL -A "localvox-install/1.0" "$RAW/fetch-models.sh" -o "$TMP/s/fetch-models.sh"
  bash "$TMP/s/fetch-models.sh" --root "$INSTALL_DIR" ${MODEL_ARGS[@]+"${MODEL_ARGS[@]}"}
fi

# ── .env. Пути АБСОЛЮТНЫЕ намеренно: под launchd рабочий каталог не наш, и относительный
#    `models/…` не найдётся — ровно тот отказ, который потом ищут полдня.
say "Настройки"
ENV_PATH="$INSTALL_DIR/.env"
if [ -f "$ENV_PATH" ]; then
  echo "  .env уже есть — не трогаю, правки ваши"
else
  cat > "$ENV_PATH" <<EOF
# Создано install-release.sh. Пути абсолютные намеренно: рабочий каталог демона под
# автозапуском не совпадает с этой папкой.
LOCALVOX_LIGHT_MODEL=$INSTALL_DIR/models/vosk-model-ru-0.42
LOCALVOX_ASR_MODEL_DIR=$INSTALL_DIR/models/gigaam-v3-e2e-ctc
LOCALVOX_LIGHT_AUDIO_DIR=$INSTALL_DIR/archive

# Сводка и чистовик. Без Ollama расшифровка всё равно работает.
LOCALVOX_LLM_BASE_URL=http://localhost:11434/v1
LOCALVOX_LLM_MODEL=qwen3.5:9b
EOF
  echo "  записан $ENV_PATH"
fi

# ── Проверка. Спрашиваем сам продукт.
say "Проверка"
if [ -x "$INSTALL_DIR/localvox-light" ]; then
  ( cd "$INSTALL_DIR" && ./localvox-light --doctor ) || true
else
  echo "  бинаря нет — проверять нечего"
fi

say "Готово: $INSTALL_DIR"
cat <<EOF
  Запуск:            cd "$INSTALL_DIR" && ./localvox-light --daemon
  Проверить ещё раз: cd "$INSTALL_DIR" && ./localvox-light --doctor
  Интерфейс:         http://127.0.0.1:3017/
EOF
