import type { ReactNode } from "react";
import type { Session } from "./api";

export const mmss = (s: number) => {
  const t = Math.max(0, Math.floor(s || 0));
  return `${String(Math.floor(t / 60)).padStart(2, "0")}:${String(t % 60).padStart(2, "0")}`;
};

/** "13 июля, 00:34–00:52 · 18 мин" instead of "20260713_003440". The duration is the
 *  WHOLE recording, pauses and silence included — that is how long the recorder ran. */
export function sessionTitle(s: Session): string {
  const named = s.title ? `${s.title} · ` : s.meeting ? "Встреча · " : "";
  const started = s.started_at ? new Date(s.started_at) : null;
  if (!started || isNaN(started.getTime())) return named + s.name;
  const hhmm = (d: Date) =>
    `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  const end = s.duration_sec > 0 ? new Date(started.getTime() + s.duration_sec * 1000) : null;
  const span = end ? `${hhmm(started)}–${hhmm(end)}` : hhmm(started);
  return `${named}${span}`;
}

export function shortTitle(s: Session): string {
  if (s.title) return s.title;
  if (s.meeting) return "Встреча";
  return sessionTitle(s);
}

/** Sessions are grouped by day: an archive is read by "when", not by file name. */
export function dayLabel(s: Session): string {
  if (!s.started_at) return "Когда-то";
  const d = new Date(s.started_at);
  if (isNaN(d.getTime())) return "Когда-то";
  const midnight = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((midnight(new Date()) - midnight(d)) / 86_400_000);
  if (days === 0) return "Сегодня";
  if (days === 1) return "Вчера";
  return d.toLocaleDateString("ru-RU", { day: "numeric", month: "long" });
}

export function stateOf(s: Session): { cls: string; text: string } {
  if (s.recording) return { cls: "rec", text: "идёт запись" };
  if (s.job === "running") return { cls: "busy", text: "обрабатывается" };
  if (s.job === "pending") return { cls: "busy", text: "в очереди" };
  if (s.job === "failed") return { cls: "err", text: "обработка сорвалась" };
  if (!s.cooked) return { cls: "busy", text: "не обработана" };
  if (s.empty) return { cls: "done", text: "речь не распознана" };
  return { cls: "done", text: "расшифрована" };
}

/** Markdown from the model, rendered into REACT ELEMENTS — never into innerHTML.
 *  The text is a model's output over someone's speech: it has no business becoming
 *  markup, and this way it structurally cannot. Supports what the summary prompt
 *  actually produces: headings, lists, bold, inline code. */
export function Markdown({ text }: { text: string }) {
  const blocks: ReactNode[] = [];
  let list: ReactNode[] = [];

  const flush = () => {
    if (list.length) {
      blocks.push(<ul key={`u${blocks.length}`}>{list}</ul>);
      list = [];
    }
  };

  for (const [i, raw] of text.split("\n").entries()) {
    const line = raw.trimEnd();
    const bullet = /^\s*[*\-•]\s+(.*)$/.exec(line) ?? /^\s*\d+[.)]\s+(.*)$/.exec(line);
    if (bullet) {
      list.push(<li key={i}>{inline(bullet[1])}</li>);
      continue;
    }
    flush();
    const head = /^(#{1,6})\s+(.*)$/.exec(line);
    if (head) blocks.push(<h2 key={i}>{inline(head[2])}</h2>);
    else if (line.trim()) blocks.push(<p key={i}>{inline(line)}</p>);
  }
  flush();
  return <>{blocks}</>;
}

/** **bold** and `code` — the only inline markup the summaries use. */
function inline(s: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /\*\*(.+?)\*\*|`(.+?)`/g;
  let last = 0;
  for (let m = re.exec(s); m; m = re.exec(s)) {
    if (m.index > last) out.push(s.slice(last, m.index));
    if (m[1]) out.push(<b key={m.index}>{m[1]}</b>);
    else out.push(<code key={m.index}>{m[2]}</code>);
    last = m.index + m[0].length;
  }
  if (last < s.length) out.push(s.slice(last));
  return out;
}
