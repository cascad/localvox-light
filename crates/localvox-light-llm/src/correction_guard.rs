//! Bounded admission of line corrections, not a semantic judge. On uncertainty keep source.
use std::collections::{HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rejection {
    Empty,
    Literal,
    Number,
    Punctuation,
    Content,
    Limit,
}

struct Word {
    key: String,
    start: usize,
    end: usize,
}

fn key(text: &str) -> String {
    text.to_lowercase().replace('ё', "е")
}

fn words(text: &str) -> Vec<Word> {
    let chars: Vec<_> = text.char_indices().collect();
    let mut result = vec![];
    let mut start = None;
    for (i, &(at, c)) in chars.iter().enumerate() {
        let camel = start.is_some()
            && c.is_uppercase()
            && i > 0
            && (chars[i - 1].1.is_lowercase()
                || (chars[i - 1].1.is_uppercase()
                    && chars
                        .get(i + 1)
                        .is_some_and(|(_, next)| next.is_lowercase())));
        if !c.is_alphanumeric() || camel {
            if let Some(from) = start.take() {
                result.push(Word {
                    key: key(&text[from..at]),
                    start: from,
                    end: at,
                });
            }
        }
        if c.is_alphanumeric() && start.is_none() {
            start = Some(at);
        }
    }
    if let Some(from) = start {
        result.push(Word {
            key: key(&text[from..]),
            start: from,
            end: text.len(),
        });
    }
    result
}

fn critical(word: &str) -> bool {
    matches!(
        word,
        "не" | "ни"
            | "нет"
            | "нельзя"
            | "без"
            | "почти"
            | "пока"
            | "только"
            | "уже"
            | "еще"
            | "все"
            | "кроме"
            | "если"
            | "когда"
            | "либо"
            | "или"
            | "вряд"
            | "кажется"
            | "вероятно"
            | "возможно"
            | "наверное"
            | "наверно"
            | "можно"
            | "нужно"
            | "надо"
            | "должен"
            | "должны"
            | "следует"
            | "минус"
            | "плюс"
            | "более"
            | "менее"
            | "больше"
            | "меньше"
            | "короче"
            | "я"
            | "ты"
            | "мы"
            | "вы"
            | "он"
            | "она"
            | "они"
            | "оно"
            | "да"
            | "no"
            | "not"
            | "never"
            | "without"
            | "almost"
            | "only"
            | "yet"
            | "already"
            | "maybe"
            | "if"
            | "unless"
            | "except"
            | "must"
            | "should"
            | "could"
            | "i"
            | "you"
            | "we"
            | "they"
            | "he"
            | "she"
    )
}

fn literals(text: &str) -> Vec<(bool, String)> {
    let mut result = vec![];
    let mut opened = None;
    for (at, c) in text.char_indices() {
        if let Some((from, close)) = opened {
            if c == close {
                result.push((
                    close == '`',
                    if close == '`' {
                        text[from..at].to_owned()
                    } else {
                        text[from..at]
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .to_lowercase()
                    },
                ));
                opened = None;
            }
        } else {
            let close = match c {
                '«' => '»',
                '“' => '”',
                '‘' => '’',
                '"' | '`' => c,
                '\'' if !text[..at]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_alphanumeric) =>
                {
                    '\''
                }
                _ => continue,
            };
            opened = Some((at + c.len_utf8(), close));
        }
    }
    // Unclosed source quotes are ambiguous: keep their tail literally as well.
    if let Some((from, close)) = opened {
        result.push((
            close == '`',
            if close == '`' {
                text[from..].to_owned()
            } else {
                text[from..]
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase()
            },
        ));
    }
    result
}

fn technical(text: &str) -> Vec<&str> {
    text.split_whitespace()
        .map(|s| s.trim_matches(['"', '«', '»', '`', '(', ')', ',', '.', ';']))
        .filter(|s| {
            s.contains("--")
                || s.chars().any(|c| {
                    !c.is_alphanumeric()
                        && !c.is_whitespace()
                        && !matches!(
                            c,
                            '.' | ','
                                | ';'
                                | ':'
                                | '?'
                                | '!'
                                | '…'
                                | '-'
                                | '—'
                                | '–'
                                | '('
                                | ')'
                                | '«'
                                | '»'
                                | '"'
                                | '\''
                                | '“'
                                | '”'
                                | '‘'
                                | '’'
                        )
                })
        })
        .collect()
}

fn punctuation(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            ',' | ';' | ':' | '?' | '!' => Some(c),
            '«' | '»' | '“' | '”' | '"' => Some('"'),
            _ => None,
        })
        .collect()
}

fn negation_punctuation(text: &str, tokens: &[Word]) -> Vec<(String, String, String)> {
    let mut profile: Vec<_> = tokens
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            matches!(
                w.key.as_str(),
                "не" | "ни" | "нет" | "нельзя" | "no" | "not" | "never"
            )
        })
        .map(|(i, w)| {
            let left = i.checked_sub(1).map_or(0, |i| tokens[i].end);
            let right = tokens.get(i + 1).map_or(text.len(), |next| next.start);
            (
                w.key.clone(),
                if i > 0 && removable(text, tokens, i - 1) {
                    String::new()
                } else {
                    punctuation(&text[left..w.start])
                },
                punctuation(&text[w.end..right]),
            )
        })
        .collect();
    profile.dedup();
    profile
}

fn number_punctuation(text: &str) -> Vec<String> {
    let chars: Vec<_> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let dash = matches!(c, '-' | '−' | '–' | '—');
            let right = chars.get(i + 1).is_some_and(|c| c.is_numeric());
            let left = i.checked_sub(1).is_some_and(|j| chars[j].is_numeric());
            (right && (dash || (left && matches!(c, '.' | ',' | ':' | '/')))).then(|| {
                if dash {
                    "-".into()
                } else {
                    c.to_string()
                }
            })
        })
        .collect()
}

fn mood_changed(a: &str, b: &str) -> bool {
    // Known dangerous near-spellings. This is deliberately not a full morphology claim.
    let endings = [
        ("и", "ил"),
        ("и", "ила"),
        ("и", "или"),
        ("и", "ило"),
        ("и", "ит"),
        ("й", "л"),
        ("й", "ла"),
        ("й", "ли"),
        ("йте", "ли"),
        ("ь", "ил"),
        ("ьте", "или"),
        ("ите", "или"),
    ];
    for (x, y) in endings {
        for (left, right) in [(a, b), (b, a)] {
            if let (Some(l), Some(r)) = (left.strip_suffix(x), right.strip_suffix(y)) {
                if l == r && l.chars().count() >= 3 {
                    return true;
                }
            }
        }
    }
    a.is_ascii()
        && b.is_ascii()
        && ["d", "ed"]
            .iter()
            .any(|s| a.len() >= 4 && (b.strip_suffix(s) == Some(a) || a.strip_suffix(s) == Some(b)))
}

fn near(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (n, m) = (a.chars().count(), b.chars().count());
    if n.min(m) < 5
        || n.max(m) > 128
        || critical(a)
        || critical(b)
        || [
            "не", "ни", "а", "ан", "un", "in", "im", "ir", "il", "de", "dis", "non", "a",
        ]
        .iter()
        .any(|prefix| a.strip_prefix(prefix) == Some(b) || b.strip_prefix(prefix) == Some(a))
        || a.chars().chain(b.chars()).any(char::is_numeric)
        || mood_changed(a, b)
    {
        return false;
    }
    let budget = if n.min(m) >= 10 { 2 } else { 1 };
    n.abs_diff(m) <= budget && strsim::damerau_levenshtein(a, b) <= budget
}

fn removable(text: &str, tokens: &[Word], i: usize) -> bool {
    let w = &tokens[i];
    if i > 0 && (tokens[i - 1].key.starts_with("букв") || tokens[i - 1].key.starts_with("слов"))
    {
        return false;
    }
    let after = text[w.end..].trim_start().starts_with(',');
    let before = i == 0 || text[tokens[i - 1].end..w.start].contains(',');
    vocal_run(text, tokens, i)
        || matches!(
            w.key.as_str(),
            "э" | "ээ" | "эээ" | "ээээ" | "эм" | "эмм" | "ммм"
        )
        || (w.key == "ну" && (before || after))
        || (w.key == "короче" && before && after)
        || (i > 0
            && w.key == tokens[i - 1].key
            && !w.key.chars().any(char::is_numeric)
            && !matches!(w.key.as_str(), "не" | "ни" | "нет" | "not" | "no" | "never")
            && !matches!(w.key.as_str(), "очень" | "самый" | "very")
            && text[tokens[i - 1].end..w.start]
                .chars()
                .all(char::is_whitespace))
}

fn vocal_run(text: &str, tokens: &[Word], i: usize) -> bool {
    if !matches!(tokens[i].key.as_str(), "а" | "э" | "и" | "м") {
        return false;
    }
    let connected = |left: usize, right: usize| {
        let gap = &text[tokens[left].end..tokens[right].start];
        tokens[left].key == tokens[right].key
            && gap.contains('-')
            && gap.chars().all(|c| c == '-' || c.is_whitespace())
    };
    let mut left = i;
    let mut right = i;
    while left > 0 && connected(left - 1, left) {
        left -= 1;
    }
    while right + 1 < tokens.len() && connected(right, right + 1) {
        right += 1;
    }
    right > left
        && !(left > 0
            && (tokens[left - 1].key.starts_with("букв")
                || tokens[left - 1].key.starts_with("слов")))
        && !tokens[left..=right]
            .iter()
            .all(|w| text[w.start..w.end].chars().all(char::is_uppercase))
}

fn joined(a: &Word, b: &Word, c: &Word) -> bool {
    !critical(&a.key)
        && !critical(&b.key)
        && !critical(&c.key)
        && !a.key.chars().chain(b.key.chars()).any(char::is_numeric)
        && a.key.chars().count() >= 3
        && b.key.chars().count() >= 3
        && format!("{}{}", a.key, b.key) == c.key
}

fn group_repair(a: &[Word], b: &[Word]) -> bool {
    if a.len() < 2 || b.len() < 2 {
        return false;
    }
    // A long damaged anchor plus a short damaged word: retains legacy ASR repairs
    // such as the joined tail in «ориентируйсям сво», not arbitrary short substitutions.
    a[0].key != b[0].key
        && a[0].key.chars().count().min(b[0].key.chars().count()) >= 8
        && a[0]
            .key
            .chars()
            .zip(b[0].key.chars())
            .take_while(|(x, y)| x == y)
            .count()
            >= 8
        && strsim::levenshtein(&a[0].key, &b[0].key) == 1
        && (2..=3).contains(&a[1].key.chars().count())
        && (2..=3).contains(&b[1].key.chars().count())
        && strsim::levenshtein(&a[1].key, &b[1].key) <= 2
        && a.iter()
            .take(2)
            .chain(b.iter().take(2))
            .all(|w| !critical(&w.key) && !w.key.chars().any(char::is_numeric))
        && !mood_changed(&a[0].key, &b[0].key)
}

pub fn check(source: &str, candidate: &str) -> Result<(), Rejection> {
    if candidate.trim().is_empty() {
        return Err(Rejection::Empty);
    }
    if source == candidate {
        return Ok(());
    }
    if literals(source) != literals(candidate) || technical(source) != technical(candidate) {
        return Err(Rejection::Literal);
    }
    if number_punctuation(source) != number_punctuation(candidate)
        || localvox_light_core::num2words::parse_spelled_in_source(source)
            != localvox_light_core::num2words::parse_spelled_in_source(candidate)
    {
        return Err(Rejection::Number);
    }
    let (a, b) = (words(source), words(candidate));
    if ['?', '!']
        .iter()
        .any(|c| source.matches(*c).count() != candidate.matches(*c).count())
        || negation_punctuation(source, &a) != negation_punctuation(candidate, &b)
    {
        return Err(Rejection::Punctuation);
    }
    if a.iter().map(|w| &w.key).eq(b.iter().map(|w| &w.key)) {
        return Ok(());
    }
    if a.len().max(b.len()) > 512 {
        return Err(Rejection::Limit);
    }
    let mut queue = VecDeque::from([(0, 0)]);
    let mut seen = HashSet::new();
    while let Some((i, j)) = queue.pop_front() {
        if !seen.insert((i, j)) {
            continue;
        }
        if seen.len() > 32768 {
            return Err(Rejection::Limit);
        }
        if i == a.len() && j == b.len() {
            return Ok(());
        }
        if i < a.len() && removable(source, &a, i) {
            queue.push_back((i + 1, j));
        }
        if i + 1 < a.len()
            && matches!(
                (a[i].key.as_str(), a[i + 1].key.as_str()),
                ("как", "бы") | ("в", "общем")
            )
            && source[a[i + 1].end..].trim_start().starts_with(',')
            && (i == 0 || source[a[i - 1].end..a[i].start].contains(','))
        {
            queue.push_back((i + 2, j));
        }
        if i == a.len() || j == b.len() {
            continue;
        }
        if near(&a[i].key, &b[j].key) {
            queue.push_back((i + 1, j + 1));
        }
        if i + 1 < a.len() && joined(&a[i], &a[i + 1], &b[j]) {
            queue.push_back((i + 2, j + 1));
        }
        if j + 1 < b.len() && joined(&b[j], &b[j + 1], &a[i]) {
            queue.push_back((i + 1, j + 2));
        }
        if group_repair(&a[i..], &b[j..]) {
            queue.push_back((i + 2, j + 2));
        }
    }
    Err(Rejection::Content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn useful_corrections_are_not_replaced_by_a_blanket_noop() {
        for (source, fixed) in [
            ("Эээ, проект готов.", "Проект готов."),
            ("Эм, Анна отправила письмо.", "Анна отправила письмо."),
            ("А-а, проект готов, и-и-и всё.", "Проект готов, всё."),
            ("Эээ, не отправлено.", "Не отправлено."),
            (
                "Я, ну, понимаю, как это работает.",
                "Я понимаю, как это работает.",
            ),
            ("Проект проект готов.", "Проект готов."),
            ("Отчёт уже уже готов.", "Отчёт уже готов."),
            (
                "Когда тесты пройдут выпустим обновление.",
                "Когда тесты пройдут, выпустим обновление.",
            ),
            ("Проект гатов.", "Проект готов."),
            ("Команда завершили задачу.", "Команда завершила задачу."),
            ("Мы сохранилиотчёт.", "Мы сохранили отчёт."),
            ("ИНе вижу", "И не вижу"),
            ("ориентируйсям сво", "ориентируйся сам"),
            ("Ну, короче, проект готов.", "Проект готов."),
            ("Ну, как бы, проект готов.", "Проект готов."),
            ("Он сказал «эээ».", "Он сказал \"эээ\"."),
            ("Документ открыт", "Документ открыт."),
            (
                "Please recieve the document.",
                "Please receive the document.",
            ),
        ] {
            assert_eq!(check(source, fixed), Ok(()), "{source} => {fixed}");
        }
    }

    #[test]
    fn protected_content_and_technical_literals_cannot_disappear() {
        for (source, fixed) in [
            ("Мы согласовали только тестовый запуск.", "Мы согласовали запуск."),
            ("Письмо не отправлено.", "Письмо отправлено."),
            ("Этого недостаточно.", "Этого достаточно."),
            ("The change is unacceptable.", "The change is acceptable."),
            ("Оплата пока не подтверждена.", "Оплата не подтверждена."),
            ("Казнить нельзя, помиловать.", "Казнить, нельзя помиловать."),
            ("Он сказал 'эээ'.", "Он сказал ''."),
            ("Он сказал «все».", "Он сказал «всё»."),
            ("Буква э стоит в начале.", "Буква стоит в начале."),
            ("Буква а-а обсуждается.", "Буква обсуждается."),
            ("Буква а-а обсуждается.", "Буква а обсуждается."),
            ("Система И-И работает.", "Система работает."),
            ("А задача уже готова.", "Задача уже готова."),
            ("Это очень, очень важный шаг.", "Это очень важный шаг."),
            ("Выполни `cargo test --offline`.", "Выполни `cargo test`."),
            ("Выполни `cargo test`.", "Выполни `Cargo test`."),
            ("Выполни cargo test --offline.", "Выполни cargo test offline."),
            ("Версия C++20 работает с API v2.", "Версия C20 работает с API v2."),
            ("Пишем на C#.", "Пишем на C."),
            ("Ставка 15%.", "Ставка 15."),
            ("Вычисли 2*3.", "Вычисли 2 3."),
            ("Сумма $15.", "Сумма 15."),
            ("Счёт 2 2.", "Счёт 2."),
            ("Ставка 1,5 процента.", "Ставка 15 процентов."),
            ("Ставка 1,5 процента.", "Ставка 1.5 процента."),
            ("Температура -12 градусов.", "Температура 12 градусов."),
            ("Проверь запись.", "Проверил запись."),
            ("Создай файл.", "Создал файл."),
            ("Save the file.", "Saved the file."),
            ("Delete the file.", "Deleted the file."),
            ("Проект готов.", "Проект готов?"),
            ("Мы перенесём запуск только после", "Мы перенесём запуск только после подтверждения оплаты и успешной проверки резервной копии."),
            ("Открой документ.", "Открой документ. Открой документ."),
        ] {
            assert!(check(source, fixed).is_err(), "unsafe: {source} => {fixed}");
        }
    }

    #[test]
    fn large_ambiguous_changes_have_a_bound_but_punctuation_keeps_a_fast_path() {
        let source = "слово ".repeat(800);
        let formatted = format!("{}.", source.trim());
        assert!(check(&source, &formatted).is_ok());
        assert_eq!(check(&source, "другой текст"), Err(Rejection::Limit));
    }
}
