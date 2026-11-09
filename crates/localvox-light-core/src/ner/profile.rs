//! Explicit numeric instrumentation; input/entity/error strings never enter this module.
use anyhow::Result;
use serde::Serialize;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pass {
    Name,
    Role,
    Comparison,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    SplitWords,
    WindowInput,
    Tokenization,
    TensorPreparation,
    SessionLock,
    TensorCreation,
    Inference,
    LogitsCopy,
    SpanDecode,
    WindowDedup,
    RoleComparison,
}

#[derive(Debug, Serialize)]
pub struct ComponentTiming {
    pub component: Component,
    pub elapsed_sec: f64,
    pub calls: u64,
    pub errors: u64,
}

#[derive(Debug, Serialize)]
pub struct PassTimings {
    pub pass: Pass,
    pub words: usize,
    pub windows_started: u64,
    pub windows_completed: u64,
    /// Sum/max of tokenized window lengths, including prompt and overlap.
    pub tokens_total: usize,
    pub tokens_max: usize,
    /// Exclusive, aggregated intervals. These are nested in the outer NER backend time.
    pub components: Vec<ComponentTiming>,
}

#[derive(Debug, Serialize)]
pub struct Timings {
    pub total_sec: f64,
    pub unattributed_sec: f64,
    pub complete: bool,
    pub passes: Vec<PassTimings>,
}

pub(super) struct Observer {
    started: Option<Instant>,
    passes: Vec<PassTimings>,
}

impl Observer {
    pub(super) fn disabled() -> Self {
        Self {
            started: None,
            passes: Vec::new(),
        }
    }

    pub(super) fn enabled() -> Self {
        Self {
            started: Some(Instant::now()),
            passes: Vec::new(),
        }
    }

    fn pass(&mut self, pass: Pass) -> &mut PassTimings {
        let index = self
            .passes
            .iter()
            .position(|p| p.pass == pass)
            .unwrap_or_else(|| {
                self.passes.push(PassTimings {
                    pass,
                    words: 0,
                    windows_started: 0,
                    windows_completed: 0,
                    tokens_total: 0,
                    tokens_max: 0,
                    components: Vec::new(),
                });
                self.passes.len() - 1
            });
        &mut self.passes[index]
    }

    pub(super) fn measure<T>(
        &mut self,
        pass: Pass,
        component: Component,
        call: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        if self.started.is_none() {
            return call();
        }
        let started = Instant::now();
        let result = call();
        let elapsed = started.elapsed().as_secs_f64();
        let timings = self.pass(pass);
        let index = timings
            .components
            .iter()
            .position(|c| c.component == component)
            .unwrap_or_else(|| {
                timings.components.push(ComponentTiming {
                    component,
                    elapsed_sec: 0.0,
                    calls: 0,
                    errors: 0,
                });
                timings.components.len() - 1
            });
        let timing = &mut timings.components[index];
        timing.elapsed_sec += elapsed;
        timing.calls += 1;
        timing.errors += u64::from(result.is_err());
        result
    }

    pub(super) fn words(&mut self, pass: Pass, words: usize) {
        if self.started.is_some() {
            self.pass(pass).words = words;
        }
    }

    pub(super) fn window_started(&mut self, pass: Pass) {
        if self.started.is_some() {
            self.pass(pass).windows_started += 1;
        }
    }

    pub(super) fn window_completed(&mut self, pass: Pass, success: bool) {
        if self.started.is_some() && success {
            self.pass(pass).windows_completed += 1;
        }
    }

    pub(super) fn tokens(&mut self, pass: Pass, count: usize) {
        if self.started.is_some() {
            let timings = self.pass(pass);
            timings.tokens_total += count;
            timings.tokens_max = timings.tokens_max.max(count);
        }
    }

    pub(super) fn finish(self, complete: bool) -> Timings {
        let total_sec = self
            .started
            .expect("only enabled observers are published")
            .elapsed()
            .as_secs_f64();
        let measured: f64 = self
            .passes
            .iter()
            .flat_map(|p| &p.components)
            .map(|c| c.elapsed_sec)
            .sum();
        Timings {
            total_sec,
            unattributed_sec: (total_sec - measured).max(0.0),
            complete,
            passes: self.passes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_observer_preserves_values_errors_and_order_without_collecting() {
        let mut observer = Observer::disabled();
        let mut order = Vec::new();
        let value = observer
            .measure(Pass::Name, Component::Tokenization, || {
                order.push(1);
                Ok("PRIVATE_VALUE")
            })
            .unwrap();
        let error = observer
            .measure::<()>(Pass::Role, Component::Inference, || {
                order.push(2);
                anyhow::bail!("PRIVATE_ERROR")
            })
            .unwrap_err();
        observer.words(Pass::Name, 300);
        observer.window_started(Pass::Name);
        observer.window_completed(Pass::Name, true);
        observer.tokens(Pass::Name, 200);
        assert_eq!(value, "PRIVATE_VALUE");
        assert_eq!(error.to_string(), "PRIVATE_ERROR");
        assert_eq!(order, vec![1, 2]);
        assert!(observer.started.is_none());
        assert!(observer.passes.is_empty());
    }

    #[test]
    fn partial_failure_is_numeric_counts_attempts_and_never_serializes_values() {
        let mut observer = Observer::enabled();
        observer.words(Pass::Name, 200);
        observer.window_started(Pass::Name);
        observer.tokens(Pass::Name, 180);
        assert_eq!(
            observer
                .measure(Pass::Name, Component::Inference, || Ok("PRIVATE_ENTITY"))
                .unwrap(),
            "PRIVATE_ENTITY"
        );
        observer.window_completed(Pass::Name, true);
        observer.window_started(Pass::Name);
        observer.tokens(Pass::Name, 210);
        assert!(observer
            .measure::<()>(Pass::Name, Component::Inference, || anyhow::bail!(
                "PRIVATE_ERROR"
            ))
            .is_err());
        observer.window_completed(Pass::Name, false);
        let report = observer.finish(false);
        assert!(!report.complete);
        assert_eq!(report.passes.len(), 1);
        let p = &report.passes[0];
        assert_eq!(
            (p.words, p.windows_started, p.windows_completed),
            (200, 2, 1)
        );
        assert_eq!((p.tokens_total, p.tokens_max), (390, 210));
        assert_eq!((p.components[0].calls, p.components[0].errors), (2, 1));
        assert!(p.components[0].elapsed_sec.is_finite());
        assert!(p.components[0].elapsed_sec >= 0.0);
        assert!(p.components[0].elapsed_sec <= report.total_sec);
        assert!(!serde_json::to_string(&report).unwrap().contains("PRIVATE_"));
    }

    #[test]
    fn pass_components_are_disjoint_and_no_role_or_window_is_invented() {
        let mut observer = Observer::enabled();
        observer
            .measure(Pass::Name, Component::SplitWords, || Ok(()))
            .unwrap();
        let report = observer.finish(true);
        assert!(report.complete);
        assert_eq!(report.passes.len(), 1);
        assert_eq!(report.passes[0].pass, Pass::Name);
        assert_eq!(report.passes[0].windows_started, 0);
        let sum: f64 = report
            .passes
            .iter()
            .flat_map(|p| &p.components)
            .map(|c| c.elapsed_sec)
            .sum();
        assert!((sum + report.unattributed_sec - report.total_sec).abs() < 1e-9);
    }

    #[test]
    #[ignore = "explicit LOCALVOX_NER_PROFILE_TEST_MODEL_DIR required; real NER, synthetic input only"]
    fn real_model_profile_preserves_entities_scores_and_byte_spans() {
        let dir = std::path::PathBuf::from(
            std::env::var_os("LOCALVOX_NER_PROFILE_TEST_MODEL_DIR")
                .expect("explicit NER model directory required"),
        );
        let ner = crate::ner::Ner::open(&dir).unwrap();
        let short = "Роман закроет задачу. Marcus will ship it. Говорящий рассказал о проекте.";
        let long = format!(
            "Роман открыл встречу. {} Marcus подведёт итог.",
            "Мы обсуждаем план проекта и результаты работы команды. ".repeat(36)
        );
        let mut found = 0;
        let mut cases = 0;
        for threshold in [0.3, 0.5] {
            for (text, role) in [
                (short, "говорящий или роль"),
                (long.as_str(), "говорящий или роль"),
                (short, ""),
                ("", "говорящий или роль"),
            ] {
                let ordinary = ner.names(text, "имя человека", role, threshold).unwrap();
                let (profiled, timings) = ner.names_profiled(text, "имя человека", role, threshold);
                let profiled = profiled.unwrap();
                assert_eq!(ordinary, profiled);
                assert!(timings.complete);
                let sum: f64 = timings
                    .passes
                    .iter()
                    .flat_map(|p| &p.components)
                    .map(|c| c.elapsed_sec)
                    .sum();
                assert!(sum <= timings.total_sec);
                assert!((sum + timings.unattributed_sec - timings.total_sec).abs() < 1e-8);
                for pass in timings.passes.iter().filter(|p| p.pass != Pass::Comparison) {
                    assert_eq!(pass.windows_started, pass.windows_completed);
                    let inference_calls: u64 = pass
                        .components
                        .iter()
                        .filter(|c| c.component == Component::Inference)
                        .map(|c| c.calls)
                        .sum();
                    assert_eq!(inference_calls, pass.windows_completed);
                    assert!(pass
                        .components
                        .iter()
                        .all(|c| c.errors == 0 && c.elapsed_sec.is_finite()));
                    if text == long {
                        assert!(pass.windows_completed > 1);
                    }
                    if text.is_empty() {
                        assert_eq!(pass.windows_completed, 0);
                    }
                }
                if role.is_empty() {
                    assert_eq!(timings.passes.len(), 1);
                }
                for entity in &profiled {
                    assert_eq!(entity.text, text[entity.start..entity.end]);
                }
                found += profiled.len();
                cases += 1;
            }
        }
        assert!(
            found > 0,
            "nonempty positive control required for meaningful entity parity"
        );
        eprintln!("native entity parity: {cases} cases, {found} returned entities, exact scores and byte spans");
    }
}
