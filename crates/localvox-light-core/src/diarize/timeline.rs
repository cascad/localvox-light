//! From the labelling of windows to a timeline of «from second X to second Y participant
//! N is speaking».
//!
//! There is not a single model here: pure arithmetic over frames. That is why everything
//! below is proven by tests without a single weights file on disk — just like the
//! clustering.
//!
//! **How local voices become people.** The segmentation model only sees its own window:
//! «A» in the fifth window and «A» in the sixth are different labels, and we are NOT
//! GOING to link them with permutations. They will be linked by the voice: for every
//! (window, local voice) an embedding is computed, all embeddings are clustered at once
//! over the whole recording, and the cluster number IS the person. Permutation stitching
//! of neighbouring windows is a patch for those who have no embeddings; we do have them.

use super::segment::{Frame, Window, FRAME_SHIFT, LOCAL_SPEAKERS, RECEPTIVE_FIELD, SAMPLE_RATE};

/// The speech of ONE local voice inside ONE window — a candidate for an embedding.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalSegment {
    pub window: usize,
    pub local: usize,
    /// The pieces where this voice sounds ALONE (in samples from the start of the
    /// recording).
    ///
    /// Overlap frames are thrown away deliberately: a mix of two voices gives the
    /// embedding of a person who does not exist, and it will drag the clustering off into
    /// nowhere.
    pub clean: Vec<(usize, usize)>,
}

impl LocalSegment {
    pub fn clean_samples(&self) -> usize {
        self.clean.iter().map(|(a, b)| b - a).sum()
    }
    pub fn clean_sec(&self) -> f64 {
        self.clean_samples() as f64 / SAMPLE_RATE as f64
    }
    /// Collect this voice's clean speech into one buffer — the embedder's input.
    pub fn audio(&self, samples: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.clean_samples());
        for &(a, b) in &self.clean {
            let (a, b) = (a.min(samples.len()), b.min(samples.len()));
            out.extend_from_slice(&samples[a..b]);
        }
        out
    }
}

/// Who speaks from which second to which. The stretches of DIFFERENT participants may
/// overlap — that is overlapping speech, not an error.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub start_sec: f64,
    pub end_sec: f64,
    pub speaker: usize,
}

/// Assembly thresholds. All in seconds, because it is a human who tunes them.
#[derive(Debug, Clone, Copy)]
pub struct Smoothing {
    /// A burst of speech shorter than this is noise (a knock, a cough, a scrap of someone
    /// else's word).
    pub min_on_sec: f64,
    /// A gap inside a turn shorter than this pause is sewn up: the person is breathing, not
    /// finishing his sentence.
    pub min_off_sec: f64,
}

impl Default for Smoothing {
    fn default() -> Self {
        Self {
            min_on_sec: 0.20,
            min_off_sec: 0.25,
        }
    }
}

/// The pieces of clean speech of every local voice of every window.
///
/// `min_run` — the minimum length of a continuous piece in samples: a 50 ms scrap
/// describes a consonant sound, not a voice.
pub fn local_segments(windows: &[Window], min_run: usize) -> Vec<LocalSegment> {
    let mut out = Vec::new();
    for (w, win) in windows.iter().enumerate() {
        for local in 0..LOCAL_SPEAKERS {
            let mut clean: Vec<(usize, usize)> = Vec::new();
            let mut prev: Option<usize> = None;
            for (i, f) in win.frames.iter().enumerate() {
                if !alone(f, local) {
                    continue;
                }
                let (s, e) = win.frame_span(i);
                match clean.last_mut() {
                    // We merge ONLY adjacent frames. The receptive fields overlap (991
                    // samples against a shift of 270), and «merge everything that
                    // intersects» would mean jumping over overlapping speech — the very
                    // thing we came here to exclude.
                    Some(last) if prev == Some(i - 1) => last.1 = e,
                    _ => clean.push((s, e)),
                }
                prev = Some(i);
            }
            clean.retain(|(a, b)| b - a >= min_run);
            if !clean.is_empty() {
                out.push(LocalSegment {
                    window: w,
                    local,
                    clean,
                });
            }
        }
    }
    out
}

/// The voice sounds ON ITS OWN (not in an overlap).
fn alone(f: &Frame, local: usize) -> bool {
    f[local] && f.iter().filter(|x| **x).count() == 1
}

/// The «who speaks when» timeline from the window labelling and the cluster labels.
///
/// `labels[i]` — the participant number for `segments[i]`; `None` — a voice that did not
/// have enough clean speech for an embedding. Its frames stay WITHOUT A NAME, and that is
/// honest: attributing a line at random is worse than not attributing it at all.
///
/// A frame is credited to a participant if at least half of the windows that saw this
/// frame voted for him. The windows overlap, and a single false burst in one window must
/// not become a turn.
pub fn turns(
    windows: &[Window],
    segments: &[LocalSegment],
    labels: &[Option<usize>],
    total_samples: usize,
    smooth: Smoothing,
) -> Vec<Turn> {
    debug_assert_eq!(segments.len(), labels.len());
    let n_speakers = labels.iter().flatten().copied().max().map_or(0, |m| m + 1);
    if n_speakers == 0 || total_samples == 0 {
        return Vec::new();
    }

    let grid = total_samples.div_ceil(FRAME_SHIFT) + 1;
    let mut votes = vec![vec![0u16; grid]; n_speakers];
    let mut seen = vec![0u16; grid];

    // Which of the local voices of which window turned out to be which participant.
    let mut label_of = std::collections::HashMap::new();
    for (seg, label) in segments.iter().zip(labels) {
        if let Some(l) = label {
            label_of.insert((seg.window, seg.local), *l);
        }
    }

    for (w, win) in windows.iter().enumerate() {
        for (i, f) in win.frames.iter().enumerate() {
            let g = cell(win.offset + i * FRAME_SHIFT, grid);
            seen[g] = seen[g].saturating_add(1);
            for (local, _) in f.iter().enumerate().filter(|(_, active)| **active) {
                if let Some(&l) = label_of.get(&(w, local)) {
                    votes[l][g] = votes[l][g].saturating_add(1);
                }
            }
        }
    }

    let mut out = Vec::new();
    for (speaker, v) in votes.iter().enumerate() {
        let active: Vec<bool> = v
            .iter()
            .zip(&seen)
            .map(|(&yes, &all)| all > 0 && yes * 2 >= all)
            .collect();
        for (a, b) in runs(&active, smooth) {
            out.push(Turn {
                start_sec: cell_start_sec(a),
                end_sec: cell_start_sec(b),
                speaker,
            });
        }
    }
    out.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    out
}

/// The cell of the frame grid a sample falls into (the centre of the receptive field).
fn cell(frame_start: usize, grid: usize) -> usize {
    ((frame_start + RECEPTIVE_FIELD / 2) / FRAME_SHIFT).min(grid - 1)
}

fn cell_start_sec(cell: usize) -> f64 {
    (cell * FRAME_SHIFT) as f64 / SAMPLE_RATE as f64
}

/// Continuous pieces of activity: first we sew up the short gaps, then we throw out the
/// short bursts. The order matters: otherwise a turn with a breath in the middle falls
/// apart into two scraps, each of which is taken for noise.
fn runs(active: &[bool], smooth: Smoothing) -> Vec<(usize, usize)> {
    let per_cell = FRAME_SHIFT as f64 / SAMPLE_RATE as f64;
    let min_on = (smooth.min_on_sec / per_cell).round() as usize;
    let min_off = (smooth.min_off_sec / per_cell).round() as usize;

    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (i, &on) in active.iter().enumerate() {
        match (on, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                spans.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        spans.push((s, active.len()));
    }

    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in spans {
        match merged.last_mut() {
            Some(last) if s - last.1 <= min_off => last.1 = e,
            _ => merged.push((s, e)),
        }
    }
    merged.retain(|(s, e)| e - s >= min_on);
    merged
}

/// Who uttered a transcript line.
///
/// The winner is the participant who accounts for the MAJORITY of the overlap in time. If
/// two people covered the line roughly equally — there is NO label: a line with a speaker
/// change in the middle belongs to nobody as a whole, and attributing it to one of them
/// means putting someone else's words into his mouth.
pub fn who_said(start_sec: f64, end_sec: f64, turns: &[Turn]) -> Option<usize> {
    let mut by_speaker: std::collections::BTreeMap<usize, f64> = Default::default();
    for t in turns {
        let o = (end_sec.min(t.end_sec) - start_sec.max(t.start_sec)).max(0.0);
        if o > 0.0 {
            *by_speaker.entry(t.speaker).or_default() += o;
        }
    }
    let total: f64 = by_speaker.values().sum();
    if total <= 0.0 {
        return None;
    }
    let (who, best) = by_speaker
        .iter()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(k, v)| (*k, *v))?;
    (best / total >= 0.6).then_some(who)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window of frames written out «by hand»: `..A`, `.B.`, `AB.`
    fn window(offset: usize, pattern: &[[bool; 3]]) -> Window {
        Window {
            offset,
            frames: pattern.to_vec(),
        }
    }

    const NONE: Frame = [false, false, false];
    const A: Frame = [true, false, false];
    const B: Frame = [false, true, false];
    const AB: Frame = [true, true, false];

    /// A mix of two voices gives the embedding of a person who does not exist — overlap
    /// frames MUST NOT get into the clean speech.
    #[test]
    fn overlapping_speech_is_excluded_from_the_voice_print() {
        let w = window(0, &[A, A, AB, AB, A]);
        let segs = local_segments(&[w], 0);
        let a = segs.iter().find(|s| s.local == 0).unwrap();
        // Two clean pieces (before the overlap and after), the overlap is thrown away.
        assert_eq!(a.clean.len(), 2, "overlap frames got into the clean speech");
        // B sounded ONLY in the overlap — it has no clean speech at all.
        assert!(
            segs.iter().all(|s| s.local != 1),
            "a voice that only sounded in an overlap got «clean» speech"
        );
    }

    /// A half-frame scrap describes a consonant sound, not a voice.
    #[test]
    fn a_scrap_of_speech_is_not_a_voice_print() {
        let w = window(0, &[A, NONE, NONE, NONE]);
        let long = local_segments(&[w], 16_000); // we demand a second
        assert!(long.is_empty(), "a scrap was taken for a voice");
    }

    /// A voice that did not have enough clean speech for an embedding stays WITHOUT A NAME.
    /// Attributing its lines at random is worse than not attributing them at all.
    #[test]
    fn a_voice_we_could_not_measure_stays_nameless() {
        let w = window(0, &[A, A, B, B]);
        let segs = local_segments(&[w], 0);
        assert_eq!(segs.len(), 2);
        // B could not be clustered — there is no label.
        let labels = vec![Some(0), None];
        let t = turns(&[window(0, &[A, A, B, B])], &segs, &labels, 4 * FRAME_SHIFT, Smoothing { min_on_sec: 0.0, min_off_sec: 0.0 });
        assert!(t.iter().all(|t| t.speaker == 0), "a nameless voice got a number");
    }

    /// A breath in the middle of a turn is not the end of the turn.
    #[test]
    fn a_breath_does_not_end_a_sentence() {
        let mut pattern = vec![A; 60]; // ~1 s
        pattern.extend(vec![NONE; 5]); // ~85 ms of pause
        pattern.extend(vec![A; 60]);
        let w = window(0, &pattern);
        let segs = local_segments(&[w], 0);
        let labels: Vec<Option<usize>> = segs.iter().map(|_| Some(0)).collect();
        let t = turns(
            &[window(0, &pattern)],
            &segs,
            &labels,
            pattern.len() * FRAME_SHIFT,
            Smoothing::default(),
        );
        assert_eq!(t.len(), 1, "the turn fell apart on a breath: {t:?}");
    }

    /// A line that two people covered equally belongs to nobody: attributing it to one of
    /// them means putting someone else's words into his mouth.
    #[test]
    fn a_line_split_between_two_people_is_left_unattributed() {
        let turns = vec![
            Turn { start_sec: 0.0, end_sec: 5.0, speaker: 0 },
            Turn { start_sec: 5.0, end_sec: 10.0, speaker: 1 },
        ];
        assert_eq!(who_said(0.0, 4.0, &turns), Some(0));
        assert_eq!(who_said(6.0, 9.0, &turns), Some(1));
        assert_eq!(who_said(4.0, 6.0, &turns), None, "50/50 — and yet a label was found");
        // Slightly catching someone else's turn does not take the line away from the
        // speaker.
        assert_eq!(who_said(0.0, 5.5, &turns), Some(0));
        // Silence beyond the timeline — nobody.
        assert_eq!(who_said(20.0, 25.0, &turns), None);
    }
}
