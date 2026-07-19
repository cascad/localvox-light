//! What the voice module is doing — published for whoever draws the screen.
//!
//! Two different things get captured in this app, and confusing them is worse than showing
//! neither: a **session** is the microphone going to disk (the red pill, `.recording_session`),
//! a **note** is a phrase on its way into a slot — someone else's file, on another disk.
//!
//! THE FIRST VERSION OF THIS ONLY PUBLISHED THE DICTATION IN FLIGHT, and that was not enough.
//! A dictation lasts a few seconds, so the indicator was visible only during those seconds; when
//! the module heard nothing at all, the screen looked exactly the same as when the module was not
//! running. The owner's questions were «работает ли вообще», «что и в какой слот пишется» and
//! «когда закончило писаться» — and a pill that appears only mid-phrase answers none of them.
//!
//! So the status carries three separate facts, because they fail separately:
//!   * `active` + `detail` — is the module even running, with what triggers and slots. Known at
//!     startup, so the screen can answer «работает» before anyone says a word;
//!   * `capturing` — a dictation in flight, growing phrase by phrase;
//!   * `last` — the note that landed, where it went, and when. The receipt.
//!
//! A FILE IN THE WORK DIRECTORY, because that is already this system's answer to «one part must
//! see what another is doing»: the engine publishes `.recording_session` and holds the work-dir
//! lock, the voice module asks the engine to start recording through `request_record_start`, and
//! the HTTP API holds NO in-process state — it reads the work directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A note being dictated right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capturing {
    /// WHERE it is headed — the slot's own name from slots.toml («идеи», «входящие»), already
    /// RESOLVED. «заметка» vs «идея» is exactly the distinction the person is looking at the
    /// screen to make, and the name they chose themselves is the only honest word for it.
    pub slot: String,
    /// Everything dictated so far. It grows across pauses — that growth IS the evidence that the
    /// module is listening and has not lost the thread.
    pub text: String,
}

/// A note that has LANDED. The receipt: what was written, where it went, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Written {
    pub slot: String,
    pub text: String,
    /// Where it physically went — the file the integration reported. This is what makes the claim
    /// checkable instead of merely reassuring.
    pub dest: String,
    pub at: String,
    /// The write failed — and saying so is the whole point. A note that vanished silently is the
    /// worst outcome available here: the person believes it was saved.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceStatus {
    /// Is the module running at all. False with a `detail` saying why — «no slots.toml» is an
    /// answer; silence is not.
    pub active: bool,
    /// Triggers, slots and TTS in one line — or the reason the module is off.
    pub detail: String,
    pub capturing: Option<Capturing>,
    pub last: Option<Written>,
}

fn path(work_dir: &Path) -> PathBuf {
    work_dir.join(".voice.json")
}

/// Publish the status. Best-effort by design: this is an observation, not the work. A note that
/// reached the slot but failed to update the screen is a cosmetic problem; the reverse would be a
/// lie, and that is why the write happens AFTER the note lands, never before.
pub fn publish(work_dir: &Path, status: &VoiceStatus) {
    let p = path(work_dir);
    let Ok(json) = serde_json::to_string(status) else {
        return;
    };
    // Rename, so a reader polling once a second never catches a half-written file.
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, &p).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Remove the marker entirely — the module is gone, not merely idle.
pub fn clear(work_dir: &Path) {
    let _ = std::fs::remove_file(path(work_dir));
}

/// What the voice module is doing. Unreadable or malformed — nothing: a status file is not worth
/// an error path in the caller.
pub fn read(work_dir: &Path) -> Option<VoiceStatus> {
    let raw = std::fs::read_to_string(path(work_dir)).ok()?;
    serde_json::from_str(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> VoiceStatus {
        VoiceStatus {
            active: true,
            detail: "triggers [запиши] · slots: идеи · TTS sapi".into(),
            capturing: None,
            last: None,
        }
    }

    #[test]
    fn a_published_status_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let s = status();
        publish(dir.path(), &s);
        assert_eq!(read(dir.path()), Some(s));
    }

    /// THE GAP THE FIRST VERSION HAD. With nothing being dictated the screen must still be able to
    /// say «модуль работает» — otherwise a silent module is indistinguishable from a dead one, and
    /// that was exactly the owner's complaint.
    #[test]
    fn an_idle_module_still_reports_that_it_is_alive() {
        let dir = tempfile::tempdir().unwrap();
        publish(dir.path(), &status());
        let got = read(dir.path()).unwrap();
        assert!(got.active, "an idle module reads as dead");
        assert!(got.capturing.is_none());
        assert!(!got.detail.is_empty(), "«works» with no detail answers nothing");
    }

    /// The receipt outlives the dictation. «Когда закончило писаться» is answered by `last`, which
    /// must survive the moment `capturing` goes away — otherwise the answer disappears exactly when
    /// the person looks for it.
    #[test]
    fn the_receipt_survives_after_the_dictation_ends() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = status();
        s.capturing = Some(Capturing { slot: "идеи".into(), text: "купить кофе".into() });
        publish(dir.path(), &s);

        s.capturing = None;
        s.last = Some(Written {
            slot: "идеи".into(),
            text: "купить кофе".into(),
            dest: "F:/vault/inbox/Идеи.md".into(),
            at: "2026-07-18T19:00:00+03:00".into(),
            error: None,
        });
        publish(dir.path(), &s);

        let got = read(dir.path()).unwrap();
        assert!(got.capturing.is_none(), "the pill outlived the note");
        let last = got.last.expect("the receipt vanished with the dictation");
        assert_eq!(last.slot, "идеи");
        assert_eq!(last.dest, "F:/vault/inbox/Идеи.md");
    }

    /// A failed write must be VISIBLE. A note that silently vanished is the worst outcome here:
    /// the person walks away believing it was saved.
    #[test]
    fn a_failed_write_is_recorded_rather_than_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = status();
        s.last = Some(Written {
            slot: "идеи".into(),
            text: "купить кофе".into(),
            dest: String::new(),
            at: "2026-07-18T19:00:00+03:00".into(),
            error: Some("F: недоступен".into()),
        });
        publish(dir.path(), &s);
        assert_eq!(read(dir.path()).unwrap().last.unwrap().error.as_deref(), Some("F: недоступен"));
    }

    /// Nothing published — nothing shown. A quiet machine is the normal case, not an error.
    #[test]
    fn no_marker_means_nothing_is_known() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()), None);
    }

    /// Garbage must not become an error path in the caller: «I cannot tell» reads as «nothing».
    #[test]
    fn a_mangled_marker_reads_as_nothing_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".voice.json"), "{ полов").unwrap();
        assert_eq!(read(dir.path()), None);
    }
}
