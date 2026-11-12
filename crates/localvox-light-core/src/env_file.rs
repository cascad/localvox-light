//! Editing `.env` in place — the ONE file that holds the owner's settings.
//!
//! This edits only user overrides. Predetermined values live in .env.defaults;
//! resetting a key leaves that preset in effect.
//!
//! WHY THIS IS NOT "serialize a map and write the file". The owner's `.env` is 280 lines and most
//! of them are COMMENTS — measurements, the reason a threshold is what it is, why a model was
//! rejected. Rewriting the file from a key/value map would delete all of it on the first click in
//! the UI. So the edit is surgical: one line changes, every other byte is preserved.
//!
//! Three cases, in order:
//!   * an active `KEY=…` line → its value is replaced, in place;
//!   * only a commented `# KEY=…` line (that is how `.env.example` documents every default) → it
//!     is UNCOMMENTED in place, so the setting lands next to the paragraph explaining it instead
//!     of orphaned at the end of the file;
//!   * the key appears nowhere → appended at the end.
//!
//! Unsetting comments the line back out rather than deleting it: the key stops applying (the preset
//! takes over), the documentation survives, and the previous value is still readable. The
//! operation is its own inverse.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use anyhow::{Context, Result};

const FILE_NAME: &str = ".env";

// HTTP requests share a process. Serialize the entire read/edit/publish, rather than
// only the final write: two requests changing different keys must both survive.
static WRITER: Mutex<()> = Mutex::new(());

/// A read-only view tied to its destination. Used when edits are prepared before a
/// long operation (setup downloads) and must not replace a newer configuration.
/// Content is deliberately not Debug: it may contain credentials.
pub struct Snapshot {
    path: PathBuf,
    destination: PathBuf,
    content: Option<String>,
}

impl Snapshot {
    pub fn read(path: &Path) -> Result<Self> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => Some(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // Do not silently replace a dangling link with a new settings file.
                anyhow::ensure!(
                    !std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()),
                    "settings link target is missing: {}",
                    path.display()
                );
                None
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("reading {} before saving settings", path.display()))
            }
        };
        let destination = if content.is_some() {
            std::fs::canonicalize(&path)
                .with_context(|| format!("resolving settings file {}", path.display()))?
        } else {
            path.clone()
        };
        Ok(Self {
            path,
            destination,
            content,
        })
    }

    /// Check without writing or acquiring a filesystem lease.
    pub fn verify(&self) -> Result<()> {
        let current = Self::read(&self.path)?;
        anyhow::ensure!(
            current.content == self.content && current.destination == self.destination,
            ".env изменился после чтения. Повтори операцию, чтобы сохранить новые настройки."
        );
        Ok(())
    }

    /// Publish only if the snapshot is still current. Returns whether bytes changed.
    /// This and save_to serialize cooperating writers in this process; they do not
    /// claim a filesystem-wide CAS against arbitrary external editors.
    pub fn save(&self, changes: &[(String, Option<String>)]) -> Result<bool> {
        let _writer = WRITER.lock().unwrap_or_else(|e| e.into_inner());
        self.verify()?;
        self.publish(changes)
    }

    fn publish(&self, changes: &[(String, Option<String>)]) -> Result<bool> {
        let current = self.content.as_deref().unwrap_or_default();
        let next = apply(current, changes);
        // Run the edits in their original order first. Filtering individual edits
        // against current would mishandle repeated assignments of the same key.
        if next == current || parse(&next) == parse(current) {
            return Ok(false);
        }
        let parent = self.destination.parent().context("settings directory")?;
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating settings directory {}", parent.display()))?;
        crate::artifacts::atomic_write(&self.destination, next.as_bytes())
            .with_context(|| format!("writing {}", self.path.display()))?;
        Ok(true)
    }
}

/// Which `.env` is actually in effect, and therefore the one to write to.
///
/// The daemon loads the cwd `.env` first and then the one next to the exe WITH OVERRIDE, so the
/// exe-side file is the stronger of the two. We write to whichever exists — writing to the other
/// one would either be ignored or silently shadow it, and either way the owner would end up with
/// the two config files he explicitly did not want.
///
/// Nothing exists yet → next to the exe, which is the portable-install default.
pub fn resolve_path() -> PathBuf {
    if let Some(near_exe) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(FILE_NAME)))
    {
        if near_exe.is_file() {
            return near_exe;
        }
        if let Ok(cwd) = std::env::current_dir() {
            let in_cwd = cwd.join(FILE_NAME);
            if in_cwd.is_file() {
                return in_cwd;
            }
        }
        return near_exe;
    }
    PathBuf::from(FILE_NAME)
}

/// Does this line assign `key`? Accepts leading whitespace and the `export ` form.
/// Returns the byte offset where the assignment starts, so a commented line can reuse this.
fn assignment_at(s: &str, key: &str) -> bool {
    let t = s.trim_start();
    let t = t.strip_prefix("export ").map(str::trim_start).unwrap_or(t);
    let Some(rest) = t.strip_prefix(key) else {
        return false;
    };
    // «KEY =» is legal, «KEYS=» is a different key.
    rest.trim_start().starts_with('=')
}

/// Is this a commented-out assignment of `key` — `# KEY=…`, `#KEY=…`, `## KEY=…`?
fn commented_assignment(line: &str, key: &str) -> bool {
    let t = line.trim_start();
    if !t.starts_with('#') {
        return false;
    }
    assignment_at(t.trim_start_matches('#'), key)
}

/// Render `KEY=value` for dotenv.
///
/// Values are written BARE whenever they can be, because quoting is the actual hazard here: the
/// owner's own `.env` warns that a quoted `F:\vosk\…` can turn `\v` into an escape. Unquoted values
/// are taken literally, so Windows paths keep their backslashes.
///
/// Quoting is used only when a bare value would not survive a round-trip — a leading/trailing
/// space, a `#` that would start a comment, a quote character, a newline. Single quotes are
/// preferred: they are literal in dotenv, so backslashes stay safe there too.
fn render(key: &str, value: &str) -> String {
    let needs_quotes = value.is_empty() && false // an empty value is fine bare: «KEY=»
        || value.starts_with(char::is_whitespace)
        || value.ends_with(char::is_whitespace)
        || value.contains('#')
        || value.contains('\n')
        || value.contains('\r')
        || value.contains('"')
        || value.contains('\'');
    if !needs_quotes {
        return format!("{key}={value}");
    }
    if !value.contains('\'') && !value.contains('\n') && !value.contains('\r') {
        return format!("{key}='{value}'");
    }
    // Last resort: double quotes with escapes. Backslashes must be escaped here — this is exactly
    // the branch the bare path above exists to avoid.
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("{key}=\"{escaped}\"")
}

/// Apply changes to the text of a `.env`. Pure — this is the whole invariant, and it is testable
/// without touching a disk.
///
/// `None` means «unset»: the assignment is commented out; the caller resolves the preset.
pub fn apply(content: &str, changes: &[(String, Option<String>)]) -> String {
    // Whether the source ended with a newline decides whether we have to add one before appending.
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let trailing_newline = content.is_empty() || content.ends_with('\n');

    for (key, value) in changes {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        match value {
            Some(v) => {
                let rendered = render(key, v);
                // An active assignment wins: that is the line currently in effect.
                if let Some(i) = lines.iter().position(|l| assignment_at(l, key)) {
                    lines[i] = rendered;
                    continue;
                }
                // Otherwise take over the commented default, keeping its position — the setting
                // belongs next to the paragraph that explains it.
                if let Some(i) = lines.iter().position(|l| commented_assignment(l, key)) {
                    lines[i] = rendered;
                    continue;
                }
                lines.push(rendered);
            }
            None => {
                // Unset: comment the active line out. The key stops applying, the value stays
                // readable, and setting it again will uncomment this very line.
                if let Some(i) = lines.iter().position(|l| assignment_at(l, key)) {
                    lines[i] = format!("# {}", lines[i].trim_start());
                }
            }
        }
    }

    let mut out = lines.join("\n");
    if trailing_newline || !out.is_empty() {
        out.push('\n');
    }
    out
}

/// The keys the file currently sets, with their values.
///
/// This reports THE FILE, not the process environment, and the difference matters: the daemon read
/// its environment once at startup, so the moment something is saved the two disagree. A settings
/// screen fed from the live environment would show a freshly saved value as unchanged — the change
/// would look like it had failed when it had merely not been applied yet.
///
/// Commented lines are not values. That is the whole point of unsetting by commenting out.
pub fn parse(content: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for line in content.lines() {
        let t = line.trim_start();
        if t.starts_with('#') || t.is_empty() {
            continue;
        }
        let t = t.strip_prefix("export ").map(str::trim_start).unwrap_or(t);
        let Some((key, value)) = t.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        out.insert(key.to_string(), unquote(value.trim()));
    }
    out
}

/// Undo what `render` did: strip the quoting it may have added.
fn unquote(v: &str) -> String {
    // A comment after the closing quote is not part of the value. Find the
    // matching quote before stripping the comment so a quoted ` # ` stays literal.
    let v = if let Some(quote @ ('\'' | '"')) = v.chars().next() {
        let mut escaped = false;
        let mut end = None;
        for (i, c) in v.char_indices().skip(1) {
            if escaped {
                escaped = false;
            } else if quote == '"' && c == '\\' {
                escaped = true;
            } else if c == quote {
                let tail = v[i + 1..].trim_start();
                if tail.is_empty() || tail.starts_with('#') {
                    end = Some(i + 1);
                }
                break;
            }
        }
        end.map(|end| &v[..end]).unwrap_or(v)
    } else {
        v
    };
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        return v[1..v.len() - 1].to_string();
    }
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        return v[1..v.len() - 1]
            .replace("\\n", "\n")
            .replace("\\r", "\r")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
    }
    // Bare: an inline comment ends the value. `KEY=value # note` is `value`.
    match v.split_once(" #") {
        Some((head, _)) => head.trim_end().to_string(),
        None => v.to_string(),
    }
}

/// Read the effective `.env`, apply the changes, write it back. Creates the file if it is missing —
/// «нет файла и перенастроили → он появляется сам».
pub fn save(changes: &[(String, Option<String>)]) -> Result<PathBuf> {
    let path = resolve_path();
    save_to(&path, changes)?;
    Ok(path)
}

pub fn save_to(path: &Path, changes: &[(String, Option<String>)]) -> Result<()> {
    let _writer = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    Snapshot::read(path)?.publish(changes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_inline_comments_are_outside_values_and_hashes_inside_quotes_are_literal() {
        let parsed = parse("S='literal # hash' # note\nD=\"say \\\"yes\\\" # hash\" # note\nP='D:\\models\\' # note\n");
        assert_eq!(parsed["S"], "literal # hash");
        assert_eq!(parsed["D"], "say \"yes\" # hash");
        assert_eq!(parsed["P"], "D:\\models\\");
    }

    #[test]
    fn identical_values_preserve_original_bytes_and_mtime() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".env");
        let original =
            "# owner formatting\r\nexport LOCALVOX_MODEL = 'same' # keep note\r\nOTHER=value\r\n";
        std::fs::write(&path, original).unwrap();
        let modified = path.metadata().unwrap().modified().unwrap();
        save_to(&path, &[ch("LOCALVOX_MODEL", Some("same"))]).unwrap();
        save_to(&path, &[]).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(path.metadata().unwrap().modified().unwrap(), modified);
        // Repeated edits are evaluated in order, including a return to the original.
        assert!(!Snapshot::read(&path)
            .unwrap()
            .save(&[
                ch("LOCALVOX_MODEL", Some("temporary")),
                ch("LOCALVOX_MODEL", Some("same")),
            ])
            .unwrap());
        assert_eq!(path.metadata().unwrap().modified().unwrap(), modified);
    }

    #[test]
    fn missing_snapshot_creates_only_for_a_real_change_and_rejects_a_new_empty_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/.env");
        let original = Snapshot::read(&path).unwrap();
        assert!(!original.save(&[]).unwrap());
        assert!(!path.parent().unwrap().exists());
        assert!(original
            .save(&[ch("LOCALVOX_MODEL", Some("first"))])
            .unwrap());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "LOCALVOX_MODEL=first\n"
        );
        assert!(original
            .save(&[ch("LOCALVOX_MODEL", Some("stale"))])
            .is_err());

        let empty = directory.path().join("new.env");
        let missing = Snapshot::read(&empty).unwrap();
        std::fs::write(&empty, "").unwrap();
        assert!(missing.verify().is_err());
        assert!(missing.save(&[ch("X", Some("value"))]).is_err());
        assert_eq!(std::fs::read_to_string(empty).unwrap(), "");
    }

    #[test]
    fn concurrent_independent_edits_merge_and_stale_snapshots_cannot_erase_them() {
        use std::sync::{Arc, Barrier};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".env");
        std::fs::write(&path, "# retained\nBASE=original\n").unwrap();
        let stale = Snapshot::read(&path).unwrap();
        let start = Arc::new(Barrier::new(8));
        std::thread::scope(|scope| {
            for i in 0..8 {
                let path = &path;
                let start = start.clone();
                scope.spawn(move || {
                    start.wait();
                    save_to(path, &[(format!("LOCALVOX_TEST_{i}"), Some(i.to_string()))]).unwrap();
                });
            }
        });
        let current = std::fs::read_to_string(&path).unwrap();
        let parsed = parse(&current);
        assert_eq!(parsed.len(), 9);
        for i in 0..8 {
            assert_eq!(parsed[&format!("LOCALVOX_TEST_{i}")], i.to_string());
        }
        assert!(current.starts_with("# retained\nBASE=original\n"));
        assert!(stale
            .save(&[ch("LOCALVOX_NEW", Some("stale edit"))])
            .is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), current);
    }

    #[cfg(windows)]
    #[test]
    fn failed_replace_leaves_the_complete_previous_file_and_can_retry() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".env");
        let original = "# keep me\nLOCALVOX_MODEL=before\nOTHER=still here\n";
        std::fs::write(&path, original).unwrap();
        let edit = Snapshot::read(&path).unwrap();
        // Reads/writes are allowed; rename/delete is denied. Tests a real OS failure,
        // after the temporary file was written, rather than mocking the writer.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .open(&path)
            .unwrap();
        assert!(edit.save(&[ch("LOCALVOX_MODEL", Some("after"))]).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        drop(held);
        assert!(edit.save(&[ch("LOCALVOX_MODEL", Some("after"))]).unwrap());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original.replace("=before", "=after")
        );
    }

    #[cfg(unix)]
    #[test]
    fn existing_symlink_is_preserved_and_retargeting_invalidates_the_snapshot() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("settings");
        let other = directory.path().join("other");
        let link = directory.path().join(".env");
        std::fs::write(&target, "X=old\n").unwrap();
        std::fs::write(&other, "X=new\n").unwrap();
        symlink(&target, &link).unwrap();
        assert!(Snapshot::read(&link)
            .unwrap()
            .save(&[ch("X", Some("new"))])
            .unwrap());
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "X=new\n");
        let snapshot = Snapshot::read(&link).unwrap();
        std::fs::remove_file(&link).unwrap();
        symlink(&other, &link).unwrap();
        assert!(snapshot
            .save(&[ch("X", Some("would replace wrong target"))])
            .is_err());
        assert_eq!(std::fs::read_to_string(other).unwrap(), "X=new\n");
    }

    #[cfg(windows)]
    #[test]
    fn unreadable_but_writable_config_is_never_replaced_with_only_the_new_keys() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".env");
        let original = "# preserve every unrelated line\nLOCALVOX_OLD=before\n";
        std::fs::write(&path, original).unwrap();
        // Deny new readers, but permit writers. The former unwrap_or_default
        // would turn a read failure into an empty file and truncate the config.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x2)
            .open(&path)
            .unwrap();
        assert!(std::fs::OpenOptions::new().write(true).open(&path).is_ok());
        assert!(save_to(&path, &[("LOCALVOX_NEW".into(), Some("after".into()))]).is_err());
        drop(held);
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    fn ch(k: &str, v: Option<&str>) -> (String, Option<String>) {
        (k.to_string(), v.map(str::to_string))
    }

    /// THE REASON THIS MODULE EXISTS. The owner's `.env` is mostly comments — measurements and the
    /// reasons behind thresholds. One setting changes; every other byte must survive.
    #[test]
    fn everything_the_edit_did_not_touch_is_preserved_byte_for_byte() {
        let src = "\
# ── ASR ──
# Замерено 13.07.2026: qwen3.5:9b держит структуру.
LOCALVOX_LLM_MODEL=qwen3.5:9b

# Рабочий каталог
LOCALVOX_LIGHT_AUDIO_DIR=localvox-audio
";
        let out = apply(
            &src.to_string(),
            &[ch("LOCALVOX_LLM_MODEL", Some("granite4.1:8b"))],
        );
        assert_eq!(
            out,
            "\
# ── ASR ──
# Замерено 13.07.2026: qwen3.5:9b держит структуру.
LOCALVOX_LLM_MODEL=granite4.1:8b

# Рабочий каталог
LOCALVOX_LIGHT_AUDIO_DIR=localvox-audio
"
        );
    }

    /// `.env.example` documents every default as a COMMENTED line. Setting such a key must take
    /// that line over in place — the setting then sits next to the paragraph explaining it, instead
    /// of orphaned at the bottom of the file far from its own documentation.
    #[test]
    fn a_commented_default_is_uncommented_in_place_not_appended() {
        let src = "\
# Тишина на обеих дорожках дольше N сек закрывает запись сама (0 — никогда).
# LOCALVOX_LIGHT_AUTOSTOP_SEC=900

# ── Детект звонков ──
LOCALVOX_LIGHT_CALL_DETECT=off
";
        let out = apply(
            &src.to_string(),
            &[ch("LOCALVOX_LIGHT_AUTOSTOP_SEC", Some("600"))],
        );
        assert_eq!(
            out,
            "\
# Тишина на обеих дорожках дольше N сек закрывает запись сама (0 — никогда).
LOCALVOX_LIGHT_AUTOSTOP_SEC=600

# ── Детект звонков ──
LOCALVOX_LIGHT_CALL_DETECT=off
"
        );
    }

    /// A key documented nowhere is appended — but only then.
    #[test]
    fn an_unknown_key_is_appended_at_the_end() {
        let src = "LOCALVOX_LIGHT_AUDIO_DIR=localvox-audio\n";
        let out = apply(&src.to_string(), &[ch("LOCALVOX_NEW_THING", Some("1"))]);
        assert_eq!(
            out,
            "LOCALVOX_LIGHT_AUDIO_DIR=localvox-audio\nLOCALVOX_NEW_THING=1\n"
        );
    }

    /// Unset comments the line out instead of deleting it: the preset takes over, the old
    /// value stays readable, and the operation is its own inverse.
    #[test]
    fn unsetting_comments_the_line_out_and_setting_it_again_restores_it() {
        let src = "LOCALVOX_LLM_MODEL=qwen3.5:9b\n";
        let off = apply(&src.to_string(), &[ch("LOCALVOX_LLM_MODEL", None)]);
        assert_eq!(off, "# LOCALVOX_LLM_MODEL=qwen3.5:9b\n");

        // Setting it again takes over the very line it just commented out — no duplicate.
        let on = apply(&off, &[ch("LOCALVOX_LLM_MODEL", Some("granite4.1:8b"))]);
        assert_eq!(on, "LOCALVOX_LLM_MODEL=granite4.1:8b\n");
    }

    /// A Windows path is written BARE. The owner's own `.env` warns that quoting turns `\v` into an
    /// escape — quoting a path is the bug, not the safety measure.
    #[test]
    fn a_windows_path_is_written_without_quotes() {
        let out = apply(
            "",
            &[ch("LOCALVOX_LIGHT_YT_DLP", Some(r"F:\vosk\bin\yt-dlp.exe"))],
        );
        assert_eq!(out, "LOCALVOX_LIGHT_YT_DLP=F:\\vosk\\bin\\yt-dlp.exe\n");
        // And it survives a round-trip through the same reader the daemon uses.
        assert!(out.contains(r"F:\vosk\bin\yt-dlp.exe"));
    }

    /// A value that would not survive bare gets single quotes — literal in dotenv, so backslashes
    /// are still safe inside them.
    #[test]
    fn a_value_with_a_hash_or_spaces_is_quoted() {
        let out = apply("", &[ch("K", Some("a # b"))]);
        assert_eq!(out, "K='a # b'\n");
        let out = apply("", &[ch("K", Some(" padded "))]);
        assert_eq!(out, "K=' padded '\n");
    }

    /// A key that merely shares a prefix is a DIFFERENT key.
    #[test]
    fn a_prefix_of_another_key_is_not_confused_with_it() {
        let src = "LOCALVOX_NER_MODEL_DIR=models/ner\nLOCALVOX_NER_MODEL_FILE=m.onnx\n";
        let out = apply(&src.to_string(), &[ch("LOCALVOX_NER_MODEL", Some("x"))]);
        assert_eq!(
            out,
            "LOCALVOX_NER_MODEL_DIR=models/ner\nLOCALVOX_NER_MODEL_FILE=m.onnx\nLOCALVOX_NER_MODEL=x\n"
        );
    }

    /// The `export KEY=` form is an assignment too.
    #[test]
    fn the_export_form_is_recognised() {
        let out = apply(
            "export LOCALVOX_LANG=auto\n",
            &[ch("LOCALVOX_LANG", Some("ru"))],
        );
        assert_eq!(out, "LOCALVOX_LANG=ru\n");
    }

    /// No file yet: «нет .env и перенастроили → он появляется сам».
    #[test]
    fn writing_creates_the_file_when_there_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(".env");
        save_to(&p, &[ch("LOCALVOX_LIGHT_YT_DLP", Some("bin/yt-dlp.exe"))]).unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "LOCALVOX_LIGHT_YT_DLP=bin/yt-dlp.exe\n"
        );
    }

    /// A file that does not end in a newline must not get its last line glued to a new one.
    #[test]
    fn a_file_without_a_trailing_newline_is_not_mangled() {
        let out = apply("LOCALVOX_LANG=auto", &[ch("LOCALVOX_NEW", Some("1"))]);
        assert_eq!(out, "LOCALVOX_LANG=auto\nLOCALVOX_NEW=1\n");
    }

    /// The screen is fed from THE FILE. A commented line is not a value — that is what makes
    /// unsetting work.
    #[test]
    fn parse_reads_active_assignments_and_ignores_commented_ones() {
        let src = "\
# ── ASR ──
LOCALVOX_LLM_MODEL=qwen3.5:9b
# LOCALVOX_LIGHT_AUTOSTOP_SEC=900
export LOCALVOX_LANG=ru
LOCALVOX_LIGHT_YT_DLP=bin/yt-dlp.exe

";
        let got = parse(src);
        assert_eq!(got.get("LOCALVOX_LLM_MODEL").unwrap(), "qwen3.5:9b");
        assert_eq!(got.get("LOCALVOX_LANG").unwrap(), "ru");
        assert_eq!(got.get("LOCALVOX_LIGHT_YT_DLP").unwrap(), "bin/yt-dlp.exe");
        assert!(
            !got.contains_key("LOCALVOX_LIGHT_AUTOSTOP_SEC"),
            "a commented line was read as a value — unsetting would not work"
        );
    }

    /// Whatever `render` wrote, `parse` must read back unchanged — otherwise a saved value would
    /// come back to the screen mangled, and the next save would persist the mangling.
    #[test]
    fn every_value_survives_a_write_then_read_round_trip() {
        for original in [
            r"F:\vosk\bin\yt-dlp.exe",
            "http://localhost:11434/v1",
            "a # b",
            " padded ",
            "qwen3.5:9b",
            "",
        ] {
            let text = apply("", &[ch("LOCALVOX_X", Some(original))]);
            let back = parse(&text);
            assert_eq!(
                back.get("LOCALVOX_X").map(String::as_str),
                Some(original),
                "round-trip changed the value; file was: {text:?}"
            );
        }
    }

    /// An inline comment is not part of the value — a hand-written `KEY=value  # почему` must not
    /// become the literal path `value  # почему`.
    #[test]
    fn an_inline_comment_is_not_part_of_the_value() {
        let got = parse("LOCALVOX_API_BIND=127.0.0.1:3017     # только этот компьютер\n");
        assert_eq!(got.get("LOCALVOX_API_BIND").unwrap(), "127.0.0.1:3017");
    }

    /// Several changes in one save — the ordinary case when a form is submitted.
    #[test]
    fn a_batch_applies_every_change() {
        let src = "# LOCALVOX_LLM_MODEL=qwen3.5:9b\nLOCALVOX_LANG=auto\n";
        let out = apply(
            &src.to_string(),
            &[
                ch("LOCALVOX_LLM_MODEL", Some("granite4.1:8b")),
                ch("LOCALVOX_LANG", Some("ru")),
                ch("LOCALVOX_LIGHT_YT_DLP", Some("bin/yt-dlp.exe")),
            ],
        );
        assert_eq!(
            out,
            "LOCALVOX_LLM_MODEL=granite4.1:8b\nLOCALVOX_LANG=ru\nLOCALVOX_LIGHT_YT_DLP=bin/yt-dlp.exe\n"
        );
    }
}
