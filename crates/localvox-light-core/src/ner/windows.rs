//! Whole-window selection by byte geometry. Inference input/context is never shortened.
use super::{
    profile::{Component, Observer, Pass},
    Entity, Word, WINDOW_OVERLAP, WINDOW_WORDS,
};
use anyhow::Result;
use std::ops::Range;

/// Union of name spans; no text, label or classifier score is needed for selection.
pub(super) struct SpanIndex(Vec<Range<usize>>);

impl SpanIndex {
    pub(super) fn new(names: &[Entity]) -> Self {
        let mut spans: Vec<_> = names.iter().map(|n| n.start..n.end).collect();
        spans.sort_unstable_by_key(|r| (r.start, r.end));
        let mut merged: Vec<Range<usize>> = Vec::new();
        for span in spans {
            if let Some(last) = merged.last_mut() {
                if span.start <= last.end {
                    last.end = last.end.max(span.end);
                    continue;
                }
            }
            merged.push(span);
        }
        Self(merged)
    }

    pub(super) fn intersects(&self, window: Range<usize>) -> bool {
        let next = self.0.partition_point(|span| span.end <= window.start);
        self.0.get(next).is_some_and(|span| span.start < window.end)
    }
}

fn ranges(count: usize) -> impl Iterator<Item = Range<usize>> {
    let mut start = 0;
    std::iter::from_fn(move || {
        if start >= count {
            return None;
        }
        let end = (start + WINDOW_WORDS).min(count);
        let range = start..end;
        start = if end == count {
            count
        } else {
            end - WINDOW_OVERLAP
        };
        Some(range)
    })
}

pub(super) fn collect(
    words: &[Word],
    relevant: Option<&SpanIndex>,
    pass: Pass,
    observer: &mut Observer,
    mut execute: impl FnMut(&[Word], &mut Observer) -> Result<Vec<Entity>>,
) -> Result<Vec<Entity>> {
    let mut out = Vec::new();
    for range in ranges(words.len()) {
        let window = &words[range];
        if let Some(index) = relevant {
            let needed = observer.measure(pass, Component::WindowSelection, || {
                Ok(index.intersects(window[0].start..window.last().unwrap().end))
            })?;
            if !needed {
                observer.window_skipped(pass);
                continue;
            }
        }
        observer.window_started(pass);
        let result = execute(window, observer);
        observer.window_completed(pass, result.is_ok());
        out.extend(result?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::{dedup_entities, names_beating_roles, split_words};
    use super::*;

    fn entity(text: &str, span: Range<usize>, score: f32) -> Entity {
        Entity {
            text: text[span.clone()].into(),
            label: "fixture".into(),
            score,
            start: span.start,
            end: span.end,
        }
    }

    #[test]
    fn original_windows_cover_boundaries_without_an_extra_tail() {
        for (count, expected) in [
            (0, vec![]),
            (1, vec![0..1]),
            (120, vec![0..120]),
            (121, vec![0..120, 100..121]),
            (220, vec![0..120, 100..220]),
            (221, vec![0..120, 100..220, 200..221]),
        ] {
            assert_eq!(ranges(count).collect::<Vec<_>>(), expected);
        }
    }

    #[test]
    fn union_handles_unsorted_nested_adjacent_spans_and_strict_unicode_boundaries() {
        let text = "Жёлтый чай, ёж и кофе";
        let names = vec![
            entity(text, 17..23, 0.8),
            entity(text, 0..12, 0.7),
            entity(text, 2..6, 0.9),
            entity(text, 12..17, 0.6),
        ];
        let index = SpanIndex::new(&names);
        assert_eq!(index.0, vec![0..23]);
        assert!(index.intersects(22..24));
        assert!(!index.intersects(23..text.len()));
        assert!(!SpanIndex::new(&[]).intersects(0..text.len()));
        // Check the index against the definition on many unsorted/overlapping spans.
        for seed in 0..60 {
            let mut names = Vec::new();
            for i in (0..25).rev() {
                let start = (seed * 13 + i * 17) % 100;
                names.push(Entity {
                    text: String::new(),
                    label: String::new(),
                    score: 0.5,
                    start,
                    end: start + 1 + (i % 12),
                });
            }
            let index = SpanIndex::new(&names);
            for start in 0..115 {
                for width in [1, 2, 12, 25] {
                    assert_eq!(
                        index.intersects(start..start + width),
                        names
                            .iter()
                            .any(|n| n.start < start + width && start < n.end)
                    );
                }
            }
        }
    }

    #[test]
    fn whole_window_selection_preserves_role_decisions_including_overlap_duplicates() {
        let text = "ёж слово онлайн-встреча, café! ".repeat(75);
        let words = split_words(&text);
        for seed in 0..150 {
            let names: Vec<_> = (0..(seed % 9))
                .map(|i| {
                    let start = (seed * 31 + i * 101) % (words.len() - 3);
                    entity(
                        &text,
                        words[start].start..words[start + 2].end,
                        0.2 + (i % 7) as f32 / 10.0,
                    )
                })
                .collect();
            let run = |selection| {
                collect(
                    &words,
                    selection,
                    Pass::Role,
                    &mut Observer::disabled(),
                    |window, _| {
                        Ok((0..window.len().saturating_sub(2))
                            .step_by(3)
                            .map(|i| {
                                let score = ((window[0].start + i * 7) % 11) as f32 / 10.0;
                                entity(&text, window[i].start..window[i + 2].end, score)
                            })
                            .collect())
                    },
                )
                .unwrap()
            };
            let full = dedup_entities(run(None));
            let index = SpanIndex::new(&names);
            let selected = dedup_entities(run(Some(&index)));
            assert_eq!(
                names_beating_roles(names.clone(), &full),
                names_beating_roles(names, &selected)
            );
        }
    }

    #[test]
    fn no_names_execute_no_roles_and_dense_names_execute_identical_complete_windows() {
        let text = "слово ".repeat(341);
        let words = split_words(&text);
        let empty = SpanIndex::new(&[]);
        let mut observer = Observer::enabled();
        assert!(
            collect(&words, Some(&empty), Pass::Role, &mut observer, |_, _| {
                panic!("irrelevant inference must not execute")
            })
            .unwrap()
            .is_empty()
        );
        let report = observer.finish(true);
        assert_eq!(report.passes[0].windows_skipped, 4);
        assert_eq!(report.passes[0].windows_started, 0);
        let names = vec![entity(&text, 0..text.len(), 0.8)];
        let index = SpanIndex::new(&names);
        let visit = |selection| {
            let mut seen = Vec::new();
            collect(
                &words,
                selection,
                Pass::Role,
                &mut Observer::disabled(),
                |window, _| {
                    seen.push(
                        window
                            .iter()
                            .map(|w| (w.text.clone(), w.start, w.end))
                            .collect::<Vec<_>>(),
                    );
                    Ok(Vec::new())
                },
            )
            .unwrap();
            seen
        };
        assert_eq!(visit(None), visit(Some(&index)));
    }

    #[test]
    fn required_window_error_is_returned_with_partial_counts() {
        let text = "слово ".repeat(341);
        let words = split_words(&text);
        let names = vec![entity(&text, words[115].start..words[115].end, 0.8)];
        let index = SpanIndex::new(&names);
        let mut observer = Observer::enabled();
        let mut calls = 0;
        let result = collect(
            &words,
            Some(&index),
            Pass::Role,
            &mut observer,
            |_, observer| {
                calls += 1;
                observer.measure(Pass::Role, Component::Inference, || {
                    if calls == 2 {
                        anyhow::bail!("fixture failure");
                    }
                    Ok(Vec::new())
                })
            },
        );
        assert_eq!(result.unwrap_err().to_string(), "fixture failure");
        assert_eq!(calls, 2);
        let report = observer.finish(false);
        assert!(!report.complete);
        assert_eq!(report.passes[0].windows_started, 2);
        assert_eq!(report.passes[0].windows_completed, 1);
        assert_eq!(
            report.passes[0]
                .components
                .iter()
                .find(|c| c.component == Component::Inference)
                .unwrap()
                .errors,
            1
        );
    }
}
