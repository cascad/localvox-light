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
    /// Ambient sessionization (WP-C6): seconds "before" the call is detected to pull into
    /// the meeting session (pre-roll). 0 — no pre-roll.
    pub preroll_sec: f64,
    /// Silence longer than this cuts the ambient session into a new one (0 — never cut).
    pub ambient_gap_sec: f64,
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

/// The session-change signal (F3, ambient sessionization): call detection tells the pipeline
/// about the start/end of a meeting. The pipeline is the sole owner of the session lifecycle
/// and the sole writer of `meta.json` (detection no longer writes the meta itself — there are
/// no two writers of one file).
pub enum SessionSignal {
    CallStarted { apps: Vec<String>, title: String },
    CallEnded,
}

/// Settings for creating NEW sessions on the fly (rotation on a call / silence / a new day).
#[derive(Clone)]
struct SessionSettings {
    work_dir: PathBuf,
    chunk_sec: f64,
    flac: bool,
    ffmpeg: PathBuf,
    /// How many seconds "before" the moment of detection to pull into the meeting session.
    preroll_sec: f64,
    /// Silence longer than this cuts the ambient session (0 — never cut).
    gap_sec: f64,
}

struct Session {
    params: Arc<ChunkParams>,
    meta: Arc<Mutex<SessionMeta>>,
}

/// Recording of continuous session chunks (F8) + ambient sessionization (WP-C6):
/// the recorders for both sources, the current session, the pre-roll ring.
/// `settings == None` — chunks are off, every call becomes a no-op.
struct ChunkLane {
    settings: Option<SessionSettings>,
    /// How much audio arrived against real time — per source.
    integrity: [Integrity; 2],
    current: Option<Session>,
    recorders: [Option<ChunkRecorder>; 2],
    /// The last `preroll_cap` samples of each source — they seed the meeting session.
    preroll: [std::collections::VecDeque<i16>; 2],
    preroll_cap: usize,
    /// Is a meeting session running right now (a call is recorded into it separately from
    /// ambient).
    in_meeting: bool,
    /// The moment of the last speech (for the ambient cut on silence); whether anything was
    /// written at all.
    last_voice: Option<Instant>,
    recorded_since_open: bool,
    /// A buffer of samples for the time while there is no "home" (create_session_dir is
    /// temporarily failing) — it will be written into the very first session that gets
    /// created; bounded in length.
    orphan: [Vec<i16>; 2],
    /// The calendar day of the current session — midnight cuts ambient (otherwise a daemon
    /// running for a week in silence-without-calls would pile up one session for all the days).
    day: Option<i32>,
    /// The frame counter until the next "finish the session" check: we poke the file once a
    /// second, not on every audio frame.
    finish_poll: u32,
    /// The human pressed "New meeting": the next session opens with this title and the
    /// "meeting" flag.
    pending_meeting: Option<String>,
}

/// Frames between checks of the "finish the session" marker (a frame is 20 ms).
const FINISH_POLL_FRAMES: u32 = 50;

impl ChunkLane {
    fn new(env: Option<(Arc<ChunkParams>, Arc<Mutex<SessionMeta>>)>, cfg: &PipelineConfig) -> Self {
        // The settings for new sessions are taken from the first (engine-created) session.
        let settings = env.as_ref().map(|(p, _)| SessionSettings {
            work_dir: cfg.work_dir.clone(),
            chunk_sec: p.chunk_sec,
            flac: p.flac,
            ffmpeg: p.ffmpeg.clone(),
            preroll_sec: cfg.preroll_sec,
            gap_sec: cfg.ambient_gap_sec,
        });
        let preroll_cap = settings
            .as_ref()
            .map(|s| (s.preroll_sec * 16_000.0) as usize)
            .unwrap_or(0);
        let current = env.map(|(params, meta)| Session { params, meta });
        let lane = Self {
            settings,
            integrity: Default::default(),
            current,
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
            day: Some(today()),
            finish_poll: 0,
            pending_meeting: None,
        };
        lane.sync_marker(); // the engine started writing into this session
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
        // The pre-roll ring: the last preroll_cap samples of the source.
        if self.preroll_cap > 0 {
            let ring = &mut self.preroll[sid];
            for &s in samples {
                if ring.len() == self.preroll_cap {
                    ring.pop_front();
                }
                ring.push_back(s);
            }
        }
        // Activity for the ambient cut: any loud track (the microphone OR the system sound in
        // loopback — a webinar, a call) keeps the session alive, otherwise a loopback-only
        // recording would be shattered into one session per chunk. True silence on both
        // sources still does not move the timer.
        if crate::audio::pcm_level_i16(samples) > 0.003 {
            self.last_voice = Some(Instant::now());
        }
        // "Finish the session" from the web UI / the tray. We check no more than once a
        // second — it is a single file-existence check, and we are called on every frame.
        self.finish_poll += 1;
        if self.finish_poll >= FINISH_POLL_FRAMES {
            self.finish_poll = 0;
            self.maybe_start_meeting();
            self.maybe_finish_on_request();
        }
        self.ensure_current_ambient();
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

    /// Create an ambient session if there is no current one (after a cut on silence).
    ///
    /// If the human pressed "New meeting", this session is born a MEETING: with their title
    /// in the directory name (`20260713_181500_planerka`) and a flag in the meta.
    fn ensure_current_ambient(&mut self) {
        if self.current.is_some() {
            return;
        }
        let Some(s) = &self.settings else { return };
        let meeting = self.pending_meeting.take();
        let label = meeting.as_deref().filter(|t| !t.is_empty());
        match crate::chunks::create_session_dir(&s.work_dir, label) {
            Ok((audio_dir, meta_path)) => {
                let meta = SessionMeta {
                    started_at: crate::versions::now_rfc3339(),
                    sample_rate: 16_000,
                    chunks: Vec::new(),
                    title: label.map(str::to_string),
                    meeting: meeting.is_some(),
                    ..Default::default()
                };
                // We write the meta IMMEDIATELY, not when the first chunk closes. Otherwise a
                // new session looks dead and empty in the archive for several minutes, and the
                // human does not understand where the sound is being written now (owner's
                // complaint).
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
                // the day of the SESSION, not of the last cut: otherwise a session opened
                // after midnight would immediately be cut "on the change of day"
                self.day = Some(today());
                self.sync_marker();
            }
            Err(e) => tracing::error!("ambient session was not created: {e}"),
        }
    }

    /// Handle a detection signal: a call → a separate session (with pre-roll); the end of the
    /// call → close the meeting and go back to ambient.
    fn on_signal(&mut self, sig: SessionSignal) {
        if self.settings.is_none() {
            return;
        }
        match sig {
            SessionSignal::CallStarted { apps, title } => {
                let label = meeting_label(&apps, &title);
                let mark = crate::chunks::MeetingMark {
                    apps,
                    window_title: title,
                    started_at: crate::versions::now_rfc3339(),
                    ended_at: None,
                };
                self.rotate(Some(&label), true, Some(mark));
                // in_meeting stays in agreement with reality: if the meeting session was not
                // created (rotate → current=None on failure), we do not set the flag.
                self.in_meeting = self.current.is_some();
            }
            SessionSignal::CallEnded => {
                if self.in_meeting {
                    self.close_meeting_mark();
                }
                // The return to ambient is lazy: we do not breed an empty directory if the
                // next call starts right away (back-to-back) or there is nothing to record.
                // The next speech will open ambient through ensure_current_ambient.
                for rec in self.recorders.iter_mut().flatten() {
                    rec.finalize_current();
                }
                self.recorders = [None, None];
                self.current = None;
                self.recorded_since_open = false;
                self.in_meeting = false;
                self.sync_marker();
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

    /// Close the current session and open a new one. `seed_preroll` — seed the new session
    /// with the pre-roll ring (the start of the meeting "before" the moment of detection).
    fn rotate(
        &mut self,
        label: Option<&str>,
        seed_preroll: bool,
        meeting: Option<crate::chunks::MeetingMark>,
    ) {
        let Some(s) = self.settings.clone() else {
            return;
        };
        for rec in self.recorders.iter_mut().flatten() {
            rec.finalize_current();
        }
        self.recorders = [None, None];
        match crate::chunks::create_session_dir(&s.work_dir, label) {
            Ok((audio_dir, meta_path)) => {
                let mut meta = SessionMeta {
                    started_at: crate::versions::now_rfc3339(),
                    sample_rate: 16_000,
                    chunks: Vec::new(),
                    ..Default::default()
                };
                if let Some(mark) = meeting {
                    meta.meetings.push(mark);
                }
                let params = Arc::new(ChunkParams {
                    audio_dir,
                    meta_path: meta_path.clone(),
                    chunk_sec: s.chunk_sec,
                    flac: s.flac,
                    ffmpeg: s.ffmpeg.clone(),
                });
                crate::chunks::save_meta_public(&meta_path, &meta);
                self.current = Some(Session {
                    params,
                    meta: Arc::new(Mutex::new(meta)),
                });
                self.recorded_since_open = false;
                self.day = Some(today());
                if seed_preroll {
                    self.seed_preroll();
                }
                self.sync_marker();
            }
            Err(e) => {
                // We do not keep the old session: a fresh recorder with seq=0 would overwrite
                // its first chunk. None → the next speech will create a new ambient one.
                tracing::error!("session was not created on rotation: {e}");
                self.current = None;
                self.sync_marker();
            }
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

    /// The human pressed "New meeting": we close the current session and open the next one —
    /// with a title and the "this is a meeting" flag.
    ///
    /// **Why a button, and not only auto-detection.** The call detector sees that an
    /// application is holding the microphone — and that honestly works for Zoom, Teams, the
    /// browser. A face-to-face standup at the table it will NEVER see: from the system's point
    /// of view that is the same background noise as the whole day. There is no reliable
    /// automatic sign of a meeting in a room, and pretending there is means lying to the human.
    /// The human's word outweighs any guess here.
    fn maybe_start_meeting(&mut self) {
        let Some(s) = &self.settings else { return };
        let work_dir = s.work_dir.clone();
        let Some(title) = crate::jobs::take_meeting_request(&work_dir) else {
            return;
        };
        // We close the current one — the cook will pick it up; the next one opens as a meeting.
        if let Some(session) = &self.current {
            let mut meta = session.meta.lock().unwrap_or_else(|e| e.into_inner());
            meta.stopped_at = Some(crate::versions::now_rfc3339());
            meta.stopped_reason = Some("начата встреча".into());
            crate::chunks::save_meta_public(&session.params.meta_path, &meta);
        }
        for rec in self.recorders.iter_mut().flatten() {
            rec.finalize_current();
        }
        self.recorders = [None, None];
        self.current = None;
        self.recorded_since_open = false;
        self.last_voice = None;
        self.pending_meeting = Some(title.clone());
        self.sync_marker();
        // We open the meeting IMMEDIATELY, without waiting for the next frame: the human
        // pressed the button and must see the recording in the archive right away, not
        // "some day".
        self.ensure_current_ambient();
        tracing::info!(
            "meeting started: {}",
            if title.is_empty() { "(untitled)" } else { &title }
        );
    }

    /// The human pressed "Finish the session": we close the current one and record in
    /// `meta.json` that they did it and when.
    ///
    /// Why: while a session is open the cook does not touch it — and to get a summary one had
    /// to KILL THE DAEMON. Now there is a button, and a trace stays in the meta: the recording
    /// was cut short by a human, not by a failure.
    fn maybe_finish_on_request(&mut self) {
        let Some(s) = &self.settings else { return };
        let work_dir = s.work_dir.clone();
        let Some(reason) = crate::jobs::take_finish_request(&work_dir) else {
            return;
        };
        if self.current.is_none() {
            tracing::info!("\"finish the session\": there is no active session");
            return;
        }
        if let Some(session) = &self.current {
            let mut meta = session.meta.lock().unwrap_or_else(|e| e.into_inner());
            meta.stopped_at = Some(crate::versions::now_rfc3339());
            meta.stopped_reason = Some(reason.clone());
            crate::chunks::save_meta_public(&session.params.meta_path, &meta);
        }
        for rec in self.recorders.iter_mut().flatten() {
            rec.finalize_current();
        }
        self.recorders = [None, None];
        self.current = None;
        self.recorded_since_open = false;
        self.last_voice = None;
        self.in_meeting = false;
        self.sync_marker();
        tracing::info!("session finished on request: {reason}");
    }

    /// The ambient cut: on long silence OR on a change of the calendar day (midnight). We
    /// close the ambient session that has accumulated audio — the next speech will open a
    /// fresh one. During a meeting we do not cut (a call across midnight is one meeting, not
    /// two).
    fn maybe_split_ambient(&mut self, now: Instant) {
        let Some(s) = &self.settings else { return };
        if self.in_meeting || !self.recorded_since_open {
            return;
        }
        // the day: a daemon running for a week must not pile up a single ambient session
        let day_changed = match (self.day, today()) {
            (Some(d), t) => d != t,
            (None, _) => false,
        };
        let silence_split = s.gap_sec > 0.0
            && self
                .last_voice
                .map(|last| now.duration_since(last).as_secs_f64() >= s.gap_sec)
                .unwrap_or(false);
        if !day_changed && !silence_split {
            return;
        }
        let gap = s.gap_sec;
        for rec in self.recorders.iter_mut().flatten() {
            rec.finalize_current();
        }
        self.recorders = [None, None];
        self.current = None; // the next speech will create a fresh ambient session
        self.recorded_since_open = false;
        // Disarm the timer until the next speech, otherwise loopback-only audio
        // (last_voice does not move) would be cut into one session per chunk.
        self.last_voice = None;
        self.day = Some(today());
        self.sync_marker();
        if day_changed {
            tracing::info!("ambient session closed on the change of day");
        } else {
            tracing::info!("ambient session closed on silence ({gap:.0} s)");
        }
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

/// The ordinal number of the calendar day (local zone) — the key of the daily cut.
fn today() -> i32 {
    use chrono::Datelike;
    chrono::Local::now().num_days_from_ce()
}

/// The directory label of a meeting session: the window title, otherwise the list of apps.
fn meeting_label(apps: &[String], title: &str) -> String {
    let base = if !title.trim().is_empty() {
        title
    } else if let Some(first) = apps.first() {
        first
    } else {
        "call"
    };
    format!("call-{base}")
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
    chunk_env: Option<(Arc<ChunkParams>, Arc<Mutex<SessionMeta>>)>,
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
    let mut chunk_lane = ChunkLane::new(chunk_env, &cfg);
    let mut mic_health =
        (cfg.mic_silence_warn_sec > 0.0).then(|| MicHealth::new(cfg.mic_silence_warn_sec));
    let mut backlog = Backlog::default();

    let timeout = std::time::Duration::from_millis(200);
    let mut prev_recording = true;

    while running.load(Ordering::Relaxed) {
        // Sessionization signals (the start/end of a call) — before processing PCM, so that
        // the pre-roll lands in the new session. While paused we DO NOT drain them: let them
        // pile up in the channel and be applied in order on resume, otherwise
        // CallStarted/Ended get lost and in_meeting goes out of sync.
        if let Some(rx) = &session_rx {
            if record_pcm.load(Ordering::Relaxed) {
                while let Ok(sig) = rx.try_recv() {
                    chunk_lane.on_signal(sig);
                }
            }
        }
        chunk_lane.maybe_split_ambient(Instant::now());

        // THE QUEUE WATCHDOG. We no longer drop audio (the channel is unbounded), but we must
        // not stay silent about a stall either: a growing queue means this thread is not
        // keeping up, and the recording is falling behind real time. Previously it was simply
        // LOST in this case, and there was nothing to learn about it from.
        //
        // A piece = 32 ms, so the queue depth is directly the seconds we are behind.
        backlog.check(pcm_rx.len(), Instant::now(), &log_tx);

        match pcm_rx.recv_timeout(timeout) {
            Ok(c) => {
                if c.source_id == 0 {
                    if let Some(h) = mic_health.as_mut() {
                        if let Some(msg) = h.on_chunk(&c.samples, Instant::now()) {
                            if let Some(ref tx) = log_tx {
                                let _ = tx.send(UiMsg::Status(msg));
                            }
                        }
                    }
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
            if let Some(h) = mic_health.as_mut() {
                if let Some(warn) = h.check(Instant::now()) {
                    tracing::warn!("{warn}");
                    if let Some(ref tx) = log_tx {
                        let _ = tx.send(UiMsg::Status(warn));
                    }
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
            preroll_sec: 0.0,
            ambient_gap_sec: 0.0,
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

    // ─── Ambient sessionization (WP-C6) ───

    fn sess_cfg(dir: &Path, preroll_sec: f64, gap_sec: f64) -> PipelineConfig {
        let mut c = cfg(dir, false);
        c.work_dir = dir.to_path_buf();
        c.preroll_sec = preroll_sec;
        c.ambient_gap_sec = gap_sec;
        c
    }

    fn lane(dir: &Path, c: &PipelineConfig) -> ChunkLane {
        let (audio_dir, meta_path) = crate::chunks::create_session_dir(dir, None).unwrap();
        let params = Arc::new(ChunkParams {
            audio_dir,
            meta_path,
            chunk_sec: 3600.0, // do not rotate by length in the test
            flac: false,
            ffmpeg: PathBuf::from("ffmpeg"),
        });
        let meta = Arc::new(Mutex::new(SessionMeta {
            started_at: crate::versions::now_rfc3339(),
            sample_rate: 16_000,
            chunks: Vec::new(),
            ..Default::default()
        }));
        ChunkLane::new(Some((params, meta)), c)
    }

    fn session_dirs(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir.join("sessions"))
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

    #[test]
    fn call_starts_own_session_seeded_with_preroll() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 1.0, 0.0); // 1 c pre-roll
        let mut lane = lane(dir.path(), &c);
        // 2 s of speech in ambient → the pre-roll ring is full (it holds the last 1 s)
        let loud = vec![5000i16; 16_000];
        lane.feed(0, &loud);
        lane.feed(0, &loud);
        // the call: a separate session "call-standup", seeded with the pre-roll
        lane.on_signal(SessionSignal::CallStarted {
            apps: vec!["zoom.exe".into()],
            title: "Standup".into(),
        });
        lane.feed(0, &vec![5000i16; 8_000]); // 0.5 s of live meeting sound
        lane.finalize_all();

        let dirs = session_dirs(dir.path());
        let call_dir = dirs
            .iter()
            .find(|d| d.contains("call-standup"))
            .expect("there is no call session");
        let meta = read_meta(&dir.path().join("sessions").join(call_dir));
        assert_eq!(meta.meetings.len(), 1);
        assert_eq!(meta.meetings[0].apps, vec!["zoom.exe"]);
        // the first chunk of the meeting = pre-roll (1 s) + live (0.5 s) ≈ 1.5 s
        let src0: f64 = meta
            .chunks
            .iter()
            .filter(|c| c.source_id == 0)
            .map(|c| c.duration_sec)
            .sum();
        assert!(src0 >= 1.4, "the pre-roll was not seeded: {src0:.2} s");
    }

    #[test]
    fn call_end_closes_meeting_and_opens_ambient() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 0.0, 0.0);
        let mut lane = lane(dir.path(), &c);
        lane.feed(0, &vec![100i16; 1600]);
        lane.on_signal(SessionSignal::CallStarted {
            apps: vec!["teams.exe".into()],
            title: String::new(),
        });
        lane.feed(0, &vec![100i16; 1600]);
        lane.on_signal(SessionSignal::CallEnded);
        lane.finalize_all();

        let dirs = session_dirs(dir.path());
        // ambient(start) + call; the return to ambient is lazy, so there is no empty one
        assert_eq!(dirs.len(), 2, "{dirs:?}");
        let call_dir = dirs.iter().find(|d| d.contains("call-")).unwrap();
        let meta = read_meta(&dir.path().join("sessions").join(call_dir));
        assert!(meta.meetings[0].ended_at.is_some(), "the meeting was not closed");
    }

    #[test]
    fn back_to_back_calls_leave_no_empty_ambient() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 0.0, 0.0);
        let mut lane = lane(dir.path(), &c);
        lane.feed(0, &vec![100i16; 1600]);
        // one call ended and another started right away (a single detection poll)
        lane.on_signal(SessionSignal::CallEnded);
        lane.on_signal(SessionSignal::CallStarted {
            apps: vec!["discord.exe".into()],
            title: String::new(),
        });
        lane.feed(0, &vec![100i16; 1600]);
        lane.finalize_all();
        let dirs = session_dirs(dir.path());
        // ambient(start) + call-discord; there is no empty ambient between the calls
        assert_eq!(dirs.len(), 2, "{dirs:?}");
        assert!(dirs.iter().any(|d| d.contains("call-discord")));
    }

    #[test]
    fn orphan_samples_survive_transient_session_failure() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 0.0, 0.0);
        let mut lane = lane(dir.path(), &c);
        // simulate "no home": current=None, but the settings are there
        lane.current = None;
        // put a file where the sessions directory should be — create_session_dir will fail
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        // repoint the sessions work_dir to a path under the file (create_dir_all will fail)
        lane.settings.as_mut().unwrap().work_dir = blocker.clone();
        lane.feed(0, &vec![7i16; 1600]);
        assert!(lane.current.is_none(), "the session must not have been created");
        assert_eq!(lane.orphan[0].len(), 1600, "the samples were not buffered");
        // "repair" the path → the next speech creates a session and writes the orphan out
        lane.settings.as_mut().unwrap().work_dir = dir.path().to_path_buf();
        lane.feed(0, &vec![7i16; 800]);
        lane.finalize_all();
        assert!(lane.orphan[0].is_empty(), "the orphan was not written out");
        // in the new session the chunk contains both the orphaned 1600 and the new 800 = 2400 samples
        let new_dir = session_dirs(dir.path()).into_iter().max().unwrap();
        let meta = read_meta(&dir.path().join("sessions").join(&new_dir));
        let dur: f64 = meta
            .chunks
            .iter()
            .filter(|c| c.source_id == 0)
            .map(|c| c.duration_sec)
            .sum();
        assert!(
            dur >= 2400.0 / 16_000.0 - 1e-6,
            "the orphaned samples are lost: {dur}"
        );
    }

    #[test]
    fn ambient_splits_on_long_silence() {
        let dir = tempfile::tempdir().unwrap();
        let c = sess_cfg(dir.path(), 0.0, 0.05); // cut after 50 ms of silence
        let mut lane = lane(dir.path(), &c);
        lane.feed(0, &vec![5000i16; 1600]); // speech → last_voice, recorded_since_open
        std::thread::sleep(std::time::Duration::from_millis(70));
        lane.maybe_split_ambient(Instant::now());
        assert!(lane.current.is_none(), "ambient was not cut on silence");
        // the next speech opens a fresh session
        lane.feed(0, &vec![5000i16; 1600]);
        lane.finalize_all();
        assert_eq!(
            session_dirs(dir.path()).len(),
            2,
            "the new ambient session was not created"
        );
    }
}
