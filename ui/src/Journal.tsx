import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, type Progress } from "./api";
import { groupJournal, journalActive, journalState, mergeJournal, type JournalEntry } from "./journal-model";

function readingAnchor(box: HTMLDivElement | null) {
  if (!box) return null;
  const viewport = box.getBoundingClientRect();
  const node = [...box.querySelectorAll<HTMLElement>("[data-journal-entry], .journal-row")].find((node) => {
    const bounds = node.getBoundingClientRect(); return bounds.bottom > viewport.top && bounds.top < viewport.bottom;
  });
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
export function Journal({ session, execution, uncertain, now = Date.now() }: { session: string; execution?: Progress["execution"]; uncertain?: boolean; now?: number }) {
  const [entries, setEntries] = useState<JournalEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
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
  }, [expanded]);
  const toggle = (id: string) => {
    manualReading.current = true; follow.current = false; viewportRevision.current++;
    const positions = [];
    for (let node: Element | null = box.current; node; node = node.parentElement) {
      positions.push({ node, top: node.scrollTop, left: node.scrollLeft });
    }
    savedViewport.current = positions;
    setExpanded((old) => { const next = new Set(old); if (next.has(id)) next.delete(id); else next.add(id); return next; });
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
    setEntries([]); setError(null); setExpanded(new Set()); setUnseen(0); setLimited(false); setOlder(null);
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
          setExpanded(new Set()); setUnseen(0);
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
          const ids = new Set(groupJournal(merged.entries).map((group) => group.id));
          setExpanded((old) => new Set([...old].filter((id) => ids.has(id))));
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
  const visible = groups.filter((g) => `${g.label} ${g.stage} ${g.details.map((d) => d.text).join(" ")}`.toLocaleLowerCase().includes(filter.toLocaleLowerCase()));
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
        setDetached(false); setExpanded(new Set()); setUnseen(0); setResetNotice(true);
        setLimited(!!(page.truncated || page.discarded || page.dropped_verbose));
      }
      for (const entry of page.events) received.current.add(entry.id);
      while (received.current.size > 5000) received.current.delete(received.current.values().next().value!);
      // Earlier pages are explicitly requested; preserve the reader's current viewport.
      const merged = mergeJournal(entriesRef.current, page, true);
      entriesRef.current = merged.entries; setEntries(merged.entries);
      const ids = new Set(groupJournal(merged.entries).map((group) => group.id));
      setExpanded((old) => new Set([...old].filter((id) => ids.has(id))));
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
    <div ref={box} className="journal-scroll" onScroll={(e) => { const el = e.currentTarget; follow.current = !manualReading.current && !filter && !tailGap.current && el.scrollHeight - el.clientHeight - el.scrollTop < 24; if (follow.current) setUnseen(0); }}>
      {older && <button className="tool journal-older" disabled={loadingOlder} onClick={() => void loadOlder()}>{loadingOlder ? "Загрузка предыдущих…" : "↑ Предыдущие события"}</button>}
      {!groups.length && !error && <p className="hint">Подробные события ещё не поступили. Состояние этапов показано выше.</p>}
      {visible.map((group, index) => {
        const active = !uncertain && journalActive(group, execution);
        const state = journalState(group, active);
        const elapsed = group.elapsed ?? (group.startedAt && (group.endedAt || active === true)
          ? ((group.endedAt ? new Date(group.endedAt).getTime() : now) - new Date(group.startedAt).getTime()) / 1000 : NaN);
        const duration = Number.isFinite(elapsed) && elapsed >= 0
          ? `${Math.floor(elapsed / 60)}:${String(Math.floor(elapsed % 60)).padStart(2, "0")}` : "время неизвестно";
        const open = expanded.has(group.id);
        const hidden = group.details.filter((d) => d.status !== "failed" && d.status !== "invalid");
        const hiddenLines = hidden.reduce((n, d) => n + (d.text?.split("\n").length ?? 0), 0);
        const errors = group.details.filter((d) => d.status === "failed" || d.status === "invalid");
        return <div className="journal-operation" key={group.id} data-journal-id={group.id} data-state={group.state ?? (group.ended ? "ended" : "active")}>
          {(index === 0 || visible[index - 1].run !== group.run) && <div className="journal-run">run <span>{group.run || "исторические события · запуск неизвестен"}</span></div>}
          {(index === 0 || visible[index - 1].run !== group.run || visible[index - 1].stage !== group.stage) && <div className="journal-stage"><span aria-hidden="true">├</span> {group.stage}</div>}
          <div className="journal-row">
            <span className="journal-branch" aria-hidden="true">└</span>
            <span className={`journal-indicator ${state.cls}`} aria-hidden="true">{state.symbol}</span>
            <span className="journal-command">{group.command?.program ?? group.label}</span>
            {group.command?.args.map((arg, i) => <span key={`${arg.name}/${i}`} className="journal-arg">{arg.name}{arg.value != null && <span className="journal-value"> {arg.value}</span>}</span>)}
            <span className={`journal-state ${state.cls}`}>{state.label}</span>
            <span className="journal-state mute" aria-label="Длительность операции">{duration}</span>
            {group.lastAt && <time className="journal-time" dateTime={group.lastAt}>{group.lastAt.slice(11, 19)}</time>}
          </div>
          {errors.map((entry) => <p key={entry.id} data-journal-entry={entry.id} className="journal-detail err">{entry.text}</p>)}
          {hidden.length > 0 && <button className="journal-toggle" aria-expanded={open} onClick={() => toggle(group.id)}>{open ? "− свернуть" : `+ ${hiddenLines} lines`}</button>}
          {open && <div className="journal-details">{hidden.map((entry) => <div key={entry.id} data-journal-entry={entry.id} className="journal-detail"><time>{entry.at.slice(11, 19)}</time><span>{entry.text}</span></div>)}</div>}
        </div>;
      })}
    </div>
  </section>;
}
