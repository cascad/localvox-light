import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, type Progress } from "./api";
import { outlineJournal, visibleOutline, outlineOpen, outlineState, outlineAncestors, journalCopy, type OutlineGroup, type OutlineStage } from "./journal-outline";
import { StageArtifact } from "./StageArtifact";
import { groupJournal, journalActive, journalSeconds, journalDuration, journalState, mergeJournal, type JournalEntry } from "./journal-model";

function readingAnchor(box: HTMLDivElement | null) {
  if (!box) return null;
  const viewport = box.getBoundingClientRect();
  const visible = (node: HTMLElement) => {
    const bounds = node.getBoundingClientRect(); return bounds.bottom > viewport.top && bounds.top < viewport.bottom;
  };
  // Branch headers can stay fixed while older operations are inserted beneath
  // them. Anchor the content being read, falling back to headers only if empty.
  const node = [...box.querySelectorAll<HTMLElement>("[data-journal-entry]")].find(visible)
    ?? [...box.querySelectorAll<HTMLElement>('[data-kind="operation"] > .journal-row')].find(visible)
    ?? [...box.querySelectorAll<HTMLElement>(".journal-row")].find(visible);
  if (!node) return null;
  return { entry: node.dataset.journalEntry, group: node.closest<HTMLElement>("[data-journal-id]")?.dataset.journalId, top: node.getBoundingClientRect().top };
}
function restoreReadingAnchor(box: HTMLDivElement | null, anchor: ReturnType<typeof readingAnchor>) {
  if (!box || !anchor) return;
  const node = anchor.entry
    ? [...box.querySelectorAll<HTMLElement>("[data-journal-entry]")].find((node) => node.dataset.journalEntry === anchor.entry)
    : [...box.querySelectorAll<HTMLElement>("[data-journal-id]")].find((node) => node.dataset.journalId === anchor.group)?.querySelector<HTMLElement>(".journal-row");
  if (node) box.scrollTop += node.getBoundingClientRect().top - anchor.top;
}

/** A separate history view. The artifact/phase result remains the ProgressChain's responsibility. */
export function Journal({ session, execution, stages = [], uncertain, now = Date.now() }: { session: string; execution?: Progress["execution"]; stages?: OutlineStage[]; uncertain?: boolean; now?: number }) {
  const [entries, setEntries] = useState<JournalEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const currentOutline = useRef({ execution, stages });
  currentOutline.current = { execution, stages };
  const preserveReadingBranch = (entries: JournalEntry[], id?: string) => {
    const { execution, stages } = currentOutline.current;
    const all = outlineJournal(groupJournal(entries), execution, stages);
    const active = new Set(all.map(g => g.id));
    const parents = id ? outlineAncestors(all, id) : new Set<string>();
    setExpanded(old => new Set([...old].filter(id => active.has(id)).concat([...parents])));
    setCollapsed(old => new Set([...old].filter(id => active.has(id) && !parents.has(id))));
  };
  const [filter, setFilter] = useState("");
  const [unseen, setUnseen] = useState(0);
  const [limited, setLimited] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  const manualReading = useRef(false);
  const viewportRevision = useRef(0);
  const savedViewport = useRef<{ node: Element; top: number; left: number }[] | null>(null);
  useLayoutEffect(() => {
    for (const point of savedViewport.current ?? []) {
      point.node.scrollTop = point.top; point.node.scrollLeft = point.left;
    }
    savedViewport.current = null;
  }, [expanded, collapsed]);
  const toggle = (group: OutlineGroup) => {
    const id = group.id;
    const open = !outlineOpen(group, expanded, collapsed);
    manualReading.current = true; follow.current = false; viewportRevision.current++;
    const positions = [];
    for (let node: Element | null = box.current; node; node = node.parentElement) {
      positions.push({ node, top: node.scrollTop, left: node.scrollLeft });
    }
    savedViewport.current = positions;
    setExpanded((old) => { const next = new Set(old); if (open) next.add(id); else next.delete(id); return next; });
    setCollapsed((old) => { const next = new Set(old); if (open) next.delete(id); else next.add(id); return next; });
  };
  const generation = useRef(0);
  const historyGeneration = useRef(0);
  const received = useRef(new Set<string>());
  const requests = useRef(new Set<AbortController>());
  const [older, setOlder] = useState<string | null>(null);
  const olderBusy = useRef(false);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [resetNotice, setResetNotice] = useState(false);
  const entriesRef = useRef<JournalEntry[]>([]);
  const cursorRef = useRef<string | undefined>(undefined);
  const tailGap = useRef(false);
  const reloadTail = useRef(false);
  const [detached, setDetached] = useState(false);
  const [droppedVerbose, setDroppedVerbose] = useState(0);

  useEffect(() => {
    const epoch = ++generation.current;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    historyGeneration.current++;
    received.current.clear();
    olderBusy.current = false; setLoadingOlder(false); setResetNotice(false);
    entriesRef.current = []; cursorRef.current = undefined;
    tailGap.current = false; reloadTail.current = false; setDetached(false);
    setDroppedVerbose(0);
    setEntries([]); setError(null); setExpanded(new Set()); setCollapsed(new Set()); setUnseen(0); setLimited(false); setOlder(null);
    follow.current = true;
    manualReading.current = false; viewportRevision.current++;
    const load = async () => {
      const history = historyGeneration.current;
      const request = new AbortController(); requests.current.add(request);
      try {
        const replace = reloadTail.current;
        const cursor = replace ? undefined : cursorRef.current;
        const page = await api.journal(session, cursor, undefined, request.signal);
        if (!alive || generation.current !== epoch) return;
        // A backwards request may already have observed rotation. Never append
        // a response from the old file to the new file's tail.
        if (history !== historyGeneration.current) { timer = setTimeout(() => void load(), 100); return; }
        const anchor = follow.current || page.reset || replace ? null : readingAnchor(box.current);
        const revision = viewportRevision.current;
        if (page.reset || replace) {
          historyGeneration.current++; received.current.clear();
          setExpanded(new Set()); setCollapsed(new Set()); setUnseen(0);
          if (page.reset) setResetNotice(true);
        }
        let added = 0;
        for (const entry of page.events) { if (!received.current.has(entry.id)) added++; received.current.add(entry.id); }
        while (received.current.size > 5000) received.current.delete(received.current.values().next().value!);
        const merged = mergeJournal(entriesRef.current, { ...page, reset: page.reset || replace });
        if (merged.evicted || page.truncated || page.discarded || page.dropped_verbose) setLimited(true);
        else if (page.reset || replace) setLimited(false);
        setDroppedVerbose(page.dropped_verbose ?? 0);
        if (replace || page.reset) { tailGap.current = false; setDetached(false); reloadTail.current = false; }
        // Keep the current reading window when a full cache would evict it.
        // Polling still counts arrivals; an explicit End reloads the current tail.
        if (!follow.current && merged.evictedHead) { tailGap.current = true; setDetached(true); }
        if (!tailGap.current) {
          entriesRef.current = merged.entries; setEntries(merged.entries);
          preserveReadingBranch(merged.entries, anchor?.group);

          if (merged.evictedHead) setOlder(merged.entries[0]?.id ?? null);
        }
        if (!cursor || page.reset) setOlder(page.truncated ? page.previous_cursor : null);
        cursorRef.current = page.next_cursor || undefined;
        if (added && !follow.current && !page.reset && !replace) setUnseen((n) => n + added);
        if (follow.current) requestAnimationFrame(() => { if (alive && generation.current === epoch && follow.current && revision === viewportRevision.current && box.current) box.current.scrollTop = box.current.scrollHeight; });
        else requestAnimationFrame(() => { if (alive && generation.current === epoch && revision === viewportRevision.current) restoreReadingAnchor(box.current, anchor); });
        setError(null); failures = 0;
        timer = setTimeout(() => void load(), page.has_more ? 100 : 2500);
      } catch (e) {
        if (!alive || generation.current !== epoch) return;
        if (history !== historyGeneration.current) { timer = setTimeout(() => void load(), 100); return; }
        setError((e as Error).message);
        failures++;
        timer = setTimeout(() => void load(), Math.min(30_000, 2500 * 2 ** Math.min(failures, 4)));
      } finally { requests.current.delete(request); }
    };
    // React's development StrictMode probes setup/cleanup synchronously. Do
    // not issue a request from an effect that was already torn down.
    queueMicrotask(() => { if (alive) void load(); });
    return () => { alive = false; clearTimeout(timer); for (const request of requests.current) request.abort(); requests.current.clear(); };
  }, [session]);

  const groups = useMemo(() => groupJournal(entries), [entries]);
  const outline = useMemo(() => outlineJournal(groups, execution, stages), [groups, execution, stages]);
  const visible = visibleOutline(outline, expanded, collapsed, filter);

  const loadOlder = async () => {
    if (!older || olderBusy.current) return;
    olderBusy.current = true;
    setLoadingOlder(true);
    const epoch = generation.current;
    const history = historyGeneration.current;
    const request = new AbortController(); requests.current.add(request);
    try {
      const page = await api.journal(session, undefined, older, request.signal);
      if (epoch !== generation.current || history !== historyGeneration.current) return;
      const anchor = page.reset ? null : readingAnchor(box.current);
      if (page.reset) {
        historyGeneration.current++; received.current.clear();
        cursorRef.current = page.next_cursor || undefined;
        tailGap.current = false; reloadTail.current = false;
        setDetached(false); setExpanded(new Set()); setCollapsed(new Set()); setUnseen(0); setResetNotice(true);
        setLimited(!!(page.truncated || page.discarded || page.dropped_verbose));
      }
      for (const entry of page.events) received.current.add(entry.id);
      while (received.current.size > 5000) received.current.delete(received.current.values().next().value!);
      // Earlier pages are explicitly requested; preserve the reader's current viewport.
      const merged = mergeJournal(entriesRef.current, page, true);
      entriesRef.current = merged.entries; setEntries(merged.entries);
      preserveReadingBranch(merged.entries, anchor?.group);

      if (merged.evictedTail) { tailGap.current = true; setDetached(true); setLimited(true); }
      setDroppedVerbose(page.dropped_verbose ?? 0);
      setOlder(page.truncated ? page.previous_cursor : null);
      follow.current = false;
      setError(null);
      requestAnimationFrame(() => { if (epoch === generation.current) restoreReadingAnchor(box.current, anchor); });
    } catch (e) { if (epoch === generation.current && history === historyGeneration.current) setError((e as Error).message); }
    finally { requests.current.delete(request); if (epoch === generation.current) { olderBusy.current = false; setLoadingOlder(false); } }
  };
  return <section className="journal" aria-label="Технический журнал">
    <div className="journal-toolbar">
      <span className="journal-label">журнал / операции</span>
      <input aria-label="Фильтр журнала" placeholder="фильтр…" value={filter} onChange={(e) => setFilter(e.target.value)} />
      <button className="tool" onClick={() => { manualReading.current = false; viewportRevision.current++; follow.current = true; setUnseen(0); if (tailGap.current) reloadTail.current = true; if (box.current) box.current.scrollTop = box.current.scrollHeight; }}>↓ {unseen ? `новые: ${unseen}` : "к хвосту"}</button>
    </div>
    {error && <p className="err" role="status">Журнал не обновляется: {error}. Последние события сохранены на экране.</p>}
    {resetNotice && <p className="journal-notice">Файл истории сменился. Показаны события нового журнала.</p>}
    {limited && <p className="journal-notice">Показана часть истории. Ограничение загрузки отличается от свёрнутых строк.</p>}
    {droppedVerbose > 0 && <p className="journal-notice">Пропущено подробностей по лимиту запуска: {droppedVerbose}. Переходы и ошибки сохраняются.</p>}
    {detached && <p className="journal-notice">Сохранено окно чтения. Переход к хвосту загрузит последние события.</p>}
    <div ref={box} className="journal-scroll chain" onScroll={(e) => { const el = e.currentTarget; follow.current = !manualReading.current && !filter && !tailGap.current && el.scrollHeight - el.clientHeight - el.scrollTop < 24; if (follow.current) setUnseen(0); }}>
      {older && <button className="tool journal-older" disabled={loadingOlder} onClick={() => void loadOlder()}>{loadingOlder ? "Загрузка предыдущих…" : "↑ Предыдущие события"}</button>}
      {!groups.length && !error && <p className="hint">Подробные события ещё не поступили. Состояние этапов показано выше.</p>}
      {visible.map((group) => {
        const active = !uncertain && (group.kind === "stage" ? group.state === "running" && (execution == null || execution.state === "running") : journalActive({ ...group, parent: group.sourceParent }, execution, stages));
        const state = journalState(group, active);
        const seconds = journalSeconds(group, execution?.run, stages, active, now);
        const duration = journalDuration(seconds);
        const open = outlineOpen(group, expanded, collapsed) || !!filter && journalCopy(group).toLocaleLowerCase().includes(filter.toLocaleLowerCase());
        const container = group.children > 0 || group.details.length > 0 || group.command != null || group.note != null;
        const logical = group.kind === "context" || group.kind === "skipped" || group.kind === "details";
        const stage = group.kind === "stage" ? stages.find(s => s.stage === group.stage && (group.run === execution?.run || group.run === "@planned")) : undefined;
        const hidden = group.details.filter((d) => d.status !== "failed" && d.status !== "invalid");
        const hiddenLines = hidden.reduce((n, d) => n + (d.text?.split("\n").length ?? 0), 0);
        const errors = group.details.filter((d) => d.status === "failed" || d.status === "invalid");
        return <div className={`journal-operation ${group.kind === "stage" ? `step ${group.state ?? "waiting"}` : ""}`} key={group.id} data-journal-id={group.id} data-kind={group.kind} data-state={group.state ?? (group.ended ? "ended" : "active")} style={{ paddingInlineStart: group.depth * 16 }}>
          <div className="journal-row t">
            {container ? <button className="journal-fold" aria-expanded={open} aria-label={`${open ? "Свернуть" : "Раскрыть"}: ${group.label}`} onClick={() => toggle(group)}>{open ? "▾" : "▸"}</button> : <span className="journal-branch" aria-hidden="true">└</span>}
            {!logical && <span className={`journal-indicator ${state.cls}`} aria-hidden="true">{state.symbol}</span>}
            <span className={`journal-command ${logical ? "journal-context" : ""}`}>{group.label}</span>
            {group.kind === "context" && group.elapsed != null && <span className="journal-duration" title="Объединение измеренных интервалов процессов в загруженной истории">измерено {journalDuration(group.elapsed)}</span>}
            {group.kind === "context" && group.firstAt && group.lastAt && <time className="journal-time" title="Границы событий в загруженной истории">события {group.firstAt.slice(11, 19)}–{group.lastAt.slice(11, 19)}</time>}
            {!logical && <span className={`journal-state st ${state.cls}`}>{outlineState(group, active)}</span>}
            {!logical && ((Number.isFinite(seconds) && seconds >= 0) || ["done", "failed", "invalid", "running"].includes(group.state ?? "") || group.ended) && <span className="journal-duration hint" aria-label={group.kind === "stage" ? "Длительность этапа" : "Длительность операции"}>{group.state === "done" && (Number.isFinite(seconds) && seconds >= 0) ? "за " : ""}{duration}</span>}
            {group.kind === "stage" && group.state === "running" && !!group.total && <span className="hint">{Math.round((group.done ?? 0) / Math.max(1, group.total) * 100)}%</span>}
            {group.issues > 0 && <span className="err journal-issues">ошибки в истории</span>}
            {group.kind === "operation" && group.lastAt && <time className="journal-time" dateTime={group.lastAt}>{group.lastAt.slice(11, 19)}</time>}
          </div>
          {group.note && (open || ["running", "failed", "invalid", "blocked", "cancelled", "skipped"].includes(group.state ?? "")) && <p className={group.state === "failed" || group.state === "invalid" ? "journal-detail err" : "journal-detail"}>{group.note}</p>}
          {group.kind === "stage" && group.state === "running" && stage?.updated_at && <p className="journal-detail">Последний сигнал: {journalDuration(Math.max(0, (now - Date.parse(stage.updated_at)) / 1000))} назад · heartbeat не означает прогресс</p>}
          {open && group.command && <div className="journal-row journal-technical" data-journal-entry={`${group.id}/command`}><span className="journal-command">{group.command.program}</span>{group.command.args.map((arg, i) => <span key={i} className="journal-arg">{arg.name}{arg.value != null && <span className="journal-value"> {arg.value}</span>}</span>)}</div>}
          {open && stage && <><StageArtifact stage={stage} />{stage.done != null && stage.total != null && <p className="journal-detail">Обработано {stage.done}/{stage.total}</p>}</>}
          {open && errors.map((entry) => <p key={entry.id} data-journal-entry={entry.id} className="journal-detail err">{entry.text}</p>)}
          {hidden.length > 0 && <button className="journal-toggle" aria-expanded={open} onClick={() => toggle(group)}>{open ? "− свернуть" : `+ ${hiddenLines} lines`}</button>}
          {open && <div className="journal-details">{hidden.map((entry) => <div key={entry.id} data-journal-entry={entry.id} className="journal-detail"><time>{entry.at.slice(11, 19)}</time><span>{entry.text}</span></div>)}</div>}
        </div>;
      })}
    </div>
  </section>;
}
