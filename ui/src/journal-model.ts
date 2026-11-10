export interface DisplayArgument { name: string; value?: string | null }
export interface JournalEntry {
  elapsed_sec?: number | null;
  id: string; run: string; operation?: string | null; parent?: string | null;
  stage: string; kind: string; at: string; label: string; status?: string | null;
  command?: { program: string; args: DisplayArgument[] } | null;
  text?: string | null; done?: number | null; total?: number | null;
}
export interface JournalPage {
  events: JournalEntry[]; next_cursor: string; previous_cursor: string; has_more: boolean;
  reset: boolean; truncated: boolean; discarded: number; read_bytes: number; dropped_verbose?: number;
}
export interface JournalGroup {
  startedAt?: string; endedAt?: string; elapsed?: number | null;
  id: string; run: string; stage: string; label: string; parent?: string | null;
  command?: JournalEntry["command"]; ended: boolean; state?: string | null;
  details: JournalEntry[]; lastAt: string;
}
export const MAX_JOURNAL_EVENTS = 5000;
const MAX_CACHE_BYTES = 4 * 1024 * 1024;

/** Cursor identity, rather than timestamps, makes page retries idempotent. */
export function mergeJournal(old: JournalEntry[], page: JournalPage, prepend = false) {
  prepend = prepend && !page.reset;
  const seen = new Set<string>();
  const retained = page.reset ? [] : old;
  const oldIds = new Set(retained.map((entry) => entry.id));
  const added = page.events.filter((entry) => !oldIds.has(entry.id)).length;
  const combined = (prepend ? [...page.events, ...retained] : [...retained, ...page.events]).filter((entry) => {
    if (seen.has(entry.id)) return false;
    seen.add(entry.id); return true;
  });
  let bytes = 0;
  let count = 0;
  const encoder = new TextEncoder();
  for (let n = 0; n < combined.length && n < MAX_JOURNAL_EVENTS; n++) {
    const i = prepend ? n : combined.length - 1 - n;
    bytes += encoder.encode(JSON.stringify(combined[i])).length;
    if (bytes > MAX_CACHE_BYTES) break;
    count++;
  }
  const entries = prepend ? combined.slice(0, count) : combined.slice(combined.length - count);
  return { entries, added, evicted: combined.length - count,
    evictedHead: prepend ? 0 : combined.length - count,
    evictedTail: prepend ? combined.length - count : 0 };
}

export function groupJournal(events: JournalEntry[]): JournalGroup[] {
  const groups = new Map<string, JournalGroup>();
  for (const entry of events) {
    const id = `${entry.run}/${entry.operation ?? `stage:${entry.stage}`}`;
    let group = groups.get(id);
    if (!group) {
      group = { id, run: entry.run, stage: entry.stage, label: entry.label || entry.stage, parent: entry.parent,
        ended: false, details: [], lastAt: entry.at };
      groups.set(id, group);
    }
    if (entry.command) group.command = entry.command;
    if (entry.parent) group.parent = entry.parent;
    if (entry.kind === "operation_start") { group.label = entry.label; group.state = "running"; group.startedAt = entry.at; }
    if (entry.kind === "operation_end") { group.ended = true; group.endedAt = entry.at; group.elapsed = entry.elapsed_sec; }
    if (entry.status) group.state = entry.status;
    if (entry.text && entry.kind !== "operation_end") group.details.push(entry);
    group.lastAt = entry.at;
  }
  return [...groups.values()];
}

export function journalActive(group: JournalGroup, execution?: {
  state: string; run?: string | null; operation?: string | null; current?: string | null;
} | null) {
  return execution?.state === "running" && !!execution.run && !!execution.operation
    && group.run === execution.run
    && (group.id === `${execution.run}/${execution.operation}`
      || group.parent === execution.operation && group.stage === execution.current);
}

export function journalState(group: JournalGroup, active?: boolean) {
  if (group.state === "cancelled") return { symbol: "■", label: "отменено", cls: "mute" };
  if (group.state === "failed" || group.state === "invalid") return { symbol: "!", label: group.state === "invalid" ? "не подтверждён" : "ошибка", cls: "err" };
  if (group.state === "done") return { symbol: "✓", label: "этап завершён", cls: "done" };
  if (group.state === "skipped") return { symbol: "–", label: "пропущено", cls: "mute" };
  if (group.state === "waiting") return { symbol: "·", label: "не начиналось", cls: "mute" };
  if (group.state === "queued") return { symbol: "·", label: "в очереди", cls: "mute" };
  if (group.state === "blocked") return { symbol: "·", label: "ждёт предыдущий этап", cls: "mute" };
  if (group.ended) return { symbol: "·", label: "операция завершилась", cls: "mute" };
  if (group.state === "running" && active === false) return { symbol: "·", label: "завершение не зафиксировано", cls: "mute" };
  return { symbol: group.state === "running" ? "●" : "·", label: group.state === "running" ? "идёт" : "события операции", cls: group.state === "running" ? "busy" : "mute" };
}
