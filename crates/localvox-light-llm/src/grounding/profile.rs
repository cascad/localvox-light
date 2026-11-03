//! Opt-in wall time only. Neither input strings nor entity values enter this observer.
use serde::Serialize;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    SourcePreparation,
    AnswerNumbers,
    NerTextPreparation,
    SourceEntityRequest,
    SourceEntityNormalization,
    AnswerEntityRequest,
    AnswerEntityNormalization,
    EntityMatching,
    LexicalMatching,
    ResultAssembly,
}

#[derive(Debug, Serialize)]
pub struct ComponentTiming {
    pub component: Component,
    pub elapsed_sec: f64,
}

#[derive(Debug, Serialize)]
pub struct CheckTimings {
    pub total_sec: f64,
    /// Sequential exclusive intervals; NER backend timings may be nested inside requests.
    pub components: Vec<ComponentTiming>,
}

pub(super) struct Observer {
    started: Option<Instant>,
    last: Option<Instant>,
    components: Vec<ComponentTiming>,
}

impl Observer {
    pub(super) fn disabled() -> Self {
        Self {
            started: None,
            last: None,
            components: Vec::new(),
        }
    }

    pub(super) fn enabled() -> Self {
        let now = Instant::now();
        Self {
            started: Some(now),
            last: Some(now),
            components: Vec::new(),
        }
    }

    pub(super) fn lap(&mut self, component: Component) {
        if let Some(last) = self.last {
            self.components.push(ComponentTiming {
                component,
                elapsed_sec: last.elapsed().as_secs_f64(),
            });
            self.last = Some(Instant::now());
        }
    }

    pub(super) fn finish(self) -> CheckTimings {
        CheckTimings {
            total_sec: self
                .started
                .expect("only enabled observers are published")
                .elapsed()
                .as_secs_f64(),
            components: self.components,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grounding::{self, Entities, SourceEntities};
    use localvox_light_core::lexicon::Lexicon;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn validate(timings: &CheckTimings) {
        let sum: f64 = timings.components.iter().map(|c| c.elapsed_sec).sum();
        assert!(sum <= timings.total_sec + 1e-6);
        assert!(timings
            .components
            .iter()
            .all(|c| c.elapsed_sec.is_finite() && c.elapsed_sec >= 0.0));
        let json = serde_json::to_string(timings).unwrap();
        assert!(!json.contains("PRIVATE_"));
    }

    #[test]
    fn disabled_ner_preserves_numbers_and_exact_verdict_without_fake_ner_timings() {
        let lex = Lexicon::builtin();
        for answer in [
            "",
            "В записи 150 задач.",
            "В записи 151 задача.",
            "PRIVATE_ANSWER",
        ] {
            let source = "PRIVATE_SOURCE: в записи 150 задач.";
            let words = format!("Шаблон просит 3 пункта.\n{source}");
            let expected = grounding::check_parts(&lex, &words, source, answer);
            let (actual, timings) = grounding::check_profiled(&lex, &words, source, answer, None);
            assert_eq!(actual, expected);
            if answer.contains("151") {
                assert_eq!(actual.numbers, vec!["151"]);
            }
            assert_eq!(timings.components.len(), 4);
            assert!(!timings
                .components
                .iter()
                .any(|c| c.component == Component::SourceEntityRequest));
            validate(&timings);
        }
    }

    #[test]
    fn ner_requests_reuse_only_the_document_source_and_never_skip_answer_checks() {
        struct Counter {
            source: AtomicUsize,
            answer: AtomicUsize,
        }
        impl Entities for Counter {
            fn people(&self, _: &str) -> Vec<String> {
                self.answer.fetch_add(1, Ordering::Relaxed);
                vec!["PRIVATE_ENTITY".into()]
            }
            fn people_in_source(&self, _: &str) -> Vec<String> {
                self.source.fetch_add(1, Ordering::Relaxed);
                vec!["PRIVATE_ENTITY".into()]
            }
        }
        let inner = Counter {
            source: AtomicUsize::new(0),
            answer: AtomicUsize::new(0),
        };
        let cached = SourceEntities::new(&inner);
        let lex = Lexicon::builtin();
        let source = "PRIVATE_SOURCE: 150 задач.";
        let answer = "PRIVATE_ENTITY проверит 151 задачу.";
        let (first, timings) =
            grounding::check_profiled(&lex, source, source, answer, Some(&cached));
        assert_eq!(timings.components.len(), 10);
        validate(&timings);
        let (second, timings) =
            grounding::check_profiled(&lex, source, source, answer, Some(&cached));
        assert_eq!(second, first);
        validate(&timings);
        assert_eq!(
            grounding::check_with_entities(&lex, source, source, answer, &cached),
            first
        );
        assert_eq!(inner.source.load(Ordering::Relaxed), 1);
        assert_eq!(inner.answer.load(Ordering::Relaxed), 3);
        grounding::check_profiled(
            &lex,
            "другой источник",
            "другой источник",
            answer,
            Some(&cached),
        );
        assert_eq!(inner.source.load(Ordering::Relaxed), 2);
        let next_document = SourceEntities::new(&inner);
        grounding::check_profiled(&lex, source, source, answer, Some(&next_document));
        assert_eq!(inner.source.load(Ordering::Relaxed), 3);
    }
}
