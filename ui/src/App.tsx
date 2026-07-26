import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { api, type Answer, type Hit, type Jobs, type RecordState, type Session } from "./api";
import { DocumentPane, type Tab } from "./DocumentPane";
import { Player, type SeekRequest } from "./Player";
import { NotesPane } from "./Notes";
import { QueuePane } from "./Queue";
import { RecordBar } from "./RecordBar";
import { SettingsPane } from "./Settings";
import { AskHistory, AskPane } from "./Ask";
import { dayLabel, mmss, shortTitle, stateOf } from "./lib";

type View = "archive" | "note" | "queue" | "settings" | "ask";

/** Cheap and deliberately dumb: only http(s), no spaces. A `file://` from the clipboard would
 *  be an invitation to read any file on the machine through an interface that is open on the
 *  LAN. The real cleaning of the link happens on the server, in one place. */
const isLink = (s: string) => /^https?:\/\/\S+$/.test(s.trim());

export default function App() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [record, setRecord] = useState<RecordState | null>(null);
  const [jobs, setJobs] = useState<Jobs | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [current, setCurrent] = useState<string | null>(null);
  const [view, setView] = useState<View>("archive");
  const [tab, setTab] = useState<Tab>("summary");
  // Selected question in the «Спросить» section (null = the new-question form); `askGen` bumps to
  // refresh the history after a create or when a request finishes.
  const [currentAsk, setCurrentAsk] = useState<string | null>(null);
  const [askGen, setAskGen] = useState(0);
  // A request to move the ONE player to a second. Clicking a transcript line, a search hit or a
  // source in an answer all funnel through here — the player has no other "fragment mode".
  const [seek, setSeek] = useState<SeekRequest | null>(null);
  const [pos, setPos] = useState(0);
  const seekTo = useCallback(
    (sec: number, until?: number) => setSeek((s) => ({ sec, until, n: (s?.n ?? 0) + 1 })),
    [],
  );
  const [toast, setToast] = useState<string | null>(null);

  const [q, setQ] = useState("");
  const [mode, setMode] = useState("");
  const [hits, setHits] = useState<Hit[] | null>(null);

  const say = useCallback((m: string) => {
    setToast(m);
    setTimeout(() => setToast((t) => (t === m ? null : t)), 2600);
  }, []);

  const reload = useCallback(async () => {
    try {
      const [list, r, j] = await Promise.all([
        api.sessions(50),
        api.record().catch(() => null),
        api.jobs().catch(() => null),
      ]);
      setSessions(list);
      if (r) setRecord(r);
      if (j) setJobs(j);
      setErr(null);
      setCurrent((c) => c ?? list.find((s) => !s.recording)?.name ?? list[0]?.name ?? null);
    } catch (e) {
      setErr((e as Error).message);
    }
  }, []);

  // The archive is alive: a recording grows, the queue cooks. Five seconds is often
  // enough to see that and rare enough not to hammer the daemon.
  useEffect(() => {
    void reload();
    const t = setInterval(() => void reload(), 5000);
    return () => clearInterval(t);
  }, [reload]);

  // The recorder has its OWN rhythm, and it is faster. A dictated note lives for seconds — it
  // closes after 3.5 s of silence — so at the archive's five-second beat the indicator would miss
  // most notes outright, and an indicator that is usually absent is worse than none: it teaches
  // you that it is broken. This costs one small file read; the list above walks fifty directories.
  // Different facts, different rhythms.
  useEffect(() => {
    const t = setInterval(() => {
      api.record().then(setRecord).catch(() => {
        /* the daemon may be busy — the next tick is a second away */
      });
    }, 1000);
    return () => clearInterval(t);
  }, []);

  const live = useMemo(() => sessions.find((s) => s.recording) ?? null, [sessions]);
  const session = useMemo(
    () => sessions.find((s) => s.name === current) ?? null,
    [sessions, current],
  );

  const open = (name: string, at?: number) => {
    setView("archive");
    if (name !== current) {
      setCurrent(name);
      setPos(0);
    }
    if (at != null) {
      setTab("transcript");
      seekTo(at); // move the player there — no separate "fragment" concept
    }
  };

  const search = async () => {
    const query = q.trim();
    if (!query) {
      setHits(null);
      return;
    }
    if (isLink(query)) return void ingest(query);
    try {
      setHits(await api.search(query, mode));
    } catch (e) {
      say((e as Error).message);
    }
  };

  /** A link → a session. It appears in the archive AT ONCE, and we open it right away on the
   *  "Ход" tab: a link that vanishes for ten minutes with nothing to look at is
   *  indistinguishable from a link that was dropped. */
  const ingest = useCallback(
    async (url: string) => {
      try {
        const r = await api.ingest(url);
        setQ("");
        setHits(null);
        setView("archive");
        setCurrent(r.session);
        setTab("progress");
        say("Взяли ссылку — качаю");
        await reload();
      } catch (e) {
        say((e as Error).message);
      }
    },
    [reload, say],
  );

  /** Ctrl+V anywhere in the window: a link in the clipboard is an intent, not a search query.
   *  Pasting into a field the person is already typing in is left alone. */
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target && ["INPUT", "TEXTAREA"].includes(target.tagName)) return;
      const text = e.clipboardData?.getData("text")?.trim() ?? "";
      if (!isLink(text)) return;
      e.preventDefault();
      void ingest(text);
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }, [ingest]);

  // Everything not finished: what is in the pot, what waits, and what fell over. `running` is a
  // flag, not a count — the daemon cooks one at a time.
  const queued = (jobs?.running ? 1 : 0) + (jobs?.pending ?? 0) + (jobs?.failed ?? 0);

  return (
    <div className="app">
      <RecordBar
        state={record}
        live={live}
        jobs={jobs}
        cooking={
          jobs?.current ? (sessions.find((s) => s.name === jobs.current)?.title ?? null) : null
        }
        // The chip used to dive into one session's stage chain, which draws its LAST FINISHED
        // run — all green, while the cook was still going. It opens the queue now: the one place
        // that answers "on what, and what is after it".
        openCooking={() => setView("queue")}
        // The voice pill opens the notebook — «записал в идеи» is only half an answer until you
        // can see the note sitting there.
        openVoice={() => setView("note")}
        onChanged={() => void reload()}
        say={say}
      />

      {/* The player is a row of its own, across the whole width: an hour of recording squeezed
          into a 300-pixel rail gave three pixels to a minute, and there was nothing to aim at. */}
      <Player session={view === "archive" ? session : null} seek={seek} onPos={setPos} />

      <aside className="side">
        <div className="side-pad">
          <div className="search">
            <input
              type="search"
              placeholder="Поиск по архиву…"
              value={q}
              onChange={(e) => {
                setQ(e.target.value);
                if (!e.target.value.trim()) setHits(null);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") void search();
              }}
            />
            <select value={mode} onChange={(e) => setMode(e.target.value)} title="Режим поиска">
              <option value="">точный</option>
              <option value="semantic">по смыслу</option>
              <option value="hybrid">гибрид</option>
            </select>
          </div>
          <nav className="nav">
            <button aria-selected={view === "archive"} onClick={() => setView("archive")}>
              📼 Записи <span className="k">{sessions.length}</span>
            </button>
            {/* The queue is a place, not a chip. The count says the machine is alive; only a list
                says what it is chewing and what is behind it. */}
            <button aria-selected={view === "queue"} onClick={() => setView("queue")}>
              ⚙ Очередь
              {/* The badge is the length of the list behind it — anything else makes the person
                  count rows to find out which number lied. */}
              {queued > 0 && <span className="k">{queued}</span>}
            </button>
            {/* Not just a form any more: this is the notebook. It used to only WRITE — a note
                left for a file on another disk and the app never mentioned it again. */}
            <button aria-selected={view === "note"} onClick={() => setView("note")}>
              📝 Заметки
            </button>
            {/* Ad-hoc question to an LLM about a dropped file or pasted text — not tied to a
                recording. Its own place, its own archive under asks/. */}
            <button aria-selected={view === "ask"} onClick={() => setView("ask")}>
              💬 Спросить
            </button>
            <button aria-selected={view === "settings"} onClick={() => setView("settings")}>
              ⚙ Настройки
            </button>
          </nav>
        </div>

        <div className="list">
          {err && <p className="err">{err}</p>}
          {view === "ask" ? (
            // In the «Спросить» section the left rail is the QUESTION history, not the sessions.
            <AskHistory current={currentAsk} onOpen={setCurrentAsk} gen={askGen} />
          ) : isLink(q) ? (
            // A link in the search box is not a search — nobody looks for a URL in their own
            // recordings. It is an intent: "take this and transcribe it".
            <div className="link-offer">
              <b>Это ссылка.</b>
              <span className="url">{q.trim()}</span>
              <span className="hint">
                Скачаем звук и расшифруем — дальше запись ничем не отличается от встречи.
              </span>
              <button className="btn primary" onClick={() => void ingest(q.trim())}>
                ↓ Расшифровать по ссылке
              </button>
            </div>
          ) : hits ? (
            <Hits hits={hits} open={open} />
          ) : (
            <SessionList
              sessions={sessions}
              current={current}
              open={open}
              onChanged={() => void reload()}
              say={say}
            />
          )}
        </div>

        <div className="side-foot">
          {jobs?.running ? (
            <span className="st busy">варится{jobs.pending ? ` (+${jobs.pending})` : ""}</span>
          ) : jobs?.pending ? (
            <span className="st busy">в очереди: {jobs.pending}</span>
          ) : jobs?.failed ? (
            <span className="st err">упало: {jobs.failed}</span>
          ) : (
            <span className="hint">архив</span>
          )}
        </div>
      </aside>

      {view === "note" ? (
        <NotesPane voice={record?.voice ?? null} say={say} />
      ) : view === "ask" ? (
        <AskPane
          current={currentAsk}
          onOpen={setCurrentAsk}
          onChanged={() => setAskGen((g) => g + 1)}
          say={say}
        />
      ) : view === "settings" ? (
        <SettingsPane say={say} />
      ) : view === "queue" ? (
        <QueuePane
          open={(n) => {
            setView("archive");
            setCurrent(n);
            setTab("progress");
          }}
          say={say}
          onChanged={() => void reload()}
        />
      ) : session ? (
        <DocumentPane
          session={session}
          tab={tab}
          setTab={setTab}
          pos={pos}
          onSeek={seekTo}
          onChanged={() => void reload()}
          say={say}
        />
      ) : (
        <main className="main">
          <div className="pane">
            <p className="hint">Записей пока нет.</p>
          </div>
        </main>
      )}

      {/* The rail is the conversation with the archive. One place for it, not two: the answer
          cites fragments, and a click goes straight to them — that is the whole reason the chat
          lives beside the document instead of on a screen of its own. */}
      <aside className="rail">
        <Chat open={open} say={say} />
      </aside>

      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

function SessionList({
  sessions,
  current,
  open,
  onChanged,
  say,
}: {
  sessions: Session[];
  current: string | null;
  open: (n: string) => void;
  onChanged: () => void;
  say: (m: string) => void;
}) {
  const out: ReactNode[] = [];
  let day: string | null = null;
  for (const s of sessions) {
    const d = dayLabel(s);
    if (d !== day) {
      day = d;
      out.push(
        <div className="day" key={`d-${s.name}`}>
          {d}
        </div>,
      );
    }
    out.push(
      <SessionRow
        key={s.name}
        s={s}
        current={s.name === current}
        open={open}
        onChanged={onChanged}
        say={say}
      />,
    );
  }
  return <>{out}</>;
}

/** One session in the list, with a two-step delete.
 *
 *  Deleting audio is the ONE irreversible thing in the app, so the interaction is built to be
 *  impossible to do by accident and effortless to do on purpose:
 *   - the trash icon shows only on hover — you cannot click what you cannot see;
 *   - clicking it does not delete, it turns the row into a named confirm ("Удалить «Созвон»?
 *     насовсем") — a second, deliberate click on a red button, aimed at THIS recording.
 *  No modal, no swipe (a touch gesture a mouse triggers by accident while selecting text). */
function SessionRow({
  s,
  current,
  open,
  onChanged,
  say,
}: {
  s: Session;
  current: boolean;
  open: (n: string) => void;
  onChanged: () => void;
  say: (m: string) => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const st = stateOf(s);
  const doubts = s.summary_doubts ?? s.processed_doubts;

  const del = async () => {
    setBusy(true);
    try {
      await api.deleteSession(s.name);
      say(`Удалено: ${shortTitle(s)}`);
      onChanged();
    } catch (e) {
      say((e as Error).message);
      setBusy(false);
      setConfirming(false);
    }
  };

  if (confirming) {
    return (
      <div className="item confirming">
        <div className="t">
          <span className="name">
            Удалить «{shortTitle(s)}»? <span className="mut">насовсем, вместе со звуком</span>
          </span>
        </div>
        <div className="confirm-actions">
          <button className="btn sm danger-solid" onClick={() => void del()} disabled={busy}>
            {busy ? "удаляю…" : "Удалить"}
          </button>
          <button className="btn sm" onClick={() => setConfirming(false)} disabled={busy}>
            Отмена
          </button>
        </div>
      </div>
    );
  }

  return (
    <div
      className="item"
      role="button"
      tabIndex={0}
      aria-selected={current}
      onClick={() => open(s.name)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") open(s.name);
      }}
    >
      <span className="t">
        <span className="name">{shortTitle(s)}</span>
        <span className="dur">{mmss(s.duration_sec)}</span>
      </span>
      <span className="m">
        <span className={`st ${st.cls}`}>{st.text}</span>
        {s.has_summary && <span>· сводка</span>}
        {doubts && <span className="warnchip">· ⚠ перепроверить</span>}
      </span>
      {/* A live recording is not deletable — the engine is still writing into it. */}
      {!s.recording && (
        <button
          className="trash"
          title="Удалить запись вместе со звуком"
          onClick={(e) => {
            e.stopPropagation(); // the trash deletes, it does not open the session
            setConfirming(true);
          }}
        >
          🗑
        </button>
      )}
    </div>
  );
}

/** Where a hit was found, in the words the tabs use. The index's own names are internal
 *  («transcript», «processed»), and showing them put English field names on a Russian screen. */
const KIND_RU: Record<string, string> = {
  transcript: "реплики",
  processed: "текст",
  summary: "сводка",
};

/** A hit with a timecode plays straight from the results: "found → listened" without
 *  opening the session first. */
function Hits({ hits, open }: { hits: Hit[]; open: (n: string, at?: number) => void }) {
  if (!hits.length) return <p className="hint">Ничего не найдено.</p>;
  return (
    <>
      {hits.map((h, i) => (
        <button
          className="item"
          key={i}
          onClick={() => open(h.session, h.start_sec ?? undefined)}
        >
          <span className="t">
            <span className="name">{h.snippet}</span>
          </span>
          <span className="m">
            <span>{KIND_RU[h.kind] ?? h.kind}</span>
            {h.start_sec != null && <span>· {mmss(h.start_sec)}</span>}
            {h.why && <span>· {h.why}</span>}
          </span>
        </button>
      ))}
    </>
  );
}

interface Turn {
  q: string;
  a?: Answer;
}

/** The conversation with the archive, right beside the document.
 *
 *  The answer is built ONLY from the found fragments and carries links to them: without sources
 *  this would not be "what happened to me" but "what usually happens" — a plausible invention
 *  taken for a fact. A citation is a BUTTON: it opens the recording at that second and plays it.
 *  That is the only way to actually check an answer instead of believing it — and the reason the
 *  chat lives next to the document rather than on a screen of its own. */
function Chat({ open, say }: { open: (n: string, at?: number) => void; say: (m: string) => void }) {
  const [q, setQ] = useState("");
  const [busy, setBusy] = useState(false);
  const [turns, setTurns] = useState<Turn[]>([]);
  const log = useRef<HTMLDivElement>(null);

  useEffect(() => {
    log.current?.scrollTo({ top: log.current.scrollHeight, behavior: "smooth" });
  }, [turns, busy]);

  const ask = async () => {
    const question = q.trim();
    if (!question || busy) return;
    setBusy(true);
    setQ("");
    setTurns((t) => [...t, { q: question }]);
    try {
      const a = await api.ask(question);
      setTurns((t) => t.map((x, i) => (i === t.length - 1 ? { ...x, a } : x)));
    } catch (e) {
      say((e as Error).message);
      setTurns((t) => t.slice(0, -1));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="chat">
      <div className="panel">
        <h3>Спросить у записей</h3>
        <p className="hint" style={{ margin: 0 }}>
          Ответ строится только по вашим записям и ссылается на фрагменты — их можно послушать.
          Чего в записях нет, того не будет и в ответе.
        </p>
      </div>

      <div className="chat-log" ref={log}>
        {turns.length === 0 && (
          <p className="hint">Например: «что решили по бэклогу?», «кто брался за старые тикеты?»</p>
        )}
        {turns.map((t, i) => (
          <Fragment key={i}>
            <div className="msg q">{t.q}</div>
            <div className="msg a">
              {t.a ? (
                <>
                  <p>{t.a.text}</p>
                  {t.a.ungrounded?.length ? (
                    <p className="err">
                      Не подтверждено записями: {t.a.ungrounded.join(", ")} — проверьте по
                      источникам.
                    </p>
                  ) : null}
                  {t.a.sources?.length ? (
                    t.a.sources.map((s) => (
                      <button
                        className="src"
                        key={s.n}
                        onClick={() => open(s.session, s.start_sec ?? undefined)}
                      >
                        <span className="tc">
                          [{s.n}] {s.start_sec != null ? mmss(s.start_sec) : s.kind}
                        </span>
                        {s.text}
                      </button>
                    ))
                  ) : (
                    <p className="hint">Ответ ни на что не сослался — верить ему не за что.</p>
                  )}
                </>
              ) : (
                <p className="hint">ищу в записях и думаю…</p>
              )}
            </div>
          </Fragment>
        ))}
      </div>

      <div className="chat-in">
        <input
          type="search"
          placeholder="Вопрос по записям…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void ask();
          }}
        />
        <button className="btn primary" onClick={() => void ask()} disabled={busy}>
          →
        </button>
      </div>
    </section>
  );
}
