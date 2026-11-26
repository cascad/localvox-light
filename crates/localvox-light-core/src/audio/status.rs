//! Engine-owned capture evidence; audio callbacks never touch this store.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

const FILE: &str = ".capture-status.json";

/// Shared by CPAL's data/error callbacks and its owning control loop. No disk
/// access, no waiting for an error queue, and no invented signal from silence.
#[derive(Clone)]
pub(super) struct StreamProbe {
    signal: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    errors: crossbeam_channel::Sender<String>,
    received: crossbeam_channel::Receiver<String>,
}

impl StreamProbe {
    pub fn new() -> Self {
        let (errors, received) = crossbeam_channel::bounded(1);
        Self {
            signal: Arc::new(AtomicBool::new(false)),
            peak: Arc::new(AtomicU64::new(0)),
            errors,
            received,
        }
    }
    pub fn observe(&self, samples: &[i16]) -> f32 {
        let rms = super::pcm_rms(samples);
        let level = (rms * 12.0).min(1.0);
        if level > 0.003 {
            self.signal.store(true, Ordering::Relaxed);
        }
        // One callback writer per stream. The control loop only reads this word.
        let word = super::levels::accumulate(
            self.peak.load(Ordering::Relaxed),
            super::levels::bucket(),
            rms,
        );
        self.peak.store(word, Ordering::Relaxed);
        level
    }
    pub fn failed(&self, error: String) {
        let _ = self.errors.try_send(error);
    }
    pub fn poll(&self, monitor: &CaptureMonitor, source: u8, reported: &mut bool) -> Result<()> {
        if let Ok(error) = self.received.try_recv() {
            anyhow::bail!("audio stream: {error}");
        }
        if let Some(page) = &monitor.levels {
            page.set(source, self.peak.load(Ordering::Relaxed));
        }
        if !*reported && self.signal.load(Ordering::Relaxed) {
            monitor.signal(source);
            *reported = true;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureSource {
    pub source_id: u8,
    pub name: String,
    pub state: String,
    pub detail: String,
    #[serde(default)]
    pub device: Option<String>,
}

pub struct CaptureMonitor {
    levels: Option<super::levels::Levels>,
    path: PathBuf,
    sources: Mutex<Vec<CaptureSource>>,
    dirty: AtomicBool,
}

impl CaptureMonitor {
    /// Called under the engine's workspace lease, before capture threads start.
    pub fn new(workspace: &Path, loopback: bool) -> Result<Arc<Self>> {
        let sources = vec![
            CaptureSource {
                source_id: 0,
                name: "Микрофон".into(),
                state: "starting".into(),
                detail: "Открываем микрофон".into(),
                device: None,
            },
            CaptureSource {
                source_id: 1,
                name: "Системный звук".into(),
                device: None,
                state: if loopback { "starting" } else { "disabled" }.into(),
                detail: if loopback {
                    "Открываем системный звук"
                } else {
                    "Выключен в настройках"
                }
                .into(),
            },
        ];
        let monitor = Arc::new(Self {
            levels: super::levels::Levels::create(workspace)
                .map_err(|err| tracing::warn!(%err, "capture meters unavailable"))
                .ok(),
            path: workspace.join(FILE),
            sources: Mutex::new(sources),
            dirty: AtomicBool::new(false),
        });
        crate::artifacts::atomic_write(
            &monitor.path,
            &serde_json::to_vec(&*monitor.sources.lock().unwrap())?,
        )?;
        Ok(monitor)
    }

    fn update(&self, source_id: u8, state: &str, detail: String, device: Option<String>) {
        let Ok(mut sources) = self.sources.lock() else {
            return;
        };
        let Some(source) = sources.iter_mut().find(|s| s.source_id == source_id) else {
            return;
        };
        // A signal belongs to the open stream. Opening, failure or disable
        // starts a new reading and invalidates the preceding endpoint.
        let device = if state == "signal" {
            source.device.clone()
        } else {
            if let Some(page) = &self.levels {
                page.set(source_id, 0);
            }
            device
        };
        if source.state == state && source.detail == detail && source.device == device {
            return;
        }
        source.device = device;
        source.state = state.into();
        source.detail = detail;
        self.dirty.store(true, Ordering::Release);
    }

    /// Only the existing device-health worker calls this in production. Even the
    /// WASAPI packet reader never waits for filesystem I/O to report a signal.
    pub fn publish(&self) -> Result<()> {
        if !self.dirty.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        let result = self
            .sources
            .lock()
            .map_err(|e| anyhow::anyhow!("capture status: {e}"))
            .and_then(|sources| Ok(serde_json::to_vec(&*sources)?))
            .and_then(|bytes| crate::artifacts::atomic_write(&self.path, &bytes));
        if result.is_err() {
            self.dirty.store(true, Ordering::Release);
        }
        result
    }

    pub fn opened(&self, source_id: u8) {
        self.open_device(source_id, None);
    }

    pub fn open_device(&self, source_id: u8, device: Option<String>) {
        let detail = if source_id == 1 {
            format!(
                "Поток открыт; сигнал ещё не подтверждён. Включите звук в другом приложении.{}",
                access_hint()
            )
        } else {
            "Поток открыт; сигнал ещё не подтверждён. Скажите несколько слов.".into()
        };
        self.update(source_id, "waiting", detail, device);
    }

    pub fn signal(&self, source_id: u8) {
        self.update(
            source_id,
            "signal",
            "Звуковой сигнал получен после открытия потока".into(),
            None,
        );
    }

    pub fn error(&self, source_id: u8, error: &str) {
        self.update(
            source_id,
            "error",
            format!("{error}{}", if source_id == 1 { access_hint() } else { "" }),
            None,
        );
    }

    pub fn disabled(&self, source_id: u8) {
        self.update(source_id, "disabled", "Выключен в настройках".into(), None);
    }
}

fn access_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        " macOS: доступ к микрофону и системному аудио выдаётся отдельно. Проверьте «Системные настройки → Конфиденциальность и безопасность → Запись экрана и системного аудио» для LocalVox или терминала, из которого запущен vox. После выдачи доступа перезапустите LocalVox."
    } else {
        ""
    }
}

/// The caller must separately establish that this workspace has a live engine.
pub fn read(workspace: &Path) -> Vec<CaptureSource> {
    std::fs::read(workspace.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_callback_bridge_distinguishes_silence_signal_and_failure_without_writing_files() {
        let root = tempfile::tempdir().unwrap();
        let monitor = CaptureMonitor::new(root.path(), true).unwrap();
        monitor.opened(1);
        let probe = StreamProbe::new();
        let callback = probe.clone();
        let mut reported = false;
        callback.observe(&[0; 512]);
        callback.observe(&[1; 512]);
        probe.poll(&monitor, 1, &mut reported).unwrap();
        assert!(
            !reported,
            "silence and rounding noise are not proof of sound"
        );
        callback.observe(&[4096; 512]);
        probe.poll(&monitor, 1, &mut reported).unwrap();
        assert!(reported);
        assert_eq!(crate::audio::levels::read(root.path()), [None, Some(4)]);
        // Independent readers do not consume the same peak.
        assert_eq!(crate::audio::levels::read(root.path()), [None, Some(4)]);
        monitor.open_device(1, Some("Built-in Output".into()));
        assert_eq!(crate::audio::levels::read(root.path()), [None; 2]);
        monitor.signal(1);
        assert_eq!(
            read(root.path())[1].state,
            "starting",
            "callbacks and control polls do not write files"
        );
        monitor.publish().unwrap();
        assert_eq!(read(root.path())[1].state, "signal");
        assert_eq!(
            read(root.path())[1].device.as_deref(),
            Some("Built-in Output")
        );
        callback.failed("device disconnected".into());
        callback.failed("duplicate failure does not block the callback".into());
        assert!(probe
            .poll(&monitor, 1, &mut reported)
            .unwrap_err()
            .to_string()
            .contains("device disconnected"));
    }

    #[test]
    fn opened_is_not_signal_and_errors_are_independent_of_the_ui_channel() {
        let root = tempfile::tempdir().unwrap();
        let monitor = CaptureMonitor::new(root.path(), true).unwrap();
        monitor.opened(0);
        monitor.opened(1);
        assert!(
            read(root.path()).iter().all(|s| s.state == "starting"),
            "capture paths do no filesystem I/O"
        );
        monitor.publish().unwrap();
        assert!(read(root.path()).iter().all(|s| s.state == "waiting"));
        monitor.signal(0);
        monitor.error(1, "capture refused");
        monitor.publish().unwrap();
        let sources = read(root.path());
        assert_eq!(sources[0].state, "signal");
        assert_eq!(sources[1].state, "error");
        assert!(sources[1].detail.contains("capture refused"));
        // Retry alone does not clear the error; successful open does.
        monitor.error(1, "capture refused");
        monitor.publish().unwrap();
        assert_eq!(sources, read(root.path()));
        monitor.opened(1);
        monitor.publish().unwrap();
        assert_eq!(read(root.path())[1].state, "waiting");
        monitor.signal(1);
        monitor.publish().unwrap();
        assert_eq!(read(root.path())[1].state, "signal");
        monitor.disabled(1);
        monitor.publish().unwrap();
        assert_eq!(read(root.path())[1].state, "disabled");
        drop(monitor);
        let _new_engine = CaptureMonitor::new(root.path(), false).unwrap();
        assert_eq!(read(root.path())[0].state, "starting");
        assert_eq!(read(root.path())[1].state, "disabled");
    }
}
