import { useRef, useState } from "react";
import { api } from "./api";
import { usePolling } from "./usePolling";

/** THE QUEUE: what is being cooked now, and what comes after it, in order.
 *
 *  It exists because counts do not answer the question. «варится: 1, в очереди: 7» says the
 *  machine is alive, not what it is chewing or what is behind it — and clicking that chip used to
 *  jump into one session's stage chain, which showed its LAST FINISHED run: all green, all done,
 *  while the cook was still going. Green for work in progress is worse than no answer.
 *
 *  So this is a list with an ORDER, and the order is the daemon's own: it takes the jobs exactly
 *  as they lie, oldest first. Nothing here re-sorts them for looks — that would be a lie about
 *  what happens next. */
export function QueuePane({
  open,
  say,
  onChanged,
}: {
  open: (n: string) => void;
  say: (m: string) => void;
  onChanged: () => void;
}) {
  const { data: items, error: err, refresh } = usePolling("queue", async (signal) => (await api.queue(signal)).queue ?? []);
  const submitting = useRef(new Map<string, "retry" | "cancel">());
  const [pending, setPending] = useState(new Map<string, "retry" | "cancel">());

  const change = async (s: string, action: "retry" | "cancel") => {
    if (submitting.current.has(s)) return;
    submitting.current.set(s, action); setPending(new Map(submitting.current));
    try {
      const r = await api[action](s);
      say(r.msg ?? (action === "cancel" ? "Обработка отменена; результаты сохранены" : "Этап поставлен в очередь"));
    } catch (e) {
      say((e as Error).message);
    } finally {
      submitting.current.delete(s); setPending(new Map(submitting.current));
      refresh(); onChanged();
    }
  };

  if (!items) return <main className="main"><div className="pane"><p className={err ? "err" : "hint"}>{err ?? "загрузка…"}</p></div></main>;

  return (
    <main className="main">
      <div className="pane">
        <h1 style={{ fontSize: 19, margin: "0 0 4px" }}>Очередь обработки</h1>
        {err && <p className="err">Очередь не обновляется: {err}. Показаны последние полученные данные.</p>}
        <p className="hint" style={{ marginTop: 0 }}>
          Порядок настоящий — демон берёт записи сверху вниз. Отменённые остаются для ручного продолжения.
        </p>

        {items.length === 0 ? (
          <p className="hint">
            Пусто — всё обработано. Записи не перевариваются сами: чтобы переделать, откройте
            запись и нажмите «Переварить».
          </p>
        ) : (
          <div className="queue-list">
            {items.map((i) => (
              <div className={`qrow ${i.state}`} key={i.session}>
                <span className="qpos">
                  {i.state === "running" ? "▶" : i.state === "failed" ? "!" : i.state === "cancelling" ? "◌" : i.state === "cancelled" ? "■" : i.position}
                </span>
                <button className="qname" onClick={() => open(i.session)} title="Открыть запись">
                  <b>{i.title}</b>
                  <span className="m">
                    {i.state === "running" && <span className="st busy">обрабатывается</span>}
                    {i.state === "pending" && <span className="st mute">ждёт очереди</span>}
                    {i.state === "cancelling" && <span className="st busy">останавливается · ждёт освобождения</span>}
                    {i.state === "cancelled" && <span className="st mute">отменено</span>}
                    {i.state === "failed" && (
                      <span className="st err">{i.retry?.state === "retry_wait" ? "ожидается повтор" : i.stuck || i.retry?.state === "failed" ? "остановлено" : "повтор не подтверждён"}</span>
                    )}
                    {i.kind === "ingest" && <span>· по ссылке</span>}
                  </span>
                  <span className="m">этап <code>{i.phase ?? "неизвестен"}</code> · попыток: {i.attempts}</span>
                  {i.state === "cancelled" && <span className="m">Продолжение — только вручную</span>}
                  {i.retry && <span className="qerr" data-retry-state={i.retry.state}>
                    {i.retry.message}
                    {i.retry.retry_at ? <><br />Повтор не раньше: {new Date(i.retry.retry_at).toLocaleString("ru-RU")}.</> : i.retry.state !== "failed" && <><br />Срок повтора неизвестен.</>}
                  </span>}
                  {i.last_error && <span className="qerr">{i.last_error}</span>}
                </button>
                {/* Exhausted jobs and explicit cancellations resume only by a human action. */}
                <div className="qactions">
                {((i.state === "failed" && i.stuck) || i.state === "cancelled") && (
                  <button className="btn sm" disabled={pending.has(i.session)} onClick={() => void change(i.session, "retry")}>
                    {pending.get(i.session) === "retry" ? "Постановка…" : i.state === "cancelled" ? "Продолжить обработку" : "♻ Повторить этап"}
                  </button>
                )}
                {["running", "pending", "failed"].includes(i.state) && <button className="btn sm" disabled={pending.has(i.session)} onClick={() => void change(i.session, "cancel")}>
                  {pending.get(i.session) === "cancel" ? "Остановка…" : "Отменить обработку"}
                </button>}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </main>
  );
}
