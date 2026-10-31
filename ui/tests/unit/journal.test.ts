import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { groupJournal, journalState, mergeJournal, type JournalEntry, type JournalPage } from "../../src/journal-model.ts";

const entry = (id: string, extra: Partial<JournalEntry> = {}): JournalEntry => ({ id, run: "r1", stage: "summary", kind: "log", at: "2026-10-31T07:00:00Z", label: "summary", ...extra });
const page = (events: JournalEntry[], reset = false): JournalPage => ({ events, reset, next_cursor: "next", previous_cursor: "previous", has_more: false, truncated: false, discarded: 0, read_bytes: 0 });

test("a start without live execution is unconfirmed and a completed operation retains its measured duration", () => {
  const events = [entry("start", { kind: "operation_start", operation: "old", at: "2026-10-31T07:00:00Z" })];
  let group = groupJournal(events)[0];
  assert.equal(journalState(group, false).label, "завершение не зафиксировано");
  events.push(entry("end", { kind: "operation_end", operation: "old", at: "2026-10-31T07:00:10Z", elapsed_sec: 9.5 }));
  group = groupJournal(events)[0];
  assert.equal(group.elapsed, 9.5);
  assert.equal(journalState(group, false).label, "операция завершилась");
});

test("the same journal fixture decodes in browser and terminal clients", () => {
  const fixture: JournalPage = JSON.parse(readFileSync(new URL("../../../tests/fixtures/journal.json", import.meta.url), "utf8"));
  const group = groupJournal(fixture.events)[0];
  assert.equal(group.id, "r1/op1"); assert.equal(group.details.length, 35);
  assert.equal(group.command?.args[0].value, "summary");
});
test("replayed pages are idempotent and a new generation discards stale events", () => {
  assert.deepEqual(mergeJournal([entry("a")], page([entry("a"), entry("b")])).entries.map((e) => e.id), ["a", "b"]);
  assert.deepEqual(mergeJournal([entry("a")], page([entry("b")], true)).entries.map((e) => e.id), ["b"]);
});
test("35 informational lines collapse independently of operation start and end", () => {
  const group = groupJournal([entry("start", { operation: "o", kind: "operation_start" }), ...Array.from({ length: 35 }, (_, i) => entry(String(i), { operation: "o", text: `line ${i}` })), entry("end", { operation: "o", kind: "operation_end", text: "finished" })])[0];
  assert.equal(group.details.length, 35);
  assert.equal(journalState(group).label, "операция завершилась");
  assert.notEqual(journalState(group).label, "этап завершён");
});
test("run identity and unknown time are not inferred from timestamp equality", () => {
  assert.equal(groupJournal([entry("a", { run: "r1" }), entry("b", { run: "r2" })]).length, 2);
  assert.equal(journalState(groupJournal([entry("failed", { status: "invalid" })])[0]).cls, "err");
});
test("cache has a hard item bound and reports eviction separately", () => {
  const merged = mergeJournal([], page(Array.from({ length: 6000 }, (_, i) => entry(String(i)))));
  assert.equal(merged.entries.length, 5000);
  assert.equal(merged.evicted, 1000);
});
test("requested previous pages survive a full cache and replay adds nothing", () => {
  const old = Array.from({ length: 5000 }, (_, i) => entry(String(i + 100)));
  const earlier = page(Array.from({ length: 100 }, (_, i) => entry(String(i))));
  const result = mergeJournal(old, earlier, true);
  assert.equal(result.entries[0].id, "0");
  assert.equal(result.entries.at(-1)?.id, "4999");
  assert.equal(result.evictedTail, 100);
  assert.equal(mergeJournal(result.entries, earlier, true).added, 0);
});
test("cache also obeys the serialized byte budget", () => {
  const result = mergeJournal([], page(Array.from({ length: 1500 }, (_, i) => entry(String(i), { text: "ж".repeat(3000) }))));
  assert.ok(result.entries.length < 1500);
  assert.ok(result.entries.reduce((sum, value) => sum + new TextEncoder().encode(JSON.stringify(value)).length, 0) <= 4 * 1024 * 1024);
});
