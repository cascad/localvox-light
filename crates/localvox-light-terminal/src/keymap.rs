//! Explicit command-position overrides for terminals that only deliver text.
use anyhow::{ensure, Context, Result};
use crossterm::event::{KeyCode, KeyEvent};
use serde::Deserialize;
use std::{collections::BTreeMap, io::Read, path::Path};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keymap {
    keys: BTreeMap<char, char>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileMap {
    version: u32,
    keys: BTreeMap<char, char>,
}

impl Keymap {
    pub fn load(path: &Path) -> Result<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .with_context(|| format!("Файл клавиш {}", path.display()))?
            .take(65537)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 65536, "Файл клавиш превышает 64 KiB");
        Self::from_json(&bytes).with_context(|| format!("Настройка клавиш {}", path.display()))
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let file: FileMap = serde_json::from_slice(bytes).context("Ожидается JSON version/keys")?;
        ensure!(file.version == 1, "Поддерживается только version: 1");
        ensure!(
            file.keys.len() <= 128,
            "Допустимо до 128 соответствий клавиш"
        );
        for (source, target) in &file.keys {
            ensure!(
                !source.is_control(),
                "Исходная клавиша должна быть печатным символом"
            );
            ensure!(
                target.is_ascii() && !target.is_ascii_control(),
                "Команда должна быть печатным ASCII символом"
            );
        }
        Ok(Self { keys: file.keys })
    }

    /// Only a command projection. The original event is retained for text editors.
    pub fn translate(&self, mut key: KeyEvent) -> Option<KeyEvent> {
        let KeyCode::Char(c) = key.code else {
            return None;
        };
        let mapped = self.keys.get(&c).copied().or_else(|| {
            if !c.is_uppercase() {
                return None;
            }
            let mut lower = c.to_lowercase();
            let first = lower.next()?;
            if lower.next().is_some() {
                return None;
            }
            self.keys.get(&first).copied().map(shifted_ascii)
        })?;
        key.code = KeyCode::Char(mapped);
        Some(key)
    }
}

pub(crate) fn shifted_ascii(c: char) -> char {
    const BASE: &str = "`1234567890-=[]\\;',./";
    const SHIFT: &str = "~!@#$%^&*()_+{}|:\"<>?";
    BASE.chars()
        .zip(SHIFT.chars())
        .find(|(base, _)| *base == c)
        .map(|(_, shifted)| shifted)
        .unwrap_or_else(|| c.to_ascii_uppercase())
}

#[cfg(test)]
mod tests;
