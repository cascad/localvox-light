import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { groupJournal, journalActive, journalSeconds, journalDuration, journalStageTiming, journalState, mergeJournal, type JournalEntry, type JournalPage } from "../../src/journal-model.ts";

const entry = (id: string, extra: Partial<JournalEntry> = {}): JournalEntry => ({ id, run: "r1", stage: "summary", kind: "log", at: "2026-10-31T07:00:00Z", label: "summary", ...extra });
const page = (events: JournalEntry[], reset = false): JournalPage => ({ events, reset, next_cursor: "next", previous_cursor: "previous", has_more: false, truncated: false, discarded: 0, read_bytes: 0 });

test("stage duration comes from the exact attempt even when the tail has no start", () => {
  const end="2026-11-10T22:28:02.718788300+03:00";
  const group=groupJournal([
    entry("heartbeat",{kind:"stage",status:"running",at:"2026-11-10T22:25:14+03:00"}),
    entry("done",{kind:"stage",status:"done",at:end}),
  ])[0];
  const stage={stage:"summary",state:"done",started_at:"2026-11-10T22:21:43.529131700+03:00",ended_at:end};
  assert.equal(group.startedAt,undefined);
  assert.equal(journalDuration(journalSeconds(group,"r1",[stage],false,0)),"6:19");
  for (const wrong of [{...stage,ended_at:"2026-11-10T22:28:03+03:00"},{...stage,started_at:null},{...stage,started_at:"bad"},{...stage,started_at:"2026-11-10T22:29:00+03:00"}]) {
    assert.equal(journalDuration(journalSeconds(group,"r1",[wrong],false,0)),"время неизвестно");
  }
  assert.equal(journalStageTiming(group,"old-run",[stage]),undefined);
  assert.equal(journalStageTiming(group,null,[stage]),undefined);
  const child=groupJournal([entry("child",{operation:"op",kind:"operation_end",elapsed_sec:0.9353011,status:"done",at:end})])[0];
  assert.equal(journalStageTiming(child,"r1",[stage]),undefined);
  assert.equal(journalDuration(journalSeconds(child,"r1",[stage],false,0)),"<1 с");
});

test("running stage uses its real start and refuses a later retry or lost activity", () => {
  const group=groupJournal([entry("beat",{kind:"stage",status:"running",at:"2026-10-31T07:00:03Z"})])[0];
  const stage={stage:"summary",state:"running",started_at:"2026-10-31T07:00:00Z"};
  const execution={run:"r1",operation:"worker",state:"running",current:"summary"};
  const now=Date.parse("2026-10-31T07:00:05Z");
  assert.equal(journalActive(group,execution,[stage]),true);
  assert.equal(journalDuration(journalSeconds(group,"r1",[stage],true,now)),"0:05");
  assert.equal(journalDuration(journalSeconds(group,"r1",[stage],false,now)),"время неизвестно");
  const retry={...stage,started_at:"2026-10-31T07:00:04Z"};
  assert.equal(journalActive(group,execution,[retry]),false);
  assert.equal(journalStageTiming(group,"r1",[retry]),undefined);
  assert.equal(journalActive(group,{...execution,state:"done"},[stage]),false);
});

test("journal distinguishes positive subsecond work from a measured zero or unknown time", () => {
  assert.equal(journalDuration(0.9353011),"<1 с");
  assert.equal(journalDuration(0.00001),"<1 с");
  assert.equal(journalDuration(0),"0:00");
  assert.equal(journalDuration(1),"0:01");
  for (const value of [NaN,Infinity,-1]) assert.equal(journalDuration(value),"время неизвестно");
});

test("grouping belongs to the current worker and keeps its own time and error", () => {
  const events = [entry("start", {stage:"transcribe",operation:"group",parent:"worker",kind:"operation_start",label:"Группировка голосов"})];
  let group = groupJournal(events)[0];
  const execution = {state:"running",run:"r1",operation:"worker",current:"transcribe"};
  assert.equal(journalActive(group,execution),true);
  for (const change of [{run:"old"},{operation:"retry"},{state:"failed"},{current:"summary"}]) {
    assert.equal(journalActive(group,{...execution,...change}),false);
  }
  events.push(entry("error",{stage:"transcribe",operation:"group",parent:"worker",kind:"log",status:"failed",text:"invalid voice embedding"}));
  events.push(entry("end",{stage:"transcribe",operation:"group",parent:"worker",kind:"operation_end",status:"failed",elapsed_sec:3.5}));
  group=groupJournal(events)[0];
  assert.equal(group.elapsed,3.5);
  assert.equal(journalState(group,false).label,"ошибка");
  assert.equal(group.details[0].text,"invalid voice embedding");
});

test("cancelled operations retain their terminal reason without becoming successful or failed", () => {
  const group = groupJournal([
    entry("start", { operation: "cancelled", kind: "operation_start" }),
    entry("end", { operation: "cancelled", kind: "operation_end", status: "cancelled", elapsed_sec: 2.5 }),
  ])[0];
  assert.equal(group.elapsed, 2.5);
  assert.equal(journalState(group, false).label, "отменено");
  assert.equal(journalState(group, false).cls, "mute");
});

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
