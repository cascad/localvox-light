//! One bounded request at a time, sharing the same owned process-tree lifetime.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct StdioReply {
    pub pid: u32,
    pub elapsed: Duration,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
pub struct StdioSession {
    owner: ManagedProcess,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    stderr: tempfile::NamedTempFile,
    used: bool,
    diagnostic_limit: u64,
}
impl StdioSession {
    pub fn spawn(mut command: Command) -> Result<Self> {
        let stderr = tempfile::NamedTempFile::new().context("creating resident error spool")?;
        command
            .env(CONTAINED_ENV, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(stderr.path())?,
            ));
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
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
        let (child, stdin, stdout, pid) = {
            let _entered = runtime.enter();
            let mut command = CommandWrap::from(tokio::process::Command::from(command));
            command.wrap(KillOnDrop);
            #[cfg(windows)]
            {
                use process_wrap::tokio::{CreationFlags, JobObject};
                use windows::Win32::System::Threading::{
                    BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW,
                };
                command
                    .wrap(CreationFlags(
                        CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS,
                    ))
                    .wrap(JobObject);
            }
            #[cfg(unix)]
            {
                command.wrap(process_wrap::tokio::ProcessGroup::leader());
            }
            let mut child = OwnedChild(command.spawn().context("spawning resident process")?);
            let pid = child.0.id().context("resident process exited at startup")?;
            let direct =
                unsafe { child.0.try_inner_child_mut() }.context("missing resident child")?;
            let stdin = direct.stdin.take().context("resident stdin")?;
            let stdout = direct.stdout.take().context("resident stdout")?;
            (child, stdin, stdout, pid)
        };
        Ok(Self {
            owner: ManagedProcess {
                runtime,
                child: Some(child),
                pid,
            },
            stdin,
            stdout,
            stderr,
            used: false,
            diagnostic_limit: 8 * 1024 * 1024,
        })
    }
    pub fn pid(&self) -> u32 {
        self.owner.pid()
    }
    pub fn has_exited(&mut self) -> Result<bool> {
        if self.stderr.as_file().metadata()?.len() > self.diagnostic_limit {
            self.owner.terminate()?;
            return Ok(true);
        }
        self.owner.has_exited()
    }

    /// Input/output are single newline-delimited frames. Any failure terminates
    /// the process tree before returning; a partial reply is never reusable.
    pub fn request(
        &mut self,
        input: &[u8],
        limits: Limits,
        cancelled: impl Fn() -> bool,
        tick: impl FnMut(u32, Duration),
    ) -> Result<StdioReply> {
        let result = self.request_inner(input, limits, cancelled, tick);
        if result.is_err() {
            let _ = self.owner.terminate();
        }
        result
    }
    fn request_inner(
        &mut self,
        input: &[u8],
        limits: Limits,
        cancelled: impl Fn() -> bool,
        mut tick: impl FnMut(u32, Duration),
    ) -> Result<StdioReply> {
        if input.is_empty()
            || input.len() > 65536
            || input.contains(&b'\n')
            || limits.timeout.is_zero()
            || limits.output_bytes == 0
        {
            let _ = self.owner.terminate();
            anyhow::bail!("resident request must have valid limits and one frame up to 64 KiB");
        }
        let pid = self.pid();
        if self.has_exited()? {
            anyhow::bail!("resident process exited or exceeded its idle diagnostic limit");
        }
        // Child stderr uses append mode, so truncating at a request boundary
        // cannot leave a stale writer offset or mix a previous session's logs.
        if self.used {
            self.stderr.as_file().set_len(0)?;
        }
        self.used = true;
        self.diagnostic_limit = limits.output_bytes;
        let started = Instant::now();
        let stderr = self.stderr.as_file();
        let child = &mut self.owner.child;
        let stdin = &mut self.stdin;
        let stdout = &mut self.stdout;
        let result = self.owner.runtime.block_on(async {
            let exchange = async {
                stdin.write_all(input).await?;
                stdin.write_all(b"\n").await?;
                stdin.flush().await?;
                let mut bytes = Vec::new();
                let mut buffer = [0; 8192];
                loop {
                    let n = stdout.read(&mut buffer).await?;
                    anyhow::ensure!(n > 0, "resident stdout closed before a complete reply");
                    anyhow::ensure!(bytes.len() as u64 + n as u64 <= limits.output_bytes, "resident output limit exceeded");
                    if let Some(end) = buffer[..n].iter().position(|b| *b == b'\n') {
                        anyhow::ensure!(end + 1 == n, "resident sent extra stdout outside the reply");
                        bytes.extend_from_slice(&buffer[..end]);
                        return Ok::<_,anyhow::Error>(bytes);
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                }
            };
            tokio::pin!(exchange);
            let mut heartbeat = Instant::now();
            tick(pid, started.elapsed());
            loop {
                anyhow::ensure!(!cancelled(), "resident request cancelled");
                anyhow::ensure!(started.elapsed() < limits.timeout, "resident request timed out");
                anyhow::ensure!(stderr.metadata()?.len() <= limits.output_bytes, "resident diagnostic limit exceeded");
                tokio::select! {
                    result = &mut exchange => break result,
                    _ = tokio::time::sleep(Duration::from_millis(25)) => {
                        if let Some(child) = child.as_mut() {
                            anyhow::ensure!(unsafe { child.0.try_inner_child_mut() }.context("missing resident process")?.try_wait()?.is_none(), "resident process exited unexpectedly");
                        }
                        if heartbeat.elapsed() >= Duration::from_secs(15) {
                            tick(pid, started.elapsed()); heartbeat = Instant::now();
                        }
                    }
                }
            }
        });
        let result = result.and_then(|bytes| {
            anyhow::ensure!(
                self.stderr.as_file().metadata()?.len() <= limits.output_bytes,
                "resident diagnostic limit exceeded"
            );
            Ok(bytes)
        });
        if result.is_err() {
            let _ = self.owner.terminate();
        }
        let mut diagnostics = self.stderr.reopen()?;
        let stderr = read_bounded(&mut diagnostics, limits.output_bytes)?;
        let result = result.with_context(|| {
            let tail = &stderr[stderr.len().saturating_sub(4096)..];
            format!(
                "resident process request failed: {}",
                String::from_utf8_lossy(tail)
            )
        });
        Ok(StdioReply {
            pid,
            elapsed: started.elapsed(),
            stdout: result?,
            stderr,
        })
    }
}
