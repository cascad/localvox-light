//! Voice module (F2, WP-B2): the interactive part of the fast lane.
//!
//! Listens to transcript lines **from the microphone only** (decision 2026-07-10),
//! recognizes trigger commands («запиши [в слот X | в X] <text>», plus custom
//! ones from `[commands]` in slots.toml), accumulates a note and writes it into a
//! slot (`localvox-light-integrations`) with a TTS confirmation.
//!
//! **End of a note** (F2 registry: «pause-silence and/or stop word»): after the
//! command the text accumulates across pauses over many phrases; the note is
//! closed when (a) a stop word was said as a separate phrase («всё», «конец»,
//! «стоп»…), (b) silence longer than `note_idle` (3.5 s by default), (c) a new
//! command was spoken (it closes the previous one), (d) dictation longer than
//! `note_max` (90 s). While dictation is in progress TTS stays silent (otherwise
//! it would talk over the recording) — the confirmation sounds on close; the
//! progress of the recording is visible in the TUI status line.
//!
//! Custom triggers live in slots.toml:
//! ```toml
//! [commands."запиши"]            # ordinary command: the slot comes from the phrase
//! [commands."запомни"]           # trigger with a hard-wired slot:
//! slot = "идеи"                  # «запомни купить кофе» → straight into «идеи»
//! ```
//! Without a `[commands]` section `LOCALVOX_LIGHT_VOICE_TRIGGER` («запиши») applies.
//!
//! Architecture: the engine calls [`TranscriptHook`] from the ASR worker → the
//! hook non-blockingly pushes the text into a channel → the worker thread drives
//! [`Brain`] (a pure state machine, covered by tests; timers go through `tick`)
//! and executes its [`Action`]s. Phrase ordering is guaranteed by ASR sharding
//! per source.

mod tts;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossbeam_channel::{unbounded, Sender};
use serde::Deserialize;

use localvox_light_core::events::{TranscriptHook, UiMsg};
use localvox_light_integrations::SlotRegistry;

pub use tts::Tts;

const MIC_SOURCE: u8 = 0;

// ─────────────────────────── config ───────────────────────────

pub struct VoiceConfig {
    /// Default trigger, used when slots.toml has no `[commands]` section.
    pub trigger: String,
    /// Whether to speak back what was recorded.
    pub confirm: bool,
    /// How long to wait for the first phrase after a command with no text.
    pub pending_timeout: Duration,
    /// End of a note by silence: this long without new phrases — we close it.
    pub note_idle: Duration,
    /// Hard cap on the duration of dictating a single note.
    pub note_max: Duration,
    /// Stop phrases: said as a SEPARATE phrase, closes the note (not part of the text).
    pub stop_words: Vec<String>,
    /// Retro mode «запиши это»: how many of the last seconds of the conversation to take.
    pub retro_window: Duration,
    /// Phrases that start a MEETING. Everything said after the phrase is its title.
    pub meeting_phrases: Vec<String>,
    /// Phrases that mean «take the link from the clipboard and transcribe it».
    ///
    /// The link is in the clipboard because that is where a link always is: it was just copied
    /// from a browser or a messenger. Making a person paste it into a field by hand is asking
    /// them to do with their hands what the machine can already see.
    pub link_phrases: Vec<String>,
    /// Working directory: the meeting marker is dropped there (the recording engine reads it).
    pub work_dir: PathBuf,
    pub tts: Tts,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            trigger: "запиши".into(),
            confirm: true,
            pending_timeout: Duration::from_secs(20),
            note_idle: Duration::from_millis(3500),
            note_max: Duration::from_secs(90),
            stop_words: ["всё", "все", "конец", "стоп", "хватит", "конец записи"]
                .into_iter()
                .map(String::from)
                .collect(),
            retro_window: Duration::from_secs(30),
            meeting_phrases: [
                "начни встречу",
                "начать встречу",
                "новая встреча",
                "запиши встречу",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            link_phrases: [
                "возьми ссылку",
                "возьми ссылку из буфера",
                "забери ссылку",
                "расшифруй ссылку",
                "ссылка из буфера",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            work_dir: PathBuf::from("localvox-audio"),
            tts: Tts::default(),
        }
    }
}

fn env_off(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .unwrap_or_default()
            .to_lowercase()
            .as_str(),
        "0" | "off" | "false" | "no"
    )
}

impl VoiceConfig {
    pub fn from_env() -> Self {
        let mut c = Self::default();
        if let Ok(d) = std::env::var("LOCALVOX_LIGHT_AUDIO_DIR") {
            if !d.trim().is_empty() {
                c.work_dir = PathBuf::from(d.trim());
            }
        }
        if let Ok(p) = std::env::var("LOCALVOX_LIGHT_VOICE_MEETING") {
            let phrases: Vec<String> = p
                .split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            if !phrases.is_empty() {
                c.meeting_phrases = phrases;
            }
        }
        if let Ok(t) = std::env::var("LOCALVOX_LIGHT_VOICE_TRIGGER") {
            if !t.trim().is_empty() {
                c.trigger = t.trim().to_lowercase();
            }
        }
        c.confirm = !env_off("LOCALVOX_LIGHT_VOICE_CONFIRM");
        if let Some(sec) = std::env::var("LOCALVOX_LIGHT_NOTE_IDLE_SEC")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
        {
            c.note_idle = Duration::from_secs_f64(sec.clamp(1.0, 60.0));
        }
        if let Some(sec) = std::env::var("LOCALVOX_LIGHT_RETRO_WINDOW_SEC")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
        {
            c.retro_window = Duration::from_secs_f64(sec.clamp(5.0, 300.0));
        }
        if let Ok(words) = std::env::var("LOCALVOX_LIGHT_NOTE_STOP_WORDS") {
            let parsed: Vec<String> = words
                .split(',')
                .map(|w| w.trim().to_lowercase())
                .filter(|w| !w.is_empty())
                .collect();
            if !parsed.is_empty() {
                c.stop_words = parsed;
            }
        }
        c.tts = Tts::from_env();
        c
    }
}

/// Trigger command: a phrase plus an optionally hard-wired slot.
#[derive(Clone)]
pub struct CommandDef {
    /// Trigger tokens («запомни», «отметь мысль»), lowercase.
    pub tokens: Vec<String>,
    /// Hard-wired slot: the address is not parsed out of the speech, the whole
    /// remainder is the note text.
    pub slot: Option<String>,
}

#[derive(Deserialize)]
struct CommandsFile {
    #[serde(default)]
    commands: BTreeMap<String, CommandEntry>,
}

#[derive(Deserialize)]
struct CommandEntry {
    #[serde(default)]
    slot: Option<String>,
}

/// Reads `[commands]` from slots.toml; if absent — a single command built from
/// `fallback_trigger`.
pub fn load_commands(slots_config: &Path, fallback_trigger: &str) -> Result<Vec<CommandDef>> {
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string(slots_config) {
        let parsed: CommandsFile =
            toml::from_str(&text).with_context(|| format!("parsing {}", slots_config.display()))?;
        for (phrase, entry) in parsed.commands {
            let tokens: Vec<String> = phrase
                .to_lowercase()
                .split_whitespace()
                .map(String::from)
                .collect();
            if tokens.is_empty() {
                continue;
            }
            out.push(CommandDef {
                tokens,
                slot: entry.slot,
            });
        }
    }
    if out.is_empty() {
        out.push(CommandDef {
            tokens: vec![fallback_trigger.to_lowercase()],
            slot: None,
        });
    }
    // long triggers first — greedy match
    out.sort_by_key(|c| std::cmp::Reverse(c.tokens.len()));
    Ok(out)
}

// ─────────────────────── grammar and state machine ───────────────────────

/// Slot address phrase as tokens + the slot name.
struct Matcher {
    phrase: Vec<String>,
    slot: String,
}

/// Builds the address phrases: «в слот <name>», «в <name>» and every alias as is.
/// A bare slot name inside free text is NOT matched — «запиши идеи проекта» must
/// go as text into the default slot, not into the slot «идеи».
fn build_matchers(registry: &SlotRegistry) -> Vec<Matcher> {
    let mut out = Vec::new();
    for slot in registry.slots() {
        let name_tokens: Vec<String> = slot
            .name
            .to_lowercase()
            .split_whitespace()
            .map(String::from)
            .collect();
        let mut phrases: Vec<Vec<String>> = vec![
            [vec!["в".into(), "слот".into()], name_tokens.clone()].concat(),
            [vec!["в".into()], name_tokens.clone()].concat(),
        ];
        for alias in &slot.aliases {
            let t: Vec<String> = alias
                .to_lowercase()
                .split_whitespace()
                .map(String::from)
                .collect();
            if !t.is_empty() {
                phrases.push(t);
            }
        }
        for phrase in phrases {
            out.push(Matcher {
                phrase,
                slot: slot.name.clone(),
            });
        }
    }
    // long phrases first — greedy match
    out.sort_by_key(|m| std::cmp::Reverse(m.phrase.len()));
    out
}

/// The text in the clipboard, if there is any. A clipboard we cannot read is not a failure of the
/// command — it is an empty clipboard as far as we are concerned, and the person hears exactly
/// that instead of silence.
fn clipboard_text() -> Option<String> {
    let text = arboard::Clipboard::new().ok()?.get_text().ok()?;
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[derive(Debug, PartialEq)]
pub enum Action {
    /// Write the text into a slot (None — the default slot).
    Write {
        slot: Option<String>,
        text: String,
    },
    Say(String),
    /// Recording progress indication for the UI (TUI status line).
    Status(String),
    /// «Начни встречу [title]»: close the current recording and start a new one —
    /// marked as a meeting.
    ///
    /// By voice this matters more than by button: a meeting starts when the person
    /// has already sat down at the table and is talking, not when their hands are
    /// free and the browser is open.
    Meeting { title: String },
    /// «Возьми ссылку»: take the link from the CLIPBOARD and transcribe it.
    ///
    /// The link is not dictated — it is copied. Nobody reads a URL out loud, and a recognizer
    /// would mangle it anyway. But it is already in the clipboard: that is where it lands the
    /// moment it is copied from a browser or a messenger. The voice only says «take it».
    Ingest,
}

struct Command {
    slot: Option<String>,
    text: String,
    /// Retro mode «запиши это [в X]»: take the last seconds of the conversation.
    retro: bool,
    /// «в слот X» with an unknown X — say so, instead of writing garbage into the default.
    unknown_slot: Option<String>,
}

/// An open note dictation.
struct Capture {
    slot: Option<String>,
    parts: Vec<String>,
    opened: Instant,
    last_activity: Instant,
}

/// A dictation in progress, in the Brain's own terms.
///
/// `Action::Status` already carries this — as a rendered LINE, for the TUI status bar. A line is
/// printf at a boundary: the screen cannot ask it which slot, and cannot draw the growing text
/// any way but ours. So the fact is exposed as a fact, and the rendering stays where it belongs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capturing {
    /// The slot AS IT WAS SAID. `None` — «the default one»; only the registry knows its name.
    pub slot: Option<String>,
    /// Everything dictated so far, glued across pauses.
    pub text: String,
}

pub struct Brain {
    commands: Vec<CommandDef>,
    matchers: Vec<Matcher>,
    confirm: bool,
    pending_timeout: Duration,
    note_idle: Duration,
    note_max: Duration,
    stop_words: Vec<String>,
    retro_window: Duration,
    meeting_phrases: Vec<String>,
    link_phrases: Vec<String>,
    capture: Option<Capture>,
    /// Ring of the latest conversation phrases (mic + loopback) for «запиши это».
    recent: std::collections::VecDeque<(Instant, String)>,
    /// The last phrase spoken by TTS — guards against self-capture through the microphone.
    last_spoken: String,
}

impl Brain {
    fn new(cfg: &VoiceConfig, registry: &SlotRegistry, commands: Vec<CommandDef>) -> Self {
        Self {
            commands,
            matchers: build_matchers(registry),
            confirm: cfg.confirm,
            pending_timeout: cfg.pending_timeout,
            note_idle: cfg.note_idle,
            note_max: cfg.note_max,
            stop_words: cfg.stop_words.iter().map(|w| normalized(w)).collect(),
            retro_window: cfg.retro_window,
            meeting_phrases: cfg
                .meeting_phrases
                .iter()
                .map(|p| p.trim().to_lowercase())
                .filter(|p| !p.is_empty())
                .collect(),
            link_phrases: cfg
                .link_phrases
                .iter()
                .map(|p| p.trim().to_lowercase())
                .filter(|p| !p.is_empty())
                .collect(),
            capture: None,
            recent: std::collections::VecDeque::new(),
            last_spoken: String::new(),
        }
    }

    /// A phrase from loopback (the other speakers): goes into the retro-context ring only.
    /// Our own TTS is kept out of it — it is audible in the system audio as well.
    pub fn on_sys_text(&mut self, text: &str, now: Instant) {
        let text = text.trim();
        if text.is_empty()
            || (!self.last_spoken.is_empty()
                && is_echo(&normalized(text), &normalized(&self.last_spoken)))
        {
            return;
        }
        self.push_recent(text, now);
    }

    fn push_recent(&mut self, text: &str, now: Instant) {
        self.recent.push_back((now, text.to_string()));
        let horizon = self.retro_window.max(Duration::from_secs(120));
        while let Some((t, _)) = self.recent.front() {
            if now.duration_since(*t) > horizon {
                self.recent.pop_front();
            } else {
                break;
            }
        }
    }

    /// Text of the last `retro_window` seconds of the conversation.
    fn recent_window_text(&self, now: Instant) -> String {
        self.recent
            .iter()
            .filter(|(t, _)| now.duration_since(*t) <= self.retro_window)
            .map(|(_, s)| s.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn parse_command(&self, text: &str) -> Option<Command> {
        let tokens: Vec<String> = text
            .to_lowercase()
            .split_whitespace()
            .map(String::from)
            .collect();
        let cmd = self
            .commands
            .iter()
            .find(|c| tokens.len() >= c.tokens.len() && tokens[..c.tokens.len()] == c.tokens[..])?;
        let rest = &tokens[cmd.tokens.len()..];

        // hard-wired slot: we do not parse an address out of the speech, all of it is
        // text; a leading «это» means retro («запомни это»)
        if let Some(slot) = &cmd.slot {
            let retro = rest.len() == 1 && rest[0] == "это";
            return Some(Command {
                slot: Some(slot.clone()),
                text: rest.join(" "),
                retro,
                unknown_slot: None,
            });
        }

        // «<trigger> это [address]» — retro, if after «это» there is nothing or a pure address
        if rest.first().map(String::as_str) == Some("это") {
            let after = &rest[1..];
            if after.is_empty() {
                return Some(Command {
                    slot: None,
                    text: String::new(),
                    retro: true,
                    unknown_slot: None,
                });
            }
            for m in &self.matchers {
                if after.len() == m.phrase.len()
                    && after
                        .iter()
                        .zip(&m.phrase)
                        .all(|(a, b)| token_matches(a, b))
                {
                    return Some(Command {
                        slot: Some(m.slot.clone()),
                        text: String::new(),
                        retro: true,
                        unknown_slot: None,
                    });
                }
            }
            // «это …text…» — an ordinary inline note that merely starts with the word «это»
        }

        for m in &self.matchers {
            if rest.len() >= m.phrase.len()
                && rest[..m.phrase.len()]
                    .iter()
                    .zip(&m.phrase)
                    .all(|(a, b)| token_matches(a, b))
            {
                return Some(Command {
                    slot: Some(m.slot.clone()),
                    text: rest[m.phrase.len()..].join(" "),
                    retro: rest[m.phrase.len()..] == ["это".to_string()],
                    unknown_slot: None,
                });
            }
        }
        // explicit form «в слот X» with an unknown X — do not swallow it silently
        if rest.len() >= 3 && rest[0] == "в" && rest[1] == "слот" {
            return Some(Command {
                slot: None,
                text: String::new(),
                retro: false,
                unknown_slot: Some(rest[2..].join(" ")),
            });
        }
        Some(Command {
            slot: None,
            text: rest.join(" "),
            retro: false,
            unknown_slot: None,
        })
    }

    /// Handle a phrase from the microphone; `now` is injected for the sake of tests.
    /// «Начни встречу [title]» is a separate command, not a note.
    ///
    /// We parse it BEFORE notes: otherwise the phrase «запиши встречу с Иваном» would
    /// go into a slot as the text «встречу с Иваном» — that is, the person asked to
    /// mark a meeting and got a line in a notepad instead, and would find out an hour
    /// later.
    fn parse_meeting(&self, text: &str) -> Option<String> {
        let lower = text.to_lowercase();
        let lower = lower.trim_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace());
        for phrase in &self.meeting_phrases {
            if let Some(rest) = lower.strip_prefix(phrase.as_str()) {
                let title = rest
                    .trim_start_matches(|c: char| !c.is_alphanumeric())
                    .trim();
                return Some(title.to_string());
            }
        }
        None
    }

    /// «Возьми ссылку» — the whole phrase, nothing after it. The link is not dictated: it is in
    /// the clipboard.
    fn is_link_command(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        let lower = lower.trim_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace());
        self.link_phrases.iter().any(|p| lower == p.as_str())
    }

    /// What is being dictated right now — a read of the Brain's own state, no timers, no effects.
    ///
    /// The worker publishes this so the screen can say «пишу в [идеи]: …» while it happens. Without
    /// it the module writes to a file on another disk in total silence, and «is it broken?» has no
    /// answer that does not involve a file manager.
    pub fn capturing(&self) -> Option<Capturing> {
        self.capture.as_ref().map(|c| Capturing {
            slot: c.slot.clone(),
            text: c.parts.join(" "),
        })
    }

    pub fn on_mic_text(&mut self, text: &str, now: Instant) -> Vec<Action> {
        let text = text.trim();
        if text.is_empty() {
            return vec![];
        }
        // Self-capture: the microphone heard our own TTS from the speakers. We filter
        // out an exact match, fragments (VAD cuts a phrase into pieces) and inexact
        // recognition (≥80% of tokens), otherwise «Готово…» closes the loop.
        if !self.last_spoken.is_empty() {
            if is_echo(&normalized(text), &normalized(&self.last_spoken)) {
                return vec![];
            }
            self.last_spoken.clear();
        }

        // Before notes: «возьми ссылку» must not become the text of a note.
        if self.is_link_command(text) {
            let mut actions = self.flush_capture();
            actions.push(Action::Ingest);
            return actions;
        }

        if let Some(title) = self.parse_meeting(text) {
            // An unfinished note is closed first: it is about something else.
            let mut actions = self.flush_capture();
            actions.push(Action::Meeting {
                title: title.clone(),
            });
            actions.extend(self.say(if title.is_empty() {
                "Встреча начата".into()
            } else {
                format!("Встреча начата: {title}")
            }));
            return actions;
        }

        if let Some(cmd) = self.parse_command(text) {
            if let Some(unknown) = cmd.unknown_slot {
                return self.say(format!("Не знаю слот {unknown}"));
            }
            // Retro mode: «запиши это [в идеи]» / «запиши в идеи это» — grab the last
            // retro_window seconds of the conversation (mic + the other speakers).
            if cmd.retro {
                let mut actions = self.flush_capture();
                let context = self.recent_window_text(now);
                if context.is_empty() {
                    actions
                        .extend(self.say("Нечего записывать: последние полминуты тишина".into()));
                    return actions;
                }
                let slot_label = cmd.slot.clone().unwrap_or_else(|| "по умолчанию".into());
                actions.push(Action::Write {
                    slot: cmd.slot,
                    text: context.clone(),
                });
                actions.push(Action::Status(format!(
                    "✓ записано в [{slot_label}] (ретро {} с): {}",
                    self.retro_window.as_secs(),
                    brief(&context)
                )));
                if self.confirm {
                    actions.extend(self.say("Готово, записал сказанное".into()));
                }
                return actions;
            }
            // a new command closes the previous dictation
            let mut actions = self.flush_capture();
            let inline = cmd.text.trim().to_string();
            let slot_label = cmd.slot.clone().unwrap_or_else(|| "…".into());
            self.capture = Some(Capture {
                slot: cmd.slot,
                parts: if inline.is_empty() {
                    vec![]
                } else {
                    vec![inline]
                },
                opened: now,
                last_activity: now,
            });
            if self.capture.as_ref().is_some_and(|c| c.parts.is_empty()) {
                actions.extend(self.say("Слушаю. Закончите словом всё или паузой".into()));
            }
            actions.push(Action::Status(format!(
                "● заметка [{slot_label}] — говорите; конец: «всё» или пауза"
            )));
            return actions;
        }

        self.push_recent(text, now);

        let Some(capture) = &mut self.capture else {
            return vec![];
        };

        // a stop word as a separate phrase — close, do not include it in the text
        if self.stop_words.contains(&normalized(text)) {
            return self.flush_capture();
        }
        // interjections and scraps («а», «угу») are not dragged into the note
        if is_filler(text) {
            return vec![];
        }
        capture.parts.push(text.to_string());
        capture.last_activity = now;
        let n = capture.parts.len();
        let slot_label = capture.slot.clone().unwrap_or_else(|| "…".into());
        vec![Action::Status(format!(
            "● заметка [{slot_label}]: {n} фраз — говорите; конец: «всё» или пауза"
        ))]
    }

    /// Timers: end by silence / by the maximum; called periodically by the worker.
    pub fn tick(&mut self, now: Instant) -> Vec<Action> {
        let Some(c) = &self.capture else {
            return vec![];
        };
        if c.parts.is_empty() {
            // waiting for the first phrase: a generous timeout, cancellation goes
            // silently into the status line
            if now.duration_since(c.opened) > self.pending_timeout {
                self.capture = None;
                return vec![Action::Status("заметка отменена: тишина".into())];
            }
            return vec![];
        }
        if now.duration_since(c.last_activity) > self.note_idle
            || now.duration_since(c.opened) > self.note_max
        {
            return self.flush_capture();
        }
        vec![]
    }

    /// Force-close the dictation (module shutdown).
    pub fn finish(&mut self) -> Vec<Action> {
        self.flush_capture()
    }

    fn flush_capture(&mut self) -> Vec<Action> {
        let Some(c) = self.capture.take() else {
            return vec![];
        };
        if c.parts.is_empty() {
            return vec![];
        }
        let text = c.parts.join(" ");
        let slot_label = c.slot.clone().unwrap_or_else(|| "по умолчанию".into());
        let mut actions = vec![
            Action::Write {
                slot: c.slot,
                text: text.clone(),
            },
            Action::Status(format!("✓ записано в [{slot_label}]: {}", brief(&text))),
        ];
        if self.confirm {
            let say = format!("Готово, записал: {}", brief(&text));
            actions.extend(self.say(say));
        }
        actions
    }

    fn say(&mut self, phrase: String) -> Vec<Action> {
        self.last_spoken = phrase.clone();
        vec![Action::Say(phrase)]
    }
}

fn brief(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().take(8).collect();
    let mut s = words.join(" ");
    if text.split_whitespace().count() > 8 {
        s.push('…');
    }
    s
}

/// Morphology-lite: «в идею/идеей» must land in the slot «идеи», «в работу» — in
/// «работа». A match is either exact or by a common prefix (a stem): for words of
/// 4 characters and up a prefix of length min(lengths)−2 is enough, but never
/// shorter than 3. Short function words («в», «слот») are compared exactly only.
fn token_matches(heard: &str, expected: &str) -> bool {
    if heard == expected {
        return true;
    }
    let (hn, en) = (heard.chars().count(), expected.chars().count());
    if hn < 4 || en < 4 {
        return false;
    }
    let need = std::cmp::max(3, std::cmp::min(hn, en).saturating_sub(2));
    heard.chars().take(need).eq(expected.chars().take(need))
}

/// Echo of our own TTS: equality, a fragment of the phrase, or a strong (≥80%)
/// token overlap with the last spoken phrase.
fn is_echo(incoming: &str, spoken: &str) -> bool {
    if incoming == spoken {
        return true;
    }
    if incoming.chars().count() >= 6 && spoken.contains(incoming) {
        return true;
    }
    let sp: std::collections::HashSet<&str> = spoken.split(' ').collect();
    let toks: Vec<&str> = incoming.split(' ').collect();
    if toks.len() < 3 {
        return false;
    }
    let hits = toks.iter().filter(|t| sp.contains(*t)).count();
    hits * 5 >= toks.len() * 4
}

/// Interjections/scraps that Vosk likes to answer sighs with.
fn is_filler(text: &str) -> bool {
    let toks: Vec<&str> = text.split_whitespace().collect();
    toks.len() <= 2
        && toks.iter().all(|t| {
            matches!(
                *t,
                "а" | "э" | "ээ" | "м" | "мм" | "ну" | "угу" | "ага" | "да" | "так"
            )
        })
}

fn normalized(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ─────────────────────────── module runner ───────────────────────────

/// Hook for the engine + the worker thread handle: after stopping the engine main
/// MUST drop the hook and join the thread — the queue and the open dictation are
/// written out.
pub type VoiceHandle = (TranscriptHook, std::thread::JoinHandle<()>);

/// Starts the voice module from the env config. `None` — the module is off (no
/// slots.toml or LOCALVOX_LIGHT_VOICE=off); the string is a status line for the UI.
/// `ui` is the TUI channel for live recording indication (None in headless).
pub fn spawn_from_env(ui: Option<Sender<UiMsg>>) -> Result<Option<(VoiceHandle, String)>> {
    if env_off("LOCALVOX_LIGHT_VOICE") {
        tracing::info!("voice module is off (LOCALVOX_LIGHT_VOICE=off)");
        return Ok(None);
    }
    let slots_path = SlotRegistry::default_config_path(None);
    if !slots_path.exists() {
        tracing::info!(
            "voice module inactive: no {} (see slots.example.toml)",
            slots_path.display()
        );
        return Ok(None);
    }
    let registry = SlotRegistry::load(&slots_path)
        .with_context(|| format!("slot config {}", slots_path.display()))?;
    let cfg = VoiceConfig::from_env();
    let commands = load_commands(&slots_path, &cfg.trigger)?;
    let summary = format!(
        "triggers [{}] · slots: {} · TTS {}",
        commands
            .iter()
            .map(|c| c.tokens.join(" "))
            .collect::<Vec<_>>()
            .join(", "),
        registry.names().join(", "),
        cfg.tts.describe(),
    );
    tracing::info!("voice module: {summary}");
    Ok(Some((
        spawn(cfg, registry, commands, ui, summary.clone()),
        summary,
    )))
}

/// Assembles the module from ready-made parts (for tests and non-standard wiring).
///
/// `detail` is what the screen shows to answer «работает ли модуль вообще» — the triggers, the
/// slots and the TTS in one line. It is passed in rather than rebuilt here because the caller has
/// already composed it for the log, and two renderings of one fact drift.
pub fn spawn(
    cfg: VoiceConfig,
    registry: SlotRegistry,
    commands: Vec<CommandDef>,
    ui: Option<Sender<UiMsg>>,
    detail: String,
) -> VoiceHandle {
    let (tx, rx) = unbounded::<(u8, String)>();
    let mut brain = Brain::new(&cfg, &registry, commands);
    let tts = cfg.tts;
    let work_dir = cfg.work_dir.clone();

    let handle = std::thread::Builder::new()
        .name("voice".into())
        .spawn(move || {
            let run_actions = |actions: Vec<Action>,
                               brain_last: &mut String,
                               status: &mut localvox_light_core::voice_note::VoiceStatus| {
                for action in actions {
                    match action {
                        Action::Write { slot, text } => {
                            let target = match &slot {
                                Some(name) => registry.resolve(name),
                                None => registry.default_slot(),
                            };
                            // THE RECEIPT. «Когда закончило писаться» is answered here and only
                            // here — a failure included, because a note that vanished silently is
                            // the worst outcome available: the person walks away believing it was
                            // saved. The screen shows whichever of the two actually happened.
                            let at = localvox_light_core::versions::now_rfc3339();
                            match target {
                                Some(s) => match s.write_note(&text) {
                                    Ok(dest) => {
                                        tracing::info!("voice → [{}] {dest}", s.name);
                                        *status = localvox_light_core::voice_note::VoiceStatus {
                                            last: Some(localvox_light_core::voice_note::Written {
                                                slot: s.name.clone(),
                                                text: text.clone(),
                                                dest,
                                                at,
                                                error: None,
                                            }),
                                            ..status.clone()
                                        };
                                    }
                                    Err(e) => {
                                        tracing::error!("voice: write failed: {e:#}");
                                        *status = localvox_light_core::voice_note::VoiceStatus {
                                            last: Some(localvox_light_core::voice_note::Written {
                                                slot: s.name.clone(),
                                                text: text.clone(),
                                                dest: String::new(),
                                                at,
                                                error: Some(format!("{e:#}")),
                                            }),
                                            ..status.clone()
                                        };
                                    }
                                },
                                None => {
                                    tracing::error!("voice: slot not found: {slot:?}");
                                    *status = localvox_light_core::voice_note::VoiceStatus {
                                        last: Some(localvox_light_core::voice_note::Written {
                                            slot: slot.clone().unwrap_or_default(),
                                            text: text.clone(),
                                            dest: String::new(),
                                            at,
                                            error: Some("такого слота нет".into()),
                                        }),
                                        ..status.clone()
                                    };
                                }
                            }
                        }
                        Action::Meeting { title } => {
                            // We ask the recording engine to START RECORDING: nothing reaches
                            // the disk before this, and the pre-roll ring is what makes the
                            // command safe — the minutes before the words "record this" go
                            // into the session too. Through a file marker, because the engine
                            // lives in another thread (if not another process), and a file is
                            // the cheapest way to shout across.
                            match localvox_light_core::jobs::request_record_start(&work_dir, &title)
                            {
                                Ok(()) => tracing::info!("voice → recording: «{title}»"),
                                Err(e) => {
                                    tracing::error!("voice: could not start the recording: {e}")
                                }
                            }
                        }
                        Action::Ingest => {
                            // The link is taken from the CLIPBOARD, not from the speech: nobody
                            // reads a URL out loud, and a recognizer would mangle it if they did.
                            // But it is already copied — that is where a link lives the moment it
                            // is worth transcribing.
                            //
                            // We answer out loud in every case. A voice command that silently does
                            // nothing when the clipboard holds a phrase instead of a link is
                            // indistinguishable from a command that was not heard.
                            let phrase = match clipboard_text() {
                                None => "Буфер обмена пуст".to_string(),
                                Some(text) => {
                                    match localvox_light_core::ingest::from_url(&work_dir, &text) {
                                        Ok(session) => {
                                            tracing::info!("voice → ingest: {session}");
                                            "Взял ссылку, качаю".to_string()
                                        }
                                        Err(e) => {
                                            tracing::warn!("voice: ingest failed: {e:#}");
                                            "В буфере не ссылка".to_string()
                                        }
                                    }
                                }
                            };
                            *brain_last = phrase.clone();
                            tts.speak(&phrase);
                        }
                        Action::Say(phrase) => {
                            *brain_last = phrase.clone();
                            tts.speak(&phrase);
                        }
                        Action::Status(line) => {
                            if let Some(ref ui) = ui {
                                let _ = ui.send(UiMsg::Status(line));
                            }
                        }
                    }
                }
            };
            // Publish what is being dictated, for whoever draws the screen. ONE place, driven by
            // the Brain's state rather than sprinkled over the five branches that emit
            // `Action::Status` — a state published from five sites goes stale at the sixth.
            //
            // The slot is RESOLVED here, because this is where the registry is: the Brain hears
            // «в идеи» or nothing at all, and «nothing at all» has a name only the registry knows.
            // The name is the whole point — «идея» and «заметка» is exactly the distinction the
            // person is looking at the screen to make.
            // The slot is RESOLVED here, because this is where the registry is: the Brain hears
            // «в идеи» or nothing at all, and «nothing at all» has a name only the registry knows.
            let capturing_now = |brain: &Brain| {
                brain.capturing().map(|c| {
                    let slot = c
                        .slot
                        .as_deref()
                        .and_then(|n| registry.resolve(n))
                        .or_else(|| registry.default_slot())
                        .map(|s| s.name.clone())
                        // An unknown slot was named: the Brain will say so out loud and write
                        // nothing. Show what was HEARD — «пишу в [ретро песня]» is how a person
                        // finds out the recognizer misheard their slot.
                        .or_else(|| c.slot.clone())
                        .unwrap_or_default();
                    localvox_light_core::voice_note::Capturing { slot, text: c.text }
                })
            };

            // «Работает ли вообще» is answered BEFORE anyone says a word — that is the whole point
            // of publishing at startup. A module that only appears on screen mid-phrase is
            // indistinguishable from a dead one for as long as nobody speaks.
            let mut status = localvox_light_core::voice_note::VoiceStatus {
                active: true,
                detail,
                capturing: None,
                last: None,
            };
            localvox_light_core::voice_note::publish(&work_dir, &status);
            let mut published = status.clone();

            let mut last_said = String::new();
            loop {
                match rx.recv_timeout(Duration::from_millis(300)) {
                    Ok((source_id, text)) => {
                        let actions = if source_id == MIC_SOURCE {
                            brain.on_mic_text(&text, Instant::now())
                        } else {
                            brain.on_sys_text(&text, Instant::now());
                            vec![]
                        };
                        run_actions(actions, &mut last_said, &mut status);
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        let actions = brain.tick(Instant::now());
                        run_actions(actions, &mut last_said, &mut status);
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        // the hook was dropped: write out the open dictation and exit
                        let actions = brain.finish();
                        run_actions(actions, &mut last_said, &mut status);
                        break;
                    }
                }
                status.capturing = capturing_now(&brain);
                // Only on change: the loop turns three times a second, and rewriting an unchanged
                // file that often is pure wear for nothing.
                if status != published {
                    localvox_light_core::voice_note::publish(&work_dir, &status);
                    published = status.clone();
                }
            }
            // The module is gone — not merely idle. The marker must not outlive the thread that
            // owns it, or the screen will claim a listener that is not there.
            localvox_light_core::voice_note::clear(&work_dir);
            tracing::debug!("voice module stopped");
        })
        .expect("spawn voice worker");

    (hook_for(tx), handle)
}

fn hook_for(tx: Sender<(u8, String)>) -> TranscriptHook {
    // both sources: mic — commands and the ring, loopback — the retro-context ring
    Arc::new(move |source_id: u8, text: &str| {
        let _ = tx.try_send((source_id, text.to_string()));
    })
}

// ─────────────────────────── tests ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn registry(dir: &Path) -> SlotRegistry {
        let cfg = format!(
            "[slots.\"идеи\"]\naliases = [\"в идейки\"]\npath = \"{0}/ideas.md\"\ntemplate = \"- {{{{text}}}}\"\ndefault = true\n\n[slots.\"ретро песни\"]\npath = \"{0}/retro.md\"\n",
            dir.display().to_string().replace('\\', "/")
        );
        let p = dir.join("slots.toml");
        fs::write(&p, cfg).unwrap();
        SlotRegistry::load(&p).unwrap()
    }

    fn default_commands() -> Vec<CommandDef> {
        vec![CommandDef {
            tokens: vec!["запиши".into()],
            slot: None,
        }]
    }

    fn brain(dir: &Path) -> Brain {
        Brain::new(&VoiceConfig::default(), &registry(dir), default_commands())
    }

    fn now() -> Instant {
        Instant::now()
    }

    fn meeting_of(actions: &[Action]) -> Option<&str> {
        actions.iter().find_map(|a| match a {
            Action::Meeting { title } => Some(title.as_str()),
            _ => None,
        })
    }

    /// «Возьми ссылку» — the link is not dictated, it is in the clipboard. Nobody reads a URL out
    /// loud, and a recognizer would mangle it if they tried.
    #[test]
    fn a_link_is_taken_from_the_clipboard_by_voice() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());

        assert_eq!(b.on_mic_text("Возьми ссылку", now()), vec![Action::Ingest]);
        assert_eq!(
            b.on_mic_text("возьми ссылку из буфера", now()),
            vec![Action::Ingest]
        );
    }

    /// And it must not swallow a NOTE that merely mentions a link: «запиши, что ссылку прислал
    /// Иван» is a note, not a command. Only the bare phrase is the command.
    #[test]
    fn a_note_about_a_link_is_still_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());

        let a = b.on_mic_text("Запиши, что ссылку пришлёт Иван", now());
        assert!(
            !a.contains(&Action::Ingest),
            "a note about a link was taken for the command: {a:?}"
        );
    }

    /// Marking a meeting by voice matters more than by button: it starts when the
    /// person has already sat down at the table and is talking, not when their hands
    /// are free and the browser is open.
    #[test]
    fn a_meeting_can_be_started_by_voice_with_a_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());

        let a = b.on_mic_text("Начни встречу планёрка по миграции", now());
        assert_eq!(meeting_of(&a), Some("планёрка по миграции"));

        // Without a title it is still a meeting.
        let a = b.on_mic_text("Новая встреча", now());
        assert_eq!(meeting_of(&a), Some(""));
    }

    /// TRAP: «запиши встречу» starts with the note trigger («запиши»). Without
    /// parsing the meeting FIRST, the person would ask to mark a meeting and get a
    /// notepad line with the text «встречу» — and would find out an hour later.
    #[test]
    fn starting_a_meeting_is_not_filed_away_as_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());

        let a = b.on_mic_text("Запиши встречу с Иваном", now());
        assert_eq!(meeting_of(&a), Some("с иваном"));
        assert!(
            !a.iter().any(|x| matches!(x, Action::Write { .. })),
            "the meeting was filed away into the notepad as a note"
        );

        // And an ordinary note does not turn into a meeting.
        let a = b.on_mic_text("Запиши купить молоко", now());
        assert!(meeting_of(&a).is_none(), "a note was taken for a meeting");
    }

    /// WHAT THE SCREEN SHOWS WHILE YOU DICTATE.
    ///
    /// The owner could not tell a working voice module from a dead one: a note is written in
    /// silence into a file on another disk, and the daemon runs the module with no channel to any
    /// screen. This is the fact the indicator is built on — it must appear with the command, grow
    /// with every phrase, and be GONE the moment the note is closed. A pill that outlives the
    /// dictation claims a recording that is not happening.
    #[test]
    fn the_screen_can_see_the_dictation_from_its_first_word_to_its_last() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        assert_eq!(b.capturing(), None, "nothing is being dictated yet");

        b.on_mic_text("запиши в идеи попробовать сортформер", now());
        let c = b.capturing().expect("the command opened a dictation the screen cannot see");
        assert_eq!(c.slot.as_deref(), Some("идеи"));
        assert_eq!(c.text, "попробовать сортформер");

        // It grows across pauses — that growth IS the proof it is still listening.
        b.on_mic_text("и померить на живой записи", now());
        assert_eq!(
            b.capturing().unwrap().text,
            "попробовать сортформер и померить на живой записи"
        );

        b.on_mic_text("всё", now());
        assert_eq!(b.capturing(), None, "the pill outlived the note it was about");
    }

    /// «запиши купить кофе» — no slot said. The Brain honestly reports `None`, meaning «the default
    /// one»: only the registry knows its name, and inventing one here would put the wrong word on
    /// the screen.
    #[test]
    fn a_note_with_no_slot_said_reports_no_slot_rather_than_guessing() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        b.on_mic_text("запиши купить кофе", now());
        let c = b.capturing().unwrap();
        assert_eq!(c.slot, None);
        assert_eq!(c.text, "купить кофе");
    }

    /// The first Write in the action list.
    fn first_write(actions: &[Action]) -> Option<(&Option<String>, &str)> {
        actions.iter().find_map(|a| match a {
            Action::Write { slot, text } => Some((slot, text.as_str())),
            _ => None,
        })
    }

    #[test]
    fn inline_command_closes_by_stop_word() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        let a1 = b.on_mic_text("запиши в идеи попробовать сортформер", now());
        assert!(first_write(&a1).is_none(), "the note is still open: {a1:?}");
        assert!(matches!(&a1[0], Action::Status(s) if s.contains("[идеи]")));
        let a2 = b.on_mic_text("всё", now());
        let (slot, text) = first_write(&a2).expect("the stop word closes the note");
        assert_eq!(slot.as_deref(), Some("идеи"));
        assert_eq!(text, "попробовать сортформер");
        assert!(a2
            .iter()
            .any(|a| matches!(a, Action::Say(s) if s.starts_with("Готово"))));
        assert!(a2
            .iter()
            .any(|a| matches!(a, Action::Status(s) if s.starts_with("✓"))));
    }

    #[test]
    fn multi_phrase_note_accumulates_and_closes_on_idle_tick() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        let t0 = now();
        b.on_mic_text("запиши в идеи проверить квен", t0);
        b.on_mic_text("и ещё сравнить с гптосс", t0 + Duration::from_secs(2));
        // silence shorter than note_idle — the note lives on
        assert!(b.tick(t0 + Duration::from_secs(4)).is_empty());
        // silence longer than note_idle after the last phrase — close
        let done = b.tick(t0 + Duration::from_secs(7));
        let (_, text) = first_write(&done).expect("idle closes the note");
        assert_eq!(text, "проверить квен и ещё сравнить с гптосс");
    }

    #[test]
    fn new_command_closes_previous_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        b.on_mic_text("запиши в идеи первая мысль", now());
        let a = b.on_mic_text("запиши в слот ретро песни владимирский централ", now());
        // the previous note is written out when the new one opens
        let (slot, text) = first_write(&a).expect("a new command closes the old one");
        assert_eq!(slot.as_deref(), Some("идеи"));
        assert_eq!(text, "первая мысль");
        // and the new one is closed by its own stop word
        let a2 = b.on_mic_text("конец", now());
        let (slot2, text2) = first_write(&a2).unwrap();
        assert_eq!(slot2.as_deref(), Some("ретро песни"));
        assert_eq!(text2, "владимирский централ");
    }

    #[test]
    fn morphology_lite_matches_inflected_slot_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        b.on_mic_text("запиши в идею купить кофе", now());
        let done = b.on_mic_text("всё", now());
        let (slot, text) = first_write(&done).unwrap();
        assert_eq!(slot.as_deref(), Some("идеи"));
        assert_eq!(text, "купить кофе");
        // and «в день» does not look like «идеи» — it goes as text into the default slot
        b.on_mic_text("запиши в день", now());
        let done = b.on_mic_text("хватит", now());
        let (slot, text) = first_write(&done).unwrap();
        assert!(slot.is_none());
        assert_eq!(text, "в день");
    }

    #[test]
    fn alias_and_bare_name_rules_hold() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        b.on_mic_text("запиши в идейки про кэш", now());
        let done = b.on_mic_text("всё", now());
        let (slot, _) = first_write(&done).unwrap();
        assert_eq!(slot.as_deref(), Some("идеи"));
        // a bare slot name is not an address
        b.on_mic_text("запиши идеи проекта на четверг", now());
        let done = b.on_mic_text("всё", now());
        let (slot, text) = first_write(&done).unwrap();
        assert!(slot.is_none());
        assert_eq!(text, "идеи проекта на четверг");
    }

    #[test]
    fn unknown_explicit_slot_gets_feedback_not_garbage_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        let a = b.on_mic_text("запиши в слот покупки хлеб и молоко", now());
        assert!(matches!(&a[0], Action::Say(s) if s.contains("Не знаю слот")));
        assert!(first_write(&a).is_none());
    }

    #[test]
    fn empty_command_waits_skips_fillers_and_expires() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        let t0 = now();
        let a1 = b.on_mic_text("запиши в идеи", t0);
        assert!(a1
            .iter()
            .any(|a| matches!(a, Action::Say(s) if s.contains("Слушаю"))));
        // an interjection does not count as a phrase
        assert!(b
            .on_mic_text("ну э", t0 + Duration::from_secs(1))
            .is_empty());
        // without a first phrase — cancelled by pending_timeout
        let cancel = b.tick(t0 + Duration::from_secs(30));
        assert!(first_write(&cancel).is_none());
        assert!(matches!(&cancel[0], Action::Status(s) if s.contains("отменена")));
    }

    #[test]
    fn own_tts_echo_including_fragments_and_fuzzy() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        b.on_mic_text("запиши в идеи", now()); // → Say("Слушаю. Закончите…")
        assert!(b
            .on_mic_text("слушаю закончите словом всё или паузой", now())
            .is_empty());
        assert!(b.on_mic_text("словом всё или паузой", now()).is_empty());
        // a real phrase does make it into the note
        b.on_mic_text("настоящая заметка", now());
        let done = b.on_mic_text("стоп", now());
        let (_, text) = first_write(&done).unwrap();
        assert_eq!(text, "настоящая заметка");
    }

    #[test]
    fn confirmation_echo_does_not_restart_command_loop() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        b.on_mic_text("запиши в идеи купить молоко", now());
        let done = b.on_mic_text("всё", now());
        assert!(done
            .iter()
            .any(|a| matches!(a, Action::Say(s) if s.starts_with("Готово"))));
        // the echo of the confirmation (even with recognition errors) does not open a new note
        let echo = b.on_mic_text("готово записал купить молоко", now());
        assert!(echo.is_empty(), "{echo:?}");
    }

    #[test]
    fn custom_commands_with_fixed_slot_and_multiword_activator() {
        let dir = tempfile::tempdir().unwrap();
        let commands = vec![
            CommandDef {
                tokens: vec!["отметь".into(), "мысль".into()],
                slot: Some("ретро песни".into()),
            },
            CommandDef {
                tokens: vec!["запомни".into()],
                slot: Some("идеи".into()),
            },
        ];
        let mut b = Brain::new(&VoiceConfig::default(), &registry(dir.path()), commands);
        b.on_mic_text("запомни в идеи не лезь", now());
        let done = b.on_mic_text("всё", now());
        let (slot, text) = first_write(&done).unwrap();
        assert_eq!(slot.as_deref(), Some("идеи"));
        assert_eq!(text, "в идеи не лезь"); // hard-wired slot: «в идеи» is part of the text
        b.on_mic_text("отметь мысль хорошая песня", now());
        let done = b.on_mic_text("всё", now());
        let (slot, _) = first_write(&done).unwrap();
        assert_eq!(slot.as_deref(), Some("ретро песни"));
        // a trigger that was not declared does not work
        assert!(b.on_mic_text("запиши что-нибудь", now()).is_empty());
    }

    #[test]
    fn retro_captures_recent_conversation_from_both_sources() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        let t0 = now();
        b.on_sys_text("коллега говорит важную вещь", t0);
        b.on_mic_text("а я отвечаю и уточняю", t0 + Duration::from_secs(2));
        // stale phrases outside the window do not get in
        let cmd_time = t0 + Duration::from_secs(4);
        let a = b.on_mic_text("запиши это в идеи", cmd_time);
        let (slot, text) = first_write(&a).expect("retro writes right away");
        assert_eq!(slot.as_deref(), Some("идеи"));
        assert_eq!(text, "коллега говорит важную вещь а я отвечаю и уточняю");
        assert!(a
            .iter()
            .any(|x| matches!(x, Action::Status(s) if s.contains("ретро"))));
    }

    #[test]
    fn retro_respects_window_and_excludes_old_phrases() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        let t0 = now();
        b.on_mic_text("очень старая фраза", t0);
        b.on_mic_text("свежая фраза", t0 + Duration::from_secs(50));
        // 30 s window: the old one (50 s before the command) misses it
        let a = b.on_mic_text("запиши это", t0 + Duration::from_secs(55));
        let (_, text) = first_write(&a).unwrap();
        assert_eq!(text, "свежая фраза");
    }

    #[test]
    fn retro_on_empty_window_says_nothing_to_save() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        let a = b.on_mic_text("запиши это", now());
        assert!(first_write(&a).is_none());
        assert!(a
            .iter()
            .any(|x| matches!(x, Action::Say(s) if s.contains("Нечего"))));
    }

    #[test]
    fn sys_source_never_triggers_commands() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = brain(dir.path());
        b.on_sys_text("запиши в идеи взломанная команда из колонок", now());
        // no note is open; and retro sees this only as context
        assert!(b.tick(now() + Duration::from_secs(10)).is_empty());
    }

    #[test]
    fn load_commands_from_toml_and_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("slots.toml");
        fs::write(
            &p,
            "[slots.\"идеи\"]\npath = \"x.md\"\n\n[commands.\"запиши\"]\n\n[commands.\"запомни\"]\nslot = \"идеи\"\n",
        )
        .unwrap();
        let cmds = load_commands(&p, "запиши").unwrap();
        assert_eq!(cmds.len(), 2);
        assert!(cmds
            .iter()
            .any(|c| c.tokens == ["запомни"] && c.slot.as_deref() == Some("идеи")));
        let p2 = dir.path().join("slots2.toml");
        fs::write(&p2, "[slots.\"идеи\"]\npath = \"x.md\"\n").unwrap();
        let cmds = load_commands(&p2, "вокс").unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].tokens, ["вокс"]);
    }

    #[test]
    fn e2e_hook_flushes_open_note_on_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let reg = registry(dir.path());
        let cfg = VoiceConfig {
            confirm: false,
            tts: Tts::Off,
            ..VoiceConfig::default()
        };
        let (hook, handle) = spawn(cfg, reg, default_commands(), None, "тест".into());
        hook(1, "запиши в идеи это loopback его игнорируем");
        hook(0, "запиши в идеи заметка через хук");
        hook(0, "и её продолжение");
        // there was no stop word: the note is open; dropping the hook must write it out
        drop(hook);
        handle.join().unwrap();
        let body = fs::read_to_string(dir.path().join("ideas.md")).unwrap();
        assert_eq!(body, "- заметка через хук и её продолжение\n");
    }
}
