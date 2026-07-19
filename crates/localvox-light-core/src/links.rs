//! A link the human gave us — validated and stripped of the junk that travels with it.
//!
//! A link copied from a browser or a messenger drags along tracking parameters (`utm_*`, `si`,
//! `feature`) and, on YouTube, the position in a playlist. They matter to nobody here, but they
//! do harm: two copies of the SAME video differ by a `si=` and become two sessions — the archive
//! grows duplicates of one lecture, and search returns both.
//!
//! Cleaning is deliberately dumb: strip the known junk, keep everything else. Guessing which of
//! someone's parameters is "unimportant" is how a link stops opening what it opened.

/// Junk parameters: analytics and the copy's provenance. `t` (the timestamp) and `list` (the
/// playlist) are NOT here — they change what the link points at.
const JUNK: [&str; 9] = [
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "si",       // YouTube: "who shared it with you"
    "feature",  // YouTube: from where it was opened
    "pp",       // YouTube: player parameters of the sharer
    "fbclid",   // Facebook click id
];

/// Is this a link at all — and if so, its cleaned form.
///
/// Only http/https: a `file://` from the clipboard would be an invitation to read any file on
/// the machine through an interface that is open on the LAN.
pub fn clean(raw: &str) -> Option<String> {
    let s = raw.trim();
    if !(s.starts_with("http://") || s.starts_with("https://")) {
        return None;
    }
    // No spaces and no newlines: a "link" with a space in it is a sentence, and it is not going
    // into a command line.
    if s.chars().any(char::is_whitespace) {
        return None;
    }
    let (base, query) = match s.split_once('?') {
        None => return Some(s.to_string()),
        Some((b, q)) => (b, q),
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter(|pair| {
            let key = pair.split_once('=').map(|(k, _)| k).unwrap_or(pair);
            !JUNK.contains(&key)
        })
        .collect();
    Some(if kept.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", kept.join("&"))
    })
}

/// A short name for the session directory: what the link is about, in a couple of words. Not a
/// title — the real one is known only after yt-dlp answers; this is so that a session appearing
/// in the archive is not called `20260714_181500` and nothing else.
pub fn label(url: &str) -> String {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_string();
    match host.as_str() {
        "youtube.com" | "m.youtube.com" | "youtu.be" => "youtube".into(),
        "" => "link".into(),
        h => h.split('.').next().unwrap_or("link").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same video with a different `si=` must be the SAME link — otherwise two copies of one
    /// lecture settle in the archive and search returns both.
    #[test]
    fn tracking_junk_is_stripped_and_the_link_stays_the_same() {
        let a = clean("https://youtu.be/dQw4w9WgXcQ?si=abc123&utm_source=tg").unwrap();
        let b = clean("https://youtu.be/dQw4w9WgXcQ").unwrap();
        assert_eq!(a, b);
    }

    /// `t` is the timestamp and `list` is the playlist: they change WHAT the link points at, and
    /// they stay. Cleaning must not decide for a person what they meant.
    #[test]
    fn parameters_that_change_the_target_are_kept() {
        let url = "https://www.youtube.com/watch?v=abc&t=120&list=PL1";
        assert_eq!(clean(url).as_deref(), Some(url));
    }

    #[test]
    fn a_link_that_is_only_junk_loses_its_question_mark() {
        assert_eq!(
            clean("https://example.com/video?utm_source=x&si=y").as_deref(),
            Some("https://example.com/video"),
        );
    }

    /// Not everything in the clipboard is a link. A phrase, a path, a `file://` — silence, not a
    /// guess.
    #[test]
    fn things_that_are_not_links_are_refused() {
        assert!(clean("что решили по бэклогу").is_none());
        assert!(clean("C:\\Users\\me\\video.mp4").is_none());
        assert!(clean("file:///C:/Windows/win.ini").is_none());
        assert!(clean("https://example.com/a b").is_none(), "a space is not a link");
        assert!(clean("").is_none());
    }

    #[test]
    fn the_label_says_where_it_came_from() {
        assert_eq!(label("https://youtu.be/abc"), "youtube");
        assert_eq!(label("https://www.youtube.com/watch?v=abc"), "youtube");
        assert_eq!(label("https://rutube.ru/video/abc/"), "rutube");
    }
}
