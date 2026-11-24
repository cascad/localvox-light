import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { groupJournal, journalSeconds } from "../../src/journal-model.ts";
import { outlineJournal, visibleOutline, journalCopy } from "../../src/journal-outline.ts";

const fixture = JSON.parse(readFileSync(new URL("../../../tests/fixtures/journal-outline.json", import.meta.url), "utf8"));
// The fixture is shared with the native TUI tests.
test("mixed recording/final history shows one result per stage and retains nested clocks", () => {
  const raw = groupJournal(fixture.events), { execution, stages } = fixture.progress;
  const all = outlineJournal(raw, execution, stages);
  const visible = visibleOutline(all, new Set(), new Set(), "");
  assert.deepEqual(visible.filter(g => g.kind === "context").map(g => g.label), ["Во время записи", "После остановки"]);
  assert.deepEqual(visible.filter(g => g.kind === "stage").map(g => g.stage), ["transcribe", "punctuation", "summary"]);
  assert.equal(visible.filter(g => g.kind === "operation").length, 0);
  assert.ok(visible[0].issues > 0);
  const summary = all.find(g => g.kind === "stage" && g.stage === "summary")!;
  assert.equal(journalSeconds(summary, execution.run, stages, false, 0), 58);
  const answer = all.find(g => g.operation === "answer")!;
  assert.equal(answer.elapsed, 54);
  assert.equal(all.find(g => g.operation === "early-punctuation")!.elapsed, 24);
  assert.ok(all.find(g => g.operation === "early-punctuation")!.depth > all.find(g => g.operation === "early-prefix")!.depth);
  const technical = all.find(g => g.id === "final-run/summary-worker")!;
  assert.match(journalCopy(technical), /--worker-phase summary/);
  assert.match(journalCopy(technical), /summary-worker/);
});
test("filter reveals ancestry without changing folds, including an out-of-page parent", () => {
  const expanded = new Set<string>(), collapsed = new Set<string>();
  for (const events of [fixture.events, fixture.events.filter((e: { operation: string }) => e.operation !== "summary-worker")]) {
    const all = outlineJournal(groupJournal(events), fixture.progress.execution, fixture.progress.stages);
    const visible = visibleOutline(all, expanded, collapsed, "Проверяемая диагностическая");
    assert.deepEqual(visible.map(g => g.kind), ["context", "stage", "operation"]);
    assert.equal(visible.at(-1)!.id, "final-run/answer");
  }
  assert.equal(expanded.size + collapsed.size, 0);
});
test("unknown legacy context and cyclic parent links never drop operations", () => {
  const old = fixture.events.map((e: object) => ({ ...e, scope: undefined, role: undefined }));
  assert.ok(outlineJournal(groupJournal(old)).filter(g => g.kind === "context").every(g => g.label.includes("контекст не записан")));
  const raw = groupJournal(["a", "b"].map((id, i) => ({ id, run: "r", stage: "summary", kind: "log", at: "", label: id, operation: id, parent: i ? "a" : "b" })));
  const ops = outlineJournal(raw).filter(g => g.kind === "operation");
  assert.equal(ops.length, 2); assert.ok(ops.every(g => g.note));
});
test("the import process wrapper stays technical instead of adding a prepare step", () => {
  const raw = groupJournal([
    { id: "worker", run: "r", scope: "import", role: "worker", operation: "worker", stage: "prepare", kind: "operation_start", label: "process", at: "" },
    { id: "download", run: "r", scope: "import", operation: "download", parent: "worker", stage: "download", kind: "operation_start", label: "download", at: "" },
  ]);
  const all = outlineJournal(raw);
  assert.deepEqual(all.filter(g => g.kind === "stage").map(g => g.stage), ["download"]);
  assert.equal(all.find(g => g.id === "r/download")!.parent, "r/import/stage:download");
  assert.equal(all.find(g => g.id === "r/worker")!.parent, "context:r/import/details");
});
