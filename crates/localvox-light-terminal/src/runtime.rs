use crate::{
    app::{Action, App},
    io::{Io, Resource, Task},
    render,
};
use anyhow::Result;
use crossterm::{
    cursor::Show,
    event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use localvox_light_client::Client;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io::{self, IsTerminal},
    time::{Duration, Instant},
};

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            LeaveAlternateScreen,
            Show
        );
    }
}
fn with_terminal<T>(
    body: impl FnOnce(&mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<T>,
) -> Result<T> {
    let _guard = TerminalGuard;
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    body(&mut terminal)
}
pub fn run(mut client: Client, mut app: App) -> Result<()> {
    anyhow::ensure!(
        io::stdout().is_terminal() && io::stdin().is_terminal(),
        "Нужен интерактивный терминал"
    );
    app.address = client.address().into();
    let mut connecting = Some(crate::launcher::Connecting::begin(client.clone(), true)?);
    let mut network: Option<Io> = None;
    let mut player = crate::player::Player::new(client.clone())?;
    let start = Instant::now();
    app.status = "Проверка API LocalVox… Подключение и токен доступны через палитру".into();
    with_terminal(|terminal| {
        let mut last_poll = Instant::now();
        let mut last_record = Instant::now();
        let mut dirty = true;
        let mut last_draw = Instant::now();
        while !app.quit {
            if let Some((address, token)) = app.connection_request.take() {
                if connecting.is_some() {
                    app.status =
                        "Проверка подключения ещё выполняется; повторите после её завершения"
                            .into();
                } else {
                    match client
                        .reconnect(&address, token)
                        .and_then(|c| crate::launcher::Connecting::begin(c, false))
                    {
                        Ok(attempt) => {
                            connecting = Some(attempt);
                            // A pending mutation is never resubmitted on a new connection.
                            network = None;
                            player = crate::player::Player::new(client.clone())?;
                            app.effects.clear();
                            app.player_effects.clear();
                            app.status = "Проверка нового адреса и авторизации…".into();
                        }
                        Err(e) => app.status = e.to_string(),
                    }
                }
                dirty = true;
            }
            if let Some(result) = connecting
                .as_ref()
                .and_then(|attempt| attempt.rx.try_recv().ok())
            {
                connecting = None;
                match result.and_then(|c| Io::new(c.clone()).map(|io| (c, io))) {
                    Ok((next, next_io)) => {
                        client = next;
                        network = Some(next_io);
                        player = crate::player::Player::new(client.clone())?;
                        let ascii = app.ascii;
                        let light = app.light;
                        let monochrome = app.monochrome;
                        let color16 = app.color16;
                        app = App {
                            ascii,
                            light,
                            monochrome,
                            color16,
                            address: client.address().into(),
                            ..Default::default()
                        };
                        app.status = "Подключено к API LocalVox".into();
                        app.refresh();
                    }
                    Err(e) => {
                        app.status = format!("{e:#}. Палитра → Подключение и токен");
                    }
                }
                dirty = true;
            }
            let size = terminal.size()?;
            app.viewport = (size.width, size.height);
            app.tick(start.elapsed().as_millis().min(u64::MAX as u128) as u64);
            app.wall_ms = chrono::Utc::now().timestamp_millis();
            for state in player.rx.try_iter() {
                if state.session.is_none() || state.session == app.current {
                    app.playback = state;
                    dirty = true;
                }
            }
            for command in std::mem::take(&mut app.player_effects) {
                if network.is_none() {
                    app.status = "Сначала подключитесь к совместимому API LocalVox".into();
                } else if !player.submit(command) {
                    app.status = "Очередь плеера занята; повторите действие".into();
                }
            }
            if let Some(network) = network.as_mut() {
                for response in network.rx.try_iter().collect::<Vec<_>>() {
                    dirty = true;
                    network.finished(response.resource, response.epoch);
                    app.apply(response);
                }
                network.set_epoch(app.epoch);
                for (resource, task) in std::mem::take(&mut app.effects) {
                    let global = matches!(
                        resource,
                        Resource::Sessions
                            | Resource::Record
                            | Resource::Jobs
                            | Resource::Action
                            | Resource::Control
                            | Resource::Route
                    );
                    if !network.submit(if global { 0 } else { app.epoch }, resource, task) {
                        app.submission_rejected(resource);
                    }
                }
                if last_record.elapsed() >= Duration::from_secs(1) {
                    network.submit(
                        0,
                        Resource::Record,
                        Task::Read(Resource::Record, String::new()),
                    );
                    last_record = Instant::now();
                }
                if last_poll.elapsed() >= Duration::from_secs(3) {
                    app.refresh();
                    last_poll = Instant::now();
                }
            } else {
                let rejected = !app.effects.is_empty();
                for (resource, _) in std::mem::take(&mut app.effects) {
                    app.submission_rejected(resource);
                }
                if rejected {
                    app.status = "Нет проверенного подключения; действие не отправлено, введённый текст сохранён".into();
                }
            }
            if dirty || last_draw.elapsed() >= Duration::from_secs(1) {
                terminal.draw(|f| render::draw(f, &app))?;
                last_draw = Instant::now();
                dirty = false;
            }
            if event::poll(Duration::from_millis(50))? {
                dirty = true;
                match event::read()? {
                    Event::Key(k) if k.kind != KeyEventKind::Release => match k.code {
                        KeyCode::F(8) => app.command(Action::StopRecording),
                        KeyCode::F(9) => app.command(Action::StartRecording),
                        _ => app.key(k),
                    },
                    Event::Paste(text) => app.paste(&text),
                    _ => {}
                }
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests;
