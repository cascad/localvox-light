import { useState } from "react";
import { api, acceptsDocument, type Line, type Session, type Version } from "../api";
import { mmss } from "../lib";
import { useArtifact } from "../useArtifact";
import { PartialStatus } from "./partial";

export function Transcript({
  session,
  pos,
  onSeek,
  onChanged,
  say,
  refresh,
}: {
  session: Session;
  pos: number;
  onSeek: (sec: number, until?: number) => void;
  onChanged: () => void;
  say: (m: string) => void;
  refresh: number;
}) {
  // The version switch lives HERE, in the tab, not in a section of its own: versions ARE the
  // transcript, and switching the working one just reloads this view. `gen` forces that reload.
  const [gen, setGen] = useState(0);
  const { data, err } = useArtifact(
    `${session.name}/tr/${gen}`,
    signal => api.transcript(session.name, signal),
    refresh,
    acceptsDocument,
  );
  const vers = useArtifact(
    `${session.name}/vr/${gen}`,
    signal => api.versions(session.name, signal),
    refresh,
  );
  const versions: Version[] = vers.data?.versions ?? [];
  const bestId = versions.find((v) => v.best)?.id;
  const [switching, setSwitching] = useState(false);

  const switchTo = async (id: number) => {
    setSwitching(true);
    try {
      await api.setBest(session.name, id);
      say(`Рабочей стала v${String(id).padStart(3, "0")}`);
      setGen((g) => g + 1);
      onChanged(); // the whole document (summary, search) reads from best — refresh the shell too
    } catch (e) {
      say((e as Error).message);
    } finally {
      setSwitching(false);
    }
  };

  if (err && !data) return <p className="err">{err}</p>;
  if (!data) return <p className="hint">загрузка…</p>;
  const lines: Line[] = data.lines ?? [];

  const switcher = versions.length > 1 && (
    <div className="ver-switch" data-nocopy>
      <span className="lbl">Версия:</span>
      <select
        value={bestId ?? ""}
        disabled={switching}
        onChange={(e) => void switchTo(Number(e.target.value))}
      >
        {versions.map((v) => (
          <option key={v.id} value={v.id}>
            v{String(v.id).padStart(3, "0")} · {v.derived ? "причёсано LLM" : "из аудио"} ·{" "}
            {v.lines} строк
          </option>
        ))}
      </select>
      <span className="hint">от рабочей считаются текст, сводка и поиск</span>
    </div>
  );

  if (!lines.length)
    return (
      <>
        {switcher}
        <PartialStatus info={data.partial} />
        <p className="hint">{data.partial ? "Ожидание закрытого фрагмента и распознавания." : "пусто"}</p>
      </>
    );

  return (
    <>
      {switcher}
      <PartialStatus info={data.partial} />
      {err && <p className="hint" data-nocopy>Обновление недоступно; показана последняя готовая часть.</p>}
      <div className="lines">
        {lines.map((l, i) => {
        // The lit line is wherever the ONE player is now. Clicking ▶ moves the player to this
        // line's start, and load() sets the position at once (the clip begins exactly there), so
        // the right line lights immediately — no lag, no "previous line first".
        const sounding = pos >= l.start_sec && pos < l.end_sec;
        return (
          <div key={l.id ?? i} className={`ln${sounding ? " playing" : ""}`}>
            <button
              className="play"
              title={`Послушать этот фрагмент (${mmss(l.start_sec)}–${mmss(l.end_sec)})`}
              onClick={() => onSeek(l.start_sec, l.end_sec)}
            >
              ▶
            </button>
            <span className="tc">{mmss(l.start_sec)}</span>
            <span className={`sp${l.who === "Я" ? " me" : ""}`}>{l.who}</span>
            <span className="tx">{l.text}</span>
          </div>
        );
        })}
      </div>
    </>
  );
}
