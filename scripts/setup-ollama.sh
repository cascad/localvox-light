#!/usr/bin/env bash
# Official macOS application handoff; no custom daemon/service installation.
set -euo pipefail
[[ $# = 2 && "$1" = --cache && -n "$2" ]] || { echo 'Usage: setup-ollama.sh --cache PATH' >&2; exit 2; }
[[ "$(uname -s)" = Darwin ]] || { echo 'Install Ollama from https://ollama.com/download, then run --setup again.' >&2; exit 1; }
if command -v ollama >/dev/null || [[ -f /Applications/Ollama.app/Contents/Resources/ollama || -f "$HOME/Applications/Ollama.app/Contents/Resources/ollama" ]]; then
 echo 'Ollama already exists; open its application, then return to the wizard.'; exit 0
fi
mkdir -p "$2"
if [[ ! -f "$2/Ollama.dmg" ]] || ! hdiutil verify "$2/Ollama.dmg" >/dev/null 2>&1; then
 curl -fL --proto '=https' https://ollama.com/download/Ollama.dmg -o "$2/Ollama.dmg.part"
 mv "$2/Ollama.dmg.part" "$2/Ollama.dmg"
 hdiutil verify "$2/Ollama.dmg" >/dev/null
fi
open "$2/Ollama.dmg"
echo 'In the official window, drag Ollama to Applications and launch it. Return to the wizard when ready.'
