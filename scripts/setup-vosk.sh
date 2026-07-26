#!/usr/bin/env bash
# Скачивает НАТИВНУЮ БИБЛИОТЕКУ Vosk (GitHub) в vosk-lib/. Только её: она своя под каждую
# ОС и архитектуру, и её грузит системный загрузчик ДО main().
#
# Модели (в том числе модель Vosk) ставит fetch-models.sh по scripts/models.json — один
# список на все платформы. Здесь их адреса лежали вторым экземпляром, и списки разошлись.
#
# Нужны: bash, curl, unzip.
#
#   ./scripts/setup-vosk.sh
#   ./scripts/setup-vosk.sh --preset=linux-x86_64   # явная архитектура (см. также setup-vosk-linux-*.sh)
#   ./scripts/setup-vosk.sh --install-root=/opt/localvox   # vosk-lib в этой папке (для install-release.sh)
#
# Переменные: LOCALVOX_VOSK_API_TAG (по умолчанию v0.3.42)

set -euo pipefail

VOSK_TAG="${LOCALVOX_VOSK_API_TAG:-v0.3.42}"
VER="${VOSK_TAG#v}"
FORCE=0
PRESET=""
INSTALL_ROOT=""

for a in "$@"; do
  case "$a" in
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

install_native

echo ""
echo "--- Дальше ---"
echo "Модели (включая модель Vosk) ставит fetch-models.sh:"
echo "  ./scripts/fetch-models.sh --root \"$ROOT\""
echo "Перед запуском бинарника (если линкер не находит libvosk):"
echo "  export LD_LIBRARY_PATH=\"$VOSK_LIB_DIR:\$LD_LIBRARY_PATH\""
echo "(macOS при необходимости: DYLD_LIBRARY_PATH)"
