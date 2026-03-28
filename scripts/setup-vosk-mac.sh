#!/usr/bin/env bash
# Явно: macOS (vosk-osx). Аргументы пробрасываются в setup-vosk.sh.
exec "$(dirname "$0")/setup-vosk.sh" --preset=darwin "$@"
