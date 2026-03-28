#!/usr/bin/env bash
# Явно: Linux aarch64 (vosk-linux-aarch64). Аргументы пробрасываются в setup-vosk.sh.
exec "$(dirname "$0")/setup-vosk.sh" --preset=linux-aarch64 "$@"
