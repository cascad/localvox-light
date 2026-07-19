import { useEffect, useRef, useState } from "react";
import { api, type Progress as Prog, type Session, type Stage, type StageState } from "./api";

const NAME: Record<Stage, string> = {
  download: "Скачали источник",
  extract: "Извлекли дорожку",
  transcribe: "Расшифровали",
  refine: "Причесали текст",
  summary: "Собрали сводку",
};

/** `waiting` is ours, not the daemon's: the daemon only reports stages that have spoken.
 *  A stage that has not started yet is a real state and must be drawn, not omitted. */
type Shown = StageState | "waiting";

const STATE: Record<Shown, { cls: string; label: string }> = {
  running: { cls: "busy", label: "идёт" },
  done: { cls: "done", label: "готово" },
  failed: { cls: "err", label: "сорвалось" },
  // Deliberately not done. This is an ANSWER, not a gap: a stage silently missing reads as
  // "still coming", and a person waits for something that will never happen.
  skipped: { cls: "mute", label: "пропущено" },
  waiting: { cls: "mute", label: "не начиналось" },
};

const CHAIN: Stage[] = ["download", "extract", "transcribe", "refine", "summary"];

/** Which stages this session will ever have.
 *
 *  Only a recording that came from a URL has a download and an extraction — drawing those grey
 *  for a microphone recording would promise steps that are never coming. So the shape of the
 *  chain follows the session, and everything in it is drawn from the start: what is done, what is
 *  going, and what has not begun. */
const chainFor = (fromLink: boolean, reported: Stage[]): Stage[] =>
  CHAIN.filter(
    (s) =>
      reported.includes(s) || (fromLink ? true : s !== "download" && s !== "extract"),
  );

const since = (from?: string | null) => {
  if (!from) return "";
  const sec = (Date.now() - new Date(from).getTime()) / 1000;
  if (!isFinite(sec) || sec < 0) return "";
  return sec < 60 ? `${Math.round(sec)} с` : `${Math.round(sec / 60)} мин`;
};

/** What is happening with the session, stage by stage.
 *
 *  The queue could already say "cooking" — which answers "is anything happening at all" but
 *  not "where is it now", and, when it breaks, not "what exactly broke". A URL is not one step:
 *  download, track, transcript, cleanup, summary — and ten minutes of a spinner is
 *  indistinguishable from a hang. */
export function ProgressChain({ session }: { session: Session }) {
  const [prog, setProg] = useState<Prog | null>(null);
  const seq = useRef(0);

  useEffect(() => {
    let alive = true;
    const my = ++seq.current;
    const load = async () => {
      try {
        const p = await api.progress(session.name);
        if (alive && my === seq.current) setProg(p);
      } catch {
        /* the chain is a story about the work, not the work: its absence breaks nothing */
      }
    };
    void load();
    // While something is running, the chain is alive — it must move on the screen, not on F5.
    const t = setInterval(() => void load(), 3000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [session.name]);

  if (!prog) return <p className="hint">загрузка…</p>;

  const stages = prog.stages ?? [];
  const heard = new Map(stages.map((s) => [s.stage, s]));
  // Every stage of this session at once — the chain is a whole, not a growing stub. Its shape
  // comes from the session (a link brings a download; a microphone does not), never from what
  // happens to have spoken already.
  const rows = chainFor(!!prog.source, [...heard.keys()]).map((stage) => {
    const s = heard.get(stage);
    return { stage, state: (s?.state ?? "waiting") as Shown, started_at: s?.started_at, note: s?.note };
  });

  return (
    <>
      {prog.source && (
        <p className="note" data-nocopy>
          <span className="i" aria-hidden="true">
            ⓘ
          </span>
          <span>
            Эта запись пришла по ссылке:{" "}
            <a href={prog.source.url} target="_blank" rel="noreferrer">
              {prog.source.url}
            </a>
            . Звук скачан и разложен по кускам — дальше она ничем не отличается от записи
            встречи: те же реплики, те же версии, тот же плеер.
          </span>
        </p>
      )}

      {/* Where the whole run stands. The chain below shows the stages; this says whether anything
          is coming for the grey ones at all — a grey stage with nobody working on it is waiting
          for a person, not for the machine. */}
      {prog.running ? (
        <p className="hint">
          {session.job === "pending" ? (
            <>
              <b>В очереди.</b> Демон возьмётся за неё, как только освободится.
            </>
          ) : (
            <>
              <b>В работе.</b> Серые этапы ещё впереди. Между этапами бывает тихо — обычно так
              выглядит загрузка модели.
            </>
          )}
        </p>
      ) : (
        stages.length === 0 && <p className="hint">Обработка этой записи не начиналась.</p>
      )}

      <div className="chain">
        {rows.map((s) => {
          const st = STATE[s.state];
          return (
            <div className={`step ${s.state}`} key={s.stage}>
              <span className="track" aria-hidden="true">
                <i />
              </span>
              <span className="body">
                <span className="t">
                  <b>{NAME[s.stage]}</b>
                  <span className={`st ${st.cls}`}>{st.label}</span>
                  {s.state === "running" && s.started_at && (
                    <span className="hint">{since(s.started_at)}</span>
                  )}
                </span>
                {s.note && <span className={s.state === "failed" ? "err" : "hint"}>{s.note}</span>}
              </span>
            </div>
          );
        })}
      </div>
    </>
  );
}
