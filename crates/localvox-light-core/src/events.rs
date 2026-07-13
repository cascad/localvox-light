//! Structured lines for TUI + messages sent into the UI channel.

/// Hook on a fast-lane transcript line: `(source_id, text)` after a successful
/// append to the jsonl. The consumer is the voice module (F2): «запиши…» commands.
/// Called from the ASR workers — the implementation MUST be non-blocking
/// (send into its own channel); heavy work belongs in its own thread.
pub type TranscriptHook = std::sync::Arc<dyn Fn(u8, &str) + Send + Sync>;

#[derive(Clone)]
pub struct StructuredLog {
    pub stage: String,
    pub source_id: u8,
    /// Duration of the audio piece (sec); its meaning depends on the stage.
    pub chunk_sec: f64,
    /// Processing time of the stage (sec).
    pub proc_sec: f64,
    pub detail: String,
    /// Show in the TUI "Debug" panel only with `--verbose` (errors go with `false`).
    pub verbose_only: bool,
}

pub enum UiMsg {
    Transcript {
        source_id: u8,
        text: String,
        /// If None — the TUI substitutes the current local time (as before).
        time: Option<String>,
    },
    /// Lines already written to transcript.jsonl at startup, so that the TUI matches the file.
    TranscriptHistory(Vec<(String, u8, String)>),
    /// transcript.jsonl has been truncated; the TUI drops its buffer.
    ClearTranscript,
    Log(StructuredLog),
    Status(String),
    /// Counters for the TUI: wavs with no line in the jsonl, the sum of their bytes,
    /// the bytes of all files in the workspace directory.
    QueuePending {
        unprocessed_wavs: usize,
        unprocessed_mb: f64,
        workspace_total_mb: f64,
    },
    /// Workspace directory (WAV + transcript.jsonl) and export directory (hotkey `e` → sorted dump).
    WorkspacePaths {
        workspace_dir: std::path::PathBuf,
        dump_dir: std::path::PathBuf,
    },
    /// Input level for the TUI meter (0..=1), as in client-reliable.
    AudioLevel {
        source_id: u8,
        level: f32,
    },
    /// The engine stopped because of an error; the TUI stays on screen until q.
    EngineFatal {
        message: String,
    },
}
