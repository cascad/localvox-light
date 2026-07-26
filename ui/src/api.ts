// The only contract with the daemon. Everything the app knows about the archive it
// learns here — no component talks to the network on its own.

export type JobState = "running" | "pending" | "failed";

export interface Session {
  name: string;
  title?: string | null;
  meeting?: boolean;
  started_at?: string | null;
  duration_sec: number;
  recording: boolean;
  cooked: boolean;
  empty: boolean;
  job?: JobState | null;
  has_summary: boolean;
  has_processed: boolean;
  summary_doubts?: string | null;
  processed_doubts?: string | null;
  stopped_reason?: string | null;
  lang?: string | null;
  lang_detected?: string | null;
}

export interface Line {
  start_sec: number;
  end_sec: number;
  who: string;
  text: string;
  source_id?: number;
}

export interface Version {
  id: number;
  best: boolean;
  derived: boolean;
  model: string;
  created_at: string;
  lines: number;
}

/** Somebody this recording's text attributes lines to.
 *
 *  Grouped from the lines themselves, so it cannot contain anyone who says nothing. The old list
 *  came from the diarization roster and did: on the owner's video it offered to rename a
 *  «Участник 1» who contributes not a single line to the document. */
export interface Participant {
  name: string;
  /** «отдельный голос» / «микрофон» / «системный звук» / «звук из источника» — why it is called
   *  what it is called, which is the difference between «Собеседники» and «Участник 1». */
  what: string;
  /** Someone gave this one a name; «Участник 2» and «Собеседники» are not names but honest
   *  placeholders. */
  named: boolean;
  /** What a rename has to touch. A row can carry both — the same name reached the text through a
   *  separated voice AND through the input's label — and then all of them are renamed together,
   *  or the screen would fix most of the lines and quietly leave the rest. */
  voices: string[];
  sources: number[];
  speech_sec: number;
  lines: number;
}

export interface Speaker {
  label: string;
  speech_sec: number;
  owner: boolean;
}

/** What an audio source is called in one session. A source is not a speaker — it is the input the
 *  sound arrived through, and its name is the fallback used when diarization has not named a voice. */
export interface SourceName {
  source_id: number;
  name: string;
  /** True — a human set this name; false — it is the default for this kind of session. */
  custom: boolean;
  /** «микрофон» / «системный звук» / «звук из источника» — what the input actually is. */
  what: string;
}

export interface Hit {
  session: string;
  kind: string;
  snippet: string;
  start_sec?: number | null;
  end_sec?: number | null;
  matched?: string[];
  why?: string;
  score?: number | null;
}

export interface AskSource {
  n: number;
  session: string;
  kind: string;
  text: string;
  start_sec?: number | null;
}

export interface Answer {
  text: string;
  ungrounded?: string[];
  sources?: AskSource[];
}

export interface Jobs {
  running: boolean;
  current?: string | null;
  pending: number;
  failed: number;
}

/** One line of the queue: a session waiting for the pot, in the order it will get there.
 *  Counts alone answer "is it working"; only a list answers "on what, and what is next". */
export interface QueueItem {
  session: string;
  title: string;
  started_at?: string | null;
  state: "running" | "pending" | "failed";
  kind: "cook" | "ingest";
  /** Place in the line; null for what is already cooking. */
  position?: number | null;
  attempts: number;
  last_error?: string | null;
  /** Out of retries — it will not move again without a human pressing "переварить". */
  stuck: boolean;
}

/** What the recorder is doing. `engine` and `recording` are DIFFERENT facts: with a dead
 *  daemon the record button must not look pressable, or a person walks away believing they
 *  are being recorded. */
/** How the settings screen renders a field. Presentation only — every value lands in `.env` as
 *  text; the kind decides the control, not the storage. */
export type SettingKind = "text" | "path" | "bool" | "number" | "secret";

/** WHEN a saved value starts working. Per setting, because the answer differs per setting:
 *  «live» is read at the moment it is used, «capture» re-opens the microphone, «restart» was read
 *  once into a ring buffer or a bound socket and cannot change under a running daemon. */
export type Applies = "live" | "capture" | "restart";

export interface Setting {
  key: string;
  group: string;
  label: string;
  applies: Applies;
  /** What happens when the key is left unset — placeholder text, never a value. The real default
   *  lives in the code, and claiming to know it here is how a catalogue starts lying. */
  hint: string;
  kind: SettingKind;
  /** Absent for secrets: a token that travels to the screen on every poll is a token in every log
   *  and cache between here and there. `set` still says whether one exists. */
  value?: string | null;
  set: boolean;
  /** Present when the value must be PICKED, not typed — audio devices. Enumerated live by the
   *  daemon, because devices come and go while it runs. */
  options?: DeviceChoice[] | null;
}

/** One pickable device. `value` is what goes into `.env` — for microphones a stable id, not an
 *  index or a name: indices shift when anything is plugged in, and a reconnected headset can
 *  return under a slightly different name. Either would point the recording somewhere else. */
export interface DeviceChoice {
  value: string;
  label: string;
  is_default: boolean;
}

export interface SettingsDoc {
  /** The `.env` actually in effect — shown on screen so nobody has to guess which file they edit. */
  path: string;
  exists: boolean;
  settings: Setting[];
  /** Everything else the file already holds. Hiding it would misrepresent what is in effect. */
  extra: { key: string; value: string }[];
}

/** A note being dictated right now. `slot` is the name from slots.toml — «идеи», «входящие» —
 *  already resolved, so the default slot has a real name here rather than a blank. */
export interface Capturing {
  slot: string;
  text: string;
}

/** A note that LANDED — the receipt. Answers «когда закончило писаться», and, when it went wrong,
 *  says so: a note that vanished silently is the worst outcome available. */
export interface Written {
  slot: string;
  text: string;
  dest: string;
  at: string;
  error?: string | null;
}

/** What the voice module is doing. Carries whether it is alive AT ALL — the first version only
 *  reported the dictation in flight, so a module that had heard nothing looked exactly like a
 *  module that was not running. */
export interface VoiceStatus {
  active: boolean;
  /** Triggers, slots and TTS in one line — or the reason the module is off. */
  detail: string;
  capturing?: Capturing | null;
  last?: Written | null;
}

/** What produced a derived document. Read off the header line the cook writes into the file —
 *  the file keeps it, the document on screen does not. */
export interface Provenance {
  /** The prompt template, as it is named in the repository: `summary-ru`, `cleanup-lines`. */
  template: string;
  model: string;
  glossary: number;
  /** RFC3339 with the offset the machine stamped. */
  at: string;
  doubts?: string | null;
}

/** One line of the readable text. Assembled by the daemon at READ time: the words come from the
 *  cleanup delta, the speaker and the timecode from the transcript and the session's metadata. */
export interface ReadableLine {
  who: string;
  source_id: number;
  start_sec: number;
  end_sec: number;
  text: string;
  /** The recognizer's own words — present ONLY where the cleanup changed something. This is what
   *  lets a person check the model instead of trusting it. */
  original?: string | null;
}

export interface ReadableDoc {
  lines: ReadableLine[];
  /** Which transcript version this was cleaned from. */
  version_id: number;
  provenance?: Provenance | null;
  /** Lines the model never answered for, and answers the guards rolled back. Both keep the
   *  transcript's own wording — and both are the honest measure of how much cleanup happened. */
  omitted: number;
  rejected: number;
}

/** Where a claim in the summary came from — already resolved to seconds, so the button beside it
 *  can simply play that moment. */
export interface Source {
  line: number;
  start_sec: number;
  end_sec: number;
}

/** One block of the summary. The citation markers are already out of `text`; a claim with no
 *  sources is one whose references did not survive verification — it keeps its words and loses
 *  its button, because a wrong source is worse than none: it sends a person to check, and shows
 *  them the wrong place. */
export interface SummaryBlock {
  kind: "heading" | "bullet" | "para";
  text: string;
  sources: Source[];
}

/** A note read back out of a slot. */
export interface StoredNote {
  /** The note as a person reads it — the markdown bullet and the date prefix removed. */
  text: string;
  /** The date the stored line carried, if any. Shown as a day header, the way the archive groups
   *  recordings: a date belongs beside the note, not inside its sentence. */
  date?: string | null;
  /** THE LINE AS IT IS IN THE FILE. Deletion matches on this — what the screen shows is a
   *  rendering, and deleting by a rendering would miss the line or match a different one. */
  raw: string;
  /** The file it lives in — what makes the list checkable rather than merely reassuring. */
  source: string;
}

export interface SlotNotes {
  name: string;
  default: boolean;
  /** False for a destination we can only write to. Then `error` says so — never an empty list,
   *  because «нельзя прочитать» and «здесь пусто» are opposite facts. */
  readable: boolean;
  notes: StoredNote[];
  error?: string | null;
}

export interface RecordState {
  engine: boolean;
  recording: boolean;
  session?: string | null;
  /** Seconds of the past held in memory — they enter the session when recording starts. */
  preroll_sec: number;
  /** What the voice module is doing — a DIFFERENT thing from `recording`, and both can be true at
   *  once: a note can be dictated in the middle of a meeting. */
  voice?: VoiceStatus | null;
}

/** The chain a session goes through. Stages nobody has said anything about are ABSENT from
 *  the list — a microphone recording has no download, and drawing it as "pending" would
 *  promise a step that will never come. */
export type Stage = "download" | "extract" | "transcribe" | "refine" | "summary";
export type StageState = "running" | "done" | "failed" | "skipped";

export interface StageStatus {
  stage: Stage;
  state: StageState;
  started_at?: string | null;
  ended_at?: string | null;
  /** Timestamp of the most recent event for this stage — the heartbeat freshness. */
  updated_at?: string | null;
  note?: string | null;
  /** Progress within the stage, when it reports it: `done` of `total` (chunks / batches). */
  done?: number | null;
  total?: number | null;
}

export interface Progress {
  stages: StageStatus[];
  source?: { url: string; title?: string | null } | null;
  /** Is the run still going? NOT derivable from the stages — a stage appears only once it has
   *  spoken, so in the gap between two of them every stage present says "done" and the chain
   *  reads as finished mid-cook. The queue holds the job and answers this. */
  running?: boolean;
  /** How long the whole run took, once finished (seconds). Absent while running. */
  elapsed_sec?: number | null;
}

const TOKEN_KEY = "lv-token";

export const token = {
  get: () => localStorage.getItem(TOKEN_KEY) ?? "",
  set: (v: string) => (v ? localStorage.setItem(TOKEN_KEY, v) : localStorage.removeItem(TOKEN_KEY)),
};

function headers(): HeadersInit {
  const t = token.get();
  return t ? { Authorization: `Bearer ${t}` } : {};
}

/** Errors carry the reason a human can act on, not an HTTP number. */
export class ApiError extends Error {}

async function call<T>(path: string, init: RequestInit & { timeoutMs?: number } = {}): Promise<T> {
  const { timeoutMs = 15_000, ...rest } = init;
  let r: Response;
  try {
    r = await fetch(path, {
      signal: AbortSignal.timeout(timeoutMs),
      ...rest,
      headers: { ...headers(), ...(rest.headers ?? {}) },
    });
  } catch (e) {
    throw new ApiError(
      (e as Error).name === "TimeoutError" ? "сервер не отвечает (таймаут)" : "сеть недоступна",
    );
  }
  const body = await r.json().catch(() => ({}) as Record<string, unknown>);
  if (r.status === 401) throw new ApiError("нет доступа — задайте токен");
  if (r.status === 403) throw new ApiError("доступ запрещён (открывайте через localhost, не по имени)");
  if (!r.ok) throw new ApiError(String((body as { error?: string }).error ?? `HTTP ${r.status}`));
  return body as T;
}

const post = <T,>(path: string, body: unknown = {}, timeoutMs?: number) =>
  call<T>(path, {
    method: "POST",
    body: JSON.stringify(body),
    headers: { "Content-Type": "application/json" },
    timeoutMs,
  });

const enc = encodeURIComponent;

// «Спросить у LLM про файл/текст» — an ad-hoc request stored under asks/, mirror of a session.
export type AskStatus = "pending" | "running" | "done" | "failed";

export interface AskSummary {
  id: string;
  created_at: string;
  provider: string;
  input_name?: string | null;
  input_chars: number;
  status: AskStatus;
}

export interface AskRecord {
  id: string;
  status: AskStatus;
  created_at: string;
  provider: string;
  model?: string | null;
  prompt: string;
  input_kind: string;
  input_name?: string | null;
  input_chars: number;
  answer?: string | null;
  cost_usd?: number | null;
  error?: string | null;
  input?: string;
}

export interface AskRequest {
  text: string;
  prompt?: string;
  provider?: string;
  input_name?: string;
}

export const api = {
  sessions: (limit = 50) => call<Session[]>(`/api/sessions?limit=${limit}`),
  jobs: () => call<Jobs>("/api/jobs"),
  langs: () => call<{ langs: string[] }>("/api/langs"),
  slots: () => call<{ slots: string[] }>("/api/slots"),

  transcript: (s: string) => call<{ lines: Line[] }>(`/api/sessions/${enc(s)}/transcript`),
  markdown: (s: string, kind: "summary" | "processed") =>
    call<{ markdown: string; provenance?: Provenance | null }>(
      `/api/sessions/${enc(s)}/${kind}?format=md`,
    ),
  /** The summary as blocks, each claim carrying the seconds it was drawn from. */
  summary: (s: string) =>
    call<{ blocks: SummaryBlock[]; provenance?: Provenance | null }>(
      `/api/sessions/${enc(s)}/summary`,
    ),
  /** The readable text as DATA — lines, not a page. The speaker names are joined in by the daemon
   *  on every read, which is why renaming one needs no re-cooking. */
  readable: (s: string) => call<ReadableDoc>(`/api/sessions/${enc(s)}/processed`),
  versions: (s: string) => call<{ versions: Version[] }>(`/api/sessions/${enc(s)}/versions`),
  speakers: (s: string) => call<{ speakers: Participant[] }>(`/api/sessions/${enc(s)}/speakers`),

  search: (q: string, mode: string, limit = 25) =>
    call<Hit[]>(`/api/search?q=${enc(q)}&limit=${limit}${mode ? `&mode=${mode}` : ""}`),
  // The model reads thousands of characters of the found fragments. Cutting it off
  // after 15 seconds would show "no answer" where it is honestly thinking.
  ask: (question: string) => post<Answer>("/api/ask", { question }, 180_000),

  // Ad-hoc «спросить у LLM про файл/текст». Long timeout: a document + the model's read can run.
  asks: () => call<{ asks: AskSummary[] }>("/api/asks"),
  getAsk: (id: string) => call<AskRecord>(`/api/asks/${enc(id)}`),
  createAsk: (req: AskRequest) => post<AskRecord>("/api/asks", req, 300_000),

  note: (text: string, slot?: string) =>
    post<{ dest?: string }>("/api/notes", slot ? { text, slot } : { text }),
  route: (text: string) =>
    post<{ suggestions: { slot: string; reason?: string }[] }>("/api/route", { text }),

  progress: (s: string) => call<Progress>(`/api/sessions/${enc(s)}/progress`),
  /** The shape of the recording: peak loudness per bucket, 0…1. REAL — people aim at the loud
   *  stretch by it, and a drawn wave would send them to the wrong place while looking just as
   *  trustworthy as a true one. */
  peaks: (s: string, buckets: number) =>
    call<{ duration_sec: number; peaks: number[] }>(
      `/api/sessions/${enc(s)}/peaks?buckets=${buckets}`,
      { timeoutMs: 120_000 },
    ),
  /** A link → a session. It appears in the archive at once, empty, carrying the stages of
   *  its own arrival: a link that vanishes for ten minutes is indistinguishable from a link
   *  that was dropped. */
  ingest: (url: string) => post<{ session: string }>("/api/ingest", { url }),

  /** The queue itself, in the order the daemon will work through it. */
  queue: () => call<{ queue: QueueItem[] }>("/api/queue"),

  /** What `.env` currently sets. Reports THE FILE, not the daemon's live environment: the daemon
   *  read its environment once at startup, so after a save the two disagree — and showing the live
   *  one would make a change that WAS saved look like it had failed. */
  /** What is actually IN the slots. The app used to take dictation and never mention the note
   *  again — the only way to check it landed was a file manager. */
  slotNotes: (limit = 50) => call<{ slots: SlotNotes[] }>(`/api/slots/notes?limit=${limit}`),
  /** Remove a note from a slot. Destroys a line in the person's own vault, so the note is named by
   *  its text AND its file — never by a position in a list the file's own editor may have shifted.
   *  The slot's configured path is the boundary; the daemon enforces it. */
  deleteNote: (slot: string, raw: string, source: string) =>
    call<{ deleted: string }>("/api/slots/notes", {
      method: "DELETE",
      body: JSON.stringify({ slot, raw, source }),
      headers: { "Content-Type": "application/json" },
    }),

  /** What this session's audio SOURCES are called. Not speakers: «which input the sound came
   *  through». For a session that arrived by link, source 0 is not the owner — so its default is
   *  «Рассказчик», not «Я», and a human can rename it. */
  sources: (s: string) => call<{ sources: SourceName[] }>(`/api/sessions/${enc(s)}/sources`),
  /** An empty name means «back to the default», which depends on where the audio came from. */
  nameSource: (s: string, source_id: number, name: string) =>
    post<{ msg: string }>(`/api/sessions/${enc(s)}/sources`, { source_id, name }),

  settings: () => call<SettingsDoc>("/api/settings"),
  /** Write keys into `.env`. `null` unsets a key — it is commented out and the code default takes
   *  over again. The write is surgical: comments and untouched lines survive. */
  saveSettings: (set: Record<string, string | null>) =>
    post<{ path: string; msg: string; saved: number; need_restart: string[] }>("/api/settings", {
      set,
    }),

  record: () => call<RecordState>("/api/record"),
  startRecording: (title: string) => post<{ msg: string }>("/api/record/start", { title }),
  stopRecording: () => post<{ session: string }>("/api/record/stop"),
  // Granular re-cook: the scope follows the tab the person is on. `summary` — only the summary;
  // `text` — the cleaned text (Реплики/Текст, one artifact) plus the summary that derives from it;
  // `all` (or omitted) — everything from the audio up.
  recook: (s: string, scope?: "summary" | "text" | "all") =>
    post<{ msg?: string }>(`/api/sessions/${enc(s)}/recook${scope ? `?scope=${scope}` : ""}`),
  /** Delete a session entirely — audio and all derivatives. The ONLY irreversible action:
   *  everywhere else the audio is kept and the rest recomputes from it. The name is echoed back
   *  as `confirm` so a stray call cannot wipe the wrong recording. */
  deleteSession: (s: string) =>
    call<{ deleted: string }>(`/api/sessions/${enc(s)}`, {
      method: "DELETE",
      body: JSON.stringify({ confirm: s }),
      headers: { "Content-Type": "application/json" },
    }),
  setLang: (s: string, lang: string) => post<{ msg?: string }>(`/api/sessions/${enc(s)}/lang`, { lang }),
  setBest: (s: string, id: number) => post<unknown>(`/api/sessions/${enc(s)}/best`, { id }),
  nameSpeaker: (s: string, speaker: string, name: string) =>
    post<{ msg: string }>(`/api/sessions/${enc(s)}/speakers`, { speaker, name }),
  confirm: (s: string, artifact: "summary" | "processed") =>
    post<{ msg: string }>(`/api/sessions/${enc(s)}/confirm`, { artifact }),

  // The audio itself is not fetched here — the player points a native <audio> at
  // `/api/sessions/{name}/audio.wav`, and the browser seeks it via HTTP Range. See Player.tsx.
};
