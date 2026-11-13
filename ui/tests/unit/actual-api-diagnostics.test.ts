import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

test("actual API runner preserves browser startup failures", () => {
  const root = mkdtempSync(join(tmpdir(), "localvox-browser-diagnostics-"));
  const script = fileURLToPath(new URL("../integration/actual-api.mjs", import.meta.url));
  try {
    const result = spawnSync(process.execPath, [script], {
      cwd: root,
      env: {
        ...process.env,
        LOCALVOX_CROSS_ROOT: root,
        LOCALVOX_CROSS_OUTPUT: root,
        LOCALVOX_CROSS_URL: "http://127.0.0.1:1",
        PLAYWRIGHT_BROWSERS_PATH: join(root, "missing-browser"),
      },
      encoding: "utf8",
      timeout: 15_000,
      windowsHide: true,
    });
    assert.ifError(result.error);
    assert.equal(result.signal, null);
    assert.equal(result.status, 1, result.stderr);
    assert.match(readFileSync(join(root, "browser-error.txt"), "utf8"), /Executable doesn't exist/);
  } finally {
    assert.equal(resolve(root, ".."), resolve(tmpdir()));
    rmSync(root, { recursive: true, force: true });
  }
});
