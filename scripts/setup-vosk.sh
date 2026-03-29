#!/usr/bin/env bash
# Скачивает нативную библиотеку Vosk (GitHub) в vosk-lib/ и модель в models/.
# Нужны: bash, curl, unzip.
#
#   ./scripts/setup-vosk.sh
#   ./scripts/setup-vosk.sh --skip-model
#   ./scripts/setup-vosk.sh --preset=linux-x86_64   # явная архитектура (см. также setup-vosk-linux-*.sh)
#   ./scripts/setup-vosk.sh --install-root=/opt/localvox   # vosk-lib + models в этой папке (для install-release.sh)
#
# Переменные: LOCALVOX_VOSK_API_TAG (по умолчанию v0.3.42), LOCALVOX_SETUP_MODEL_URL, LOCALVOX_SETUP_FORCE=1

set -euo pipefail

VOSK_TAG="${LOCALVOX_VOSK_API_TAG:-v0.3.42}"
VER="${VOSK_TAG#v}"
MODEL_URL="${LOCALVOX_SETUP_MODEL_URL:-https://alphacephei.com/vosk/models/vosk-model-ru-0.42.zip}"
SKIP_MODEL=0
FORCE=0
PRESET=""
INSTALL_ROOT=""

for a in "$@"; do
  case "$a" in
    --skip-model) SKIP_MODEL=1 ;;
    --force) FORCE=1 ;;
    --preset=*) PRESET="${a#*=}" ;;
    --install-root=*) INSTALL_ROOT="${a#*=}" ;;
    *)
      echo "Неизвестный аргумент: $a" >&2
      exit 1
      ;;
  esac
done

if [ -n "${LOCALVOX_SETUP_FORCE:-}" ] && [ "$LOCALVOX_SETUP_FORCE" != "0" ]; then
  FORCE=1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -n "$INSTALL_ROOT" ]; then
  ROOT="$(cd "$INSTALL_ROOT" && pwd)"
else
  ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
fi
VOSK_LIB_DIR="$ROOT/vosk-lib"

detect_zip() {
  if [ -n "$PRESET" ]; then
    case "$PRESET" in
      linux-x86_64) echo "vosk-linux-x86_64-${VER}.zip" ;;
      linux-aarch64) echo "vosk-linux-aarch64-${VER}.zip" ;;
      linux-x86) echo "vosk-linux-x86-${VER}.zip" ;;
      darwin|macos|osx) echo "vosk-osx-${VER}.zip" ;;
      *)
        echo "Неизвестный --preset=$PRESET (linux-x86_64|linux-aarch64|linux-x86|darwin)" >&2
        exit 1
        ;;
    esac
    return
  fi
  case "$(uname -s)" in
    Linux)
      case "$(uname -m)" in
        x86_64) echo "vosk-linux-x86_64-${VER}.zip" ;;
        aarch64 | arm64) echo "vosk-linux-aarch64-${VER}.zip" ;;
        i386 | i686 | x86) echo "vosk-linux-x86-${VER}.zip" ;;
        *)
          echo "Неподдерживаемая архитектура Linux: $(uname -m). Задайте --preset=…" >&2
          exit 1
          ;;
      esac
      ;;
    Darwin) echo "vosk-osx-${VER}.zip" ;;
    *)
      echo "На Windows используйте scripts/setup-vosk.ps1" >&2
      exit 1
      ;;
  esac
}

install_native() {
  local zip url tmp inner
  zip="$(detect_zip)"
  url="https://github.com/alphacep/vosk-api/releases/download/${VOSK_TAG}/${zip}"
  echo "Скачивание $url"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  curl -fsSL -A "localvox-light-setup/1.0" -o "$tmp/vosk.zip" "$url"
  unzip -q "$tmp/vosk.zip" -d "$tmp/ex"
  inner="$(find "$tmp/ex" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
  if [ -z "$inner" ]; then
    echo "Неверная структура архива vosk" >&2
    exit 1
  fi
  rm -rf "$VOSK_LIB_DIR"
  mkdir -p "$VOSK_LIB_DIR"
  cp -R "$inner/"* "$VOSK_LIB_DIR/" 2>/dev/null || true
  # на случай скрытых файлов в корне архива
  (
    shopt -s dotglob nullglob
    for f in "$inner"/.*; do
      [ ! -e "$f" ] && continue
      base="$(basename "$f")"
      [[ "$base" == "." || "$base" == ".." ]] && continue
      cp -R "$f" "$VOSK_LIB_DIR/"
    done
  )
  touch "$VOSK_LIB_DIR/.gitkeep"
  printf '%s\n%s\n' "$VOSK_TAG" "$zip" >"$VOSK_LIB_DIR/.vosk_native_version"
  echo "Нативная библиотека -> $VOSK_LIB_DIR"
}

model_dir_name() {
  basename "${MODEL_URL%.zip}"
}

install_model() {
  [ "$SKIP_MODEL" = 1 ] && return 0
  local name dest tmp
  name="$(model_dir_name)"
  dest="$ROOT/models/$name"
  if [ -f "$dest/am/final.mdl" ] && [ "$FORCE" != 1 ]; then
    echo "Модель уже есть: $dest (пропуск). Для перекачки: --force или LOCALVOX_SETUP_FORCE=1"
    return 0
  fi
  echo "Скачивание модели $MODEL_URL"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  curl -fsSL -A "localvox-light-setup/1.0" -o "$tmp/model.zip" "$MODEL_URL"
  mkdir -p "$ROOT/models"
  if [ "$FORCE" = 1 ] && [ -d "$dest" ]; then
    rm -rf "$dest"
  fi
  unzip -q "$tmp/model.zip" -d "$ROOT/models"
  if [ ! -f "$dest/am/final.mdl" ]; then
    echo "После распаковки не найден $dest/am/final.mdl (нужен полный архив модели Vosk)" >&2
    exit 1
  fi
  echo "Модель -> $dest"
}

install_native
install_model

echo ""
echo "--- Дальше ---"
echo "В .env:"
echo "  LOCALVOX_LIGHT_MODEL=$ROOT/models/$(model_dir_name)"
echo "Перед запуском бинарника (если линкер не находит libvosk):"
echo "  export LD_LIBRARY_PATH=\"$VOSK_LIB_DIR:\$LD_LIBRARY_PATH\""
echo "(macOS при необходимости: DYLD_LIBRARY_PATH)"
