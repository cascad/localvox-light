//! Diarization: WHO is speaking, not merely where the sound comes from.
//!
//! **Why.** Today a transcript line is tagged with its SOURCE: `[Я]` — the microphone,
//! `[Собеседники]` — system audio. On a four-way call all three of the other people
//! arrive as one mixed stream and become a single faceless «Собеседники». Because of
//! that the summary cannot honestly write «Иван взял миграцию»: we do not know that it
//! was Иван who said it — and guessing is not allowed.
//!
//! **How it is built** (assembled from ready-made pieces, like the rest of the ONNX
//! stack):
//!
//! 1. **segmentation** ([`Segmentation`], the model is [`segment::Segmenter`]) marks who
//!    speaks in every frame, including overlapping speech when two people talk at once;
//! 2. **embeddings** ([`Embedder`]) — for every stretch of CLEAN speech a VOICE vector is
//!    computed (timbre, not words: it is language-independent by nature);
//! 3. **clustering** ([`cluster`]) — the vectors are merged into people. We do NOT KNOW
//!    the number of speakers in advance, and we must not ask the human for it: he did not
//!    count them either;
//! 4. **timeline** ([`timeline`]) — «from 12.4 to 18.9 participant 2 is speaking»; the
//!    transcript lines are labelled from it;
//! 5. **profiles** ([`profiles`]) — once a person has given a name, the voice is
//!    recognised in the next session too.
//!
//! Not a single step generates text. Like NER, these are classifiers over the input:
//! they physically cannot invent a person who was not in the audio.
//!
//! **Why there is no window stitching.** The segmentation model only sees its own
//! window: its «A» in the fifth window and «A» in the sixth are different labels. The
//! customary fix is to stitch them with permutations at the seams; we do not, because we
//! have a firmer anchor — the voice. All embeddings are clustered AT ONCE over the whole
//! recording, and the cluster number IS the person. A stitching error at one seam would
//! spread across the whole timeline; we have no seams.
//!
//! **Why streaming, and not «read the recording and label it».** An hour of audio in f32
//! is 230 MB, and there is no reason to hold it in memory: both frames and embeddings
//! are computed inside a ten-second window. [`Runner`] keeps only those — a megabyte and
//! a half per hour.

pub mod cluster;
#[cfg(feature = "onnx")]
pub mod embed;
#[cfg(feature = "onnx")]
pub mod fbank;
pub mod profiles;
pub mod roster;
pub mod segment;
pub mod timeline;

use anyhow::Result;

use segment::{frames_covering, Frame, Window, FRAME_SHIFT, SAMPLE_RATE, WINDOW_SAMPLES};
use timeline::{LocalSegment, Smoothing, Turn};

/// Whether we label speakers at all. The ONLY place where this is decided.
///
/// Everyone must ask: the daemon, when it works out which recipe a session should be
/// cooked with, and the cook itself. This is not pedantry: a recipe mismatch between the
/// daemon and the child process has already hung the queue FOREVER twice (the session is
/// cooked, the recipe does not match, the session goes back into the queue). So the
/// decision lives here, and there is no «I have my own path to the model».
///
/// `LOCALVOX_DIARIZE=off` — turn it off even if the model is in place.
pub fn enabled() -> bool {
    // Without the ONNX feature there is nobody to label with — and we must not promise it
    // in the recipe: the session would wait forever for labelling that this binary
    // physically cannot do.
    if !cfg!(feature = "onnx") {
        return false;
    }
    if std::env::var("LOCALVOX_DIARIZE").is_ok_and(|v| {
        matches!(v.trim().to_ascii_lowercase().as_str(), "off" | "0" | "false")
    }) {
        return false;
    }
    model_dir().is_some()
}

/// The diarization model directory — or `None` if there is none. No models, no labelling:
/// silently pretending that the speakers are identified is not allowed.
pub fn model_dir() -> Option<std::path::PathBuf> {
    segment::model_dir_near(None)
}

/// Both models at once: the frame labeller and the voice meter.
///
/// They open together and they fail together. Half of diarization (frames without
/// voices) is useless: to label who speaks but be unable to say it is THE SAME person in
/// different windows means producing a recording full of «participants», each of whom
/// lived for ten seconds.
#[cfg(feature = "onnx")]
pub struct Models {
    pub seg: segment::Segmenter,
    pub emb: embed::Voice,
}

#[cfg(feature = "onnx")]
impl Models {
    pub fn open() -> Result<Self> {
        let dir = model_dir().ok_or_else(|| {
            anyhow::anyhow!(
                "no diarization model directory (models/diarize or LOCALVOX_DIARIZE_MODEL_DIR)"
            )
        })?;
        Ok(Self {
            seg: segment::Segmenter::open(&dir)?,
            emb: embed::Voice::open(&dir)?,
        })
    }
}

/// Labels one window: who speaks in each frame. A trait, not a struct, so that the whole
/// pipeline can be proven by a test without a weights file on disk.
pub trait Segmentation: Send + Sync {
    /// Exactly [`WINDOW_SAMPLES`] samples of PCM 16 kHz mono f32 → activity frames.
    fn frames(&self, window: &[f32]) -> Result<Vec<Frame>>;
}

/// Computes the voice print. The embedder model changes independently of everything else
/// — hence a trait as well.
pub trait Embedder: Send + Sync {
    /// Voice vector for a piece of speech (PCM 16 kHz mono f32).
    fn embed(&self, samples: &[f32]) -> Result<Vec<f32>>;
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Cosine DISTANCE beyond which voices are considered different.
    ///
    /// Measured on our model and on Russian speech (bench/golos-crowd): for one and the
    /// same person the similarity is 0.55–0.77 (distance 0.23–0.45), for different people
    /// 0.18–0.24 (distance 0.76–0.82). The threshold belongs between these two clouds.
    pub distance: f32,
    /// Cap on participants: a call never has forty people, but forty false clusters from
    /// noise — that does happen.
    pub max_speakers: Option<usize>,
    /// Less clean speech than this in a stretch — no embedding is computed. Half a second
    /// of timbre describes one sound, not a person.
    pub min_speech_sec: f64,
    /// The similarity at which a voice from the microphone is recognised as THE SAME one
    /// coming out of the speakers — and therefore cannot be the owner.
    ///
    /// This is a SEPARATE threshold, and it is deliberately looser than the clustering
    /// one. Clustering compares voices within one channel, whereas here we compare ACROSS
    /// channels: the microphone hears the room, loopback takes the sound from the system,
    /// and one and the same voice is noticeably less similar to itself in them. We are not
    /// obliged to merge them into one cluster — it is enough to understand that this voice
    /// cannot be the owner.
    pub echo_similarity: f32,
    /// Less speech than this OVER THE WHOLE RECORDING — this is not a participant.
    ///
    /// On a real recording «participants» with 1.1 and 3.1 seconds of speech showed up: a
    /// scrap of music, a knock, a piece of someone else's phrase at a seam. Declaring a
    /// person from them means seating at the table someone who was not at the meeting; he
    /// will be seen in the roster, he will be offered a name, and he will end up in the
    /// summary. The lines of such a «participant» stay UNATTRIBUTED — that is, an honest
    /// «we do not know who this is».
    pub min_participant_sec: f64,
    /// Sliding window step. A smaller step means more precise turn boundaries and more
    /// expensive inference (the window is always 10 s). 5 s is a sensible middle: every
    /// frame is seen by two windows.
    pub step_sec: f64,
    pub smoothing: Smoothing,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            // STRICT. Measured on a real recording: two DIFFERENT speakers (a cooking
            // video and a comedy sketch) came apart at similarity 0.546 — at a threshold
            // of 0.55 they would have stuck together into one person. For one and the same
            // voice the similarity is 0.62–0.77 (measured on bench/golos-crowd, stretches
            // ≥ 3 s). A threshold of 0.40 by distance (that is, 0.60 by similarity) lies
            // between these clouds with a margin on both sides.
            distance: 0.40,
            max_speakers: Some(8),
            // Looser than the clustering threshold: across channels (microphone versus
            // system audio) one and the same voice is noticeably less similar to itself.
            echo_similarity: 0.50,
            // On a one-second stretch voices still differ (self-self 0.55 against
            // self-other 0.18), but the margin is small. A second is the lower bound, and
            // below it we do not compute: better to leave a line unattributed than to
            // attribute it to the wrong person.
            min_speech_sec: 1.0,
            min_participant_sec: 5.0,
            step_sec: 5.0,
            smoothing: Smoothing::default(),
        }
    }
}

/// One participant that was found — and their voice.
#[derive(Debug, Clone)]
pub struct Speaker {
    pub id: usize,
    /// The voice centroid: what goes into the profile and by which the participant is
    /// recognised in the next session.
    pub embedding: Vec<f32>,
    /// How many seconds of clean speech went into the centroid. A short profile must not
    /// be trusted, and [`profiles::enroll`] checks that.
    pub speech_sec: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Diarization {
    /// Who speaks when. An empty timeline is an honest answer, not a failure: it means no
    /// voices were found, or none of them had enough clean speech.
    pub turns: Vec<Turn>,
    pub speakers: Vec<Speaker>,
}

/// Streaming labelling of a recording: samples arrive in pieces, memory does not grow.
pub struct Runner<'a> {
    seg: &'a dyn Segmentation,
    emb: &'a dyn Embedder,
    opts: Options,
    step: usize,

    /// Sliding buffer: from `buf_start` (in samples from the start of the recording) on.
    buf: Vec<f32>,
    buf_start: usize,
    total: usize,

    windows: Vec<Window>,
    segments: Vec<LocalSegment>,
    /// The embeddings and which segment each one belongs to: a segment without enough
    /// clean speech has no vector at all.
    embeddings: Vec<Vec<f32>>,
    of_segment: Vec<usize>,
}

impl<'a> Runner<'a> {
    pub fn new(seg: &'a dyn Segmentation, emb: &'a dyn Embedder, opts: Options) -> Self {
        let step = ((opts.step_sec * SAMPLE_RATE as f64) as usize).clamp(FRAME_SHIFT, WINDOW_SAMPLES);
        Self {
            seg,
            emb,
            opts,
            step,
            buf: Vec::new(),
            buf_start: 0,
            total: 0,
            windows: Vec::new(),
            segments: Vec::new(),
            embeddings: Vec::new(),
            of_segment: Vec::new(),
        }
    }

    /// Feed in the next piece of the recording (PCM 16 kHz mono f32, consecutive).
    pub fn push(&mut self, samples: &[f32]) -> Result<()> {
        self.buf.extend_from_slice(samples);
        self.total += samples.len();
        while self.buf.len() >= WINDOW_SAMPLES {
            self.window(WINDOW_SAMPLES)?;
            self.buf.drain(..self.step);
            self.buf_start += self.step;
        }
        Ok(())
    }

    /// Squeeze out the tail and count the people.
    pub fn finish(mut self) -> Result<Diarization> {
        if !self.buf.is_empty() {
            let tail = self.buf.len();
            self.window(tail)?;
        }
        self.resolve()
    }

    /// One window: frame labelling + embeddings of each voice's clean speech.
    ///
    /// `real` — how many samples of the window belong to the RECORDING (the tail is
    /// shorter than a window). The rest is our own silence, with which we padded the model
    /// input; its labelling is thrown away, otherwise the padding would become a turn.
    fn window(&mut self, real: usize) -> Result<()> {
        let mut padded = vec![0.0f32; WINDOW_SAMPLES];
        let n = real.min(self.buf.len()).min(WINDOW_SAMPLES);
        padded[..n].copy_from_slice(&self.buf[..n]);

        let mut frames = self.seg.frames(&padded)?;
        frames.truncate(frames_covering(n));
        let w = Window {
            offset: self.buf_start,
            frames,
        };

        // The clean speech of every local voice — right here, while the window is in hand.
        let min_run = (self.opts.min_speech_sec * SAMPLE_RATE as f64) as usize / 4;
        let index = self.windows.len();
        for mut seg in timeline::local_segments(std::slice::from_ref(&w), min_run) {
            seg.window = index;
            if seg.clean_sec() >= self.opts.min_speech_sec {
                // audio() is addressed from the start of the RECORDING — shift it to the
                // start of the buffer.
                let local = LocalSegment {
                    window: seg.window,
                    local: seg.local,
                    clean: seg
                        .clean
                        .iter()
                        .map(|(a, b)| (a - self.buf_start, b - self.buf_start))
                        .collect(),
                };
                match self.emb.embed(&local.audio(&padded)) {
                    Ok(v) if !v.is_empty() => {
                        self.embeddings.push(v);
                        self.of_segment.push(self.segments.len());
                    }
                    Ok(_) => tracing::warn!("diarization: empty voice vector — stretch skipped"),
                    // One broken stretch must not bring down the labelling of the whole
                    // recording.
                    Err(e) => tracing::warn!("diarization: stretch skipped ({e})"),
                }
            }
            self.segments.push(seg);
        }
        self.windows.push(w);
        Ok(())
    }

    /// Vectors → people → timeline.
    fn resolve(self) -> Result<Diarization> {
        if self.embeddings.is_empty() {
            return Ok(Diarization::default());
        }
        let clusters = cluster::agglomerative(
            &self.embeddings,
            self.opts.distance,
            self.opts.max_speakers,
        );

        let mut labels: Vec<Option<usize>> = vec![None; self.segments.len()];
        for (v, &seg) in self.of_segment.iter().enumerate() {
            labels[seg] = Some(clusters[v]);
        }

        let turns = timeline::turns(
            &self.windows,
            &self.segments,
            &labels,
            self.total,
            self.opts.smoothing,
        );

        // The participant's voice centroid: a long stretch describes a person better than
        // a short one — and weighs more.
        let n = clusters.iter().copied().max().map_or(0, |m| m + 1);
        let mut sums: Vec<Vec<f32>> = vec![Vec::new(); n];
        let mut secs = vec![0.0f64; n];
        for ((v, &seg), &who) in self.of_segment.iter().enumerate().zip(&clusters) {
            let e = &self.embeddings[v];
            let w = self.segments[seg].clean_sec();
            if sums[who].is_empty() {
                sums[who] = vec![0.0; e.len()];
            }
            if sums[who].len() == e.len() {
                for (acc, x) in sums[who].iter_mut().zip(e) {
                    *acc += x * w as f32;
                }
                secs[who] += w;
            }
        }

        let speakers = sums
            .into_iter()
            .zip(secs)
            .enumerate()
            .map(|(id, (mut v, speech_sec))| {
                let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm > 0.0 {
                    for x in &mut v {
                        *x /= norm;
                    }
                }
                Speaker {
                    id,
                    embedding: v,
                    speech_sec,
                }
            })
            .collect();

        Ok(Diarization { turns, speakers })
    }
}

/// Label a whole recording (handy for tests and short files; long ones are better pushed
/// through [`Runner`] in pieces — memory does not grow).
pub fn diarize(
    samples: &[f32],
    seg: &dyn Segmentation,
    emb: &dyn Embedder,
    opts: Options,
) -> Result<Diarization> {
    let mut r = Runner::new(seg, emb, opts);
    r.push(samples)?;
    r.finish()
}

/// Labelling of ONE TRACK of the recording: your own microphone (`source_id` = 0) or the
/// system audio (1). Tracks are diarized separately — they are different streams — and
/// brought together by [`merge`].
pub struct Track {
    pub source_id: u8,
    pub diarization: Diarization,
}

/// A session participant — already global, shared across both tracks.
#[derive(Debug, Clone)]
pub struct Participant {
    pub id: usize,
    pub embedding: Vec<f32>,
    pub speech_sec: f64,
    /// This is the OWNER («Я»).
    pub owner: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Merged {
    /// Who spoke when and on which track. The participant number is already global.
    pub turns: Vec<(u8, Turn)>,
    pub participants: Vec<Participant>,
}

/// How to name the participants in the transcript.
///
/// The order of preference is always the same: **the name the person gave** (the voice
/// profile) → **«Я»** for the owner → a faceless number. We never invent a name:
/// «Участник 2» is an honest «we do not know who this is», and it is better than a
/// plausible invention. The numbers run consecutively in order of appearance, with no
/// gaps: a participant who became «Иван» does not take up a number.
///
/// `lang` is the language of the RECORDING: in an English transcript «Участник 2» looks
/// absurd, and the summary over it is written in the language of the recording.
pub fn names(participants: &[Participant], known: &profiles::Profiles, lang: &str) -> Vec<String> {
    let (me, nth) = match lang {
        "ru" => ("Я", "Участник"),
        _ => ("Me", "Speaker"),
    };

    // One name — ONE participant of the recording. Several voices may turn out to be
    // similar to a profile (the recognition threshold is not identity), and then there
    // would be two «Ивановых» in the minutes: a person who does not exist, split in two.
    // The name goes to the most similar one; the rest honestly get a number.
    let mut best: std::collections::HashMap<&str, (usize, f32)> = Default::default();
    for (i, p) in participants.iter().enumerate() {
        if let Some((name, score)) = known.recognise(&p.embedding) {
            let e = best.entry(name).or_insert((i, score));
            if score > e.1 {
                *e = (i, score);
            }
        }
    }
    let named: std::collections::HashMap<usize, &str> =
        best.iter().map(|(name, (i, _))| (*i, *name)).collect();

    let mut n = 0;
    participants
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if let Some(name) = named.get(&i) {
                return (*name).to_string();
            }
            if p.owner {
                return me.to_string();
            }
            n += 1;
            format!("{nth} {n}")
        })
        .collect()
}

/// Fold the participants of the tracks into the same people.
///
/// **Why.** The voice of an interlocutor who is heard both from the speakers and in the
/// microphone (echo, open sound in the room) lands on BOTH tracks. Diarized separately,
/// they would give two different «participants» — one person cut in half. So the voice
/// centroids of all tracks are clustered once more, together: one voice — one person, no
/// matter how many sources he arrived from.
///
/// **Who «Я» is.** The owner is the one whose speech mostly arrived FROM HIS OWN
/// MICROPHONE. The microphone stands closest to precisely him, and on his own track he
/// dominates. The rule can be wrong at an in-person meeting where the laptop listens to
/// the whole room — and then the human renames the voice by hand ([`profiles`]). What
/// MUST NOT happen is silently declaring everyone who got into the microphone to be «me».
pub fn merge(tracks: Vec<Track>, opts: Options) -> Merged {
    let mut centroids: Vec<Vec<f32>> = Vec::new();
    // Where each centroid came from: (track, local participant number).
    let mut origin: Vec<(usize, usize)> = Vec::new();
    for (t, track) in tracks.iter().enumerate() {
        for s in &track.diarization.speakers {
            if s.embedding.is_empty() {
                continue;
            }
            centroids.push(s.embedding.clone());
            origin.push((t, s.id));
        }
    }
    if centroids.is_empty() {
        return Merged::default();
    }

    let global = cluster::agglomerative(&centroids, opts.distance, opts.max_speakers);
    let n = global.iter().copied().max().map_or(0, |m| m + 1);

    let mut of_local: std::collections::HashMap<(usize, usize), usize> = Default::default();
    for (i, &(t, local)) in origin.iter().enumerate() {
        of_local.insert((t, local), global[i]);
    }

    let mut sums: Vec<Vec<f32>> = vec![Vec::new(); n];
    let mut secs = vec![0.0f64; n];
    let mut from_mic = vec![0.0f64; n];
    let mut from_speakers = vec![0.0f64; n];
    for (i, &(t, local)) in origin.iter().enumerate() {
        let who = global[i];
        let track = &tracks[t];
        let Some(s) = track.diarization.speakers.iter().find(|s| s.id == local) else {
            continue;
        };
        if sums[who].is_empty() {
            sums[who] = vec![0.0; s.embedding.len()];
        }
        if sums[who].len() == s.embedding.len() {
            for (acc, x) in sums[who].iter_mut().zip(&s.embedding) {
                *acc += x * s.speech_sec as f32;
            }
            secs[who] += s.speech_sec;
            match track.source_id {
                0 => from_mic[who] += s.speech_sec,
                _ => from_speakers[who] += s.speech_sec,
            }
        }
    }

    // The owner is the one whose voice IS IN THE MICROPHONE AND IS NOT COMING OUT OF THE
    // SPEAKERS.
    //
    // This is not a heuristic about «who talked the most», it is physics: system audio is
    // what the machine PLAYS BACK, and it does not play back your own voice. So any voice
    // noticeably present in the system track came from OUTSIDE (a call, a video, music)
    // and cannot be the owner — however much of it got into the microphone from the room.
    //
    // The first version of the rule («talked into the microphone the most, and there is
    // more microphone than speakers») broke on a real recording: the microphone heard the
    // room where videos were playing, the same voices also came in over loopback, and the
    // narrator of the video became «me». A share threshold, not a plain comparison: the
    // segmentation of the two tracks counts seconds slightly differently, and a clean zero
    // on the speakers never happens.
    //
    // There may be no owner at all — if he did not say a word for the whole recording.
    // That is an honest answer: «Я» then simply goes to nobody.
    // We look for the owner ONLY among the real participants. Otherwise a three-second
    // scrap of noise from the microphone (which of course is not on the speakers) takes
    // the title of «Я» — and is immediately thrown out as a non-participant, leaving the
    // recording with no owner at all. Caught on a real recording.
    const OWNER_MAX_LOOPBACK: f64 = 0.05;

    // A voice SIMILAR to any of those that sounded from the speakers is not the owner
    // either — even if they did not merge into one cluster. Clustering is deliberately
    // strict (otherwise two different speakers become one person), and across channels —
    // microphone versus system audio — one and the same voice is NOTICEABLY LESS similar
    // to itself. Because of that its microphone copy stays a separate cluster with
    // «speakers 0.0», and by the loopback share alone it would look like the owner. So we
    // ask directly: is this same voice coming out of the speakers?
    let from_loopback: Vec<usize> = (0..n).filter(|&i| from_speakers[i] > 0.0).collect();
    let is_echo = |i: usize| {
        from_loopback.iter().any(|&j| {
            j != i
                && !sums[i].is_empty()
                && sums[i].len() == sums[j].len()
                && cluster::similarity(&sums[i], &sums[j]) >= opts.echo_similarity
        })
    };

    let echo: Vec<bool> = (0..n).map(is_echo).collect();

    let owner = from_mic
        .iter()
        .enumerate()
        .filter(|&(i, &mic)| {
            let total = mic + from_speakers[i];
            secs[i] >= opts.min_participant_sec
                && mic > 0.0
                && total > 0.0
                && from_speakers[i] / total <= OWNER_MAX_LOOPBACK
                && !echo[i]
        })
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, _)| i);

    // The numbers the decision was made on — into the log. Diarization thresholds are
    // picked on REAL recordings, not on gut feeling, and for that they have to be visible.
    for (i, p) in (0..n).zip(&sums) {
        let _ = p;
        tracing::debug!(
            "participant {i}: speech {:.1} s (microphone {:.1}, speakers {:.1}){}{}",
            secs[i],
            from_mic[i],
            from_speakers[i],
            if echo[i] { " [comes out of the speakers]" } else { "" },
            if owner == Some(i) { " ← owner" } else { "" }
        );
    }

    // Who is NOT a participant: a voice credited with a couple of seconds over the whole
    // recording. That is a scrap of music, a knock, a piece of someone else's phrase at a
    // seam — but not a person. Its lines stay unattributed, and it itself gets into
    // neither the roster nor the summary.
    let mut renumber = vec![None; n];
    let mut participants: Vec<Participant> = Vec::new();
    for (old, (mut v, speech_sec)) in sums.into_iter().zip(&secs).enumerate() {
        if *speech_sec < opts.min_participant_sec {
            tracing::debug!("voice with {speech_sec:.1} s of speech — not a participant, its lines stay unattributed");
            continue;
        }
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        }
        let id = participants.len();
        renumber[old] = Some(id);
        participants.push(Participant {
            id,
            embedding: v,
            speech_sec: *speech_sec,
            owner: owner == Some(old),
        });
    }

    let mut turns = Vec::new();
    for (t, track) in tracks.iter().enumerate() {
        for turn in &track.diarization.turns {
            let Some(&old) = of_local.get(&(t, turn.speaker)) else {
                continue;
            };
            let Some(who) = renumber[old] else {
                continue; // not a participant — the line stays unattributed
            };
            turns.push((
                track.source_id,
                Turn {
                    speaker: who,
                    ..turn.clone()
                },
            ));
        }
    }
    turns.sort_by(|a, b| a.1.start_sec.total_cmp(&b.1.start_sec));

    Merged {
        turns,
        participants,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake segmentation: «speech» is where the signal is louder than a threshold, and the
    /// voice is the first or the second depending on the sign. Lets us run the WHOLE
    /// pipeline without a weights file on disk. That is exactly why segmentation is a
    /// trait.
    struct Loudness;
    impl Segmentation for Loudness {
        fn frames(&self, window: &[f32]) -> Result<Vec<Frame>> {
            let n = window.len().div_ceil(FRAME_SHIFT);
            Ok((0..n)
                .map(|i| {
                    let a = i * FRAME_SHIFT;
                    let b = (a + FRAME_SHIFT).min(window.len());
                    let mean: f32 = window[a..b].iter().sum::<f32>() / (b - a).max(1) as f32;
                    match mean {
                        m if m > 0.3 => [true, false, false],  // loud — the first
                        m if m < -0.3 => [false, true, false], // «in antiphase» — the second
                        _ => [false, false, false],
                    }
                })
                .collect())
        }
    }

    /// Fake embedder: the voice is the mean sign of the signal.
    struct Sign;
    impl Embedder for Sign {
        fn embed(&self, samples: &[f32]) -> Result<Vec<f32>> {
            let mean: f32 = samples.iter().sum::<f32>() / samples.len().max(1) as f32;
            Ok(vec![mean, 1.0 - mean.abs()])
        }
    }

    /// Two minutes of conversation: the first speaks for 8 s, the second for 8 s, and so on
    /// round and round. We check everything at once — windows, clusters, timeline and
    /// profiles.
    fn conversation(secs: usize) -> Vec<f32> {
        (0..secs * SAMPLE_RATE as usize)
            .map(|i| {
                let sec = i / SAMPLE_RATE as usize;
                if (sec / 8) % 2 == 0 {
                    0.9
                } else {
                    -0.9
                }
            })
            .collect()
    }

    #[test]
    fn two_voices_become_two_people_on_the_timeline() {
        let samples = conversation(32);
        let d = diarize(&samples, &Loudness, &Sign, Options::default()).unwrap();

        assert_eq!(d.speakers.len(), 2, "the two voices did not come apart: {:?}", d.turns);
        let first = timeline::who_said(2.0, 6.0, &d.turns);
        let second = timeline::who_said(10.0, 14.0, &d.turns);
        assert!(first.is_some() && second.is_some(), "the timeline is empty: {:?}", d.turns);
        assert_ne!(first, second, "the second voice got the first one's number");
        // The same voice came back 16 seconds later — and must get back ITS OWN number.
        assert_eq!(
            timeline::who_said(18.0, 22.0, &d.turns),
            first,
            "the same voice in a new window became a new person"
        );
    }

    /// Memory must not grow with the length of the recording: frames and vectors are
    /// computed inside the window, and the recording itself is not held by `Runner`.
    #[test]
    fn a_long_recording_is_processed_in_windows_not_held_whole() {
        let samples = conversation(32);
        let mut r = Runner::new(&Loudness, &Sign, Options::default());
        for chunk in samples.chunks(5 * SAMPLE_RATE as usize) {
            r.push(chunk).unwrap();
            assert!(
                r.buf.len() <= WINDOW_SAMPLES + 5 * SAMPLE_RATE as usize,
                "the buffer grew to {} samples",
                r.buf.len()
            );
        }
        let d = r.finish().unwrap();
        assert_eq!(d.speakers.len(), 2);
    }

    /// A broken embedder must not bring down the labelling of the whole recording. An
    /// empty timeline is honest: the lines will simply be left without names.
    #[test]
    fn a_broken_embedder_yields_no_speakers_not_a_crash() {
        struct Broken;
        impl Embedder for Broken {
            fn embed(&self, _: &[f32]) -> Result<Vec<f32>> {
                anyhow::bail!("the model did not open")
            }
        }
        let d = diarize(&conversation(16), &Loudness, &Broken, Options::default()).unwrap();
        assert!(d.turns.is_empty() && d.speakers.is_empty());
    }

    /// The profile is built from ALL of the participant's speech and knows how many
    /// seconds went into it: a short profile must not be trusted.
    #[test]
    fn a_speaker_profile_knows_how_much_speech_it_is_built_from() {
        let d = diarize(&conversation(32), &Loudness, &Sign, Options::default()).unwrap();
        for s in &d.speakers {
            assert!(
                s.speech_sec > 8.0,
                "participant {} was assembled from only {:.1} s of speech",
                s.id,
                s.speech_sec
            );
            assert!((s.embedding.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-3);
        }
    }

    /// A voice that got into both the microphone and the speakers (echo) is ONE person, not
    /// two. Tracks diarized separately would give two «participants» — one person cut in
    /// half.
    #[test]
    fn the_same_voice_on_both_tracks_is_one_person() {
        let voice = |sign: f32| Speaker {
            id: 0,
            embedding: vec![sign, 0.0],
            speech_sec: 20.0,
        };
        let track = |source_id, speakers: Vec<Speaker>| Track {
            source_id,
            diarization: Diarization {
                turns: speakers
                    .iter()
                    .map(|s| Turn {
                        start_sec: 0.0,
                        end_sec: 20.0,
                        speaker: s.id,
                    })
                    .collect(),
                speakers,
            },
        };
        // The interlocutor (the same vector) is heard both from the speakers and in the
        // microphone.
        let mut echo = voice(1.0);
        echo.id = 1;
        let m = merge(
            vec![
                track(0, vec![voice(-1.0), echo]), // microphone: owner + interlocutor's echo
                track(1, vec![voice(1.0)]),        // speakers: the interlocutor
            ],
            Options::default(),
        );
        assert_eq!(m.participants.len(), 2, "the echo became a third person");
        let owner: Vec<_> = m.participants.iter().filter(|p| p.owner).collect();
        assert_eq!(owner.len(), 1, "there must be exactly one owner");
        // The owner is the one whose voice came FROM THE MICROPHONE and who is not on the
        // speakers.
        assert!(owner[0].embedding[0] < 0.0, "an interlocutor was appointed «me»");
    }

    /// We never invent a name: first the one the person gave, then «Я», and only then a
    /// faceless number. The numbers run consecutively — a participant who became «Иван»
    /// does not take up a number.
    #[test]
    fn a_named_voice_keeps_its_name_and_the_rest_get_honest_numbers() {
        let d = tempfile::tempdir().unwrap();
        profiles::enroll(d.path(), "Иван", &[0.0, 1.0], 60.0).unwrap();
        let known = profiles::load(d.path());

        let p = |id: usize, embedding: Vec<f32>, owner: bool| Participant {
            id,
            embedding,
            speech_sec: 30.0,
            owner,
        };
        let all = vec![
            p(0, vec![1.0, 0.0], true),  // the owner
            p(1, vec![0.0, 1.0], false), // Иван — recognised by voice
            // Similar to Иван (0.71 against a threshold of 0.70), but Иван is already in
            // the recording — and this is a DIFFERENT person, not a second Иван.
            p(2, vec![0.7, 0.7], false),
            p(3, vec![-1.0, 0.2], false),
        ];
        assert_eq!(
            names(&all, &known, "ru"),
            ["Я", "Иван", "Участник 1", "Участник 2"]
        );
        assert_eq!(names(&all, &known, "en")[0], "Me");
    }

    /// **A voice that is heard from the speakers can never be the owner.** Caught on a real
    /// recording: the microphone heard the room where videos were playing, the same voices
    /// also came in over the system audio, and the narrator of the video became «me». The
    /// machine does not play back its owner's own voice — so presence on the speakers rules
    /// out the owner, however much of him got into the microphone.
    #[test]
    fn a_voice_coming_out_of_the_speakers_is_never_the_owner() {
        let voice = |id: usize, sign: f32, sec: f64| Speaker {
            id,
            embedding: vec![sign, 0.0],
            speech_sec: sec,
        };
        let track = |source_id: u8, speakers: Vec<Speaker>| Track {
            source_id,
            diarization: Diarization {
                turns: speakers
                    .iter()
                    .map(|s| Turn {
                        start_sec: 0.0,
                        end_sec: s.speech_sec,
                        speaker: s.id,
                    })
                    .collect(),
                speakers,
            },
        };
        // The narrator of the video sounds loudly IN THE ROOM (60 s in the microphone!) and
        // he is also in the system audio. The owner said only 20 s, and only into the
        // microphone.
        let m = merge(
            vec![
                track(0, vec![voice(0, 1.0, 60.0), voice(1, -1.0, 20.0)]),
                track(1, vec![voice(0, 1.0, 55.0)]),
            ],
            Options::default(),
        );
        let owner: Vec<_> = m.participants.iter().filter(|p| p.owner).collect();
        assert_eq!(owner.len(), 1, "there must be exactly one owner");
        assert!(
            owner[0].embedding[0] < 0.0,
            "a voice that came out of the speakers was appointed «me»"
        );
    }

    /// There may be no owner at all: if he did not say a word for the whole recording, «Я»
    /// goes to nobody. Appointing someone else's voice the owner is worse than staying
    /// silent.
    #[test]
    fn a_recording_where_the_owner_never_spoke_has_no_owner() {
        let s = |id: usize, sign: f32| Speaker {
            id,
            embedding: vec![sign, 0.0],
            speech_sec: 40.0,
        };
        let track = |source_id: u8, speakers: Vec<Speaker>| Track {
            source_id,
            diarization: Diarization {
                turns: speakers
                    .iter()
                    .map(|x| Turn {
                        start_sec: 0.0,
                        end_sec: 40.0,
                        speaker: x.id,
                    })
                    .collect(),
                speakers,
            },
        };
        // Everything the microphone heard was the speakers.
        let m = merge(
            vec![track(0, vec![s(0, 1.0)]), track(1, vec![s(0, 1.0)])],
            Options::default(),
        );
        assert!(
            m.participants.iter().all(|p| !p.owner),
            "the owner became someone who never spoke into the microphone at all"
        );
    }

    /// A scrap of noise from the microphone must not carry off the title of «Я».
    ///
    /// Caught on a real recording: a three-second scrap (which of course is not on the
    /// speakers) took the owner — and was immediately thrown out as a non-participant. The
    /// recording was left with no «Я» at all, even though the owner did speak in it.
    #[test]
    fn a_scrap_of_noise_cannot_steal_the_owner_title() {
        let s = |id: usize, e: Vec<f32>, sec: f64| Speaker {
            id,
            embedding: e,
            speech_sec: sec,
        };
        let m = merge(
            vec![Track {
                source_id: 0,
                diarization: Diarization {
                    turns: vec![
                        Turn { start_sec: 0.0, end_sec: 120.0, speaker: 0 },
                        Turn { start_sec: 130.0, end_sec: 133.0, speaker: 1 },
                    ],
                    speakers: vec![
                        s(0, vec![1.0, 0.0], 120.0), // the owner spoke for two minutes
                        s(1, vec![0.0, 1.0], 3.0),   // and somewhere something knocked
                    ],
                },
            }],
            Options::default(),
        );
        assert_eq!(m.participants.len(), 1, "the scrap became a participant");
        assert!(
            m.participants[0].owner,
            "the owner lost «Я» because of three seconds of noise"
        );
    }

    /// A voice credited with a couple of seconds over the whole recording is not a person
    /// but a scrap of music or a knock. Declaring a participant from it means seating at
    /// the table someone who was not at the meeting.
    #[test]
    fn a_two_second_noise_does_not_become_a_participant() {
        let m = merge(
            vec![Track {
                source_id: 1,
                diarization: Diarization {
                    speakers: vec![
                        Speaker {
                            id: 0,
                            embedding: vec![1.0, 0.0],
                            speech_sec: 90.0,
                        },
                        Speaker {
                            id: 1,
                            embedding: vec![0.0, 1.0],
                            speech_sec: 1.1, // a scrap
                        },
                    ],
                    turns: vec![
                        Turn { start_sec: 0.0, end_sec: 90.0, speaker: 0 },
                        Turn { start_sec: 95.0, end_sec: 96.1, speaker: 1 },
                    ],
                },
            }],
            Options::default(),
        );
        assert_eq!(m.participants.len(), 1, "the scrap became a participant");
        assert!(
            timeline::who_said(95.0, 96.0, &m.turns.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>())
                .is_none(),
            "the scrap's line got attributed"
        );
    }

    /// Silence is not a participant.
    #[test]
    fn silence_is_not_a_speaker() {
        let d = diarize(&vec![0.0; 30 * SAMPLE_RATE as usize], &Loudness, &Sign, Options::default())
            .unwrap();
        assert!(d.speakers.is_empty(), "silence became a participant");
    }
}
