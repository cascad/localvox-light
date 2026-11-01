//! Bounded client history and pure projection into operation groups.
use localvox_light_client::{safe_text, DisplayCommand, JournalEntry, JournalPage};
use std::collections::{HashMap, HashSet};

const MAX_EVENTS: usize = 5000;
const MAX_BYTES: usize = 4 * 1024 * 1024;
#[derive(Default)]
pub struct Journal {
    pub entries: Vec<JournalEntry>,
    pub after: String,
    pub before: Option<String>,
    pub has_more: bool,
    pub truncated: bool,
    pub discarded: usize,
    pub dropped_verbose: u64,
    pub follow: bool,
    pub unseen: usize,
    pub expanded: HashSet<String>,
    pub tail_gap: bool,
    pub filter: String,
    pub reset_notice: bool,
    received: HashSet<String>,
}
#[derive(Clone, Default)]
pub struct Group {
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub elapsed_sec: Option<f64>,
    pub id: String,
    pub run: String,
    pub stage: String,
    pub label: String,
    pub command: Option<DisplayCommand>,
    pub ended: bool,
    pub status: String,
    pub at: String,
    pub details: Vec<JournalEntry>,
}
impl Journal {
    pub fn new() -> Self {
        Self {
            follow: true,
            ..Default::default()
        }
    }
    pub fn merge(&mut self, page: JournalPage, prepend: bool) {
        // A rotated file returns its current tail even to a backwards request.
        // Its continuation must replace the old tail cursor as well.
        let prepend = prepend && !page.reset;
        if page.reset {
            self.entries.clear();
            self.expanded.clear();
            self.before = None;
            self.received.clear();
            self.tail_gap = false;
            self.after.clear();
            self.truncated = false;
            self.unseen = 0;
            self.reset_notice = true;
        }
        if self.after.is_empty() || page.reset {
            self.before = page.truncated.then(|| page.previous_cursor.clone());
        }
        if prepend {
            self.before = page.truncated.then(|| page.previous_cursor.clone());
        } else {
            self.after = page.next_cursor.clone();
        }
        let added = page
            .events
            .iter()
            .filter(|e| !self.received.contains(&e.id))
            .count();
        if !self.follow && !prepend && !page.reset {
            self.unseen = self.unseen.saturating_add(added);
        }
        for e in &page.events {
            self.received.insert(e.id.clone());
        }
        if self.received.len() > MAX_EVENTS * 2 {
            self.received = self
                .entries
                .iter()
                .chain(&page.events)
                .map(|e| e.id.clone())
                .collect();
        }
        self.has_more = page.has_more;
        self.truncated |= page.truncated || page.discarded > 0 || page.dropped_verbose > 0;
        self.discarded = page.discarded;
        self.dropped_verbose = page.dropped_verbose;
        if self.tail_gap && !prepend && !page.reset {
            return;
        }
        let old = self.entries.clone();
        let combined = if prepend {
            page.events
                .into_iter()
                .chain(std::mem::take(&mut self.entries))
                .collect::<Vec<_>>()
        } else {
            self.entries
                .drain(..)
                .chain(page.events)
                .collect::<Vec<_>>()
        };
        let mut seen = HashSet::new();
        self.entries = combined
            .into_iter()
            .filter(|e| seen.insert(e.id.clone()))
            .collect();
        let mut bytes = 0;
        let mut count = 0;
        let ordered: Box<dyn Iterator<Item = &JournalEntry>> = if prepend {
            Box::new(self.entries.iter())
        } else {
            Box::new(self.entries.iter().rev())
        };
        for e in ordered.take(MAX_EVENTS) {
            let size = serde_json::to_vec(e).map(|v| v.len()).unwrap_or(MAX_BYTES);
            if bytes + size > MAX_BYTES {
                break;
            }
            bytes += size;
            count += 1;
        }
        if count < self.entries.len() {
            self.truncated = true;
            if prepend {
                self.entries.truncate(count);
                self.tail_gap = true;
            } else if !self.follow {
                self.entries = old;
                self.tail_gap = true;
            } else {
                self.entries.drain(..self.entries.len() - count);
                self.before = self.entries.first().map(|e| e.id.clone());
            }
        }
        let active = self
            .groups()
            .into_iter()
            .map(|g| g.id)
            .collect::<HashSet<_>>();
        self.expanded.retain(|id| active.contains(id));
    }
    pub fn groups(&self) -> Vec<Group> {
        let mut indices = HashMap::new();
        let mut groups: Vec<Group> = vec![];
        for e in &self.entries {
            let id = format!(
                "{}/{}",
                e.run,
                e.operation
                    .as_deref()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("stage:{}", e.stage))
            );
            let index = *indices.entry(id.clone()).or_insert_with(|| {
                groups.push(Group {
                    id,
                    run: e.run.clone(),
                    stage: safe_text(&e.stage),
                    label: safe_text(&e.label),
                    ..Default::default()
                });
                groups.len() - 1
            });
            let g = &mut groups[index];
            if let Some(command) = &e.command {
                g.command = Some(command.clone());
            }
            if e.kind == "operation_start" {
                g.label = safe_text(&e.label);
                g.status = "running".into();
                g.started_at = Some(e.at.clone());
            }
            if e.kind == "operation_end" {
                g.ended = true;
                g.ended_at = Some(e.at.clone());
                g.elapsed_sec = e.elapsed_sec;
            }
            if let Some(status) = &e.status {
                g.status = status.clone();
            }
            if e.text.is_some() && e.kind != "operation_end" {
                g.details.push(e.clone());
            }
            g.at = e.at.clone();
        }
        groups
    }
    pub fn visible_groups(&self) -> Vec<Group> {
        let filter = self.filter.to_lowercase();
        self.groups()
            .into_iter()
            .filter(|g| {
                filter.is_empty()
                    || format!(
                        "{} {} {}",
                        g.label,
                        g.stage,
                        g.details
                            .iter()
                            .filter_map(|e| e.text.as_deref())
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                    .to_lowercase()
                    .contains(&filter)
            })
            .collect()
    }
    pub fn toggle(&mut self, index: usize) {
        if let Some(g) = self.visible_groups().get(index) {
            if !self.expanded.remove(&g.id) {
                self.expanded.insert(g.id.clone());
            }
        }
    }
}
impl Group {
    pub fn state(&self) -> &str {
        if self.ended
            && !matches!(
                self.status.as_str(),
                "failed" | "invalid" | "done" | "cancelled"
            )
        {
            "ended"
        } else {
            &self.status
        }
    }
    pub fn hidden_lines(&self) -> usize {
        self.details
            .iter()
            .filter(|e| !matches!(e.status.as_deref(), Some("failed" | "invalid")))
            .map(|e| e.text.as_deref().unwrap_or("").split('\n').count())
            .sum()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_is_idempotent_and_reset_drops_old_runs() {
        let mut j = Journal::new();
        let e = JournalEntry {
            id: "a".into(),
            ..Default::default()
        };
        j.merge(
            JournalPage {
                events: vec![e.clone()],
                ..Default::default()
            },
            false,
        );
        j.merge(
            JournalPage {
                events: vec![e],
                ..Default::default()
            },
            false,
        );
        assert_eq!(j.entries.len(), 1);
        j.merge(
            JournalPage {
                reset: true,
                ..Default::default()
            },
            false,
        );
        assert!(j.entries.is_empty());
    }
    #[test]
    fn reset_during_backwards_read_replaces_both_cursors_and_old_notices() {
        let mut j = Journal::new();
        j.after = "old-tail".into();
        j.before = Some("old-head".into());
        j.truncated = true;
        j.unseen = 100;
        j.follow = false;
        j.merge(
            JournalPage {
                reset: true,
                next_cursor: "new-tail".into(),
                events: vec![JournalEntry {
                    id: "new".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            true,
        );
        assert_eq!(j.after, "new-tail");
        assert!(j.before.is_none());
        assert_eq!(j.unseen, 0);
        assert!(!j.truncated && !j.tail_gap);
        assert!(j.reset_notice);
        assert_eq!(j.entries[0].id, "new");
    }
    #[test]
    fn exactly_35_lines_are_independently_collapsible() {
        let mut j = Journal::new();
        j.entries = (0..35)
            .map(|i| JournalEntry {
                id: i.to_string(),
                run: "r".into(),
                operation: Some("o".into()),
                text: Some(format!("line{i}")),
                ..Default::default()
            })
            .collect();
        assert_eq!(j.groups()[0].hidden_lines(), 35);
        j.toggle(0);
        assert!(j.expanded.contains("r/o"));
        j.toggle(0);
        assert!(!j.expanded.contains("r/o"));
    }
    #[test]
    fn ending_an_operation_does_not_accept_an_artifact() {
        let mut j = Journal::new();
        j.entries = vec![
            JournalEntry {
                kind: "operation_start".into(),
                operation: Some("o".into()),
                ..Default::default()
            },
            JournalEntry {
                kind: "operation_end".into(),
                operation: Some("o".into()),
                ..Default::default()
            },
        ];
        assert_eq!(j.groups()[0].state(), "ended");
        j.entries.last_mut().unwrap().status = Some("cancelled".into());
        assert_eq!(j.groups()[0].state(), "cancelled");
    }
    #[test]
    fn paused_reader_survives_eviction_and_replay_is_not_new() {
        let mut j = Journal::new();
        let page = JournalPage {
            events: (0..5000)
                .map(|i| JournalEntry {
                    id: i.to_string(),
                    run: "r".into(),
                    operation: Some("o".into()),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        j.merge(page.clone(), false);
        j.follow = false;
        j.expanded.insert("r/o".into());
        j.merge(page, false);
        assert_eq!(j.unseen, 0);
        j.merge(
            JournalPage {
                events: vec![JournalEntry {
                    id: "5000".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            false,
        );
        assert!(j.tail_gap);
        assert_eq!(j.entries[0].id, "0");
        assert_eq!(j.unseen, 1);
        assert!(j.expanded.contains("r/o"));
    }
    #[test]
    fn earlier_page_survives_a_full_cache() {
        let mut j = Journal::new();
        j.merge(
            JournalPage {
                events: (100..5100)
                    .map(|i| JournalEntry {
                        id: i.to_string(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
            false,
        );
        j.follow = false;
        j.merge(
            JournalPage {
                events: (0..100)
                    .map(|i| JournalEntry {
                        id: i.to_string(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
            true,
        );
        assert_eq!(j.entries[0].id, "0");
        assert_eq!(j.entries.last().unwrap().id, "4999");
        assert!(j.tail_gap);
    }
    #[test]
    fn byte_budget_is_independent_of_the_item_limit() {
        let mut j = Journal::new();
        j.merge(
            JournalPage {
                events: (0..1500)
                    .map(|i| JournalEntry {
                        id: i.to_string(),
                        text: Some("ж".repeat(3000)),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
            false,
        );
        assert!(j.entries.len() < 1500);
        assert!(
            j.entries
                .iter()
                .map(|e| serde_json::to_vec(e).unwrap().len())
                .sum::<usize>()
                <= MAX_BYTES
        );
    }
}
