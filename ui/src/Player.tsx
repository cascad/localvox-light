import { useEffect, useRef, useState } from "react";
import { api, acceptsDocument, token, type Session, type PlaybackDoc } from "./api";
import { mmss } from "./lib";
import { useArtifact } from "./useArtifact";

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
  const reportPos = useRef(onPos);

  const [pos, setPos] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [note, setNote] = useState("");
  const [seg, setSeg] = useState<{ start: number; end: number } | null>(null);

  const name = session?.name ?? null;
  const [active, setActive] = useState<(PlaybackDoc & {name: string}) | null>(null);
  const [tick, setTick] = useState(0);
  const pendingSeek = useRef<{sec:number; autoplay:boolean; until?:number; source:string} | null>(null);
  const seekCommand = useRef(0);
  const legacy = !!session && !session.recording && (session.cooked || !session.job);
  const changing = !!session && (!legacy || active?.snapshot != null);
  useEffect(() => {
    if (!changing) return;
    const timer=setInterval(() => setTick(n=>n+1),3000);
    return () => clearInterval(timer);
  },[changing]);
  const media = useArtifact(`${name ?? ""}/playback`, signal => name
    ? legacy && active?.snapshot == null ? Promise.resolve({duration_sec:session?.duration_sec ?? 0,snapshot:null,partial:null} as PlaybackDoc) : api.playback(name,signal)
    : Promise.resolve({duration_sec:0,snapshot:null,partial:null} as PlaybackDoc),tick+(session?.duration_sec ?? 0),acceptsDocument);
  useEffect(() => {
    if (name && media.data && media.data.duration_sec>0) setActive(previous => previous?.name===name ? previous : {...media.data!,name});
  },[name,media.data]);
  const current = active?.name === name ? active : null;
  const total = current?.snapshot ? current.duration_sec : legacy ? session?.duration_sec ?? 0 : current?.duration_sec ?? 0;
  const available = name != null && total > 0;
  const auth = available ? token.get() : "";
  const mediaUrl = (snapshot?:string|null) => {
    const query=new URLSearchParams();if(auth)query.set("token",auth);if(snapshot)query.set("snapshot",snapshot);
    return `/api/sessions/${encodeURIComponent(name ?? "")}/audio.wav${query.size ? `?${query}` : ""}`;
  };
  const source = available ? mediaUrl(current?.snapshot) : null;

  // A duration refresh can update the wave without interrupting the native audio. Slow reads
  // coalesce; useArtifact retains the last successful shape even if the refresh fails.
  const waveform = useArtifact(
    `${name ?? ""}/peaks/${available}/${current?.snapshot ?? "final"}`,
    signal => available ? api.peaks(name, BARS, signal,current?.snapshot) : Promise.resolve({ duration_sec: 0, peaks: [] as number[] }),
    total,
  );
  const peaks = waveform.data?.peaks ?? (waveform.err ? [] : null);

  useEffect(() => { reportPos.current = onPos; }, [onPos]);

  // The audio source: one URL for the whole recording. Set when the session changes; the browser
  // fetches ranges of it on demand. The token (if any) rides in the query — an <audio> element
  // cannot send an Authorization header, and this stream is same-origin and local.
  useEffect(() => {
    const pending=pendingSeek.current;
    stopAt.current = null;
    setSeg(null);
    setPos(pending?.source===source ? pending.sec : 0);
    reportPos.current(pending?.source===source ? pending.sec : 0);
    setPlaying(false);
    setNote("");
    const el = audio.current;
    if (!el || !source) return;
    el.src = source;
    el.load();
    return () => {
      el.pause();
      el.removeAttribute("src");
      el.load();
    };
  }, [source]);

  const goTo = async (sec: number, autoplay: boolean, until?: number) => {
    const command=++seekCommand.current;
    const el = audio.current;
    if (!el || total <= 0) return;
    let bound=total;
    if (name && (sec>total || (sec===total && until!=null))) {
      try {
        const latest=media.data && media.data.duration_sec>sec ? media.data : await api.playback(name);
        if(command!==seekCommand.current || audio.current!==el || el.getAttribute("src")!==source)return;
        if(sec>=latest.duration_sec){setNote(`Доступно до ${mmss(latest.duration_sec)}`);return;}
        if(latest.snapshot!==current?.snapshot){
          pendingSeek.current={sec,autoplay,until,source:mediaUrl(latest.snapshot)};
          setActive({...latest,name});return;
        }
        bound=latest.duration_sec;
      }catch{setNote("Не удалось обновить границу аудио");return;}
    }
    stopAt.current = until ?? null;
    setSeg(until != null ? { start: sec, end: until } : null);
    el.currentTime = Math.max(0, Math.min(sec, bound - 0.05)); // native, instant, exact
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
    ++seekCommand.current;
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
    ++seekCommand.current;
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
        onLoadedMetadata={() => {
          const pending=pendingSeek.current;const el=audio.current;
          if (!pending || !el || el.getAttribute("src")!==pending.source) return;
          pendingSeek.current=null;el.currentTime=pending.sec;stopAt.current=pending.until ?? null;
          setSeg(pending.until==null ? null : {start:pending.sec,end:pending.until});
          if(pending.autoplay)void el.play().catch(()=>{});
        }}
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

      {media.data && media.data.duration_sec>total && <button className="icon-btn" title={`Доступно новое аудио до ${mmss(media.data.duration_sec)}`} onClick={() => {
        ++seekCommand.current;
        if(!name || !media.data)return;
        pendingSeek.current={sec:pos,autoplay:playing,until:stopAt.current ?? undefined,source:mediaUrl(media.data.snapshot)};
        setActive({...media.data,name});
      }}>+ {mmss(media.data.duration_sec-total)}</button>}

      <Wave peaks={peaks} total={total} pos={pos} seg={seg} note={note} onSeek={scrub} onKeySeek={(sec) => goTo(sec, playing)} />

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
  onKeySeek,
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
  onKeySeek: (sec: number) => void;
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
      tabIndex={0}
      onKeyDown={(event) => {
        const next = event.key === "ArrowLeft" ? pos - 5 : event.key === "ArrowRight" ? pos + 5 : event.key === "Home" ? 0 : event.key === "End" ? total : null;
        if (next != null) { event.preventDefault(); onKeySeek(next); }
      }}
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
