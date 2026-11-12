#!/usr/bin/env bash
# Install the complete macOS/Linux bundle. --archive=path installs an Actions archive.
set -euo pipefail
REPO=cascad/localvox-light TAG=latest INSTALL_DIR="$(pwd)/localvox-light"
ARCHIVE='' SKIP_MODELS=0 SKIP_BINARY=0
MODEL_ARGS=()
for arg in "$@"; do
  case "$arg" in
    --repo=*) REPO="${arg#*=}";; --tag=*) TAG="${arg#*=}";;
    --install-dir=*) INSTALL_DIR="${arg#*=}";; --archive=*) ARCHIVE="${arg#*=}";;
    --branch=*) : ;; # Helpers are pinned to the selected package now.
    --required-only) MODEL_ARGS+=(--required-only);;
    --skip-models) SKIP_MODELS=1;; --skip-binary) SKIP_BINARY=1;;
    -h|--help) echo 'install-release.sh [--install-dir=PATH] [--tag=TAG] [--archive=PATH] [--required-only] [--skip-models] [--skip-binary]'; exit 0;;
    *) echo "Unknown argument: $arg" >&2; exit 2;;
  esac
done
fail() { echo "$*" >&2; exit 1; }
[[ -z "$ARCHIVE" || "$SKIP_BINARY" = 0 ]] || fail '--archive and --skip-binary cannot be combined'
for tool in curl tar jq; do command -v "$tool" >/dev/null || fail "Required: $tool (macOS: brew install jq)"; done
if command -v shasum >/dev/null; then SHA=(shasum -a 256); elif command -v sha256sum >/dev/null; then SHA=(sha256sum); else fail 'Required: shasum or sha256sum'; fi
sha256() { "${SHA[@]}" "$1" | cut -d ' ' -f1; }
case "$(uname -s)/$(uname -m)" in
 Darwin/arm64) TARGET=aarch64-apple-darwin; NATIVE=libvosk.dylib;;
 Darwin/x86_64) TARGET=x86_64-apple-darwin; NATIVE=libvosk.dylib;;
 Linux/x86_64) TARGET=x86_64-unknown-linux-gnu; NATIVE=libvosk.so;;
 Linux/aarch64|Linux/arm64) TARGET=aarch64-unknown-linux-gnu; NATIVE=libvosk.so;;
 *) fail "Unsupported platform: $(uname -s)/$(uname -m)";;
esac
WORK="$(mktemp -d -t localvox-install.XXXXXXXX)"
trap 'rm -rf "$WORK"' EXIT
REQUIRED=(localvox-light localvox-process localvox-tui "$NATIVE" .env.defaults scripts/models.json scripts/fetch-models.sh licenses/rs-morphy4/NOTICE)
if [[ "$SKIP_BINARY" = 0 ]]; then
 if [[ -z "$ARCHIVE" ]]; then
  if [[ "$TAG" = latest ]]; then API="https://api.github.com/repos/$REPO/releases/latest"; else API="https://api.github.com/repos/$REPO/releases/tags/$TAG"; fi
  curl -fsSL -A localvox-install/2 "$API" -o "$WORK/release.json"
  NAME="localvox-$TARGET.tar.gz"
  URL="$(jq -er --arg name "$NAME" '[.assets[] | select(.name == $name)] | if length == 1 then .[0].browser_download_url else error("No complete platform archive; use a new Actions bundle with --archive or select a newer --tag") end' "$WORK/release.json")"
  ARCHIVE="$WORK/$NAME"
  curl -fL -A localvox-install/2 "$URL" -o "$ARCHIVE"
  EXPECTED="$(jq -r --arg name "$NAME" '.assets[] | select(.name == $name) | .digest // ""' "$WORK/release.json")"
  if [[ "$EXPECTED" = sha256:* ]]; then [[ "$(sha256 "$ARCHIVE")" = "${EXPECTED#sha256:}" ]] || fail 'Archive SHA-256 mismatch'; fi
 fi
 [[ -f "$ARCHIVE" ]] || fail "Archive not found: $ARCHIVE"
 # Reject paths/links that could escape the staging directory before extraction.
 tar -tzf "$ARCHIVE" > "$WORK/members"
 while IFS= read -r name; do
  case "$name" in /*|*\\*|*:*|../*|*/../*|*/..) fail "Unsafe archive path: $name";; esac
 done < "$WORK/members"
 tar -tvzf "$ARCHIVE" > "$WORK/member-details"
 if grep -Eq '^[lh]' "$WORK/member-details"; then fail 'Archive links are not supported'; fi
 mkdir "$WORK/bundle"
 tar -xzf "$ARCHIVE" -C "$WORK/bundle"
 BUNDLE="$WORK/bundle"
 [[ -f "$BUNDLE/bundle.json" ]] || fail 'Incomplete package: bundle.json missing'
 jq -e --arg target "$TARGET" '.schema == 1 and .target == $target and (.files | type == "array")' "$BUNDLE/bundle.json" >/dev/null || fail 'Package platform/schema mismatch'
 for name in "${REQUIRED[@]}"; do jq -e --arg name "$name" 'any(.files[]; .name == $name)' "$BUNDLE/bundle.json" >/dev/null || fail "Incomplete package: $name"; done
 jq -r '.files[].name' "$BUNDLE/bundle.json" > "$WORK/files"
 while IFS= read -r name; do
  case "$name" in ''|.env|/*|*\\*|*:*|../*|*/../*|*/..) fail "Invalid package member: $name";; esac
  [[ -f "$BUNDLE/$name" ]] || fail "Package member missing: $name"
  EXPECTED="$(jq -r --arg name "$name" '.files[] | select(.name == $name) | .sha256' "$BUNDLE/bundle.json")"
  [[ "$(sha256 "$BUNDLE/$name")" = "$EXPECTED" ]] || fail "Package SHA-256 mismatch: $name"
 done < "$WORK/files"
 mkdir -p "$INSTALL_DIR"
 while IFS= read -r name; do mkdir -p "$INSTALL_DIR/$(dirname "$name")"; cp -f "$BUNDLE/$name" "$INSTALL_DIR/$name"; done < "$WORK/files"
 cp "$BUNDLE/bundle.json" "$INSTALL_DIR/bundle.json"
fi
mkdir -p "$INSTALL_DIR"
INSTALL_DIR="$(cd "$INSTALL_DIR" && pwd)"
for name in "${REQUIRED[@]}"; do [[ -f "$INSTALL_DIR/$name" ]] || fail "Installation missing: $name"; done
for name in localvox-light localvox-process localvox-tui localvox-desktop; do
 if [[ -f "$INSTALL_DIR/$name" ]]; then chmod +x "$INSTALL_DIR/$name"; fi
done
"$INSTALL_DIR/localvox-process" --worker-protocol >/dev/null
"$INSTALL_DIR/localvox-tui" --version >/dev/null
"$INSTALL_DIR/localvox-light" --help >/dev/null
if [[ "$SKIP_MODELS" = 0 ]]; then bash "$INSTALL_DIR/scripts/fetch-models.sh" --root "$INSTALL_DIR" ${MODEL_ARGS[@]+"${MODEL_ARGS[@]}"}; fi
if [[ ! -f "$INSTALL_DIR/.env" ]]; then
 # dotenv quoted paths also work when the chosen directory contains spaces.
 ENV_DIR="${INSTALL_DIR//\\/\\\\}"; ENV_DIR="${ENV_DIR//\"/\\\"}"; ENV_DIR="${ENV_DIR//\$/\\\$}"
 printf '%s\n' '# Created by install-release.sh; presets are in .env.defaults.' \
  "LOCALVOX_LIGHT_MODEL=\"$ENV_DIR/models/vosk-model-ru-0.42\"" \
  "LOCALVOX_ASR_MODEL_DIR=\"$ENV_DIR/models/gigaam-v3-e2e-ctc\"" \
  "LOCALVOX_LIGHT_AUDIO_DIR=\"$ENV_DIR/archive\"" > "$INSTALL_DIR/.env"
else echo 'Existing .env preserved.'; fi
DOCTOR=0
(cd "$INSTALL_DIR" && ./localvox-light --doctor) || DOCTOR=$?
if [[ "$DOCTOR" -gt 2 || ("$DOCTOR" = 2 && "$SKIP_MODELS" = 0) ]]; then fail "Doctor failed ($DOCTOR)"; fi
echo "Installed: $INSTALL_DIR"
if [[ "$SKIP_MODELS" = 1 ]]; then echo 'Models skipped. Install them before recording/transcription.'; fi
printf 'Run: cd "%s" && ./localvox-light --daemon\nUI: http://127.0.0.1:3017/   TUI: ./localvox-tui\n' "$INSTALL_DIR"
