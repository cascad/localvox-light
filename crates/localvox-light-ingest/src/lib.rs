//! Media sources for localvox-light: yt-dlp + ffmpeg → PCM s16le 16 kHz mono.
//!
//! The layer is completely ASR-agnostic: it returns f32 samples (or a path to a
//! temporary file), and how exactly to recognize them is decided by the binary crate.
//!
//! Contents:
//! * [`tools`] — settings and resolvers of the paths to `yt-dlp` / `ffmpeg` / the JS
//!   runtime.
//! * [`download`] — the low-level calls of `yt-dlp` and `ffmpeg`.
//! * [`progress`] — wrappers with indicatif spinners on top of [`download`] + the
//!   universal [`progress::with_spinner`] for any long-running work (loading ASR
//!   models).
//! * [`output`] — the choice of result paths for one / several sources.

pub mod download;
pub mod output;
pub mod progress;
pub mod tools;

pub use download::pcm_s16le_to_f32;
pub use output::{resolve_output_paths, OutputSpec};
pub use progress::with_spinner;
pub use tools::{
    load_settings, load_settings_named, resolve_ffmpeg, resolve_ffmpeg_location_for_ytdlp,
    resolve_js_runtime, resolve_yt_dlp, verify_ffmpeg, verify_js_runtime_path_if_explicit,
    verify_yt_dlp, Settings,
};
