//! Legacy standalone import policy: new directory, spinner feedback and local files.
//! Queued import deliberately retains separate receipt/retry/title guarantees (BL-006).
use anyhow::{Context, Result};
use localvox_light_core::chunks::SessionMeta;
use localvox_light_ingest::{
    load_settings_named,
    progress::{convert_to_pcm_with_progress, download_audio_with_progress},
    MediaTools,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub fn ingest_new_session(input: &str, work_dir: &Path) -> Result<PathBuf> {
    let settings = load_settings_named(&["localvox-asr-settings.json", "settings.json"]);
    let tools = MediaTools::resolve(&settings);
    let ffmpeg = &tools.ffmpeg;

    let is_url = input.starts_with("http://") || input.starts_with("https://");
    let (pcm_bytes, label) = if is_url {
        let temp = download_audio_with_progress(
            false,
            &tools.yt_dlp,
            input,
            tools.ffmpeg_location.as_deref(),
            tools.js_runtime.as_deref(),
            false,
        )
        .with_context(|| format!("downloading {input}"))?;
        let pcm =
            convert_to_pcm_with_progress(false, &ffmpeg, &temp, false).context("ffmpeg → pcm")?;
        let _ = std::fs::remove_file(&temp);
        let label = temp
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "youtube".into());
        (pcm, label)
    } else {
        let path = PathBuf::from(input);
        anyhow::ensure!(path.is_file(), "neither a file nor a URL: {input}");
        let pcm =
            convert_to_pcm_with_progress(false, &ffmpeg, &path, false).context("ffmpeg → pcm")?;
        let label = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        (pcm, label)
    };

    let samples = super::pcm_samples(&pcm_bytes);
    anyhow::ensure!(!samples.is_empty(), "empty audio after conversion");

    let (audio_dir, meta_path) =
        localvox_light_core::chunks::create_session_dir(work_dir, Some(&label))?;
    let session_dir = meta_path.parent().unwrap_or(&audio_dir).to_path_buf();
    let meta = Arc::new(Mutex::new(SessionMeta {
        started_at: localvox_light_core::versions::now_rfc3339(),
        sample_rate: 16_000,
        chunks: Vec::new(),
        ..Default::default()
    }));
    let mut rec = super::recorder(audio_dir, meta_path.clone(), ffmpeg, meta.clone());
    for block in samples.chunks(64 * 1024) {
        rec.feed(block);
    }
    rec.finalize_current();
    // The directory could have been swept away (a neighbouring daemon's sweep of empty sessions
    // in the moment between creation and the first chunk) — the loss must be loud.
    anyhow::ensure!(
        !meta.lock().unwrap().chunks.is_empty(),
        "not a single audio chunk was written (did the session directory vanish?) — repeat the ingest"
    );
    Ok(session_dir)
}
