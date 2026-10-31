import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import type { Session, Jobs, SettingsDoc, AskRecord } from "../../src/api.ts";

test("shared workspace fixture preserves counts, document identity, providers and unset settings", () => {
  const fixture = JSON.parse(readFileSync(new URL("../../../tests/fixtures/workspace.json", import.meta.url), "utf8")) as {
    sessions: Session[]; jobs: Jobs; settings: SettingsDoc; ask: AskRecord;
    transcript: { lines: { start_sec: number; end_sec: number; text: string }[] };
  };
  assert.equal(typeof fixture.jobs.running, "number");
  assert.equal(fixture.jobs.running, 1);
  assert.ok(fixture.sessions[0].name && fixture.sessions[0].title);
  assert.ok(fixture.transcript.lines.every((line) => typeof line.text === "string" && line.end_sec >= line.start_sec));
  assert.equal(fixture.ask.provider, "ollama");
  assert.ok(fixture.settings.settings.every((setting) => setting.applies === "restart"));
  assert.deepEqual(fixture.settings.extra, []);
});
