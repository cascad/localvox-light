#!/usr/bin/env bash
# Download GitHub Release binary + vosk-lib + model into one directory.
# Requires: bash, curl, unzip; jq for JSON (brew install jq / apt install jq).
#
# Env: LOCALVOX_LIGHT_REPO, LOCALVOX_LIGHT_TAG (or latest), LOCALVOX_LIGHT_INSTALL_DIR, LOCALVOX_LIGHT_BRANCH
#
# Example:
#   curl -fsSL https://raw.githubusercontent.com/cascad/localvox-light/main/scripts/install-release.sh | bash
#   LOCALVOX_LIGHT_INSTALL_DIR=~/lv bash install-release.sh

set -euo pipefail

REPO="${LOCALVOX_LIGHT_REPO:-cascad/localvox-light}"
TAG="${LOCALVOX_LIGHT_TAG:-latest}"
BRANCH="${LOCALVOX_LIGHT_BRANCH:-main}"
INSTALL_DIR="${LOCALVOX_LIGHT_INSTALL_DIR:-$HOME/localvox-light}"

for a in "$@"; do
  case "$a" in
    --repo=*) REPO="${a#*=}" ;;
    --tag=*) TAG="${a#*=}" ;;
    --install-dir=*) INSTALL_DIR="${a#*=}" ;;
    --branch=*) BRANCH="${a#*=}" ;;
    --skip-vosk) SKIP_VOSK=1 ;;
    --skip-binary) SKIP_BINARY=1 ;;
    *)
      echo "Unknown arg: $a" >&2
      exit 1
      ;;
  esac
done

command -v curl >/dev/null || { echo "Need curl" >&2; exit 1; }
command -v unzip >/dev/null || { echo "Need unzip" >&2; exit 1; }
command -v jq >/dev/null || { echo "Need jq (parse GitHub API). Install: apt install jq / brew install jq" >&2; exit 1; }

mkdir -p "$INSTALL_DIR"
INSTALL_DIR="$(cd "$INSTALL_DIR" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if [ -z "${SKIP_VOSK:-}" ]; then
  echo "Fetching setup-vosk.sh (branch $BRANCH)..."
  curl -fsSL -A "localvox-light-install/1.0" \
    "https://raw.githubusercontent.com/$REPO/$BRANCH/scripts/setup-vosk.sh" \
    -o "$TMP/setup-vosk.sh"
  bash "$TMP/setup-vosk.sh" --install-root="$INSTALL_DIR"
fi

if [ -z "${SKIP_BINARY:-}" ]; then
  if [ "$TAG" = "latest" ]; then
    API="https://api.github.com/repos/$REPO/releases/latest"
  else
    API="https://api.github.com/repos/$REPO/releases/tags/$TAG"
  fi
  echo "Release API: $API"
  JSON="$(curl -fsSL -H "Accept: application/vnd.github+json" -A "localvox-light-install/1.0" "$API")"

  case "$(uname -s)" in
    Linux)
      case "$(uname -m)" in
        x86_64)
          URL="$(echo "$JSON" | jq -r '.assets[] | select(.name | test("x86_64-unknown-linux-gnu")) | .browser_download_url' | head -1)"
          ;;
        aarch64 | arm64)
          URL="$(echo "$JSON" | jq -r '.assets[] | select(.name | test("aarch64-unknown-linux-gnu")) | .browser_download_url' | head -1)"
          ;;
        *)
          echo "Unsupported Linux arch: $(uname -m)" >&2
          exit 1
          ;;
      esac
      ;;
    Darwin)
      case "$(uname -m)" in
        arm64)
          URL="$(echo "$JSON" | jq -r '.assets[] | select(.name | test("aarch64-apple-darwin")) | .browser_download_url' | head -1)"
          ;;
        *)
          URL="$(echo "$JSON" | jq -r '.assets[] | select(.name | test("x86_64-apple-darwin")) | .browser_download_url' | head -1)"
          ;;
      esac
      ;;
    *)
      echo "Use install-release.ps1 on Windows." >&2
      exit 1
      ;;
  esac

  if [ -z "$URL" ] || [ "$URL" = "null" ]; then
    echo "No matching asset. Assets in this release:" >&2
    echo "$JSON" | jq -r '.assets[].name' >&2 || true
    echo "Create a GitHub Release and upload binaries (names like CI artifacts)." >&2
    exit 1
  fi

  NAME="$(basename "$URL" | cut -d'?' -f1)"
  OUT="$TMP/$NAME"
  echo "Downloading $URL"
  curl -fsSL -A "localvox-light-install/1.0" -L "$URL" -o "$OUT"

  if [[ "$NAME" == *.zip ]]; then
    unzip -o -q "$OUT" -d "$INSTALL_DIR"
  else
    chmod +x "$OUT"
    cp -f "$OUT" "$INSTALL_DIR/$NAME"
    BIN_NAME="$NAME"
  fi

  echo ""
  echo "Done. Example:"
  echo "  cd \"$INSTALL_DIR\""
  echo "  export LD_LIBRARY_PATH=\"$INSTALL_DIR/vosk-lib:\$LD_LIBRARY_PATH\""
  if [ -n "${BIN_NAME:-}" ]; then
    echo "  ./$BIN_NAME --tui"
  else
    echo "  ./localvox-light --tui"
  fi
fi
