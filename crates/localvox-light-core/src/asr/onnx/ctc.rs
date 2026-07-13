//! Greedy CTC decoding: argmax over timesteps → collapse-and-strip-blanks.

/// Decode the logits into a sequence of token ids.
///
/// * `logits` — a flat buffer `[n_frames * vocab_size]`, row-major by frames.
/// * `vocab_size` — the length of a logits row (including the blank).
/// * `blank_id` — the index of the blank token (usually `0` for the SP vocabs of GigaAM
///   v3 E2E, or `vocab_size - 1` for the char vocabs of NeMo/v3_ctc).
///
/// The algorithm: for every frame we take the argmax; then we collapse consecutive
/// repeats (`AAB` → `AB`) and strip out the blank id.
pub fn greedy_decode(logits: &[f32], vocab_size: usize, blank_id: usize) -> Vec<usize> {
    greedy_decode_timed(logits, vocab_size, blank_id)
        .into_iter()
        .map(|(id, _frame)| id)
        .collect()
}

/// The same, but every token remembers the FRAME NUMBER on which it won.
///
/// CTC knows when every token was uttered — that information was simply being thrown
/// away. Out of it come per-word timecodes, and out of those come turns split by phrase
/// instead of one line for the whole 30-second window: previously the «▶» in the player
/// and in the search jumped to the start of the window, that is, it could miss by half a
/// minute.
pub fn greedy_decode_timed(
    logits: &[f32],
    vocab_size: usize,
    blank_id: usize,
) -> Vec<(usize, usize)> {
    if vocab_size == 0 || logits.is_empty() {
        return Vec::new();
    }
    debug_assert_eq!(
        logits.len() % vocab_size,
        0,
        "logits.len() is a multiple of vocab_size"
    );
    let n_frames = logits.len() / vocab_size;

    let mut out = Vec::with_capacity(n_frames);
    let mut prev: Option<usize> = None;

    for t in 0..n_frames {
        let row = &logits[t * vocab_size..(t + 1) * vocab_size];
        let mut best = 0usize;
        let mut best_val = row[0];
        for (i, &v) in row.iter().enumerate().skip(1) {
            if v > best_val {
                best_val = v;
                best = i;
            }
        }
        if Some(best) == prev {
            continue;
        }
        prev = Some(best);
        if best != blank_id {
            out.push((best, t));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A utility: build logits where for every frame t the token `winners[t]` wins by
    /// construction.
    fn logits_from_winners(winners: &[usize], vocab_size: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; winners.len() * vocab_size];
        for (t, &w) in winners.iter().enumerate() {
            out[t * vocab_size + w] = 10.0;
        }
        out
    }

    #[test]
    fn empty_inputs() {
        assert!(greedy_decode(&[], 4, 0).is_empty());
        assert!(greedy_decode(&[1.0, 2.0, 3.0], 0, 0).is_empty());
    }

    #[test]
    fn collapses_repeats_and_strips_blanks() {
        let vocab = 4;
        let blank = 0;
        // [blank, A, A, blank, B, B, B, A]  →  A, B, A
        let winners = [0, 1, 1, 0, 2, 2, 2, 1];
        let logits = logits_from_winners(&winners, vocab);
        let ids = greedy_decode(&logits, vocab, blank);
        assert_eq!(ids, vec![1, 2, 1]);
    }

    #[test]
    fn blank_at_end_of_vocab_also_works() {
        let vocab = 4;
        let blank = 3;
        let winners = [3, 1, 1, 3, 2];
        let logits = logits_from_winners(&winners, vocab);
        let ids = greedy_decode(&logits, vocab, blank);
        assert_eq!(ids, vec![1, 2]);
    }
}
