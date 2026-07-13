//! The lexicon — configurable dictionaries: names, abbreviations, numerals.
//!
//! These are DATA, not code, so they live in TOML and are extended by the user, like the
//! glossary and the prompt templates. The design is the same:
//!
//! * **the built-in values** are baked into the binary (`lexicon/*.toml`) — out of the
//!   box it works without a single file on disk;
//! * **the user directory** (`LOCALVOX_LEXICON_DIR`, `assets/lexicon` by default) is read
//!   on top and **extends** the built-in one rather than replacing it: colleagues' names,
//!   your own abbreviations, your own numeral forms.
//!
//! Merging rather than replacing is the sensible DEFAULT: otherwise, by adding one
//! colleague's name you would silently lose the other eighty and never find out.
//!
//! But a default is not a prison. A built-in dictionary can be **thrown out entirely** and
//! replaced with your own: `replace = ["names"]` wipes everything accumulated for that
//! kind before merging with your file. Otherwise the baked-in list would be a hard
//! coupling: our names do not suit you, and there is nowhere to escape them.
//!
//! A file may contain any set of keys — the ones that are there get read:
//! ```toml
//! replace = ["names"]                        # throw out ALL accumulated names
//! names = ["стас", "кассандра"]              # …and keep only these
//! abbreviations = ["сбп", "фот"]             # not facts → not checked
//! [forms]                                    # numerals in words
//! "полтора" = 1
//! ```
//!
//! The kinds for `replace`: `names`, `abbreviations`, `numerals`.
//! The files of the directory are read in alphabetical order — a `replace` in the file
//! `00-base.toml` will take effect before the additions from `10-team.toml`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

const BUILTIN_NAMES: &str = include_str!("../lexicon/names-ru.toml");
const BUILTIN_ABBREVIATIONS: &str = include_str!("../lexicon/abbreviations.toml");
const BUILTIN_NUMERALS: &str = include_str!("../lexicon/numerals-ru.toml");
/// The numerals of different languages live in ONE lexicon: the words do not collide
/// («five» does not get in the way of «пять»), so there is no point splitting it by
/// language.
const BUILTIN_NUMERALS_EN: &str = include_str!("../lexicon/numerals-en.toml");

/// The directory of user dictionaries.
pub fn user_dir() -> PathBuf {
    std::env::var_os("LOCALVOX_LEXICON_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("assets/lexicon"))
}

#[derive(Deserialize, Default)]
struct LexiconFile {
    /// The kinds of dictionary that this file REPLACES rather than extends:
    /// `names`, `abbreviations`, `numerals`. Whatever has been accumulated for those kinds
    /// is wiped before merging. Without this, the baked-in list would be a hard coupling.
    #[serde(default)]
    replace: Vec<String>,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    abbreviations: Vec<String>,
    /// Words that LOOK like a one but mean an article: «одна из задач», «one of the
    /// tasks». As part of a number («двадцать один») they work as usual.
    #[serde(default)]
    articles: Vec<String>,
    /// Connectors INSIDE a number: «one hundred **and** fifty». They do not break the group.
    #[serde(default)]
    connectors: Vec<String>,
    /// `[forms]` — a numeral in words → its value.
    #[serde(default)]
    forms: BTreeMap<String, u64>,
}

#[derive(Default, Clone)]
pub struct Lexicon {
    /// Personal names: they are checked as people — regardless of the position in the line
    /// and by a strict similarity rule.
    pub names: BTreeSet<String>,
    /// Abbreviations: they carry no facts and are not checked for grounding.
    pub abbreviations: BTreeSet<String>,
    /// Numerals in words → value («сто пятьдесят» → 100, 50).
    pub numeral_forms: BTreeMap<String, u64>,
    /// Article words that do not count as a number on their own («одна задача»).
    /// A FACT OF LANGUAGE, not a rule of the code: the parsing of numbers must not contain
    /// hardcoded Russian and English words — there are many languages.
    pub numeral_articles: BTreeSet<String>,
    /// Connectors inside a number («one hundred and fifty») — they do not break the group.
    pub numeral_connectors: BTreeSet<String>,
}

impl Lexicon {
    /// The built-in one only — without reading the disk.
    pub fn builtin() -> Self {
        let mut lex = Self::default();
        for text in [
            BUILTIN_NAMES,
            BUILTIN_ABBREVIATIONS,
            BUILTIN_NUMERALS,
            BUILTIN_NUMERALS_EN,
        ] {
            match toml::from_str::<LexiconFile>(text) {
                Ok(f) => lex.merge(f),
                // The baked-in files are checked by a test — you can only end up here by
                // breaking them at build time, and we must not keep quiet about that.
                Err(e) => tracing::error!("the built-in lexicon is corrupt: {e}"),
            }
        }
        lex
    }

    /// The built-in one + every `*.toml` from the directory (the directory may be missing —
    /// that is normal).
    pub fn load_dir(dir: &Path) -> Self {
        let mut lex = Self::builtin();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return lex;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
            .collect();
        files.sort(); // the reading order is deterministic
        for path in files {
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| toml::from_str::<LexiconFile>(&t).map_err(|e| e.to_string()))
            {
                Ok(f) => {
                    tracing::debug!("lexicon: {}", path.display());
                    lex.merge(f);
                }
                // A corrupt user file is a loud error, not a quiet skip: silently working
                // with half a dictionary is worse than crashing.
                Err(e) => tracing::error!("lexicon {}: {e}", path.display()),
            }
        }
        lex
    }

    fn merge(&mut self, f: LexiconFile) {
        // First wipe whatever the file declared replaceable — and only then merge.
        // Otherwise «replace» would be indistinguishable from «extend».
        for kind in &f.replace {
            match kind.to_lowercase().as_str() {
                "names" => self.names.clear(),
                "abbreviations" => self.abbreviations.clear(),
                "numerals" | "forms" => {
                    self.numeral_forms.clear();
                    self.numeral_articles.clear();
                    self.numeral_connectors.clear();
                }
                other => tracing::error!(
                    "lexicon: replace = «{other}» — there is no such kind \
                     (there are: names, abbreviations, numerals)"
                ),
            }
        }
        self.names
            .extend(f.names.into_iter().map(|s| s.to_lowercase()));
        self.abbreviations
            .extend(f.abbreviations.into_iter().map(|s| s.to_lowercase()));
        self.numeral_articles
            .extend(f.articles.into_iter().map(|s| s.to_lowercase()));
        self.numeral_connectors
            .extend(f.connectors.into_iter().map(|s| s.to_lowercase()));
        for (word, value) in f.forms {
            self.numeral_forms.insert(word.to_lowercase(), value);
        }
    }

    pub fn is_person_name(&self, word: &str) -> bool {
        self.names.contains(&word.to_lowercase())
    }

    /// Whether the names are empty. An empty dictionary of names is a DELIBERATE choice
    /// (`replace = ["names"]` without `names`), and the check must know that it has nothing
    /// to lean on: silently pretending that a language has no names is not allowed.
    pub fn has_names(&self) -> bool {
        !self.names.is_empty()
    }

    /// An abbreviation carries no facts: you cannot invent a person, a deadline or a
    /// quantity with one, while it does produce plenty of false alarms (UI, UX, HR, UAT).
    pub fn is_abbreviation(&self, word: &str) -> bool {
        self.abbreviations.contains(&word.to_lowercase())
    }
}

/// The process's active lexicon: the built-in one + the user directory.
/// It is read once — the dictionaries do not change on the fly.
pub fn active() -> &'static Lexicon {
    static ACTIVE: OnceLock<Lexicon> = OnceLock::new();
    ACTIVE.get_or_init(|| Lexicon::load_dir(&user_dir()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_lexicon_parses_and_is_not_empty() {
        let lex = Lexicon::builtin();
        assert!(lex.names.len() > 50, "names: {}", lex.names.len());
        assert!(lex.abbreviations.len() > 20);
        assert!(lex.numeral_forms.len() > 50);
        assert!(lex.is_person_name("Иван"));
        assert!(lex.is_abbreviation("UI"));
        assert_eq!(lex.numeral_forms.get("пятьдесят"), Some(&50));
        // «пять» and «пятьдесят» must not stick together — that is why the table is explicit
        assert_eq!(lex.numeral_forms.get("пять"), Some(&5));
    }

    /// …but a default is not a prison: a dictionary can be THROWN OUT and replaced with
    /// your own. A baked-in list without that possibility is precisely a hard coupling —
    /// our names do not suit you, and there is nowhere to escape them.
    #[test]
    fn a_user_file_can_replace_a_builtin_dictionary_not_only_extend_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("00-my-names.toml"),
            "replace = [\"names\"]\nnames = [\"Кассандра\"]\n",
        )
        .unwrap();
        let lex = Lexicon::load_dir(dir.path());

        assert!(lex.is_person_name("кассандра"));
        assert!(
            !lex.is_person_name("иван"),
            "the built-in names survived — replacing the dictionary is impossible"
        );
        // the neighbouring kinds of dictionary were not harmed
        assert!(lex.is_abbreviation("api"), "the abbreviations got wiped along the way");
        assert_eq!(
            lex.numeral_forms.get("сорок"),
            Some(&40),
            "the numerals got wiped along the way"
        );
    }

    /// A user file EXTENDS the built-in one rather than replacing it: by adding one
    /// colleague's name you must not silently lose the other eighty.
    #[test]
    fn user_files_extend_builtin_not_replace_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("my.toml"),
            "names = [\"Кассандра\"]\nabbreviations = [\"СБП\"]\n[forms]\n\"полтора\" = 1\n",
        )
        .unwrap();
        let lex = Lexicon::load_dir(dir.path());
        assert!(lex.is_person_name("кассандра"), "one's own name was not picked up");
        assert!(lex.is_person_name("иван"), "the built-in names were lost");
        assert!(lex.is_abbreviation("сбп"));
        assert!(lex.is_abbreviation("api"));
        assert_eq!(lex.numeral_forms.get("полтора"), Some(&1));
        assert_eq!(lex.numeral_forms.get("сорок"), Some(&40));
    }

    #[test]
    fn missing_dir_is_not_an_error() {
        let lex = Lexicon::load_dir(Path::new("нет-такого-каталога"));
        assert!(lex.is_person_name("иван"));
    }
}
