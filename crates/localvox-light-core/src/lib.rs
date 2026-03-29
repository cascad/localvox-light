//! Ядро **localvox-light**: захват → VAD → WAV → Vosk → `transcript.jsonl`.
//!
//! Бинарник `localvox-light` и другие клиенты подключают этот крейт
//! и вызывают [`run_engine`] с каналом [`UiMsg`], либо без UI.
//!
//! TUI вынесен в крейт `localvox-light-tui`.

pub mod asr;
pub mod audio;
pub mod cli;
pub mod engine;
pub mod events;
pub mod light_config;
pub mod pipeline;
pub mod session;
pub mod transcript;

pub use cli::{
    init_tracing, join_engine_thread, merge_env_bools, print_devices,
    resolve_audio_from_cli_and_file, validate_vosk_model, Cli,
};
pub use engine::run_engine;
pub use events::UiMsg;
pub use light_config::LightDeviceConfig;
