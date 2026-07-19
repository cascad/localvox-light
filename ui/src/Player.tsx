import { useEffect, useRef, useState } from "react";
import { api, token, type Session } from "./api";
import { mmss } from "./lib";

/** A request to move the ONE player to a second on the timeline. `n` is a nonce so that clicking
 *  the same line twice replays it — the value alone would not change.
 *
 *  `until` — stop playback at this second. Clicking a transcript line asks to hear THAT line and
 *  nothing after it; dragging the wave leaves `until` empty and plays straight on. Either way the
 *  player looks the same — a segment click just stops when the segment ends. */
export interface SeekRequest {
  sec: number;
  until?: number;
  n: number;
}

/** How many bars the wave is drawn with. Across the full width they come out thin — the point: a
 *  minute of the recording deserves better than one fat stroke. */
const BARS = 320;

interface Props {
  session: Session | null;
  seek: SeekRequest | null;
  onPos: (pos: number) => void;
}

/** THE player — the only place audio is ever played, and it is ONE player in look and behaviour.
 *
 *  The whole recording is handed to a native <audio> as a single `audio.wav` with HTTP Range
 *  support, so the BROWSER does the seeking: instantly, exactly, fetching only the bytes around
 *  the target. There is no window management, no per-seek fetch, no manual buffering — the lag is
 *  gone because we stopped re-loading on every click. Seeking is just `currentTime = sec`.
 *
 *  There is no "fragment mode" either: clicking a line and dragging the wave do the same thing —
 *  move this player and play on. A line also carries an end, so it stops there and the piece is
 *  drawn red on the wave; that is the only difference, and it is one player throughout. */
export function Player({ session, seek, onPos }: Props) {
  const audio = useRef<HTMLAudioElement | null>(null);
  /** Stop playback at this second (a segment click). null — play straight on. */
  const stopAt = useRef<number | null>(null);
  const lastSeek = useRef(0);

  const [pos, setPos] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [note, setNote] = useState("");
  const [peaks, setPeaks] = useState<number[] | null>(null);
  const [seg, setSeg] = useState<{ start: number; end: number } | null>(null);

  const total = session?.duration_sec ?? 0;
  const name = session?.name ?? null;

  // The shape of the recording (the wave). One pass on the daemon side, cached — asked once per
  // session, never on every render.
  useEffect(() => {
    setPeaks(null);
    if (!name || total <= 0) return;
    let alive = true;
    api
      .peaks(name, BARS)
      .then((p) => alive && setPeaks(p.peaks ?? []))
      .catch(() => alive && setPeaks([]));
    return () => {
      alive = false;
    };
  }, [name, total]);

  // The audio source: one URL for the whole recording. Set when the session changes; the browser
  // fetches ranges of it on demand. The token (if any) rides in the query — an <audio> element
  // cannot send an Authorization header, and this stream is same-origin and local.
  useEffect(() => {
    const el = audio.current;
    if (!el) return;
    stopAt.current = null;
    setSeg(null);
    setPos(0);
    setPlaying(false);
    setNote("");
    if (name && total > 0) {
      const t = token.get();
      el.src = `/api/sessions/${encodeURIComponent(name)}/audio.wav${t ? `?token=${encodeURIComponent(t)}` : ""}`;
      el.load();
    } else {
      el.removeAttribute("src");
      el.load();
    }
  }, [name, total]);

  const goTo = (sec: number, autoplay: boolean, until?: number) => {
    const el = audio.current;
    if (!el || total <= 0) return;
    stopAt.current = until ?? null;
    setSeg(until != null ? { start: sec, end: until } : null);
    el.currentTime = Math.max(0, Math.min(sec, total - 0.05)); // native, instant, exact
    setPos(el.currentTime);
    onPos(el.currentTime);
    if (autoplay) void el.play().catch(() => {});
  };

  // A line was clicked (or a source in an answer): move the player there and play. A line carries
  // an `until`; a wave scrub does not.
  useEffect(() => {
    if (seek && seek.n !== lastSeek.current) {
      lastSeek.current = seek.n;
      goTo(seek.sec, true, seek.until);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seek]);

  const onTime = () => {
    const el = audio.current;
    if (!el) return;
    setPos(el.currentTime);
    onPos(el.currentTime);
    if (stopAt.current != null && el.currentTime >= stopAt.current) {
      stopAt.current = null;
      el.pause(); // the segment is over; the red stays until you seek or press ▶
    }
  };

  const toggle = () => {
    const el = audio.current;
    if (!el || total <= 0) return;
    if (playing) {
      el.pause();
      return;
    }
    // ▶ means "play the recording on from here" — it leaves the segment behind.
    stopAt.current = null;
    setSeg(null);
    void el.play().catch(() => {});
  };

  const nudge = (d: number) => {
    const el = audio.current;
    if (!el || total <= 0) return;
    stopAt.current = null;
    setSeg(null);
    el.currentTime = Math.max(0, Math.min(el.currentTime + d, total - 0.05));
  };

  const scrub = (e: React.MouseEvent<HTMLDivElement>) => {
    if (total <= 0) return;
    const r = e.currentTarget.getBoundingClientRect();
    goTo(((e.clientX - r.left) / r.width) * total, playing);
  };

  // No session, or a session with no audio (a text import, a retention cleanup): the strip is
  // absent entirely. A player that plays nothing is worse than no player.
  if (!session || total <= 0) return null;

  return (
    <div className="playbar">
      {/* `metadata`, not `auto`: with Range the browser fetches only the bytes around a seek
          target (milliseconds on localhost), so it feels instant WITHOUT downloading the whole
          ~200 MB of every session the moment it is opened. */}
      <audio
        ref={audio}
        preload="metadata"
        onTimeUpdate={onTime}
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onWaiting={() => setNote("буферизую…")}
        onPlaying={() => setNote("")}
        onCanPlay={() => setNote("")}
        onError={() => setNote("аудио недоступно")}
      />

      <button className="play-big" onClick={toggle} title={playing ? "Пауза" : "Слушать"}>
        {playing ? "❚❚" : "▶"}
      </button>

      <Wave peaks={peaks} total={total} pos={pos} seg={seg} note={note} onSeek={scrub} />

      <span className="ptime">
        {mmss(pos)} <span className="mut">/ {mmss(total)}</span>
      </span>

      <button className="icon-btn" onClick={() => nudge(-15)} title="Назад 15 с">
        ⏪
      </button>
      <button className="icon-btn" onClick={() => nudge(15)} title="Вперёд 15 с">
        ⏩
      </button>
    </div>
  );
}

/** The shape of the recording. The wave is REAL — it is what the loud stretches actually are, so
 *  aiming at "where the argument was" lands there. Until the peaks arrive the bars are flat rather
 *  than absent: a strip that changes height on load makes the whole panel jump. */
function Wave({
  peaks,
  total,
  pos,
  seg,
  note,
  onSeek,
}: {
  peaks: number[] | null;
  total: number;
  pos: number;
  /** The segment being played (a line click), drawn red so it is visible which piece sounds. */
  seg: { start: number; end: number } | null;
  /** A transient status, shown as an OVERLAY, never a flex sibling: as a sibling it took
   *  horizontal space and the wave "breathed" — shrank while it showed, sprang back after. */
  note: string;
  onSeek: (e: React.MouseEvent<HTMLDivElement>) => void;
}) {
  // Where the click would land. A seek bar that shows nothing until you have already clicked makes
  // you aim by guesswork and then undo — an hour of recording means a pixel is seconds.
  const [aim, setAim] = useState<{ pct: number; at: number } | null>(null);

  const bars = peaks?.length ? peaks : new Array<number>(BARS).fill(0.06);
  const played = (pos / total) * bars.length;

  const track = (e: React.MouseEvent<HTMLDivElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const pct = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    setAim({ pct, at: pct * total });
  };

  return (
    <div
      className="wave"
      onClick={onSeek}
      onMouseMove={track}
      onMouseLeave={() => setAim(null)}
      role="slider"
      aria-label="Перемотка"
      aria-valuemin={0}
      aria-valuemax={Math.round(total)}
      aria-valuenow={Math.round(pos)}
    >
      {bars.map((v, i) => {
        const from = (i / bars.length) * total;
        const to = ((i + 1) / bars.length) * total;
        const inSeg = seg != null && to > seg.start && from < seg.end;
        // Inside the played segment the sounded part goes amber, the rest red: you see both which
        // piece is playing and how far it has got, on the same wave, no separate marker.
        const cls = inSeg ? (i < played ? "seg-on" : "seg") : i < played ? "on" : "";
        return (
          <i key={i} className={cls} style={{ height: `${Math.max(2, Math.round(v * 34))}px` }} />
        );
      })}

      {/* The playhead: where the sound IS right now. It rides with playback — the current position
          seen without a clock, and the witness that a click landed where it was aimed. */}
      {total > 0 && <span className="playhead" style={{ left: `${(pos / total) * 100}%` }} />}

      {note && <span className="wave-note">{note}</span>}

      {aim && (
        <>
          <span className="aim" style={{ left: `${aim.pct * 100}%` }} />
          {/* The second it will land on, pinned to the cursor but kept inside the strip — at the
              very edge the panel would clip it, exactly where the number matters most. */}
          <span
            className="aim-t"
            style={{
              left: `${aim.pct * 100}%`,
              transform: `translateX(${aim.pct < 0.5 ? 4 : -100}%)`,
            }}
          >
            {mmss(aim.at)}
          </span>
        </>
      )}
    </div>
  );
}
