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
  const submitting = useRef(new Set<string>());
  const [pending, setPending] = useState(new Set<string>());

  const retry = async (s: string) => {
    if (submitting.current.has(s)) return;
    submitting.current.add(s); setPending(new Set(submitting.current));
    try {
      const r = await api.retry(s);
      say(r.msg ?? "Этап поставлен в очередь");
      refresh();
      onChanged();
    } catch (e) {
      say((e as Error).message);
    } finally {
      submitting.current.delete(s); setPending(new Set(submitting.current));
    }
  };

  if (!items) return <main className="main"><div className="pane"><p className={err ? "err" : "hint"}>{err ?? "загрузка…"}</p></div></main>;

  return (
    <main className="main">
      <div className="pane">
        <h1 style={{ fontSize: 19, margin: "0 0 4px" }}>Очередь обработки</h1>
        {err && <p className="err">Очередь не обновляется: {err}. Показаны последние полученные данные.</p>}
        <p className="hint" style={{ marginTop: 0 }}>
          Порядок настоящий — демон берёт записи сверху вниз. Готовых здесь нет: они уже в архиве.
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
                  {i.state === "running" ? "▶" : i.state === "failed" ? "!" : i.position}
                </span>
                <button className="qname" onClick={() => open(i.session)} title="Открыть запись">
                  <b>{i.title}</b>
                  <span className="m">
                    {i.state === "running" && <span className="st busy">обрабатывается</span>}
                    {i.state === "pending" && <span className="st mute">ждёт очереди</span>}
                    {i.state === "failed" && (
                      <span className="st err">{i.stuck ? "остановлено" : "сорвалось, повторю"}</span>
                    )}
                    {i.kind === "ingest" && <span>· по ссылке</span>}
                  </span>
                  {i.last_error && <span className="qerr">{i.last_error}</span>}
                </button>
                {/* Out of retries: it will not move again by itself, and saying so beats a queue
                    that silently never advances. The way out is the human's, so it is right here. */}
                {i.stuck && (
                  <button className="btn sm" disabled={pending.has(i.session)} onClick={() => void retry(i.session)}>
                    {pending.has(i.session) ? "Постановка…" : "♻ Повторить этап"}
                  </button>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </main>
  );
}
