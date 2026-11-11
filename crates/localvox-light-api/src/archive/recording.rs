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
            engine,
            recording: engine && session.is_some(),
            session: session.filter(|_| engine),
            preroll_sec: env_f64("LOCALVOX_LIGHT_PREROLL_SEC", 300.0),
            // Gated on `engine` for the same reason as `session`: the marker is a file, and a
            // killed daemon does not clear it. Without the engine there is nobody listening, and
            // a screen saying otherwise is the one lie this system must never tell.
            voice: engine
                .then(|| localvox_light_core::voice_note::read(&self.work_dir))
                .flatten(),
        }
    }

    /// Start recording. Nothing is written to disk until this is asked for — but the last
    /// minutes are held in memory, and they go into the session, so a conversation that
    /// began before the button is not lost.
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
