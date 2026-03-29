//! Structured lines for TUI + сообщения в UI-канал.

#[derive(Clone)]
pub struct StructuredLog {
    pub stage: String,
    pub source_id: u8,
    /// Длительность аудио-куска (сек), смысл зависит от этапа.
    pub chunk_sec: f64,
    /// Время обработки этапа (сек).
    pub proc_sec: f64,
    pub detail: String,
    /// В TUI панели «Debug» показывать только при `--verbose` (ошибки — с `false`).
    pub verbose_only: bool,
}

pub enum UiMsg {
    Transcript {
        source_id: u8,
        text: String,
        /// Если None — в TUI подставляется текущее локальное время (как раньше).
        time: Option<String>,
    },
    /// Уже записанные в transcript.jsonl строки при старте, чтобы TUI совпадал с файлом.
    TranscriptHistory(Vec<(String, u8, String)>),
    /// Файл transcript.jsonl обнулён; TUI сбрасывает буфер.
    ClearTranscript,
    Log(StructuredLog),
    Status(String),
    /// Счётчики для TUI: wav без строки в jsonl, сумма их байт, байты всех файлов в рабочем каталоге.
    QueuePending {
        unprocessed_wavs: usize,
        unprocessed_mb: f64,
        workspace_total_mb: f64,
    },
    /// Рабочий каталог (WAV + transcript.jsonl) и каталог экспорта (хоткей `e` → sorted dump).
    WorkspacePaths {
        workspace_dir: std::path::PathBuf,
        dump_dir: std::path::PathBuf,
    },
    /// Уровень входа для полоски в TUI (0..=1), как в client-reliable.
    AudioLevel { source_id: u8, level: f32 },
    /// Движок остановлен из‑за ошибки; TUI остаётся на экране до q.
    EngineFatal { message: String },
}
