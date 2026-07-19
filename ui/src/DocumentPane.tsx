import { useEffect, useRef, useState } from "react";
import {
  api,
  type Line,
  type Provenance as Prov,
  type Session,
  type Speaker,
  type Version,
} from "./api";
import { ProgressChain } from "./Progress";
import { Markdown, mmss, sessionTitle, stateOf } from "./lib";

export type Tab = "summary" | "transcript" | "processed" | "speakers" | "versions" | "progress";

const TABS: [Tab, string][] = [
  ["summary", "Сводка"],
  ["transcript", "Расшифровка"],
  ["processed", "Читаемый текст"],
  ["speakers", "Участники"],
  ["versions", "Версии"],
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

  const recook = async () => {
    if (!confirm("Выбросить сводку и расшифровку и сварить заново из аудио?\n\nАудиозапись не пострадает."))
      return;
    try {
      const r = await api.recook(session.name);
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
          <button className="tool" onClick={recook} title="Выбросить производное и сварить заново из аудио">
            ♻ Переварить
          </button>
        </div>
      </div>

      <div className="doc" ref={doc}>
        {tab === "transcript" && <Transcript session={session} pos={pos} onSeek={onSeek} />}
        {tab === "summary" && (
          <Article session={session} kind="summary" onChanged={onChanged} say={say} />
        )}
        {tab === "processed" && <Readable session={session} onChanged={onChanged} say={say} />}
        {tab === "speakers" && <Speakers session={session} say={say} />}
        {tab === "versions" && <Versions session={session} say={say} />}
        {tab === "progress" && <ProgressChain session={session} />}
      </div>
    </main>
  );
}

/** Loading an artifact. A stale answer must never touch the screen: clicking another
 *  session while a transcript loads used to hand the player one session's timecodes
 *  and another one's audio. */
function useArtifact<T>(key: string, load: () => Promise<T>) {
  const [data, setData] = useState<T | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const seq = useRef(0);

  useEffect(() => {
    const my = ++seq.current;
    setData(null);
    setErr(null);
    load()
      .then((d) => {
        if (my === seq.current) setData(d);
      })
      .catch((e: Error) => {
        if (my === seq.current) setErr(e.message);
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  return { data, err };
}

function Transcript({
  session,
  pos,
  onSeek,
}: {
  session: Session;
  pos: number;
  onSeek: (sec: number, until?: number) => void;
}) {
  const { data, err } = useArtifact(session.name, () => api.transcript(session.name));
  if (err) return <p className="err">{err}</p>;
  if (!data) return <p className="hint">загрузка…</p>;
  const lines: Line[] = data.lines ?? [];
  if (!lines.length) return <p className="hint">пусто</p>;

  return (
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
  );
}

function Article({
  session,
  kind,
  onChanged,
  say,
}: {
  session: Session;
  kind: "summary";
  onChanged: () => void;
  say: (m: string) => void;
}) {
  const { data, err } = useArtifact(`${session.name}/${kind}`, () => api.markdown(session.name, kind));
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
      const r = await api.recook(session.name);
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
            <b>Стоит перепроверить.</b> В расшифровке этого нет (или сказано другими словами):{" "}
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
      <Markdown text={data.markdown} />
    </>
  );
}

/** The readable text, drawn from DATA rather than from a page.
 *
 *  Nothing here is stored the way it is shown: the daemon joins each line's wording with the
 *  speaker and the timecode on every read. That is why renaming a voice takes effect at once —
 *  there is no name inside the artifact to have gone stale.
 *
 *  And because the artifact is a delta, the screen can say WHAT THE MODEL DID to each line. An
 *  edited line offers its original — the recognizer's own words — so a person can check the
 *  cleanup instead of trusting it. */
function Readable({
  session,
  onChanged,
  say,
}: {
  session: Session;
  onChanged: () => void;
  say: (m: string) => void;
}) {
  const { data, err } = useArtifact(`${session.name}/readable`, () => api.readable(session.name));
  const [shown, setShown] = useState<Set<number>>(new Set());

  const recook = async () => {
    try {
      const r = await api.recook(session.name);
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

  const toggle = (i: number) =>
    setShown((s) => {
      const next = new Set(s);
      if (!next.delete(i)) next.add(i);
      return next;
    });

  return (
    <>
      {session.processed_doubts && (
        <div className="doubt" data-nocopy>
          <span aria-hidden="true">⚠</span>
          <div className="txt">
            <b>Стоит перепроверить.</b> В расшифровке этого нет (или сказано другими словами):{" "}
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
          Тот же разговор без слов-паразитов, обрывов и повторов — но слово в слово по смыслу.
          Исходная расшифровка остаётся на месте: она первична, этот текст производный. Имена
          голосов подставляются при показе — переименование действует сразу, переваривать не надо.
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
        <span className="sep">·</span>
        <span>из расшифровки v{String(data.version_id).padStart(3, "0")}</span>
      </div>
      <div className="lines readable">
        {lines.map((l, i) => (
          <div key={i} className={`ln${l.original ? " edited" : ""}`}>
            <span className="tc">{mmss(l.start_sec)}</span>
            <span className={`sp${l.who === "Я" ? " me" : ""}`}>{l.who}</span>
            <span className="tx">
              {l.text}
              {l.original && (
                <>
                  {" "}
                  <button
                    className="was"
                    data-nocopy
                    title="Показать, что было в расшифровке до чистки"
                    onClick={() => toggle(i)}
                  >
                    ✎
                  </button>
                  {shown.has(i) && (
                    <span className="orig" data-nocopy>
                      было: {l.original}
                    </span>
                  )}
                </>
              )}
            </span>
          </div>
        ))}
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

function Speakers({ session, say }: { session: Session; say: (m: string) => void }) {
  const [gen, setGen] = useState(0);
  const { data, err } = useArtifact(`${session.name}/speakers/${gen}`, () =>
    api.speakers(session.name),
  );
  const { data: src } = useArtifact(`${session.name}/sources/${gen}`, () =>
    api.sources(session.name),
  );

  /** Rename an audio SOURCE — «Я» for a downloaded video is a wrong statement about who spoke.
   *  Empty input puts the default back rather than setting a blank name. */
  const renameSource = async (id: number, current: string) => {
    const who = prompt(`Как подписывать этот голос? (пусто — вернуть по умолчанию)`, current);
    if (who === null) return;
    try {
      const r = await api.nameSource(session.name, id, who.trim());
      say(r.msg);
      setGen((g) => g + 1);
    } catch (e) {
      say((e as Error).message);
    }
  };

  const rename = async (label: string) => {
    const who = prompt(`Как зовут «${label}»?`);
    if (!who?.trim()) return;
    try {
      const r = await api.nameSpeaker(session.name, label, who.trim());
      say(r.msg);
      setGen((g) => g + 1);
    } catch (e) {
      say((e as Error).message);
    }
  };

  if (err) return <p className="err">{err}</p>;
  if (!data) return <p className="hint">загрузка…</p>;
  const people: Speaker[] = data.speakers ?? [];

  return (
    <>
      <p className="note" data-nocopy>
        <span className="i" aria-hidden="true">
          ⓘ
        </span>
        <span>
          Имена живут <b>в этой записи</b>: переименование переподпишет её строки и пересоберёт
          сводку. Звук не переваривается — он не изменился.
        </span>
      </p>
      {/* The source labels. They are what the transcript shows wherever diarization did not name a
          voice — which is EVERY line of a downloaded video, since there is no diarization there at
          all. That is the case where «Я» was plainly wrong. */}
      {(src?.sources ?? []).map((s) => (
        <div className="voice" key={s.source_id}>
          <span className="av" style={{ background: "var(--mute)" }}>
            {s.source_id === 0 ? "1" : "2"}
          </span>
          <span className="grow">
            <b>{s.name}</b>
            <div className="sub">
              {s.what}
              {!s.custom && " · по умолчанию"}
            </div>
          </span>
          <span className="acts">
            <button className="btn sm" onClick={() => renameSource(s.source_id, s.name)}>
              ✎ Переименовать
            </button>
          </span>
        </div>
      ))}

      {people.length === 0 ? (
        // Empty is an ANSWER, not a breakage: either voices were not counted (no model)
        // or none were found. We are not going to invent participants.
        <p className="hint">
          Говорящие не размечены: либо нет модели диаризации, либо в записи не нашлось разборчивой
          речи.
        </p>
      ) : (
        people.map((s, i) => (
          <div className="voice" key={s.label}>
            <span className="av" style={{ background: AVATAR[i % AVATAR.length] }}>
              {s.label.slice(0, 2)}
            </span>
            <span className="grow">
              <b>{s.owner ? "Вы" : s.label}</b>
              <div className="sub">{Math.round(s.speech_sec / 60)} мин речи</div>
            </span>
            {/* The owner is renameable too. «Вы» is a GUESS — true for a recording made here,
                false for anything that arrived by link, where this voice belongs to whoever was in
                the video. Withholding the button asserted the guess was a fact. */}
            <span className="acts">
              <button className="btn sm" onClick={() => rename(s.label)}>
                ✎ Назвать
              </button>
            </span>
          </div>
        ))
      )}
    </>
  );
}

function Versions({ session, say }: { session: Session; say: (m: string) => void }) {
  const [gen, setGen] = useState(0);
  const { data, err } = useArtifact(`${session.name}/versions/${gen}`, () =>
    api.versions(session.name),
  );

  const makeBest = async (id: number) => {
    try {
      await api.setBest(session.name, id);
      say(`Рабочей стала v${String(id).padStart(3, "0")}`);
      setGen((g) => g + 1);
    } catch (e) {
      say((e as Error).message);
    }
  };

  if (err) return <p className="err">{err}</p>;
  if (!data) return <p className="hint">загрузка…</p>;
  const versions: Version[] = (data.versions ?? []).slice().reverse();

  return (
    <>
      <p className="note" data-nocopy>
        <span className="i" aria-hidden="true">
          ⓘ
        </span>
        <span>
          От <b>рабочей</b> версии считаются сводка, поиск и экспорт. Старые не удаляются: аудио
          первично, и из него всё можно сварить заново.
        </span>
      </p>
      {versions.map((v) => (
        <div className="voice" key={v.id}>
          <span className="grow">
            <b>
              v{String(v.id).padStart(3, "0")} · {v.derived ? "причёсано LLM" : "сварено из аудио"}
            </b>
            <div className="sub">
              {v.model} · {(v.created_at ?? "").slice(0, 16).replace("T", " ")} · {v.lines} строк
            </div>
          </span>
          {v.best ? (
            <span className="st done">рабочая</span>
          ) : (
            <span className="acts">
              <button className="btn sm" onClick={() => makeBest(v.id)}>
                ↑ Сделать рабочей
              </button>
            </span>
          )}
        </div>
      ))}
    </>
  );
}
