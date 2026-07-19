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

/// One note read back out of a destination.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredNote {
    /// The note as a person would read it — the markdown bullet and the date prefix removed.
    pub text: String,
    /// The date the line carried, if it carried one. Shown as its OWN column, the way a session
    /// shows its day: a date belongs beside the note, not inside its sentence.
    pub date: Option<String>,
    /// THE LINE AS IT ACTUALLY IS IN THE FILE. Deletion matches on this, never on the cleaned
    /// text: what we show is a rendering, and deleting by a rendering would either miss the line
    /// or, worse, match a different one.
    pub raw: String,
    /// Which file it physically lives in. What makes the list checkable rather than merely
    /// reassuring: the person can open that file and see the same line.
    pub source: String,
}

/// Split a stored line into «when» and «what».
///
/// The file keeps the full line — the template is the owner's own choice of how his vault looks,
/// and for a line in an append-only file the date is THE ONLY record of when the note was made.
/// Stripping it from the file would destroy that; stripping it from the DISPLAY only removes noise.
///
/// Conservative on purpose: a leading markdown bullet or task box, then an ISO date, then a
/// separator. Anything it does not recognise passes through untouched — a parser that guesses would
/// eventually eat someone's actual words.
fn split_date(line: &str) -> (Option<String>, String) {
    let mut rest = line.trim();
    for marker in ["- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "+ "] {
        if let Some(r) = rest.strip_prefix(marker) {
            rest = r.trim_start();
            break;
        }
    }
    // `YYYY-MM-DD` and nothing looser: a bare number or a partial date is somebody's text.
    let is_iso = rest.len() >= 10
        && rest.as_bytes()[..10]
            .iter()
            .enumerate()
            .all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() });
    if !is_iso {
        return (None, rest.to_string());
    }
    let (date, tail) = rest.split_at(10);
    let tail = tail.trim_start();
    // The separator the template puts between the date and the text. Without one, the date is
    // simply how the sentence begins, and we leave it alone.
    let tail = ["—", "-", "–", ":", "|"]
        .iter()
        .find_map(|s| tail.strip_prefix(*s))
        .map(str::trim_start)
        .unwrap_or(tail);
    (Some(date.to_string()), tail.to_string())
}

/// Where the note physically goes. Implementations: [`FsIntegration`],
/// [`mcp::McpIntegration`]. `Send + Sync` — the slot registry lives in the voice
/// module thread.
pub trait Integration: Send + Sync {
    /// Writes the note; returns a human-readable «where to» (path/tool).
    fn write_note(&self, text: &str) -> Result<String>;

    /// Read recent notes back, NEWEST FIRST.
    ///
    /// Not every destination can be read: an MCP server we only send to has no listing. The default
    /// says so out loud rather than returning an empty list — «нельзя прочитать» and «у вас нет
    /// идей» are opposite facts, and showing the second when the first is true is the kind of quiet
    /// lie this system is built to avoid.
    fn read_notes(&self, _limit: usize) -> Result<Vec<StoredNote>> {
        bail!("это назначение нельзя прочитать обратно — сюда можно только писать")
    }

    /// Whether `read_notes` will work. Lets the screen offer the list only where it exists.
    fn can_read(&self) -> bool {
        false
    }

    /// Remove a note. Identified by the text AND the file it came from, never by index: the file is
    /// the person's own, they edit it by hand, and a position captured a second ago may already
    /// point at a different line.
    fn delete_note(&self, _note: &StoredNote) -> Result<()> {
        bail!("из этого назначения нельзя удалять")
    }
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

    fn can_read(&self) -> bool {
        true
    }

    /// Delete a note.
    ///
    /// THIS TOUCHES THE PERSON'S OWN VAULT, so the target is verified before anything is removed:
    /// the file must lie INSIDE the slot's configured path. Without that check a crafted `source`
    /// would turn this endpoint into "delete any file on the machine" — the slot's path is the
    /// boundary of what this integration is allowed to touch, and it is enforced here rather than
    /// trusted from the caller.
    ///
    /// A folder slot deletes the file; a file slot removes the LINE. The line is matched by its
    /// exact text, not by an index: the file is hand-edited too, and a position read a second ago
    /// may already point somewhere else. If the text is no longer there, that is not an error —
    /// it is already gone, which is what was asked for.
    fn delete_note(&self, note: &StoredNote) -> Result<()> {
        let target = PathBuf::from(&note.source);
        if self.is_dir_target() {
            let root = self
                .path
                .canonicalize()
                .with_context(|| format!("slot folder {}", self.path.display()))?;
            let file = target
                .canonicalize()
                .with_context(|| format!("note file {}", target.display()))?;
            if !file.starts_with(&root) {
                bail!("эта заметка не из слота — удалять отказываюсь");
            }
            fs::remove_file(&file).with_context(|| format!("removing {}", file.display()))?;
            return Ok(());
        }

        if target != self.path {
            bail!("эта заметка не из слота — удалять отказываюсь");
        }
        let body = fs::read_to_string(&self.path)
            .with_context(|| format!("reading {}", self.path.display()))?;
        // MATCHED ON THE RAW LINE, never on the cleaned text. What the screen shows is a rendering
        // — bullet and date stripped — and deleting by a rendering would either miss the line or
        // match a different one that happens to render the same way.
        let wanted = note.raw.trim();
        let mut removed = false;
        let kept: Vec<&str> = body
            .lines()
            .filter(|l| {
                // Only the FIRST match goes: two identical lines are two separate notes, and
                // deleting both when one was asked for would destroy something not selected.
                if !removed && l.trim() == wanted {
                    removed = true;
                    return false;
                }
                true
            })
            .collect();
        if !removed {
            return Ok(()); // already gone — the desired state, not a failure
        }
        let mut out = kept.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        fs::write(&self.path, out).with_context(|| format!("writing {}", self.path.display()))?;
        Ok(())
    }

    /// Newest first, because that is the order a person looks: the thing just dictated is the thing
    /// they want to confirm landed.
    ///
    /// A destination that does not exist yet is an EMPTY list, not an error — nobody has written
    /// there so far, which is a perfectly ordinary state and not a fault to report.
    fn read_notes(&self, limit: usize) -> Result<Vec<StoredNote>> {
        if self.is_dir_target() {
            if !self.path.is_dir() {
                return Ok(Vec::new());
            }
            // One file per note, named `YYYYMMDD-HHMMSS-…`, so the name sorts by time.
            let mut files: Vec<PathBuf> = fs::read_dir(&self.path)
                .with_context(|| format!("reading {}", self.path.display()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file())
                .collect();
            files.sort();
            Ok(files
                .iter()
                .rev()
                .take(limit)
                .filter_map(|p| {
                    let body = fs::read_to_string(p).ok()?;
                    let body = body.trim();
                    if body.is_empty() {
                        return None;
                    }
                    let (date, text) = split_date(body);
                    Some(StoredNote {
                        text,
                        date,
                        raw: body.to_string(),
                        source: p.display().to_string(),
                    })
                })
                .collect())
        } else {
            if !self.path.is_file() {
                return Ok(Vec::new());
            }
            let body = fs::read_to_string(&self.path)
                .with_context(|| format!("reading {}", self.path.display()))?;
            let source = self.path.display().to_string();
            Ok(body
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .rev()
                .take(limit)
                .map(|l| {
                    let (date, text) = split_date(l);
                    StoredNote {
                        text,
                        date,
                        raw: l.to_string(),
                        source: source.clone(),
                    }
                })
                .collect())
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

    /// Recent notes, newest first — «полистать посмотреть».
    pub fn read_notes(&self, limit: usize) -> Result<Vec<StoredNote>> {
        self.integration.read_notes(limit)
    }

    pub fn can_read(&self) -> bool {
        self.integration.can_read()
    }

    pub fn delete_note(&self, note: &StoredNote) -> Result<()> {
        self.integration.delete_note(note)
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

    /// «Раздел со списком идей, чтобы можно было полистать посмотреть». Newest first, because the
    /// note just dictated is the one the person is looking to confirm.
    #[test]
    fn notes_are_read_back_newest_first() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("ideas.md");
        let slot = FsIntegration::new(file, "- {{text}}".into());
        slot.write_note("первая").unwrap();
        slot.write_note("вторая").unwrap();
        slot.write_note("третья").unwrap();

        let got = slot.read_notes(10).unwrap();
        // `text` is what a person reads — the bullet is formatting, not words.
        assert_eq!(
            got.iter().map(|n| n.text.as_str()).collect::<Vec<_>>(),
            vec!["третья", "вторая", "первая"]
        );
        // `raw` is the file's own line, and it keeps the format the owner chose.
        assert_eq!(got[0].raw, "- третья");
        assert!(got[0].source.ends_with("ideas.md"), "the note must be traceable to its file");
    }

    /// The list is capped, and the cap keeps the NEWEST — a limit that returned the oldest notes
    /// would be worse than no limit at all.
    #[test]
    fn the_limit_keeps_the_newest_notes() {
        let dir = tempdir().unwrap();
        let slot = FsIntegration::new(dir.path().join("ideas.md"), "{{text}}".into());
        for i in 1..=5 {
            slot.write_note(&format!("заметка {i}")).unwrap();
        }
        let got = slot.read_notes(2).unwrap();
        assert_eq!(
            got.iter().map(|n| n.text.as_str()).collect::<Vec<_>>(),
            vec!["заметка 5", "заметка 4"]
        );
    }

    /// A folder slot: one file per note, and the timestamped names carry the order.
    #[test]
    fn a_folder_slot_reads_its_files_back() {
        let dir = tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        let slot = FsIntegration::new(inbox, "{{text}}".into());
        slot.write_note("одна мысль").unwrap();
        let got = slot.read_notes(10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "одна мысль");
    }

    /// Nobody has written there yet — an EMPTY list, not an error. «Ничего ещё не записано» is an
    /// ordinary state, and reporting it as a failure would put a red error on a fresh install.
    #[test]
    fn a_destination_that_does_not_exist_yet_reads_as_empty() {
        let dir = tempdir().unwrap();
        let slot = FsIntegration::new(dir.path().join("nothing-here.md"), "{{text}}".into());
        assert_eq!(slot.read_notes(10).unwrap(), Vec::new());
    }

    /// THE DATE IS METADATA, NOT PART OF THE SENTENCE. The file keeps the whole line — for a line
    /// in an append-only file the date is the only record of WHEN it was written, and deleting it
    /// from the file would destroy that. The screen gets it as its own column, the way a session
    /// shows its day.
    #[test]
    fn the_bullet_and_the_date_are_metadata_not_text() {
        let dir = tempdir().unwrap();
        let slot = FsIntegration::new(dir.path().join("ideas.md"), "- [ ] {{date}} — {{text}}".into());
        slot.write_note("купить кофе").unwrap();

        let n = &slot.read_notes(10).unwrap()[0];
        assert_eq!(n.text, "купить кофе", "the checkbox and date leaked into the text");
        assert!(n.date.is_some(), "the date was lost instead of moved");
        // The file is untouched: the person's vault keeps the format they chose.
        assert!(n.raw.starts_with("- [ ] "), "the file lost its own formatting: {}", n.raw);
        assert!(n.raw.ends_with("— купить кофе"), "{}", n.raw);
    }

    /// A parser that guesses eventually eats someone's actual words. Anything it does not clearly
    /// recognise must pass through whole.
    #[test]
    fn text_that_only_looks_like_a_date_is_left_alone() {
        // No separator — this is how the sentence begins.
        assert_eq!(split_date("2026-07-18 годовой отчёт"), (Some("2026-07-18".into()), "годовой отчёт".into()));
        // Not an ISO date at all.
        assert_eq!(split_date("18.07.2026 — отчёт").0, None);
        assert_eq!(split_date("2026 год был странный").0, None);
        // A bullet with no date: only the bullet goes.
        assert_eq!(split_date("- просто мысль"), (None, "просто мысль".into()));
        // A dash INSIDE the text must survive.
        assert_eq!(split_date("что-то про кое-что").1, "что-то про кое-что");
    }

    /// Deletion matches the RAW line. The screen shows a cleaned rendering, and deleting by that
    /// rendering would miss the line — or match a different one that renders the same way.
    #[test]
    fn deletion_matches_the_raw_line_not_the_rendered_one() {
        let dir = tempdir().unwrap();
        let slot = FsIntegration::new(dir.path().join("ideas.md"), "- [ ] {{date}} — {{text}}".into());
        slot.write_note("купить кофе").unwrap();
        slot.write_note("позвонить в банк").unwrap();

        let note = slot
            .read_notes(10)
            .unwrap()
            .into_iter()
            .find(|n| n.text == "купить кофе")
            .unwrap();
        slot.delete_note(&note).unwrap();

        let left = slot.read_notes(10).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].text, "позвонить в банк");
    }

    /// Deleting a note removes exactly that line and leaves the rest of the file alone — this is
    /// the person's own vault, not our storage.
    #[test]
    fn deleting_a_note_removes_only_that_line() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("ideas.md");
        let slot = FsIntegration::new(file.clone(), "{{text}}".into());
        for t in ["первая", "вторая", "третья"] {
            slot.write_note(t).unwrap();
        }
        let notes = slot.read_notes(10).unwrap();
        let second = notes.iter().find(|n| n.text == "вторая").unwrap().clone();
        slot.delete_note(&second).unwrap();

        let left = slot.read_notes(10).unwrap();
        assert_eq!(
            left.iter().map(|n| n.text.as_str()).collect::<Vec<_>>(),
            vec!["третья", "первая"]
        );
    }

    /// Two identical lines are two separate notes. Deleting one must not take the other with it.
    #[test]
    fn a_duplicate_line_loses_only_one_copy() {
        let dir = tempdir().unwrap();
        let slot = FsIntegration::new(dir.path().join("ideas.md"), "{{text}}".into());
        slot.write_note("купить кофе").unwrap();
        slot.write_note("купить кофе").unwrap();
        let note = slot.read_notes(10).unwrap()[0].clone();
        slot.delete_note(&note).unwrap();
        assert_eq!(slot.read_notes(10).unwrap().len(), 1);
    }

    /// THE SLOT'S PATH IS THE BOUNDARY. A crafted `source` must not turn deletion into «remove any
    /// file on this machine» — the check lives here, not in the caller's good intentions.
    #[test]
    fn a_note_from_outside_the_slot_is_refused() {
        let dir = tempdir().unwrap();
        let slot = FsIntegration::new(dir.path().join("ideas.md"), "{{text}}".into());
        slot.write_note("своя").unwrap();

        let outsider = dir.path().join("secret.txt");
        fs::write(&outsider, "не трогать\n").unwrap();
        let forged = StoredNote {
            text: "не трогать".into(),
            date: None,
            raw: "не трогать".into(),
            source: outsider.display().to_string(),
        };
        assert!(slot.delete_note(&forged).is_err(), "deleted a file outside the slot");
        assert!(outsider.exists(), "a file outside the slot was destroyed");
    }

    /// Already gone is the desired state, not a failure — a second click must not raise an error.
    #[test]
    fn deleting_something_that_is_no_longer_there_is_not_an_error() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("ideas.md");
        let slot = FsIntegration::new(file.clone(), "{{text}}".into());
        slot.write_note("одна").unwrap();
        let note = slot.read_notes(10).unwrap()[0].clone();
        slot.delete_note(&note).unwrap();
        slot.delete_note(&note).unwrap();
        assert!(slot.read_notes(10).unwrap().is_empty());
    }

    /// A folder slot stores one file per note, so deleting the note deletes that file.
    #[test]
    fn a_folder_slot_deletes_the_note_file() {
        let dir = tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        let slot = FsIntegration::new(inbox.clone(), "{{text}}".into());
        slot.write_note("одна мысль").unwrap();
        let note = slot.read_notes(10).unwrap()[0].clone();
        slot.delete_note(&note).unwrap();
        assert!(slot.read_notes(10).unwrap().is_empty());
        assert!(!PathBuf::from(&note.source).exists());
    }

    /// «Нельзя прочитать» and «у вас нет идей» are opposite facts. A destination we can only write
    /// to must say the first, never quietly show the second.
    #[test]
    fn a_write_only_destination_says_so_instead_of_showing_an_empty_list() {
        struct WriteOnly;
        impl Integration for WriteOnly {
            fn write_note(&self, _t: &str) -> Result<String> {
                Ok("ушло".into())
            }
        }
        assert!(!WriteOnly.can_read());
        assert!(WriteOnly.read_notes(10).is_err(), "an empty list would read as «нет идей»");
    }
}
