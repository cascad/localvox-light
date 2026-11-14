#!/usr/bin/env bash
# Invoked only for components in a confirmed installation plan.
set -euo pipefail
ONLY='' INSTALL=''
while [[ $# -gt 0 ]]; do
 case "$1" in --only) ONLY="$2"; shift 2;; --install-dir) INSTALL="$2"; shift 2;; *) echo 'Usage: setup-tools.sh --only ffmpeg,mpv,jq,unzip --install-dir PATH' >&2; exit 2;; esac
done
[[ -n "$ONLY" && -n "$INSTALL" ]] || exit 2
IFS=',' read -r -a TOOLS <<<"$ONLY"
MISSING=()
for tool in "${TOOLS[@]}"; do
 case "$tool" in ffmpeg|yt-dlp|mpv|jq|unzip) :;; *) echo "Unsupported tool: $tool" >&2; exit 2;; esac
 MISSING+=("$tool") # The native planner already checked presence and supported versions.
done
[[ ${#MISSING[@]} -gt 0 ]] || exit 0
if [[ "$(uname -s)" = Darwin ]]; then
 BREW=''
 for candidate in /opt/homebrew/bin/brew /usr/local/bin/brew; do [[ ! -x "$candidate" ]] || BREW="$candidate"; done
 if [[ -z "$BREW" ]] && command -v brew >/dev/null; then BREW="$(command -v brew)"; fi
 if [[ -z "$BREW" ]]; then
  mkdir -p "$INSTALL/cache/installers"
  script="$INSTALL/cache/installers/homebrew-install.sh"
  curl -fsSL --proto '=https' https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh -o "$script.part"
  mv "$script.part" "$script"
  /bin/bash "$script"
  for candidate in /opt/homebrew/bin/brew /usr/local/bin/brew; do [[ ! -x "$candidate" ]] || BREW="$candidate"; done
 fi
 [[ -n "$BREW" ]] || { echo 'Homebrew installation did not complete' >&2; exit 1; }
 PACKAGES=()
 for tool in "${MISSING[@]}"; do [[ "$tool" = unzip ]] || PACKAGES+=("$tool"); done
 [[ ${#PACKAGES[@]} = 0 ]] || "$BREW" install "${PACKAGES[@]}"
elif [[ "$(uname -s)" = Linux ]]; then
 SUDO=(); if [[ "$(id -u)" != 0 ]]; then SUDO=(sudo); fi
 if command -v apt-get >/dev/null; then "${SUDO[@]}" apt-get update; "${SUDO[@]}" apt-get install -y "${MISSING[@]}"
 elif command -v dnf >/dev/null; then "${SUDO[@]}" dnf install -y "${MISSING[@]}"
 elif command -v pacman >/dev/null; then "${SUDO[@]}" pacman -S --needed --noconfirm "${MISSING[@]}"
 elif command -v zypper >/dev/null; then "${SUDO[@]}" zypper --non-interactive install "${MISSING[@]}"
 else echo 'No supported package manager (apt/dnf/pacman/zypper)' >&2; exit 1; fi
else echo 'Unsupported OS' >&2; exit 1; fi
