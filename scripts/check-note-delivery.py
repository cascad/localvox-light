"""Isolated NOTE-002/004 audit. No real slots, servers, network, or user notes.

Build first: cargo build --locked -p localvox-light-integrations --bin localvox-note
Run: python scripts/check-note-delivery.py
Reports ambiguous retry duplicates as an observation, not a required behavior.
"""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
SERVER = r'''
import json, pathlib, sys, time
mode, state_dir = sys.argv[1:]
root = pathlib.Path(state_dir)
for line in sys.stdin:
    msg = json.loads(line)
    if msg.get("method") == "initialize":
        result = {"protocolVersion": "2024-11-05", "capabilities": {},
                  "serverInfo": {"name": "isolated-audit", "version": "1"}}
    elif msg.get("method") == "tools/call":
        attempts = root / "attempts.txt"
        count = int(attempts.read_text()) + 1 if attempts.exists() else 1
        attempts.write_text(str(count))
        if mode == "silent":
            time.sleep(10)
            continue
        if mode == "reject-once" and count == 1:
            result = {"isError": True, "content": [{"type": "text", "text": "fixture refusal"}]}
        else:
            with (root / "destination.txt").open("a", encoding="utf-8") as f:
                f.write(msg["params"]["arguments"]["content"] + "\n")
            if mode == "lost-ack" and count == 1:
                sys.exit(0)
            result = {"content": [{"type": "text", "text": "fixture stored"}]}
    else:
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}), flush=True)
'''


def main():
    exe = ROOT / "target" / "debug" / ("localvox-note.exe" if os.name == "nt" else "localvox-note")
    if not exe.is_file():
        raise SystemExit("Build localvox-note first (see this script's docstring).")
    env = {k: v for k, v in os.environ.items() if not k.startswith("LOCALVOX_")}
    env["LOCALVOX_MCP_TIMEOUT_SEC"] = "1"
    env["PYTHONUTF8"] = "1"
    observations = {}
    with tempfile.TemporaryDirectory(prefix="localvox-note-audit-") as tmp:
        root = Path(tmp)
        (root / ".env").write_text("", encoding="utf-8")
        server = root / "server.py"
        server.write_text(SERVER, encoding="utf-8")

        def invoke(config, text="synthetic audit note"):
            path = root / "slots.toml"
            path.write_text(config, encoding="utf-8")
            return subprocess.run([str(exe), "--config", str(path), text], cwd=root,
                                  env=env, capture_output=True, timeout=6)

        # A destination file and a write failure use only this temporary tree.
        dest = root / "notes.md"
        config = '[slots.audit]\ntype="files"\npath=' + json.dumps(str(dest)) + '\n'
        assert invoke(config).returncode == 0
        assert "synthetic audit note" in dest.read_text(encoding="utf-8")
        assert invoke(config, "second audit note").returncode == 0
        assert "second audit note" in dest.read_text(encoding="utf-8")
        observations["file_append"] = "two notes preserved"
        blocked = root / "blocked"
        blocked.write_text("file, not directory")
        result = invoke(config.replace(json.dumps(str(dest)), json.dumps(str(blocked / "notes.md"))))
        assert result.returncode != 0
        observations["file_failure"] = "nonzero exit; no successful acknowledgement"

        for mode in ("reject-once", "lost-ack", "silent"):
            state = root / mode
            state.mkdir()
            config = ('[slots.audit]\ntype="mcp"\ncommand=' + json.dumps(sys.executable)
                      + '\nargs=' + json.dumps(["-X", "utf8", str(server), mode, str(state)])
                      + '\ntool="append_note"\ntext_arg="content"\n')
            first = invoke(config)
            assert first.returncode != 0, (mode, first.stdout, first.stderr)
            report = {"first_exit": first.returncode, "first_reported_success": False}
            if mode != "silent":
                retry = invoke(config)
                assert retry.returncode == 0, retry.stderr
                lines = (state / "destination.txt").read_text(encoding="utf-8").splitlines()
                report.update(retry_exit=retry.returncode, destination_copies=len(lines),
                              attempts=int((state / "attempts.txt").read_text()))
                if mode == "reject-once":
                    assert len(lines) == 1
            else:
                assert b"did not answer" in first.stderr, first.stderr
                report["timeout_reported"] = True
            observations[mode] = report
    print(json.dumps(observations, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
