//! Жадное CTC-декодирование: argmax по timesteps → collapse-and-strip-blanks.

/// Декодировать логиты в последовательность id-токенов.
///
/// * `logits` — плоский буфер `[n_frames * vocab_size]`, row-major по фреймам.
/// * `vocab_size` — длина строки logits (включая blank).
/// * `blank_id` — индекс blank-токена (обычно `0` для SP-вокабов GigaAM v3 E2E,
///   либо `vocab_size - 1` для char-вокабов NeMo/v3_ctc).
///
/// Алгоритм: для каждого фрейма берём argmax; затем сжимаем последовательные
/// повторы (`AAB` → `AB`) и убираем blank-id.
pub fn greedy_decode(logits: &[f32], vocab_size: usize, blank_id: usize) -> Vec<usize> {
    if vocab_size == 0 || logits.is_empty() {
        return Vec::new();
    }
    debug_assert_eq!(logits.len() % vocab_size, 0, "logits.len() кратно vocab_size");
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
            out.push(best);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Утилита: построить логиты, где для каждого фрейма t заранее выигрывает токен `winners[t]`.
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
