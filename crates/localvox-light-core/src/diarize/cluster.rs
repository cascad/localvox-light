//! Clustering of voice embeddings: «how many people are here and who is who».
//!
//! Model-independent cosine distances and average-linkage clustering. Correctness and
//! scaling can be checked without a weights file or an audio device.
//!
//! **Agglomerative clustering by cosine, average-linkage.** We start with every stretch
//! of speech being its own person, and merge the two nearest clusters over and over until
//! the nearest ones turn out to be farther apart than the threshold. The threshold IS the
//! answer to «how many people»: we do NOT KNOW the number of speakers in advance, and we
//! must not ask the human for it — he did not count them either.
//!
//! Why average-linkage and not single: single-linkage glues clusters together in a chain
//! (the «chaining effect») — one borderline stretch joins two different people into one.
//! Average is robust to that. A condensed distance matrix and nearest-neighbor chain
//! keep the work quadratic even when a long recording contains thousands of stretches.

use anyhow::{ensure, Context, Result};

/// Bound the additional quadratic workspace, independently of recording duration.
/// A larger input is an ordinary diarization error; the caller can still save ASR.
const MAX_DISTANCE_BYTES: usize = 256 * 1024 * 1024;

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
pub fn agglomerative(
    embeddings: &[Vec<f32>],
    threshold: f32,
    max_speakers: Option<usize>,
) -> Result<Vec<usize>> {
    let n = embeddings.len();
    let pairs = distance_pairs(n, MAX_DISTANCE_BYTES)?;
    ensure!(
        threshold.is_finite(),
        "non-finite speaker distance threshold"
    );
    let dimensions = embeddings.first().map_or(0, Vec::len);
    let norms: Vec<f64> = embeddings
        .iter()
        .map(|v| {
            ensure!(
                !v.is_empty() && v.len() == dimensions && v.iter().all(|x| x.is_finite()),
                "invalid voice embedding: dimensions or non-finite values"
            );
            Ok(v.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>().sqrt())
        })
        .collect::<Result<_>>()?;
    if n == 0 {
        return Ok(Vec::new());
    }
    if n == 1 {
        return Ok(vec![0]);
    }

    let mut distances = Vec::new();
    distances
        .try_reserve_exact(pairs)
        .context("allocating speaker distance matrix")?;
    for i in 0..n - 1 {
        for j in i + 1..n {
            let distance = if norms[i] == 0.0 || norms[j] == 0.0 {
                1.0
            } else {
                let dot: f64 = embeddings[i]
                    .iter()
                    .zip(&embeddings[j])
                    .map(|(&a, &b)| f64::from(a) * f64::from(b))
                    .sum();
                1.0 - dot / (norms[i] * norms[j])
            };
            distances.push(distance);
        }
    }
    // Average linkage admits an exact O(n²) nearest-neighbor-chain algorithm.
    // Kodama sorts/relabels its dendrogram before returning it; cuts must be applied
    // to that ordered hierarchy, never to the out-of-order reciprocal merges.
    let tree = kodama::linkage(&mut distances, n, kodama::Method::Average);
    let mut parents: Vec<usize> = (0..n).collect();
    let mut representatives = vec![0; 2 * n - 1];
    for (i, item) in representatives.iter_mut().take(n).enumerate() {
        *item = i;
    }
    let mut groups = n;
    for (index, step) in tree.steps().iter().enumerate() {
        let over_cap = max_speakers.is_some_and(|m| m > 0 && groups > m);
        if step.dissimilarity > f64::from(threshold) && !over_cap {
            break;
        }
        let a = root(&mut parents, representatives[step.cluster1]);
        let b = root(&mut parents, representatives[step.cluster2]);
        // Minimum input index determines the public local label, independently of
        // the library's internal cluster numbering and merge scheduling.
        parents[a.max(b)] = a.min(b);
        representatives[n + index] = a.min(b);
        groups -= 1;
    }
    let mut labels = Vec::with_capacity(n);
    let mut local = vec![usize::MAX; n];
    let mut next = 0;
    for i in 0..n {
        let r = root(&mut parents, i);
        if local[r] == usize::MAX {
            local[r] = next;
            next += 1;
        }
        labels.push(local[r]);
    }
    Ok(labels)
}

fn distance_pairs(n: usize, budget: usize) -> Result<usize> {
    let pairs = n
        .checked_mul(n.saturating_sub(1))
        .map(|v| v / 2)
        .context("speaker distance matrix size overflow")?;
    let bytes = pairs
        .checked_mul(std::mem::size_of::<f64>())
        .context("speaker distance matrix byte size overflow")?;
    ensure!(bytes <= budget,
        "speaker distance matrix needs {bytes} bytes for {n} fragments; budget is {budget} bytes; transcript remains available without speaker labels");
    Ok(pairs)
}

fn root(parents: &mut [usize], mut item: usize) -> usize {
    while parents[item] != item {
        parents[item] = parents[parents[item]];
        item = parents[item];
    }
    item
}

#[cfg(test)]
fn reference(embeddings: &[Vec<f32>], threshold: f32, max_speakers: Option<usize>) -> Vec<usize> {
    let n = embeddings.len();
    let mut members: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    loop {
        let mut best: Option<(usize, usize, f32)> = None;
        for i in 0..members.len() {
            for j in i + 1..members.len() {
                let mut sum = 0.0;
                for &a in &members[i] {
                    for &b in &members[j] {
                        sum += cosine_distance(&embeddings[a], &embeddings[b]);
                    }
                }
                let distance = sum / (members[i].len() * members[j].len()) as f32;
                if best.is_none_or(|(_, _, previous)| distance < previous) {
                    best = Some((i, j, distance));
                }
            }
        }
        let Some((i, j, d)) = best else { break };
        let over_cap = max_speakers.is_some_and(|m| m > 0 && members.len() > m);
        if d > threshold && !over_cap {
            break;
        }
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

    fn vectors(n: usize, dimensions: usize, seed: u64) -> Vec<Vec<f32>> {
        let mut state = seed;
        (0..n)
            .map(|i| {
                (0..dimensions)
                    .map(|d| {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        let jitter = (state >> 40) as f32 / (1u32 << 24) as f32 - 0.5;
                        if d == i % 6 {
                            1.0 + jitter * 0.15
                        } else {
                            jitter * 0.15
                        }
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn average_linkage_matches_original_partitions() {
        for seed in 1..=24 {
            let input = vectors(36, 12, seed);
            for threshold in [0.001, 0.4, 0.95, 1.1] {
                for cap in [None, Some(0), Some(1), Some(3), Some(8)] {
                    assert_eq!(
                        agglomerative(&input, threshold, cap).unwrap(),
                        reference(&input, threshold, cap),
                        "seed={seed} threshold={threshold} cap={cap:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn zero_vectors_ties_and_threshold_boundary_are_deterministic() {
        let input = vec![vec![0.0; 3]; 8];
        assert_eq!(
            agglomerative(&input, 0.99, None).unwrap(),
            (0..8).collect::<Vec<_>>()
        );
        assert_eq!(agglomerative(&input, 1.0, None).unwrap(), vec![0; 8]);
        let capped = agglomerative(&input, 0.5, Some(3)).unwrap();
        assert_eq!(capped.iter().max(), Some(&2));
        for _ in 0..10 {
            assert_eq!(agglomerative(&input, 0.5, Some(3)).unwrap(), capped);
        }
        assert_eq!(
            agglomerative(&[vec![1.0, 0.0], vec![0.0, 1.0]], 1.0, None).unwrap(),
            vec![0, 0]
        );
    }

    #[test]
    fn invalid_inputs_and_oversize_matrix_return_errors() {
        for input in [
            vec![vec![]],
            vec![vec![1.0], vec![1.0, 2.0]],
            vec![vec![f32::NAN]],
            vec![vec![f32::INFINITY]],
        ] {
            assert!(agglomerative(&input, 0.4, Some(8)).is_err());
        }
        assert!(agglomerative(&[vec![1.0]], f32::NAN, None).is_err());
        assert!(distance_pairs(8192, MAX_DISTANCE_BYTES).is_ok());
        assert!(distance_pairs(8193, MAX_DISTANCE_BYTES).is_err());
        assert!(distance_pairs(usize::MAX, MAX_DISTANCE_BYTES).is_err());
        // Budget check precedes inspecting/allocating any matrix from the input.
        assert!(agglomerative(&vec![vec![]; 8193], 0.4, None)
            .unwrap_err()
            .to_string()
            .contains("budget"));
        assert_eq!(
            agglomerative(&[vec![f32::MAX], vec![f32::MAX]], 0.4, None).unwrap(),
            vec![0, 0]
        );
    }

    #[test]
    fn thousands_of_fragments_keep_six_distinct_voices() {
        let input = vectors(2048, 32, 42);
        let labels = agglomerative(&input, 0.4, Some(8)).unwrap();
        for (i, &label) in labels.iter().enumerate() {
            assert_eq!(label, i % 6);
        }
    }

    /// Explicit release measurement, excluded from normal CI and never touches an archive/model.
    #[test]
    #[ignore = "release clustering cost measurement"]
    fn profile_long_recording_cluster() {
        for n in [200, 400, 2000, 4000, 8000] {
            let input = vectors(n, 256, 42);
            let start = std::time::Instant::now();
            let labels = agglomerative(&input, 0.4, Some(8)).unwrap();
            let seconds = start.elapsed().as_secs_f64();
            assert!(labels.iter().enumerate().all(|(i, &label)| label == i % 6));
            let reference_seconds = if n <= 400 {
                let start = std::time::Instant::now();
                assert_eq!(labels, reference(&input, 0.4, Some(8)));
                Some(start.elapsed().as_secs_f64())
            } else {
                None
            };
            println!(
                "{}",
                serde_json::json!({"vectors":n,"dimensions":256,"matrix_bytes":distance_pairs(n,MAX_DISTANCE_BYTES).unwrap()*8,"seconds":seconds,"reference_seconds":reference_seconds,"voices":6})
            );
        }
    }

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
        let labels = agglomerative(&emb, 0.5, Some(6)).unwrap();

        assert_eq!(labels[0], labels[2], "two of Alice's phrases came apart");
        assert_eq!(labels[0], labels[3]);
        assert_eq!(labels[1], labels[4], "two of Bob's phrases came apart");
        assert_ne!(
            labels[0], labels[1],
            "Alice and Bob stuck together into one"
        );

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
        let labels = agglomerative(&emb, 0.5, Some(6)).unwrap();
        let n: std::collections::BTreeSet<_> = labels.iter().collect();
        assert_eq!(
            n.len(),
            1,
            "the monologue was split into {} people",
            n.len()
        );
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
        let labels = agglomerative(&emb, 0.5, Some(3)).unwrap();
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
        assert!(agglomerative(&[], 0.5, None).unwrap().is_empty());
        assert_eq!(
            agglomerative(&[vec![1.0, 0.0]], 0.5, None).unwrap(),
            vec![0]
        );
    }
}
