//! Fast-lane VAD segmentation: PCM → segments for the draft ASR (Vosk).
//!
//! Since WP-B1 the segments live in **RAM** (`SegmentPayload::Ram`): durability is provided
//! by the continuous session chunks (F8), and the draft loop does not need the disk — the
//! queue of WAV files and its recovery both disappear. The legacy on-disk mode
//! (`LOCALVOX_LIGHT_SEGMENTS_DISK=1`) is kept for the transition period; recovery of old
//! WAVs from previous runs works in both modes.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender as CrossbeamSender};
use tracing::debug;

use crate::events::{StructuredLog, UiMsg};
use hound::{SampleFormat, WavSpec, WavWriter};
use webrtc_vad::{SampleRate, Vad, VadMode};

use crate::audio::PcmChunk;
use crate::chunks::{ChunkParams, ChunkRecorder, SessionMeta};
use std::sync::Mutex;

const FRAME_SAMPLES: usize = 320; // 20 ms at 16 kHz
const FRAME_BYTES: usize = FRAME_SAMPLES * 2;

/// Where a segment's audio lives.
pub enum SegmentPayload {
    /// The fast-lane default: samples in memory (the draft does not need the disk —
    /// on a crash the slow lane will re-cook everything from the chunks).
    Ram(Vec<i16>),
    /// The legacy disk mode and recovery of WAVs from previous runs.
    Disk(PathBuf),
}

/// Notification of a ready segment for the ASR pool.
pub struct SegmentReady {
    /// Stable id (`src{N}_{SEQ:06}`) — the dedup key in transcript.jsonl.
    pub seg_id: String,
    pub payload: SegmentPayload,
    pub source_id: u8,
    /// Segment duration (sec), 16 kHz mono.
    pub duration_sec: f64,
    /// A segment from a previous run (recovery): into the transcript — yes, into the voice
    /// hook — no (otherwise yesterday's «запиши…» would be executed again at startup).
    pub from_recovery: bool,
}

pub struct PipelineConfig {
    pub max_chunk_sec: f64,
    pub min_chunk_sec: f64,
    pub vad_silence_sec: f64,
    pub work_dir: PathBuf,
    /// After `open_new` the first new file will be `max(existing seq)+1` for each source.
    pub initial_seg_seq: [u32; 2],
    /// Legacy: write segments to disk, as before WP-B1.
    pub segments_to_disk: bool,
    /// Watchdog: warn if the microphone is silent for longer (sec); 0 — disabled.
    pub mic_silence_warn_sec: f64,

    /// Session chunks (F8) at all. `false` — the engine records nothing to disk, ever.
    pub session_chunks: bool,
    /// Length of one chunk file.
    pub chunk_sec: f64,
    pub chunk_flac: bool,
    pub ffmpeg: PathBuf,

    /// The pre-roll ring: how many seconds of the PAST are kept in memory. Nothing here
    /// touches the disk — but the moment a human presses "record", this is what enters the
    /// session, so a conversation that began before the button is not lost.
    pub preroll_sec: f64,
    /// Silence on both tracks longer than this stops the recording (0 — never).
    pub autostop_sec: f64,
}

/// The sink for the current segment.
enum SegSink {
    Ram(Vec<i16>),
    Disk {
        writer: WavWriter<File>,
        part_path: PathBuf,
    },
}

struct SourceState {
    source_id: u8,
    vad: Vad,
    seq: u32,
    duration_sec: f64,
    silence_frames: u32,
    silence_threshold_frames: u32,
    to_disk: bool,
    sink: Option<SegSink>,
    pcm_remainder: Vec<u8>,
}

impl SourceState {
    /// `last_seq_on_disk` — the highest already existing segment number; the next one will be +1.
    fn new(
        source_id: u8,
        silence_threshold_frames: u32,
        last_seq_on_disk: u32,
        to_disk: bool,
    ) -> Self {
        let mut vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::LowBitrate);
        vad.set_sample_rate(SampleRate::Rate16kHz);
        Self {
            source_id,
            vad,
            seq: last_seq_on_disk,
            duration_sec: 0.0,
            silence_frames: 0,
            silence_threshold_frames,
            to_disk,
            sink: None,
            pcm_remainder: Vec::new(),
        }
    }

    fn seg_id(&self) -> String {
        format!("src{}_{:06}", self.source_id, self.seq)
    }

    fn feed(
        &mut self,
        samples: &[i16],
        cfg: &PipelineConfig,
    ) -> Vec<(String, SegmentPayload, f64, f64)> {
        let pcm_bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.pcm_remainder.extend_from_slice(&pcm_bytes);

        let chunk_sec = samples.len() as f64 / 16000.0;
        self.duration_sec += chunk_sec;

        let (_any_speech, should_flush) = self.run_vad();
        let flush_vad =
            should_flush && self.duration_sec >= cfg.min_chunk_sec && self.sink.is_some();
        let flush_time = self.duration_sec >= cfg.max_chunk_sec && self.sink.is_some();

        let mut completed = Vec::new();

        if flush_vad || flush_time {
            let seg_dur = self.duration_sec;
            self.write_samples(samples);
            let t_write = Instant::now();
            let seg_id = self.seg_id();
            if let Some(payload) = self.finalize() {
                let write_sec = t_write.elapsed().as_secs_f64();
                completed.push((seg_id, payload, seg_dur, write_sec));
            }
            self.duration_sec = 0.0;
            self.silence_frames = 0;
            self.open_new(&cfg.work_dir);
        } else {
            if self.sink.is_none() {
                self.open_new(&cfg.work_dir);
            }
            self.write_samples(samples);
        }
        completed
    }

    fn run_vad(&mut self) -> (bool, bool) {
        let mut any_speech = false;
        while self.pcm_remainder.len() >= FRAME_BYTES {
            let frame: Vec<u8> = self.pcm_remainder.drain(..FRAME_BYTES).collect();
            let i16_samples: Vec<i16> = frame
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            let is_speech = self.vad.is_voice_segment(&i16_samples).unwrap_or(true);
            if is_speech {
                any_speech = true;
                self.silence_frames = 0;
            } else {
                self.silence_frames += 1;
            }
        }
        let should_flush = self.silence_frames >= self.silence_threshold_frames;
        (any_speech, should_flush)
    }

    fn open_new(&mut self, work_dir: &Path) {
        self.seq += 1;
        if !self.to_disk {
            self.sink = Some(SegSink::Ram(Vec::new()));
            return;
        }
        let path = work_dir.join(format!("{}.part", self.seg_id()));
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        match File::create(&path)
            .and_then(|f| WavWriter::new(f, spec).map_err(std::io::Error::other))
        {
            Ok(w) => {
                self.sink = Some(SegSink::Disk {
                    writer: w,
                    part_path: path,
                });
            }
            Err(e) => tracing::error!("Failed to create segment file: {e}"),
        }
    }

    fn write_samples(&mut self, samples: &[i16]) {
        match &mut self.sink {
            Some(SegSink::Ram(buf)) => buf.extend_from_slice(samples),
            Some(SegSink::Disk { writer, .. }) => {
                for &s in samples {
                    let _ = writer.write_sample(s);
                }
            }
            None => {}
        }
    }

    fn finalize(&mut self) -> Option<SegmentPayload> {
        match self.sink.take()? {
            SegSink::Ram(buf) => {
                if buf.is_empty() {
                    None
                } else {
                    Some(SegmentPayload::Ram(buf))
                }
            }
            SegSink::Disk {
                mut writer,
                part_path,
            } => {
                let _ = writer.flush();
                drop(writer);
                let size = fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
                if size <= 44 {
                    let _ = fs::remove_file(&part_path);
                    return None;
                }
                let final_path = part_path.with_extension("wav");
                if fs::rename(&part_path, &final_path).is_err() {
                    return None;
                }
                Some(SegmentPayload::Disk(final_path))
            }
        }
    }

    fn flush(&mut self) -> Option<(String, SegmentPayload, f64, f64)> {
        let dur = self.duration_sec;
        let t_write = Instant::now();
        let seg_id = self.seg_id();
        let payload = self.finalize()?;
        let write_sec = t_write.elapsed().as_secs_f64();
        self.duration_sec = 0.0;
        self.silence_frames = 0;
        Some((seg_id, payload, dur, write_sec))
    }

    /// Recording pause: drop the current segment (RAM — just drop it; disk — delete the `.part`).
    fn discard_open_segment(&mut self) {
        if let Some(SegSink::Disk { part_path, .. }) = self.sink.take() {
            let _ = fs::remove_file(&part_path);
        }
        self.duration_sec = 0.0;
        self.silence_frames = 0;
        self.pcm_remainder.clear();
    }
}

/// The session-change signal (F3): call detection tells the pipeline that a call started or
/// ended. Since the recording became explicit, this signal no longer STARTS anything — it
/// only marks a running recording ("a call was going here"). The pipeline stays the sole
/// owner of the session lifecycle and the sole writer of `meta.json`.
pub enum SessionSignal {
    CallStarted { apps: Vec<String>, title: String },
    CallEnded,
}

/// Settings for creating a session when a human starts a recording.
#[derive(Clone)]
struct SessionSettings {
    work_dir: PathBuf,
    chunk_sec: f64,
    flac: bool,
    ffmpeg: PathBuf,
    /// How many seconds of the past the ring keeps — and therefore how much of the
    /// conversation before the button lands in the session.
    preroll_sec: f64,
    /// Silence on both tracks longer than this stops the recording (0 — never).
    autostop_sec: f64,
}

struct Session {
    params: Arc<ChunkParams>,
    meta: Arc<Mutex<SessionMeta>>,
}

/// Recording of session chunks (F8).
///
/// **The recording is not the default state.** Capture runs always — the microphone and the
/// loopback are open, the levels are measured, the watchdogs are watching — but NOTHING
/// reaches the disk until a human says so. A recorder running all day through an office
/// writes other people's conversations, and none of those people agreed to that.
///
/// What makes the button safe is the pre-roll ring: the last `preroll_sec` of both tracks
/// live in memory (five minutes ≈ 19 MB) and never touch the disk. Press "record" — and they
/// go INTO the session, so a conversation that began before the button is not lost. This is
/// loop recording, the same trick a dashcam has used for twenty years.
///
/// `settings == None` — chunks are off entirely, every call is a no-op.
struct ChunkLane {
    settings: Option<SessionSettings>,
    /// How much audio arrived against real time — per source. Runs regardless of recording:
    /// it watches the DEVICE, not our willingness to write.
    integrity: [Integrity; 2],
    /// Is a recording running right now. The single source of truth for "does the sound reach
    /// the disk".
    armed: bool,
    current: Option<Session>,
    recorders: [Option<ChunkRecorder>; 2],
    /// The last `preroll_cap` samples of each source. Filled always — this is the past we can
    /// still save.
    preroll: [std::collections::VecDeque<i16>; 2],
    preroll_cap: usize,
    /// A call is being recorded into the running session right now (a mark, not a session).
    in_meeting: bool,
    /// The moment of the last sound (for the auto-stop on silence); whether anything was
    /// written at all.
    last_voice: Option<Instant>,
    recorded_since_open: bool,
    /// A buffer of samples for the time while there is no "home" (create_session_dir is
    /// temporarily failing) — it will be written into the very first session that gets
    /// created; bounded in length.
    orphan: [Vec<i16>; 2],
    /// The frame counter until the next command check: we poke the files once a second, not
    /// on every audio frame.
    poll: u32,
    /// The title the human gave when starting ("" — a recording without a name).
    title: Option<String>,
}

/// Frames between checks of the start/stop request files (a frame is 20 ms).
const COMMAND_POLL_FRAMES: u32 = 50;

impl ChunkLane {
    fn new(cfg: &PipelineConfig) -> Self {
        let settings = cfg.session_chunks.then(|| SessionSettings {
            work_dir: cfg.work_dir.clone(),
            chunk_sec: cfg.chunk_sec,
            flac: cfg.chunk_flac,
            ffmpeg: cfg.ffmpeg.clone(),
            preroll_sec: cfg.preroll_sec,
            autostop_sec: cfg.autostop_sec,
        });
        let preroll_cap = settings
            .as_ref()
            .map(|s| (s.preroll_sec * 16_000.0) as usize)
            .unwrap_or(0);
        let lane = Self {
            settings,
            integrity: Default::default(),
            armed: false,
            current: None,
            recorders: [None, None],
            preroll: [
                std::collections::VecDeque::new(),
                std::collections::VecDeque::new(),
            ],
            preroll_cap,
            in_meeting: false,
            last_voice: None,
            recorded_since_open: false,
            orphan: [Vec::new(), Vec::new()],
            poll: 0,
            title: None,
        };
        // No recording is running. A marker left by a killed daemon would make the archive
        // claim forever that a session is live — we clear it here, at the one place that
        // knows the truth.
        lane.sync_marker();
        lane
    }

    /// Pause: stop the integrity watchdog's clock (a pause is not a loss).
    fn pause_integrity(&mut self) {
        for i in &mut self.integrity {
            i.pause();
        }
    }

    fn feed(&mut self, source_id: u8, samples: &[i16]) {
        if self.settings.is_none() {
            return;
        }
        let sid = source_id as usize;
        if sid > 1 {
            return;
        }
        // The integrity watchdog: how much audio arrived against real time.
        // We no longer create the loss ourselves (the channel is unbounded), but the device
        // can fail — and then we MUST SEE it, rather than guess by ear, as the owner had to.
        // The watchdog asks THE DEVICE, not our queue: a loss is when the sound did not
        // arrive, not when we did not manage to process it in time.
        self.integrity[sid].observe(source_id, crate::audio::captured(source_id), Instant::now());
        // The pre-roll ring: the last preroll_cap samples of the source. Filled ALWAYS,
        // recording or not — it is memory, not disk, and it is the only thing that can still
        // save a conversation that began before the button.
        if self.preroll_cap > 0 {
            let ring = &mut self.preroll[sid];
            for &s in samples {
                if ring.len() == self.preroll_cap {
                    ring.pop_front();
                }
                ring.push_back(s);
            }
        }
        // Activity for the auto-stop: any loud track (the microphone OR the system sound in
        // loopback — a webinar, a call) keeps the recording alive. True silence on both
        // sources is what moves the timer.
        if crate::audio::pcm_level_i16(samples) > 0.003 {
            self.last_voice = Some(Instant::now());
        }
        // Start/stop from the web UI, the tray or the voice command. We check no more than
        // once a second — these are file-existence checks, and we are called on every frame.
        self.poll += 1;
        if self.poll >= COMMAND_POLL_FRAMES {
            self.poll = 0;
            self.poll_commands();
        }

        // THE GATE. No recording — nothing reaches the disk. Everything above this line
        // (levels, watchdogs, the ring) keeps running: we listen always, we write on command.
        if !self.armed {
            return;
        }

        self.ensure_current();
        let Some(session) = &self.current else {
            // No "home" (create_session_dir is temporarily failing): we accumulate into a
            // bounded buffer and will write it into the very first session that gets created.
            let cap = self.preroll_cap.max(16_000 * 60); // ≥1 min, no unbounded growth
            let buf = &mut self.orphan[sid];
            buf.extend_from_slice(samples);
            if buf.len() > cap {
                buf.drain(..buf.len() - cap);
            }
            return;
        };
        let params = session.params.clone();
        let meta = session.meta.clone();
        let rec =
            self.recorders[sid].get_or_insert_with(|| ChunkRecorder::new(source_id, params, meta));
        if !self.orphan[sid].is_empty() {
            let pending = std::mem::take(&mut self.orphan[sid]);
            rec.feed(&pending);
        }
        rec.feed(samples);
        self.recorded_since_open = true;
    }

    /// Synchronize the `.recording_session` marker file with the current session:
    /// the auto-cook (WP-C7) uses it to exclude the actively-being-written session from the cook.
    fn sync_marker(&self) {
        let Some(s) = &self.settings else { return };
        let marker = s.work_dir.join(crate::jobs::RECORDING_MARKER);
        match &self.current {
            Some(session) => {
                if let Some(name) = session
                    .params
                    .meta_path
                    .parent()
                    .and_then(|d| d.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                {
                    let _ = std::fs::write(&marker, name);
                }
            }
            None => {
                let _ = std::fs::remove_file(&marker);
            }
        }
    }

    /// The session for the running recording. Called on arming, and again on every frame
    /// while armed — so that a session that failed to be created (a full disk, a locked
    /// directory) is retried instead of silently dropping the recording on the floor.
    fn ensure_current(&mut self) {
        if self.current.is_some() || !self.armed {
            return;
        }
        let Some(s) = self.settings.clone() else { return };
        let title = self.title.clone().unwrap_or_default();
        let label = (!title.is_empty()).then_some(title.as_str());

        match crate::chunks::create_session_dir(&s.work_dir, label) {
            Ok((audio_dir, meta_path)) => {
                // THE RECORDING BEGAN BEFORE THE BUTTON. The ring holds the last minutes, and
                // they go into this session — so `started_at` is moved back by exactly as much
                // audio as we are about to seed. Otherwise the archive's clock lies: the file
                // would open with a conversation that, by its own timestamps, had not started
                // yet.
                let seeded = self.preroll_len_sec();
                let began = chrono::Local::now()
                    - chrono::Duration::milliseconds((seeded * 1000.0).round() as i64);
                let meta = SessionMeta {
                    started_at: began.to_rfc3339(),
                    sample_rate: 16_000,
                    chunks: Vec::new(),
                    title: label.map(str::to_string),
                    meeting: label.is_some(),
                    ..Default::default()
                };
                // The meta is written IMMEDIATELY, not when the first chunk closes: otherwise a
                // new session looks dead and empty in the archive for minutes, and the human
                // cannot tell where the sound is going now.
                crate::chunks::save_meta_public(&meta_path, &meta);
                self.current = Some(Session {
                    params: Arc::new(ChunkParams {
                        audio_dir,
                        meta_path,
                        chunk_sec: s.chunk_sec,
                        flac: s.flac,
                        ffmpeg: s.ffmpeg.clone(),
                    }),
                    meta: Arc::new(Mutex::new(meta)),
                });
                self.recorded_since_open = false;
                self.sync_marker();
                self.seed_preroll();
                tracing::info!(preroll_sec = seeded, title = %title, "recording started");
            }
            Err(e) => tracing::error!("session was not created: {e}"),
        }
    }

    /// How many seconds of the past the ring is holding right now (the longer track: the two
    /// are seeded together and the session must cover both).
    fn preroll_len_sec(&self) -> f64 {
        let longest = self.preroll.iter().map(|r| r.len()).max().unwrap_or(0);
        longest as f64 / 16_000.0
    }

    /// A detection signal only MARKS a running recording — it starts nothing.
    ///
    /// Detection sees a call by an application holding the microphone, and for Zoom, Teams and
    /// the browser that honestly works. But it cannot see a stand-up at the table, and it
    /// cannot tell a work call from a private one. Letting it start the recorder by itself
    /// means recording people who never agreed to it. So the human presses the button, and
    /// detection's job is to leave a mark in the meta: "a call was going here, in these apps".
    /// That mark is what a future "start recording?" prompt will be built on.
    fn on_signal(&mut self, sig: SessionSignal) {
        if !self.armed {
            return;
        }
        match sig {
            SessionSignal::CallStarted { apps, title } => {
                let Some(session) = &self.current else { return };
                let mark = crate::chunks::MeetingMark {
                    apps,
                    window_title: title,
                    started_at: crate::versions::now_rfc3339(),
                    ended_at: None,
                };
                if let Ok(mut m) = session.meta.lock() {
                    m.meetings.push(mark);
                    crate::chunks::save_meta_public(&session.params.meta_path, &m);
                }
                self.in_meeting = true;
            }
            SessionSignal::CallEnded => {
                if self.in_meeting {
                    self.close_meeting_mark();
                }
                self.in_meeting = false;
            }
        }
    }

    /// Set `ended_at` on the last meeting of the current session.
    fn close_meeting_mark(&mut self) {
        let Some(session) = &self.current else { return };
        if let Ok(mut m) = session.meta.lock() {
            if let Some(last) = m.meetings.last_mut() {
                if last.ended_at.is_none() {
                    last.ended_at = Some(crate::versions::now_rfc3339());
                }
            }
            crate::chunks::save_meta_public(&session.params.meta_path, &m);
        }
    }

    /// Seed the new session with the accumulated pre-roll ring (both sources).
    fn seed_preroll(&mut self) {
        for sid in 0..2usize {
            if self.preroll[sid].is_empty() {
                continue;
            }
            let ring: Vec<i16> = self.preroll[sid].iter().copied().collect();
            let Some(session) = &self.current else { return };
            let rec = self.recorders[sid].get_or_insert_with(|| {
                ChunkRecorder::new(sid as u8, session.params.clone(), session.meta.clone())
            });
            rec.feed(&ring);
            self.recorded_since_open = true;
        }
    }

    /// Start/stop requests from the web UI, the tray or the voice command.
    ///
    /// Files, not a channel: the request comes from another thread (HTTP, tray) or another
    /// process, while the session is owned by this loop. A file is the cheapest way to shout
    /// across any boundary, and it survives everything short of the disk being deleted.
    fn poll_commands(&mut self) {
        let Some(s) = &self.settings else { return };
        let work_dir = s.work_dir.clone();
        if let Some(title) = crate::jobs::take_record_start(&work_dir) {
            self.start(title);
        }
        if let Some(reason) = crate::jobs::take_record_stop(&work_dir) {
            self.stop(&reason);
        }
    }

    /// Start recording. The session is created RIGHT HERE, not on the next frame: the human
    /// pressed the button and must see the recording in the archive at once, not "some day".
    fn start(&mut self, title: String) {
        if self.settings.is_none() {
            return;
        }
        if self.armed {
            // A second press is not a second session. Silently opening a new one would split
            // the conversation in half at the moment the human doubted the button worked.
            tracing::info!("recording is already running — the start is ignored");
            return;
        }
        self.armed = true;
        self.title = Some(title);
        // The auto-stop counts from the start: a recording begun in silence must not close on
        // the very first check.
        self.last_voice = Some(Instant::now());
        self.ensure_current();
    }

    /// Stop recording. The reason goes into `meta.json` — the archive must be able to say who
    /// ended the recording and why: a human, a voice command, or the silence watchdog.
    fn stop(&mut self, reason: &str) {
        if !self.armed && self.current.is_none() {
            tracing::info!("\"stop\": no recording is running");
            return;
        }
        if self.in_meeting {
            self.close_meeting_mark();
        }
        if let Some(session) = &self.current {
            let mut meta = session.meta.lock().unwrap_or_else(|e| e.into_inner());
            meta.stopped_at = Some(crate::versions::now_rfc3339());
            meta.stopped_reason = Some(reason.to_string());
            crate::chunks::save_meta_public(&session.params.meta_path, &meta);
        }
        for rec in self.recorders.iter_mut().flatten() {
            rec.finalize_current();
        }
        self.recorders = [None, None];
        self.current = None;
        self.armed = false;
        self.in_meeting = false;
        self.recorded_since_open = false;
        self.title = None;
        self.last_voice = None;
        // The marker goes away only after the last chunk is written: while it is there, the
        // cook keeps its hands off the session.
        self.sync_marker();
        tracing::info!("recording stopped: {reason}");
    }

    /// The auto-stop: silence on BOTH tracks for longer than `autostop_sec` closes the
    /// recording.
    ///
    /// Without it a recording nobody ended runs until the daemon dies: the cook waits for a
    /// session that never closes, and the archive fills with hours of an empty room. The
    /// reason lands in the meta, so this never looks like a crash.
    fn maybe_autostop(&mut self, now: Instant) {
        let Some(s) = &self.settings else { return };
        if !self.armed || s.autostop_sec <= 0.0 {
            return;
        }
        let quiet = self
            .last_voice
            .map(|last| now.duration_since(last).as_secs_f64())
            .unwrap_or(0.0);
        if quiet < s.autostop_sec {
            return;
        }
        let minutes = (s.autostop_sec / 60.0).round().max(1.0);
        self.stop(&format!("тишина {minutes:.0} мин"));
    }

    /// Pause: close the open chunks, but DO NOT touch the meeting's `ended_at` — the call is
    /// still going, the end will be set by the real CallEnded (or by the exit).
    fn flush_recorders(&mut self) {
        for rec in self.recorders.iter_mut().flatten() {
            rec.finalize_current();
        }
    }

    /// Process shutdown: the chunks are the primary artifact, we close them cleanly; an open
    /// meeting is closed best-effort (there will be no real CallEnded any more).
    fn finalize_all(&mut self) {
        if self.in_meeting {
            self.close_meeting_mark();
        }
        // First finish writing, THEN remove the marker — as everywhere else.
        // The marker is a promise "the session is still being touched": removing it before the
        // last meta write lets a re-cook and a language change at the session at exactly the
        // moment we are still writing over it.
        for rec in self.recorders.iter_mut().flatten() {
            rec.finalize_current();
        }
        // The marker is removed: the session is finished, the auto-cook will finish cooking it
        // on the next start.
        if let Some(s) = &self.settings {
            let _ = std::fs::remove_file(s.work_dir.join(crate::jobs::RECORDING_MARKER));
        }
    }
}

/// Microphone watchdog (F9): "went silent" = the PCM stream is flowing but the level is near
/// zero for longer than the threshold (muted physically / in the OS), or PCM stopped arriving
/// altogether. Only for source 0 — silence in loopback is normal (there is simply no sound in
/// the system).
struct MicHealth {
    warn_after: Duration,
    last_data: Option<Instant>,
    last_loud: Option<Instant>,
    warned: bool,
}

impl MicHealth {
    fn new(warn_after_sec: f64) -> Self {
        Self {
            warn_after: Duration::from_secs_f64(warn_after_sec.max(3.0)),
            last_data: None,
            last_loud: None,
            warned: false,
        }
    }

    /// Update on an incoming piece; returns a recovery message.
    fn on_chunk(&mut self, samples: &[i16], now: Instant) -> Option<String> {
        self.last_data = Some(now);
        // level: the RMS-like estimate from audio.rs; the ~0.003 threshold catches "digital silence"
        if crate::audio::pcm_level_i16(samples) > 0.003 {
            self.last_loud = Some(now);
            if self.warned {
                self.warned = false;
                return Some("Микрофон снова слышен".into());
            }
        }
        None
    }

    /// Periodic check; returns a warning (once, until recovery).
    fn check(&mut self, now: Instant) -> Option<String> {
        if self.warned {
            return None;
        }
        let started = self.last_data?; // before the first piece we do not complain — the mic thread handles that
        let quiet_since = self.last_loud.unwrap_or(started);
        let (elapsed, what) = match self.last_data {
            Some(t) if now.duration_since(t) > self.warn_after => {
                (now.duration_since(t), "нет аудио-потока с микрофона")
            }
            _ if now.duration_since(quiet_since) > self.warn_after => {
                (now.duration_since(quiet_since), "микрофон молчит (mute?)")
            }
            _ => return None,
        };
        self.warned = true;
        Some(format!("⚠ {what} уже {} с", elapsed.as_secs()))
    }
}

/// The SYSTEM-AUDIO watchdog: a call is running, but the other side is not being recorded.
///
/// Why there was none, and why that reasoning was wrong. `MicHealth` above says: "only for source
/// 0 — silence in loopback is normal (there is simply no sound in the system)". The premise is
/// true. The conclusion is not: from «silence is sometimes normal» does not follow «silence is
/// always normal». Half an hour of silence on the system track WHILE A CALL IS RUNNING is not
/// normal — it means the other side's voice is going somewhere we are not listening.
///
/// Measured, session 20260716_164443 (16.07.2026). The call moved from Firefox to Chrome:
///   16:46:51.7 — the last live sample of the other side
///   16:46:52.9 — firefox.exe «Звонок в Яндекс Телемосте» closes (1.2 s later)
///   16:47:07.9 → 17:13:41 — chrome.exe holds the call, 26.5 minutes, not one sample
/// Chrome rendered past the endpoint we had bound to. The stream stayed alive and kept delivering
/// packets — at ±1 LSB, the converter's dither — so nothing looked broken: `Integrity` measures
/// TIME and honestly saw no gap; `MicHealth` was gated off for this source; `Backlog` watches the
/// queue. Half the conversation was lost for 27 minutes and the owner learned it two days later,
/// by ear. `rms=0` had been sitting in the meta since 17:33 with nobody to read it.
///
/// So the alarm is not on «quiet» but on the CORRELATION: the detector sees an app holding the
/// microphone (a call is on) AND the system track has carried nothing for minutes. That is a fact
/// about the world, not a guess — and it is the exact signal that was there to catch all along.
struct LoopbackHealth {
    warn_after: Duration,
    /// Last time the system track carried actual sound.
    last_loud: Option<Instant>,
    /// Have we seen ANY system-audio data? `None` — loopback is off or has not started, and then
    /// there is nothing to complain about: the human turned it off, that is their choice.
    seen_data: bool,
    /// Since when a call has been running. Silence is only counted from that moment: what happened
    /// before the call is none of the watchdog's business.
    call_since: Option<Instant>,
    warned: bool,
}

impl LoopbackHealth {
    fn new(warn_after_sec: f64) -> Self {
        Self {
            // Below a minute this would cry over an ordinary pause in a conversation.
            warn_after: Duration::from_secs_f64(warn_after_sec.max(60.0)),
            last_loud: None,
            seen_data: false,
            call_since: None,
            warned: false,
        }
    }

    fn on_chunk(&mut self, samples: &[i16], now: Instant) -> Option<String> {
        self.seen_data = true;
        // The same "digital silence" threshold as the microphone's. It catches the case measured
        // here with room to spare: ±1 LSB is ~0.00001, the threshold is 0.003.
        if crate::audio::pcm_level_i16(samples) > 0.003 {
            self.last_loud = Some(now);
            if self.warned {
                self.warned = false;
                return Some("Системный звук снова слышен".into());
            }
        }
        None
    }

    /// `in_call` — the detector sees an application holding the microphone.
    fn check(&mut self, now: Instant, in_call: bool) -> Option<String> {
        if !in_call {
            // No call — silence on the system track is the norm, and there is nothing to watch.
            self.call_since = None;
            self.warned = false;
            return None;
        }
        let since = *self.call_since.get_or_insert(now);
        if self.warned || !self.seen_data {
            return None;
        }
        // Count the silence from the later of: the call's start, the last sound.
        let quiet_since = match self.last_loud {
            Some(loud) if loud > since => loud,
            _ => since,
        };
        let quiet = now.duration_since(quiet_since);
        if quiet <= self.warn_after {
            return None;
        }
        self.warned = true;
        Some(format!(
            "⚠ идёт звонок, но системный звук молчит уже {} мин — собеседника не слышно, \
             проверьте устройство вывода (звук ушёл на другое?)",
            quiet.as_secs() / 60
        ))
    }
}

fn sync_pause_discard(
    sources: &mut [Option<SourceState>; 2],
    chunk_lane: &mut ChunkLane,
    record_pcm: &Arc<AtomicBool>,
    prev_recording: &mut bool,
) {
    let cur = record_pcm.load(Ordering::Relaxed);
    if !cur && *prev_recording {
        for src in sources.iter_mut().flatten() {
            src.discard_open_segment();
        }
        // Pause: close the chunks, but do not finish the meeting (the call may still be going).
        chunk_lane.flush_recorders();
        // We stop the integrity watchdog's clock: otherwise a pause would look like a loss of
        // recording, and it would shout for nothing.
        chunk_lane.pause_integrity();
    }
    *prev_recording = cur;
}

#[allow(clippy::too_many_arguments)]
fn feed_pcm_chunk(
    sources: &mut [Option<SourceState>; 2],
    chunk_lane: &mut ChunkLane,
    chunk: PcmChunk,
    silence_frames: u32,
    cfg: &PipelineConfig,
    seg_tx: &CrossbeamSender<SegmentReady>,
    pending: &Arc<AtomicUsize>,
    log_tx: &Option<CrossbeamSender<UiMsg>>,
) {
    let sid = chunk.source_id as usize;
    if sid > 1 {
        return;
    }

    // The primary artifact: the continuous session chunks (F8), in parallel with the fast-lane
    // segments.
    chunk_lane.feed(chunk.source_id, &chunk.samples);

    let state = sources[sid].get_or_insert_with(|| {
        SourceState::new(
            chunk.source_id,
            silence_frames,
            cfg.initial_seg_seq[sid],
            cfg.segments_to_disk,
        )
    });

    let completed = state.feed(&chunk.samples, cfg);
    for (seg_id, payload, duration_sec, write_sec) in completed {
        send_segment(
            seg_tx,
            pending,
            log_tx,
            SegmentReady {
                seg_id,
                payload,
                source_id: chunk.source_id,
                duration_sec,
                from_recovery: false,
            },
            write_sec,
            "",
        );
    }
}

fn send_segment(
    seg_tx: &CrossbeamSender<SegmentReady>,
    pending: &Arc<AtomicUsize>,
    log_tx: &Option<CrossbeamSender<UiMsg>>,
    seg: SegmentReady,
    write_sec: f64,
    suffix: &str,
) {
    let detail = format!("{}{suffix}", seg.seg_id);
    let source_id = seg.source_id;
    let duration_sec = seg.duration_sec;
    pending.fetch_add(1, Ordering::Relaxed);
    if seg_tx.send(seg).is_err() {
        pending.fetch_sub(1, Ordering::Relaxed);
        return;
    }
    debug!("Segment ready src{source_id}: {detail}");
    if let Some(ref tx) = log_tx {
        let _ = tx.send(UiMsg::Log(StructuredLog {
            stage: "segment".into(),
            source_id,
            chunk_sec: duration_sec,
            proc_sec: write_sec,
            detail,
            verbose_only: true,
        }));
    }
}

/// The RECORDING INTEGRITY watchdog: did as much audio arrive as time went by.
///
/// Acceptance 13.07.2026: under load HALF the time got recorded, the file played twice as
/// fast, the ASR received mush — and there was NOTHING to learn about it from. The owner found
/// it, by ear, two weeks later. We eliminated the loss itself (the channel is unbounded), but
/// the device can drop out without our help, and then we MUST SEE it, not guess.
///
/// A pause is counted honestly: the clock runs only while recording runs.
#[derive(Default)]
struct Integrity {
    samples: u64,
    /// The device counter is cumulative (from the start of the process) while the session
    /// starts later — we remember the reference point.
    base: Option<u64>,
    since: Option<Instant>,
    /// The worst discrepancy over the session, seconds. Written into `meta.json`.
    worst_gap_sec: f64,
    warned: bool,
}

impl Integrity {
    /// The discrepancy at which it is time to shout: 5 % loss is audible.
    const ALARM: f64 = 0.05;
    /// Before this we pass no judgement: in the first seconds the measurement noise is large.
    const MIN_WALL: f64 = 10.0;

    /// `captured` — how many samples ARRIVED FROM THE DEVICE over the whole life of the
    /// process ([`crate::audio::captured`]). Not "how many we managed to process": the queue is
    /// latency, not loss, and the two must not be confused.
    ///
    /// Caught live (13.07.2026): the daemon queued 17 sessions, the cook saturated the cores,
    /// the consumer fell 45 pieces behind — and the watchdog screamed "RECORDING LOSS" about
    /// audio that was sitting calmly in memory. A warning that lies is worse than no warning:
    /// people stop reading it, and the real loss gets missed.
    fn observe(&mut self, source_id: u8, captured: u64, now: Instant) {
        let start = *self.since.get_or_insert(now);
        let base = *self.base.get_or_insert(captured);
        self.samples = captured.saturating_sub(base);
        let wall = (now - start).as_secs_f64();
        if wall < Self::MIN_WALL {
            return;
        }
        let audio = self.samples as f64 / f64::from(crate::audio::SAMPLE_RATE);
        let gap = wall - audio;
        if gap > self.worst_gap_sec {
            self.worst_gap_sec = gap;
        }
        if !self.warned && gap / wall > Self::ALARM {
            self.warned = true;
            // WHICH track is behind — otherwise the alarm is useless: the microphone and the
            // system audio are lost for entirely different reasons, and the first question a
            // human asks is «which of the two?».
            let track = if source_id == 0 {
                "microphone"
            } else {
                "system audio"
            };
            tracing::error!(
                "RECORDING LOSS ({track}): received {audio:.0} s of audio over {wall:.0} s of \
                 real time ({:.0} % missing). The recording will play faster than reality, \
                 and the transcript will come out as mush",
                gap / wall * 100.0
            );
        }
    }

    /// Pause: we stop the clock, otherwise a pause would look like a loss. We reset the
    /// reference point too — after a pause we count anew.
    fn pause(&mut self) {
        self.since = None;
        self.base = None;
        self.samples = 0;
    }
}

#[cfg(test)]
mod integrity_tests {
    use super::Integrity;
    use std::time::{Duration, Instant};

    /// A normal recording: as much time passed — as much audio arrived.
    #[test]
    fn a_healthy_capture_raises_no_alarm() {
        let mut g = Integrity::default();
        let t0 = Instant::now();
        let mut captured = 0u64;
        // 30 seconds in 32 ms pieces, exactly on tempo
        for i in 0..(30_000 / 32) {
            captured += 512;
            g.observe(0, captured, t0 + Duration::from_millis(i * 32));
        }
        assert!(
            g.worst_gap_sec < 0.5,
            "false alarm: {:.2} s",
            g.worst_gap_sec
        );
        assert!(!g.warned);
    }

    /// THAT VERY FAILURE: the device gave back half as much audio as time went by.
    /// The watchdog MUST shout.
    #[test]
    fn losing_half_the_audio_is_loud() {
        let mut g = Integrity::default();
        let t0 = Instant::now();
        let mut captured = 0u64;
        for i in 0..(30_000 / 32) {
            captured += 256; // ← half
            g.observe(0, captured, t0 + Duration::from_millis(i * 32));
        }
        assert!(g.warned, "the loss of half the recording passed in silence");
        assert!(g.worst_gap_sec > 10.0, "{:.1} s", g.worst_gap_sec);
    }

    /// THE QUEUE IS NOT A LOSS, and this is the main reason the watchdog was reworked.
    ///
    /// Live (13.07.2026): the cook saturated the cores, the consumer fell 45 pieces behind —
    /// and the watchdog screamed "RECORDING LOSS" about audio that was lying in memory intact.
    /// The device gave back EVERYTHING; it was us who fell behind. The counting must be done at
    /// the device.
    #[test]
    fn a_backlog_in_our_own_queue_is_not_a_loss() {
        let mut g = Integrity::default();
        let t0 = Instant::now();
        let mut captured = 0u64;
        for i in 0..(30_000 / 32) {
            // The device faithfully gives back 512 samples every 32 ms…
            captured += 512;
            // …while we, say, processed only a third — but it is THE DEVICE that we ask.
            g.observe(0, captured, t0 + Duration::from_millis(i * 32));
        }
        assert!(
            !g.warned,
            "the watchdog mistook our own queue for a loss of recording"
        );
    }

    /// A PAUSE is not a loss. The clock stops together with the recording, otherwise the
    /// watchdog would shout on every pause and people would stop listening to it.
    #[test]
    fn a_pause_is_not_a_loss() {
        let mut g = Integrity::default();
        let t0 = Instant::now();
        let mut captured = 0u64;
        for i in 0..(20_000 / 32) {
            captured += 512;
            g.observe(0, captured, t0 + Duration::from_millis(i * 32));
        }
        g.pause(); // the human pressed pause
        let t1 = t0 + Duration::from_secs(600); // …and came back 10 minutes later
        for i in 0..(20_000 / 32) {
            captured += 512;
            g.observe(0, captured, t1 + Duration::from_millis(i * 32));
        }
        assert!(!g.warned, "the pause was taken for a loss of recording");
    }
}


/// The PCM queue depth watchdog.
///
/// We no longer drop audio — the channel is unbounded. But a growing queue is still trouble:
/// it means this thread is not keeping up, the recording is falling behind real time, and
/// memory is leaking. Previously the recording in this case was simply LOST, and there was
/// NOTHING to learn about it from. We must be silent neither about the loss nor about its cause.
#[derive(Default)]
struct Backlog {
    warned_at: Option<Instant>,
    worst: usize,
}

impl Backlog {
    /// A PCM piece is 32 ms, so the queue depth is directly the seconds we are behind.
    const CHUNK_MS: usize = 32;
    /// The threshold: half a second behind is already suspicious; beyond that it grows like an
    /// avalanche.
    const WARN_CHUNKS: usize = 16;
    /// No more than once every 15 seconds: the watchdog must not become the noise itself.
    const EVERY: Duration = Duration::from_secs(15);

    fn check(&mut self, depth: usize, now: Instant, log_tx: &Option<CrossbeamSender<UiMsg>>) {
        if depth < Self::WARN_CHUNKS {
            return;
        }
        self.worst = self.worst.max(depth);
        if self.warned_at.is_some_and(|t| now - t < Self::EVERY) {
            return;
        }
        self.warned_at = Some(now);
        let behind_sec = (depth * Self::CHUNK_MS) as f64 / 1000.0;
        // The log speaks to the developer, the status line speaks to the owner of the
        // recording — one string cannot serve both, and they are not even in the same
        // language. Nothing is lost, either: audio is NOT dropped (the channel is
        // unbounded), it piles up in memory. This is a delay, not a loss — and it must not
        // sound like one.
        tracing::warn!(
            "processing is falling behind capture: {depth} pieces queued (~{behind_sec:.1} s). \
             The audio is safe (it piles up in memory), but the recording is lagging"
        );
        if let Some(tx) = log_tx {
            let _ = tx.send(UiMsg::Status(format!(
                "обработка отстаёт: в очереди {depth} кусков (~{behind_sec:.1} с). \
                 Аудио цело — копится в памяти"
            )));
        }
    }
}

/// Main pipeline loop: PCM from the audio threads → VAD segments (RAM/disk) → the ASR pool.
#[allow(clippy::too_many_arguments)]

pub fn run(
    cfg: PipelineConfig,
    session_rx: Option<Receiver<SessionSignal>>,
    pcm_rx: Receiver<PcmChunk>,
    seg_tx: CrossbeamSender<SegmentReady>,
    pending: Arc<AtomicUsize>,
    running: Arc<AtomicBool>,
    record_pcm: Arc<AtomicBool>,
    log_tx: Option<CrossbeamSender<UiMsg>>,
) {
    let silence_frames = (16000.0 * cfg.vad_silence_sec / FRAME_SAMPLES as f64).ceil() as u32;

    let mut sources: [Option<SourceState>; 2] = [None, None];
    let mut chunk_lane = ChunkLane::new(&cfg);
    let mut mic_health =
        (cfg.mic_silence_warn_sec > 0.0).then(|| MicHealth::new(cfg.mic_silence_warn_sec));
    // Three minutes of a dead system track DURING A CALL. A call always has pauses; three minutes
    // of them in a row is not a pause any more.
    let mut loopback_health = LoopbackHealth::new(180.0);
    let mut backlog = Backlog::default();

    let timeout = std::time::Duration::from_millis(200);
    let mut prev_recording = true;

    while running.load(Ordering::Relaxed) {
        // Detection signals (a call started / ended) — they mark a RUNNING recording and start
        // nothing on their own. While paused we DO NOT drain them: let them pile up in the
        // channel and be applied in order on resume, otherwise CallStarted/Ended get lost and
        // in_meeting goes out of sync with the meta.
        if let Some(rx) = &session_rx {
            if record_pcm.load(Ordering::Relaxed) {
                while let Ok(sig) = rx.try_recv() {
                    chunk_lane.on_signal(sig);
                }
            }
        }
        // The silence auto-stop is checked on the loop, not on the audio frame: a recording of
        // an empty room delivers frames just fine — that is exactly the case we must close.
        chunk_lane.maybe_autostop(Instant::now());

        // THE QUEUE WATCHDOG. We no longer drop audio (the channel is unbounded), but we must
        // not stay silent about a stall either: a growing queue means this thread is not
        // keeping up, and the recording is falling behind real time. Previously it was simply
        // LOST in this case, and there was nothing to learn about it from.
        //
        // A piece = 32 ms, so the queue depth is directly the seconds we are behind.
        backlog.check(pcm_rx.len(), Instant::now(), &log_tx);

        match pcm_rx.recv_timeout(timeout) {
            Ok(c) => {
                let recovered = if c.source_id == 0 {
                    mic_health
                        .as_mut()
                        .and_then(|h| h.on_chunk(&c.samples, Instant::now()))
                } else {
                    // The system track gets a level watchdog of its own now. It used to have
                    // none — and that is how half of a 30-minute call went missing unnoticed.
                    loopback_health.on_chunk(&c.samples, Instant::now())
                };
                if let (Some(msg), Some(tx)) = (recovered, log_tx.as_ref()) {
                    let _ = tx.send(UiMsg::Status(msg));
                }
                sync_pause_discard(
                    &mut sources,
                    &mut chunk_lane,
                    &record_pcm,
                    &mut prev_recording,
                );
                if record_pcm.load(Ordering::Relaxed) {
                    feed_pcm_chunk(
                        &mut sources,
                        &mut chunk_lane,
                        c,
                        silence_frames,
                        &cfg,
                        &seg_tx,
                        &pending,
                        &log_tx,
                    );
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                sync_pause_discard(
                    &mut sources,
                    &mut chunk_lane,
                    &record_pcm,
                    &mut prev_recording,
                );
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
        if record_pcm.load(Ordering::Relaxed) {
            let now = Instant::now();
            let warnings = [
                mic_health.as_mut().and_then(|h| h.check(now)),
                // The system track is judged ONLY against a running call: without one, silence
                // there is the normal state of a quiet machine and proves nothing.
                loopback_health.check(now, chunk_lane.in_meeting),
            ];
            for warn in warnings.into_iter().flatten() {
                tracing::warn!("{warn}");
                if let Some(ref tx) = log_tx {
                    let _ = tx.send(UiMsg::Status(warn));
                }
            }
        }
    }

    // The remainder of the queue after the stop. After this `pcm_rx` is dropped on exit — a
    // send in the cpal callbacks will get Disconnected and will not wait for the capture thread
    // to release `drop(stream)` (on Windows that is often tens of seconds). There used to be a
    // second endless loop here until Disconnected → a hang after Ctrl+C.
    while let Ok(c) = pcm_rx.try_recv() {
        sync_pause_discard(
            &mut sources,
            &mut chunk_lane,
            &record_pcm,
            &mut prev_recording,
        );
        if record_pcm.load(Ordering::Relaxed) {
            feed_pcm_chunk(
                &mut sources,
                &mut chunk_lane,
                c,
                silence_frames,
                &cfg,
                &seg_tx,
                &pending,
                &log_tx,
            );
        }
    }

    // We close the chunks cleanly before exiting — they are the primary artifact.
    chunk_lane.finalize_all();

    for src in sources.iter_mut().flatten() {
        let sid = src.source_id;
        if let Some((seg_id, payload, duration_sec, write_sec)) = src.flush() {
            send_segment(
                &seg_tx,
                &pending,
                &log_tx,
                SegmentReady {
                    seg_id,
                    payload,
                    source_id: sid,
                    duration_sec,
                    from_recovery: false,
                },
                write_sec,
                " (flush)",
            );
        }
    }
    debug!("Pipeline stopped, flushed remaining segments");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(dir: &Path, to_disk: bool) -> PipelineConfig {
        PipelineConfig {
            max_chunk_sec: 0.1, // 100 ms — the segments close quickly
            min_chunk_sec: 0.0,
            vad_silence_sec: 0.02,
            work_dir: dir.to_path_buf(),
            initial_seg_seq: [0, 0],
            segments_to_disk: to_disk,
            mic_silence_warn_sec: 0.0,
            session_chunks: false,
            chunk_sec: 3600.0, // do not rotate by length in tests
            chunk_flac: false,
            ffmpeg: PathBuf::from("ffmpeg"),
            preroll_sec: 0.0,
            autostop_sec: 0.0,
        }
    }

    #[test]
    fn ram_mode_emits_payload_without_files() {
        let dir = tempfile::tempdir().unwrap();
        let c = cfg(dir.path(), false);
        let mut st = SourceState::new(0, 1, 0, false);
        let mut got = Vec::new();
        for _ in 0..20 {
            got.extend(st.feed(&vec![0i16; FRAME_SAMPLES], &c));
        }
        assert!(!got.is_empty());
        let (seg_id, payload, dur, _) = &got[0];
        assert!(seg_id.starts_with("src0_"), "{seg_id}");
        assert!(*dur > 0.0);
        match payload {
            SegmentPayload::Ram(buf) => assert!(!buf.is_empty()),
            SegmentPayload::Disk(_) => panic!("in RAM mode there must be no files"),
        }
        // and the disk is empty
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn disk_mode_still_writes_wav_files() {
        let dir = tempfile::tempdir().unwrap();
        let c = cfg(dir.path(), true);
        let mut st = SourceState::new(1, 1, 0, true);
        let mut got = Vec::new();
        for _ in 0..20 {
            got.extend(st.feed(&vec![7i16; FRAME_SAMPLES], &c));
        }
        assert!(!got.is_empty());
        match &got[0].1 {
            SegmentPayload::Disk(p) => {
                assert!(p.exists());
                assert_eq!(p.extension().unwrap(), "wav");
            }
            SegmentPayload::Ram(_) => panic!("in disk mode files were expected"),
        }
    }

    #[test]
    fn discard_drops_ram_segment_silently() {
        let dir = tempfile::tempdir().unwrap();
        let c = cfg(dir.path(), false);
        let mut st = SourceState::new(0, 1000, 0, false);
        let _ = st.feed(&vec![0i16; FRAME_SAMPLES], &c);
        st.discard_open_segment();
        assert!(st.flush().is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    // ─── The system-audio watchdog (WP-C68) ───

    /// Loud enough to count as real speech; the level check uses ~0.003.
    fn loud() -> Vec<i16> {
        vec![5000i16; 1600]
    }

    /// WHAT THE CONVERTER ACTUALLY GAVE US for 27 minutes: live packets of ±1 dither. This is NOT
    /// our silence padding — the endpoint was alive, nothing was playing into it. The distinction
    /// is the whole reason the incident went unnoticed, so the test uses the real thing.
    fn dither() -> Vec<i16> {
        (0..1600).map(|i| if i % 2 == 0 { 1 } else { -1 }).collect()
    }

    /// THE INCIDENT, 16.07.2026. A call is being recorded, the owner is talking, and the system
    /// track has been dead for minutes because the call moved to another browser and renders into
    /// an endpoint we are not listening to. Half the conversation is being lost, and before this
    /// watchdog NOTHING said a word — he found out two days later, by ear.
    #[test]
    fn a_call_with_a_dead_system_track_is_reported() {
        let t0 = Instant::now();
        let mut h = LoopbackHealth::new(180.0);

        // The other side is audible at first — as it was for the first two minutes.
        h.on_chunk(&loud(), t0);
        assert!(h.check(t0, true).is_none(), "a live call must not be reported");

        // Then it dies: live packets keep coming, but there is no sound in them.
        for m in 1..=2 {
            h.on_chunk(&dither(), t0 + Duration::from_secs(60 * m));
            assert!(
                h.check(t0 + Duration::from_secs(60 * m), true).is_none(),
                "reported after {m} min — too early, a call has pauses"
            );
        }

        let warn = h
            .check(t0 + Duration::from_secs(200), true)
            .expect("the system track has been dead for over three minutes DURING A CALL — silence");
        assert!(warn.contains("звонок"), "{warn}");
        assert!(warn.contains("системный звук молчит"), "{warn}");
        // Said once, not on every frame: a warning that repeats stops being read.
        assert!(h.check(t0 + Duration::from_secs(400), true).is_none());
    }

    /// AND IT MUST NOT CRY WOLF. A voice note dictated alone: the microphone is loud, the system
    /// is silent — and that is exactly right, there is no call and nothing is playing. This is the
    /// case for which `MicHealth` was gated to source 0 in the first place, and the reasoning was
    /// sound; only the conclusion («silence is ALWAYS normal») was wrong.
    #[test]
    fn silence_without_a_call_is_never_reported() {
        let t0 = Instant::now();
        let mut h = LoopbackHealth::new(180.0);
        for m in 0..30 {
            let now = t0 + Duration::from_secs(60 * m);
            h.on_chunk(&dither(), now);
            assert!(
                h.check(now, false).is_none(),
                "reported silence with no call — a false alarm teaches people to ignore alarms"
            );
        }
    }

    /// The wrong endpoint FROM THE VERY FIRST SECOND: the call never sounded at all, so there is
    /// no "last time we heard it" to count from. The clock starts at the call.
    #[test]
    fn a_call_that_never_sounded_is_reported_too() {
        let t0 = Instant::now();
        let mut h = LoopbackHealth::new(180.0);
        h.check(t0, true); // the call starts — nothing has ever been heard
        h.on_chunk(&dither(), t0 + Duration::from_secs(60));
        assert!(h.check(t0 + Duration::from_secs(60), true).is_none());
        assert!(
            h.check(t0 + Duration::from_secs(200), true).is_some(),
            "a call that was mute from the start went unreported"
        );
    }

    /// The sound comes back — say so. A warning that never lifts is indistinguishable from a
    /// broken one, and the person stops believing it.
    #[test]
    fn the_system_track_coming_back_is_announced() {
        let t0 = Instant::now();
        let mut h = LoopbackHealth::new(180.0);
        h.check(t0, true);
        h.on_chunk(&dither(), t0 + Duration::from_secs(10));
        assert!(h.check(t0 + Duration::from_secs(200), true).is_some());

        let back = h
            .on_chunk(&loud(), t0 + Duration::from_secs(240))
            .expect("the return of the sound was not announced");
        assert!(back.contains("снова"), "{back}");
        // And it can report again if it dies a second time.
        for m in 5..=9 {
            h.on_chunk(&dither(), t0 + Duration::from_secs(60 * m));
        }
        assert!(h.check(t0 + Duration::from_secs(600), true).is_some());
    }

    // ─── Recording on command, with a pre-roll ring (WP-C60) ───

    fn sess_cfg(dir: &Path, preroll_sec: f64, autostop_sec: f64) -> PipelineConfig {
        let mut c = cfg(dir, false);
        c.work_dir = dir.to_path_buf();
        c.session_chunks = true;
        c.preroll_sec = preroll_sec;
        c.autostop_sec = autostop_sec;
        c
    }

    fn session_dirs(dir: &Path) -> Vec<String> {
        let sessions = dir.join("sessions");
        if !sessions.exists() {
            return Vec::new();
        }
        let mut v: Vec<String> = std::fs::read_dir(sessions)
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    fn read_meta(session_dir: &Path) -> SessionMeta {
        let s = std::fs::read_to_string(session_dir.join("meta.json")).unwrap();
        serde_json::from_str(&s).unwrap()
    }

    fn only_meta(dir: &Path) -> SessionMeta {
        let dirs = session_dirs(dir);
        assert_eq!(dirs.len(), 1, "expected exactly one session: {dirs:?}");
        read_meta(&dir.join("sessions").join(&dirs[0]))
    }

    fn recorded_sec(meta: &SessionMeta, source_id: u8) -> f64 {
        meta.chunks
            .iter()
            .filter(|c| c.source_id == source_id)
            .map(|c| c.duration_sec)
            .sum()
    }

    /// THE POINT OF THE WHOLE MODEL. Until a human says "record", the sound does not reach the
    /// disk — not one directory, not one byte. A recorder running by itself through an office
    /// writes people who never agreed to be written.
    #[test]
    fn nothing_reaches_the_disk_until_the_human_starts() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 60.0, 0.0);
        let mut lane = ChunkLane::new(&c);
        for _ in 0..20 {
            lane.feed(0, &vec![5000i16; 16_000]); // 20 s of loud speech
            lane.feed(1, &vec![5000i16; 16_000]);
        }
        lane.finalize_all();
        assert!(
            session_dirs(dir.path()).is_empty(),
            "audio was written without a command: {:?}",
            session_dirs(dir.path())
        );
        assert!(!lane.armed);
    }

    /// The button is late — the conversation is not. The ring holds the last minutes, and they
    /// enter the session on the press.
    #[test]
    fn start_seeds_the_recording_with_the_preroll_ring() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 1.0, 0.0); // the ring holds the last 1 s
        let mut lane = ChunkLane::new(&c);
        lane.feed(0, &vec![5000i16; 16_000]); // 1 s before the button
        lane.feed(0, &vec![5000i16; 16_000]); // and another one (the ring keeps the last)
        lane.start("Планёрка".into());
        lane.feed(0, &vec![5000i16; 8_000]); // 0.5 s after the button
        lane.finalize_all();

        let dirs = session_dirs(dir.path());
        assert_eq!(dirs.len(), 1, "{dirs:?}");
        assert!(dirs[0].contains("planerka"), "the title is not in the name: {dirs:?}");
        let meta = read_meta(&dir.path().join("sessions").join(&dirs[0]));
        // pre-roll (1 s) + live (0.5 s) ≈ 1.5 s
        let src0 = recorded_sec(&meta, 0);
        assert!(src0 >= 1.4, "the pre-roll was not seeded: {src0:.2} s");
        assert_eq!(meta.title.as_deref(), Some("Планёрка"));
    }

    /// The recording began BEFORE the button, so `started_at` must be moved back by exactly as
    /// much audio as we seeded. Otherwise the archive's clock lies: the file opens with a
    /// conversation that, by its own timestamps, had not started yet.
    #[test]
    fn started_at_is_shifted_back_by_the_seeded_preroll() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 2.0, 0.0);
        let mut lane = ChunkLane::new(&c);
        lane.feed(0, &vec![5000i16; 32_000]); // 2 s into the ring
        let pressed = chrono::Local::now();
        lane.start(String::new());
        lane.finalize_all();

        let meta = only_meta(dir.path());
        let began = chrono::DateTime::parse_from_rfc3339(&meta.started_at).unwrap();
        let back = (pressed - began.with_timezone(&chrono::Local)).num_milliseconds();
        assert!(
            (1500..=2500).contains(&back),
            "started_at is not shifted by the pre-roll: {back} ms back"
        );
    }

    /// A second press is not a second session: splitting the conversation in half at the moment
    /// the human doubted the button worked is the worst possible answer.
    #[test]
    fn pressing_start_twice_does_not_split_the_recording() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 0.0, 0.0);
        let mut lane = ChunkLane::new(&c);
        lane.start("Раз".into());
        lane.feed(0, &vec![5000i16; 1600]);
        lane.start("Два".into());
        lane.feed(0, &vec![5000i16; 1600]);
        lane.finalize_all();
        assert_eq!(session_dirs(dir.path()).len(), 1);
    }

    /// Silence on both tracks closes the recording — with a reason in the meta, so it never
    /// looks like a crash.
    #[test]
    fn silence_auto_stops_the_recording() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 0.0, 0.05); // stop after 50 ms of silence
        let mut lane = ChunkLane::new(&c);
        lane.start(String::new());
        lane.feed(0, &vec![5000i16; 1600]); // speech → the timer is alive
        std::thread::sleep(std::time::Duration::from_millis(70));
        lane.maybe_autostop(Instant::now());

        assert!(!lane.armed, "the recording was not stopped by silence");
        let meta = only_meta(dir.path());
        assert!(meta.stopped_at.is_some());
        assert!(
            meta.stopped_reason.as_deref().unwrap_or("").contains("тишина"),
            "the reason for the stop is not in the meta: {:?}",
            meta.stopped_reason
        );
        // and after the stop the sound does not reach the disk again
        let before = recorded_sec(&meta, 0);
        lane.feed(0, &vec![5000i16; 16_000]);
        lane.finalize_all();
        assert_eq!(session_dirs(dir.path()).len(), 1, "a new session opened by itself");
        assert!((recorded_sec(&only_meta(dir.path()), 0) - before).abs() < 1e-6);
    }

    /// Detection only MARKS a running recording. It starts nothing: it cannot tell a work call
    /// from a private one, and it cannot ask anyone's consent.
    #[test]
    fn call_detection_never_starts_a_recording_by_itself() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 0.0, 0.0);
        let mut lane = ChunkLane::new(&c);
        lane.on_signal(SessionSignal::CallStarted {
            apps: vec!["zoom.exe".into()],
            title: "Standup".into(),
        });
        lane.feed(0, &vec![5000i16; 16_000]);
        lane.finalize_all();
        assert!(session_dirs(dir.path()).is_empty(), "detection started a recording");
    }

    #[test]
    fn call_detection_marks_a_running_recording() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 0.0, 0.0);
        let mut lane = ChunkLane::new(&c);
        lane.start(String::new());
        lane.on_signal(SessionSignal::CallStarted {
            apps: vec!["teams.exe".into()],
            title: "Планёрка".into(),
        });
        lane.feed(0, &vec![5000i16; 1600]);
        lane.on_signal(SessionSignal::CallEnded);
        lane.finalize_all();

        let meta = only_meta(dir.path());
        assert_eq!(meta.meetings.len(), 1);
        assert_eq!(meta.meetings[0].apps, vec!["teams.exe"]);
        assert!(meta.meetings[0].ended_at.is_some(), "the call was not closed");
        // one recording, not two: the call did not rotate the session
        assert_eq!(session_dirs(dir.path()).len(), 1);
    }

    #[test]
    fn orphan_samples_survive_transient_session_failure() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 0.0, 0.0);
        let mut lane = ChunkLane::new(&c);
        // put a file where the sessions directory should be — create_session_dir will fail
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        lane.settings.as_mut().unwrap().work_dir = blocker.clone();
        lane.start(String::new());
        assert!(lane.current.is_none(), "the session must not have been created");

        lane.feed(0, &vec![7i16; 1600]);
        assert_eq!(lane.orphan[0].len(), 1600, "the samples were not buffered");
        // "repair" the path → the next frame creates a session and writes the orphan out
        lane.settings.as_mut().unwrap().work_dir = dir.path().to_path_buf();
        lane.feed(0, &vec![7i16; 800]);
        lane.finalize_all();
        assert!(lane.orphan[0].is_empty(), "the orphan was not written out");

        let meta = only_meta(dir.path());
        let dur = recorded_sec(&meta, 0);
        assert!(
            dur >= 2400.0 / 16_000.0 - 1e-6,
            "the orphaned samples are lost: {dur}"
        );
    }

    /// The ring is BOUNDED. It runs for months without anyone touching it, and an unbounded
    /// buffer of raw PCM eats the machine in an afternoon.
    #[test]
    fn the_preroll_ring_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 0.5, 0.0); // 0.5 s = 8000 samples
        let mut lane = ChunkLane::new(&c);
        for _ in 0..50 {
            lane.feed(0, &vec![1i16; 16_000]); // 50 s of audio through a 0.5 s ring
        }
        assert_eq!(lane.preroll[0].len(), 8_000, "the ring grew past its bound");
        assert!(session_dirs(dir.path()).is_empty());
    }
}
