import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Session } from "../api";
import { sessionTitle } from "../lib";
import type { Tab } from "./types";

export function useDocumentController({ session, tab, onChanged, say }: { session: Session; tab: Tab; onChanged: () => void; say: (message: string) => void }) {

  const doc = useRef<HTMLDivElement>(null);

  // Live-refresh while the recording is being processed: re-read the open tab every few seconds so
  // replicas / text / summary appear as each stage finishes — no clicking away and back. It runs
  // ONLY while there is work (recording, queued or cooking), fires one final reload the moment that
  // work ends, and — thanks to `useArtifact`'s in-place reload — never flickers to «загрузка…».
  const live =
    session.recording || session.job === "waiting_input" || session.job === "running" || session.job === "pending" || session.job === "cancelling";
  const controlPending = useRef(false);
  const [controlBusy, setControlBusy] = useState(false);
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
  const control = async (action: "cancel" | "retry") => {
    if (controlPending.current) return;
    controlPending.current = true; setControlBusy(true);
    try {
      const reply = await api[action](session.name);
      say(`${sessionTitle(session)}: ${reply.msg ?? (action === "cancel" ? "обработка отменена" : "обработка продолжится")}`);
    } catch (e) { say((e as Error).message); }
    finally { controlPending.current = false; setControlBusy(false); onChanged(); setTick((n) => n + 1); }
  };

    return { doc, controlBusy, tick, copy, recook, scopeWord, control };
}
