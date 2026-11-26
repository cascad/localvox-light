//! Feature operations behind the stable Archive API facade.
use super::*;

impl Archive {
    /// What the recorder is doing right now.
    ///
    /// `engine` is a separate fact from `recording`, and the app needs both: with a dead
    /// daemon a "record" button would look pressable and go nowhere, and the person would
    /// be left believing they are being recorded. That is the one lie this system must
    /// never tell.
    pub fn record_state(&self) -> RecordState {
        let session = localvox_light_core::jobs::recording_session(&self.work_dir);
        let engine = engine_alive(&self.work_dir);
        RecordState {
            live_levels: engine,
            capture_sources: if engine {
                localvox_light_core::audio::status::read(&self.work_dir)
            } else {
                Vec::new()
            },
            model_notices: localvox_light_core::doctor::model_notices(
                &localvox_light_core::doctor::Layout::from_env(
                    localvox_light_core::model_paths::default_vosk().into(),
                    self.work_dir.clone(),
                ),
            ),
            engine,
            recording: engine && session.is_some(),
            session: session.filter(|_| engine),
            // Kept in the response schema for older clients; history is no longer archived.
            preroll_sec: 0.0,
            // Gated on `engine` for the same reason as `session`: the marker is a file, and a
            // killed daemon does not clear it. Without the engine there is nobody listening, and
            // a screen saying otherwise is the one lie this system must never tell.
            voice: engine
                .then(|| localvox_light_core::voice_note::read(&self.work_dir))
                .flatten(),
        }
    }

    pub fn capture_levels(&self) -> [Option<u8>; 2] {
        if engine_alive(&self.work_dir) {
            localvox_light_core::audio::levels::read(&self.work_dir)
        } else {
            [None; 2]
        }
    }

    /// Start recording. Only audio between the accepted Start and Stop is archived.
    pub fn start_recording(&self, title: &str) -> Result<String> {
        if let Some(name) = localvox_light_core::jobs::recording_session(&self.work_dir) {
            bail!("запись уже идёт: {name}");
        }
        let title = title.trim();
        localvox_light_core::jobs::request_record_start(&self.work_dir, title)
            .context("не удалось попросить движок начать запись")?;
        Ok(if title.is_empty() {
            "запись начата".into()
        } else {
            format!("запись начата: {title}")
        })
    }

    /// Stop recording: the engine closes the session, and the cook picks it up. The reason
    /// goes into `meta.json` — the archive must be able to say who ended the recording.
    pub fn stop_recording(&self) -> Result<String> {
        let Some(name) = localvox_light_core::jobs::recording_session(&self.work_dir) else {
            bail!("сейчас ничего не записывается");
        };
        localvox_light_core::jobs::request_record_stop(&self.work_dir, "вручную")
            .context("не удалось попросить движок остановить запись")?;
        Ok(name)
    }
}
