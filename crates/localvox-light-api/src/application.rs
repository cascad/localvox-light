//! A shutdown request asks the existing runtime owner to stop, then waits for its
//! cleanup outcome. The owner also lets the HTTP handler finish its reply.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex,
};
use std::time::Duration;

#[derive(Default)]
struct State {
    requested: bool,
    response_pending: bool,
    outcome: Option<Result<(), String>>,
}

pub struct ApplicationControl {
    running: Arc<AtomicBool>,
    state: Mutex<State>,
    changed: Condvar,
}

impl ApplicationControl {
    pub fn new(running: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            running,
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        })
    }

    pub fn is_stopping(&self) -> bool {
        !self.running.load(Ordering::SeqCst)
    }

    pub fn request(self: &Arc<Self>) -> Result<ShutdownRequest, String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.requested || self.is_stopping() || state.outcome.is_some() {
            return Err("LocalVox уже завершается".into());
        }
        state.requested = true;
        state.response_pending = true;
        self.running.store(false, Ordering::SeqCst);
        Ok(ShutdownRequest(self.clone()))
    }

    /// Called once, after the runtime's normal bounded cleanup, including its
    /// error collection. Waiting is only needed when an HTTP reply is pending.
    pub fn complete_and_drain(&self, outcome: Result<(), String>, timeout: Duration) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.outcome = Some(outcome);
        self.changed.notify_all();
        let _ = self
            .changed
            .wait_timeout_while(state, timeout, |s| s.response_pending)
            .unwrap_or_else(|e| e.into_inner());
    }
}

pub struct ShutdownRequest(Arc<ApplicationControl>);

impl ShutdownRequest {
    pub fn wait(&self, timeout: Duration) -> Result<(), String> {
        let state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        let (state, _) = self
            .0
            .changed
            .wait_timeout_while(state, timeout, |s| s.outcome.is_none())
            .unwrap_or_else(|e| e.into_inner());
        state.outcome.clone().unwrap_or_else(|| {
            Err("Результат завершения не получен вовремя; состояние неизвестно".into())
        })
    }
}

impl Drop for ShutdownRequest {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        state.response_pending = false;
        self.0.changed.notify_all();
    }
}

#[cfg(test)]
mod tests;
