import { useState } from "react";
import { api, type Participant, type Session } from "../api";
import { useArtifact } from "../useArtifact";

const AVATAR = ["#2f6ea3", "#8a6fbc", "#2c7a52", "#9d6a12", "#b0554a"];


/** WHO IS IN THIS RECORDING — one list, built from the lines the document actually shows.
 *
 *  There used to be two lists side by side: the audio SOURCES and the diarization ROSTER. Neither
 *  said what it was, so the screen showed «Собеседники» and «Участник 1» as peers with no hint of
 *  the difference, and it offered renames for people who say nothing. Measured on the owner's
 *  archive: the video's roster listed a «Участник 1» with not one line in the document, and the
 *  meeting's listed a «Участник 3» the same way.
 *
 *  The daemon now groups the lines themselves, so a phantom has nothing to be grouped from, the
 *  minutes are the minutes in the text, and every entry says WHY it is called what it is called. */
export function Speakers({
  session,
  say,
  refresh,
}: {
  session: Session;
  say: (m: string) => void;
  refresh: number;
}) {
  const [gen, setGen] = useState(0);
  const { data, err } = useArtifact(
    `${session.name}/speakers/${gen}`,
    signal => api.speakers(session.name, signal),
    refresh,
  );

  /** One button, two mechanisms — genuinely different, which is why the row says which ones it
   *  is made of. Naming a VOICE re-labels its lines only in this session;
   *  naming a SOURCE changes what its unattributed lines are called.
   *
   *  A row can be made of both, and then both are renamed. Sequentially and reporting the first
   *  failure: half a rename leaves some lines under the old name, and the person has to see that
   *  rather than find it later in the text. */
  const rename = async (p: Participant) => {
    const hasVoice = p.voices.length > 0;
    const asked = prompt(
      hasVoice
        ? `Как зовут «${p.name}»? Имя сохранится только в этой записи.`
        : `Как подписывать этот звук? (пусто — вернуть по умолчанию)`,
      hasVoice ? "" : p.name,
    );
    if (asked === null) return;
    const name = asked.trim();
    if (hasVoice && !name) return say("Имя не может быть пустым");
    try {
      let msg = "";
      for (const v of p.voices) msg = (await api.nameSpeaker(session.name, v, name)).msg;
      for (const id of p.sources) msg = (await api.nameSource(session.name, id, name)).msg;
      say(msg);
      setGen((g) => g + 1);
    } catch (e) {
      say((e as Error).message);
      setGen((g) => g + 1);
    }
  };

  if (err && !data) return <p className="err">{err}</p>;
  if (!data) return <p className="hint">загрузка…</p>;
  const people: Participant[] = data.speakers ?? [];

  return (
    <>
      <p className="note" data-nocopy>
        <span className="i" aria-hidden="true">
          ⓘ
        </span>
        <span>
          Здесь только те, кому в этой записи приписаны реплики. <b>Отдельный голос</b> — его
          выделила диаризация, имя сохранится только в этой записи. <b>Микрофон</b> и{" "}
          <b>системный звук</b> — это не голос, а вход: так подписаны реплики, в которых голос
          выделить не удалось. Переименование меняет подписи сразу; пересобирается только сводка.
        </span>
      </p>

      {people.length === 0 ? (
        // Empty is an ANSWER, not a breakage: nothing has been attributed yet because nothing has
        // been cooked. We are not going to invent participants to fill the screen.
        <p className="hint">
          Пока некому: в записи нет ни одной расшифрованной реплики.
        </p>
      ) : (
        people.map((p, i) => (
          <div className="voice" key={p.name}>
            <span
              className="av"
              style={{
                background: p.voices.length ? AVATAR[i % AVATAR.length] : "var(--mute)",
              }}
            >
              {p.name.slice(0, 2)}
            </span>
            <span className="grow">
              <b>{p.name}</b>
              <div className="sub">
                {p.what}
                {!p.named && " · имени пока нет"}
                {" · "}
                {Math.round(p.speech_sec / 60)} мин, реплик: {p.lines}
              </div>
            </span>
            <span className="acts">
              <button className="btn sm" onClick={() => void rename(p)}>
                ✎ {p.named ? "Переименовать" : "Назвать"}
              </button>
            </span>
          </div>
        ))
      )}
    </>
  );
}
