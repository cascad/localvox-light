#!/usr/bin/env bash
# Явно: Linux x86_64 (vosk-linux-x86_64). Аргументы пробрасываются в setup-vosk.sh.
exec "$(dirname "$0")/setup-vosk.sh" --preset=linux-x86_64 "$@"
