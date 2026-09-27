//! The roster of the RECORDING: who spoke in it, with which voice and under which name.
//!
//! It lives in `<session>/speakers.json` — next to the recording itself, not in the shared
//! archive. Legacy [`super::profiles`] are no longer used for naming participants.
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
    /// Used only for diarization and matching a manual name within this recording.
    pub embedding: Vec<f32>,
    pub speech_sec: f64,
    pub owner: bool,
    /// Missing in legacy rosters: an old label is not proof of a manual choice.
    #[serde(default)]
    pub manually_named: bool,
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
    crate::artifacts::atomic_write(&path(session_dir), &serde_json::to_vec_pretty(roster)?)
}

/// Rebuild a roster from this session's participants, preserving only unambiguous
/// manual names from this same session. Shared profiles are deliberately not an input.
pub fn for_participants(
    participants: &[super::Participant],
    previous: &Roster,
    lang: &str,
    imported: bool,
) -> Roster {
    let mut participants = participants.to_vec();
    if imported {
        for participant in &mut participants {
            participant.owner = false;
        }
    }
    let labels = super::names(&participants, lang);
    let matches = |embedding: &[f32], other: &[f32]| {
        !embedding.is_empty()
            && embedding.len() == other.len()
            && super::cluster::similarity(embedding, other) >= 0.90
    };
    let mut members: Vec<_> = participants
        .iter()
        .zip(labels)
        .map(|(p, label)| Member {
            id: p.id,
            label,
            embedding: p.embedding.clone(),
            speech_sec: p.speech_sec,
            owner: p.owner,
            manually_named: false,
        })
        .collect();
    for (i, participant) in participants.iter().enumerate() {
        let candidates: Vec<_> = previous
            .members
            .iter()
            .filter(|m| m.manually_named && matches(&participant.embedding, &m.embedding))
            .collect();
        if let [old] = candidates.as_slice() {
            if participants
                .iter()
                .filter(|p| matches(&p.embedding, &old.embedding))
                .count()
                == 1
                && !members
                    .iter()
                    .enumerate()
                    .any(|(j, m)| j != i && m.label == old.label)
            {
                members[i].label = old.label.clone();
                members[i].manually_named = true;
            }
        }
    }
    Roster { members }
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
                m.manually_named = true;
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
            manually_named: false,
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
        assert!(
            !r.rename("Участник 7", "Пётр"),
            "the named one was not in the recording"
        );
        assert_eq!(r.members.len(), 1);
    }

    #[test]
    fn a_corrupt_roster_is_not_a_crash() {
        let d = tempdir().unwrap();
        std::fs::write(path(d.path()), b"{ not json").unwrap();
        assert!(load(d.path()).members.is_empty());
    }

    #[test]
    fn recook_preserves_only_unambiguous_manual_names_in_this_session() {
        let p = super::super::Participant {
            id: 0,
            embedding: vec![1.0, 0.0],
            speech_sec: 30.0,
            owner: true,
        };
        let mut old = Roster {
            members: vec![member(0, "Old guessed name")],
        };
        assert_eq!(
            for_participants(&[p.clone()], &old, "ru", true).members[0].label,
            "Участник 1"
        );
        old.rename("Old guessed name", "Local manual name");
        let current = for_participants(&[p.clone()], &old, "ru", true);
        assert_eq!(current.members[0].label, "Local manual name");
        assert!(current.members[0].manually_named);
        assert!(
            !current.members[0].owner,
            "an imported voice is not the microphone owner"
        );
        assert_eq!(
            for_participants(&[p.clone()], &Roster::default(), "ru", true).members[0].label,
            "Участник 1"
        );
        let mut second = p.clone();
        second.id = 1;
        let ambiguous = for_participants(&[p.clone(), second], &old, "ru", true);
        assert!(ambiguous.members.iter().all(|m| !m.manually_named));
        old.members[0].embedding.push(1.0);
        assert!(!for_participants(&[p], &old, "ru", true).members[0].manually_named);
    }

    #[test]
    fn legacy_names_are_not_treated_as_manual_and_new_renames_persist_origin() {
        let legacy = r#"{"members":[{"id":0,"label":"Someone","embedding":[1,0],"speech_sec":20,"owner":false}]}"#;
        let mut roster: Roster = serde_json::from_str(legacy).unwrap();
        assert!(!roster.members[0].manually_named);
        roster.rename("Someone", "Someone");
        let dir = tempdir().unwrap();
        save(dir.path(), &roster).unwrap();
        assert!(load(dir.path()).members[0].manually_named);
    }
}
