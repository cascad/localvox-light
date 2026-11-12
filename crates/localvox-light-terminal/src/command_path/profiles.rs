//! Pure profile planning + atomic publication; also tested on Windows with temp files.
use super::read_optional;
use anyhow::{bail, ensure, Context, Result};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const BEGIN: &str = "# >>> LocalVox vox PATH >>>";
const END: &str = "# <<< LocalVox vox PATH <<<";

#[derive(Clone, Copy)]
pub(super) enum Shell {
    Posix,
    Fish,
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(super) fn block(directory: &Path, shell: Shell) -> Result<String> {
    let value = directory.to_str().context("PATH требует Unicode путь")?;
    ensure!(
        !value.contains([':', '\r', '\n', '\0']),
        "Каталог нельзя записать в PATH: {value}"
    );
    let quoted = quote(value);
    let body = match shell {
        Shell::Posix => format!("case \":${{PATH-}}:\" in\n  *:{quoted}:*) ;;\n  *) export PATH=\"${{PATH:+$PATH:}}\"{quoted} ;;\nesac"),
        Shell::Fish => format!("fish_add_path --append --path {quoted}"),
    };
    Ok(format!("{BEGIN}\n{body}\n{END}\n"))
}

pub(super) fn targets(
    home: &Path,
    shell: &str,
    zdot: Option<PathBuf>,
    config: Option<PathBuf>,
) -> Result<Vec<(PathBuf, Shell)>> {
    match Path::new(shell).file_name().and_then(|s| s.to_str()) {
        Some("bash") => {
            let login = [".bash_profile", ".bash_login", ".profile"].into_iter()
                .find(|name| home.join(name).exists()).unwrap_or(".profile");
            Ok(vec![(home.join(".bashrc"), Shell::Posix), (home.join(login), Shell::Posix)])
        }
        Some("zsh") => {
            let root = zdot.unwrap_or_else(|| home.to_owned());
            ensure!(root.is_absolute(), "ZDOTDIR должен быть абсолютным; добавьте каталог LocalVox в PATH вручную");
            Ok(vec![(root.join(".zshrc"), Shell::Posix)])
        }
        Some("fish") => {
            let root = config.unwrap_or_else(|| home.join(".config"));
            ensure!(root.is_absolute(), "XDG_CONFIG_HOME должен быть абсолютным");
            Ok(vec![(root.join("fish/conf.d/localvox.fish"), Shell::Fish)])
        }
        _ => bail!("Shell {shell} не поддержан автоматически. Добавьте каталог LocalVox в PATH вручную или установите с --no-path-update"),
    }
}

pub(super) struct Edit {
    path: PathBuf,
    destination: PathBuf,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}

impl Edit {
    pub(super) fn plan(path: PathBuf, body: &str, exclusive: bool) -> Result<Self> {
        let before = read_optional(&path)?;
        let destination = if before.is_some() {
            path.canonicalize()?
        } else {
            ensure!(
                fs::symlink_metadata(&path).is_err(),
                "Нет цели ссылки {}",
                path.display()
            );
            path.clone()
        };
        let current = before.as_deref().unwrap_or_default();
        let ready = current
            .windows(body.len())
            .any(|part| part == body.as_bytes());
        if !ready {
            ensure!(
                !current.windows(BEGIN.len()).any(|p| p == BEGIN.as_bytes())
                    && !current.windows(END.len()).any(|p| p == END.as_bytes()),
                "В {} уже есть другой блок LocalVox; он не изменён",
                path.display()
            );
            ensure!(
                !exclusive || current.is_empty(),
                "Файл {} уже существует и не принадлежит LocalVox",
                path.display()
            );
        }
        let mut after = current.to_vec();
        if !ready {
            if !after.is_empty() && !after.ends_with(b"\n") {
                after.push(b'\n');
            }
            after.extend_from_slice(body.as_bytes());
        }
        Ok(Self {
            path,
            destination,
            before,
            after,
        })
    }

    pub(super) fn changed(&self) -> bool {
        self.before.as_deref().unwrap_or_default() != self.after
    }

    pub(super) fn apply(&self) -> Result<bool> {
        if !self.changed() {
            return Ok(false);
        }
        let verify = || -> Result<()> {
            ensure!(
                read_optional(&self.path)? == self.before,
                "Профиль {} изменился; повторите установку",
                self.path.display()
            );
            if self.before.is_some() {
                ensure!(
                    self.path.canonicalize()? == self.destination,
                    "Ссылка профиля изменилась"
                );
            } else {
                ensure!(
                    fs::symlink_metadata(&self.path).is_err(),
                    "Профиль появился после проверки"
                );
            }
            Ok(())
        };
        verify()?;
        let parent = self.destination.parent().context("Каталог профиля")?;
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".localvox-profile-")
            .tempfile_in(parent)?;
        temporary.write_all(&self.after)?;
        if self.before.is_some() {
            temporary
                .as_file()
                .set_permissions(fs::metadata(&self.destination)?.permissions())?;
        }
        temporary.as_file().sync_all()?;
        verify()?;
        temporary
            .persist(&self.destination)
            .map_err(|e| e.error)
            .with_context(|| format!("Сохранение {}", self.path.display()))?;
        Ok(true)
    }
}

#[cfg(not(windows))]
fn plan(directory: &Path) -> Result<Vec<Edit>> {
    let home = PathBuf::from(std::env::var_os("HOME").context("Нет HOME; задайте PATH вручную")?);
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh"
        } else {
            "/bin/bash"
        }
        .into()
    });
    let env_path = |key| {
        std::env::var_os(key)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    };
    targets(
        &home,
        &shell,
        env_path("ZDOTDIR"),
        env_path("XDG_CONFIG_HOME"),
    )?
    .into_iter()
    .map(|(path, shell)| {
        Edit::plan(
            path,
            &block(directory, shell)?,
            matches!(shell, Shell::Fish),
        )
    })
    .collect()
}

#[cfg(not(windows))]
pub(super) fn configured(directory: &Path) -> Result<bool> {
    Ok(plan(directory)?.iter().all(|e| !e.changed()))
}

#[cfg(not(windows))]
pub(super) fn install(directory: &Path) -> Result<bool> {
    let edits = plan(directory)?;
    let mut changed = false;
    for edit in edits {
        changed |= edit.apply()?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests;
