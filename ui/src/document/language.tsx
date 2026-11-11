import { useEffect, useState } from "react";
import { api, type Session } from "../api";

export function SessionLanguage({ session, onChanged, say }: { session: Session; onChanged: () => void; say: (message: string) => void }) {
  const [langs, setLangs] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let live = true;
    api.langs().then((doc) => { if (live) setLangs(doc.langs); }).catch(() => {});
    return () => { live = false; };
  }, []);
  const current = session.lang || "auto";
  const change = async (lang: string) => {
    if (lang === current || !confirm(`Изменить язык на «${lang}» и заново обработать всю запись?\nНужны исходное аудио и модель этого языка.`)) return;
    setBusy(true);
    try { const result = await api.setLang(session.name, lang); say(result.msg ?? "Язык сохранён; начата полная переработка"); onChanged(); }
    catch (e) { say((e as Error).message); }
    finally { setBusy(false); }
  };
  return <div className="ver-switch" data-nocopy>
    <label className="lbl" htmlFor="session-language">Язык:</label>
    <select id="session-language" value={current} disabled={session.recording || busy} onChange={(event) => void change(event.target.value)}>
      {[...new Set(["auto", ...langs, current])].map((lang) => <option value={lang} key={lang}>{lang === "auto" ? "авто" : lang}</option>)}
    </select>
    <span className="hint">смена языка запускает полную переработку</span>
  </div>;
}
