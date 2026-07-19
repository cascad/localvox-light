//! A link → a session.
//!
//! One place, because there are two doors into it: the window (a link in the search box, Ctrl+V)
//! and the voice («возьми ссылку» — the link is taken from the clipboard). Two writers of one
//! thing would drift apart the first time either of them was fixed.
//!
//! The session is created RIGHT AWAY, empty, with the link in its meta — and only then is the job
//! queued. That order is the point: the card shows up in the archive at once and carries the
//! stages of its own arrival. A link that vanishes for ten minutes with nothing to look at is
//! indistinguishable from a link that was dropped.
//!
//! The audio is fetched by whoever runs the jobs (the daemon): it owns the child processes and
//! can kill them on exit.

use std::path::Path;

use anyhow::{Context, Result};

/// Queue a link for transcription. Returns the name of the session it landed in.
pub fn from_url(work_dir: &Path, raw_url: &str) -> Result<String> {
    let url = crate::links::clean(raw_url).context("это не ссылка: нужен http(s)-адрес")?;
    let label = crate::links::label(&url);

    let (_audio_dir, meta_path) = crate::chunks::create_session_dir(work_dir, Some(&label))
        .context("не удалось создать сессию")?;
    let meta = crate::chunks::SessionMeta {
        started_at: crate::versions::now_rfc3339(),
        sample_rate: 16_000,
        chunks: Vec::new(),
        source: Some(crate::chunks::Source {
            url: url.clone(),
            title: None,
        }),
        ..Default::default()
    };
    crate::chunks::save_meta_public(&meta_path, &meta);

    let name = meta_path
        .parent()
        .and_then(|d| d.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .context("у сессии нет имени")?;

    let mut queue = crate::jobs::JobQueue::load(work_dir);
    queue.enqueue_ingest(&name, true, true, true);
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_becomes_an_empty_session_with_a_queued_job() {
        let d = tempfile::tempdir().unwrap();
        let name = from_url(d.path(), "https://youtu.be/abc?si=junk").unwrap();

        // The session exists NOW — before a single byte has been downloaded.
        let dir = d.path().join("sessions").join(&name);
        assert!(dir.join("meta.json").exists());
        let meta: crate::chunks::SessionMeta =
            serde_json::from_slice(&std::fs::read(dir.join("meta.json")).unwrap()).unwrap();
        // The tracking junk is gone: two copies of one lecture must not become two sessions.
        assert_eq!(meta.source.as_ref().unwrap().url, "https://youtu.be/abc");
        assert!(meta.chunks.is_empty(), "there is no audio yet, and that is correct");

        let queue = crate::jobs::JobQueue::load(d.path());
        let job = queue.jobs().iter().find(|j| j.session == name).expect("no job");
        assert_eq!(job.kind, crate::jobs::JobKind::Ingest);
    }

    /// Not everything is a link. A phrase from the clipboard, a path, a `file://` — a refusal, not
    /// an empty session in the archive.
    #[test]
    fn a_non_link_creates_nothing() {
        let d = tempfile::tempdir().unwrap();
        assert!(from_url(d.path(), "что решили по бэклогу").is_err());
        assert!(from_url(d.path(), "file:///C:/Windows/win.ini").is_err());
        assert!(!d.path().join("sessions").exists(), "a session was created for a non-link");
    }
}
