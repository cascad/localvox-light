import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Session } from "../api";
import { sessionTitle } from "../lib";
import type { Tab } from "./types";

export function useDocumentController({ session, tab, onChanged, say }: { session: Session; tab: Tab; onChanged: () => void; say: (message: string) => void }) {

  const doc = useRef<HTMLDivElement>(null);

  const controlPending = useRef(false);
  const [controlBusy, setControlBusy] = useState(false);
  const [tick, setTick] = useState(0);
  // Poll the open document even after completion or a transient read failure.
  // useArtifact keeps the previous content and scroll while the read is pending.
  useEffect(() => {
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") setTick((n) => n + 1);
    }, 3000);
    return () => clearInterval(timer);
  }, []);

  // The document goes to the clipboard WITHOUT the system footnotes and without the
  // doubt banner: our own explanation has no business opening someone else's protocol.
  const copy = useCallback(async () => {
    const box = doc.current?.cloneNode(true) as HTMLElement | undefined;
    if (!box) return;
    box.querySelectorAll("[data-nocopy]").forEach((n) => n.remove());
    const plain = box.querySelector("[data-plain-text]");
    const text = (plain?.textContent ?? box.innerText ?? "").replace(/\n{3,}/g, "\n\n").trim();
    if (!text) { say("Нет текста для копирования"); return; }
    try {
      await navigator.clipboard.writeText(text);
      say("Скопировано");
    } catch {
      say("Буфер недоступен");
    }
  }, [say]);

  useEffect(() => {
    if (tab !== "processed") return;
    const keydown = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || !event.shiftKey || event.altKey || event.repeat || event.isComposing || event.code !== "KeyC") return;
      if (event.target instanceof Element && event.target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])')) return;
      event.preventDefault();
      void copy();
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, [tab, copy]);

  const scopeWord = "запись с сохранённого аудио";
  const recook = useCallback(async () => {
    if (controlPending.current || session.recording || session.job === "cancelling") return;
    if (!confirm("Переварить запись с сохранённого аудио?\n\nРаспознавание и включённая сводка будут созданы заново. Аудио сохранится.")) return;
    controlPending.current = true; setControlBusy(true);
    try {
      const reply = await api.recook(session.name, "all");
      say(reply.msg ?? "Запись поставлена на переварку");
    } catch (e) { say((e as Error).message); }
    finally { controlPending.current = false; setControlBusy(false); onChanged(); setTick((n) => n + 1); }
  }, [session.name, session.recording, session.job, onChanged, say]);
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey || event.repeat || event.isComposing || event.code !== "KeyR") return;
      if (event.target instanceof Element && event.target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])')) return;
      event.preventDefault(); void recook();
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, [recook]);
  const control = async (action: "cancel") => {
    if (controlPending.current) return;
    controlPending.current = true; setControlBusy(true);
    try {
      const reply = await api[action](session.name);
      say(`${sessionTitle(session)}: ${reply.msg ?? "обработка отменена"}`);
    } catch (e) { say((e as Error).message); }
    finally { controlPending.current = false; setControlBusy(false); onChanged(); setTick((n) => n + 1); }
  };

    return { doc, controlBusy, tick, copy, recook, scopeWord, control };
}
