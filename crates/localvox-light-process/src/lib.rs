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

/// Synchronous boundary for background threads. `tick` must be quick; called at start and every
/// 15 seconds, with PID and wall-clock elapsed time. Arguments/environment are never logged here.
/// File-backed stdio avoids the stdin-vs-stdout pipe deadlock and descendants holding pipes open.
pub fn run(
    mut command: Command,
    input: Option<&[u8]>,
    limits: Limits,
    cancelled: impl Fn() -> bool,
    mut tick: impl FnMut(u32, Duration),
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
    let mut stdout = tempfile::tempfile().context("creating process output spool")?;
    let mut stderr = tempfile::tempfile().context("creating process error spool")?;
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
            stdout: read_bounded(&mut stdout, limits.output_bytes)?,
            stderr: read_bounded(&mut stderr, limits.output_bytes)?,
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
