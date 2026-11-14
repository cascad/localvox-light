//! Installation intent is an operation checkpoint, never active runtime configuration.
use anyhow::{Context, Result};
use localvox_light_core::{artifacts, model_paths};
use localvox_setup_ui::Selection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Pending {
    schema: u32,
    selected: Selection,
}
fn path(install: &Path) -> PathBuf {
    install.join("cache/installation.pending.json")
}
fn user_root() -> Option<PathBuf> {
    model_paths::Context::capture().user_root()
}

pub fn location() -> Option<PathBuf> {
    let data = std::fs::read_to_string(user_root()?.join("install-location")).ok()?;
    let value = data.trim_end_matches(['\r', '\n']);
    let path = PathBuf::from(value);
    (path.is_absolute() && !value.chars().any(char::is_control)).then_some(path)
}

pub fn read(install: &Path) -> Result<Option<Selection>> {
    let bytes = match std::fs::read(path(install)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let pending: Pending =
        serde_json::from_slice(&bytes).context("Сохранённый план установки повреждён")?;
    anyhow::ensure!(
        pending.schema == 1,
        "Неизвестная версия сохранённого плана установки"
    );
    anyhow::ensure!(
        super::bundle::canonical_destination(Path::new(&pending.selected.program_dir))?
            == super::bundle::canonical_destination(install)?,
        "Сохранённый план относится к другой папке"
    );
    Ok(Some(pending.selected))
}

pub fn write(install: &Path, selected: &Selection, registered: bool) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(&Pending {
        schema: 1,
        selected: selected.clone(),
    })?;
    let path = path(install);
    std::fs::create_dir_all(path.parent().unwrap())?;
    artifacts::atomic_write(&path, &bytes)?;
    if registered {
        if let Some(root) = user_root() {
            #[cfg(windows)]
            let created = !root.exists();
            std::fs::create_dir_all(&root)?;
            #[cfg(windows)]
            if created {
                use std::os::windows::ffi::OsStrExt;
                use windows_sys::Win32::Storage::FileSystem::{
                    GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN,
                    INVALID_FILE_ATTRIBUTES,
                };
                let name: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
                let current = unsafe { GetFileAttributesW(name.as_ptr()) };
                anyhow::ensure!(
                    current != INVALID_FILE_ATTRIBUTES
                        && unsafe {
                            SetFileAttributesW(name.as_ptr(), current | FILE_ATTRIBUTE_HIDDEN)
                        } != 0,
                    "Не удалось подготовить пользовательскую папку LocalVox"
                );
            }
            let destination = root.join("install-location");
            let value = format!("{}\n", install.display());
            if std::fs::read(&destination).ok().as_deref() != Some(value.as_bytes()) {
                artifacts::atomic_write(&destination, value.as_bytes())?;
            }
        }
    }
    Ok(bytes)
}

pub fn complete(install: &Path, expected: &[u8]) -> Result<()> {
    let path = path(install);
    if std::fs::read(&path).ok().as_deref() == Some(expected) {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
