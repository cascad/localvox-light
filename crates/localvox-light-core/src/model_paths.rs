//! Read-only model discovery shared by runtime, doctor, setup and installed downloaders.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::OnceLock,
};

#[derive(Deserialize)]
struct Manifest {
    storage: Storage,
    components: Vec<Component>,
}
#[derive(Deserialize)]
struct Storage {
    user_directory: String,
}

#[derive(Deserialize)]
pub struct Component {
    pub id: String,
    pub env: String,
    pub directory: PathBuf,
    #[serde(default)]
    sibling_of: Option<String>,
    items: Vec<Item>,
}
#[derive(Deserialize)]
struct Item {
    kind: String,
    dest: Option<PathBuf>,
    unpack_to: Option<PathBuf>,
    marker: Option<PathBuf>,
}

fn manifest() -> &'static Manifest {
    static MANIFEST: OnceLock<Manifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(include_str!("../../../scripts/models.json"))
            .expect("invalid bundled model placement manifest")
    })
}
pub fn components() -> &'static [Component] {
    &manifest().components
}
pub fn component(id: &str) -> &'static Component {
    components()
        .iter()
        .find(|c| c.id == id)
        .expect("unknown model component")
}

pub fn normalize(value: &str) -> &str {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value[1..value.len() - 1].trim()
    } else {
        value
    }
}

impl Component {
    /// Same manifest payload presence check as fetch-models, without loading weights.
    pub fn ready(&self, path: &Path) -> bool {
        !path.as_os_str().is_empty()
            && !self.items.is_empty()
            && self.items.iter().all(|item| match item.kind.as_str() {
                "file" => item
                    .dest
                    .as_deref()
                    .and_then(|p| p.strip_prefix(&self.directory).ok())
                    .is_some_and(|relative| path.join(relative).is_file()),
                "zip" => {
                    item.unpack_to.as_ref() == Some(&self.directory)
                        && item
                            .marker
                            .as_ref()
                            .is_some_and(|marker| path.join(marker).is_file())
                }
                "text" => true,
                _ => false,
            })
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Explicit,
    Asr,
    Portable,
    Cwd,
    User,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct Location {
    pub id: String,
    pub env: String,
    pub path: Option<PathBuf>,
    pub source: Source,
    pub ready: bool,
}

/// Inputs are explicit so tests never need the developer's home or global env mutation.
pub struct Context {
    pub home: Option<PathBuf>,
    pub exe: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub overrides: BTreeMap<String, String>,
}
impl Context {
    pub fn capture() -> Self {
        Self {
            home: std::env::home_dir().filter(|p| !p.as_os_str().is_empty()),
            exe: std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf)),
            cwd: std::env::current_dir().ok(),
            overrides: components()
                .iter()
                .filter_map(|c| {
                    std::env::var(&c.env)
                        .ok()
                        .map(|value| (c.env.clone(), value))
                })
                .collect(),
        }
    }
    pub fn user_root(&self) -> Option<PathBuf> {
        self.home
            .as_ref()
            .filter(|home| home.is_absolute())
            .map(|home| home.join(&manifest().storage.user_directory))
    }
    fn absolute(&self, path: &Path) -> Option<PathBuf> {
        if path.as_os_str().is_empty() || path.is_absolute() {
            Some(path.into())
        } else {
            self.cwd.as_ref().map(|cwd| cwd.join(path))
        }
    }
    pub fn resolve(&self, c: &Component, asr: Option<&Path>) -> Location {
        let location = |path: Option<PathBuf>, source| Location {
            id: c.id.clone(),
            env: c.env.clone(),
            ready: path.as_deref().is_some_and(|p| c.ready(p)),
            path,
            source,
        };
        if let Some(value) = self.overrides.get(&c.env) {
            return location(self.absolute(Path::new(normalize(value))), Source::Explicit);
        }
        if let Some(parent) = &c.sibling_of {
            let default_parent = self.resolve(component(parent), None).path;
            for asr in [asr, default_parent.as_deref()].into_iter().flatten() {
                if let Some(path) = c
                    .directory
                    .file_name()
                    .and_then(|name| asr.parent().map(|parent| parent.join(name)))
                    .filter(|p| p.is_dir())
                {
                    return location(Some(path), Source::Asr);
                }
            }
        }
        for (base, source) in [(&self.exe, Source::Portable), (&self.cwd, Source::Cwd)] {
            if let Some(path) = base
                .as_ref()
                .map(|base| base.join(&c.directory))
                .filter(|p| p.is_dir())
            {
                return location(Some(path), source);
            }
        }
        match self.user_root() {
            Some(root) => location(Some(root.join(&c.directory)), Source::User),
            None => location(None, Source::Unavailable),
        }
    }
    pub fn locations(&self) -> Vec<Location> {
        components().iter().map(|c| self.resolve(c, None)).collect()
    }
    pub fn side(&self, env: &str, directory: &str, asr: Option<&Path>) -> Location {
        if let Some(c) = components().iter().find(|c| c.env == env) {
            self.resolve(c, asr)
        } else {
            self.resolve(
                &Component {
                    id: directory.into(),
                    env: env.into(),
                    directory: Path::new("models").join(directory),
                    sibling_of: Some("gigaam".into()),
                    items: Vec::new(),
                },
                asr,
            )
        }
    }
}

pub fn default_dir(id: &str) -> PathBuf {
    Context::capture()
        .resolve(component(id), None)
        .path
        .unwrap_or_default()
}
pub fn default_vosk() -> String {
    default_dir("vosk").to_string_lossy().into_owned()
}

pub fn report(vosk_override: Option<&str>) -> serde_json::Value {
    let context = Context::capture();
    let mut locations = context.locations();
    let vosk_location = locations.iter_mut().find(|c| c.id == "vosk").unwrap();
    if let Some(vosk) = vosk_override {
        vosk_location.path = context.absolute(Path::new(normalize(vosk)));
        vosk_location.source = Source::Explicit;
        vosk_location.ready = vosk_location
            .path
            .as_deref()
            .is_some_and(|p| component("vosk").ready(p));
    }
    serde_json::json!({"schema": 1, "user_root": context.user_root(), "components": locations})
}

#[cfg(test)]
mod tests;
