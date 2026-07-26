//! **localvox-light** core: capture → VAD → WAV → Vosk → `transcript.jsonl`.
//!
//! The `localvox-light` binary and other clients link this crate
//! and call [`run_engine`] with a [`UiMsg`] channel, or with no UI at all.
//!
//! The TUI lives in the `localvox-light-tui` crate.

pub mod asks;
pub mod asr;
pub mod audio;
#[cfg(windows)]
pub mod autostart;
pub mod chunks;
pub mod citations;
pub mod cli;
#[cfg(feature = "onnx")]
pub mod cook;
#[cfg(windows)]
pub mod detect;
pub mod diarize;
pub mod doctor;
pub mod engine;
pub mod env_file;
pub mod events;
pub mod export;
pub mod ingest;
pub mod jobs;
pub mod lang;
pub mod lexicon;
pub mod links;
pub mod light_config;
/// Entity extraction WITHOUT a generative model (GLiNER via ONNX).
/// Behind the `onnx` feature: the model is optional — without it names are simply not checked.
#[cfg(feature = "onnx")]
pub mod ner;
/// Shared ONNX Runtime setup (log level) — one per process.
#[cfg(feature = "onnx")]
pub mod onnx;
pub mod num2words;
pub mod pipeline;
pub mod processing;
pub mod progress;
pub mod provenance;
pub mod readable;
pub mod session;
pub mod settings;
pub mod transcript;
pub mod versions;
pub mod voice_note;

pub use cli::{
    init_tracing, join_engine_thread, merge_env_bools, normalized_model_path, print_devices,
    resolve_audio_from_cli_and_file, validate_vosk_model, validate_vosk_model_dir, Cli,
};
pub use engine::run_engine;
pub use events::UiMsg;
pub use light_config::LightDeviceConfig;
