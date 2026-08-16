import { useEffect, useRef, useState } from "react";
import {
  api,
  type Line,
  type Participant,
  type Provenance as Prov,
  type ReadableLine,
  type SummaryBlock,
  type Session,
  type Version,
} from "./api";
import { ProgressChain } from "./Progress";
import { mmss, sessionTitle, stateOf } from "./lib";

export type Tab = "summary" | "transcript" | "processed" | "speakers" | "progress";

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
  const doc = useRef<HTMLDivElement>(null);
  const st = stateOf(session);

  // Live-refresh while the recording is being processed: re-read the open tab every few seconds so
  // replicas / text / summary appear as each stage finishes — no clicking away and back. It runs
  // ONLY while there is work (recording, queued or cooking), fires one final reload the moment that
  // work ends, and — thanks to `useArtifact`'s in-place reload — never flickers to «загрузка…».
  const live =
    session.recording || session.job === "running" || session.job === "pending";
  const [tick, setTick] = useState(0);
  const wasLive = useRef(false);
  useEffect(() => {
    if (wasLive.current && !live) setTick((n) => n + 1); // finished → one last reload for the result
    wasLive.current = live;
    if (!live) return;
    const t = setInterval(() => setTick((n) => n + 1), 3000);
    return () => clearInterval(t);
  }, [live]);

  // The document goes to the clipboard WITHOUT the system footnotes and without the
  // doubt banner: our own explanation has no business opening someone else's protocol.
  const copy = async () => {
    const box = doc.current?.cloneNode(true) as HTMLElement | undefined;
    if (!box) return;
    box.querySelectorAll("[data-nocopy]").forEach((n) => n.remove());
    const text = (box.innerText ?? "").replace(/\n{3,}/g, "\n\n").trim();
    try {
      await navigator.clipboard.writeText(text);
      say("Скопировано");
    } catch {
      say("Буфер недоступен");
    }
  };

  // The one «Переварить» does what the tab implies: on «Сводка» — only the summary; on
  // «Реплики»/«Текст» — the cleaned text and the summary that follows; elsewhere — everything.
  const scope: "summary" | "text" | "all" =
    tab === "summary" ? "summary" : tab === "transcript" || tab === "processed" ? "text" : "all";
  const scopeWord =
    scope === "summary" ? "сводку" : scope === "text" ? "текст и сводку" : "всё (расшифровку, текст, сводку)";
  const recook = async () => {
    if (!confirm(`Переварить ${scopeWord}?\n\nАудиозапись не пострадает.`)) return;
    try {
      const r = await api.recook(session.name, scope);
      say(r.msg ?? "Поставлено на переварку");
      onChanged();
    } catch (e) {
      say((e as Error).message);
    }
  };

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
          <button className="tool" onClick={copy} title="Скопировать документ (без служебных сносок)">
            ⧉ Копировать
          </button>
          <button className="tool" onClick={recook} title={`Переварить ${scopeWord}`}>
            ♻ Переварить
          </button>
        </div>
      </div>

      <div className="doc" ref={doc}>
        {tab === "transcript" && (
          <Transcript
            session={session}
            pos={pos}
            onSeek={onSeek}
            onChanged={onChanged}
            say={say}
            refresh={tick}
          />
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

/** Loading an artifact. A stale answer must never touch the screen: clicking another
 *  session while a transcript loads used to hand the player one session's timecodes
 *  and another one's audio. */
function useArtifact<T>(key: string, load: () => Promise<T>, refresh = 0) {
  const [data, setData] = useState<T | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const seq = useRef(0);
  const shownKey = useRef<string | null>(null);

  useEffect(() => {
    const my = ++seq.current;
    // Blank ONLY when the identity changed (a new session/tab/version): there we must not show one
    // recording's text over another's. A refresh tick — same `key`, new `refresh` — reloads IN
    // PLACE, keeping the current content on screen until the fresh one arrives, so live-updating
    // during a cook never flickers to «загрузка…».
    if (shownKey.current !== key) {
      setData(null);
      setErr(null);
      shownKey.current = key;
    }
    load()
      .then((d) => {
        if (my === seq.current) {
          setData(d);
          setErr(null);
        }
      })
      .catch((e: Error) => {
        if (my === seq.current) setErr(e.message);
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, refresh]);

  return { data, err };
}

function Transcript({
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
    () => api.transcript(session.name),
    refresh,
  );
  const vers = useArtifact(
    `${session.name}/vr/${gen}`,
    () => api.versions(session.name),
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

  if (err) return <p className="err">{err}</p>;
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
        <p className="hint">пусто</p>
      </>
    );

  return (
    <>
      {switcher}
      <div className="lines">
        {lines.map((l, i) => {
        // The lit line is wherever the ONE player is now. Clicking ▶ moves the player to this
        // line's start, and load() sets the position at once (the clip begins exactly there), so
        // the right line lights immediately — no lag, no "previous line first".
        const sounding = pos >= l.start_sec && pos < l.end_sec;
        return (
          <div key={i} className={`ln${sounding ? " playing" : ""}`}>
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

/** THE SUMMARY, with every claim tied to the seconds it came from.
 *
 *  A summary is the one artifact a person cannot check by reading it: the line-by-line view sits
 *  next to the recording, but this is prose about half an hour of speech. So each claim carries
 *  the lines the model drew it from — verified at cook time, because an unverified citation is
 *  just another confident answer — and the button beside it plays exactly that moment.
 *
 *  A claim with no button is one whose references did not check out. It keeps its words: the
 *  check is literal and gets things wrong, and a summary emptied by its own verifier is a failure
 *  this app has already paid for twice. */
function Article({
  session,
  onSeek,
  onChanged,
  say,
  refresh,
}: {
  session: Session;
  onSeek: (sec: number, until?: number) => void;
  onChanged: () => void;
  say: (m: string) => void;
  refresh: number;
}) {
  const { data, err } = useArtifact(
    `${session.name}/summary`,
    () => api.summary(session.name),
    refresh,
  );
  const kind = "summary" as const;
  const doubts = session.summary_doubts;

  const confirm = async () => {
    try {
      const r = await api.confirm(session.name, kind);
      say(r.msg);
      onChanged();
    } catch (e) {
      say((e as Error).message);
    }
  };

  const recook = async () => {
    try {
      const r = await api.recook(session.name, "summary");
      say(r.msg ?? "Поставлено на переварку");
      onChanged();
    } catch (e) {
      say((e as Error).message);
    }
  };

  if (err) return <p className="err">{err}</p>;
  if (!data) return <p className="hint">загрузка…</p>;

  return (
    <>
      {/* The doubt mark is a NOTE, not a verdict. The check compares literally and does
          get it wrong — on a live recording it called "Сергей" an invention, and he had
          been said out loud. The person decides: they can listen to the recording, the
          machine cannot. Both exits stand right here. */}
      {doubts && (
        <div className="doubt" data-nocopy>
          <span aria-hidden="true">⚠</span>
          <div className="txt">
            <b>Стоит перепроверить.</b> В записи этого нет (или сказано другими словами):{" "}
            <code>{doubts}</code>. Может быть враньё модели, а может — неточность. Решаете вы:
            запись можно послушать.
          </div>
          <div className="acts">
            <button className="btn" onClick={confirm}>
              ✓ Всё верно
            </button>
            <button className="btn" onClick={recook}>
              ♻ Переварить
            </button>
          </div>
        </div>
      )}
      <Provenance p={data.provenance} />
      <div className="summary">
        {(data.blocks ?? []).map((b: SummaryBlock, i: number) => {
          if (b.kind === "heading") return <h2 key={i}>{b.text}</h2>;
          const src = b.sources?.[0];
          const cite = src && (
            <button
              className="cite"
              data-nocopy
              title={`Послушать, откуда это: ${mmss(src.start_sec)}`}
              onClick={() => onSeek(src.start_sec, b.sources[b.sources.length - 1].end_sec)}
            >
              ▶ {mmss(src.start_sec)}
            </button>
          );
          return b.kind === "bullet" ? (
            <div className="claim" key={i}>
              <span className="tx">{b.text}</span>
              {cite}
            </div>
          ) : (
            <p key={i}>
              {b.text} {cite}
            </p>
          );
        })}
      </div>
    </>
  );
}

/** THE TEXT AS A BOOK. The view for reading and for copying — the one a person forwards.
 *
 *  Nothing here is stored the way it is shown: the daemon joins each line's wording with its
 *  speaker on every read, and this screen turns those lines into continuous prose. Timecodes,
 *  labels, brackets — none of it exists in the copy, because a page pasted into a letter should
 *  need no cleaning up. The line-by-line view next door is the one anchored to the audio.
 *
 *  WHERE PARAGRAPHS COME FROM, and why it is not one rule but three. Transcript lines are cut by
 *  the recognizer, not by meaning, and their size depends on which version was cleaned. Measured
 *  19.07.2026 on the owner's archive:
 *
 *    20260718_230541_youtube  76 lines, median 247 chars, NOT ONE gap between lines
 *    20260716_164443         149 lines, median  77 chars, 16 gaps, up to 48 s
 *    20260714_181036         217 lines, median  64 chars, 54 gaps
 *
 *  A refined version has its windows glued, so pauses simply do not exist in it — a pause-only
 *  rule turned those 76 already-paragraph-sized lines into ONE paragraph of nineteen minutes.
 *  A meeting is the opposite: short scraps that must be joined or the page reads like a chat log.
 *
 *  So: a change of speaker always breaks; a real pause breaks; and a paragraph that has grown to a
 *  book's length breaks at the end of a sentence.
 *
 *  THE LAST RULE IS TYPOGRAPHY AND NOTHING MORE, and it is here because the alternative was
 *  measured too. Without it, with only speaker changes and real pauses:
 *
 *    20260718_230541_youtube   6 paragraphs, the largest 14 952 characters — five unbroken pages
 *    20260713_220457          28 paragraphs, the largest  8 326
 *
 *  A wall like that is not «honest», it is unreadable. But it breaks ONLY at a full stop, and only
 *  after a paragraph is already book-sized, so it never cuts a thought in half. */
const PARAGRAPH_PAUSE_SEC = 2;
/** A book's paragraph. Below this we do not look for a place to break at all. */
const PARAGRAPH_SOFT_CHARS = 900;

/** The cleanup sometimes returns a line that already opens with a dash — measured on the owner's
 *  archive, 6 of 76 lines in one session, 8 of 217 in another. Prefixing ours on top of it prints
 *  «— — текст», and that lands in the clipboard too. */
const OPENS_WITH_DASH = /^[\s]*[—–-][\s]/;

interface Turn {
  who: string;
  /** One speaker's stretch of speech, already broken into paragraphs. */
  paras: string[];
}

/** A sentence boundary IN THE TEXT: a full stop, then space, then something that starts a
 *  sentence.
 *
 *  This is the difference between breaking по предложениям and breaking «как попало». Paragraphs
 *  used to be cut only where one transcript LINE ended and the next began — and those boundaries
 *  are the cook's 15-second clock, not the speaker's punctuation. A line whose last character
 *  happened to be a dot looked like the end of a thought and was not. Measured on the owner's
 *  video, the page he objected to:
 *
 *  ```text
 *  paragraph ends:   …С, почему я?
 *  paragraph starts: Я не сделал. Типа, я триллионы просмотров собрал…
 *  ```
 *
 *  Requiring a capital letter after the stop is also what keeps a cut word from opening a
 *  paragraph: «Топ-три. ...ри качества» has a dot, but «...» is not the start of a sentence, so
 *  there is no boundary there to break at.
 *
 *  AND AN ELLIPSIS IS NOT A FULL STOP HERE. The recognizer marks a window cut off mid-phrase with
 *  «...», so a fragment ending in one has not finished its sentence — the window finished. Left in
 *  the set it produced exactly the boundary this rule exists to prevent:
 *
 *  ```text
 *  paragraph ends:   …вот единственная проблема, которая...
 *  paragraph starts: Отличается наша экономика от западной…
 *  ```
 *
 *  A real ellipsis therefore never ends a paragraph either. That is the cheap half of the trade:
 *  an ellipsis rarely closes a thought, and a cut always does not. */
const SENTENCE_BREAK = /(?<![.…])([.!?]["»)]?)\s+(?=[«"(]?[А-ЯЁA-Z0-9])/g;

/** Lines → turns. Consecutive lines by the same speaker become one turn; a pause inside a turn
 *  starts a new paragraph; a long paragraph is cut at a sentence boundary. */
export function toTurns(lines: ReadableLine[]): Turn[] {
  const turns: Turn[] = [];
  let last: ReadableLine | null = null;
  // Blocks of continuous speech: what a pause or a change of speaker separates. Paragraphing
  // happens INSIDE a block, over its whole text, so it never depends on where a line was cut.
  const push = (who: string, block: string) => {
    const paras = paragraphs(block);
    const turn = turns[turns.length - 1];
    if (turn && turn.who === who) turn.paras.push(...paras);
    else turns.push({ who, paras });
  };
  let block = "";
  for (const l of lines) {
    const text = l.text.trim();
    if (!text) continue;
    const paused = last ? l.start_sec - last.end_sec >= PARAGRAPH_PAUSE_SEC : false;
    if (!last || l.who !== last.who || paused) {
      if (last && block) push(last.who, block);
      block = text;
    } else {
      block += " " + text;
    }
    last = l;
  }
  if (last && block) push(last.who, block);
  return turns;
}

/** One block of continuous speech → paragraphs of about a book's length, cut at sentence ends.
 *
 *  Sentences are never split: a paragraph goes over the target rather than cutting a thought in
 *  half, and a single sentence longer than the whole target simply stands alone. */
function paragraphs(block: string): string[] {
  // `split` on a pattern with a capturing group interleaves the captured terminators, so the
  // «.»/«?» is not lost — it comes back as the next element and is glued onto its own sentence.
  const pieces = block.split(SENTENCE_BREAK);
  const parts: string[] = [];
  for (let i = 0; i < pieces.length; i += 2) {
    const s = ((pieces[i] ?? "") + (pieces[i + 1] ?? "")).trim();
    if (s) parts.push(s);
  }
  const out: string[] = [];
  let open = "";
  for (const s of parts) {
    if (open && open.length >= PARAGRAPH_SOFT_CHARS) {
      out.push(open);
      open = s;
    } else {
      open = open ? open + " " + s : s;
    }
  }
  if (open) out.push(open);
  return out;
}

function Readable({
  session,
  onChanged,
  say,
  refresh,
}: {
  session: Session;
  onChanged: () => void;
  say: (m: string) => void;
  refresh: number;
}) {
  const { data, err } = useArtifact(
    `${session.name}/readable`,
    () => api.readable(session.name),
    refresh,
  );

  const recook = async () => {
    try {
      const r = await api.recook(session.name, "text");
      say(r.msg ?? "Поставлено на переварку");
      onChanged();
    } catch (e) {
      say((e as Error).message);
    }
  };

  if (err) return <p className="err">{err}</p>;
  if (!data) return <p className="hint">загрузка…</p>;

  const lines = data.lines ?? [];
  const edited = lines.filter((l) => l.original).length;
  const turns = toTurns(lines);
  // One voice — a lecture, a video, a dictation — is not a dialogue, so nothing marks a change of
  // speaker: there is none. Plain paragraphs, the way a book prints a monologue.
  const solo = new Set(turns.map((t) => t.who)).size <= 1;

  return (
    <>
      {session.processed_doubts && (
        <div className="doubt" data-nocopy>
          <span aria-hidden="true">⚠</span>
          <div className="txt">
            <b>Стоит перепроверить.</b> В записи этого нет (или сказано другими словами):{" "}
            <code>{session.processed_doubts}</code>. Решаете вы: запись можно послушать.
          </div>
          <div className="acts">
            <button className="btn" onClick={recook}>
              ♻ Переварить
            </button>
          </div>
        </div>
      )}
      <p className="note" data-nocopy>
        <span className="i" aria-hidden="true">
          ⓘ
        </span>
        <span>
          Тот же разговор без слов-паразитов, обрывов и повторов — слово в слово по смыслу.
          Копируется как есть: ни таймкодов, ни меток, чистить после вставки не надо. Проверить
          кусок по звуку можно рядом, по строкам.
        </span>
      </p>
      <Provenance p={data.provenance} />
      {/* How much cleanup actually happened. «Модель ничего не поправила» and «модель молчала»
          are different facts, and a document that hid the difference let 120 lost lines look like
          a job well done. */}
      <div className="prov" data-nocopy>
        <span>
          поправлено <b>{edited}</b> из <b>{lines.length}</b> строк
        </span>
        {data.omitted > 0 && (
          <>
            <span className="sep">·</span>
            <span>модель не ответила по {data.omitted}</span>
          </>
        )}
        {data.rejected > 0 && (
          <>
            <span className="sep">·</span>
            <span>откачено проверкой: {data.rejected}</span>
          </>
        )}
      </div>
      <div className="book">
        {turns.map((t, i) =>
          t.paras.map((p, j) => (
            // A change of speaker is marked the way a book marks it: the dash of direct speech, on
            // the first paragraph of a turn only — a person continuing after a pause is still the
            // same person, and Russian typography gives them no second dash.
            //
            // The dash is in the TEXT, not in a `::before`: a pseudo-element does not survive the
            // clipboard, and the dialogue would paste as an undifferentiated wall.
            <p key={`${i}.${j}`}>{!solo && j === 0 && !OPENS_WITH_DASH.test(p) ? `— ${p}` : p}</p>
          )),
        )}
      </div>
    </>
  );
}

/** What produced this text. A derived document is only disposable because it can be rebuilt — and
 *  it can only be rebuilt if you know which model and which template made it. Comparing two
 *  summaries of the same recording is the whole point of being able to swap models, and without
 *  this line the two are indistinguishable.
 *
 *  Absent for a document written before the header existed, or one edited by hand. Then there is
 *  nothing to show — and inventing a plausible model name would be the worst of both. */
function Provenance({ p }: { p?: Prov | null }) {
  if (!p) return null;
  // Sliced, not re-parsed into a Date: the stamp already carries the offset it was written in,
  // and re-rendering it in the browser's zone would silently restate WHEN the cook ran.
  const when = p.at ? p.at.slice(0, 16).replace("T", " ") : "";
  return (
    <div className="prov" data-nocopy>
      <span>
        сварено <b>{p.model || "—"}</b>
      </span>
      {p.template && (
        <>
          <span className="sep">·</span>
          <span>шаблон {p.template}</span>
        </>
      )}
      <span className="sep">·</span>
      <span>глоссарий: {p.glossary} {plural(p.glossary, "замена", "замены", "замен")}</span>
      {when && (
        <>
          <span className="sep">·</span>
          <span>{when}</span>
        </>
      )}
    </div>
  );
}

/** 1 замена, 2 замены, 5 замен. */
function plural(n: number, one: string, few: string, many: string): string {
  const mod100 = n % 100;
  if (mod100 >= 11 && mod100 <= 14) return many;
  const mod10 = n % 10;
  if (mod10 === 1) return one;
  if (mod10 >= 2 && mod10 <= 4) return few;
  return many;
}

const AVATAR = ["#2f6ea3", "#8a6fbc", "#2c7a52", "#9d6a12", "#b0554a"];

/** WHO IS IN THIS RECORDING — one list, built from the lines the document actually shows.
 *
 *  There used to be two lists side by side: the audio SOURCES and the diarization ROSTER. Neither
 *  said what it was, so the screen showed «Собеседники» and «Участник 1» as peers with no hint of
 *  the difference, and it offered renames for people who say nothing. Measured on the owner's
 *  archive: the video's roster listed a «Участник 1» with not one line in the document, and the
 *  meeting's listed a «Участник 3» the same way.
 *
 *  The daemon now groups the lines themselves, so a phantom has nothing to be grouped from, the
 *  minutes are the minutes in the text, and every entry says WHY it is called what it is called. */
function Speakers({
  session,
  say,
  refresh,
}: {
  session: Session;
  say: (m: string) => void;
  refresh: number;
}) {
  const [gen, setGen] = useState(0);
  const { data, err } = useArtifact(
    `${session.name}/speakers/${gen}`,
    () => api.speakers(session.name),
    refresh,
  );

  /** One button, two mechanisms — genuinely different, which is why the row says which ones it
   *  is made of. Naming a VOICE re-labels its lines everywhere and remembers the print for later
   *  recordings; naming a SOURCE only changes what unattributed lines are called.
   *
   *  A row can be made of both, and then both are renamed. Sequentially and reporting the first
   *  failure: half a rename leaves some lines under the old name, and the person has to see that
   *  rather than find it later in the text. */
  const rename = async (p: Participant) => {
    const hasVoice = p.voices.length > 0;
    const asked = prompt(
      hasVoice
        ? `Как зовут «${p.name}»? Голос запомнится и будет узнаваться в других записях.`
        : `Как подписывать этот звук? (пусто — вернуть по умолчанию)`,
      hasVoice ? "" : p.name,
    );
    if (asked === null) return;
    const name = asked.trim();
    if (hasVoice && !name) return say("Имя не может быть пустым");
    try {
      let msg = "";
      for (const v of p.voices) msg = (await api.nameSpeaker(session.name, v, name)).msg;
      for (const id of p.sources) msg = (await api.nameSource(session.name, id, name)).msg;
      say(msg);
      setGen((g) => g + 1);
    } catch (e) {
      say((e as Error).message);
      setGen((g) => g + 1);
    }
  };

  if (err) return <p className="err">{err}</p>;
  if (!data) return <p className="hint">загрузка…</p>;
  const people: Participant[] = data.speakers ?? [];

  return (
    <>
      <p className="note" data-nocopy>
        <span className="i" aria-hidden="true">
          ⓘ
        </span>
        <span>
          Здесь только те, кому в этой записи приписаны реплики. <b>Отдельный голос</b> — его
          выделила диаризация, имя запомнится и будет узнаваться дальше. <b>Микрофон</b> и{" "}
          <b>системный звук</b> — это не голос, а вход: так подписаны реплики, в которых голос
          выделить не удалось. Переименование меняет подписи сразу; пересобирается только сводка.
        </span>
      </p>

      {people.length === 0 ? (
        // Empty is an ANSWER, not a breakage: nothing has been attributed yet because nothing has
        // been cooked. We are not going to invent participants to fill the screen.
        <p className="hint">
          Пока некому: в записи нет ни одной расшифрованной реплики.
        </p>
      ) : (
        people.map((p, i) => (
          <div className="voice" key={p.name}>
            <span
              className="av"
              style={{
                background: p.voices.length ? AVATAR[i % AVATAR.length] : "var(--mute)",
              }}
            >
              {p.name.slice(0, 2)}
            </span>
            <span className="grow">
              <b>{p.name}</b>
              <div className="sub">
                {p.what}
                {!p.named && " · имени пока нет"}
                {" · "}
                {Math.round(p.speech_sec / 60)} мин, реплик: {p.lines}
              </div>
            </span>
            <span className="acts">
              <button className="btn sm" onClick={() => void rename(p)}>
                ✎ {p.named ? "Переименовать" : "Назвать"}
              </button>
            </span>
          </div>
        ))
      )}
    </>
  );
}
