//! Verify the API before dispatching workspace requests. The daemon outlives
//! this client; unlike mpv it is deliberately not a kill-on-drop child.
use anyhow::{bail, Context, Result};
use localvox_light_client::Client;
use std::{
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

pub const START_TIMEOUT: Duration = Duration::from_secs(15);

pub struct Connecting {
    pub rx: Receiver<Result<Client>>,
}
impl Connecting {
    pub fn begin(client: Client, allow_start: bool) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("tui-connect".into())
            .spawn(move || {
                let result = ensure(&client, allow_start, START_TIMEOUT, || {
                    spawn_daemon(&client)
                });
                let _ = tx.send(result.map(|()| client));
            })?;
        Ok(Self { rx })
    }
}

fn compatible(client: &Client, timeout: Duration) -> Result<()> {
    let health = client.health_timeout(timeout)?;
    anyhow::ensure!(
        health.compatible(),
        "Этот адрес не предоставляет совместимый API LocalVox. Обновите комплект демона и клиента"
    );
    Ok(())
}

fn ensure(
    client: &Client,
    allow_start: bool,
    budget: Duration,
    mut start: impl FnMut() -> Result<Child>,
) -> Result<()> {
    let deadline = Instant::now() + budget;
    let initial = compatible(client, budget.min(Duration::from_secs(3)));
    if initial.is_ok() {
        return Ok(());
    }
    let failure = initial.unwrap_err();
    if !allow_start
        || client.local_http_bind().is_none()
        || !failure
            .downcast_ref::<localvox_light_client::NetworkUnavailable>()
            .is_some_and(|e| e.connection_refused())
    {
        return Err(failure.context("Подключение не установлено; проверьте адрес и токен"));
    }
    // Do not start a daemon over an occupied/unauthorized/incompatible port.
    // Only the HTTP transport's explicit connection-refused cause permits the
    // single startup attempt. A timeout, DNS/TLS error or any HTTP response
    // does not. In particular Windows can take over a second to report refusal.
    let mut child = start().context("Не удалось запустить соседний localvox-light --daemon")?;
    loop {
        if let Some(status) = child.try_wait()? {
            bail!("Демон завершился до готовности ({status}). Запустите localvox-light --daemon и проверьте диагностику");
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            // Startup timeout does not authorize killing a daemon that could
            // still be opening devices. Reap it if it eventually terminates.
            reap_later(child);
            bail!("API не стал готов за {} с. Проверьте localvox-light --daemon; подключитесь повторно через палитру", budget.as_secs_f32());
        }
        if compatible(client, remaining.min(Duration::from_millis(500))).is_ok() {
            reap_later(child);
            return Ok(());
        }
        std::thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100)),
        );
    }
}

fn reap_later(mut child: Child) {
    let _ = std::thread::Builder::new()
        .name("tui-daemon-wait".into())
        .spawn(move || {
            let _ = child.wait();
        });
}

fn spawn_daemon(client: &Client) -> Result<Child> {
    let exe = std::env::current_exe()?;
    let folder = exe.parent().context("Каталог клиента неизвестен")?;
    let binary = folder.join(if cfg!(windows) {
        "localvox-light.exe"
    } else {
        "localvox-light"
    });
    anyhow::ensure!(
        binary.is_file(),
        "Рядом с localvox-tui нет localvox-light; соберите или установите полный комплект"
    );
    let mut command = Command::new(binary);
    command
        .arg("--daemon")
        .current_dir(std::env::current_dir()?)
        .env(
            "LOCALVOX_API_BIND",
            client
                .local_http_bind()
                .context("Нужен локальный HTTP адрес")?,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().context("Запуск localvox-light")
}

#[cfg(test)]
mod tests;
