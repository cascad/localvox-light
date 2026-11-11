import { type Session } from "./api";
import { ProgressChain } from "./Progress";
import { sessionTitle, stateOf } from "./lib";
import type { Tab } from "./document/types";
export type { Tab } from "./document/types";
export { toTurns } from "./document/readable";
import { useDocumentController } from "./document/controller";
import { SessionLanguage } from "./document/language";
import { Transcript } from "./document/transcript";
import { Article } from "./document/summary";
import { Readable } from "./document/readable";
import { Speakers } from "./document/speakers";

const TABS: [Tab, string][] = [
  ["summary", "Сводка"],
  // Named by PURPOSE, which is the only thing that tells them apart now: one is checked against
  // the sound line by line, the other is read and copied. Both show the same wording.
  ["transcript", "Реплики"],
  ["processed", "Текст"],
  ["speakers", "Участники"],
  ["progress", "Ход"],
];

interface Props {
  session: Session;
  tab: Tab;
  setTab: (t: Tab) => void;
  /** Where the ONE player is now — drives which transcript line is lit. */
  pos: number;
  /** Move the player to a second. `until` — a transcript line asks to hear only that line. */
  onSeek: (sec: number, until?: number) => void;
  onChanged: () => void;
  say: (msg: string) => void;
}
export function DocumentPane({ session, tab, setTab, pos, onSeek, onChanged, say }: Props) {
  const st = stateOf(session);
  const { doc, controlBusy, tick, copy, recook, scopeWord, control } = useDocumentController({ session, tab, onChanged, say });
  return (
    <main className="main">
      <div className="doc-head">
        <h1>{sessionTitle(session)}</h1>
        <div className="m">
          <span>{Math.round(session.duration_sec / 60)} мин</span>
          <span className="sep">·</span>
          <span className={`st ${st.cls}`}>{st.text}</span>
          {session.stopped_reason && (
            <>
              <span className="sep">·</span>
              <span>остановлена {session.stopped_reason}</span>
            </>
          )}
        </div>
        <div className="tabs">
          {TABS.map(([k, label]) => (
            <button key={k} aria-selected={tab === k} onClick={() => setTab(k)}>
              {label}
            </button>
          ))}
          <span className="grow" />
          {session.job && ["running", "pending", "failed", "waiting_input"].includes(session.job) && <button className="tool" disabled={controlBusy} onClick={() => void control("cancel")}>
            {controlBusy ? "Остановка…" : "Отменить обработку"}
          </button>}
          {session.job === "cancelling" && <span className="hint">Ожидается остановка…</span>}
          {session.job === "cancelled" && <button className="tool" disabled={controlBusy} onClick={() => void control("retry")}>
            {controlBusy ? "Постановка…" : "Продолжить обработку"}
          </button>}
          <button className="tool" onClick={copy}
            aria-keyshortcuts={tab === "processed" ? "Control+Shift+C Meta+Shift+C" : undefined}
            title={`Скопировать документ (без служебных сносок)${tab === "processed" ? " · Ctrl+Shift+C" : ""}`}>
            ⧉ Копировать
          </button>
          <button className="tool" disabled={session.recording || controlBusy || session.job === "cancelling"} onClick={recook} title={`Переварить ${scopeWord}`}>
            ♻ Переварить
          </button>
        </div>
      </div>

      <div className="doc" ref={doc}>
        {tab === "transcript" && (
          <>
          <SessionLanguage session={session} onChanged={onChanged} say={say} />
          <Transcript
            session={session}
            pos={pos}
            onSeek={onSeek}
            onChanged={onChanged}
            say={say}
            refresh={tick}
          />
          </>
        )}
        {tab === "summary" && (
          <Article session={session} onSeek={onSeek} onChanged={onChanged} say={say} refresh={tick} />
        )}
        {tab === "processed" && (
          <Readable session={session} onChanged={onChanged} say={say} refresh={tick} />
        )}
        {tab === "speakers" && <Speakers session={session} say={say} refresh={tick} />}
        {tab === "progress" && <ProgressChain session={session} />}
      </div>
    </main>
  );
}
