//! The roster of the RECORDING: who spoke in it, with which voice and under which name.
//!
//! It lives in `<session>/speakers.json` — next to the recording itself, not in the shared
//! archive. Not to be confused with [`super::profiles`]: there live the NAMES the human
//! gave, and they are shared across all sessions; here are the participants of one
//! particular recording.
//!
//! **Why keep the participants' voices after the cook.** So that a person can be named
//! LATER. Without the roster, «Участник 2 is Иван» would mean running both models over the
//! recording again; with the list it is an edit of one line. And renaming must be cheap:
//! otherwise nobody will use it, and the minutes will forever be about «Участник 2».

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

const FILE: &str = "speakers.json";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Member {
    pub id: usize,
    /// How he is labelled in the transcript: «Я», «Иван», «Участник 2».
    pub label: String,
    /// The voice. It is also what goes into the shared profile once the person gives a name.
    pub embedding: Vec<f32>,
    pub speech_sec: f64,
    pub owner: bool,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Roster {
    #[serde(default)]
    pub members: Vec<Member>,
}

fn path(session_dir: &Path) -> PathBuf {
    session_dir.join(FILE)
}

/// A corrupt roster file must not bring down the reading of the session: we assume there is
/// no roster.
pub fn load(session_dir: &Path) -> Roster {
    let Ok(bytes) = std::fs::read(path(session_dir)) else {
        return Roster::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        tracing::warn!("the participant roster is corrupt ({e}) — we assume there is none");
        Roster::default()
    })
}

pub fn save(session_dir: &Path, roster: &Roster) -> Result<()> {
    let target = path(session_dir);
    let tmp = target.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(roster)?)?;
    std::fs::rename(&tmp, &target)?;
    Ok(())
}

impl Roster {
    pub fn by_label(&self, label: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.label == label)
    }

    /// Rename a participant. Returns `false` if there is no such person in the recording —
    /// silently creating a new participant out of a name nobody uttered is not allowed.
    pub fn rename(&mut self, from: &str, to: &str) -> bool {
        match self.members.iter_mut().find(|m| m.label == from) {
            Some(m) => {
                m.label = to.to_string();
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn member(id: usize, label: &str) -> Member {
        Member {
            id,
            label: label.into(),
            embedding: vec![1.0, 0.0],
            speech_sec: 42.0,
            owner: false,
        }
    }

    #[test]
    fn a_roster_survives_a_round_trip() {
        let d = tempdir().unwrap();
        let r = Roster {
            members: vec![member(0, "Я"), member(1, "Участник 1")],
        };
        save(d.path(), &r).unwrap();

        let back = load(d.path());
        assert_eq!(back.members.len(), 2);
        assert_eq!(back.by_label("Участник 1").unwrap().id, 1);
        assert!(back.by_label("Иван").is_none());
    }

    /// Only someone who SPOKE in the recording can be renamed. Creating a participant out of
    /// a name nobody uttered is not allowed: a person who was not at the meeting would show
    /// up in the minutes.
    #[test]
    fn only_a_voice_that_spoke_can_be_given_a_name() {
        let mut r = Roster {
            members: vec![member(1, "Участник 1")],
        };
        assert!(r.rename("Участник 1", "Иван"));
        assert_eq!(r.members[0].label, "Иван");
        assert!(!r.rename("Участник 7", "Пётр"), "the named one was not in the recording");
        assert_eq!(r.members.len(), 1);
    }

    #[test]
    fn a_corrupt_roster_is_not_a_crash() {
        let d = tempdir().unwrap();
        std::fs::write(path(d.path()), b"{ not json").unwrap();
        assert!(load(d.path()).members.is_empty());
    }
}
