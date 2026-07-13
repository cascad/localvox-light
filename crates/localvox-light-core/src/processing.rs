//! The session processing log — `<session>/processing.json`.
//!
//! **Why.** The «processed» status used to be inferred from the PRESENCE OF A FILE, and
//! that is ambiguous: a missing `summary.md` means both «we have not done it yet» and «we
//! did it, but there was nothing to do» (there is no speech in the recording). Out of
//! that ambiguity one had to wriggle with heuristics («were the flags set back then?»),
//! and any mistake in them gave either an endless loop or a summary lost forever — both
//! variants we got at the acceptance.
//!
//! Here the fact is stored EXPLICITLY: «we did X with recipe Y, the result was Z, at such
//! and such a time». Hence:
//!
//! * **no record** → it has to be done;
//! * **a record with a DIFFERENT recipe** (the model changed, the template, the prompt
//!   version, the cook parameters) → it has to be redone;
//! * **a record with the same recipe**, whatever it ended with (done / nothing to do /
//!   unverified) → do not touch it. An endless loop is impossible by construction, not by
//!   vigilance.
//! * **`failed`** → retry, but at the pace of the queue (backoff), not head-on.
//!
//! Why a file in the session directory and not a shared DB: everything about a session
//! lives in its directory — it can be copied to another machine and it is self-contained
//! (P5). A centralized store would duplicate the state and drift out of sync with the
//! files. A dashboard over thousands of sessions, if it is ever needed, is built as a
//! DERIVED cache, rebuildable from the directories — but not as the source of truth.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "processing.json";

/// What exactly was done.
pub const TRANSCRIPT: &str = "transcript";
pub const SUMMARY: &str = "summary";
pub const PROCESSED: &str = "processed";
pub const REFINED: &str = "refined";

/// The revision of the prompts. Bumped when the TEMPLATES or the validation rules change
/// — otherwise an improvement of the prompt never reaches the already processed archive.
pub const PROMPT_REV: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// The artifact was created.
    Ok,
    /// There was nothing to do (the recording has no meaningful speech). That is FINISHED
    /// work, not unfinished: repeating it is pointless.
    Nothing,
    /// Created but not confirmed by the recording — it lies there as a draft.
    Unverified,
    /// A HUMAN looked at the draft and said it is fine.
    ///
    /// Our checks will always have false positives — that is the price of catching real
    /// inventions (measured on a live recording: «Сергей» and «муж» were both flagged as
    /// invented although both were said out loud). A person who has read the draft knows
    /// better than the machine, and his verdict must be REMEMBERED — otherwise the same
    /// warning comes back at him on the next cook, and he stops reading warnings altogether.
    ///
    /// This is the antonym of «re-cook»: there the human says «the machine is wrong, redo
    /// it», here he says «the machine is fussing, it is fine».
    Confirmed,
    /// It fell through (the LLM is unavailable, etc.) — retry, but at the pace of the
    /// queue.
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Record {
    /// The fingerprint of the WAY it was made: it changed — we redo it.
    pub recipe: String,
    pub outcome: Outcome,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The transcript version the artifact was MADE FROM.
    ///
    /// It tells apart two completely different reasons to throw a summary away: it is OUT
    /// OF DATE (the recording was re-cooked — the old summary speaks about a different
    /// text) or it DID NOT PASS THE CHECK (the text is the same, but the model's new
    /// answer is dubious). The first is a legitimate reason to delete it, the second is
    /// not: deleting yesterday's confirmed summary because of today's dubious one means
    /// punishing a human for our own mistake in the check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<u32>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct ProcessingLog {
    #[serde(default)]
    pub artifacts: BTreeMap<String, Record>,
}

/// The recipe of an LLM artifact: the kind of work + the style + the model + the prompt
/// revision.
///
/// `kind` — [`SUMMARY`] or [`PROCESSED`]. `style` — see [`llm_style`].
pub fn llm_recipe(kind: &str, style: &str, model: &str) -> String {
    format!("{kind}/{style}/{model}/p{PROMPT_REV}")
}

/// The processing style: the template explicitly chosen by a human, otherwise the
/// LANGUAGE of the recording (the template will be picked by it).
///
/// The language in the recipe IS the answer to «the language changed → redo everything
/// derived»: the recipe diverged, which means the artifact was made the wrong way, which
/// means it will be made again. No cleanup code at all.
///
/// Both discovery (before the work, to understand «is it needed») and the processor
/// (after it, to record «this is how we did it») MUST compute the style IDENTICALLY. If
/// they diverge, the artifact will be made forever. That is why the function is a single
/// one and lives here.
///
/// A note: the concrete template the work ended up with is NOT part of the recipe. A short
/// recording we process as a note rather than as a summary — but that is derived from the
/// transcript itself, and the transcript is already covered by its own recipe.
pub fn llm_style(template_override: Option<&str>, lang: &str) -> String {
    // An empty string means «not set», not «a template with an empty name». Otherwise an
    // empty environment variable (`LOCALVOX_LLM_SUMMARY_TEMPLATE=`) would give the
    // different sides different recipes: one would filter it out, the other would not.
    // So that the sides cannot diverge, the decision is taken HERE, not at their place.
    template_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(lang)
        .to_string()
}

fn path(session_dir: &Path) -> PathBuf {
    session_dir.join(FILE)
}

/// A broken log must not bring the processing down: we assume we did nothing.
/// It is better to over-process than to get wedged.
pub fn load(session_dir: &Path) -> ProcessingLog {
    let Ok(bytes) = fs::read(path(session_dir)) else {
        return ProcessingLog::default();
    };
    match serde_json::from_slice(&bytes) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("processing.json is corrupted ({e}) — assuming we did not process it");
            ProcessingLog::default()
        }
    }
}

/// Record the fact. Atomically (tmp + rename): the log must not be torn in half.
///
/// `source` — the transcript version the artifact was made from (for the cook — the
/// version itself). It is needed to tell «out of date» apart from «did not pass the
/// check»: see [`Record`].
pub fn record(
    session_dir: &Path,
    artifact: &str,
    recipe: &str,
    outcome: Outcome,
    detail: Option<String>,
    source: Option<u32>,
) {
    let mut log = load(session_dir);
    log.artifacts.insert(
        artifact.to_string(),
        Record {
            recipe: recipe.to_string(),
            outcome,
            at: crate::versions::now_rfc3339(),
            detail,
            source,
        },
    );
    let p = path(session_dir);
    let tmp = p.with_extension("json.tmp");
    let write = serde_json::to_vec_pretty(&log)
        .map_err(std::io::Error::other)
        .and_then(|b| fs::write(&tmp, b))
        .and_then(|()| fs::rename(&tmp, &p));
    if let Err(e) = write {
        tracing::warn!("processing.json was not written: {e}");
    }
}

/// Forget that we did it. Called when an artifact is THROWN AWAY (a re-cook on demand, a
/// change of language).
///
/// The invariant: **no file — no record about it either.** Erasing `summary.md` while
/// leaving «made with this recipe» in the log means lying to discovery: it will see the
/// record, decide the work is finished, and the summary will never come back.
pub fn forget(session_dir: &Path, artifacts: &[&str]) {
    let mut log = load(session_dir);
    let mut changed = false;
    for a in artifacts {
        changed |= log.artifacts.remove(*a).is_some();
    }
    if !changed {
        return;
    }
    let p = path(session_dir);
    let tmp = p.with_extension("json.tmp");
    let write = serde_json::to_vec_pretty(&log)
        .map_err(std::io::Error::other)
        .and_then(|b| fs::write(&tmp, b))
        .and_then(|()| fs::rename(&tmp, &p));
    if let Err(e) = write {
        tracing::warn!("processing.json was not written: {e}");
    }
}

/// Whether it was done EXACTLY THIS WAY. `false` — it has to be done (we did not do it, we
/// did it differently, or it fell through).
///
/// `Nothing` is «done» too: the work is finished, no result is required. It is this very
/// line that kills the endless loop: «there is no summary» stops meaning «the summary was
/// not made».
/// What the check doubted about, if the human has not yet said it is fine.
///
/// `None` — either there were no doubts, or the person has already looked and confirmed. A
/// warning that comes back after it has been answered stops being read at all, and then the
/// real one goes unnoticed too.
pub fn doubts(session_dir: &Path, artifact: &str) -> Option<String> {
    let r = load(session_dir).artifacts.get(artifact).cloned()?;
    (r.outcome == Outcome::Unverified).then_some(r.detail).flatten()
}

/// «It is fine» — the antonym of «re-cook».
///
/// There the human says «the machine is wrong, redo it»; here he says «the machine is
/// fussing, it is fine». Our checks compare literally and will always have false positives
/// (measured: «Сергей» and «муж» were flagged as invented although both were said out
/// loud). The person has read the document and can listen to the recording — the machine
/// cannot. His verdict outweighs and is REMEMBERED.
pub fn confirm(session_dir: &Path, artifact: &str) -> bool {
    let mut log = load(session_dir);
    let Some(r) = log.artifacts.get_mut(artifact) else {
        return false;
    };
    if r.outcome != Outcome::Unverified {
        return false; // there was nothing to confirm
    }
    r.outcome = Outcome::Confirmed;
    r.detail = Some("confirmed by the human".into());
    r.at = crate::versions::now_rfc3339();
    write_log(session_dir, &log);
    true
}

/// Atomic write of the ledger (tmp + rename).
fn write_log(session_dir: &Path, log: &ProcessingLog) {
    let p = path(session_dir);
    let tmp = p.with_extension("json.tmp");
    let write = serde_json::to_vec_pretty(log)
        .map_err(std::io::Error::other)
        .and_then(|b| fs::write(&tmp, b))
        .and_then(|()| fs::rename(&tmp, &p));
    if let Err(e) = write {
        tracing::warn!("processing.json was not written: {e}");
    }
}

pub fn is_current(session_dir: &Path, artifact: &str, recipe: &str) -> bool {
    load(session_dir)
        .artifacts
        .get(artifact)
        .map(|r| r.recipe == recipe && r.outcome != Outcome::Failed)
        .unwrap_or(false)
}

/// Whether the artifact lying there is out of date: it was made from a DIFFERENT
/// transcript version, that is, it speaks about a different text.
///
/// Only that is a legitimate reason to delete it. «Did not pass the check» is NOT a
/// reason: the text is the same, and yesterday's confirmed summary is worth more than
/// today's dubious one. Otherwise one false positive of the check erases a good document,
/// and there is nowhere to restore it from.
pub fn is_stale(session_dir: &Path, artifact: &str, current_source: u32) -> bool {
    load(session_dir)
        .artifacts
        .get(artifact)
        .map(|r| r.source != Some(current_source))
        // There is no record at all (an archive from before the log) — there is nothing to
        // judge by: we consider it out of date, otherwise a document of unknown origin
        // would hang next to the new draft forever.
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn nothing_to_do_is_done_and_never_loops() {
        let dir = tempdir().unwrap();
        let r = llm_recipe(SUMMARY, "ru", "qwen3.5:9b");

        assert!(
            !is_current(dir.path(), SUMMARY, &r),
            "we did not do it — it has to be done"
        );

        // we did it and found out there was nothing to do (there is no speech in the
        // recording)
        record(
            dir.path(),
            SUMMARY,
            &r,
            Outcome::Nothing,
            Some("no speech recognized".into()),
            Some(1),
        );
        assert!(
            is_current(dir.path(), SUMMARY, &r),
            "«nothing to do» is FINISHED work; a repeat = an endless loop"
        );
    }

    #[test]
    fn a_new_recipe_means_redo() {
        let dir = tempdir().unwrap();
        record(
            dir.path(),
            SUMMARY,
            &llm_recipe(SUMMARY, "ru", "qwen3.5:4b"),
            Outcome::Ok,
            None,
            Some(1),
        );
        // the model changed — the previous result is out of date
        assert!(!is_current(
            dir.path(),
            SUMMARY,
            &llm_recipe(SUMMARY, "ru", "qwen3.5:9b")
        ));
    }

    #[test]
    fn a_failure_is_retried() {
        let dir = tempdir().unwrap();
        let r = llm_recipe(PROCESSED, "ru", "qwen3.5:9b");
        record(
            dir.path(),
            PROCESSED,
            &r,
            Outcome::Failed,
            Some("ollama is down".into()),
            None,
        );
        assert!(
            !is_current(dir.path(), PROCESSED, &r),
            "it fell through — we retry"
        );
    }

    #[test]
    fn unverified_is_a_finished_job_too() {
        let dir = tempdir().unwrap();
        let r = llm_recipe(SUMMARY, "ru", "qwen3.5:9b");
        record(dir.path(), SUMMARY, &r, Outcome::Unverified, None, Some(1));
        assert!(
            is_current(dir.path(), SUMMARY, &r),
            "a draft is a result too: there is nothing to redo"
        );
    }

    #[test]
    fn an_empty_template_setting_is_no_setting_at_all() {
        // The daemon and the processor read the template from the same environment
        // variable, but by different paths. An empty variable must not give them DIFFERENT
        // recipes: once the recipes diverge, the artifact is made forever.
        assert_eq!(llm_style(None, "ru"), llm_style(Some(""), "ru"));
        assert_eq!(llm_style(None, "ru"), llm_style(Some("  "), "ru"));
        assert_eq!(llm_style(Some("video-notes-ru"), "ru"), "video-notes-ru");
    }

    #[test]
    fn a_broken_log_does_not_wedge_processing() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(FILE), "{ not json").unwrap();
        assert!(!is_current(dir.path(), SUMMARY, "any"));
    }
}
