//! Owned mpv + bounded JSON IPC. Media credentials only travel over the private IPC.
use anyhow::{Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use localvox_light_client::Client;
use localvox_light_process::{Limits, ManagedProcess};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaybackState {
    pub session: Option<String>,
    pub position: f64,
    pub duration: Option<f64>,
    pub paused: bool,
    pub idle: bool,
    pub error: Option<String>,
    pub fragment_end: Option<f64>,
}
#[derive(Clone, Debug)]
pub enum PlaybackCommand {
    Load {
        session: String,
        start: f64,
        end: Option<f64>,
    },
    Toggle,
    Seek {
        session: String,
        position: f64,
    },
    Stop,
    Shutdown,
}
pub struct Player {
    tx: Sender<Queued>,
    pub rx: Receiver<PlaybackState>,
    stop: Arc<AtomicBool>,
    revision: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
}
struct Queued {
    revision: u64,
    command: PlaybackCommand,
}
impl Player {
    pub fn new(client: Client) -> Result<Self> {
        Self::with_backend(client, executable(), false)
    }
    pub fn with_backend(client: Client, executable: PathBuf, null_audio: bool) -> Result<Self> {
        let (tx, commands) = bounded(8);
        let (states, rx) = bounded(2);
        let obsolete = rx.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let revision = Arc::new(AtomicU64::new(0));
        let current = revision.clone();
        let thread = std::thread::Builder::new()
            .name("terminal-player".into())
            .spawn(move || {
                let mut backend = Backend {
                    mpv: None,
                    state: PlaybackState {
                        paused: true,
                        idle: true,
                        ..Default::default()
                    },
                    client,
                    executable,
                    null_audio,
                };
                let mut sent = None;
                while !stopping.load(Ordering::Relaxed) {
                    let command = match commands.recv_timeout(Duration::from_millis(200)) {
                        Ok(Queued { revision, command })
                            if revision == current.load(Ordering::Relaxed) =>
                        {
                            Some((revision, command))
                        }
                        Ok(_) => continue,
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => None,
                        Err(_) => break,
                    };
                    if stopping.load(Ordering::Relaxed)
                        || matches!(command, Some((_, PlaybackCommand::Shutdown)))
                    {
                        break;
                    }
                    if let Some((revision, command)) = command {
                        if let Err(error) = backend.apply(command, &|| {
                            stopping.load(Ordering::Relaxed)
                                || revision != current.load(Ordering::Relaxed)
                        }) {
                            backend.fail(&crate::diagnostics::message(&format!("{error:#}")));
                        }
                        if revision != current.load(Ordering::Relaxed) {
                            continue;
                        }
                    }
                    backend.poll();
                    if sent.as_ref() != Some(&backend.state) {
                        if states.is_full() {
                            let _ = obsolete.try_recv();
                        }
                        let _ = states.try_send(backend.state.clone());
                        sent = Some(backend.state.clone());
                    }
                }
            })
            .context("Не удалось создать поток плеера")?;
        Ok(Self {
            tx,
            rx,
            stop,
            revision,
            thread: Some(thread),
        })
    }
    pub fn submit(&self, command: PlaybackCommand) -> bool {
        let revision = if matches!(
            command,
            PlaybackCommand::Load { .. } | PlaybackCommand::Stop
        ) {
            self.revision.fetch_add(1, Ordering::Relaxed) + 1
        } else {
            self.revision.load(Ordering::Relaxed)
        };
        self.tx.try_send(Queued { revision, command }).is_ok()
    }
}
struct Backend {
    mpv: Option<Mpv>,
    state: PlaybackState,
    client: Client,
    executable: PathBuf,
    null_audio: bool,
}
impl Backend {
    fn apply(&mut self, command: PlaybackCommand, cancelled: &impl Fn() -> bool) -> Result<()> {
        match &command {
            PlaybackCommand::Stop => {
                self.state = PlaybackState {
                    paused: true,
                    idle: true,
                    ..Default::default()
                };
                if let Some(player) = &mut self.mpv {
                    player.stop()?;
                }
                return Ok(());
            }
            PlaybackCommand::Load {
                session,
                start,
                end,
            } => {
                // Even a failed startup belongs to the newly selected session.
                self.state = PlaybackState {
                    session: Some(session.clone()),
                    position: start.max(0.0),
                    fragment_end: *end,
                    paused: true,
                    idle: true,
                    ..Default::default()
                };
            }
            _ => {}
        }
        if self.mpv.is_none() {
            let backend = Mpv::start_cancellable(&self.executable, self.null_audio, cancelled)
                .map_err(|error| {
                let spawning = error.chain().any(|cause| cause.to_string().starts_with("spawning "));
                let missing = error.chain().any(|cause| {
                    cause.downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                });
                if spawning && missing {
                    anyhow::anyhow!("Файл mpv или его зависимости не найдены. Установите: npm --prefix ui run setup:mpv; либо укажите LOCALVOX_MPV")
                } else if error.to_string().contains("0.38") {
                    anyhow::anyhow!("Нужен mpv версии 0.38 или новее; обновите выбранный mpv")
                } else {
                    error.context("Не удалось запустить mpv или открыть его IPC")
                }
            })?;
            self.mpv = Some(backend);
        }
        anyhow::ensure!(!cancelled(), "obsolete playback command");
        let player = self.mpv.as_mut().unwrap();
        match command {
            PlaybackCommand::Load {
                session,
                start,
                end,
            } => {
                let media = self.client.media(&session);
                player
                    .load(&media.url, &media.headers, start, end)
                    .context("Не удалось открыть аудио")?;
            }
            PlaybackCommand::Toggle => {
                player
                    .command(json!(["set_property", "file-local-options/end", "none"]))
                    .context("Не удалось снять границу фрагмента")?;
                player.command(json!(["cycle", "pause"]))?;
                self.state.fragment_end = None;
            }
            PlaybackCommand::Seek { session, position } => {
                if self.state.session.as_ref() == Some(&session) && !self.state.idle {
                    player
                        .seek(position)
                        .context("Не удалось перемотать аудио")?;
                } else {
                    let media = self.client.media(&session);
                    player
                        .load(&media.url, &media.headers, position, None)
                        .context("Не удалось открыть аудио для перехода")?;
                    player.command(json!(["set_property", "pause", true]))?;
                    self.state.session = Some(session);
                }
                self.state.fragment_end = None;
            }
            _ => {}
        }
        Ok(())
    }
    fn fail(&mut self, message: &str) {
        self.state.error = Some(message.into());
        self.state.paused = true;
        self.state.idle = true;
        self.mpv = None;
    }
    fn poll(&mut self) {
        if let Some(player) = &mut self.mpv {
            match player.state() {
                Ok(mut next) => {
                    // mpv unloads a bounded fragment at EOF. Keep its endpoint
                    // visible so ordinary Play can continue from there.
                    if next.idle && next.error.is_none() {
                        next.duration = self.state.duration;
                        next.position = if !self.state.idle {
                            self.state
                                .fragment_end
                                .or(self.state.duration)
                                .unwrap_or(self.state.position)
                        } else {
                            self.state.position
                        };
                        next.paused = true;
                    }
                    next.fragment_end = self.state.fragment_end;
                    let session = self.state.session.take();
                    self.state = next;
                    self.state.session = session;
                }
                Err(_) => {
                    self.fail("Соединение с плеером потеряно; выберите воспроизведение снова")
                }
            }
        }
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.tx.try_send(Queued {
            revision: self.revision.load(Ordering::Relaxed),
            command: PlaybackCommand::Shutdown,
        });
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn executable() -> PathBuf {
    if let Some(path) = std::env::var_os("LOCALVOX_MPV") {
        return path.into();
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_owned))
    {
        let name = if cfg!(windows) { "mpv.exe" } else { "mpv" };
        for file in [dir.join(name), dir.join("tools/mpv").join(name)] {
            if file.is_file() {
                return file;
            }
        }
    }
    if let Some(root) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().and_then(Path::parent).map(Path::to_owned))
    {
        let file = root
            .join("tools/mpv")
            .join(if cfg!(windows) { "mpv.exe" } else { "mpv" });
        if file.is_file() {
            return file;
        }
    }
    PathBuf::from("mpv")
}
fn process_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command.env_remove("LOCALVOX_API_TOKEN");
    command
}

trait Transport: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Transport for T {}
struct Connection {
    stream: BufReader<Box<dyn Transport>>,
    seq: u64,
    media_error: bool,
}
impl Connection {
    async fn call(&mut self, command: Value) -> Result<Value> {
        self.seq += 1;
        let id = self.seq;
        let mut bytes = serde_json::to_vec(&json!({"command":command,"request_id":id}))?;
        bytes.push(b'\n');
        self.stream.get_mut().write_all(&bytes).await?;
        self.stream.get_mut().flush().await?;
        for _ in 0..256 {
            let mut line = Vec::new();
            let n = (&mut self.stream)
                .take(65536)
                .read_until(b'\n', &mut line)
                .await?;
            anyhow::ensure!(
                n > 0 && line.last() == Some(&b'\n'),
                "IPC frame limit or disconnect"
            );
            let reply: Value = serde_json::from_slice(&line).context("Invalid IPC JSON")?;
            if reply["event"].as_str() == Some("end-file")
                && reply["reason"].as_str() == Some("error")
            {
                self.media_error = true;
            }
            if reply["request_id"].as_u64() == Some(id) {
                let code = reply["error"].as_str().unwrap_or("unknown");
                let code = if code.len() <= 80
                    && code
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'_' | b'-'))
                {
                    code
                } else {
                    "unknown"
                };
                anyhow::ensure!(code == "success", "mpv IPC: {code}");
                return Ok(reply.get("data").cloned().unwrap_or(Value::Null));
            }
        }
        anyhow::bail!("IPC event limit")
    }
}

/// One backend. The temporary directory is 0700 on Unix; mpv's Windows pipe
/// grants only the current SID/integrity level and rejects remote clients.
pub struct Mpv {
    connection: Connection,
    runtime: tokio::runtime::Runtime,
    process: ManagedProcess,
    _directory: tempfile::TempDir,
}
impl Mpv {
    pub fn start(executable: &Path, null_audio: bool) -> Result<Self> {
        Self::start_cancellable(executable, null_audio, &|| false)
    }
    fn start_cancellable(
        executable: &Path,
        null_audio: bool,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self> {
        anyhow::ensure!(!cancelled(), "mpv startup cancelled");
        let mut version = process_command(executable);
        version.arg("--version");
        let mut limits = Limits::new(Duration::from_secs(10));
        limits.background = false;
        let output = localvox_light_process::run(version, None, limits, cancelled, |_, _| {})?;
        anyhow::ensure!(
            output.termination.success(),
            "mpv version preflight failed: {:?}",
            output.termination
        );
        let version = String::from_utf8_lossy(&output.stdout);
        let version = version
            .lines()
            .next()
            .unwrap_or("")
            .trim_start_matches("mpv ")
            .trim_start_matches('v');
        let mut numbers = version.split('.');
        let major = numbers
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        let minor = numbers
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        anyhow::ensure!(major > 0 || minor >= 38, "mpv >= 0.38 is required");
        let directory = tempfile::tempdir()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        }
        #[cfg(windows)]
        let endpoint = format!(
            "\\\\.\\pipe\\localvox-{}-{}",
            std::process::id(),
            directory.path().file_name().unwrap().to_string_lossy()
        );
        #[cfg(unix)]
        let endpoint = directory.path().join("ipc").to_string_lossy().into_owned();
        let mut command = process_command(executable);
        command
            .args([
                "--no-config",
                "--load-scripts=no",
                "--idle=yes",
                "--pause=yes",
                "--no-video",
                "--terminal=no",
                "--input-terminal=no",
                "--input-default-bindings=no",
                "--ytdl=no",
                "--network-timeout=5",
                "--demuxer-max-bytes=1048576",
                "--demuxer-readahead-secs=1",
            ])
            .arg(format!("--input-ipc-server={endpoint}"));
        if null_audio {
            command.arg("--ao=null");
        }
        let mut process = ManagedProcess::spawn(command)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let connection = runtime.block_on(async {
            let connect = async {
                loop {
                    anyhow::ensure!(!cancelled(), "mpv startup cancelled");
                    #[cfg(windows)]
                    let result =
                        tokio::net::windows::named_pipe::ClientOptions::new().open(&endpoint);
                    #[cfg(unix)]
                    let result = tokio::net::UnixStream::connect(&endpoint).await;
                    if let Ok(stream) = result {
                        #[cfg(windows)]
                        crate::player_security::verify_pipe(&stream, process.pid())?;
                        return Ok::<Connection, anyhow::Error>(Connection {
                            stream: BufReader::new(Box::new(stream)),
                            seq: 0,
                            media_error: false,
                        });
                    }
                    anyhow::ensure!(!process.has_exited()?, "mpv exited during IPC startup");
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            };
            tokio::time::timeout(Duration::from_secs(3), connect)
                .await
                .context("mpv IPC startup timed out")?
        })?;
        Ok(Self {
            connection,
            runtime,
            process,
            _directory: directory,
        })
    }
    pub fn command(&mut self, command: Value) -> Result<Value> {
        let connection = &mut self.connection;
        self.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), connection.call(command))
                .await
                .context("mpv IPC timeout")?
        })
    }
    pub fn pid(&self) -> u32 {
        self.process.pid()
    }
    pub fn load(
        &mut self,
        url: &str,
        headers: &[String],
        start: f64,
        end: Option<f64>,
    ) -> Result<()> {
        anyhow::ensure!(
            start.is_finite()
                && start >= 0.0
                && end.is_none_or(|end| end.is_finite() && end > start),
            "Invalid media interval"
        );
        self.stop()?;
        self.connection.media_error = false;
        self.command(json!(["set_property", "http-header-fields", headers]))?;
        let mut options = json!({"start":start.to_string()});
        if let Some(end) = end {
            options["end"] = json!(end.to_string());
        }
        self.command(json!(["loadfile", url, "replace", -1, options]))?;
        self.command(json!(["set_property", "pause", false]))?;
        Ok(())
    }
    pub fn stop(&mut self) -> Result<()> {
        self.command(json!(["stop"]))?;
        self.command(json!(["set_property", "http-header-fields", []]))?;
        self.connection.media_error = false;
        Ok(())
    }
    /// Absolute seek keeps mpv's pause state and clears the fragment boundary.
    pub fn seek(&mut self, position: f64) -> Result<()> {
        anyhow::ensure!(
            position.is_finite() && position >= 0.0,
            "Invalid seek position"
        );
        let duration = self.command(json!(["get_property", "duration"]))?.as_f64();
        let position = duration
            .filter(|v| v.is_finite() && *v > 0.0)
            .map_or(position, |d| position.min((d - 0.01).max(0.0)));
        self.command(json!(["set_property", "file-local-options/end", "none"]))?;
        self.command(json!(["seek", position, "absolute+exact"]))?;
        Ok(())
    }
    pub fn state(&mut self) -> Result<PlaybackState> {
        anyhow::ensure!(!self.process.has_exited()?, "mpv exited");
        let paused = self
            .command(json!(["get_property", "pause"]))?
            .as_bool()
            .unwrap_or(true);
        let idle = self
            .command(json!(["get_property", "idle-active"]))?
            .as_bool()
            .unwrap_or(true);
        let duration = if idle {
            None
        } else {
            self.command(json!(["get_property", "duration"]))
                .ok()
                .and_then(|v| v.as_f64())
        };
        let position = if idle {
            0.0
        } else {
            self.command(json!(["get_property", "time-pos"]))
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
        };
        Ok(PlaybackState {
            paused,
            idle,
            duration,
            position,
            error: self
                .connection
                .media_error
                .then(|| "Аудио недоступно или не удалось декодировать источник".into()),
            ..Default::default()
        })
    }
}
impl Drop for Mpv {
    fn drop(&mut self) {
        let connection = &mut self.connection;
        let _ = self.runtime.block_on(async {
            tokio::time::timeout(Duration::from_millis(300), connection.call(json!(["quit"]))).await
        });
        let _ = self.process.wait_for_exit(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_backend_error_belongs_to_the_requested_session_and_stop_recovers() {
        let directory = tempfile::tempdir().unwrap();
        let client = Client::new("http://127.0.0.1:1", "synthetic-token".into()).unwrap();
        let player =
            Player::with_backend(client, directory.path().join("missing-mpv.exe"), true).unwrap();
        assert!(player.submit(PlaybackCommand::Load {
            session: "B".into(),
            start: 12.0,
            end: None
        }));
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let state = player
                .rx
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .unwrap();
            if state.error.is_some() {
                assert_eq!(state.session.as_deref(), Some("B"));
                assert!(state.idle && state.paused);
                let error = state.error.unwrap();
                assert!(error.contains("mpv") && error.contains("не найдены"));
                assert!(!error.contains("synthetic-token"));
                break;
            }
        }
        assert!(player.submit(PlaybackCommand::Stop));
        let state = player.rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(state.session.is_none() && state.error.is_none() && state.idle);
    }
    #[test]
    fn ipc_rejects_an_unbounded_or_disconnected_frame() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (stream, mut peer) = tokio::io::duplex(65536);
            let mut connection = Connection {
                stream: BufReader::new(Box::new(stream)),
                seq: 0,
                media_error: false,
            };
            let writer = tokio::spawn(async move {
                peer.write_all(&vec![b'x'; 65536]).await.unwrap();
            });
            assert!(tokio::time::timeout(
                Duration::from_secs(2),
                connection.call(json!(["get_property", "pause"]))
            )
            .await
            .unwrap()
            .is_err());
            writer.await.unwrap();
        });
    }
    #[test]
    fn inherited_api_token_is_removed_from_the_backend_environment() {
        let command = process_command(Path::new("mpv"));
        assert!(command
            .get_envs()
            .any(|(key, value)| key == "LOCALVOX_API_TOKEN" && value.is_none()));
        assert!(command.get_args().next().is_none());
    }
    #[cfg(windows)]
    #[test]
    fn an_unowned_ipc_endpoint_is_rejected() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let endpoint = format!(
            "\\\\.\\pipe\\localvox-fixture-{}",
            directory.path().file_name().unwrap().to_string_lossy()
        );
        runtime.block_on(async {
            let server = tokio::net::windows::named_pipe::ServerOptions::new()
                .first_pipe_instance(true)
                .create(&endpoint)
                .unwrap();
            let client = tokio::net::windows::named_pipe::ClientOptions::new()
                .open(&endpoint)
                .unwrap();
            server.connect().await.unwrap();
            assert!(crate::player_security::verify_pipe(
                &client,
                std::process::id().wrapping_add(1)
            )
            .is_err());
        });
    }
}
