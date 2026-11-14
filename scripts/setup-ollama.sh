#!/usr/bin/env bash
# Official installer, only after the LocalVox plan was confirmed.
set -euo pipefail
[[ $# = 2 && "$1" = --cache && -n "$2" ]] || { echo 'Usage: setup-ollama.sh --cache PATH' >&2; exit 2; }
if command -v ollama >/dev/null || [[ -f /Applications/Ollama.app/Contents/Resources/ollama || -f "$HOME/Applications/Ollama.app/Contents/Resources/ollama" ]]; then
 echo 'Ollama already exists; keeping the installed program.'; exit 0
fi
mkdir -p "$2"
curl -fsSL --proto '=https' https://ollama.com/install.sh -o "$2/install.sh.part"
mv "$2/install.sh.part" "$2/install.sh"
sh "$2/install.sh"
