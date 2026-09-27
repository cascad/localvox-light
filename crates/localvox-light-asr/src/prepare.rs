//! Preparation of an existing queued source session, executed only inside the worker.
use anyhow::Result;
use std::path::Path;

pub fn ingest_into_session(session: &Path) -> Result<()> {
    use localvox_light_core::chunks::{ChunkParams, ChunkRecorder, SessionMeta};
    use localvox_light_core::progress::{activity, step, Stage};
    use std::sync::{Arc, Mutex};

    let meta_path = session.join("meta.json");
    let mut meta: SessionMeta = serde_json::from_slice(&std::fs::read(&meta_path)?)?;
    let source = meta
        .source
        .clone()
        .ok_or_else(|| anyhow::anyhow!("у сессии нет ссылки: нечего скачивать"))?;
    localvox_light_core::artifacts::begin_preparation(session)?;

    // The tools are checked BEFORE the download: "yt-dlp is not installed" must be said at the
    // start, not after a minute of waiting for a network that was never going to be used.
    let settings = localvox_light_ingest::load_settings();
    let yt_dlp = localvox_light_ingest::resolve_yt_dlp(&settings, None);
    let ffmpeg = localvox_light_ingest::resolve_ffmpeg(&settings, None);
    activity(
        session,
        Stage::Download,
        "Проверка yt-dlp и ffmpeg",
        || -> Result<()> {
            localvox_light_ingest::verify_yt_dlp(&yt_dlp)?;
            localvox_light_ingest::verify_ffmpeg(&ffmpeg)?;
            Ok(())
        },
    )?;
    let ffmpeg_location = localvox_light_ingest::resolve_ffmpeg_location_for_ytdlp(&ffmpeg);
    let js_runtime = localvox_light_ingest::resolve_js_runtime(&settings, None, None);

    // The title, if the source names itself. A session called "youtube" is useless in a list —
    // a person looks for the lecture by its name, not by where it was hosted.
    if let Some(title) = localvox_light_core::progress::activity(
        session,
        Stage::Download,
        "Получение названия источника",
        || source_title(&yt_dlp, &source.url),
    ) {
        meta.title = Some(title.clone());
        meta.source = Some(localvox_light_core::chunks::Source {
            url: source.url.clone(),
            title: Some(title),
        });
        localvox_light_core::chunks::save_meta_public(&meta_path, &meta);
    }

    let file = step(session, Stage::Download, || {
        activity(
            session,
            Stage::Download,
            "Скачивание звуковой дорожки",
            || {
                localvox_light_ingest::download::download_audio(
                    &yt_dlp,
                    &source.url,
                    ffmpeg_location.as_deref(),
                    js_runtime.as_deref(),
                    false,
                )
            },
        )
    })?;

    let outcome = step(session, Stage::Extract, || {
        activity(
            session,
            Stage::Extract,
            "Декодирование и сохранение полного аудио",
            || -> Result<f64> {
                let pcm =
                    localvox_light_ingest::download::convert_to_pcm_s16le(&ffmpeg, &file, false)?;
                let samples: Vec<i16> = pcm
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]))
                    .collect();
                if samples.is_empty() {
                    anyhow::bail!("в источнике нет звуковой дорожки");
                }
                let params = Arc::new(ChunkParams {
                    audio_dir: session.join("audio"),
                    meta_path: meta_path.clone(),
                    chunk_sec: 300.0,
                    flac: false,
                    ffmpeg: std::path::PathBuf::from(&ffmpeg),
                });
                // A retry replaces the downloaded track; old partial metadata must not duplicate it.
                meta.chunks.clear();
                let shared = Arc::new(Mutex::new(meta));
                // Track 0 — as if it were the microphone: a lecture has one voice line, and inventing a
                // second, empty track would only make the player lie about a silent interlocutor.
                let mut rec = ChunkRecorder::new(0, params, shared);
                rec.feed(&samples);
                rec.finalize_current();
                localvox_light_core::artifacts::commit_preparation(session, samples.len() as u64)?;
                let saved: SessionMeta = serde_json::from_slice(&std::fs::read(&meta_path)?)?;
                let seconds = saved.chunks.iter().map(|c| c.duration_sec).sum::<f64>();
                anyhow::ensure!(
                    (seconds - samples.len() as f64 / 16_000.0).abs() < 0.05,
                    "дорожка сохранена не полностью: {} с из {} с",
                    seconds,
                    samples.len() as f64 / 16_000.0
                );
                Ok(samples.len() as f64 / 16_000.0)
            },
        )
    });

    // The temporary download is removed on ANY outcome: a failed ingest must not leave hundreds
    // of megabytes in the temp directory to be found by nobody.
    let _ = std::fs::remove_file(&file);
    let seconds = outcome?;

    tracing::info!(
        "ingest: {} — {:.0} с звука из {}",
        session.display(),
        seconds,
        source.url
    );
    Ok(())
}

/// What the source calls itself. A failure here is not a failure of the ingest: a session with no
/// title is worse than one with a title, but it is still a session.
fn source_title(yt_dlp: &str, url: &str) -> Option<String> {
    use localvox_light_process::{Limits, Scope};
    let mut command = std::process::Command::new(yt_dlp);
    command.args([
        "--encoding",
        "utf-8",
        "--skip-download",
        "--print",
        "%(title)s",
        url,
    ]);
    let mut limits = Limits::new(std::time::Duration::from_secs(30));
    if std::env::var(localvox_light_process::CONTAINED_ENV).as_deref() == Ok("1") {
        limits.scope = Scope::Inherit;
    }
    let out = localvox_light_process::run(command, None, limits, || false, |_, _| {}).ok()?;
    if !out.termination.success() {
        return None;
    }
    let title = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!title.is_empty()).then_some(title)
}
