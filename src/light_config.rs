//! Сохранение устройств из TUI (F2), как client-config в client-reliable.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "localvox-light-config.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LightDeviceConfig {
    #[serde(default)]
    pub mic: String,
    #[serde(default)]
    pub loopback: bool,
    #[serde(default = "default_loopback_device")]
    pub loopback_device: String,
}

fn default_loopback_device() -> String {
    "default-output".into()
}

impl LightDeviceConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

/// Явный путь из `--config` или env `LOCALVOX_LIGHT_CONFIG_FILE`.
pub fn explicit_config_path(cli_path: &Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = cli_path {
        if !p.as_os_str().is_empty() {
            return Some(p.clone());
        }
    }
    std::env::var("LOCALVOX_LIGHT_CONFIG_FILE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Файл в cwd, если существует (автозагрузка без `--config`).
pub fn cwd_config_path() -> Option<PathBuf> {
    let p = std::env::current_dir().ok()?.join(FILE_NAME);
    p.is_file().then_some(p)
}

pub fn save_path_for_write() -> PathBuf {
    explicit_config_path(&None)
        .or_else(cwd_config_path)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(FILE_NAME))
}
