//! Slots and integrations (F2/F5, WP-B3).
//!
//! A **slot** is the voice name of a destination («запиши в слот идеи»): name +
//! aliases + the default flag. An **integration** is where and how the note
//! physically goes; a slot references exactly one of them. V1 adapters (owner's
//! decision 2026-07-11):
//! - `files` — a file or a folder on disk (an Obsidian vault = just files):
//!   a path to an `.md` file → append a line built from the template; a path to a
//!   folder → a new file per note;
//! - `mcp` — a universal MCP client (stdio): Obsidian-MCP, Notion and any other
//!   server, without writing separate integrations (see `mcp.rs`).
//!
//! The crate does not depend on core (P5 — detachability). Consumers: the CLI
//! `localvox-note` today, the voice module (WP-B2) and routing (F5) later.
//! How to configure it — `docs/integrations.md` and `slots.example.toml`.

pub mod mcp;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// Where the note physically goes. Implementations: [`FsIntegration`],
/// [`mcp::McpIntegration`]. `Send + Sync` — the slot registry lives in the voice
/// module thread.
pub trait Integration: Send + Sync {
    /// Writes the note; returns a human-readable «where to» (path/tool).
    fn write_note(&self, text: &str) -> Result<String>;
}

// ─────────────────────────── config ───────────────────────────

#[derive(Deserialize)]
struct ConfigFile {
    #[serde(default)]
    slots: BTreeMap<String, SlotConfig>,
}

/// Flat `[slots."name"]` section: the slot fields + the integration fields picked by `type`.
#[derive(Deserialize, Clone)]
pub struct SlotConfig {
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub default: bool,
    /// Free-form description of «what goes here» — a hint for LLM routing (F5).
    #[serde(default)]
    pub description: String,

    /// files | mcp
    #[serde(default = "default_type", rename = "type")]
    pub kind: String,

    // ── files ──
    /// An `.md` file (append) or a folder (a file per note).
    pub path: Option<PathBuf>,
    /// For a file — the line to write; for a folder — the contents of the new file.
    /// Placeholders: {{text}}, {{date}}, {{time}}.
    #[serde(default = "default_template")]
    pub template: String,

    // ── mcp ──
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// Name of the MCP tool (for example, `append_content`).
    pub tool: Option<String>,
    /// Which tool argument the note text goes into.
    #[serde(default = "default_text_arg")]
    pub text_arg: String,
    /// Constant tool arguments (the note path and the like).
    #[serde(default)]
    pub static_args: BTreeMap<String, toml::Value>,
}

fn default_type() -> String {
    "files".into()
}
fn default_template() -> String {
    "- {{date}} {{time}} — {{text}}".into()
}
fn default_text_arg() -> String {
    "content".into()
}

// ─────────────────────────── files adapter ───────────────────────────

pub struct FsIntegration {
    path: PathBuf,
    template: String,
}

impl FsIntegration {
    pub fn new(path: PathBuf, template: String) -> Self {
        Self { path, template }
    }

    fn render(&self, text: &str) -> String {
        let now = chrono::Local::now();
        self.template
            .replace("{{text}}", text)
            .replace("{{date}}", &now.format("%Y-%m-%d").to_string())
            .replace("{{time}}", &now.format("%H:%M").to_string())
    }

    /// A folder — if the path exists as a directory or has no extension.
    fn is_dir_target(&self) -> bool {
        self.path.is_dir() || self.path.extension().is_none()
    }
}

impl Integration for FsIntegration {
    fn write_note(&self, text: &str) -> Result<String> {
        let text = text.trim();
        if text.is_empty() {
            bail!("empty note");
        }
        let line = self.render(text);

        if self.is_dir_target() {
            // folder: a new file per note — `HHMMSS-first-words.md`
            fs::create_dir_all(&self.path)
                .with_context(|| format!("creating {}", self.path.display()))?;
            let now = chrono::Local::now();
            let slug: String = text
                .split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join("-")
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' {
                        c
                    } else {
                        '-'
                    }
                })
                .take(40)
                .collect();
            let file = self
                .path
                .join(format!("{}-{slug}.md", now.format("%Y%m%d-%H%M%S")));
            fs::write(&file, line + "\n")?;
            Ok(file.display().to_string())
        } else {
            // file: append a line
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut f = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .with_context(|| format!("opening {}", self.path.display()))?;
            let needs_newline = fs::metadata(&self.path)
                .map(|m| m.len() > 0)
                .unwrap_or(false)
                && !ends_with_newline(&self.path);
            if needs_newline {
                writeln!(f)?;
            }
            writeln!(f, "{line}")?;
            Ok(self.path.display().to_string())
        }
    }
}

fn ends_with_newline(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = fs::File::open(path) else {
        return true;
    };
    if f.seek(SeekFrom::End(-1)).is_err() {
        return true;
    }
    let mut b = [0u8; 1];
    f.read_exact(&mut b).map(|()| b[0] == b'\n').unwrap_or(true)
}

// ─────────────────────────── slot registry ───────────────────────────

pub struct Slot {
    pub name: String,
    pub aliases: Vec<String>,
    pub default: bool,
    /// Description for LLM routing (F5); empty — the name+aliases are used.
    pub description: String,
    integration: Box<dyn Integration>,
}

impl Slot {
    pub fn write_note(&self, text: &str) -> Result<String> {
        self.integration.write_note(text)
    }

    fn from_config(name: String, c: SlotConfig) -> Result<Self> {
        let integration: Box<dyn Integration> = match c.kind.as_str() {
            "files" => {
                let path = c
                    .path
                    .clone()
                    .with_context(|| format!("slot «{name}»: type=\"files\" needs a path"))?;
                Box::new(FsIntegration::new(path, c.template.clone()))
            }
            "mcp" => Box::new(mcp::McpIntegration::from_config(&name, &c)?),
            other => bail!("slot «{name}»: unknown type «{other}» (files | mcp)"),
        };
        Ok(Self {
            name,
            aliases: c.aliases,
            default: c.default,
            description: c.description,
            integration,
        })
    }
}

pub struct SlotRegistry {
    slots: Vec<Slot>,
}

impl SlotRegistry {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading the slot config {}", path.display()))?;
        let cfg: ConfigFile =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if cfg.slots.is_empty() {
            bail!("{} has no slots at all [slots.\"name\"]", path.display());
        }
        let defaults = cfg.slots.values().filter(|s| s.default).count();
        if defaults > 1 {
            bail!("default = true may be set on one slot only (currently {defaults})");
        }
        Ok(Self {
            slots: cfg
                .slots
                .into_iter()
                .map(|(name, c)| Slot::from_config(name, c))
                .collect::<Result<Vec<_>>>()?,
        })
    }

    /// Config path: explicit argument → env LOCALVOX_SLOTS_CONFIG → ./slots.toml →
    /// slots.toml next to the exe (portable install: cwd ≠ the program's directory).
    pub fn default_config_path(explicit: Option<&Path>) -> PathBuf {
        if let Some(p) = explicit {
            return p.to_path_buf();
        }
        if let Ok(p) = std::env::var("LOCALVOX_SLOTS_CONFIG") {
            return PathBuf::from(p);
        }
        let cwd = PathBuf::from("slots.toml");
        if cwd.exists() {
            return cwd;
        }
        if let Some(exe_dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
        {
            let near_exe = exe_dir.join("slots.toml");
            if near_exe.exists() {
                return near_exe;
            }
        }
        cwd
    }

    /// Lookup by name/alias (case-insensitive).
    pub fn resolve(&self, query: &str) -> Option<&Slot> {
        let q = query.trim().to_lowercase();
        self.slots.iter().find(|s| {
            s.name.to_lowercase() == q || s.aliases.iter().any(|a| a.trim().to_lowercase() == q)
        })
    }

    pub fn default_slot(&self) -> Option<&Slot> {
        self.slots
            .iter()
            .find(|s| s.default)
            .or_else(|| self.slots.first())
    }

    /// All slots (the voice grammar needs the names/aliases).
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    pub fn names(&self) -> Vec<&str> {
        self.slots.iter().map(|s| s.name.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn load(dir: &Path, cfg: &str) -> SlotRegistry {
        let p = dir.join("slots.toml");
        fs::write(&p, cfg).unwrap();
        SlotRegistry::load(&p).unwrap()
    }

    #[test]
    fn file_append_with_aliases_and_default() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("v/ideas.md");
        let cfg = format!(
            "[slots.\"идеи\"]\naliases = [\"в идеи\"]\npath = \"{}\"\ntemplate = \"- {{{{text}}}}\"\ndefault = true\n",
            file.display().to_string().replace('\\', "/")
        );
        let reg = load(dir.path(), &cfg);
        assert_eq!(reg.resolve("В ИДЕИ").unwrap().name, "идеи");
        assert_eq!(reg.default_slot().unwrap().name, "идеи");
        reg.resolve("идеи").unwrap().write_note("раз").unwrap();
        reg.resolve("идеи").unwrap().write_note("два").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "- раз\n- два\n");
    }

    #[test]
    fn folder_target_creates_file_per_note() {
        let dir = tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        let cfg = format!(
            "[slots.\"входящие\"]\npath = \"{}\"\ntemplate = \"{{{{text}}}}\"\n",
            inbox.display().to_string().replace('\\', "/")
        );
        let reg = load(dir.path(), &cfg);
        let dest = reg
            .resolve("входящие")
            .unwrap()
            .write_note("проверить sherpa onnx")
            .unwrap();
        assert!(dest.ends_with(".md"), "{dest}");
        let files: Vec<_> = fs::read_dir(&inbox).unwrap().flatten().collect();
        assert_eq!(files.len(), 1);
        let body = fs::read_to_string(files[0].path()).unwrap();
        assert_eq!(body, "проверить sherpa onnx\n");
    }

    #[test]
    fn files_without_path_is_config_error() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("slots.toml");
        fs::write(&p, "[slots.\"пустой\"]\n").unwrap();
        let err = SlotRegistry::load(&p).map(|_| ()).unwrap_err().to_string();
        assert!(err.contains("needs a path"), "{err}");
    }

    #[test]
    fn unknown_type_rejected() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("slots.toml");
        fs::write(&p, "[slots.\"x\"]\ntype = \"carrier-pigeon\"\n").unwrap();
        assert!(SlotRegistry::load(&p).map(|_| ()).is_err());
    }
}
