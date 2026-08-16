import { useEffect, useRef, useState } from "react";
import type { DragEvent, KeyboardEvent } from "react";
import { api, type AskRecord, type AskStatus, type AskSummary } from "./api";
import { Markdown } from "./md";

/** Copy to clipboard with a one-word acknowledgement. */
async function copyText(text: string, say: (m: string) => void) {
  try {
    await navigator.clipboard.writeText(text);
    say("Скопировано");
  } catch {
    say("Буфер недоступен");
  }
}

/** «Спросить у LLM про файл / текст» — an ad-hoc request that behaves like a session: created
 *  instantly (Pending), answered by the daemon's worker in the background, and shown by its status.
 *  The button no longer blocks, and leaving the section does not lose the work.
 *
 *  Two parts: `AskHistory` lives in the left rail (the list of questions, replacing the session
 *  list while this section is open); `AskPane` is the main area — the new-question form, or the
 *  selected request with its input source and answer. */

const PROVIDERS = [
  { id: "claude", label: "Claude (подписка)" },
  { id: "ollama", label: "Локальная (Ollama)" },
];

const STATUS: Record<AskStatus, { cls: string; label: string }> = {
  pending: { cls: "busy", label: "в очереди" },
  running: { cls: "busy", label: "спрашиваю…" },
  done: { cls: "done", label: "готово" },
  failed: { cls: "err", label: "ошибка" },
};

const busyStatus = (s: AskStatus) => s === "pending" || s === "running";

/** The question history — the left rail of the «Спросить» section. Polls while anything is still
 *  in flight, so «спрашиваю…» turns into «готово» on its own. */
export function AskHistory({
  current,
  onOpen,
  gen,
  say,
}: {
  current: string | null;
  onOpen: (id: string | null) => void;
  gen: number;
  say: (m: string) => void;
}) {
  const [list, setList] = useState<AskSummary[]>([]);

  const load = async () => {
    try {
      setList((await api.asks()).asks);
    } catch {
      /* the history is a view; its absence breaks nothing */
    }
  };
  useEffect(() => {
    void load();
  }, [gen]);
  useEffect(() => {
    if (!list.some((a) => busyStatus(a.status))) return;
    const t = setInterval(() => void load(), 2000);
    return () => clearInterval(t);
  }, [list]);

  return (
    <div className="asklist">
      <button
        className={"askrow new" + (current === null ? " on" : "")}
        onClick={() => onOpen(null)}
      >
        ✎ Новый вопрос
      </button>
      {list.map((a) => (
        <AskRow
          key={a.id}
          a={a}
          current={current === a.id}
          onOpen={onOpen}
          say={say}
          onDeleted={(id) => {
            if (current === id) onOpen(null); // the open one was removed → back to the new-question form
            void load();
          }}
        />
      ))}
      {!list.length && <p className="hint">Пока пусто. Задайте первый вопрос.</p>}
    </div>
  );
}

/** One question in the history, with a two-step delete — the same interaction a recording's row
 *  has: the trash shows on hover; the first click does not delete, it turns the row into a named
 *  confirm; a second, deliberate click removes it. An ask is re-runnable, so this is less grave than
 *  deleting a recording, but the gesture is identical so the app has one way to delete, not two. */
function AskRow({
  a,
  current,
  onOpen,
  onDeleted,
  say,
}: {
  a: AskSummary;
  current: boolean;
  onOpen: (id: string | null) => void;
  onDeleted: (id: string) => void;
  say: (m: string) => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const st = STATUS[a.status];
  const label = a.input_name || `${a.input_chars} симв.`;

  const del = async () => {
    setBusy(true);
    try {
      await api.deleteAsk(a.id);
      say("Вопрос удалён");
      onDeleted(a.id);
    } catch (e) {
      say((e as Error).message);
      setBusy(false);
      setConfirming(false);
    }
  };

  if (confirming) {
    return (
      <div className="askrow confirming">
        <span className="what">Удалить «{label}»?</span>
        <span className="confirm-actions">
          <button className="btn sm danger-solid" onClick={() => void del()} disabled={busy}>
            {busy ? "удаляю…" : "Удалить"}
          </button>
          <button className="btn sm" onClick={() => setConfirming(false)} disabled={busy}>
            Отмена
          </button>
        </span>
      </div>
    );
  }

  return (
    <div
      className={"askrow" + (current ? " on" : "")}
      role="button"
      tabIndex={0}
      aria-selected={current}
      onClick={() => onOpen(a.id)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") onOpen(a.id);
      }}
    >
      <span className="what">{label}</span>
      <span className={`st ${st.cls}`}>{st.label}</span>
      <button
        className="trash"
        title="Удалить вопрос"
        onClick={(e) => {
          e.stopPropagation(); // the trash deletes, it does not open the question
          setConfirming(true);
        }}
      >
        🗑
      </button>
    </div>
  );
}

/** The main area: the new-question form (`current === null`) or the selected request. */
export function AskPane({
  current,
  onOpen,
  onChanged,
  say,
}: {
  current: string | null;
  onOpen: (id: string | null) => void;
  onChanged: () => void;
  say: (m: string) => void;
}) {
  if (current === null) {
    return (
      <main className="main ask">
        <NewAsk
          onCreated={(id) => {
            onOpen(id);
            onChanged();
          }}
          say={say}
        />
      </main>
    );
  }
  return (
    <main className="main ask">
      <AskDetail id={current} onChanged={onChanged} say={say} />
    </main>
  );
}

function NewAsk({ onCreated, say }: { onCreated: (id: string) => void; say: (m: string) => void }) {
  const [text, setText] = useState("");
  const [prompt, setPrompt] = useState("");
  const [provider, setProvider] = useState("claude");
  const [name, setName] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [drag, setDrag] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  const readFile = async (f: File) => {
    const c = await f.text();
    setText(c);
    setName(f.name);
    say(`Файл «${f.name}» прочитан (${c.length} симв.)`);
  };
  const onDrop = async (e: DragEvent) => {
    e.preventDefault();
    setDrag(false);
    const f = e.dataTransfer.files?.[0];
    if (f) await readFile(f);
  };

  const submit = async () => {
    if (!text.trim()) {
      setErr("Нечего отправлять: вставьте текст или перетащите файл.");
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      // Instant: this only CREATES the request. The answer arrives in the background — we select
      // the new request and its detail pane polls for the result.
      const rec = await api.createAsk({
        text,
        prompt: prompt.trim() || undefined,
        provider,
        input_name: name || undefined,
      });
      say("Отправлено — ответ придёт в фоне");
      onCreated(rec.id);
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  // Enter sends, Shift+Enter breaks a line — the chat convention. Only inside this form's fields,
  // so it never fires elsewhere in the app.
  const onEnter = (e: KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submit();
    }
  };

  return (
    <div className="pane ask-form">
      <h2>Новый вопрос</h2>
      <p className="hint">
        Перетащите файл, вставьте или введите текст, добавьте задачу — ответ придёт в фоне и
        сохранится в истории слева. Уходить из раздела можно.
      </p>
      <div
        className={"dropzone" + (drag ? " over" : "")}
        onDragOver={(e) => {
          e.preventDefault();
          setDrag(true);
        }}
        onDragLeave={() => setDrag(false)}
        onDrop={(e) => void onDrop(e)}
      >
        <textarea
          value={text}
          placeholder="Вставьте текст или перетащите сюда файл… (Enter — спросить, Shift+Enter — перенос строки)"
          onChange={(e) => {
            setText(e.target.value);
            if (name) setName(null);
          }}
          onKeyDown={onEnter}
          rows={10}
        />
        {name && <span className="badge">📄 {name}</span>}
        {/* The reliable path in. Drag-and-drop from Explorer can be silently blocked by Windows
            itself (a drop across processes of different integrity levels — e.g. one of them running
            as admin — is refused by UIPI, and no app config fixes that), so a plain file picker is
            always here as the way that cannot fail. */}
        <div className="pickrow">
          <button type="button" className="pick" onClick={() => fileRef.current?.click()}>
            📎 Выбрать файл
          </button>
          <span className="hint">или перетащите сюда из проводника</span>
        </div>
        <input
          ref={fileRef}
          type="file"
          hidden
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) void readFile(f);
            e.currentTarget.value = ""; // let the same file be chosen again
          }}
        />
      </div>
      <input
        className="ask-prompt"
        value={prompt}
        placeholder="Задача (необязательно): по умолчанию — выжимка главного / ответ по существу"
        onChange={(e) => setPrompt(e.target.value)}
        onKeyDown={onEnter}
      />
      <div className="ask-actions">
        <select value={provider} onChange={(e) => setProvider(e.target.value)}>
          {PROVIDERS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </select>
        <button className="btn primary" disabled={busy} onClick={() => void submit()}>
          {busy ? "Отправляю…" : "Спросить"}
        </button>
      </div>
      {err && <p className="err">{err}</p>}
    </div>
  );
}

function AskDetail({
  id,
  onChanged,
  say,
}: {
  id: string;
  onChanged: () => void;
  say: (m: string) => void;
}) {
  const [rec, setRec] = useState<AskRecord | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const wasBusy = useRef(false);

  const load = async () => {
    try {
      setRec(await api.getAsk(id));
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  useEffect(() => {
    setRec(null);
    setErr(null);
    wasBusy.current = false;
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  // Poll while the answer is not in. When it lands, refresh the history once so its row updates.
  useEffect(() => {
    if (!rec) return;
    if (busyStatus(rec.status)) {
      wasBusy.current = true;
      const t = setInterval(() => void load(), 2000);
      return () => clearInterval(t);
    }
    if (wasBusy.current) {
      wasBusy.current = false;
      onChanged();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rec?.status, id]);

  if (err) return <p className="err">{err}</p>;
  if (!rec) return <p className="hint">загрузка…</p>;
  const st = STATUS[rec.status];

  return (
    <div className="pane ask-detail">
      <div className="ask-src">
        <div className="ask-meta">
          <span className={`st ${st.cls}`}>{st.label}</span>
          <span>
            {rec.provider}
            {rec.model ? ` · ${rec.model}` : ""}
          </span>
          {typeof rec.cost_usd === "number" && <span>~${rec.cost_usd.toFixed(3)}</span>}
        </div>
        <div className="ask-input" data-nocopy>
          <span className="lbl">
            {rec.input_kind === "url" ? "Ссылка" : rec.input_name ? "Файл" : "Текст"}
            {rec.input_name ? `: ${rec.input_name}` : ""} · {rec.input_chars} симв.
          </span>
          {rec.input && (
            <button
              className="btn sm"
              title="Скопировать источник (ссылку / путь / текст)"
              onClick={() => void copyText(rec.input ?? "", say)}
            >
              ⧉ Копировать
            </button>
          )}
          {rec.input && <div className="ask-input-body">{rec.input}</div>}
        </div>
      </div>

      {busyStatus(rec.status) ? (
        <p className="hint">Ответ готовится… можно уйти в другой раздел — не потеряется.</p>
      ) : rec.error ? (
        <p className="err">Ошибка: {rec.error}</p>
      ) : (
        <div className="ask-answer-wrap">
          <div className="ask-answer-bar" data-nocopy>
            <button
              className="btn sm"
              title="Скопировать ответ"
              onClick={() => void copyText(rec.answer ?? "", say)}
            >
              ⧉ Копировать ответ
            </button>
          </div>
          <Markdown text={rec.answer ?? ""} className="answer" />
        </div>
      )}
    </div>
  );
}
