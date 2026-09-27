import { useEffect, useRef, useState } from "react";
import { api, type Progress as Prog, type Session, type Stage, type StageState } from "./api";

const NAME: Record<Stage, string> = {
  download: "Скачали источник",
  extract: "Извлекли дорожку",
  transcribe: "Расшифровали",
  refine: "Причесали текст",
  cleanup: "Подготовили читаемый текст",
  summary: "Собрали сводку",
};

/** `waiting` and `interrupted` are ours, not the daemon's. `waiting`: a stage that has not started
 *  yet is a real state and must be drawn, not omitted. */
type Shown = StageState | "waiting";

const STATE: Record<Shown, { cls: string; label: string }> = {
  running: { cls: "busy", label: "идёт" },
  done: { cls: "done", label: "готово" },
  failed: { cls: "err", label: "сорвалось" },
  // Deliberately not done. This is an ANSWER, not a gap: a stage silently missing reads as
  // "still coming", and a person waits for something that will never happen.
  skipped: { cls: "mute", label: "пропущено" },
  waiting: { cls: "mute", label: "не начиналось" },
  queued: { cls: "mute", label: "в очереди" },
  blocked: { cls: "mute", label: "ждёт предыдущий этап" },
  invalid: { cls: "err", label: "результат не подтверждён" },
};

/** A ticking clock «M:SS» (or «N с» under a minute), for the per-stage timer: how long a step has
 *  been going, and how long a finished one took. */
const hms = (sec: number) => {
  if (!isFinite(sec) || sec < 0) return "";
  const s = Math.floor(sec % 60);
  const m = Math.floor(sec / 60);
  return m > 0 ? `${m}:${String(s).padStart(2, "0")}` : `${s} с`;
};

/** Seconds between an ISO timestamp and `nowMs`. */
const elapsed = (from: string | null | undefined, nowMs: number) =>
  from ? (nowMs - new Date(from).getTime()) / 1000 : NaN;

const CHAIN: Stage[] = ["download", "extract", "transcribe", "refine", "cleanup", "summary"];

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

/** What is happening with the session, stage by stage.
 *
 *  The queue could already say "cooking" — which answers "is anything happening at all" but
 *  not "where is it now", and, when it breaks, not "what exactly broke". A URL is not one step:
 *  download, track, transcript, cleanup, summary — and ten minutes of a spinner is
 *  indistinguishable from a hang. */
export function ProgressChain({ session }: { session: Session }) {
  const [prog, setProg] = useState<Prog | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);

  // A once-a-second heartbeat so the running stage's timer TICKS between the 3-second data polls —
  // a step that takes ten minutes must visibly count, or it reads as a hang. Runs only while
  // something is actually running.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!prog?.running) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [prog?.running]);

  useEffect(() => {
    let alive = true;
    setProg(null);
    setError(null);
    const my = ++seq.current;
    const load = async () => {
      try {
        const p = await api.progress(session.name);
        if (alive && my === seq.current) { setProg(p); setNow(Date.now()); setError(null); }
      } catch (e) {
        if (alive && my === seq.current) setError((e as Error).message);
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

  if (!prog) return <p className={error ? "err" : "hint"}>{error ? `Статус недоступен: ${error}` : "загрузка…"}</p>;

  const stages = prog.stages ?? [];
  const heard = new Map(stages.map((s) => [s.stage, s]));
  // Every stage of this session at once — the chain is a whole, not a growing stub. Its shape
  // comes from the session (a link brings a download; a microphone does not), never from what
  // happens to have spoken already.
  const rows = chainFor(!!prog.source, [...heard.keys()]).map((stage) => {
    const s = heard.get(stage);
    return {
      stage,
      state: (s?.state ?? "waiting") as Shown,
      started_at: s?.started_at,
      ended_at: s?.ended_at,
      updated_at: s?.updated_at,
      note: s?.note,
      done: s?.done,
      total: s?.total,
      files: s?.files,
      artifact: s?.artifact,
    };
  });

  return (
    <>
      {error && <p className="err">Статус не обновляется: {error}. Ниже — последние полученные данные.</p>}
      {prog.queue_error && <p className="err">Очередь недоступна, обработка не запустится: {prog.queue_error}</p>}
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

      <p className="hint">
        {prog.running ? <b>Обработка продолжается.</b> : <b>Текущее состояние результатов.</b>}
        {" "}«Готово» означает, что файл результата проверен. Время ролика не входит в расчёт.
      </p>
      {prog.timing && <p className="hint">
        Ожидание старта: <b>{prog.timing.queue_sec == null ? "—" : hms(prog.timing.queue_sec)}</b>
        {" · "}После старта: <b>{prog.timing.processing_sec == null ? "—" : hms(prog.timing.processing_sec)}</b>
        {" · "}Всего: <b>{prog.timing.total_sec == null ? "—" : hms(prog.timing.total_sec)}</b>
      </p>}

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
                  {s.state === "running" && s.total ? (
                    <span className="hint">
                      {Math.round(((s.done ?? 0) / Math.max(s.total, 1)) * 100)}%
                    </span>
                  ) : null}
                  {/* Time is ALWAYS shown: a running stage TICKS (how long it has gone), a finished
                      one shows how long it took — so a slow step never reads as a hang. */}
                  {s.state === "running" && s.started_at ? (
                    <span className="hint">⏱ {hms(elapsed(s.started_at, now))}</span>
                  ) : s.started_at && s.ended_at ? (
                    <span className="hint">
                      за {hms((new Date(s.ended_at).getTime() - new Date(s.started_at).getTime()) / 1000)}
                    </span>
                  ) : null}
                </span>
                {s.state === "running" && s.updated_at && <span className="hint">
                  Последний сигнал: {hms(Math.max(0, elapsed(s.updated_at, now)))} назад
                </span>}
                {s.note && <span className={s.state === "failed" || s.state === "invalid" ? "err" : "hint"}>{s.note}</span>}
                {!!s.files?.length && <details className="hint">
                  <summary>Результат: {s.files.join(", ")}</summary>
                  {s.artifact?.source != null && <div>Версия текста: v{s.artifact.source}</div>}
                  {s.artifact && !s.artifact.recorded_checksums && <div>Старая запись: проверены файл и версия; историческая контрольная сумма не сохранялась.</div>}
                  {s.artifact?.receipt?.outputs.map(f => <div key={f.path}>
                    <code>{f.path}</code> · {f.bytes} байт · {f.transcript ? "SHA-256 текста и таймкодов" : "SHA-256"}: <code>{f.sha256.slice(0, 16)}…</code>
                  </div>)}
                </details>}
              </span>
            </div>
          );
        })}
      </div>
    </>
  );
}
