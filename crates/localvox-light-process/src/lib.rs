//! Process lifetime is owned by the caller. No pipe-draining threads, shell, or model dependencies.
//! Windows uses a kill-on-close Job Object; Unix uses a process group. Nested Unix workers must
//! inherit the enclosing group so that cancelling the coordinator's child also kills its tools.
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{sync_channel, Receiver, SyncSender},
    Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}
pub struct OutputChunk {
    pub stream: Stream,
    pub bytes: Vec<u8>,
}
/// Observation is optional and lossy under backpressure; supervising the child always wins.
pub struct OutputObserver {
    sender: SyncSender<OutputChunk>,
    dropped: Arc<AtomicU64>,
}
impl OutputObserver {
    pub fn channel(capacity: usize) -> (Self, Receiver<OutputChunk>, Arc<AtomicU64>) {
        let (sender, receiver) = sync_channel(capacity.clamp(1, 64));
        let dropped = Arc::new(AtomicU64::new(0));
        (
            Self {
                sender,
                dropped: dropped.clone(),
            },
            receiver,
            dropped,
        )
    }
}
fn pump(file: &mut File, stream: Stream, observer: &OutputObserver, limit: u64) -> Result<usize> {
    let mut total = 0;
    let mut bytes = [0; 8192];
    for _ in 0..8 {
        let remaining = limit.saturating_sub(file.stream_position()?);
        if remaining == 0 {
            break;
        }
        let n = file.read(&mut bytes[..remaining.min(8192) as usize])?;
        if n == 0 {
            break;
        }
        total += n;
        if observer
            .sender
            .try_send(OutputChunk {
                stream,
                bytes: bytes[..n].to_vec(),
            })
            .is_err()
        {
            observer.dropped.fetch_add(n as u64, Ordering::Relaxed);
        }
    }
    Ok(total)
}

pub const CONTAINED_ENV: &str = "LOCALVOX_PROCESS_CONTAINED";

#[derive(Clone, Copy, Debug, Default)]
pub enum Scope {
    #[default]
    Tree,
    /// A tool within an already supervised worker. On Windows nested jobs remain in the parent job.
    Inherit,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub timeout: Duration,
    /// Monitored per-stream spool limit; output returned in memory never exceeds this limit.
    pub output_bytes: u64,
    pub background: bool,
    pub scope: Scope,
}

impl Limits {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            output_bytes: 8 * 1024 * 1024,
            background: true,
            scope: Scope::Tree,
        }
    }
}

#[derive(Debug)]
pub enum Termination {
    Exited(ExitStatus),
    Cancelled,
    TimedOut,
    OutputLimit,
}

impl Termination {
    pub fn success(&self) -> bool {
        matches!(self, Self::Exited(status) if status.success())
    }
}

#[derive(Debug)]
pub struct Output {
    pub termination: Termination,
    pub pid: u32,
    pub elapsed: Duration,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

struct OwnedChild(Box<dyn ChildWrapper>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Also kills surviving descendants when the immediate child exits successfully or a
        // callback panics. KillOnDrop additionally closes the Windows job on abrupt parent death.
        let _ = self.0.start_kill();
    }
}

/// A long-lived tool owned by one adapter. It has no daemon lifetime authority.
/// The adapter sends its protocol's graceful quit before dropping this guard.
pub struct ManagedProcess {
    runtime: tokio::runtime::Runtime,
    child: Option<OwnedChild>,
    pid: u32,
}
impl ManagedProcess {
    pub fn spawn(mut command: Command) -> Result<Self> {
        command.env(CONTAINED_ENV, "1");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let child = {
            let _entered = runtime.enter();
            let mut command = CommandWrap::from(tokio::process::Command::from(command));
            command.wrap(KillOnDrop);
            #[cfg(windows)]
            {
                use process_wrap::tokio::{CreationFlags, JobObject};
                use windows::Win32::System::Threading::CREATE_NO_WINDOW;
                command
                    .wrap(CreationFlags(CREATE_NO_WINDOW))
                    .wrap(JobObject);
            }
            #[cfg(unix)]
            {
                command.wrap(process_wrap::tokio::ProcessGroup::leader());
            }
            OwnedChild(command.spawn().context("spawning managed tool")?)
        };
        let pid = child.0.id().context("managed tool exited before startup")?;
        Ok(Self {
            runtime,
            child: Some(child),
            pid,
        })
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn has_exited(&mut self) -> Result<bool> {
        match &mut self.child {
            Some(child) => Ok(unsafe { child.0.try_inner_child_mut() }
                .context("missing managed tool")?
                .try_wait()?
                .is_some()),
            None => Ok(true),
        }
    }
    pub fn wait_for_exit(&mut self, timeout: Duration) -> Result<()> {
        if let Some(child) = &mut self.child {
            let direct =
                unsafe { child.0.try_inner_child_mut() }.context("missing managed tool")?;
            self.runtime
                .block_on(async { tokio::time::timeout(timeout, direct.wait()).await })
                .context("managed tool did not exit")??;
        }
        self.child.take();
        Ok(())
    }
    pub fn terminate(&mut self) -> Result<()> {
        if let Some(child) = &mut self.child {
            let _ = child.0.start_kill();
        }
        let result = self.wait_for_exit(Duration::from_secs(3));
        self.child.take();
        result
    }
}
impl Drop for ManagedProcess {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

/// Synchronous boundary for background threads. `tick` must be quick; called at start and every
/// 15 seconds, with PID and wall-clock elapsed time. Arguments/environment are never logged here.
/// File-backed stdio avoids the stdin-vs-stdout pipe deadlock and descendants holding pipes open.
pub fn run(
    command: Command,
    input: Option<&[u8]>,
    limits: Limits,
    cancelled: impl Fn() -> bool,
    tick: impl FnMut(u32, Duration),
) -> Result<Output> {
    run_observed(command, input, limits, cancelled, tick, None)
}

pub fn run_observed(
    mut command: Command,
    input: Option<&[u8]>,
    limits: Limits,
    cancelled: impl Fn() -> bool,
    mut tick: impl FnMut(u32, Duration),
    observer: Option<OutputObserver>,
) -> Result<Output> {
    anyhow::ensure!(
        !limits.timeout.is_zero(),
        "process timeout must be positive"
    );
    anyhow::ensure!(
        limits.output_bytes > 0,
        "process output limit must be positive"
    );
    let mut stdin = tempfile::tempfile().context("creating process input spool")?;
    stdin.write_all(input.unwrap_or_default())?;
    stdin.rewind()?;
    let mut stdout_spool =
        tempfile::NamedTempFile::new().context("creating process output spool")?;
    let mut stderr_spool =
        tempfile::NamedTempFile::new().context("creating process error spool")?;
    // Reopen yields independent file offsets; seeking a clone would move the child's writer.
    let mut stdout_reader = if observer.is_some() {
        Some(stdout_spool.reopen()?)
    } else {
        None
    };
    let mut stderr_reader = if observer.is_some() {
        Some(stderr_spool.reopen()?)
    } else {
        None
    };
    let stdout = stdout_spool.as_file_mut();
    let stderr = stderr_spool.as_file_mut();
    command.stdin(Stdio::from(stdin));
    command.stdout(Stdio::from(stdout.try_clone()?));
    command.stderr(Stdio::from(stderr.try_clone()?));
    command.env(CONTAINED_ENV, "1");
    #[cfg(unix)]
    if limits.background {
        use std::os::unix::process::CommandExt;
        // nice is async-signal-safe; background processing yields to audio capture.
        unsafe {
            command.pre_exec(|| {
                libc::nice(5);
                Ok(())
            });
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let started = Instant::now();
        let mut command = CommandWrap::from(tokio::process::Command::from(command));
        command.wrap(KillOnDrop);
        #[cfg(windows)]
        {
            use process_wrap::tokio::{CreationFlags, JobObject};
            use windows::Win32::System::Threading::{
                BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW,
            };
            let flags = if limits.background {
                CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS
            } else {
                CREATE_NO_WINDOW
            };
            command.wrap(CreationFlags(flags)).wrap(JobObject);
        }
        #[cfg(unix)]
        if matches!(limits.scope, Scope::Tree) {
            command.wrap(process_wrap::tokio::ProcessGroup::leader());
        }
        let mut child = OwnedChild(command.spawn().context("spawning supervised process")?);
        drop(command);
        let pid = child
            .0
            .id()
            .context("child exited before its PID was available")?;
        tick(pid, started.elapsed());
        let mut heartbeat = Instant::now();
        let mut termination = loop {
            if cancelled() {
                break Termination::Cancelled;
            }
            if stdout.metadata()?.len() > limits.output_bytes
                || stderr.metadata()?.len() > limits.output_bytes
            {
                break Termination::OutputLimit;
            }
            if let Some(observer) = &observer {
                if let Some(reader) = &mut stdout_reader {
                    pump(reader, Stream::Stdout, observer, limits.output_bytes)?;
                }
                if let Some(reader) = &mut stderr_reader {
                    pump(reader, Stream::Stderr, observer, limits.output_bytes)?;
                }
            }
            // We only use the native wait APIs; wrapper wait caches are never consulted.
            // Waiting for the entire job here would hang if a descendant outlived its parent.
            let status = unsafe { child.0.try_inner_child_mut() }
                .context("supervisor requires a native child")?
                .try_wait()?;
            if let Some(status) = status {
                break Termination::Exited(status);
            }
            if started.elapsed() >= limits.timeout {
                break Termination::TimedOut;
            }
            if heartbeat.elapsed() >= Duration::from_secs(15) {
                tick(pid, started.elapsed());
                heartbeat = Instant::now();
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let _ = child.0.start_kill();
        // Reap the direct child before dropping files. The OS container owns descendants.
        let direct = unsafe { child.0.try_inner_child_mut() }.context("missing native child")?;
        tokio::time::timeout(Duration::from_secs(5), direct.wait())
            .await
            .context("process did not exit after termination")??;
        drop(child);
        if let Some(observer) = &observer {
            for (reader, stream) in [
                (&mut stdout_reader, Stream::Stdout),
                (&mut stderr_reader, Stream::Stderr),
            ] {
                if let Some(reader) = reader {
                    while pump(reader, stream, observer, limits.output_bytes)? > 0 {}
                }
            }
        }
        // The child can fill the file between the last metadata check and try_wait.
        if termination.success()
            && (stdout.metadata()?.len() > limits.output_bytes
                || stderr.metadata()?.len() > limits.output_bytes)
        {
            termination = Termination::OutputLimit;
        }
        Ok(Output {
            termination,
            pid,
            elapsed: started.elapsed(),
            stdout: read_bounded(stdout, limits.output_bytes)?,
            stderr: read_bounded(stderr, limits.output_bytes)?,
        })
    })
}

fn read_bounded(file: &mut File, limit: u64) -> Result<Vec<u8>> {
    // Keep the tail: the failure reason is usually the final line.
    let len = file.metadata()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(limit)))?;
    let mut out = Vec::new();
    file.take(limit).read_to_end(&mut out)?;
    Ok(out)
}
