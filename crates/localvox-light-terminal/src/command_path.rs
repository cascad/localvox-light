//! User command registration. No daemon, models, capture or shell execution.
#[cfg(any(unix, test))]
use anyhow::Context;
use anyhow::{ensure, Result};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[cfg(any(unix, test))]
mod profiles;
#[cfg(windows)]
mod windows;

pub struct Report {
    pub code: i32,
    pub message: String,
}

pub fn executable(install: &Path) -> PathBuf {
    install.join(if cfg!(windows) { "vox.exe" } else { "vox" })
}

fn same(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn conflict(install: &Path, path: Option<OsString>) -> Option<PathBuf> {
    let expected = executable(install);
    let names: &[&str] = if cfg!(windows) {
        &["vox.exe", "vox.cmd", "vox.bat", "vox.com", "vox.ps1"]
    } else {
        &["vox"]
    };
    for directory in std::env::split_paths(&path.unwrap_or_default()) {
        for name in names {
            let candidate = directory.join(name);
            if candidate.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let Ok(metadata) = candidate.metadata() else {
                        continue;
                    };
                    if metadata.permissions().mode() & 0o111 == 0 {
                        continue;
                    }
                }
                if !same(&candidate, &expected) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_foreign_command_is_detected_without_modification() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("ours");
        let foreign = root.path().join("foreign");
        for folder in [&install, &foreign] {
            std::fs::create_dir(folder).unwrap();
            std::fs::write(executable(folder), b"owner executable").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    executable(folder),
                    std::fs::Permissions::from_mode(0o755),
                )
                .unwrap();
            }
        }
        let path = std::env::join_paths([&foreign, &install]).unwrap();
        assert_eq!(conflict(&install, Some(path)), Some(executable(&foreign)));
        assert!(conflict(&install, Some(std::env::join_paths([&install]).unwrap())).is_none());
        assert_eq!(
            std::fs::read(executable(&foreign)).unwrap(),
            b"owner executable"
        );
    }
}

fn preflight(install: &Path) -> Result<Option<Report>> {
    ensure!(
        executable(install).is_file(),
        "Нет {}. Нужен новый полный комплект LocalVox",
        executable(install).display()
    );
    if let Some(other) = conflict(install, std::env::var_os("PATH")) {
        return Ok(Some(Report {
            code: 1,
            message: format!(
                "Имя vox занято: {}. PATH не изменён. LocalVox: {}",
                other.display(),
                executable(install).display()
            ),
        }));
    }
    Ok(None)
}

pub fn check(install: &Path) -> Result<Report> {
    if let Some(report) = preflight(install)? {
        return Ok(report);
    }
    let visible = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .any(|p| same(&p, install));
    if visible {
        return Ok(Report {
            code: 0,
            message: format!("vox доступна: {}", executable(install).display()),
        });
    }
    #[cfg(windows)]
    let registered = windows::registered(install)?;
    #[cfg(not(windows))]
    let registered = profiles::configured(install)?;
    Ok(Report {
        code: if registered { 0 } else { 1 },
        message: if registered {
            "vox зарегистрирована; перезапустите терминал.".into()
        } else {
            format!(
                "vox не добавлена в PATH. Для настройки: {} --install-command",
                executable(install).display()
            )
        },
    })
}

pub fn install(install: &Path) -> Result<Report> {
    if let Some(report) = preflight(install)? {
        return Ok(report);
    }
    #[cfg(windows)]
    let changed = windows::install(install)?;
    #[cfg(not(windows))]
    let changed = profiles::install(install)?;
    Ok(Report {
        code: 0,
        message: if changed {
            "Команда vox добавлена в пользовательский PATH. Перезапустите терминал, затем: vox"
                .into()
        } else {
            "Команда vox уже зарегистрирована; повторная запись не нужна.".into()
        },
    })
}

#[cfg(any(unix, test))]
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("Чтение {}", path.display())),
    }
}
