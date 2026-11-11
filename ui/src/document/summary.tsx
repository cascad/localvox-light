import { api, type SummaryBlock, type Session } from "../api";
import { mmss } from "../lib";
import { useArtifact } from "../useArtifact";
import { Provenance } from "./provenance";

/** THE SUMMARY, with every claim tied to the seconds it came from.
 *
 *  A summary is the one artifact a person cannot check by reading it: the line-by-line view sits
 *  next to the recording, but this is prose about half an hour of speech. So each claim carries
 *  the lines the model drew it from — verified at cook time, because an unverified citation is
 *  just another confident answer — and the button beside it plays exactly that moment.
 *
 *  A claim with no button is one whose references did not check out. It keeps its words: the
 *  check is literal and gets things wrong, and a summary emptied by its own verifier is a failure
 *  this app has already paid for twice. */
export function Article({
  session,
  onSeek,
  onChanged,
  say,
  refresh,
}: {
  session: Session;
  onSeek: (sec: number, until?: number) => void;
  onChanged: () => void;
  say: (m: string) => void;
  refresh: number;
}) {
  const { data, err } = useArtifact(
    `${session.name}/summary`,
    signal => api.summary(session.name, signal),
    refresh,
  );
  const kind = "summary" as const;
  const doubts = session.summary_doubts;

  const confirm = async () => {
    try {
      const r = await api.confirm(session.name, kind);
      say(r.msg);
      onChanged();
    } catch (e) {
      say((e as Error).message);
    }
  };

  const recook = async () => {
    try {
      const r = await api.recook(session.name, "summary");
      say(r.msg ?? "Поставлено на переварку");
      onChanged();
    } catch (e) {
      say((e as Error).message);
    }
  };

  if (err && !data) return <p className="err">{err}</p>;
  if (!data) return <p className="hint">загрузка…</p>;
  if (data.available === false) return <p className="hint">Сводка ещё не создана. Текст и реплики доступны отдельно; ход обработки — во вкладке «Ход».</p>;

  return (
    <>
      {/* The doubt mark is a NOTE, not a verdict. The check compares literally and does
          get it wrong — on a live recording it called "Сергей" an invention, and he had
          been said out loud. The person decides: they can listen to the recording, the
          machine cannot. Both exits stand right here. */}
      {doubts && (
        <div className="doubt" data-nocopy>
          <span aria-hidden="true">⚠</span>
          <div className="txt">
            <b>Стоит перепроверить.</b> В записи этого нет (или сказано другими словами):{" "}
            <code>{doubts}</code>. Может быть враньё модели, а может — неточность. Решаете вы:
            запись можно послушать.
          </div>
          <div className="acts">
            <button className="btn" onClick={confirm}>
              ✓ Всё верно
            </button>
            <button className="btn" onClick={recook}>
              ♻ Переварить
            </button>
          </div>
        </div>
      )}
      <Provenance p={data.provenance} />
      <div className="summary">
        {(data.blocks ?? []).map((b: SummaryBlock, i: number) => {
          if (b.kind === "heading") return <h2 key={i}>{b.text}</h2>;
          const src = b.sources?.[0];
          const cite = src && (
            <button
              className="cite"
              data-nocopy
              title={`Послушать, откуда это: ${mmss(src.start_sec)}`}
              onClick={() => onSeek(src.start_sec, b.sources[b.sources.length - 1].end_sec)}
            >
              ▶ {mmss(src.start_sec)}
            </button>
          );
          return b.kind === "bullet" ? (
            <div className="claim" key={i}>
              <span className="tx">{b.text}</span>
              {cite}
            </div>
          ) : (
            <p key={i}>
              {b.text} {cite}
            </p>
          );
        })}
      </div>
    </>
  );
}
