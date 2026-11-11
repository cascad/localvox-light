import { api, acceptsDocument, type ReadableLine, type Session } from "../api";
import { mmss } from "../lib";
import { useArtifact } from "../useArtifact";
import { PartialStatus } from "./partial";
import { Provenance } from "./provenance";

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


export function Readable({
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
    signal => api.readable(session.name, signal),
    refresh,
    acceptsDocument,
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

  if (err && !data) return <p className="err">{err}</p>;
  if (!data) return <p className="hint">загрузка…</p>;

  const lines = data.lines ?? [];
  const edited = lines.filter((l) => l.original).length;
  const turns = toTurns(lines);
  // One voice — a lecture, a video, a dictation — is not a dialogue, so nothing marks a change of
  // speaker: there is none. Plain paragraphs, the way a book prints a monologue.
  const solo = new Set(turns.map((t) => t.who)).size <= 1;

  return (
    <>
      <PartialStatus info={data.partial} />
      {err && <p className="hint" data-nocopy>Обновление недоступно; показана последняя готовая часть.</p>}
      {!data.from_transcript && session.processed_doubts && (
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
          {data.from_transcript
            ? "Отдельной очищенной версии нет. Исходная расшифровка · Ctrl+Shift+C — скопировать весь текст."
            : "Тот же разговор без слов-паразитов, обрывов и повторов — слово в слово по смыслу."}
          {!data.from_transcript && <> Копируется без таймкодов и служебных меток. Проверить кусок по звуку можно во вкладке «Реплики».</>}
        </span>
      </p>
      <Provenance p={data.provenance} />
      {/* How much cleanup actually happened. «Модель ничего не поправила» and «модель молчала»
          are different facts, and a document that hid the difference let 120 lost lines look like
          a job well done. */}
      {!data.from_transcript && <div className="prov" data-nocopy>
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
      </div>}
      {edited > 0 && <details data-nocopy className="prov">
        <summary>Исходные реплики ({edited})</summary>
        {lines.filter((line) => line.original).map((line, index) => <div key={index}>
          <span className="tc">{mmss(line.start_sec)}</span> {line.who}: {line.original}
        </div>)}
      </details>}
      <div className="book">
        {data.from_transcript ? <p className="raw-transcript" data-plain-text>
          {lines.map((line, i) => <span key={line.id ?? i}>{i > 0 ? " " : ""}{line.text.trim()}</span>)}
        </p> : turns.map((t, i) =>
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
