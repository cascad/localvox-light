import { useEffect, useState } from "react";
import { Journal } from "./Journal";
import { api, type Session, type Stage, type StageState } from "./api";
import { usePolling } from "./usePolling";

const NAME: Record<Stage, string> = {
  download: "Скачали источник",
  extract: "Извлекли дорожку",
  transcribe: "Распознавание · GigaAM",
  refine: "Исправление реплик · refine",
  cleanup: "Читаемый текст · cleanup",
  summary: "Сводка · summary",
};

/** `waiting` and `interrupted` are ours, not the daemon's. `waiting`: a stage that has not started
 *  yet is a real state and must be drawn, not omitted. */
type Shown = StageState | "waiting";

const STATE: Record<Shown, { cls: string; label: string }> = {
  running: { cls: "busy", label: "идёт" },
  done: { cls: "done", label: "готово" },
  failed: { cls: "err", label: "сорвалось" },
  cancelled: { cls: "mute", label: "отменено" },
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
  const { data: prog, error } = usePolling(session.name, (signal) => api.progress(session.name, signal));

  // A once-a-second heartbeat so the running stage's timer TICKS between the 3-second data polls —
  // a step that takes ten minutes must visibly count, or it reads as a hang. Runs only while
  // something is actually running.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (error || !prog?.running && !["queued", "retry_wait", "blocked", "stalled", "recording"].includes(prog?.execution?.state ?? "")) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [prog?.running, prog?.execution?.state, error]);

  useEffect(() => { if (prog) setNow(Date.now()); }, [prog]);

  if (!prog) return <p className={error ? "err" : "hint"}>{error ? `Статус недоступен: ${error}` : "загрузка…"}</p>;
  const clockNow = error && prog.at ? new Date(prog.at).getTime() : now;

  const stages = prog.stages ?? [];
  const heard = new Map(stages.map((s) => [s.stage, s]));
  // Every stage of this session at once — the chain is a whole, not a growing stub. Its shape
  // comes from the session (a link brings a download; a microphone does not), never from what
  // happens to have spoken already.
  const rows = chainFor(!!prog.source, [...heard.keys()]).map((stage) => {
    const s = heard.get(stage);
    const seconds = s?.state === "running" ? elapsed(s.started_at, clockNow)
      : s?.ended_at ? elapsed(s.started_at, new Date(s.ended_at).getTime()) : NaN;
    return {
      duration: Number.isFinite(seconds) && seconds >= 0 ? seconds : null,
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
            {heard.get("download")?.state === "done" && heard.get("extract")?.state === "done"
              ? ". Звук скачан и разложен по кускам — дальше она ничем не отличается от записи встречи: те же реплики, те же версии, тот же плеер."
              : ". Источник должен пройти скачивание и подготовку аудио перед транскрибацией. Готовность каждого этапа показана ниже."}
          </span>
        </p>
      )}

      {prog.partial && <p className="hint" data-nocopy>
        ├ Сохранено до {hms(prog.partial.sealed_end_sec)}
        {" · "}STT до {hms(prog.partial.stt_end_sec)}
        {" · "}текст до {hms(prog.partial.text_end_sec)}
        {" · "}финализация и summary — после Stop и полного STT
      </p>}
      <p className="hint">
        {prog.running ? <b>Задание ещё не завершено.</b> : <b>Текущее состояние результатов.</b>}
        {" "}«Готово» означает, что файл результата проверен. Время ролика не входит в расчёт.
      </p>
      {prog.timing && <p className="hint">
        Ожидание старта: <b>{prog.timing.queue_sec == null ? "—" : hms(prog.timing.queue_sec)}</b>
        {" · "}После старта: <b>{prog.timing.processing_sec == null ? "—" : hms(prog.timing.processing_sec)}</b>
        {" · "}Всего: <b>{prog.timing.total_sec == null ? "—" : hms(prog.timing.total_sec)}</b>
      </p>}
      {prog.execution && <div className={["blocked", "failed", "stalled", "attention", "unavailable"].includes(prog.execution.state) ? "note err" : "note"} data-state={prog.execution.state} data-nocopy aria-live="polite">
        <span>
          <b>{error ? "Текущее выполнение не подтверждено; показан последний полученный статус" : prog.execution.message}</b>
          {prog.execution.current && <> · {NAME[prog.execution.current]}</>}
          {["running", "queued", "retry_wait", "blocked", "stalled", "cancelling"].includes(prog.execution.state) && <>
            {" · "}{prog.execution.state === "running" ? "После запуска" : "Ожидание"}: {prog.execution.since ? hms(Math.max(0, elapsed(prog.execution.since, clockNow))) : "время неизвестно"}
          </>}
          <br />
          Далее: {prog.execution.next.length ? prog.execution.next.map(s => NAME[s]).join(" → ") : prog.execution.state === "untracked" ? "следующие шаги не определены" : "этапов нет"}.
          {prog.execution.retry_at && <><br />Повтор не раньше: {new Date(prog.execution.retry_at).toLocaleString("ru-RU")}.</>}
        </span>
      </div>}

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
                  {s.state === "running" && s.duration != null ? (
                    <span className="hint">⏱ {hms(s.duration)}</span>
                  ) : s.duration != null ? (
                    <span className="hint">
                      {["done", "failed", "invalid", "cancelled"].includes(s.state) ? "за" : "ранее"} {hms(s.duration)}
                    </span>
                  ) : ["done", "failed", "invalid", "cancelled"].includes(s.state) ? <span className="hint">длительность неизвестна</span> : s.state === "running" ? <span className="hint">начало не записано</span> : null}
                </span>
                {s.state === "running" && s.updated_at && <span className="hint">
                  Последний сигнал: {hms(Math.max(0, elapsed(s.updated_at, clockNow)))} назад
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
      <Journal session={session.name} execution={prog.execution} stages={prog.stages} uncertain={!!error} now={clockNow} />
    </>
  );
}
