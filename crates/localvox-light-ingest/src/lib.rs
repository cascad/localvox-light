//! Источники медиа для localvox-light: yt-dlp + ffmpeg → PCM s16le 16 kHz mono.
//!
//! Слой полностью ASR-агностичен: возвращает f32-сэмплы (или путь к временному файлу),
//! а как именно их распознавать — решает крейт-бинарник.
//!
//! Состав:
//! * [`tools`] — настройки и резолверы путей к `yt-dlp` / `ffmpeg` / JS-runtime.
//! * [`download`] — низкоуровневые вызовы `yt-dlp` и `ffmpeg`.
//! * [`progress`] — обёртки с indicatif-спиннерами поверх [`download`] + универсальный
//!   [`progress::with_spinner`] для произвольной долгой работы (загрузка ASR-моделей).
//! * [`output`] — выбор путей результата для одного / нескольких источников.

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

fn run() {
    let a = "say my name";

}