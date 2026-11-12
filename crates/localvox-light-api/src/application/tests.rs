use super::*;

#[test]
fn owner_publishes_cleanup_before_reply_and_waits_for_response_guard() {
    for outcome in [Ok(()), Err("engine: final chunk could not be saved".into())] {
        let running = Arc::new(AtomicBool::new(true));
        let control = ApplicationControl::new(running.clone());
        let request = control.request().unwrap();
        assert!(!running.load(Ordering::SeqCst));
        assert!(control.request().is_err());
        let (done, received) = std::sync::mpsc::channel();
        let owner = control.clone();
        let expected = outcome.clone();
        let thread = std::thread::spawn(move || {
            owner.complete_and_drain(outcome, Duration::from_secs(2));
            done.send(()).unwrap();
        });
        assert_eq!(request.wait(Duration::from_secs(1)), expected);
        assert!(received.try_recv().is_err(), "owner exited before reply");
        drop(request);
        received.recv_timeout(Duration::from_secs(1)).unwrap();
        thread.join().unwrap();
        assert!(control.request().is_err());
    }
}

#[test]
fn missing_cleanup_is_unknown_and_drain_is_bounded_even_with_abandoned_response() {
    let control = ApplicationControl::new(Arc::new(AtomicBool::new(true)));
    let request = control.request().unwrap();
    assert!(request
        .wait(Duration::ZERO)
        .unwrap_err()
        .contains("неизвестно"));
    control.complete_and_drain(Ok(()), Duration::ZERO);
    assert!(request.wait(Duration::ZERO).is_ok());
}

#[test]
fn signal_shutdown_does_not_wait_for_an_http_request() {
    let control = ApplicationControl::new(Arc::new(AtomicBool::new(false)));
    assert!(control.request().is_err());
    control.complete_and_drain(Ok(()), Duration::from_secs(60));
}
