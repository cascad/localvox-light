#!/usr/bin/env bash
# One-command bootstrap. No Rust, Node, Python or jq is required.
main() (
 set -euo pipefail
 local RELEASE_BASE='https://github.com/cascad/localvox-light' TAG=latest INSTALL='' UPDATE=0 CHECK=0 NO_PATH=0
 while [[ $# -gt 0 ]]; do
  case "$1" in
   --install-dir) INSTALL="$2"; shift 2;; --tag) TAG="$2"; shift 2;;
   --release-base) RELEASE_BASE="${2%/}"; shift 2;; --update) UPDATE=1; shift;;
   --check) CHECK=1; shift;; --no-path-update) NO_PATH=1; shift;;
   -h|--help) echo 'install.sh [--install-dir PATH] [--tag vX.Y.Z] [--update] [--check] [--no-path-update]'; return;;
   *) echo "Unknown argument: $1" >&2; return 2;;
  esac
 done
 case "$RELEASE_BASE" in https://*|http://127.0.0.1:*|http://localhost:*) :;; *) echo 'Release base must be HTTPS (or a local test server)' >&2; return 2;; esac
 local TARGET NATIVE
 case "$(uname -s)/$(uname -m)" in
  Darwin/arm64) TARGET=aarch64-apple-darwin; NATIVE=libvosk.dylib;;
  Darwin/x86_64) TARGET=x86_64-apple-darwin; NATIVE=libvosk.dylib;;
  Linux/x86_64) TARGET=x86_64-unknown-linux-gnu; NATIVE=libvosk.so;;
  Linux/aarch64|Linux/arm64) TARGET=aarch64-unknown-linux-gnu; NATIVE=libvosk.so;;
  *) echo "Unsupported platform: $(uname -s)/$(uname -m)" >&2; return 2;;
 esac
 if [[ -z "$INSTALL" ]]; then
  if [[ -f "$HOME/.localvox/install-location" ]]; then
   IFS= read -r INSTALL < "$HOME/.localvox/install-location" || true
   INSTALL="${INSTALL%$'\r'}"
   [[ "$INSTALL" = /* && ! "$INSTALL" =~ [[:cntrl:]] ]] || INSTALL=''
  fi
 fi
 if [[ -z "$INSTALL" ]]; then
  if [[ "$(uname -s)" = Darwin ]]; then INSTALL="$HOME/Applications/LocalVox"; else INSTALL="$HOME/.local/share/localvox"; fi
 fi
 [[ "$INSTALL" = /* ]] || INSTALL="$PWD/$INSTALL"
 local OPTIONS=(--install-dir "$INSTALL")
 [[ "$NO_PATH" = 0 ]] || OPTIONS+=(--no-path-update)
 if [[ "$CHECK" = 1 ]]; then
  [[ -x "$INSTALL/localvox-install" ]] || { echo 'LocalVox is not installed.'; return 2; }
  "$INSTALL/localvox-install" "${OPTIONS[@]}" --check
  return
 fi
 local LATEST_REQUESTED=0
 [[ "$TAG" != latest ]] || LATEST_REQUESTED=1
 command -v curl >/dev/null || { echo 'curl is required' >&2; return 2; }
 command -v tar >/dev/null || { echo 'tar is required' >&2; return 2; }
 local SHA
 if command -v shasum >/dev/null; then SHA=(shasum -a 256); elif command -v sha256sum >/dev/null; then SHA=(sha256sum); else echo 'shasum or sha256sum is required' >&2; return 2; fi
 if [[ "$TAG" = latest ]]; then
  echo 'Checking the latest LocalVox release...'
  local resolved prefix="$RELEASE_BASE/releases/tag/"
  resolved="$(curl -fsSIL --connect-timeout 20 --max-time 60 -w '%{url_effective}' "$RELEASE_BASE/releases/latest" | tail -n 1)"
  resolved="${resolved%$'\r'}"
  [[ "$resolved" = "$prefix"* ]] || { echo 'Could not resolve latest release tag' >&2; return 1; }
  TAG="${resolved#"$prefix"}"
 fi
 [[ "$TAG" =~ ^v[0-9][A-Za-z0-9._-]*$ ]] || { echo 'Invalid release tag' >&2; return 2; }
 if [[ "$UPDATE" = 0 && -x "$INSTALL/localvox-install" ]]; then
  local state=0
  "$INSTALL/localvox-install" --bundle-check --release-tag "$TAG" --install-dir "$INSTALL" 2>/dev/null || state=$?
  if [[ "$state" = 0 || ( "$state" = 3 && "$LATEST_REQUESTED" = 1 ) ]]; then
   if [[ "$state" = 3 ]]; then echo "Installed LocalVox is newer than $TAG; download skipped."
   else echo "LocalVox $TAG is verified and current; download skipped."; fi
   "$INSTALL/localvox-install" "${OPTIONS[@]}" </dev/tty
   return
  fi
 fi
 local NAME="localvox-$TARGET.tar.gz" WORK
 local TEMP_BASE
 TEMP_BASE="$(cd "${TMPDIR:-/tmp}" && pwd -P)"
 WORK="$(mktemp -d "$TEMP_BASE/localvox-bootstrap.XXXXXXXX")"
 local DOWNLOAD_PID='' DOWNLOAD_LINE_OPEN=0
 cleanup() {
  if [[ -n "$DOWNLOAD_PID" ]]; then kill "$DOWNLOAD_PID" 2>/dev/null || true; wait "$DOWNLOAD_PID" 2>/dev/null || true; fi
  [[ "$DOWNLOAD_LINE_OPEN" = 0 ]] || printf '\n' >&2
  [[ "$WORK" != "$TEMP_BASE"/localvox-bootstrap.* ]] || rm -rf -- "$WORK"
 }
 trap cleanup EXIT
 local URL="$RELEASE_BASE/releases/download/$TAG/$NAME"
 echo "LocalVox $TAG"
 curl -fsSL "$URL.sha256" > "$WORK/checksum"
 local expected file
 read -r expected file < "$WORK/checksum"
 file="${file%$'\r'}"
 [[ "$expected" =~ ^[a-fA-F0-9]{64}$ && "${file#\*}" = "$NAME" ]] || { echo 'Invalid release checksum file' >&2; return 1; }
 download_bundle() {
  local url="$1" target="$2" headers="$WORK/download.headers"
  local interactive=0 accent='' reset='' started=$SECONDS last=-10 frame=0
  if [[ -t 2 && "${TERM:-}" != dumb ]]; then
   interactive=1
   if [[ -z "${NO_COLOR+x}" ]]; then accent=$'\033[36m'; reset=$'\033[0m'; fi
  fi
  draw_download() {
   local done="$1" seconds=$((SECONDS-started)) bytes=0 total=0 percent filled bar='' size elapsed line i
   if [[ "$done" = 0 && "$interactive" = 0 && $((seconds-last)) -lt 5 ]]; then return; fi
   last=$seconds
   if [[ -f "$target" ]]; then bytes="$(stat -c %s "$target" 2>/dev/null || stat -f %z "$target")"; fi
   if [[ -f "$headers" ]]; then
    total="$(awk '/^HTTP\// {size=0} tolower($1)=="content-length:" {gsub("\r", "", $2); size=$2} END {printf "%.0f\n", size+0}' "$headers")"
   fi
   [[ "$total" =~ ^[0-9]{1,15}$ ]] || total=0
   size="$((bytes/1048576)).$(((bytes%1048576)*10/1048576))"
   printf -v elapsed '%02d:%02d' "$((seconds/60))" "$((seconds%60))"
   if [[ "$total" -gt 0 ]]; then
    percent=$((bytes*100/total)); [[ "$percent" -lt 100 ]] || percent=99
    [[ "$done" = 0 ]] || percent=100
    filled=$((16*percent/100))
    for ((i=0; i<16; i++)); do if [[ "$i" -lt "$filled" ]]; then bar+='━'; else bar+='─'; fi; done
    line="  │ $accent[$bar]$reset $percent% · $size/$((total/1048576)).$(((total%1048576)*10/1048576)) MiB · $elapsed"
   else
    local label=Download frames=('|' '/' '-' '\')
    if [[ "$done" = 1 ]]; then label=Downloaded; elif [[ "$interactive" = 1 ]]; then label="${frames[$((frame%4))]} Download"; fi
    line="  │ $accent$label$reset · $size MiB · $elapsed"
   fi
   if [[ "$interactive" = 1 ]]; then printf '\r%s\033[K' "$line" >&2; DOWNLOAD_LINE_OPEN=1; else printf '%s\n' "$line" >&2; fi
   frame=$((frame+1))
  }
  # Metadata comes from this GET, including the final response after redirects.
  # Observe the private output file, never parse curl's human progress meter.
  curl -fsSL --connect-timeout 20 --speed-time 60 --speed-limit 1 -D "$headers" -o "$target" "$url" 2>"$WORK/download.error" &
  DOWNLOAD_PID=$!
  while kill -0 "$DOWNLOAD_PID" 2>/dev/null; do draw_download 0; sleep 0.2; done
  local status=0
  wait "$DOWNLOAD_PID" || status=$?
  DOWNLOAD_PID=''
  if [[ "$status" != 0 ]]; then
   [[ "$DOWNLOAD_LINE_OPEN" = 0 ]] || { printf '\n' >&2; DOWNLOAD_LINE_OPEN=0; }
   cat "$WORK/download.error" >&2
   return "$status"
  fi
  draw_download 1
  [[ "$DOWNLOAD_LINE_OPEN" = 0 ]] || { printf '\n' >&2; DOWNLOAD_LINE_OPEN=0; }
 }
 download_bundle "$URL" "$WORK/$NAME.part"
 echo '  │ Verifying SHA-256...'
 [[ "$("${SHA[@]}" "$WORK/$NAME.part" | cut -d ' ' -f1 | tr '[:upper:]' '[:lower:]')" = "$(printf '%s' "$expected" | tr '[:upper:]' '[:lower:]')" ]] || { echo 'Release SHA-256 mismatch; no installation was performed' >&2; return 1; }
 echo '  │ Unpacking verified bundle...'
 tar -tzf "$WORK/$NAME.part" > "$WORK/members"
 local name detail
 while IFS= read -r name; do case "$name" in /*|*\\*|*:*|../*|*/../*|*/..) echo 'Unsafe archive path' >&2; return 1;; esac; done < "$WORK/members"
 tar -tvzf "$WORK/$NAME.part" > "$WORK/types"
 while IFS= read -r detail; do case "${detail:0:1}" in -|d) :;; *) echo 'Release archive links/special files are not supported' >&2; return 1;; esac; done < "$WORK/types"
 mkdir "$WORK/bundle"
 tar -xzf "$WORK/$NAME.part" -C "$WORK/bundle"
 [[ -x "$WORK/bundle/localvox-install" && -f "$WORK/bundle/$NATIVE" && -f "$WORK/bundle/bundle.json" ]] || { echo 'Incomplete release; choose a complete newer release.' >&2; return 1; }
 "$WORK/bundle/localvox-install" "${OPTIONS[@]}" --staging-root "$WORK" </dev/tty
)
main "$@"
