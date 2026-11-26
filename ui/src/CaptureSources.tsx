import { api, type CaptureSource } from "./api";
import { useEffect, useRef, useState } from "react";
import { usePolling } from "./usePolling";

function LiveMeters({ sources }: { sources: CaptureSource[] }) {
  const { data, error } = usePolling("capture/levels", async signal => {
    const started = performance.now();
    const values = await api.captureLevels(signal);
    return { values, expires: started + 750 };
  }, 250);
  // A slow or disconnected API must not leave a bright bar on screen.
  const [expired, setExpired] = useState<typeof data>(null);
  useEffect(() => {
    if (!data) return;
    const timer = setTimeout(() => setExpired(data), Math.max(0, data.expires - performance.now()));
    return () => clearTimeout(timer);
  }, [data]);
  return <>{sources.map(source => {
    const open = source.state === "waiting" || source.state === "signal";
    const level = open && !error && data && expired !== data && performance.now() < data.expires ? data.values[source.source_id] : null;
    const icon = source.state === "error" ? "!" : source.state === "disabled" ? "—" : "?";
    const message = source.state === "error" ? source.detail : source.state === "disabled" ? "Выключен" :
      level == null ? "Нет свежего измерения" : level === 0 ? "Тишина" : `Уровень ${level} из 6`;
    return <span key={source.source_id} className="capture-input" title={`${source.device ?? source.name} · ${message}`}>
      {source.source_id === 0 ? "мик" : "звук"}
      <span className="capture-meter" role="img" aria-label={`${source.name} · ${message}`}>
        {open && Array.from({ length: 6 }, (_, i) => <i key={i} data-lit={i < (level ?? 0)} />)}
        {level == null && <b className={source.state === "error" ? "err" : ""}>{icon}</b>}
      </span>
    </span>;
  })}</>;
}

/** Stream creation is not evidence of sound. Expand for the exact state/remedy. */
export function CaptureSources({ sources, live }: { sources?: CaptureSource[]; live?: boolean }) {
  const details = useRef<HTMLDetailsElement>(null);
  const previous = useRef(new Map<number, string>());
  const [history, setHistory] = useState<{ name: string; detail: string }[]>([]);
  const [unread, setUnread] = useState(0);
  const [visible, setVisible] = useState(() => !document.hidden);
  useEffect(() => {
    const changed = () => setVisible(!document.hidden);
    document.addEventListener("visibilitychange", changed);
    return () => document.removeEventListener("visibilitychange", changed);
  }, []);
  useEffect(() => {
    const errors: { name: string; detail: string }[] = [];
    const present = new Set((sources ?? []).map(source => source.source_id));
    const current = new Map([...previous.current].filter(([id]) => present.has(id)));
    for (const source of sources ?? []) {
      if (source.state === "signal" || source.state === "disabled") current.delete(source.source_id);
      if (source.state !== "error") continue;
      current.set(source.source_id, source.detail);
      if (previous.current.get(source.source_id) !== source.detail) errors.push({ name: source.name, detail: source.detail });
    }
    previous.current = current;
    if (errors.length) {
      setHistory(old => [...old, ...errors].slice(-30));
      if (!details.current?.open) setUnread(old => Math.min(30, old + errors.length));
    }
  }, [sources]);
  if (!sources?.length) return null;
  return (
    <details ref={details} className="capture-sources" onToggle={event => { if (event.currentTarget.open) setUnread(0); }}>
      <summary aria-label="Источники записи">
        {live && visible ? <LiveMeters sources={sources} /> : sources.map(source => (
          <span key={source.source_id}>
            {source.source_id === 0 ? "мик" : "звук"}{" "}
            {source.state === "signal" ? "✓" : source.state === "error" ? "!" : source.state === "disabled" ? "—" : "?"}
          </span>
        ))}
        {unread > 0 && <span className="err" data-capture-unread>! {unread}</span>}
      </summary>
      <div className="capture-source-details">
        {sources.map(source => (
          <p key={source.source_id}><strong>{source.name}</strong>{source.device && ` · ${source.device}`}<br /><span>{source.state === "error" ? "Ошибка захвата · подробности ниже" : source.detail}</span></p>
        ))}
        {history.length > 0 && <section aria-label="Ошибки захвата — история">
          <strong>Ошибки захвата · история</strong>
          {history.map((entry, index) => <p key={index}><span>{entry.name}</span><br /><span>{entry.detail}</span></p>)}
        </section>}
      </div>
    </details>
  );
}
