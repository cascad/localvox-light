//! Bounded, cursor-based diagnostics over the existing append-only progress file.
//! This is a view of work, never evidence that an output artifact was accepted.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    hash::{Hash, Hasher},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

pub const MAX_SCAN: u64 = 1024 * 1024;
pub const MAX_EVENTS: usize = 200;
pub const MAX_PAGE_BYTES: usize = 256 * 1024;
pub const MAX_TEXT: usize = 4096;
const MAX_VERBOSE: u64 = 8 * 1024 * 1024;
const FILE: &str = "progress.jsonl";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayCommand {
    pub program: String,
    pub args: Vec<DisplayArgument>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayArgument {
    pub name: String,
    pub value: Option<String>,
}
impl DisplayCommand {
    /// Unknown positional arguments, paths, URLs and secret flags never enter the wire data.
    pub fn from_args(program: &str, args: &[String]) -> Self {
        let mut shown = Vec::new();
        let mut i = 0;
        while i < args.len() && shown.len() < 64 {
            let name = args[i].as_str();
            if matches!(
                name,
                "--worker-phase"
                    | "--scope"
                    | "--summary-provider"
                    | "--llm-model"
                    | "-ar"
                    | "-ac"
                    | "-f"
            ) {
                if let Some(value) = args.get(i + 1) {
                    // Model identifiers/phase names only. Never accept arbitrary display values.
                    let value = if value.len() <= 128
                        && value
                            .chars()
                            .all(|c| c.is_alphanumeric() || "._:-".contains(c))
                    {
                        value.clone()
                    } else {
                        "[скрыто]".into()
                    };
                    shown.push(DisplayArgument {
                        name: name.into(),
                        value: Some(value),
                    });
                    i += 2;
                    continue;
                }
            } else if matches!(
                name,
                "--cleanup" | "--refine" | "--force" | "--worker-protocol"
            ) {
                shown.push(DisplayArgument {
                    name: name.into(),
                    value: None,
                });
            }
            i += 1;
        }
        Self {
            program: Path::new(program)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            args: shown,
        }
    }
    fn sanitized(self) -> Self {
        let args = self
            .args
            .into_iter()
            .take(64)
            .flat_map(|a| std::iter::once(a.name).chain(a.value))
            .collect::<Vec<_>>();
        Self::from_args(&safe_text(&self.program), &args)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_sec: Option<f64>,
    pub journal: u8,
    pub id: String,
    pub run: String,
    pub operation: Option<String>,
    pub parent: Option<String>,
    pub stage: String,
    pub kind: String,
    pub at: String,
    pub label: String,
    pub status: Option<String>,
    pub command: Option<DisplayCommand>,
    pub text: Option<String>,
    pub done: Option<u32>,
    pub total: Option<u32>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Metadata {
    run: String,
    verbose_bytes: u64,
    dropped: u64,
    execution: Option<String>,
    active_operation: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Page {
    pub events: Vec<Entry>,
    pub next_cursor: String,
    pub previous_cursor: String,
    pub has_more: bool,
    pub reset: bool,
    pub truncated: bool,
    pub discarded: usize,
    pub read_bytes: u64,
    pub dropped_verbose: u64,
}
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub after: Option<String>,
    pub before: Option<String>,
    pub limit: usize,
}

pub fn safe_text(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '\n'
                || c == '\t'
                || (!c.is_control() && !matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}'))
            {
                c
            } else {
                '�'
            }
        })
        .take(MAX_TEXT)
        .collect()
}
fn metadata(session: &Path) -> Metadata {
    File::open(session.join(".journal-meta.json"))
        .ok()
        .and_then(|f| {
            let mut bytes = Vec::new();
            f.take(4096).read_to_end(&mut bytes).ok()?;
            serde_json::from_slice(&bytes).ok()
        })
        .unwrap_or_default()
}
/// Saved identity only; reading does not create a journal or a new run.
pub fn execution_identity(
    session: &Path,
    execution: Option<&str>,
) -> (Option<String>, Option<String>) {
    let meta = metadata(session);
    let operation = if execution.is_some() && meta.execution.as_deref() == execution {
        meta.active_operation
    } else {
        None
    };
    ((!meta.run.is_empty()).then_some(meta.run), operation)
}
fn save_metadata(session: &Path, m: &Metadata) -> io::Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(session)?;
    serde_json::to_writer(&mut temp, m)?;
    temp.persist(session.join(".journal-meta.json"))
        .map_err(|e| e.error)?;
    Ok(())
}

/// Same cross-process lock for old stage records and new operation records.
pub(crate) fn append_line(session: &Path, line: &[u8]) -> io::Result<()> {
    let mut file = locked_file(session)?;
    file.write_all(line)
}
fn locked_file(session: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(session.join(FILE))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(20);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(e) => {
                if std::time::Instant::now() >= deadline {
                    return Err(io::Error::other(format!("journal append lock: {e}")));
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }
}
pub fn begin_run(session: &Path, run: &str) {
    if let Err(e) = save_metadata(
        session,
        &Metadata {
            run: run.into(),
            ..Default::default()
        },
    ) {
        tracing::debug!("journal run: {e}");
    }
}
pub fn append(session: &Path, mut entry: Entry) -> io::Result<()> {
    let mut file = locked_file(session)?;
    let mut meta = metadata(session);
    entry.journal = 1;
    if entry.run.is_empty() {
        entry.run = meta.run.clone();
    }
    entry.label = safe_text(&entry.label);
    entry.text = entry.text.map(|s| safe_text(&s));
    entry.command = entry.command.map(DisplayCommand::sanitized);
    let mut bytes = serde_json::to_vec(&entry)?;
    bytes.push(b'\n');
    if entry.kind == "log" && !matches!(entry.status.as_deref(), Some("failed" | "invalid")) {
        if meta.verbose_bytes.saturating_add(bytes.len() as u64) > MAX_VERBOSE {
            meta.dropped = meta.dropped.saturating_add(1);
            save_metadata(session, &meta)?;
            return Ok(());
        }
        meta.verbose_bytes += bytes.len() as u64;
        save_metadata(session, &meta)?;
    }
    file.write_all(&bytes)
}
pub struct Operation<'a> {
    session: &'a Path,
    id: String,
    run: String,
    stage: String,
    label: String,
    started: std::time::Instant,
}
impl<'a> Operation<'a> {
    pub fn start(
        session: &'a Path,
        stage: &str,
        label: &str,
        command: Option<DisplayCommand>,
    ) -> Self {
        let op = Self {
            session,
            id: uuid::Uuid::now_v7().to_string(),
            run: metadata(session).run,
            stage: stage.into(),
            label: label.into(),
            started: std::time::Instant::now(),
        };
        op.emit("operation_start", command, None);
        op
    }
    fn emit(&self, kind: &str, command: Option<DisplayCommand>, text: Option<String>) {
        let e = Entry {
            elapsed_sec: (kind == "operation_end").then(|| self.started.elapsed().as_secs_f64()),
            run: self.run.clone(),
            operation: Some(self.id.clone()),
            parent: Some(format!("stage:{}", self.stage)),
            stage: self.stage.clone(),
            kind: kind.into(),
            at: crate::versions::now_rfc3339(),
            label: self.label.clone(),
            command,
            text,
            ..Default::default()
        };
        if let Err(e) = append(self.session, e) {
            tracing::debug!("journal operation: {e}");
        }
    }
    pub fn log(&self, text: &str) {
        self.emit("log", None, Some(safe_text(text)));
    }
    /// Bind to the supervisor's durable phase attempt; a start record alone
    /// never proves that an old operation is still running.
    pub fn bind_execution(&self, execution: &str) -> io::Result<()> {
        let _lock = locked_file(self.session)?;
        let mut meta = metadata(self.session);
        meta.execution = Some(execution.into());
        meta.active_operation = Some(self.id.clone());
        save_metadata(self.session, &meta)
    }
}
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        self.emit(
            "operation_end",
            None,
            Some(format!(
                "Выполнение завершено за {:.3} с; результат проверяется отдельно",
                self.started.elapsed().as_secs_f64()
            )),
        );
        if let Ok(_lock) = locked_file(self.session) {
            let mut meta = metadata(self.session);
            if meta.active_operation.as_deref() == Some(&self.id) {
                meta.active_operation = None;
                let _ = save_metadata(self.session, &meta);
            }
        }
    }
}

fn generation(file: &mut File) -> Result<(String, Vec<u8>)> {
    let identity = same_file::Handle::from_file(file.try_clone()?)?;
    let mut id = std::collections::hash_map::DefaultHasher::new();
    identity.hash(&mut id);
    let mut head = [0; 32];
    let n = file.read(&mut head)?;
    Ok((format!("{:016x}", id.finish()), head[..n].to_vec()))
}
fn cursor(generation: &str, offset: u64, skip: bool) -> String {
    format!("{generation}:{offset}:{}", u8::from(skip))
}
fn fingerprint(head: &[u8], offset: u64) -> String {
    format!(
        "{:x}",
        Sha256::digest(&head[..head.len().min(offset.min(32) as usize)])
    )[..16]
        .to_owned()
}
fn snapshot_cursor(generation: &str, offset: u64, skip: bool, head: &[u8]) -> String {
    format!(
        "{}:{}",
        cursor(generation, offset, skip),
        fingerprint(head, offset)
    )
}
fn parse_cursor(s: &str) -> Result<(&str, u64, bool, Option<&str>)> {
    let fields = s.split(':').collect::<Vec<_>>();
    anyhow::ensure!(
        matches!(fields.len(), 3 | 4) && fields[0].len() == 16,
        "Некорректный cursor журнала"
    );
    let offset = fields[1].parse().context("Некорректный offset журнала")?;
    anyhow::ensure!(
        matches!(fields[2], "0" | "1"),
        "Некорректный cursor журнала"
    );
    let fingerprint = fields.get(3).copied();
    anyhow::ensure!(
        fingerprint.is_none_or(|f| f.len() == 16 && f.bytes().all(|b| b.is_ascii_hexdigit())),
        "Некорректная проверка cursor"
    );
    Ok((fields[0], offset, fields[2] == "1", fingerprint))
}
pub fn read(session: &Path, query: &Query) -> Result<Page> {
    anyhow::ensure!(
        !(query.after.is_some() && query.before.is_some()),
        "after и before несовместимы"
    );
    let mut file = match File::open(session.join(FILE)) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(Page {
                reset: query.after.is_some() || query.before.is_some(),
                ..Default::default()
            })
        }
        Err(e) => return Err(e.into()),
    };
    let len = file.metadata()?.len();
    let (generation, head) = generation(&mut file)?;
    let mut reset = false;
    let mut skip = false;
    let mut boundary = None;
    if let Some(s) = query.after.as_deref().or(query.before.as_deref()) {
        let (g, offset, s, checksum) = parse_cursor(s)?;
        if g != generation
            || offset > len
            || checksum.is_some_and(|value| value != fingerprint(&head, offset))
        {
            reset = true;
        } else {
            boundary = Some(offset);
            skip = s;
        }
    }
    let forward = query.after.is_some() && !reset;
    let end = if forward {
        len
    } else {
        boundary.unwrap_or(len)
    };
    let start = if forward {
        boundary.unwrap_or(0)
    } else {
        end.saturating_sub(MAX_SCAN)
    };
    let end = end.min(start.saturating_add(MAX_SCAN));
    let mut starts_at_boundary = start == 0;
    if start > 0 {
        file.seek(SeekFrom::Start(start - 1))?;
        let mut previous = [0];
        file.read_exact(&mut previous)?;
        starts_at_boundary = previous[0] == b'\n';
    }
    anyhow::ensure!(
        !forward || skip || starts_at_boundary,
        "Cursor не указывает на границу события"
    );
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(end - start).read_to_end(&mut bytes)?;
    let mut page = Page {
        reset,
        truncated: start > 0 && !forward,
        read_bytes: bytes.len() as u64 + 33,
        dropped_verbose: metadata(session).dropped,
        ..Default::default()
    };
    let mut offset = start;
    let mut committed = start;
    let mut first = true;
    let mut rows = Vec::new();
    let mut run = String::new();
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        if line.last() != Some(&b'\n') {
            break;
        }
        let next = offset + line.len() as u64;
        if (first && !starts_at_boundary && !forward) || skip {
            skip = false;
            page.discarded += 1;
            offset = next;
            committed = next;
            first = false;
            continue;
        }
        first = false;
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) {
            if value.get("journal").and_then(|v| v.as_u64()) == Some(1) {
                if let Ok(mut entry) = serde_json::from_value::<Entry>(value) {
                    entry.id = cursor(&generation, offset, false);
                    entry.label = safe_text(&entry.label);
                    entry.at = safe_text(&entry.at).chars().take(64).collect();
                    entry.text = entry.text.map(|s| safe_text(&s));
                    entry.command = entry.command.map(DisplayCommand::sanitized);
                    entry.run = safe_text(&entry.run).chars().take(128).collect();
                    entry.operation = entry
                        .operation
                        .map(|s| safe_text(&s).chars().take(128).collect());
                    entry.parent = entry
                        .parent
                        .map(|s| safe_text(&s).chars().take(128).collect());
                    entry.stage = safe_text(&entry.stage).chars().take(64).collect();
                    entry.kind = safe_text(&entry.kind).chars().take(64).collect();
                    entry.status = entry
                        .status
                        .map(|s| safe_text(&s).chars().take(64).collect());
                    rows.push((offset, next, entry));
                } else {
                    page.discarded += 1;
                }
            } else if let Some(r) = value.get("run").and_then(|v| v.as_str()) {
                run = safe_text(r).chars().take(128).collect();
            } else if let Ok(event) = serde_json::from_value::<crate::progress::StageEvent>(value) {
                rows.push((
                    offset,
                    next,
                    Entry {
                        id: cursor(&generation, offset, false),
                        run: run.clone(),
                        stage: format!("{:?}", event.stage).to_lowercase(),
                        kind: "stage".into(),
                        at: safe_text(&event.at).chars().take(64).collect(),
                        label: format!("{:?}", event.stage).to_lowercase(),
                        status: Some(format!("{:?}", event.state).to_lowercase()),
                        text: event.note.map(|s| safe_text(&s)),
                        done: event.done,
                        total: event.total,
                        ..Default::default()
                    },
                ));
            }
        } else {
            page.discarded += 1;
        }
        offset = next;
        committed = next;
    }
    // A pathological giant record must not trap every future request at the same cursor.
    if committed == start && bytes.len() as u64 == MAX_SCAN {
        committed = end;
        skip = true;
        page.discarded += 1;
    }
    let limit = query.limit.clamp(1, MAX_EVENTS);
    let mut output_bytes = 0;
    let mut selected = Vec::new();
    if forward {
        for (from, to, entry) in rows {
            let size = serde_json::to_vec(&entry)?.len() + 1;
            if selected.len() >= limit || output_bytes + size > MAX_PAGE_BYTES - 1024 {
                committed = from;
                page.has_more = true;
                break;
            }
            output_bytes += size;
            selected.push((from, entry));
            committed = to;
        }
    } else {
        for (from, _, entry) in rows.into_iter().rev() {
            let size = serde_json::to_vec(&entry)?.len() + 1;
            if selected.len() >= limit || output_bytes + size > MAX_PAGE_BYTES - 1024 {
                page.truncated = true;
                break;
            }
            output_bytes += size;
            selected.push((from, entry));
        }
        selected.reverse();
    }
    page.has_more |=
        forward && committed > start && committed < len && (skip || bytes.last() == Some(&b'\n'));
    page.previous_cursor = snapshot_cursor(
        &generation,
        selected.first().map(|(o, _)| *o).unwrap_or(start),
        false,
        &head,
    );
    page.next_cursor = snapshot_cursor(&generation, committed, skip, &head);
    page.events = selected.into_iter().map(|(_, e)| e).collect();
    Ok(page)
}

#[cfg(test)]
mod tests {
    #[test]
    fn operation_identity_is_bound_to_its_attempt_and_old_ends_cannot_clear_a_retry() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(execution_identity(dir.path(), None), (None, None));
        assert!(!dir.path().join(".journal-meta.json").exists());
        begin_run(dir.path(), "run");
        let old = Operation::start(dir.path(), "summary", "worker", None);
        old.bind_execution("attempt-1").unwrap();
        assert!(execution_identity(dir.path(), Some("attempt-1"))
            .1
            .is_some());
        let retry = Operation::start(dir.path(), "summary", "worker", None);
        retry.bind_execution("attempt-2").unwrap();
        assert!(execution_identity(dir.path(), Some("attempt-1"))
            .1
            .is_none());
        let active = execution_identity(dir.path(), Some("attempt-2")).1;
        assert!(active.is_some());
        drop(old);
        assert_eq!(execution_identity(dir.path(), Some("attempt-2")).1, active);
        drop(retry);
        assert!(execution_identity(dir.path(), Some("attempt-2"))
            .1
            .is_none());
        let page = read(
            dir.path(),
            &Query {
                limit: 100,
                ..Default::default()
            },
        )
        .unwrap();
        let ends: Vec<_> = page
            .events
            .iter()
            .filter(|e| e.kind == "operation_end")
            .collect();
        assert_eq!(ends.len(), 2);
        assert!(ends.iter().all(|e| e
            .elapsed_sec
            .is_some_and(|seconds| seconds >= 0.0 && seconds.is_finite())));
    }
    use super::*;
    fn log(s: &Path, text: &str) {
        append(
            s,
            Entry {
                kind: "log".into(),
                label: text.into(),
                ..Default::default()
            },
        )
        .unwrap();
    }
    #[test]
    fn pages_replay_without_duplication_and_tail_waits_for_newline() {
        let d = tempfile::tempdir().unwrap();
        log(d.path(), "one");
        let p = read(
            d.path(),
            &Query {
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.events.len(), 1);
        let mut f = OpenOptions::new()
            .append(true)
            .open(d.path().join(FILE))
            .unwrap();
        f.write_all(b"{\"journal\":1,\"label\":\"two\"").unwrap();
        let q = Query {
            after: Some(p.next_cursor.clone()),
            limit: 200,
            ..Default::default()
        };
        let pending = read(d.path(), &q).unwrap();
        assert!(pending.events.is_empty());
        assert_eq!(pending.next_cursor, p.next_cursor);
        f.write_all(b"}\n").unwrap();
        let two = read(d.path(), &q).unwrap();
        assert_eq!(two.events[0].label, "two");
        assert_eq!(read(d.path(), &q).unwrap().events[0].id, two.events[0].id);
    }
    #[test]
    fn backward_pages_preserve_the_first_complete_record() {
        let d = tempfile::tempdir().unwrap();
        for n in 0..10 {
            log(d.path(), &n.to_string());
        }
        let last = read(
            d.path(),
            &Query {
                limit: 3,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(last.events[0].label, "7");
        let prev = read(
            d.path(),
            &Query {
                before: Some(last.previous_cursor),
                limit: 3,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(prev.events[2].label, "6");
    }
    #[test]
    fn truncation_resets_cursor_and_legacy_stays_readable() {
        let d = tempfile::tempdir().unwrap();
        log(d.path(), "first");
        let p = read(
            d.path(),
            &Query {
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        std::fs::write(
            d.path().join(FILE),
            b"{\"stage\":\"summary\",\"state\":\"failed\",\"at\":\"t\"}\n",
        )
        .unwrap();
        let p = read(
            d.path(),
            &Query {
                after: Some(p.next_cursor),
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(p.reset);
        assert_eq!(p.events[0].status.as_deref(), Some("failed"));
    }
    #[test]
    fn display_arguments_are_allowlisted_and_text_is_bounded() {
        let command = DisplayCommand::from_args(
            "worker",
            &[
                "--token".into(),
                "secret".into(),
                "--worker-phase".into(),
                "text".into(),
                "--unknown".into(),
                "hidden".into(),
            ],
        );
        let text = serde_json::to_string(&command).unwrap();
        assert!(text.contains("text"));
        assert!(!text.contains("secret"));
        assert!(!text.contains("hidden"));
        assert!(!safe_text("\x1b]52;c;data\x07").contains(['\x1b', '\x07']));
        assert_eq!(safe_text(&"ж".repeat(10000)).chars().count(), MAX_TEXT);
    }
    #[test]
    fn quota_reports_lost_details_but_keeps_errors_and_transitions() {
        let d = tempfile::tempdir().unwrap();
        begin_run(d.path(), "run-one");
        save_metadata(
            d.path(),
            &Metadata {
                run: "run-one".into(),
                verbose_bytes: MAX_VERBOSE,
                ..Default::default()
            },
        )
        .unwrap();
        log(d.path(), "dropped");
        append(
            d.path(),
            Entry {
                kind: "log".into(),
                status: Some("failed".into()),
                text: Some("visible failure".into()),
                ..Default::default()
            },
        )
        .unwrap();
        append(
            d.path(),
            Entry {
                kind: "operation_end".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let p = read(
            d.path(),
            &Query {
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.dropped_verbose, 1);
        assert_eq!(p.events.len(), 2);
        assert_eq!(p.events[0].text.as_deref(), Some("visible failure"));
        assert_eq!(p.events[0].run, "run-one");
        begin_run(d.path(), "retry-two");
        log(d.path(), "retained after retry");
        let retry = read(
            d.path(),
            &Query {
                after: Some(p.next_cursor),
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(retry.dropped_verbose, 0);
        assert_eq!(retry.events[0].run, "retry-two");
    }
    #[test]
    fn giant_and_bad_records_make_bounded_progress() {
        let d = tempfile::tempdir().unwrap();
        log(d.path(), "start");
        let p = read(
            d.path(),
            &Query {
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        let mut file = OpenOptions::new()
            .append(true)
            .open(d.path().join(FILE))
            .unwrap();
        file.write_all(&vec![b'x'; MAX_SCAN as usize * 2 + 101])
            .unwrap();
        file.write_all(b"\nnot-json\n").unwrap();
        drop(file);
        log(d.path(), "end");
        let mut cursor = p.next_cursor;
        let mut found = false;
        let mut lost = 0;
        for _ in 0..5 {
            let p = read(
                d.path(),
                &Query {
                    after: Some(cursor.clone()),
                    limit: 200,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(p.read_bytes <= MAX_SCAN + 33);
            lost += p.discarded;
            found |= p.events.iter().any(|e| e.label == "end");
            assert_ne!(p.next_cursor, cursor);
            cursor = p.next_cursor;
            if found {
                break;
            }
            assert!(p.has_more);
        }
        assert!(found);
        assert!(lost >= 2);
    }
    #[test]
    fn hostile_display_fields_are_sanitized_even_in_existing_files() {
        let d = tempfile::tempdir().unwrap();
        let mut e = Entry {
            journal: 1,
            at: "ж".repeat(300_000),
            label: "<img onerror=alert(1)>\x1b]52;c;secret\x07".into(),
            text: Some("x".repeat(10000)),
            command: Some(DisplayCommand {
                program: "/private/bin/worker".into(),
                args: vec![
                    DisplayArgument {
                        name: "--token".into(),
                        value: Some("secret-token".into()),
                    },
                    DisplayArgument {
                        name: "--scope".into(),
                        value: Some("summary".into()),
                    },
                ],
            }),
            ..Default::default()
        };
        let mut bytes = serde_json::to_vec(&e).unwrap();
        bytes.push(b'\n');
        std::fs::write(d.path().join(FILE), bytes).unwrap();
        let p = read(
            d.path(),
            &Query {
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.events.len(), 1);
        assert!(p.events[0].at.chars().count() <= 64);
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("secret-token"));
        assert!(!json.contains("/private"));
        assert!(json.len() < MAX_PAGE_BYTES);
        assert!(!p.events[0].label.contains('\x1b'));
        // HTML remains inert text, escaped by React; the server does not execute or parse it.
        e.command = None;
        e.at = "same-time".into();
        assert!(p.events[0].label.starts_with("<img"));
    }
    #[test]
    fn append_failure_is_an_error_and_does_not_accept_an_artifact() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join(FILE)).unwrap();
        assert!(append(
            d.path(),
            Entry {
                kind: "operation_end".into(),
                ..Default::default()
            }
        )
        .is_err());
        assert!(append_line(d.path(), b"{}\n").is_err());
    }
    #[test]
    fn concurrent_writers_never_interleave_json_lines() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().to_owned();
        let writers = (0..4)
            .map(|w| {
                let path = path.clone();
                std::thread::spawn(move || {
                    (0..100)
                        .filter(|n| {
                            append(
                                &path,
                                Entry {
                                    kind: "operation_start".into(),
                                    label: format!("{w}/{n}"),
                                    ..Default::default()
                                },
                            )
                            .is_ok()
                        })
                        .count()
                })
            })
            .collect::<Vec<_>>();
        let accepted = writers
            .into_iter()
            .map(|t| t.join().unwrap())
            .sum::<usize>();
        assert!(accepted >= 100);
        let text = std::fs::read_to_string(path.join(FILE)).unwrap();
        assert_eq!(text.lines().count(), accepted);
        for line in text.lines() {
            assert!(serde_json::from_str::<Entry>(line).is_ok());
        }
    }
    #[test]
    fn large_history_poll_and_payload_are_bounded() {
        let d = tempfile::tempdir().unwrap();
        let mut file = File::create(d.path().join(FILE)).unwrap();
        let record = serde_json::to_string(&Entry {
            journal: 1,
            kind: "log".into(),
            text: Some("x".repeat(4096)),
            ..Default::default()
        })
        .unwrap()
            + "\n";
        for _ in 0..8000 {
            file.write_all(record.as_bytes()).unwrap();
        }
        drop(file);
        let len = std::fs::metadata(d.path().join(FILE)).unwrap().len();
        assert!(len > 32 * 1024 * 1024);
        let page = read(
            d.path(),
            &Query {
                limit: usize::MAX,
                ..Default::default()
            },
        )
        .unwrap();
        let response = serde_json::to_vec(&page).unwrap().len();
        assert!(response <= MAX_PAGE_BYTES);
        assert!(page.read_bytes <= MAX_SCAN + 33);
        assert!(page.truncated);
        let idle = read(
            d.path(),
            &Query {
                after: Some(page.next_cursor),
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(idle.events.is_empty());
        assert!(idle.read_bytes <= 33);
        eprintln!(
            "journal measurement: file={len}, tail_read={}, response={response}, idle_read={}",
            page.read_bytes, idle.read_bytes
        );
    }
    #[test]
    fn forged_cursor_inside_record_is_rejected() {
        let d = tempfile::tempdir().unwrap();
        log(d.path(), "one");
        let p = read(
            d.path(),
            &Query {
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        let g = p.next_cursor.split(':').next().unwrap();
        assert!(read(
            d.path(),
            &Query {
                after: Some(cursor(g, 3, false)),
                limit: 200,
                ..Default::default()
            }
        )
        .is_err());
        assert!(read(
            d.path(),
            &Query {
                after: Some("bogus".into()),
                limit: 200,
                ..Default::default()
            }
        )
        .is_err());
    }
    #[test]
    fn an_operation_keeps_its_original_run_when_a_retry_starts() {
        let d = tempfile::tempdir().unwrap();
        begin_run(d.path(), "first");
        let operation = Operation::start(d.path(), "summary", "actual worker", None);
        begin_run(d.path(), "retry");
        operation.log("old operation");
        drop(operation);
        let page = read(
            d.path(),
            &Query {
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.events.len(), 3);
        assert!(page.events.iter().all(|e| e.run == "first"));
        assert!(page
            .events
            .iter()
            .all(|e| e.status.as_deref() != Some("done")));
    }
    #[test]
    fn first_partial_record_is_not_a_file_generation_change() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join(FILE),
            b"{\"journal\":1,\"kind\":\"log\"",
        )
        .unwrap();
        let partial = read(
            directory.path(),
            &Query {
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(partial.events.is_empty());
        let mut file = OpenOptions::new()
            .append(true)
            .open(directory.path().join(FILE))
            .unwrap();
        file.write_all(b",\"label\":\"completed\"}\n").unwrap();
        drop(file);
        let completed = read(
            directory.path(),
            &Query {
                after: Some(partial.next_cursor),
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!completed.reset);
        assert_eq!(completed.events[0].label, "completed");
    }
    #[test]
    fn replacing_a_file_with_identical_contents_resets_its_cursor() {
        let directory = tempfile::tempdir().unwrap();
        log(directory.path(), "same contents");
        let page = read(
            directory.path(),
            &Query {
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        let bytes = std::fs::read(directory.path().join(FILE)).unwrap();
        let mut replacement = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
        replacement.write_all(&bytes).unwrap();
        replacement.persist(directory.path().join(FILE)).unwrap();
        let replaced = read(
            directory.path(),
            &Query {
                after: Some(page.next_cursor),
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(replaced.reset);
        assert_eq!(replaced.events.len(), 1);
        assert_ne!(replaced.events[0].id, page.events[0].id);
    }
    #[test]
    fn oversize_legacy_fields_cannot_trap_the_next_cursor() {
        let directory = tempfile::tempdir().unwrap();
        let data = format!(
            "{}\n{}\n",
            serde_json::json!({"run":"r".repeat(100000)}),
            serde_json::json!({"stage":"summary","state":"failed","at":"a".repeat(300000)})
        );
        std::fs::write(directory.path().join(FILE), data).unwrap();
        let page = read(
            directory.path(),
            &Query {
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.events[0].at.len(), 64);
        assert_eq!(page.events[0].run.len(), 128);
        assert!(serde_json::to_vec(&page).unwrap().len() < MAX_PAGE_BYTES);
        let idle = read(
            directory.path(),
            &Query {
                after: Some(page.next_cursor),
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(idle.events.is_empty());
        assert!(!idle.has_more);
    }
}
