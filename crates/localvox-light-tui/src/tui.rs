//! Terminal UI: транскрипт + лог (как client-reliable: цвета, хоткеи по физ. клавишам, F2 — устройства).

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::path::PathBuf;

use anyhow::Result;
use chrono::Local;
use crossbeam_channel::{Receiver, Sender};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;

use localvox_light_core::audio::{collect_input_devices, format_device_display, list_output_device_names};
use localvox_light_core::events::{StructuredLog, UiMsg};
use localvox_light_core::light_config::LightDeviceConfig;
use localvox_light_core::transcript::export_sorted_jsonl;

use crate::keys::key_matches;

/// Файл transcript.jsonl не ограничен — буфер TUI должен быть большим, иначе «в файле больше строк».
const MAX_TRANSCRIPT_LINES: usize = 50_000;
const MAX_LOG_LINES: usize = 800;

fn src_prefix_chars(source_id: u8) -> usize {
    match source_id {
        0 | 1 => 4,
        _ => 0,
    }
}

/// Высота в строках терминала после переноса по ширине (как в client-reliable).
fn row_visual_height(row: &TranscriptRow, inner_w: usize) -> usize {
    if inner_w == 0 {
        return 1;
    }
    let n = row.ts.chars().count()
        + 1
        + src_prefix_chars(row.source_id)
        + row.text.chars().count();
    n.div_ceil(inner_w).max(1)
}

/// Визуальные строки блока «Статус» с Wrap (оценка по ширине `Line`, как у Paragraph).
fn status_wrapped_row_count(lines: &[Line], inner_w: usize) -> usize {
    if lines.is_empty() {
        return 1;
    }
    if inner_w == 0 {
        return lines.len().max(1);
    }
    lines
        .iter()
        .map(|line| line.width().div_ceil(inner_w).max(1))
        .sum()
}

#[derive(Clone, Debug)]
struct TranscriptRow {
    ts: String,
    source_id: u8,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TuiMode {
    Main,
    Settings,
}

struct SettingsState {
    input_devices: Vec<(usize, String)>,
    output_devices: Vec<(usize, String)>,
    input_state: ListState,
    output_state: ListState,
    focus: u8,
    saved_msg: Option<String>,
    saved_at: Option<Instant>,
}

fn format_log_line(log: &StructuredLog) -> String {
    let ts = Local::now().format("%H:%M:%S");
    let src = format!("src{}", log.source_id);
    format!(
        "{} {:<8} {:<4} chunk={:>5.2}s proc={:>5.3}s {}",
        ts,
        log.stage,
        src,
        log.chunk_sec,
        log.proc_sec,
        log.detail
    )
}

fn input_selected_index(mic_query: &str, devices: &[(usize, String)]) -> usize {
    if let Ok(idx) = mic_query.parse::<usize>() {
        if idx < devices.len() {
            return idx;
        }
    }
    let needle = mic_query.to_lowercase();
    devices
        .iter()
        .position(|(_, n)| n.to_lowercase().contains(&needle))
        .unwrap_or(0)
}

/// Индекс в `output_devices` (без строки «— нет —»).
fn output_device_row_index(loopback_device: &str, outputs: &[(usize, String)]) -> usize {
    if let Ok(idx) = loopback_device.parse::<usize>() {
        return outputs
            .iter()
            .position(|(i, _)| *i == idx)
            .unwrap_or(0);
    }
    let needle = loopback_device.to_lowercase();
    outputs
        .iter()
        .position(|(_, n)| {
            n.eq_ignore_ascii_case(loopback_device) || n.to_lowercase().contains(&needle)
        })
        .unwrap_or(0)
}

fn build_settings_state(current: &LightDeviceConfig) -> SettingsState {
    let input_devices: Vec<_> = collect_input_devices()
        .into_iter()
        .enumerate()
        .map(|(i, (dev, n))| (i, format_device_display(&dev, &n, "")))
        .collect();
    let output_devices = list_output_device_names();
    let input_sel = input_selected_index(&current.mic, &input_devices)
        .min(input_devices.len().saturating_sub(1));
    // Строка 0 в списке — «— нет —» (без loopback), дальше как в client-reliable.
    let output_sel = if !current.loopback {
        0
    } else {
        let row = output_device_row_index(&current.loopback_device, &output_devices)
            .min(output_devices.len().saturating_sub(1));
        (1 + row).min(output_devices.len())
    };

    let mut input_state = ListState::default();
    input_state.select(Some(input_sel));
    let mut output_state = ListState::default();
    output_state.select(Some(output_sel));

    SettingsState {
        input_devices,
        output_devices,
        input_state,
        output_state,
        focus: 0,
        saved_msg: None,
        saved_at: None,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    ui_rx: &Receiver<UiMsg>,
    reset_tx: Sender<()>,
    running: Arc<AtomicBool>,
    record_pcm: Arc<AtomicBool>,
    session_hint: String,
    mut device_config: LightDeviceConfig,
    config_save_path: PathBuf,
    verbose: bool,
) -> Result<()> {
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut transcript_rows: Vec<TranscriptRow> = Vec::new();
    let mut log_lines: Vec<String> = Vec::new();
    let mut status = session_hint;
    // (число wav без строки в jsonl, сумма их МБ, МБ всех файлов в рабочем каталоге) — см. блок «Статус».
    let mut queue_pending: Option<(usize, f64, f64)> = None;
    // Уровень mic (src0) / sys (src1), как в client-reliable.
    let mut audio_level: f32 = 0.0;
    let mut audio_level2: f32 = 0.0;
    let mut focus_log: bool = false;
    let mut log_scroll: usize = 0;
    // Смещение по визуальным строкам после wrap (не по записям).
    let mut t_visual_scroll: usize = 0;
    let mut t_follow_bottom: bool = true;
    let mut t_pending_follow: bool = false;
    let mut last_inner_w: usize = 80;
    let mut last_inner_h: usize = 5;
    let mut t_scroll_max_hint: usize = 0;
    let mut t_suppress_until_clear: bool = false;

    let mut mode = TuiMode::Main;
    let mut settings_state: Option<SettingsState> = None;

    // Всплывающее уведомление снизу (дамп, F2 и т.д.)
    let mut toast_text: Option<String> = None;
    let mut toast_success: bool = true;
    let mut toast_at: Option<Instant> = None;

    let mut engine_workspace_dir: Option<PathBuf> = None;
    let mut engine_dump_dir: Option<PathBuf> = None;
    // Ошибка движка: не выходим из TUI по running == false, пока пользователь не нажмёт q.
    let mut fatal_error: Option<String> = None;

    let mut last_draw = Instant::now();

    loop {
        if let Some(ref at) = toast_at {
            if at.elapsed() >= Duration::from_secs(2) {
                toast_text = None;
                toast_at = None;
            }
        }

        if let (TuiMode::Settings, Some(ref mut st)) = (&mode, &mut settings_state) {
            if let Some(saved_at) = st.saved_at {
                if saved_at.elapsed() >= Duration::from_secs(2) {
                    st.saved_msg = None;
                    st.saved_at = None;
                }
            }
        }

        if event::poll(Duration::from_millis(30))? {
            match event::read()? {
                Event::Key(key) if key.kind == crossterm::event::KeyEventKind::Press => {
                    if mode == TuiMode::Settings {
                        let st = settings_state.as_mut().unwrap();
                        match key.code {
                            KeyCode::Esc => {
                                mode = TuiMode::Main;
                            }
                            code if key_matches(code, 's') => {
                                let input_idx = st.input_state.selected().unwrap_or(0);
                                let mic = st
                                    .input_devices
                                    .get(input_idx)
                                    .map(|(i, _)| format!("{i}"))
                                    .or_else(|| st.input_devices.first().map(|(i, _)| format!("{i}")))
                                    .unwrap_or_default();
                                let loopback_sel = st.output_state.selected().unwrap_or(0);
                                let (loopback, loopback_device) = if loopback_sel == 0 {
                                    (false, "default-output".into())
                                } else if let Some((_, name)) =
                                    st.output_devices.get(loopback_sel.saturating_sub(1))
                                {
                                    (true, name.clone())
                                } else {
                                    (false, "default-output".into())
                                };
                                device_config = LightDeviceConfig {
                                    mic,
                                    loopback,
                                    loopback_device,
                                };
                                let path = &config_save_path;
                                st.saved_msg = Some(match device_config.save(path) {
                                    Ok(_) => {
                                        format!("Сохранено: {}. Перезапустите localvox-light.", path.display())
                                    }
                                    Err(e) => format!("Ошибка: {e}"),
                                });
                                st.saved_at = Some(Instant::now());
                            }
                            KeyCode::Tab | KeyCode::BackTab => st.focus = 1 - st.focus,
                            code if code == KeyCode::Up || key_matches(code, 'k') => {
                                let state = if st.focus == 0 {
                                    &mut st.input_state
                                } else {
                                    &mut st.output_state
                                };
                                let len = if st.focus == 0 {
                                    st.input_devices.len()
                                } else {
                                    1 + st.output_devices.len()
                                };
                                let i = state.selected().unwrap_or(0).saturating_sub(1);
                                state.select(Some(i.min(len.saturating_sub(1))));
                            }
                            code if code == KeyCode::Down || key_matches(code, 'j') => {
                                let state = if st.focus == 0 {
                                    &mut st.input_state
                                } else {
                                    &mut st.output_state
                                };
                                let len = if st.focus == 0 {
                                    st.input_devices.len()
                                } else {
                                    1 + st.output_devices.len()
                                };
                                let i = state.selected().unwrap_or(0).saturating_add(1);
                                state.select(Some(i.min(len.saturating_sub(1))));
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::F(2) => {
                                settings_state = Some(build_settings_state(&device_config));
                                mode = TuiMode::Settings;
                            }
                            code if key_matches(code, 'q') || code == KeyCode::Esc => {
                                running.store(false, Ordering::SeqCst);
                                break;
                            }
                            code if key.modifiers.contains(KeyModifiers::CONTROL)
                                && key_matches(code, 'c') =>
                            {
                                running.store(false, Ordering::SeqCst);
                                break;
                            }
                            code if key_matches(code, 'x') => {
                                t_suppress_until_clear = true;
                                let _ = reset_tx.send(());
                            }
                            code if key_matches(code, 'r') => {
                                let cur = record_pcm.load(Ordering::Relaxed);
                                record_pcm.store(!cur, Ordering::Relaxed);
                            }
                            code if key_matches(code, 'e') && !focus_log => {
                                let (Some(sd), Some(dd)) =
                                    (engine_workspace_dir.as_ref(), engine_dump_dir.as_ref())
                                else {
                                    toast_text = Some("Экспорт: нет путей рабочего каталога (внутренняя ошибка)".into());
                                    toast_success = false;
                                    toast_at = Some(Instant::now());
                                    continue;
                                };
                                if dd.as_os_str().is_empty() {
                                    toast_text = Some(
                                        "Экспорт: задайте LOCALVOX_LIGHT_TRANSCRIPT_DUMP_DIR".into(),
                                    );
                                    toast_success = false;
                                    toast_at = Some(Instant::now());
                                    continue;
                                }
                                match export_sorted_jsonl(sd, dd) {
                                    Ok((path, n)) => {
                                        let detail = format!("[dump] {n} строк → {}", path.display());
                                        toast_text = Some(format!("Экспорт: {n} строк → {}", path.display()));
                                        toast_success = true;
                                        toast_at = Some(Instant::now());
                                        log_lines.push(format_log_line(&StructuredLog {
                                            stage: "export".into(),
                                            source_id: 0,
                                            chunk_sec: n as f64,
                                            proc_sec: 0.0,
                                            detail,
                                            verbose_only: false,
                                        }));
                                        if log_lines.len() > MAX_LOG_LINES {
                                            log_lines.drain(..log_lines.len() - MAX_LOG_LINES);
                                        }
                                    }
                                    Err(e) => {
                                        toast_text = Some(format!("Экспорт: {e}"));
                                        toast_success = false;
                                        toast_at = Some(Instant::now());
                                    }
                                }
                            }
                            KeyCode::Tab | KeyCode::BackTab => focus_log = !focus_log,
                            code if code == KeyCode::Up || key_matches(code, 'k') => {
                                if focus_log {
                                    log_scroll = log_scroll.saturating_sub(1);
                                } else {
                                    t_follow_bottom = false;
                                    t_visual_scroll = t_visual_scroll.saturating_sub(1);
                                }
                            }
                            code if code == KeyCode::Down || key_matches(code, 'j') => {
                                if focus_log {
                                    log_scroll = log_scroll.saturating_add(1);
                                } else {
                                    t_visual_scroll =
                                        (t_visual_scroll + 1).min(t_scroll_max_hint);
                                    if t_visual_scroll >= t_scroll_max_hint {
                                        t_follow_bottom = true;
                                    }
                                }
                            }
                            KeyCode::PageUp => {
                                if focus_log {
                                    log_scroll = log_scroll.saturating_sub(10);
                                } else {
                                    t_follow_bottom = false;
                                    let step = last_inner_h.max(1);
                                    t_visual_scroll = t_visual_scroll.saturating_sub(step);
                                }
                            }
                            KeyCode::PageDown => {
                                if focus_log {
                                    log_scroll = log_scroll.saturating_add(10);
                                } else {
                                    let step = last_inner_h.max(1);
                                    t_visual_scroll =
                                        (t_visual_scroll + step).min(t_scroll_max_hint);
                                    if t_visual_scroll >= t_scroll_max_hint {
                                        t_follow_bottom = true;
                                    }
                                }
                            }
                            KeyCode::Home => {
                                if focus_log {
                                    log_scroll = 0;
                                } else {
                                    t_visual_scroll = 0;
                                    t_follow_bottom = false;
                                }
                            }
                            KeyCode::End => {
                                if focus_log {
                                    log_scroll = usize::MAX;
                                } else {
                                    t_follow_bottom = true;
                                    t_pending_follow = true;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    if mode == TuiMode::Settings {
                        let st = settings_state.as_mut().unwrap();
                        if let MouseEventKind::ScrollUp = mouse.kind {
                            let state = if st.focus == 0 {
                                &mut st.input_state
                            } else {
                                &mut st.output_state
                            };
                            let len = if st.focus == 0 {
                                st.input_devices.len()
                            } else {
                                1 + st.output_devices.len()
                            };
                            let i = state.selected().unwrap_or(0).saturating_sub(1);
                            state.select(Some(i.min(len.saturating_sub(1))));
                        } else if let MouseEventKind::ScrollDown = mouse.kind {
                            let state = if st.focus == 0 {
                                &mut st.input_state
                            } else {
                                &mut st.output_state
                            };
                            let len = if st.focus == 0 {
                                st.input_devices.len()
                            } else {
                                1 + st.output_devices.len()
                            };
                            let i = state.selected().unwrap_or(0).saturating_add(1);
                            state.select(Some(i.min(len.saturating_sub(1))));
                        }
                    } else if let MouseEventKind::ScrollUp = mouse.kind {
                        if focus_log {
                            log_scroll = log_scroll.saturating_sub(3);
                        } else {
                            t_follow_bottom = false;
                            t_visual_scroll = t_visual_scroll.saturating_sub(3);
                        }
                    } else if let MouseEventKind::ScrollDown = mouse.kind {
                        if focus_log {
                            log_scroll = log_scroll.saturating_add(3);
                        } else {
                            t_visual_scroll =
                                (t_visual_scroll + 3).min(t_scroll_max_hint);
                            if t_visual_scroll >= t_scroll_max_hint {
                                t_follow_bottom = true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        loop {
            match ui_rx.try_recv() {
                Ok(UiMsg::Transcript {
                    source_id,
                    text,
                    time,
                }) => {
                    if !t_suppress_until_clear {
                        let ts = time.unwrap_or_else(|| {
                            Local::now().format("%H:%M:%S").to_string()
                        });
                        transcript_rows.push(TranscriptRow {
                            ts,
                            source_id,
                            text,
                        });
                        if transcript_rows.len() > MAX_TRANSCRIPT_LINES {
                            let drop = transcript_rows.len() - MAX_TRANSCRIPT_LINES;
                            let w = last_inner_w.max(40);
                            let dropped_h: usize = transcript_rows
                                .iter()
                                .take(drop)
                                .map(|r| row_visual_height(r, w))
                                .sum();
                            transcript_rows.drain(..drop);
                            t_visual_scroll = t_visual_scroll.saturating_sub(dropped_h);
                        }
                        if t_follow_bottom {
                            t_pending_follow = true;
                        }
                    }
                }
                Ok(UiMsg::TranscriptHistory(rows)) => {
                    // Не зависит от t_suppress_until_clear: иначе гонка с [x] в первом кадре —
                    // история с диска отбрасывается до ClearTranscript, экран пустой после перезапуска/сброса.
                    transcript_rows = rows
                        .into_iter()
                        .map(|(ts, source_id, text)| TranscriptRow {
                            ts,
                            source_id,
                            text,
                        })
                        .collect();
                    t_follow_bottom = true;
                    t_pending_follow = true;
                    t_visual_scroll = 0;
                }
                Ok(UiMsg::ClearTranscript) => {
                    transcript_rows.clear();
                    t_visual_scroll = 0;
                    t_follow_bottom = true;
                    t_pending_follow = false;
                    t_suppress_until_clear = false;
                }
                Ok(UiMsg::Log(l)) => {
                    if l.verbose_only && !verbose {
                        continue;
                    }
                    log_lines.push(format_log_line(&l));
                    if log_lines.len() > MAX_LOG_LINES {
                        log_lines.drain(..log_lines.len() - MAX_LOG_LINES);
                    }
                }
                Ok(UiMsg::Status(s)) => status = s,
                Ok(UiMsg::QueuePending {
                    unprocessed_wavs,
                    unprocessed_mb,
                    workspace_total_mb,
                }) => {
                    queue_pending = Some((
                        unprocessed_wavs,
                        unprocessed_mb,
                        workspace_total_mb,
                    ));
                }
                Ok(UiMsg::WorkspacePaths {
                    workspace_dir,
                    dump_dir,
                }) => {
                    engine_workspace_dir = Some(workspace_dir);
                    engine_dump_dir = Some(dump_dir);
                }
                Ok(UiMsg::AudioLevel { source_id, level }) => {
                    if source_id == 0 {
                        audio_level = level;
                    } else {
                        audio_level2 = level;
                    }
                }
                Ok(UiMsg::EngineFatal { message }) => {
                    fatal_error = Some(message);
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => break,
            }
        }

        if !running.load(Ordering::Relaxed) && fatal_error.is_none() {
            break;
        }

        if last_draw.elapsed() < Duration::from_millis(33) {
            continue;
        }
        last_draw = Instant::now();

        terminal.draw(|f| {
            if mode == TuiMode::Settings {
                if let Some(ref mut st) = settings_state {
                    let area = f.area();
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(5), Constraint::Length(1)])
                        .split(area);
                    let main_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(chunks[0]);

                    let input_items: Vec<ListItem> = st
                        .input_devices
                        .iter()
                        .map(|(i, n)| ListItem::new(format!("[{i}] {n}")))
                        .collect();
                    let input_list = List::new(input_items)
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title(if st.focus == 0 {
                                    "Микрофон ◄"
                                } else {
                                    "Микрофон"
                                })
                                .border_style(Style::default().fg(if st.focus == 0 {
                                    Color::Yellow
                                } else {
                                    Color::Reset
                                })),
                        )
                        .highlight_style(Style::default().bg(Color::DarkGray))
                        .highlight_symbol(">> ");
                    f.render_stateful_widget(input_list, main_chunks[0], &mut st.input_state);

                    let output_items: Vec<ListItem> = std::iter::once(ListItem::new("— нет —"))
                        .chain(
                            st.output_devices
                                .iter()
                                .map(|(i, n)| ListItem::new(format!("[{i}] {n}"))),
                        )
                        .collect();
                    let output_list = List::new(output_items)
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title(if st.focus == 1 {
                                    "Loopback ◄"
                                } else {
                                    "Loopback"
                                })
                                .border_style(Style::default().fg(if st.focus == 1 {
                                    Color::Yellow
                                } else {
                                    Color::Reset
                                })),
                        )
                        .highlight_style(Style::default().bg(Color::DarkGray))
                        .highlight_symbol(">> ");
                    f.render_stateful_widget(output_list, main_chunks[1], &mut st.output_state);

                    f.render_widget(
                        Paragraph::new(
                            "Tab — переключить | ↑↓ / колёсико — выбор | S — сохранить | Esc — назад",
                        )
                        .style(Style::default().fg(Color::DarkGray)),
                        chunks[1],
                    );

                    if let (Some(ref msg), Some(saved_at)) = (&st.saved_msg, st.saved_at) {
                        if saved_at.elapsed() < Duration::from_secs(2) {
                            let a = f.area();
                            let popup = ratatui::layout::Rect {
                                x: a.width / 4,
                                y: a.height.saturating_sub(2),
                                width: a.width / 2,
                                height: 1,
                            };
                            f.render_widget(
                                Paragraph::new(msg.as_str())
                                    .block(Block::default().borders(Borders::ALL).title("Сохранено"))
                                    .style(Style::default().fg(Color::Green)),
                                popup,
                            );
                        }
                    }
                }
                return;
            }

            let area = f.area();
            let (q_n, q_um, q_sm) = queue_pending.unwrap_or((0, 0.0, 0.0));
            // Строка со статистикой после первого тика движка (раз в 1 с). Не прячем при пустой очереди:
            // там же «рабочая папка N MB»; иначе строка всплывала только после [x], когда все wav снова «без jsonl».
            let show_queue_line = queue_pending.is_some();
            // Длинная справка — только если терминал достаточно большой (после ресайза пересчитается).
            let show_status_help = area.height >= 28 && area.width >= 72;
            let rec_on = record_pcm.load(Ordering::Relaxed);
            let rec_icon = if rec_on { "● REC" } else { "○ STOP" };
            let rec_color = if rec_on { Color::Red } else { Color::DarkGray };
            let lb_on = device_config.loopback;
            let lb_icon = if lb_on { "●" } else { "○" };
            let lb_color = if lb_on { Color::Cyan } else { Color::DarkGray };
            let level_bars = (audio_level * 8.0) as usize;
            let level_str: String =
                "▮".repeat(level_bars.min(8)) + &"▯".repeat(8_usize.saturating_sub(level_bars));
            let level2_src = if lb_on { audio_level2 } else { 0.0 };
            let level2_bars = (level2_src * 8.0) as usize;
            let level2_str: String =
                "▮".repeat(level2_bars.min(8)) + &"▯".repeat(8_usize.saturating_sub(level2_bars));
            let line1 = Line::from(vec![
                Span::styled(rec_icon, Style::default().fg(rec_color)),
                Span::raw("  "),
                Span::styled(lb_icon, Style::default().fg(lb_color)),
                Span::raw(format!(
                    " loopback {}  ",
                    if lb_on { "вкл" } else { "выкл" }
                )),
                Span::raw("mic "),
                Span::styled(&level_str, Style::default().fg(Color::Green)),
                Span::raw("  sys "),
                Span::styled(&level2_str, Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled(status.as_str(), Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled("[r] record  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[x] сброс  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[e] экспорт  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[F2] устройства  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[q] выход", Style::default().fg(Color::DarkGray)),
            ]);
            let y = Style::default().fg(Color::Yellow);
            let line_queue_vals = Line::from(vec![
                Span::raw("Очередь WAV: "),
                Span::styled(format!("{q_n}"), y),
                Span::raw(" шт. · "),
                Span::styled(format!("{q_um:.1} MB"), y),
                Span::raw(" (ожидают ASR) · рабочая папка "),
                Span::styled(format!("{q_sm:.1} MB"), y),
            ]);
            let line_status_help = Line::from(vec![
                Span::styled(
                    "• 1-е число — wav без строки в transcript.jsonl  • 2-я MB — сумма размеров этих wav  • 3-я MB — все файлы в рабочем каталоге  ·  ",
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    "Tab — фокус панели  ↑↓kj  Pg  Home/End  мышь — прокрутка",
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            let mut status_lines: Vec<Line> = Vec::new();
            if let Some(ref fe) = fatal_error {
                status_lines.push(Line::from(vec![
                    Span::styled(
                        "ОШИБКА ",
                        Style::default()
                            .fg(Color::Red)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(fe.as_str(), Style::default().fg(Color::LightRed)),
                    Span::styled(" — нажмите q", Style::default().fg(Color::DarkGray)),
                ]));
            }
            status_lines.push(line1);
            if show_queue_line {
                status_lines.push(line_queue_vals);
            }
            if show_status_help {
                status_lines.push(line_status_help);
            }

            // Не использовать Constraint::Min для статуса — ratatui отдаёт «лишнюю» высоту первому Min,
            // внутри Paragraph остаются пустые строки, транскрипт сжимается. Считаем строки с переносами.
            const LOG_PANEL_H: u16 = 8;
            const TRANSCRIPT_MIN_H: u16 = 5;
            const STATUS_BORDER_H: u16 = 2;
            let inner_w_status = area.width.saturating_sub(2) as usize;
            let status_content_rows = status_wrapped_row_count(&status_lines, inner_w_status);
            let status_content_rows_u16 = u16::try_from(status_content_rows).unwrap_or(u16::MAX);
            let desired_status_h = status_content_rows_u16.saturating_add(STATUS_BORDER_H);

            let h = area.height;
            let max_status_h = h.saturating_sub(LOG_PANEL_H + TRANSCRIPT_MIN_H);
            let mut status_h = desired_status_h
                .min(max_status_h.max(STATUS_BORDER_H + 1))
                .max(STATUS_BORDER_H + 1);

            let mut transcript_h = h.saturating_sub(status_h + LOG_PANEL_H);
            if transcript_h < TRANSCRIPT_MIN_H {
                let deficit = TRANSCRIPT_MIN_H - transcript_h;
                status_h = status_h.saturating_sub(deficit);
                transcript_h = h.saturating_sub(status_h + LOG_PANEL_H);
            }
            status_h = status_h.max(STATUS_BORDER_H + 1);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(status_h),
                    Constraint::Length(transcript_h),
                    Constraint::Length(LOG_PANEL_H),
                ])
                .split(area);
            let status_border = if fatal_error.is_some() {
                Color::Red
            } else {
                Color::Reset
            };
            let status_title = if fatal_error.is_some() {
                "Статус — движок остановлен"
            } else {
                "Статус"
            };
            let status_widget = Paragraph::new(status_lines)
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(status_title)
                        .border_style(Style::default().fg(status_border)),
                );
            f.render_widget(status_widget, chunks[0]);

            let t_title = if !focus_log {
                "Транскрипт ◄"
            } else {
                "Транскрипт"
            };
            let t_border = if !focus_log {
                Color::Yellow
            } else {
                Color::Reset
            };
            let inner_w = chunks[1].width.saturating_sub(2) as usize;
            last_inner_w = inner_w.max(40);
            let inner_h = chunks[1].height.saturating_sub(2) as usize;
            last_inner_h = inner_h.max(1);

            let total_visual: usize = transcript_rows
                .iter()
                .map(|r| row_visual_height(r, last_inner_w))
                .sum();
            let max_v_scroll = total_visual.saturating_sub(inner_h);
            t_scroll_max_hint = max_v_scroll;
            if t_pending_follow && t_follow_bottom {
                t_visual_scroll = max_v_scroll;
                t_pending_follow = false;
            }
            t_visual_scroll = t_visual_scroll.min(max_v_scroll);

            let t_lines: Vec<Line> = transcript_rows
                .iter()
                .map(|row| {
                    let (prefix, color) = match row.source_id {
                        0 => ("mic ", Color::Green),
                        1 => ("sys ", Color::Cyan),
                        _ => ("", Color::White),
                    };
                    Line::from(vec![
                        Span::styled(
                            format!("{} ", row.ts),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(
                            prefix,
                            Style::default().fg(color).add_modifier(Modifier::DIM),
                        ),
                        Span::styled(row.text.as_str(), Style::default().fg(color)),
                    ])
                })
                .collect();
            let scroll_y = (t_visual_scroll.min(u16::MAX as usize)) as u16;
            let t_widget = Paragraph::new(t_lines)
                .wrap(Wrap { trim: false })
                .scroll((scroll_y, 0))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(t_title)
                        .border_style(Style::default().fg(t_border)),
                );
            f.render_widget(t_widget, chunks[1]);

            let log_title = match (focus_log, verbose) {
                (true, true) => "Debug ◄",
                (false, true) => "Debug",
                (true, false) => "Debug ◄ (--verbose — этапы)",
                (false, false) => "Debug (--verbose — этапы)",
            };
            let log_border = if focus_log {
                Color::Yellow
            } else {
                Color::Reset
            };
            let header = "time     stage    src  chunk    proc    detail";
            let log_inner = chunks[2].height.saturating_sub(2) as usize;
            let mut log_body: Vec<Line> = vec![Line::from(Span::styled(
                header,
                Style::default().fg(Color::DarkGray),
            ))];
            for line in &log_lines {
                log_body.push(Line::raw(line.as_str()));
            }
            let body_len = log_body.len().saturating_sub(1);
            let l_max_scroll = body_len.saturating_sub(log_inner.max(1));
            let l_off = log_scroll.min(l_max_scroll);
            let start = 1 + l_off;
            let end = (start + log_inner.max(1)).min(log_body.len());
            let log_visible: Vec<Line> = if log_body.is_empty() {
                vec![]
            } else {
                let mut out = vec![log_body[0].clone()];
                if start < end {
                    out.extend(log_body[start..end].iter().cloned());
                }
                out
            };
            let log_widget = Paragraph::new(log_visible)
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(log_title)
                        .border_style(Style::default().fg(log_border)),
                );
            f.render_widget(log_widget, chunks[2]);

            if let (Some(ref msg), Some(ref at)) = (&toast_text, &toast_at) {
                if at.elapsed() < Duration::from_secs(2) {
                    let a = f.area();
                    let popup = ratatui::layout::Rect {
                        x: a.width / 4,
                        y: a.height.saturating_sub(2),
                        width: a.width / 2,
                        height: 1,
                    };
                    let (title, fg) = if toast_success {
                        ("Сохранено", Color::Green)
                    } else {
                        ("Ошибка", Color::Red)
                    };
                    f.render_widget(
                        Paragraph::new(msg.as_str())
                            .block(Block::default().borders(Borders::ALL).title(title))
                            .style(Style::default().fg(fg)),
                        popup,
                    );
                }
            }
        })?;
    }

    execute!(terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    Ok(())
}

#[cfg(test)]
mod layout_tests {
    use super::{row_visual_height, status_wrapped_row_count, TranscriptRow};
    use ratatui::text::Line;

    #[test]
    fn status_wrapped_empty_is_one_row() {
        assert_eq!(status_wrapped_row_count(&[], 80), 1);
    }

    #[test]
    fn status_wrapped_narrow_width_splits_line() {
        let line = Line::from("abcdefghij");
        assert_eq!(status_wrapped_row_count(&[line], 4), 3);
    }

    #[test]
    fn row_visual_height_transcript_row() {
        let row = TranscriptRow {
            ts: "12:34:56".into(),
            source_id: 0,
            text: "abc".into(),
        };
        assert_eq!(row_visual_height(&row, 100), 1);
        let w = "12:34:56 ".len() + "mic ".len() + "abc".len();
        assert_eq!(row_visual_height(&row, w), 1);
        assert_eq!(row_visual_height(&row, w - 1), 2);
    }
}
