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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("cfg.json");
        let cfg = LightDeviceConfig {
            mic: "2".into(),
            loopback: true,
            loopback_device: "Speakers".into(),
        };
        cfg.save(&path).unwrap();
        let loaded = LightDeviceConfig::load(&path).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn explicit_config_path_prefers_cli() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("explicit.json");
        assert_eq!(
            explicit_config_path(&Some(p.clone())),
            Some(p)
        );
    }

    #[test]
    fn explicit_config_path_ignores_empty_path() {
        assert!(explicit_config_path(&Some(PathBuf::new())).is_none());
    }

    #[test]
    fn default_loopback_device_in_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("c.json");
        std::fs::write(&path, r#"{"mic":"0","loopback":false}"#).unwrap();
        let c = LightDeviceConfig::load(&path).unwrap();
        assert_eq!(c.loopback_device, "default-output");
    }
}
