import { useEffect, useMemo, useState } from "react";
import { api, type Setting, type SettingsDoc } from "./api";

/** The settings screen — an editor for ONE file, `.env`.
 *
 *  There is no second config file, and that is the design: defaults live in the code, `.env`
 *  overrides them, and this screen edits `.env`. The write is surgical, so the 280 lines of
 *  comments in that file — measurements, why a threshold is what it is — survive every save.
 *
 *  THE SCREEN NEVER PRETENDS A CHANGE IS LIVE. These values are read once, when the daemon starts;
 *  saving writes the file and nothing more. Saying so plainly is the difference between "it did not
 *  work" and "not yet" — and the app has exactly one rule about lying to the person using it. */
export function SettingsPane({ say }: { say: (m: string) => void }) {
  const [doc, setDoc] = useState<SettingsDoc | null>(null);
  const [err, setErr] = useState<string | null>(null);
  // Only what the person actually touched is sent. Sending the whole form back would rewrite keys
  // nobody edited — and would turn a value that is currently a code default into a pinned one.
  const [edits, setEdits] = useState<Record<string, string | null>>({});
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);

  const load = async () => {
    try {
      setDoc(await api.settings());
      setErr(null);
    } catch (e) {
      setErr((e as Error).message);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const groups = useMemo(() => {
    const out: [string, Setting[]][] = [];
    for (const s of doc?.settings ?? []) {
      const last = out[out.length - 1];
      if (last && last[0] === s.group) last[1].push(s);
      else out.push([s.group, [s]]);
    }
    return out;
  }, [doc]);

  const dirty = Object.keys(edits).length > 0;

  const save = async () => {
    if (!dirty) return;
    setBusy(true);
    try {
      const r = await api.saveSettings(edits);
      setEdits({});
      setSaved(true);
      say(r.msg);
      await load();
    } catch (e) {
      say((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  if (err) return <main className="main"><div className="pane"><p className="err">{err}</p></div></main>;
  if (!doc) return <main className="main"><div className="pane"><p className="hint">загрузка…</p></div></main>;

  return (
    <main className="main">
      <div className="pane">
        <h1 style={{ fontSize: 19, margin: "0 0 4px" }}>Настройки</h1>
        <p className="hint" style={{ margin: "0 0 14px" }}>
          Всё живёт в одном файле: <code>{doc.path}</code>
          {!doc.exists && " — его пока нет, он появится при первом сохранении"}. Пустое поле — значит
          работает то, что зашито в коде.
        </p>

        {saved && (
          <p className="note" data-nocopy>
            <span className="i" aria-hidden="true">ⓘ</span>
            <span>
              <b>Сохранено, но ещё не действует.</b> Эти значения читаются один раз — при запуске
              демона. Перезапустите его из трея, чтобы они вступили в силу.
            </span>
          </p>
        )}

        {groups.map(([group, items]) => (
          <section key={group} className="set-group">
            <h2>{group}</h2>
            {items.map((s) => (
              <Field
                key={s.key}
                s={s}
                edit={edits[s.key]}
                onChange={(v) =>
                  setEdits((e) => {
                    const next = { ...e };
                    // Back to what the file already says — then it is not an edit at all.
                    const current = s.value ?? "";
                    if (v === current) delete next[s.key];
                    else next[s.key] = v === "" ? null : v;
                    return next;
                  })
                }
              />
            ))}
          </section>
        ))}

        <Extra doc={doc} edits={edits} setEdits={setEdits} />

        <div className="row" style={{ marginTop: 18 }}>
          <button className="btn primary" onClick={() => void save()} disabled={!dirty || busy}>
            {busy ? "сохраняю…" : dirty ? `Сохранить (${Object.keys(edits).length})` : "Сохранено"}
          </button>
          {dirty && (
            <button className="btn" onClick={() => setEdits({})} disabled={busy}>
              Отменить правки
            </button>
          )}
        </div>
      </div>
    </main>
  );
}

function Field({
  s,
  edit,
  onChange,
}: {
  s: Setting;
  edit: string | null | undefined;
  onChange: (v: string) => void;
}) {
  // `edit === null` means "cleared" — an unset, which is a real edit and must not read back as the
  // stored value.
  const shown = edit === undefined ? (s.value ?? "") : (edit ?? "");
  const changed = edit !== undefined;

  // A device is PICKED. Typing its name by hand is how a recording silently opens the wrong
  // microphone — or none at all — and the mistake is invisible until you play the file back.
  if (s.options && s.options.length > 0) {
    // The stored value may name a device that is not plugged in right now. It stays selectable:
    // dropping it would silently re-point the setting at something else the moment a headset is
    // unplugged.
    const known = s.options.some((o) => o.value === shown);
    return (
      <div className={`set-row${changed ? " changed" : ""}`}>
        <label>
          <b>{s.label}</b>
          <span className="k">{s.key}</span>
        </label>
        <div className="set-ctl">
          <select value={shown} onChange={(e) => onChange(e.target.value)}>
            <option value="">по умолчанию ({s.hint})</option>
            {s.options.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
            {shown && !known && (
              <option value={shown}>{shown} — сейчас не подключено</option>
            )}
          </select>
        </div>
      </div>
    );
  }

  if (s.kind === "bool") {
    // on/off as the words, because that is what the readers in the daemon accept.
    const on = shown === "" ? null : !["off", "0", "false", "no"].includes(shown.toLowerCase());
    return (
      <div className={`set-row${changed ? " changed" : ""}`}>
        <label>
          <b>{s.label}</b>
          <span className="k">{s.key}</span>
        </label>
        <div className="set-ctl">
          <select value={on === null ? "" : on ? "on" : "off"} onChange={(e) => onChange(e.target.value)}>
            <option value="">по умолчанию ({s.hint})</option>
            <option value="on">включено</option>
            <option value="off">выключено</option>
          </select>
        </div>
      </div>
    );
  }

  return (
    <div className={`set-row${changed ? " changed" : ""}`}>
      <label>
        <b>{s.label}</b>
        <span className="k">{s.key}</span>
      </label>
      <div className="set-ctl">
        <input
          type={s.kind === "secret" ? "password" : "text"}
          inputMode={s.kind === "number" ? "numeric" : undefined}
          value={shown}
          // A secret is never sent back to the screen, so an empty box does not mean "unset".
          // Saying which it is beats a blank that could mean either.
          placeholder={s.kind === "secret" && s.set ? "задан — введите новый, чтобы заменить" : s.hint}
          onChange={(e) => onChange(e.target.value)}
        />
      </div>
    </div>
  );
}

/** Everything the file holds beyond the catalogue.
 *
 *  Every setting is just a `.env` key, so this covers the rest of them without a hand-written field
 *  for each — and, more importantly, it does not hide what is already in effect. A settings screen
 *  that showed only the keys it knows about would misrepresent the file it claims to edit. */
function Extra({
  doc,
  edits,
  setEdits,
}: {
  doc: SettingsDoc;
  edits: Record<string, string | null>;
  setEdits: React.Dispatch<React.SetStateAction<Record<string, string | null>>>;
}) {
  const [open, setOpen] = useState(false);
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");

  const add = () => {
    const k = key.trim().toUpperCase();
    if (!k) return;
    setEdits((e) => ({ ...e, [k]: value.trim() === "" ? null : value.trim() }));
    setKey("");
    setValue("");
  };

  return (
    <section className="set-group">
      <h2>
        <button className="btn sm" onClick={() => setOpen((o) => !o)}>
          {open ? "▾" : "▸"} Остальное в файле ({doc.extra.length})
        </button>
      </h2>
      {open && (
        <>
          {doc.extra.map((x) => (
            <div className={`set-row${edits[x.key] !== undefined ? " changed" : ""}`} key={x.key}>
              <label>
                <span className="k">{x.key}</span>
              </label>
              <div className="set-ctl">
                <input
                  value={edits[x.key] === undefined ? x.value : (edits[x.key] ?? "")}
                  onChange={(e) =>
                    setEdits((s) => {
                      const next = { ...s };
                      if (e.target.value === x.value) delete next[x.key];
                      else next[x.key] = e.target.value === "" ? null : e.target.value;
                      return next;
                    })
                  }
                />
              </div>
            </div>
          ))}
          <div className="row" style={{ marginTop: 8 }}>
            <input
              placeholder="LOCALVOX_…"
              value={key}
              onChange={(e) => setKey(e.target.value)}
              style={{ flex: 1, minWidth: 0 }}
            />
            <input
              placeholder="значение"
              value={value}
              onChange={(e) => setValue(e.target.value)}
              style={{ flex: 1, minWidth: 0 }}
            />
            <button className="btn" onClick={add}>
              Добавить
            </button>
          </div>
          <p className="hint">
            Писать можно только ключи <code>LOCALVOX_*</code> и <code>RUST_LOG</code>: через этот
            список нельзя подменить окружение демона, например <code>PATH</code>.
          </p>
        </>
      )}
    </section>
  );
}
