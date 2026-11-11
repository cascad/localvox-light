//! Feature operations behind the stable Archive API facade.
use super::*;

impl Archive {
    pub fn list_sessions(&self, limit: usize) -> Vec<SessionInfo> {
        let mut names: Vec<String> = fs::read_dir(self.sessions_root())
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names.reverse(); // freshest first
        let live = localvox_light_core::jobs::recording_session(&self.work_dir);
        // We ask about the engine's liveness ONCE for the whole list, not per session:
        // this takes a file lock, and doing it fifty times in a row is silly.
        let engine = engine_alive(&self.work_dir);
        // What the secretary is doing with each session — from the queue, in one read.
        let job_of: std::collections::HashMap<String, String> = {
            let q = localvox_light_core::jobs::JobQueue::load(&self.work_dir);
            use localvox_light_core::jobs::JobState as S;
            q.jobs()
                .iter()
                .filter_map(|j| {
                    let state = match j.state {
                        S::Running => "running",
                        S::Cancelling => "cancelling",
                        S::Cancelled => "cancelled",
                        S::Pending => "pending",
                        S::WaitingInput => "waiting_input",
                        S::Failed => "failed",
                        S::Done => return None,
                    };
                    Some((j.session.clone(), state.to_string()))
                })
                .collect()
        };
        names
            .into_iter()
            .take(limit.clamp(1, 100))
            .map(|name| {
                let dir = self.sessions_root().join(&name);
                let meta: Option<localvox_light_core::chunks::SessionMeta> =
                    std::fs::read(dir.join("meta.json"))
                        .ok()
                        .and_then(|b| serde_json::from_slice(&b).ok());
                // Duration — from the start of the recording to the end of the last
                // chunk: silence and pauses are included, because that IS the
                // recording time.
                let duration_sec = meta
                    .as_ref()
                    .map(|m| {
                        m.chunks
                            .iter()
                            .map(|c| c.start_offset_sec + c.duration_sec)
                            .fold(0.0f64, f64::max)
                    })
                    .unwrap_or(0.0);
                let store = VersionStore::open(&dir).ok();
                let lines = store
                    .as_ref()
                    .and_then(|s| s.best())
                    .and_then(|v| store.as_ref().and_then(|s| s.resolve(v.id)))
                    .and_then(|p| read_transcript_lines(&p).ok())
                    .map(|l| l.len())
                    .unwrap_or(0);
                SessionInfo {
                    started_at: meta.as_ref().map(|m| m.started_at.clone()),
                    source_url: meta
                        .as_ref()
                        .and_then(|m| m.source.as_ref())
                        .map(|s| s.url.clone()),
                    stopped_at: meta.as_ref().and_then(|m| m.stopped_at.clone()),
                    stopped_reason: meta.as_ref().and_then(|m| m.stopped_reason.clone()),
                    duration_sec,
                    empty: lines == 0,
                    lang: meta.as_ref().and_then(|m| m.lang.clone()),
                    lang_detected: meta.as_ref().and_then(|m| m.lang_detected.clone()),
                    title: meta.as_ref().and_then(|m| m.title.clone()),
                    // A meeting is either one marked by a person or a caught call:
                    // both signs are equally real.
                    meeting: meta
                        .as_ref()
                        .map(|m| m.meeting || !m.meetings.is_empty())
                        .unwrap_or(false),
                    transcript_versions: store
                        .as_ref()
                        .map(|s| s.load().versions.len())
                        .unwrap_or(0),
                    has_summary: dir.join("summary.md").exists(),
                    has_processed: localvox_light_core::readable::exists(&dir),
                    // The marker is the engine's own word. We check not "file
                    // freshness" but whether the engine is alive: an empty newborn
                    // session is a recording, not a corpse.
                    recording: live.as_deref() == Some(name.as_str()) && engine,
                    cooked: store
                        .as_ref()
                        .map(|s| !s.load().versions.is_empty())
                        .unwrap_or(false),
                    job: job_of.get(&name).cloned(),
                    summary_doubts: localvox_light_core::processing::doubts(
                        &dir,
                        localvox_light_core::processing::SUMMARY,
                    ),
                    processed_doubts: localvox_light_core::processing::doubts(
                        &dir,
                        localvox_light_core::processing::PROCESSED,
                    ),
                    name,
                }
            })
            .collect()
    }

    /// The session directory, guarded against escaping the archive: the name MUST be
    /// exactly one normal path component — this cuts off not only `/`, `\`, `..` but
    /// also Windows exotica like `C:`, `.` and UNC prefixes.
    pub fn session_dir(&self, session: &str) -> Result<PathBuf> {
        use std::path::Component;
        let mut comps = Path::new(session).components();
        if !matches!(
            (comps.next(), comps.next()),
            (Some(Component::Normal(_)), None)
        ) {
            bail!("некорректное имя сессии");
        }
        let dir = self.sessions_root().join(session);
        if !dir.is_dir() {
            bail!("сессия не найдена: {session}");
        }
        Ok(dir)
    }

    /// The languages we can REALLY recognize: the ones we have a model for. The web
    /// builds its selector from this list rather than from one hardcoded into the
    /// HTML — offering a language we cannot do means promising the impossible.
    pub fn langs(&self) -> Vec<String> {
        let Some(default_dir) = localvox_light_core::lang::default_model_dir() else {
            return Vec::new();
        };
        // The default one plus those that have a model on disk. A list of short codes,
        // not a sweep of the whole of ISO-639: we look at what is on disk and in the
        // environment.
        let mut out = Vec::new();
        for code in localvox_light_core::lang::candidates(&default_dir) {
            if localvox_light_core::lang::has_model(&code, &default_dir) {
                out.push(code);
            }
        }
        out
    }
}
