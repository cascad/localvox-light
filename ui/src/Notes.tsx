import { Fragment, useEffect, useState } from "react";
import { api, type SlotNotes, type StoredNote, type VoiceStatus } from "./api";

/** The notebook: what is actually IN the slots.
 *
 *  Until this existed the app took dictation and never mentioned the note again — it went into a
 *  file on another disk, and the only way to check anything had been written was a file manager.
 *  A secretary that takes dictation and then refuses to show you the notebook is half a secretary.
 *
 *  Each note carries the file it lives in, so the list is CHECKABLE rather than merely reassuring:
 *  the person can open that file and see the same line. */
export function NotesPane({ voice, say }: { voice: VoiceStatus | null; say: (m: string) => void }) {
  const [slots, setSlots] = useState<SlotNotes[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [active, setActive] = useState<string | null>(null);

  const load = async () => {
    try {
      const r = await api.slotNotes(100);
      setSlots(r.slots);
      setActive((a) => a ?? r.slots.find((s) => s.default)?.name ?? r.slots[0]?.name ?? null);
      setErr(null);
    } catch (e) {
      setErr((e as Error).message);
    }
  };

  useEffect(() => {
    void load();
    // A note dictated right now must show up without a reload — that is the point of the screen.
    const t = setInterval(() => void load(), 5000);
    return () => clearInterval(t);
  }, []);

  const current = slots?.find((s) => s.name === active) ?? null;

  return (
    <main className="main">
      <div className="pane">
        <h1 style={{ fontSize: 19, margin: "0 0 4px" }}>Заметки</h1>
        <p className="hint" style={{ margin: "0 0 12px" }}>
          То, что записано голосом и с этого экрана. Слоты и их файлы настраиваются в{" "}
          <code>slots.toml</code>.
        </p>

        {/* The module's own state, spelled out. «Работает ли вообще» is the question that had no
            answer anywhere in the app — and an empty notebook is the same picture whether the
            module is dead or you simply have not dictated yet. */}
        {voice ? (
          <p className="note" data-nocopy>
            <span className="i" aria-hidden="true">🎙</span>
            <span>
              {voice.capturing ? (
                <>
                  <b>Пишу в «{voice.capturing.slot}»:</b> {voice.capturing.text || "слушаю…"}
                </>
              ) : (
                <>
                  <b>Голосовой модуль слушает.</b> <span className="mut">{voice.detail}</span>
                </>
              )}
              {voice.last && (
                <>
                  <br />
                  {voice.last.error ? (
                    <span className="err">
                      Последняя заметка НЕ записалась: {voice.last.error}
                    </span>
                  ) : (
                    <>
                      Последняя легла в «{voice.last.slot}»: «{voice.last.text}» →{" "}
                      <code>{voice.last.dest}</code>
                    </>
                  )}
                </>
              )}
            </span>
          </p>
        ) : (
          <p className="note" data-nocopy>
            <span className="i" aria-hidden="true">🎙</span>
            <span>
              <b>Голосовой модуль не запущен.</b> Он включается, когда рядом найден{" "}
              <code>slots.toml</code>, и слушает команды только с микрофона.
            </span>
          </p>
        )}

        {err && <p className="err">{err}</p>}
        {!slots && !err && <p className="hint">загрузка…</p>}

        {slots && slots.length === 0 && (
          <p className="hint">Слоты не настроены — заведите <code>slots.toml</code>.</p>
        )}

        {slots && slots.length > 0 && (
          <>
            <div className="tabs" style={{ marginTop: 4 }}>
              {slots.map((s) => (
                <button
                  key={s.name}
                  aria-selected={s.name === active}
                  onClick={() => setActive(s.name)}
                >
                  {s.name}
                  {s.readable && <span className="k"> {s.notes.length}</span>}
                </button>
              ))}
            </div>

            {current && (
              <SlotBody
                slot={current}
                say={say}
                onPickSlot={setActive}
                onChanged={() => void load()}
              />
            )}
          </>
        )}
      </div>
    </main>
  );
}

function SlotBody({
  slot,
  say,
  onPickSlot,
  onChanged,
}: {
  slot: SlotNotes;
  say: (m: string) => void;
  onPickSlot: (name: string) => void;
  onChanged: () => void;
}) {
  const [text, setText] = useState("");
  const [hints, setHints] = useState<{ slot: string; reason?: string }[]>([]);
  const [routing, setRouting] = useState(false);

  const add = async () => {
    if (!text.trim()) return;
    try {
      const j = await api.note(text, slot.name);
      say(j.dest ?? "Записано");
      setText("");
      setHints([]);
      onChanged(); // the note must appear in the list at once, not on the next poll
    } catch (e) {
      say((e as Error).message);
    }
  };

  /** «Куда занести?» — the LLM reads the note and suggests a slot.
   *
   *  IT USED TO LOOK BROKEN, and for two reasons, both of them silence. With an empty box it
   *  returned without a word — nothing to route, but the button just did nothing. And with text it
   *  took several seconds of a local LLM with no sign of life, so the click read as "не работает".
   *  A button that says nothing is indistinguishable from a button that does not work. */
  const route = async () => {
    if (routing) return;
    if (!text.trim()) {
      say("Напишите заметку — подскажу, куда её занести");
      return;
    }
    setRouting(true);
    try {
      const r = await api.route(text);
      const found = r.suggestions ?? [];
      setHints(found);
      if (found.length === 0) say("Модель не выбрала слот — решайте сами");
    } catch (e) {
      say((e as Error).message);
    } finally {
      setRouting(false);
    }
  };

  return (
    <>
      <div className="row" style={{ margin: "12px 0" }}>
        <input
          placeholder={`Записать в «${slot.name}»…`}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void add();
          }}
          style={{ flex: 1, minWidth: 0 }}
        />
        <button
          className="btn"
          onClick={() => void route()}
          disabled={routing}
          title="Куда занести? Спросить модель"
        >
          {routing ? "думаю…" : "🧭"}
        </button>
        <button className="btn primary" onClick={() => void add()}>
          Записать
        </button>
      </div>
      {hints.length > 0 && (
        <div className="row" style={{ marginBottom: 10, alignItems: "baseline" }}>
          <span className="hint">Занести в:</span>
          {hints.map((h) => (
            <button className="btn sm" key={h.slot} title={h.reason} onClick={() => onPickSlot(h.slot)}>
              {h.slot}
            </button>
          ))}
        </div>
      )}

      {/* «Нельзя прочитать» is not «здесь пусто». An MCP destination we only send to has no
          listing, and showing an empty page for it would claim the notes are gone. */}
      {slot.error && <p className="hint">Не показать содержимое: {slot.error}</p>}

      {!slot.error && slot.notes.length === 0 && (
        <p className="hint">Здесь пока ничего не записано.</p>
      )}

      {/* Grouped by day, the way the archive groups recordings — the date belongs BESIDE the note,
          not inside its sentence. A row is then just the note itself. */}
      <div className="notes-list">
        {groupByDay(slot.notes).map(([day, items]) => (
          <Fragment key={day}>
            {day && <div className="day">{day}</div>}
            {items.map((n, i) => (
              // Newest first — the note just dictated is the one being looked for.
              <NoteRow
                key={`${n.source}:${day}:${i}`}
                slot={slot.name}
                note={n}
                say={say}
                onChanged={onChanged}
              />
            ))}
          </Fragment>
        ))}
      </div>
    </>
  );
}

/** One note, with a two-step delete.
 *
 *  Deleting here removes a line from the person's OWN vault — a file they also edit by hand, that
 *  we do not own and cannot restore. So it is built to be impossible by accident and effortless on
 *  purpose, the same way the archive's delete is: the trash appears only on hover, and clicking it
 *  does not delete — it turns the row into a named confirm aimed at THIS line. */
function NoteRow({
  slot,
  note,
  say,
  onChanged,
}: {
  slot: string;
  note: StoredNote;
  say: (m: string) => void;
  onChanged: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);

  const del = async () => {
    setBusy(true);
    try {
      await api.deleteNote(slot, note.raw, note.source);
      say("Удалено");
      onChanged();
    } catch (e) {
      say((e as Error).message);
      setBusy(false);
      setConfirming(false);
    }
  };

  if (confirming) {
    return (
      <div className="note-row confirming">
        <span className="tx">
          Удалить эту заметку? <span className="mut">насовсем, прямо в файле</span>
        </span>
        <button className="btn sm danger-solid" onClick={() => void del()} disabled={busy}>
          {busy ? "удаляю…" : "Удалить"}
        </button>
        <button className="btn sm" onClick={() => setConfirming(false)} disabled={busy}>
          Отмена
        </button>
      </div>
    );
  }

  return (
    // Just the note. The file it lives in is still one hover away — it is what makes the list a
    // record rather than a reassurance — but it is not something to read past on every line.
    <div className="note-row" title={note.source}>
      <span className="tx">{note.text}</span>
      <button className="note-trash" title="Удалить заметку" onClick={() => setConfirming(true)}>
        🗑
      </button>
    </div>
  );
}

/** Notes bucketed by their day, newest day first — the same shape the archive gives recordings.
 *
 *  A note with no date (the slot's template does not add one) falls into a single unlabelled group
 *  rather than inventing a day for it: we do not know when it was written, and guessing would put a
 *  wrong date on the screen with the same confidence as a right one. */
function groupByDay(notes: StoredNote[]): [string, StoredNote[]][] {
  const out: [string, StoredNote[]][] = [];
  for (const n of notes) {
    const day = n.date ?? "";
    const last = out[out.length - 1];
    if (last && last[0] === day) last[1].push(n);
    else out.push([day, [n]]);
  }
  return out;
}
