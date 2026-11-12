//! A request owns its slot until completion or drop, including failed thread starts.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[derive(Clone)]
pub(super) struct RequestBudget(Arc<State>);
struct State {
    active: AtomicUsize,
    limit: usize,
}
pub(super) struct Permit(Arc<State>);

impl RequestBudget {
    pub(super) fn new(limit: usize) -> Self {
        Self(Arc::new(State {
            active: AtomicUsize::new(0),
            limit,
        }))
    }

    pub(super) fn try_acquire(&self) -> Option<Permit> {
        self.0
            .active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.0.limit).then(|| active + 1)
            })
            .ok()?;
        Some(Permit(Arc::clone(&self.0)))
    }
}

impl Permit {
    pub(super) fn run(self, handler: impl FnOnce()) {
        let _permit = self;
        handler();
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{mpsc, Condvar, Mutex},
        time::Duration,
    };

    #[test]
    fn full_budget_returns_slots_on_completion_and_dropped_task() {
        let budget = RequestBudget::new(1);
        let permit = budget.try_acquire().unwrap();
        assert!(budget.try_acquire().is_none());
        permit.run(|| ());
        let permit = budget.try_acquire().unwrap();
        let task = move || permit.run(|| ());
        assert!(budget.try_acquire().is_none());
        drop(task); // A spawner rejected the owned closure without running it.
        assert!(budget.try_acquire().is_some());
    }

    #[test]
    fn actual_thread_unwind_returns_its_slot() {
        let budget = RequestBudget::new(1);
        let permit = budget.try_acquire().unwrap();
        assert!(
            std::thread::spawn(move || permit.run(|| panic!("synthetic handler panic")))
                .join()
                .is_err()
        );
        assert!(budget.try_acquire().is_some());
    }

    #[test]
    fn concurrent_acquisition_never_exceeds_the_limit() {
        let budget = RequestBudget::new(4);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (tx, rx) = mpsc::channel();
        let mut threads = Vec::new();
        for _ in 0..16 {
            let (budget, gate, tx) = (budget.clone(), gate.clone(), tx.clone());
            threads.push(std::thread::spawn(move || {
                let permit = budget.try_acquire();
                tx.send(permit.is_some()).unwrap();
                if permit.is_some() {
                    let (lock, wake) = &*gate;
                    drop(
                        wake.wait_while(lock.lock().unwrap(), |open| !*open)
                            .unwrap(),
                    );
                }
                drop(permit);
            }));
        }
        let outcomes: Vec<_> = (0..16)
            .map(|_| rx.recv_timeout(Duration::from_secs(5)))
            .collect();
        let was_full = budget.try_acquire().is_none();
        let (lock, wake) = &*gate;
        *lock.lock().unwrap() = true;
        wake.notify_all();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(
            outcomes
                .into_iter()
                .map(Result::unwrap)
                .filter(|ok| *ok)
                .count(),
            4
        );
        assert!(was_full);
        let slots: Vec<_> = (0..4).map(|_| budget.try_acquire().unwrap()).collect();
        assert!(budget.try_acquire().is_none());
        drop(slots);
    }
}
