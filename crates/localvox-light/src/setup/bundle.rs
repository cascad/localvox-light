//! Validate a complete staging tree before publishing any program files.
use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Component, Path, PathBuf},
};

#[derive(Deserialize)]
struct Member {
    name: String,
    sha256: String,
    bytes: u64,
}
#[derive(Deserialize)]
pub struct Bundle {
    schema: u32,
    target: String,
    files: Vec<Member>,
}

pub fn target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        _ => "unsupported",
    }
}

pub fn executable(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// Identify a not-yet-created destination through its nearest existing parent.
pub fn canonical_destination(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut parent = absolute.as_path();
    let mut suffix = vec![];
    while !parent.exists() {
        suffix.push(
            parent
                .file_name()
                .context("Неверная папка назначения")?
                .to_owned(),
        );
        parent = parent.parent().context("Родитель папки назначения")?;
    }
    let mut resolved = parent.canonicalize()?;
    for part in suffix.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

pub fn digest(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn checked(root: &Path, name: &str) -> Result<PathBuf> {
    let relative = Path::new(name);
    anyhow::ensure!(
        !name.is_empty()
            && !name.contains(['\\', ':'])
            && name != ".env"
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "Недопустимый файл комплекта: {name}"
    );
    let mut path = root.to_owned();
    for part in relative.components() {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                let link = metadata.file_type().is_symlink();
                #[cfg(windows)]
                let link = {
                    use std::os::windows::fs::MetadataExt;
                    link || metadata.file_attributes() & 0x400 != 0
                };
                anyhow::ensure!(!link, "Ссылка в пути комплекта: {}", path.display());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

impl Bundle {
    pub fn read(root: &Path) -> Result<Self> {
        let data = std::fs::read(root.join("bundle.json"))
            .context("Не найден манифест полного комплекта")?;
        let bundle: Self = serde_json::from_slice(&data).context("Повреждён bundle.json")?;
        anyhow::ensure!(
            bundle.schema == 1 && bundle.target == target(),
            "Комплект для другой платформы или версии installer"
        );
        let mut names = BTreeSet::new();
        for member in &bundle.files {
            checked(root, &member.name)?;
            anyhow::ensure!(
                names.insert(member.name.as_str()),
                "Повтор файла в комплекте"
            );
            anyhow::ensure!(
                member.sha256.len() == 64 && member.sha256.bytes().all(|c| c.is_ascii_hexdigit()),
                "Неверный hash в комплекте"
            );
        }
        for name in [
            "localvox-light",
            "localvox-process",
            "localvox-tui",
            "vox",
            "localvox-install",
        ] {
            anyhow::ensure!(
                names.contains(executable(name).as_str()),
                "Неполный комплект: {name}"
            );
        }
        let native = if cfg!(windows) {
            "libvosk.dll"
        } else if cfg!(target_os = "macos") {
            "libvosk.dylib"
        } else {
            "libvosk.so"
        };
        for name in [native, ".env.defaults", "scripts/models.json"] {
            anyhow::ensure!(names.contains(name), "Неполный комплект: {name}");
        }
        for script in ["fetch-models", "model-paths", "setup-ollama", "setup-tools"] {
            let name = format!(
                "scripts/{script}.{}",
                if cfg!(windows) { "ps1" } else { "sh" }
            );
            anyhow::ensure!(names.contains(name.as_str()), "Неполный комплект: {name}");
        }
        if cfg!(windows) {
            for name in [
                "libgcc_s_seh-1.dll",
                "libstdc++-6.dll",
                "libwinpthread-1.dll",
            ] {
                anyhow::ensure!(names.contains(name), "Неполный комплект: {name}");
            }
        }
        bundle.verify(root)?;
        Ok(bundle)
    }

    pub fn verify(&self, root: &Path) -> Result<()> {
        for member in &self.files {
            let path = checked(root, &member.name)?;
            let metadata = path
                .metadata()
                .with_context(|| format!("Нет файла: {}", member.name))?;
            anyhow::ensure!(
                metadata.is_file()
                    && metadata.len() == member.bytes
                    && digest(&path)? == member.sha256.to_ascii_lowercase(),
                "Не совпала контрольная сумма: {}",
                member.name
            );
        }
        Ok(())
    }

    pub fn publish(&self, source: &Path, destination: &Path) -> Result<()> {
        self.verify(source)?;
        // All paths are checked before the first mutation. The manifest is published last,
        // so an interrupted update is detected as incomplete and can be reconciled.
        for member in &self.files {
            checked(destination, &member.name)?;
        }
        for name in self
            .files
            .iter()
            .map(|m| m.name.as_str())
            .chain(std::iter::once("bundle.json"))
        {
            let from = source.join(name);
            let to = checked(destination, name)?;
            if to.is_file() && digest(&from)? == digest(&to)? {
                continue;
            }
            let parent = to.parent().context("Папка файла комплекта")?;
            std::fs::create_dir_all(parent)?;
            let staged = tempfile::NamedTempFile::new_in(parent)?;
            std::fs::copy(from, staged.path())?;
            staged.as_file().sync_all()?;
            staged.persist(&to).map_err(|e| e.error).with_context(|| format!("Не удалось обновить {}. Закройте запущенные приложения LocalVox и повторите мастер.", to.display()))?;
        }
        Ok(())
    }
}
