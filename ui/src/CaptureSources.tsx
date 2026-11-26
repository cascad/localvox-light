import type { CaptureSource } from "./api";
import { useEffect, useRef, useState } from "react";

/** Stream creation is not evidence of sound. Expand for the exact state/remedy. */
export function CaptureSources({ sources }: { sources?: CaptureSource[] }) {
  const details = useRef<HTMLDetailsElement>(null);
  const previous = useRef(new Map<number, string>());
  const [history, setHistory] = useState<{ name: string; detail: string }[]>([]);
  const [unread, setUnread] = useState(0);
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
        {sources.map(source => (
          <span key={source.source_id}>
            {source.source_id === 0 ? "мик" : "звук"}{" "}
            {source.state === "signal" ? "✓" : source.state === "error" ? "!" : source.state === "disabled" ? "—" : "?"}
          </span>
        ))}
        {unread > 0 && <span className="err" data-capture-unread>! {unread}</span>}
      </summary>
      <div className="capture-source-details">
        {sources.map(source => (
          <p key={source.source_id}><strong>{source.name}</strong><br /><span>{source.state === "error" ? "Ошибка захвата · подробности ниже" : source.detail}</span></p>
        ))}
        {history.length > 0 && <section aria-label="Ошибки захвата — история">
          <strong>Ошибки захвата · история</strong>
          {history.map((entry, index) => <p key={index}><span>{entry.name}</span><br /><span>{entry.detail}</span></p>)}
        </section>}
      </div>
    </details>
  );
}
