//! Real loopback HTTP with a bounded/injectable thread-start boundary.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::mpsc,
    thread::JoinHandle,
    time::Duration,
};

struct Running {
    server: Arc<Server>,
    thread: Option<JoinHandle<Result<()>>>,
    address: String,
}
impl Running {
    fn start(
        root: &Path,
        spawn: impl FnMut(RequestHandler) -> std::io::Result<()> + Send + 'static,
    ) -> Self {
        let server = Arc::new(bind("127.0.0.1:0").unwrap());
        let address = server.server_addr().to_string();
        let serving = server.clone();
        let archive = Arc::new(Archive::new(root.to_path_buf()));
        let thread = std::thread::spawn(move || {
            serve_with(
                &serving,
                archive,
                HttpConfig {
                    bind: "127.0.0.1:0".into(),
                    token: None,
                },
                None,
                RequestBudget::new(MAX_INFLIGHT),
                spawn,
            )
        });
        Self {
            server,
            thread: Some(thread),
            address,
        }
    }
    fn stop(mut self) {
        self.server.unblock();
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
    fn request(&self) -> TcpStream {
        let mut socket = TcpStream::connect(&self.address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            socket,
            "GET /api/health HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            self.address
        )
        .unwrap();
        socket
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.server.unblock();
            let _ = thread.join();
        }
    }
}

#[test]
fn rejecting_thread_starts_does_not_eventually_lock_the_server_at_503() {
    let root = tempfile::tempdir().unwrap();
    let mut rejected = 0;
    let running = Running::start(root.path(), move |handler| {
        if rejected < MAX_INFLIGHT {
            rejected += 1;
            // Same owned-closure disposal as Builder::spawn failing. Do not
            // exhaust actual OS threads to produce a resource error.
            drop(handler);
            Err(std::io::Error::from(std::io::ErrorKind::WouldBlock))
        } else {
            spawn_request(handler)
        }
    });
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(5))
        .build();
    let url = format!("http://{}/api/health", running.address);
    for _ in 0..MAX_INFLIGHT {
        // tiny_http keeps its existing behavior: an unhandled request gets 500.
        assert!(matches!(
            agent.get(&url).call(),
            Err(ureq::Error::Status(500, _))
        ));
    }
    assert_eq!(agent.get(&url).call().unwrap().status(), 200);
    running.stop();
}

#[test]
fn full_http_budget_returns_503_and_recovers_when_a_handler_finishes() {
    let root = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let running = Running::start(root.path(), move |handler| {
        tx.send(handler)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    });
    let mut connections = Vec::new();
    let mut handlers = Vec::new();
    for _ in 0..MAX_INFLIGHT {
        connections.push(running.request());
        handlers.push(rx.recv_timeout(Duration::from_secs(5)).unwrap());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(5))
        .build();
    assert!(matches!(
        agent
            .get(&format!("http://{}/api/health", running.address))
            .call(),
        Err(ureq::Error::Status(503, _))
    ));
    handlers.pop().unwrap()();
    let mut recovered = running.request();
    rx.recv_timeout(Duration::from_secs(5)).unwrap()();
    let mut response = String::new();
    recovered.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("localvox-light"));
    drop(handlers);
    drop(connections);
    running.stop();
}
