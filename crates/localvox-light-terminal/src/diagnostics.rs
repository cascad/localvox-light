//! Bounded client diagnostics. Reading the console acknowledges errors, not recovery.
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub source: String,
    pub operation: String,
    pub session: Option<String>,
    pub phase: Option<String>,
}
impl Context {
    fn clean(&self) -> Self {
        let clean = |value: &str| {
            localvox_light_client::safe_label(value)
                .chars()
                .take(1024)
                .collect::<String>()
        };
        Self {
            source: clean(&self.source),
            operation: clean(&self.operation),
            session: self.session.as_deref().map(clean),
            phase: self.phase.as_deref().map(clean),
        }
    }
    fn operation_key(&self) -> String {
        format!("{}\0{}\0{:?}", self.source, self.operation, self.session)
    }
    fn key(&self) -> String {
        format!(
            "{}\0{}\0{:?}\0{:?}",
            self.source, self.operation, self.session, self.phase
        )
    }
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: u64,
    pub at_ms: i64,
    pub context: Context,
    pub text: String,
    pub repeats: u32,
    pub unread: bool,
    pub expanded: bool,
    pub repeat_times: VecDeque<i64>,
}
#[derive(Default)]
pub struct Console {
    pub open: bool,
    pub scroll: u16,
    pub selected: usize,
    pub follow_selection: bool,
    pub entries: VecDeque<Entry>,
    active: HashMap<String, u64>,
    next_id: u64,
}
pub fn message(value: &str) -> String {
    localvox_light_client::safe_text(value)
        .lines()
        .map(|line| {
            let lower = line.to_lowercase();
            if let Some(at) = line
                .as_bytes()
                .windows(7)
                .position(|part| part.eq_ignore_ascii_case(b"bearer "))
            {
                format!("{}Bearer [скрыто]", &line[..at])
            } else if lower.contains("authorization:") {
                "[данные авторизации скрыты]".into()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(3000)
        .collect()
}
impl Console {
    pub fn unread(&self) -> usize {
        self.entries.iter().filter(|e| e.unread).count()
    }
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            for entry in &mut self.entries {
                entry.unread = false;
            }
            self.scroll = 0;
            self.selected = self.entries.len().saturating_sub(1);
            self.follow_selection = true;
        }
    }
    pub fn recover(&mut self, context: &Context) {
        self.active.remove(&context.clean().key());
    }
    pub fn recover_operation(&mut self, context: &Context) {
        let prefix = format!("{}\0", context.clean().operation_key());
        self.active.retain(|key, _| !key.starts_with(&prefix));
    }
    pub fn record_attempt(&mut self, context: Context, text: &str, at_ms: i64) {
        let key = context.clean().key();
        self.record(context, text, at_ms);
        if !self.open {
            if let Some(entry) = self
                .active
                .get(&key)
                .and_then(|id| self.entries.iter_mut().find(|entry| entry.id == *id))
            {
                entry.unread = true;
            }
        }
    }
    pub fn record(&mut self, context: Context, text: &str, at_ms: i64) {
        let context = context.clean();
        let at_ms = if at_ms > 0 {
            at_ms
        } else {
            chrono::Utc::now().timestamp_millis()
        };
        let key = context.key();
        let text = message(text);
        if let Some(entry) = self
            .active
            .get(&key)
            .and_then(|id| self.entries.iter_mut().find(|e| e.id == *id))
        {
            if entry.text == text && entry.context.phase == context.phase {
                entry.repeats = entry.repeats.saturating_add(1);
                entry.repeat_times.push_back(at_ms);
                while entry.repeat_times.len() > 32 {
                    entry.repeat_times.pop_front();
                }
                return;
            }
        }
        self.next_id += 1;
        self.active.insert(key, self.next_id);
        let tail = self.entries.is_empty() || self.selected == self.entries.len() - 1;
        self.entries.push_back(Entry {
            id: self.next_id,
            at_ms,
            context,
            text,
            repeats: 1,
            unread: !self.open,
            expanded: false,
            repeat_times: VecDeque::new(),
        });
        if tail {
            self.selected = self.entries.len() - 1;
        }
        while self.entries.len() > 128 {
            if let Some(old) = self.entries.pop_front() {
                self.active.retain(|_, id| *id != old.id);
                self.selected = self.selected.saturating_sub(1);
            }
        }
    }
    pub fn text(&self) -> String {
        self.entries
            .iter()
            .map(|e| {
                format!(
                    "{} · {} · {} · сессия {} · фаза {}\n{}{}",
                    chrono::DateTime::from_timestamp_millis(e.at_ms)
                        .map(|at| at.format("%H:%M:%SZ").to_string())
                        .unwrap_or_else(|| "время неизвестно".into()),
                    e.context.source,
                    e.context.operation,
                    e.context.session.as_deref().unwrap_or("—"),
                    e.context.phase.as_deref().unwrap_or("неизвестна"),
                    e.text,
                    if e.repeats > 1 {
                        let last = e
                            .repeat_times
                            .back()
                            .and_then(|at| {
                                chrono::DateTime::from_timestamp_millis(*at)
                                    .map(|at| at.format("%H:%M:%SZ").to_string())
                            })
                            .unwrap_or_else(|| "время неизвестно".into());
                        format!(
                            "\nДополнительных повторов: {} · последний {last}",
                            e.repeats - 1
                        )
                    } else {
                        String::new()
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reading_acknowledges_an_episode_but_recovery_allows_a_new_unread_error() {
        let mut console = Console::default();
        let context = Context {
            source: "API".into(),
            operation: "Загрузка сводки".into(),
            session: Some("original".into()),
            phase: Some("summary".into()),
        };
        console.record(context.clone(), "offline", 1);
        assert_eq!(console.unread(), 1);
        console.toggle();
        assert_eq!(console.unread(), 0);
        console.toggle();
        console.record(context.clone(), "offline", 2);
        assert_eq!(console.unread(), 0);
        assert_eq!(console.entries.len(), 1);
        assert_eq!(console.entries[0].repeats, 2);
        console.recover(&context);
        console.record(context, "offline", 3);
        assert_eq!(console.unread(), 1);
        assert_eq!(console.entries.len(), 2);
        assert!(console.text().contains("фаза summary"));
    }
    #[test]
    fn history_is_bounded_and_does_not_echo_authorization_or_terminal_controls() {
        let mut console = Console::default();
        for i in 0..150 {
            console.record(
                Context {
                    operation: i.to_string(),
                    ..Default::default()
                },
                "Authorization: Bearer synthetic-secret\n\x1b[2Jboom",
                i,
            );
        }
        assert_eq!(console.entries.len(), 128);
        assert!(!console.text().contains("synthetic-secret"));
        assert!(!console.text().contains('\x1b'));
    }
    #[test]
    fn explicit_attempt_rearms_but_polling_stays_read_and_phases_do_not_merge() {
        let mut console = Console::default();
        let mut context = Context {
            source: "API".into(),
            operation: "Обработка".into(),
            session: Some("session".into()),
            phase: Some("transcribe".into()),
        };
        for at in 1..=50 {
            console.record(context.clone(), "offline", at);
        }
        assert_eq!(console.entries.len(), 1);
        assert_eq!(console.entries[0].repeats, 50);
        assert_eq!(console.entries[0].repeat_times.len(), 32);
        assert_eq!(console.entries[0].repeat_times.back(), Some(&50));
        console.toggle();
        console.record_attempt(context.clone(), "offline", 51);
        assert_eq!(console.unread(), 0);
        console.toggle();
        console.record(context.clone(), "offline", 52);
        assert_eq!(console.unread(), 0);
        console.record_attempt(context.clone(), "offline", 53);
        assert_eq!(console.unread(), 1);
        context.phase = Some("summary".into());
        console.record(context.clone(), "offline", 54);
        assert_eq!(console.entries.len(), 2);
        assert_eq!(
            console.entries[0].context.phase.as_deref(),
            Some("transcribe")
        );
        console.recover_operation(&context);
        context.phase = Some("transcribe".into());
        console.record(context, "offline", 55);
        assert_eq!(console.entries.len(), 3);
    }
}
