//! Voice profiles: «this is Иван again».
//!
//! Diarization by itself only gives «Участник 1, 2, 3» — and in the next session the same
//! people will get different numbers. A profile ties a voice to the NAME the person gave
//! once, and from then on recognises it by itself.
//!
//! **A voice print is biometrics.** Therefore:
//!
//! * profiles live ONLY locally, in the working directory, next to the recordings;
//! * a profile appears only once the person has given a name HIMSELF — silently
//!   accumulating voice prints of everyone who got into the microphone is not allowed;
//! * deletion is one line: remove the entry from `speakers.json` and the print is gone.
//!
//! The recognition threshold is deliberately STRICT. Erring on the side of «did not
//! recognise» is cheap: the person will give the name once more. Erring on the side of
//! «recognised the wrong one» is expensive: the summary will attribute someone else's
//! words to a living person, and that is defamation, not a typo.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const FILE: &str = "speakers.json";

/// The cosine similarity above which a voice is considered to be the same person.
///
/// **The number is MEASURED on our model and on Russian speech, not taken from a
/// textbook.** The first version stood at 0.70 «with a margin» — and that turned out to be
/// wide of the mark: with ERes2Net, on real «one and the same person» pairs the similarity
/// averages 0.55–0.77 (short stretches sit closer to the lower edge), and a threshold of
/// 0.70 would not have accepted almost a single correct match. Strictness that recognises
/// nobody is not strictness but a broken function.
///
/// The measurement (bench/golos-crowd, stretches ≥ 3 s):
///
/// ```text
///   self-self    0.62–0.77        equal-error-rate threshold    ~0.40–0.67
///   self-other   0.18–0.24        threshold at 0.1% false        ~0.57–0.60
/// ```
///
/// We take 0.60 — the right edge of «0.1% false positives». Erring on the side of «did not
/// recognise» is cheap: the person will give the name once more. Erring on the side of
/// «recognised the wrong one» is expensive: the summary will attribute someone else's words
/// to a living person.
///
/// The threshold depends on the length of the stretch (on short ones the voice is described
/// worse), and that is why a profile is only built from [`MIN_SPEECH_SEC`] seconds of speech
/// and up.
pub const RECOGNISE_AT: f32 = 0.60;

/// Less speech than this — we do not build a profile. A single «uh-huh» describes a cough,
/// not a voice.
pub const MIN_SPEECH_SEC: f64 = 8.0;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Profile {
    /// The name the HUMAN gave. We do not invent names ourselves.
    pub name: String,
    /// The voice centroid — the averaged and normalised vector.
    pub embedding: Vec<f32>,
    /// How many seconds of speech went into the profile: it shows how much to trust it.
    pub speech_sec: f64,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Profiles {
    #[serde(default)]
    pub speakers: Vec<Profile>,
}

fn path(work_dir: &Path) -> PathBuf {
    work_dir.join(FILE)
}

/// A corrupt profiles file must not bring down the cook: we assume there are no profiles.
/// Not recognising a person is unpleasant; not transcribing a recording is a loss.
pub fn load(work_dir: &Path) -> Profiles {
    let Ok(bytes) = std::fs::read(path(work_dir)) else {
        return Profiles::default();
    };
    match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("speakers.json is corrupt ({e}) — the profiles are not applied");
            Profiles::default()
        }
    }
}

fn save(work_dir: &Path, p: &Profiles) -> std::io::Result<()> {
    let target = path(work_dir);
    let tmp = target.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(p).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, &target)
}

impl Profiles {
    /// Who this is, if we know him. `None` — did not recognise, and that is a NORMAL
    /// answer: silently substituting a similar person is worse than honestly saying
    /// «Участник 2».
    pub fn recognise(&self, embedding: &[f32]) -> Option<(&str, f32)> {
        let mut best: Option<(&Profile, f32)> = None;
        for p in &self.speakers {
            if p.embedding.len() != embedding.len() {
                continue; // a profile from another model — we do not compare
            }
            let s = super::cluster::similarity(&p.embedding, embedding);
            if best.is_none_or(|(_, bs)| s > bs) {
                best = Some((p, s));
            }
        }
        best.filter(|(_, s)| *s >= RECOGNISE_AT)
            .map(|(p, s)| (p.name.as_str(), s))
    }
}

/// Remember a voice under the name the HUMAN gave.
///
/// If the name is already known — we update: the voice drifts (a different microphone, a
/// cold, a room), and the profile must drift along with it. The new print is taken with a
/// weight proportional to the length of the speech: ten minutes of conversation describe a
/// person better than eight seconds.
pub fn enroll(
    work_dir: &Path,
    name: &str,
    embedding: &[f32],
    speech_sec: f64,
) -> anyhow::Result<()> {
    anyhow::ensure!(!name.trim().is_empty(), "the name cannot be empty");
    anyhow::ensure!(!embedding.is_empty(), "empty voice print");
    anyhow::ensure!(
        speech_sec >= MIN_SPEECH_SEC,
        "only {speech_sec:.0} s of speech — a profile needs at least {MIN_SPEECH_SEC:.0} s. \
         A short turn describes chance, not a voice"
    );

    let mut all = load(work_dir);
    match all.speakers.iter_mut().find(|p| p.name == name) {
        Some(p) if p.embedding.len() == embedding.len() => {
            let w_old = p.speech_sec.max(1.0);
            let w_new = speech_sec;
            let total = w_old + w_new;
            for (i, v) in p.embedding.iter_mut().enumerate() {
                *v = ((*v as f64 * w_old + embedding[i] as f64 * w_new) / total) as f32;
            }
            normalise(&mut p.embedding);
            p.speech_sec = total;
            p.updated_at = crate::versions::now_rfc3339();
        }
        Some(p) => {
            // A print from ANOTHER model — the old one is incomparable, we replace it whole.
            tracing::info!("profile «{name}»: the embedding model changed, the print was rebuilt");
            p.embedding = normalised(embedding);
            p.speech_sec = speech_sec;
            p.updated_at = crate::versions::now_rfc3339();
        }
        None => all.speakers.push(Profile {
            name: name.to_string(),
            embedding: normalised(embedding),
            speech_sec,
            updated_at: crate::versions::now_rfc3339(),
        }),
    }
    save(work_dir, &all)?;
    Ok(())
}

/// Forget a voice. A person must be able to erase his biometrics — with one command.
pub fn forget(work_dir: &Path, name: &str) -> anyhow::Result<bool> {
    let mut all = load(work_dir);
    let before = all.speakers.len();
    all.speakers.retain(|p| p.name != name);
    let removed = all.speakers.len() != before;
    if removed {
        save(work_dir, &all)?;
    }
    Ok(removed)
}

/// The names and «how much to trust the profile» — for the UI.
pub fn list(work_dir: &Path) -> BTreeMap<String, f64> {
    load(work_dir)
        .speakers
        .into_iter()
        .map(|p| (p.name, p.speech_sec))
        .collect()
}

fn normalise(v: &mut [f32]) {
    let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        for x in v {
            *x /= n;
        }
    }
}

fn normalised(v: &[f32]) -> Vec<f32> {
    let mut out = v.to_vec();
    normalise(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn a_named_voice_is_recognised_next_time() {
        let d = tempdir().unwrap();
        enroll(d.path(), "Иван", &[1.0, 0.1, 0.0], 60.0).unwrap();

        let same = [0.98, 0.15, 0.02];
        let all = load(d.path());
        assert_eq!(all.recognise(&same).map(|(n, _)| n), Some("Иван"));
    }

    /// Not recognising is a NORMAL answer. Silently substituting a similar person is worse
    /// than honestly saying «Участник 2»: the summary would attribute someone else's words
    /// to a living person, and that is defamation, not a typo.
    #[test]
    fn a_stranger_is_not_mistaken_for_a_friend() {
        let d = tempdir().unwrap();
        enroll(d.path(), "Иван", &[1.0, 0.0, 0.0], 60.0).unwrap();

        let stranger = [0.0, 1.0, 0.0];
        assert!(load(d.path()).recognise(&stranger).is_none());
    }

    /// A profile DRIFTS along with the voice (a different microphone, a cold, a room), but a
    /// long recording weighs more than a short one.
    #[test]
    fn a_profile_drifts_with_the_voice_weighted_by_speech_length() {
        let d = tempdir().unwrap();
        enroll(d.path(), "Иван", &[1.0, 0.0, 0.0], 600.0).unwrap();
        // a short new recording is slightly different — the profile must not run away
        enroll(d.path(), "Иван", &[0.0, 1.0, 0.0], 10.0).unwrap();

        let p = &load(d.path()).speakers[0];
        assert!(
            p.embedding[0] > p.embedding[1],
            "ten seconds outweighed ten minutes: {:?}",
            p.embedding
        );
        assert!((p.speech_sec - 610.0).abs() < 0.1);
    }

    /// A short turn describes chance, not a voice.
    #[test]
    fn a_two_second_grunt_is_not_a_profile() {
        let d = tempdir().unwrap();
        let e = enroll(d.path(), "Иван", &[1.0, 0.0], 2.0).unwrap_err();
        assert!(format!("{e}").contains("at least"), "{e}");
    }

    /// A person must be able to erase his biometrics.
    #[test]
    fn a_voice_can_be_forgotten() {
        let d = tempdir().unwrap();
        enroll(d.path(), "Иван", &[1.0, 0.0], 60.0).unwrap();
        assert!(forget(d.path(), "Иван").unwrap());
        assert!(load(d.path()).speakers.is_empty());
        assert!(!forget(d.path(), "Иван").unwrap(), "a repeat is not an error");
    }

    #[test]
    fn a_profile_from_another_model_is_not_compared() {
        let d = tempdir().unwrap();
        enroll(d.path(), "Иван", &[1.0, 0.0, 0.0], 60.0).unwrap();
        // a print of a different dimension — there is nothing to compare, but we must not
        // crash either
        assert!(load(d.path()).recognise(&[1.0, 0.0]).is_none());
    }
}
