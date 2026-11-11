use crate::{
    app::{Action, App},
    io::{Io, Resource, Task},
    render,
};
use anyhow::Result;
use crossterm::{
    cursor::Show,
    event::{DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use localvox_light_client::Client;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io::{self, IsTerminal},
    time::{Duration, Instant},
};
mod input;

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
        let mut input = input::Input::new()?;
        let mut last_poll = Instant::now();
        let mut last_record = Instant::now();
        let mut dirty = true;
        let mut last_draw = Instant::now();
        let mut last_animation = None;
        let mut last_quit_hint = false;
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
                        Err(e) => {
                            app.client_error("Подключение", &format!("{e:#}"));
                            app.status = "Не удалось подключиться · ~ консоль ошибок".into();
                        }
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
                        app.console.recover_operation(&crate::diagnostics::Context {
                            source: "Клиент".into(),
                            operation: "Подключение".into(),
                            ..Default::default()
                        });
                        let console = std::mem::take(&mut app.console);
                        app = App {
                            console,
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
                        app.client_error("Подключение", &format!("{e:#}"));
                        app.status = "Нет подключения · ~ консоль · : Подключение и токен".into();
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
                    app.apply_playback(state);
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
                            | Resource::Queue
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
            let animation = render::animation_frame(&app);
            let quit_hint = app.quit_pending();
            if dirty
                || animation != last_animation
                || quit_hint != last_quit_hint
                || last_draw.elapsed() >= Duration::from_secs(1)
            {
                terminal.draw(|f| render::draw(f, &app))?;
                last_draw = Instant::now();
                last_animation = animation;
                last_quit_hint = quit_hint;
                dirty = false;
            }
            if let Some(event) = input.read(Duration::from_millis(50))? {
                dirty = true;
                dispatch(&mut app, event, || {
                    Ok(arboard::Clipboard::new()?.get_text()?)
                });
            }
        }
        Ok(())
    })
}

fn dispatch(app: &mut App, event: Event, clipboard: impl FnOnce() -> Result<String>) {
    use crossterm::event::KeyModifiers;
    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release => {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && matches!(key.code, KeyCode::Char('v' | 'V' | 'м' | 'М' | '\u{16}'))
            {
                app.cancel_quit_confirmation();
                match clipboard() {
                    Ok(text) => app.paste(&text),
                    Err(error) => {
                        app.client_error(
                            "Вставка",
                            &format!("Не удалось прочитать текст буфера обмена: {error}"),
                        );
                        app.status = "Вставка недоступна · ~ консоль ошибок".into();
                    }
                }
            } else {
                match key.code {
                    KeyCode::F(8) => app.command(Action::StopRecording),
                    KeyCode::F(9) => app.command(Action::StartRecording),
                    _ => app.key(key),
                }
            }
        }
        Event::Paste(text) => app.paste(&text),
        _ => {}
    }
}

#[cfg(all(test, windows))]
mod paste_probe;
#[cfg(test)]
mod tests;
