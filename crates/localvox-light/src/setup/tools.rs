//! System tools are discovered without installing or invoking model inference.
use anyhow::{Context, Result};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Ffmpeg,
    YtDlp,
    Mpv,
    Jq,
    Unzip,
}
impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::YtDlp => "yt-dlp",
            Self::Mpv => "mpv",
            Self::Jq => "jq",
            Self::Unzip => "unzip",
        }
    }
    pub fn key(self) -> Option<&'static str> {
        match self {
            Self::Ffmpeg => Some("LOCALVOX_LIGHT_YT_FFMPEG"),
            Self::YtDlp => Some("LOCALVOX_LIGHT_YT_DLP"),
            Self::Mpv => Some("LOCALVOX_MPV"),
            _ => None,
        }
    }
    pub fn find(self, config: &BTreeMap<String, String>, install: &Path) -> Option<PathBuf> {
        if let Some(value) = self
            .key()
            .and_then(|key| config.get(key))
            .filter(|s| !s.is_empty())
        {
            let path = PathBuf::from(value);
            let path = if path.is_absolute() {
                path
            } else {
                install.join(path)
            };
            return path.is_file().then_some(path);
        }
        let name = format!("{}{}", self.name(), std::env::consts::EXE_SUFFIX);
        // The verified Windows bundle supplies a compatible player, even if an
        // older unrelated mpv appears earlier in the inherited PATH.
        if self == Self::Mpv {
            let bundled = install.join("tools/mpv").join(&name);
            if bundled.is_file() {
                return Some(bundled);
            }
        }
        let mut directories: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        #[cfg(windows)]
        directories.extend(registered_paths());
        directories.push(install.to_owned());
        directories.push(install.join("tools/mpv"));
        directories.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]);
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            directories.push(PathBuf::from(local).join("Microsoft/WinGet/Links"));
        }
        #[cfg(windows)]
        if self == Self::YtDlp {
            directories.extend(registered_package_directories("yt-dlp.yt-dlp"));
        }
        directories
            .into_iter()
            .map(|dir| dir.join(&name))
            .find(|path| path.is_file())
    }
    pub fn ready(self, config: &BTreeMap<String, String>, install: &Path) -> bool {
        let Some(path) = self.find(config, install) else {
            return false;
        };
        let mut command = Command::new(path);
        command.arg(if self == Self::Ffmpeg {
            "-version"
        } else if self == Self::Unzip {
            "-v"
        } else {
            "--version"
        });
        let output = localvox_light_process::run_observed(
            command,
            None,
            // The Windows portable yt-dlp unpacks its Python runtime even for
            // --version; allow for that startup without assuming the tool is missing.
            localvox_light_process::Limits::new(Duration::from_secs(if self == Self::YtDlp {
                15
            } else {
                3
            })),
            || false,
            |_, _| {},
            None,
        );
        let Ok(output) = output else {
            return false;
        };
        if !output.termination.success() {
            return false;
        }
        if self != Self::Mpv {
            return true;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let version = text
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .trim_start_matches('v');
        let mut numbers = version.split('.').filter_map(|s| s.parse::<u32>().ok());
        matches!((numbers.next(), numbers.next()), (Some(major), Some(minor)) if (major, minor) >= (0, 38))
    }
}

pub fn install(
    tools: &[Tool],
    source: &Path,
    config: &BTreeMap<String, String>,
    destination: &Path,
) -> Result<()> {
    let missing: Vec<_> = tools
        .iter()
        .copied()
        .filter(|tool| !tool.ready(config, destination))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let list = missing
        .iter()
        .map(|tool| tool.name())
        .collect::<Vec<_>>()
        .join(",");
    let mut command = super::execute::script_command(source, "setup-tools");
    command
        .arg(if cfg!(windows) { "-Only" } else { "--only" })
        .arg(list)
        .arg(if cfg!(windows) {
            "-InstallDir"
        } else {
            "--install-dir"
        })
        .arg(destination);
    // Package managers may legitimately ask for OS authorization on their own TTY.
    let status = command
        .status()
        .context("Запуск установки системных инструментов")?;
    anyhow::ensure!(
        status.success(),
        "Доустановка инструментов не завершена; готовые файлы сохранены"
    );
    for tool in missing {
        anyhow::ensure!(
            tool.ready(config, destination),
            "{} не найден или версия не поддерживается после установки",
            tool.name()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn package_directories(key: &winreg::RegKey, package: &str) -> Vec<PathBuf> {
    key.enum_keys()
        .filter_map(Result::ok)
        .filter(|name| {
            name.split_once('_')
                .is_some_and(|(id, _)| id.eq_ignore_ascii_case(package))
        })
        .filter_map(|name| {
            key.open_subkey(name)
                .ok()?
                .get_value::<String, _>("InstallLocation")
                .ok()
        })
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .collect()
}

#[cfg(windows)]
fn registered_package_directories(package: &str) -> Vec<PathBuf> {
    use winreg::{
        enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
        RegKey,
    };
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .flat_map(|hive| {
            RegKey::predef(hive)
                .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall")
                .map(|key| package_directories(&key, package))
                .unwrap_or_default()
        })
        .collect()
}

#[cfg(windows)]
pub(super) fn registered_paths() -> Vec<PathBuf> {
    use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
    use winreg::{
        enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
        RegKey,
    };
    let mut paths = vec![];
    for (hive, key) in [
        (HKEY_CURRENT_USER, "Environment"),
        (
            HKEY_LOCAL_MACHINE,
            "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
        ),
    ] {
        let Ok(key) = RegKey::predef(hive).open_subkey(key) else {
            continue;
        };
        let Ok(value) = key.get_value::<String, _>("Path") else {
            continue;
        };
        let input: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        // Windows expands REG_EXPAND_SZ using the current process's environment.
        let needed = unsafe { ExpandEnvironmentStringsW(input.as_ptr(), std::ptr::null_mut(), 0) };
        if needed == 0 {
            continue;
        }
        let mut expanded = vec![0; needed as usize];
        let written =
            unsafe { ExpandEnvironmentStringsW(input.as_ptr(), expanded.as_mut_ptr(), needed) };
        if written == 0 || written > needed {
            continue;
        }
        let value = String::from_utf16_lossy(&expanded[..written.saturating_sub(1) as usize]);
        paths.extend(std::env::split_paths(&value));
    }
    paths
}

pub fn command_path(
    config: &BTreeMap<String, String>,
    install: &Path,
) -> Result<std::ffi::OsString> {
    let mut directories: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    for tool in [Tool::Jq, Tool::Unzip] {
        if let Some(path) = tool
            .find(config, install)
            .and_then(|p| p.parent().map(Path::to_owned))
        {
            directories.push(path);
        }
    }
    Ok(std::env::join_paths(directories)?)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn registered_portable_package_is_found_without_accepting_other_ids_or_relative_roots() {
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        let temp = tempfile::tempdir().unwrap();
        let name = format!(
            "Software\\LocalVox\\Tests\\{}",
            temp.path().file_name().unwrap().to_string_lossy()
        );
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.0);
            }
        }
        let _cleanup = Cleanup(name.clone());
        let (root, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(&name)
            .unwrap();
        for (id, location) in [
            ("yt-dlp.yt-dlp_Source", temp.path().display().to_string()),
            (
                "yt-dlp.yt-dlp-extra_Source",
                temp.path().display().to_string(),
            ),
            ("yt-dlp.yt-dlp_Other", "relative-folder".into()),
        ] {
            let (key, _) = root.create_subkey(id).unwrap();
            key.set_value("InstallLocation", &location).unwrap();
        }
        root.create_subkey("yt-dlp.yt-dlp_NoLocation").unwrap();
        assert_eq!(
            package_directories(&root, "yt-dlp.yt-dlp"),
            vec![temp.path().to_owned()]
        );
    }
}
