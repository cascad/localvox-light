//! Clustering of voice embeddings: «how many people are here and who is who».
//!
//! This is the only part of diarization that depends neither on a model nor on a library
//! — pure arithmetic over vectors. That is why it was written and covered by tests first:
//! it can be proven without a single model on disk.
//!
//! **Agglomerative clustering by cosine, average-linkage.** We start with every stretch
//! of speech being its own person, and merge the two nearest clusters over and over until
//! the nearest ones turn out to be farther apart than the threshold. The threshold IS the
//! answer to «how many people»: we do NOT KNOW the number of speakers in advance, and we
//! must not ask the human for it — he did not count them either.
//!
//! Why average-linkage and not single: single-linkage glues clusters together in a chain
//! (the «chaining effect») — one borderline stretch joins two different people into one.
//! Average is robust to that and is cheap on a few dozen stretches.

/// Cosine distance: 0 — the same, 2 — the opposite.
/// The embedding vectors are not normalised, so we normalise here.
fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return 1.0; // a zero vector is similar to nothing
    }
    1.0 - dot / (na * nb)
}

/// How many people and who is who.
///
/// `threshold` — the cosine distance beyond which voices are considered different.
/// Returns one cluster label per embedding.
///
/// `max_speakers` — the cap: a call never has forty people, but forty false clusters from
/// noise — that does happen. Zero or `None` — no cap.
pub fn agglomerative(embeddings: &[Vec<f32>], threshold: f32, max_speakers: Option<usize>) -> Vec<usize> {
    let n = embeddings.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![0];
    }

    // Every stretch is its own cluster.
    let mut members: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();

    loop {
        // The nearest pair of clusters (average-linkage).
        let mut best: Option<(usize, usize, f32)> = None;
        for i in 0..members.len() {
            for j in (i + 1)..members.len() {
                let d = average_linkage(embeddings, &members[i], &members[j]);
                if best.is_none_or(|(_, _, bd)| d < bd) {
                    best = Some((i, j, d));
                }
            }
        }
        let Some((i, j, d)) = best else { break };

        // We stop when the nearest ones are already farther apart than the threshold — OR
        // when there are already more clusters than there can be people in the room.
        let over_cap = max_speakers.is_some_and(|m| m > 0 && members.len() > m);
        if d > threshold && !over_cap {
            break;
        }
        if members.len() == 1 {
            break;
        }

        // Merge j into i (j is always to the right — remove it first).
        let moved = members.remove(j);
        members[i].extend(moved);
    }

    let mut labels = vec![0usize; n];
    for (label, group) in members.iter().enumerate() {
        for &idx in group {
            labels[idx] = label;
        }
    }
    labels
}

/// The mean distance between all pairs of two clusters.
fn average_linkage(emb: &[Vec<f32>], a: &[usize], b: &[usize]) -> f32 {
    let mut sum = 0.0;
    for &i in a {
        for &j in b {
            sum += cosine_distance(&emb[i], &emb[j]);
        }
    }
    sum / (a.len() * b.len()) as f32
}

/// A cluster centroid — the averaged and NORMALISED vector.
///
/// Needed for persistent profiles: «this is Иван again» is decided by comparison with the
/// centroid, not with one random stretch of his speech.
pub fn centroid(embeddings: &[Vec<f32>], members: &[usize]) -> Vec<f32> {
    if members.is_empty() || embeddings.is_empty() {
        return Vec::new();
    }
    let dim = embeddings[0].len();
    let mut c = vec![0.0f32; dim];
    for &i in members {
        for (k, v) in embeddings[i].iter().enumerate() {
            c[k] += v;
        }
    }
    let norm: f32 = c.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut c {
            *v /= norm;
        }
    }
    c
}

/// The similarity of two voices, 0..1 (1 — the same person). For profiles and thresholds.
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    1.0 - cosine_distance(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three stretches of one voice and two of another — they MUST split into TWO people.
    /// We do not know the number of speakers in advance: it is found by the threshold, not
    /// by a hint.
    #[test]
    fn two_voices_are_found_without_being_told_how_many() {
        let alice = vec![1.0, 0.0, 0.0, 0.0];
        let bob = vec![0.0, 1.0, 0.0, 0.0];
        let jitter = |v: &Vec<f32>, d: f32| {
            let mut x = v.clone();
            x[2] += d; // slight noise, as between different phrases of the same person
            x
        };
        let emb = vec![
            jitter(&alice, 0.05),
            jitter(&bob, 0.03),
            jitter(&alice, -0.04),
            jitter(&alice, 0.02),
            jitter(&bob, -0.05),
        ];
        let labels = agglomerative(&emb, 0.5, Some(6));

        assert_eq!(labels[0], labels[2], "two of Alice's phrases came apart");
        assert_eq!(labels[0], labels[3]);
        assert_eq!(labels[1], labels[4], "two of Bob's phrases came apart");
        assert_ne!(labels[0], labels[1], "Alice and Bob stuck together into one");

        let n: std::collections::BTreeSet<_> = labels.iter().collect();
        assert_eq!(n.len(), 2, "{} people found instead of two", n.len());
    }

    /// One person, many turns — must stay ONE. Splitting a monologue into five
    /// «participants» is worse than not splitting it at all: the summary will attribute the
    /// lines to invented people.
    #[test]
    fn one_voice_does_not_split_into_a_crowd() {
        let base = vec![0.9, 0.1, 0.2, 0.3];
        let emb: Vec<Vec<f32>> = (0..6)
            .map(|i| {
                let mut v = base.clone();
                v[1] += i as f32 * 0.01;
                v
            })
            .collect();
        let labels = agglomerative(&emb, 0.5, Some(6));
        let n: std::collections::BTreeSet<_> = labels.iter().collect();
        assert_eq!(n.len(), 1, "the monologue was split into {} people", n.len());
    }

    /// The cap on participants: a call never has forty people, but forty false clusters
    /// from noise — that does happen.
    #[test]
    fn the_speaker_cap_is_respected() {
        // ten mutually distant vectors — without a cap they would become ten people
        let emb: Vec<Vec<f32>> = (0..10)
            .map(|i| {
                let mut v = vec![0.0f32; 10];
                v[i] = 1.0;
                v
            })
            .collect();
        let labels = agglomerative(&emb, 0.5, Some(3));
        let n: std::collections::BTreeSet<_> = labels.iter().collect();
        assert!(n.len() <= 3, "the cap did not work: {} clusters", n.len());
    }

    #[test]
    fn a_centroid_recognises_the_same_voice_again() {
        // A profile is built from SEVERAL stretches: one random piece of speech is a poor
        // anchor (a cough, a scrap, an «uh-huh»).
        let alice: Vec<Vec<f32>> = vec![
            vec![1.0, 0.1, 0.0],
            vec![0.9, 0.2, 0.1],
            vec![1.0, 0.0, 0.1],
        ];
        let profile = centroid(&alice, &[0, 1, 2]);

        let alice_again = vec![0.95, 0.15, 0.05];
        let bob = vec![0.0, 1.0, 0.0];
        assert!(
            similarity(&profile, &alice_again) > 0.9,
            "one's own voice was not recognised: {}",
            similarity(&profile, &alice_again)
        );
        assert!(
            similarity(&profile, &bob) < 0.5,
            "a stranger's voice was taken for one's own: {}",
            similarity(&profile, &bob)
        );
    }

    #[test]
    fn an_empty_input_is_not_a_crash() {
        assert!(agglomerative(&[], 0.5, None).is_empty());
        assert_eq!(agglomerative(&[vec![1.0, 0.0]], 0.5, None), vec![0]);
    }
}
