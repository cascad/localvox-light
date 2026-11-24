import { type JournalGroup, type JournalStageClock } from "./journal-model.ts";
import type { StageStatus } from "./api";

export type OutlineKind = "context" | "stage" | "skipped" | "details" | "operation";
export interface OutlineGroup extends JournalGroup {
  kind: OutlineKind; depth: number; children: number; issues: number; defaultOpen: boolean;
  sourceParent?: string | null; note?: string | null; done?: number | null; total?: number | null;
}
export interface OutlineStage extends JournalStageClock {
  updated_at?: string | null; note?: string | null; done?: number | null; total?: number | null;
  files?: string[]; artifact?: StageStatus["artifact"];
}
const chain = ["download", "extract", "transcribe", "punctuation", "refine", "cleanup", "summary"];
export function stageName(stage: string) {
  const names: Record<string, string> = { download: "Скачивание источника", extract: "Подготовка аудио", transcribe: "Расшифровка",
    punctuation: "Пунктуация", refine: "Правка текста", cleanup: "Очистка текста", summary: "Сводка" };
  return Object.hasOwn(names, stage) ? names[stage] : stage;
}
const contextId = (run: string, scope?: string | null) => `context:${run}/${scope ?? "unknown"}`;
const stageId = (run: string, scope: string | null | undefined, stage: string) => `${run}/${scope != null ? `${scope}/` : ""}stage:${stage}`;
const worker = (g: JournalGroup) => g.role === "worker" || g.command?.program === "localvox-process";
const phaseWrapper = (g: JournalGroup) => worker(g) && !chain.includes(g.stage);
function node(group: Partial<OutlineGroup> & { id: string }): OutlineGroup {
  return { run: "", stage: "", label: "", details: [], lastAt: "", ended: false,
    kind: "operation", depth: 0, children: 0, issues: 0, defaultOpen: false, ...group };
}
export function journalCopy(group: JournalGroup) {
  return [`Идентификатор: ${group.id}`, `Запуск ${group.run} · этап ${group.stage}`, `${group.label} · ${group.ended && !["failed", "invalid", "done", "cancelled"].includes(group.state ?? "") ? "операция завершилась" : group.state ?? "события"} · ${group.lastAt}`,
    group.elapsed == null ? "" : `Длительность ${group.elapsed.toFixed(3)} с`,
    group.command ? [group.command.program, ...group.command.args.map(a => `${a.name}${a.value == null ? "" : ` ${a.value}`}`)].join(" ") : "",
    ...group.details.map(e => e.text ?? "")].filter(Boolean).join("\n");
}

/** A page can omit parents; no names/UUID patterns are used to guess context. */
export function outlineJournal(raw: JournalGroup[], execution?: { run?: string | null } | null, stages: readonly OutlineStage[] = []) {
  const run = execution?.run ?? (stages.length ? "@planned" : undefined);
  const scope = raw.findLast(g => g.run === run)?.scope;
  const nodes: OutlineGroup[] = [], byId = new Map<string, OutlineGroup>();
  const add = (g: OutlineGroup) => { nodes.push(g); byId.set(g.id, g); return g; };
  const context = (run: string, scope?: string | null) => {
    const id = contextId(run, scope);
    return byId.get(id) ?? add(node({ id, run, scope, kind: "context",
      label: run === "@planned" ? "План обработки" : scope === "recording" ? "Во время записи" : scope === "final" ? "После остановки" : scope === "import" ? "Импорт" : `Проход ${nodes.filter(n => n.kind === "context").length + 1} · контекст не записан`,
      defaultOpen: scope !== "recording",
    }));
  };
  const stage = (run: string, scope: string | null | undefined, name: string) => {
    context(run, scope);
    const id = stageId(run, scope, name);
    return byId.get(id) ?? add(node({ id, run, scope, stage: name, label: stageName(name),
      kind: "stage", parent: contextId(run, scope), state: "events" }));
  };
  for (const g of raw) context(g.run, g.scope);
  if (run != null) context(run, scope);
  for (const g of raw) {
    const header = context(g.run, g.scope);
    if (Number.isFinite(Date.parse(g.firstAt ?? "")) && (!Number.isFinite(Date.parse(header.firstAt ?? "")) || Date.parse(g.firstAt!) < Date.parse(header.firstAt!))) header.firstAt = g.firstAt;
    if (Number.isFinite(Date.parse(g.lastAt)) && (!Number.isFinite(Date.parse(header.lastAt)) || Date.parse(g.lastAt) > Date.parse(header.lastAt))) header.lastAt = g.lastAt;
  }
  for (const g of raw) {
    if (phaseWrapper(g)) continue;
    const s = stage(g.run, g.scope, g.stage);
    if (g.operation == null) Object.assign(s, { state: g.scope === "recording" ? "events" : g.state,
      lastAt: g.lastAt, details: [...g.details], startedAt: g.startedAt, endedAt: g.endedAt });
  }
  if (run != null && scope !== "recording") for (const s of stages) {
    const start = Date.parse(s.started_at ?? ""), end = Date.parse(s.ended_at ?? "");
    Object.assign(stage(run, scope, s.stage), { state: s.state, startedAt: s.started_at ?? undefined,
      endedAt: s.ended_at ?? undefined, lastAt: s.ended_at ?? s.updated_at ?? "", note: s.note, done: s.done, total: s.total,
      elapsed: Number.isFinite(start) && end >= start ? (end - start) / 1000 : undefined });
  }
  const workers = new Map(raw.filter(worker)
    .map(g => [g.id, phaseWrapper(g) ? contextId(g.run, g.scope) : stageId(g.run, g.scope, g.stage)]));
  const rawById = new Map(raw.map(g => [g.id, g]));
  const intervals = new Map<string, [number, number][] | null>();
  for (const g of raw) if (workers.has(g.id)) {
    const id = contextId(g.run, g.scope), start = Date.parse(g.startedAt ?? ""), end = Date.parse(g.endedAt ?? "");
    if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) intervals.set(id, null);
    else if (intervals.get(id) !== null) { const list = intervals.get(id) ?? []; list.push([start, end]); intervals.set(id, list); }
  }
  for (const [id, list] of intervals) if (list) {
    list.sort((a, b) => a[0] - b[0]); let boundary = -Infinity, total = 0;
    for (const [start, end] of list) { total += Math.max(0, end - Math.max(start, boundary)); boundary = Math.max(end, boundary); }
    byId.get(id)!.elapsed = total / 1000;
  }
  for (const g of raw) {
    if (g.operation == null) continue;
    if (workers.has(g.id)) {
      add(node({ ...g, sourceParent: g.parent, parent: `${workers.get(g.id)}/details` }));
      continue;
    }
    const stageNode = stage(g.run, g.scope, g.stage);
    const parentId = g.parent ? `${g.run}/${g.parent}` : undefined;
    const parent = parentId ? rawById.get(parentId) : undefined;
    const workerParent = parentId && workers.get(parentId) === stageNode.id ? stageNode.id : undefined;
    add(node({ ...g, sourceParent: g.parent, parent: workerParent ??
      (parent && parent.scope === g.scope && parent.stage === g.stage && parent.id !== g.id ? parent.id : stageNode.id),
      note: g.parent && !g.parent.startsWith("stage:") && !parent ? "Родительская операция вне загруженного окна" : undefined }));
  }
  for (const g of [...nodes]) if (g.kind === "stage" && g.state === "skipped" && ["refine", "cleanup"].includes(g.stage)) {
    const id = `${g.parent}/not-requested`;
    if (!byId.has(id)) add(node({ id, kind: "skipped", parent: g.parent, label: "Не запускались", state: "skipped" }));
    g.parent = id;
  }
  const workerStages = new Set(workers.values());
  for (const g of [...nodes]) if ((g.kind === "stage" || g.kind === "context") && (g.details.length || g.command || workerStages.has(g.id))) {
    add(node({ id: `${g.id}/details`, run: g.run, scope: g.scope, stage: g.stage, kind: "details",
      label: g.kind === "context" ? "Логи запуска" : "Логи и команда", parent: g.id, details: g.details, command: g.command }));
    g.details = []; g.command = undefined;
  }
  const resolved = new Set<string>();
  for (const start of nodes) {
    if (resolved.has(start.id)) continue;
    const path: OutlineGroup[] = [], seen = new Map<string, number>(); let current: OutlineGroup | undefined = start;
    while (current && !resolved.has(current.id)) {
      const offset = seen.get(current.id);
      if (offset != null) {
        for (const g of path.slice(offset)) g.note = "Циклическая ссылка родительских операций";
        current.parent = stageId(current.run, current.scope, current.stage); break;
      }
      seen.set(current.id, path.length); path.push(current);
      current = current.parent ? byId.get(current.parent) : undefined;
    }
    for (const g of path) resolved.add(g.id);
  }
  const children = new Map<string, OutlineGroup[]>();
  for (const g of nodes) if (g.parent) { const list = children.get(g.parent) ?? []; list.push(g); children.set(g.parent, list); }
  for (const list of children.values()) if (list.every(g => g.kind === "stage" || g.kind === "skipped")) {
    list.sort((a, b) => (chain.includes(a.stage) ? chain.indexOf(a.stage) : 99) - (chain.includes(b.stage) ? chain.indexOf(b.stage) : 99));
  }
  for (const g of nodes) if (g.kind === "skipped") g.label = `Не запускались · ${(children.get(g.id) ?? []).map(g => g.label).join(", ")}`;
  const ordered: OutlineGroup[] = [], visited = new Set<string>();
  const stack = nodes.filter(g => !g.parent).reverse().map(g => ({ g, depth: 0 }));
  while (stack.length) {
    const { g, depth } = stack.pop()!;
    if (visited.has(g.id)) continue; visited.add(g.id);
    const list = children.get(g.id) ?? [];
    g.depth = Math.min(depth, 32); g.children = list.length; ordered.push(g);
    stack.push(...[...list].reverse().map(g => ({ g, depth: depth + 1 })));
  }
  for (const g of nodes) if (!visited.has(g.id)) { g.depth = 2; g.note = "Родительская операция вне дерева"; ordered.push(g); }
  for (const g of [...ordered].reverse()) {
    g.issues += Number(g.state === "failed" || g.state === "invalid") + g.details.filter(e => e.status === "failed" || e.status === "invalid").length;
    const parent = g.parent ? byId.get(g.parent) : undefined;
    if (parent && parent !== g) parent.issues += g.issues;
  }
  return ordered;
}
export const outlineOpen = (g: OutlineGroup, expanded: ReadonlySet<string>, collapsed: ReadonlySet<string>) => expanded.has(g.id) || g.defaultOpen && !collapsed.has(g.id);
export function outlineAncestors(all: OutlineGroup[], id: string) {
  const parents = new Map(all.map(g => [g.id, g.parent]));
  const result = new Set<string>();
  let parent = parents.get(id);
  while (parent && result.size < all.length && !result.has(parent)) { result.add(parent); parent = parents.get(parent); }
  return result;
}
export function visibleOutline(all: OutlineGroup[], expanded: ReadonlySet<string>, collapsed: ReadonlySet<string>, filter: string) {
  const keep = new Set<string>(), parents = new Map(all.map(g => [g.id, g.parent]));
  if (filter) for (const g of all) if (`${g.label} ${g.stage} ${journalCopy(g)}`.toLocaleLowerCase().includes(filter.toLocaleLowerCase())) {
    keep.add(g.id); let parent = g.parent;
    for (let n = 0; parent && n < all.length; n++) { if (keep.has(parent)) break; keep.add(parent); parent = parents.get(parent); }
  }
  let blocked: number | undefined;
  return all.filter(g => {
    if (filter) return keep.has(g.id);
    if (blocked != null && g.depth > blocked) return false;
    blocked = !outlineOpen(g, expanded, collapsed) && g.children ? g.depth : undefined;
    return true;
  });
}
export function outlineState(g: OutlineGroup, active: boolean) {
  if (g.ended && !["failed", "invalid", "done", "cancelled"].includes(g.state ?? "")) return "завершено";
  if (g.kind === "stage" && g.state === "done") return "готово";
  if (g.state === "running") return active ? "идёт" : "завершение не зафиксировано";
  if (g.state === "failed") return "ошибка";
  if (g.state === "invalid") return "результат не подтверждён";
  if (g.state === "waiting") return "не начиналось";
  if (g.state === "queued") return "в очереди";
  if (g.state === "blocked") return "ждёт предыдущий этап";
  if (g.state === "skipped") return "пропущено";
  if (g.state === "cancelled") return "отменено";
  return g.ended ? "завершено" : "операции";
}
