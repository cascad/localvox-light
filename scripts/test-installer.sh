#!/usr/bin/env bash
# Isolated fixtures only: never installs tools/models or starts services.
set -euo pipefail
cd "$(dirname "$0")/.."
export PYTHONDONTWRITEBYTECODE=1
mkdir -p target/tmp
export TMPDIR="$PWD/target/tmp"
if [[ "${1:-}" != --skip-rust ]]; then
 cargo build --locked -p localvox-setup-ui --example terminal-capture
fi
terminal_runner="${CARGO_TARGET_DIR:-target}/debug/examples/terminal-capture"
if [[ -x "$terminal_runner" ]]; then
 export LOCALVOX_TEST_PTY_RUNNER="$(cd "$(dirname "$terminal_runner")" && pwd)/terminal-capture"
fi
python3 -m unittest discover -s scripts/tests -p test_bootstrap_delivery.py -v
python3 -m unittest discover -s scripts/tests -p test_model_sources.py -v
python3 -m unittest discover -s scripts/tests -p test_model_storage_delivery.py -v
python3 -m unittest discover -s scripts/tests -p test_release_delivery.py -v
if [[ "${1:-}" != --skip-rust ]]; then
 export RUSTFLAGS="${RUSTFLAGS:-} -L native=$PWD/vosk-lib"
 export LD_LIBRARY_PATH="$PWD/vosk-lib:${LD_LIBRARY_PATH:-}"
 export DYLD_LIBRARY_PATH="$PWD/vosk-lib:${DYLD_LIBRARY_PATH:-}"
 cargo test --locked -p localvox-light-core --lib env_file:: -- --test-threads=2
 cargo test --locked -p localvox-light --lib --bins --test setup --test install -- --test-threads=2
 cargo test --locked -p localvox-setup-ui -- --test-threads=1
fi
