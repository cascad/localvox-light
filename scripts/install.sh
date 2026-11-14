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
 if [[ "$UPDATE" = 0 && "$TAG" = latest && -x "$INSTALL/localvox-install" ]]; then
  if "$INSTALL/localvox-install" --bundle-check --install-dir "$INSTALL" 2>/dev/null; then
   echo 'LocalVox files are intact; download skipped.'
   "$INSTALL/localvox-install" "${OPTIONS[@]}" </dev/tty
   return
  fi
 fi
 command -v curl >/dev/null || { echo 'curl is required' >&2; return 2; }
 command -v tar >/dev/null || { echo 'tar is required' >&2; return 2; }
 local SHA
 if command -v shasum >/dev/null; then SHA=(shasum -a 256); elif command -v sha256sum >/dev/null; then SHA=(sha256sum); else echo 'shasum or sha256sum is required' >&2; return 2; fi
 if [[ "$TAG" = latest ]]; then
  local resolved prefix="$RELEASE_BASE/releases/tag/"
  resolved="$(curl -fsSIL -w '%{url_effective}' "$RELEASE_BASE/releases/latest" | tail -n 1)"
  resolved="${resolved%$'\r'}"
  [[ "$resolved" = "$prefix"* ]] || { echo 'Could not resolve latest release tag' >&2; return 1; }
  TAG="${resolved#"$prefix"}"
 fi
 [[ "$TAG" =~ ^v[0-9][A-Za-z0-9._-]*$ ]] || { echo 'Invalid release tag' >&2; return 2; }
 local NAME="localvox-$TARGET.tar.gz" WORK
 local TEMP_BASE
 TEMP_BASE="$(cd "${TMPDIR:-/tmp}" && pwd -P)"
 WORK="$(mktemp -d "$TEMP_BASE/localvox-bootstrap.XXXXXXXX")"
 trap '[[ "$WORK" = "$TEMP_BASE"/localvox-bootstrap.* ]] && rm -rf -- "$WORK"' EXIT
 local URL="$RELEASE_BASE/releases/download/$TAG/$NAME"
 echo "Downloading LocalVox $TAG..."
 curl -fsSL "$URL.sha256" > "$WORK/checksum"
 local expected file
 read -r expected file < "$WORK/checksum"
 file="${file%$'\r'}"
 [[ "$expected" =~ ^[a-fA-F0-9]{64}$ && "${file#\*}" = "$NAME" ]] || { echo 'Invalid release checksum file' >&2; return 1; }
 curl -fSL "$URL" > "$WORK/$NAME.part"
 [[ "$("${SHA[@]}" "$WORK/$NAME.part" | cut -d ' ' -f1 | tr '[:upper:]' '[:lower:]')" = "$(printf '%s' "$expected" | tr '[:upper:]' '[:lower:]')" ]] || { echo 'Release SHA-256 mismatch; no installation was performed' >&2; return 1; }
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
