import { useEffect, useState } from "react";
import { api, type Jobs, type RecordState, type Session, type VoiceStatus } from "./api";
import { mmss, shortTitle } from "./lib";

interface Props {
  state: RecordState | null;
  live: Session | null;
  jobs: Jobs | null;
  /** The name of what is being cooked, as a person would call it — not the directory. */
  cooking: string | null;
  openCooking: () => void;
  /** The voice pill opens the notebook — the notes that actually landed. */
  openVoice: () => void;
  onChanged: () => void;
  say: (m: string) => void;
}

/** The record bar. Permanent, and with TWO honest states.
 *
 *  Recording is NOT the default: capture runs always, but nothing reaches the disk until a
 *  human says so. A recorder left running all day through an office writes other people's
 *  conversations, and none of them agreed to that.
 *
 *  What makes the button safe is the pre-roll ring — the last minutes live in memory and
 *  enter the session at the press. That is what the chip on the right is about: it is not a
 *  decoration, it is the answer to "I forgot to press it". */
/** The button asks; the ENGINE decides. Between the two there is a real gap: the request travels
 *  through a file, and the recording loop looks at it about once a second.
 *
 *  So the press has a state of its own — "starting" — and it holds until the engine confirms that
 *  the recording is actually running. Painting "● recording" on faith would be the one lie this
 *  system must never tell: a person would walk away believing they are being recorded. */
const CONFIRM_TIMEOUT_MS = 12_000;
const CONFIRM_POLL_MS = 350;

export function RecordBar({
  state,
  live,
  jobs,
  cooking,
  openCooking,
  openVoice,
  onChanged,
  say,
}: Props) {
  const [now, setNow] = useState(() => Date.now());
  const [pending, setPending] = useState<"start" | "stop" | null>(null);

  const recording = state?.recording ?? false;

  useEffect(() => {
    if (!recording) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [recording]);

  const started = live?.started_at ? new Date(live.started_at).getTime() : null;
  const elapsed = started ? (now - started) / 1000 : (live?.duration_sec ?? 0);

  /** Wait for the engine to actually say so. Not a sleep with a guess of a delay: we ask, and we
   *  stop asking the moment the answer changes. */
  const confirm = async (want: boolean): Promise<boolean> => {
    const deadline = Date.now() + CONFIRM_TIMEOUT_MS;
    while (Date.now() < deadline) {
      try {
        const r = await api.record();
        if (r.recording === want) return true;
      } catch {
        /* the daemon may be busy — keep asking until the deadline */
      }
      await new Promise((r) => setTimeout(r, CONFIRM_POLL_MS));
    }
    return false;
  };

  const start = async (named: boolean) => {
    const title = named ? prompt("Название записи (можно пусто):", "") : "";
    if (title === null) return;
    setPending("start");
    try {
      await api.startRecording(title.trim());
    } catch (e) {
      setPending(null);
      say((e as Error).message);
      return;
    }
    const ok = await confirm(true);
    setPending(null);
    onChanged();
    say(
      ok
        ? `Пишу. Последние ${mmss(state?.preroll_sec ?? 0)} из буфера вошли в запись`
        : "Движок не подтвердил старт — проверьте, запущен ли он",
    );
  };

  const stop = async () => {
    setPending("stop");
    let name = "";
    try {
      name = (await api.stopRecording()).session;
    } catch (e) {
      setPending(null);
      say((e as Error).message);
      return;
    }
    const ok = await confirm(false);
    setPending(null);
    onChanged();
    say(ok ? `Остановлена: ${name} — ушла в обработку` : "Движок не подтвердил остановку");
  };

  const dead = state != null && !state.engine;
  const voice = state?.voice ?? null;

  return (
    <header className="bar">
      <div className="brand">
        localvox <span>light</span>
      </div>

      {recording ? (
        // ONE button. It is the same place, and it is where the hand already knows to go: you
        // pressed here to start, you press here to end. A stop hiding on the other side of the bar
        // is a stop hunted for while the recording runs.
        <>
          <button
            className="rec-btn stop"
            onClick={() => void stop()}
            disabled={pending === "stop"}
            title="Остановить запись"
          >
            <span className="glyph square" />
            {pending === "stop" ? "останавливаю…" : "Остановить"}
          </button>
          {/* Recording — and, if a note is being dictated at the same time, the SAME pill says so.
              Two capture indicators side by side was the confusion: a second blinking thing next
              to the recording one reads as "am I being recorded twice?". One place answers
              «что сейчас захватывается», whatever it is. */}
          <div className={`recpill${voice?.capturing ? " plus-note" : ""}`}>
            <span className="dot" />
            <span className="who">{live ? shortTitle(live) : (state?.session ?? "запись")}</span>
            <span className="clock">{mmss(elapsed)}</span>
            {voice?.capturing && <DictationSegment cap={voice.capturing} />}
          </div>
          <div className="grow" />
        </>
      ) : pending === "start" ? (
        // Asked — not yet confirmed. The request travels through a file, and the recording loop
        // reads it about once a second. In that gap the button used to look broken; painting
        // "● recording" on faith instead would be worse than broken — it would be a lie.
        <>
          <div className="recpill waiting">
            <span className="dot wait" />
            <span className="who">запись включается…</span>
          </div>
          <div className="grow" />
        </>
      ) : voice?.capturing ? (
        // Dictating with no meeting running. It takes the recording pill's PLACE and shape, so the
        // eye has one spot to look at for «что-то пишется». Amber, not red: a note goes into a slot
        // file, not into a session — different destination, honestly different colour.
        <>
          <button
            className="rec-btn"
            onClick={() => void start(false)}
            disabled={dead}
            title="Начать запись встречи"
          >
            <span className="glyph" /> Запись
          </button>
          <button className="recpill dictating" onClick={openVoice} title="Идёт диктовка заметки. Закончить: «всё», «конец», «стоп» — либо помолчите 3.5 секунды.">
            <DictationSegment cap={voice.capturing} />
          </button>
          <div className="grow" />
        </>
      ) : (
        <>
          <button
            className="rec-btn"
            onClick={() => void start(false)}
            disabled={dead}
            title={dead ? "Движок не запущен — записывать нечем" : "Начать запись"}
          >
            <span className="glyph" /> Запись
          </button>
          <button className="btn" onClick={() => void start(true)} disabled={dead}>
            🎙 С названием…
          </button>
          <div className="grow" />
          {dead ? (
            <span className="err">движок не запущен</span>
          ) : (
            state && (
              <span
                className="preroll"
                title="Звук последних минут держится в памяти и не пишется на диск. Нажмёте «Запись» — он войдёт в неё: разговор, начавшийся до кнопки, не потерян."
              >
                ⟲ буфер <b>{mmss(state.preroll_sec)}</b>
              </span>
            )
          )}
        </>
      )}

      {/* NOT a capture indicator — it never pulses and never takes the pill's place. It answers a
          different question: is the module alive, and did the last note land. Those are facts about
          the past and about readiness; «что пишется прямо сейчас» is the pill's job alone. */}
      <VoiceStatusChip voice={voice} open={openVoice} />
      <Queue jobs={jobs} cooking={cooking} open={openCooking} />
      <Theme />
    </header>
  );
}

/** How long the receipt stays on screen before the pill returns to «слушаю». Long enough to notice
 *  after looking away, short enough that it never reads as the current state. */
const RECEIPT_VISIBLE_MS = 90_000;

/** How long ago, in words. A receipt with no time on it does not answer «когда». */
const ago = (iso?: string) => {
  if (!iso) return "";
  const sec = (Date.now() - new Date(iso).getTime()) / 1000;
  if (!isFinite(sec) || sec < 0) return "";
  if (sec < 60) return "только что";
  if (sec < 3600) return `${Math.round(sec / 60)} мин назад`;
  if (sec < 86400) return `${Math.round(sec / 3600)} ч назад`;
  return `${Math.round(sec / 86400)} дн назад`;
};

/** The dictation itself, inside whichever pill is showing it.
 *
 *  It lives in the capture pill — the same one that reports a meeting — because «что сейчас
 *  захватывается» must have exactly ONE place to look. A second indicator beside the first was the
 *  confusion: two things blinking at once read as "am I being recorded twice?". */
function DictationSegment({ cap }: { cap: NonNullable<VoiceStatus["capturing"]> }) {
  return (
    <span className="note-seg">
      <span className="dot dictating-dot" />
      <span className="slot">{cap.slot}</span>
      {/* The text grows phrase by phrase — that growth IS the proof it is still listening. */}
      <span className="said">{cap.text || "слушаю…"}</span>
    </span>
  );
}

/** The voice module's own state — readiness and the last receipt. NOT a capture indicator.
 *
 *  It never pulses and never takes the capture pill's place, because it answers different
 *  questions: is the module alive at all, and did the last note actually land. Both are facts about
 *  the past or about readiness. «Пишется прямо сейчас» belongs to the capture pill alone — mixing
 *  the two is what made a finished note look like an ongoing recording. */
function VoiceStatusChip({ voice, open }: { voice: VoiceStatus | null; open: () => void }) {
  if (!voice) return null;
  // While dictating, the capture pill is already saying it. Repeating it here would put the same
  // fact on screen twice, in two shapes.
  if (voice.capturing) return null;

  // The receipt is shown, then it goes back to «слушаю».
  //
  // «Не вижу, когда деактивировался»: a receipt that stays forever answers «что записалось» but
  // stops answering «а сейчас-то что» — ten minutes later it still reads as the current state. So
  // it is a transient, and the resting state is itself the answer «модуль дослушал и снова ждёт».
  // The receipt is not lost — it lives on in the notebook, which this chip clicks through to.
  //
  // A FAILURE NEVER FADES: «не записалось» must not dissolve into a calm «слушаю», because it is
  // the one outcome the person has to act on.
  const last = voice.last;
  const settled =
    last && !last.error && Date.now() - new Date(last.at).getTime() > RECEIPT_VISIBLE_MS;

  if (last && !settled) {
    return (
      <button
        className={`vchip${last.error ? " bad" : " done"}`}
        onClick={open}
        title={
          last.error
            ? `Не записалось: ${last.error}`
            : `Записано в ${last.dest}. Модуль дослушал и снова ждёт команду.`
        }
      >
        <span className="tick" aria-hidden="true">
          {last.error ? "✕" : "✓"}
        </span>
        <span className="said">
          {last.error ? "не записалось" : `записано в ${last.slot}`}
        </span>
        <span className="when">{ago(last.at)}</span>
      </button>
    );
  }

  // Alive and quiet. Deliberately the faintest thing in the bar: it is a readiness light, not an
  // event, and it must never compete with the pill that reports actual capture.
  return (
    <button className="vchip idle" onClick={open} title={voice.detail}>
      🎙 {voice.active ? "слушаю" : "голос выключен"}
    </button>
  );
}

/** What the secretary is doing with the archive right now.
 *
 *  It lives in the top bar and not in a corner of the list, because the answer to "is it still
 *  working or has it hung" must not depend on which screen you happen to be on. The dot pulses
 *  while work is running: a static one is indistinguishable from a frozen one.
 *
 *  A click opens the chain of stages of exactly the recording being cooked — "where is it now"
 *  is the very next question after "is it working". */
function Queue({
  jobs,
  cooking,
  open,
}: {
  jobs: Jobs | null;
  cooking: string | null;
  open: () => void;
}) {
  if (!jobs) return null;
  const { running, pending, failed } = jobs;
  if (!running && !pending && !failed) return null;

  if (!running && failed) {
    return (
      <button className="queue failed" onClick={open} title="Обработка сорвалась — откройте «Ход»">
        <span className="qdot err" />
        сорвалось: {failed}
      </button>
    );
  }

  return (
    <button className="queue" onClick={open} title="Что обрабатывается сейчас — откроется «Ход»">
      <span className="qdot busy" />
      {running ? (
        <>
          обрабатывается{cooking ? <b>{cooking}</b> : null}
        </>
      ) : (
        <>в очереди</>
      )}
      {pending > 0 && <span className="mut">+{pending}</span>}
    </button>
  );
}

function Theme() {
  const flip = () => {
    const root = document.documentElement;
    const cur =
      root.getAttribute("data-theme") ??
      (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
    root.setAttribute("data-theme", cur === "dark" ? "light" : "dark");
  };
  return (
    <button className="icon-btn" onClick={flip} title="Тема">
      ◐
    </button>
  );
}
