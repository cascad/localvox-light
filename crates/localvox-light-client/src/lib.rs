//! HTTP only: no capture, models, archive filesystem or server dependencies.
pub mod types;
use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Read, time::Duration};
pub use types::*;
use url::Url;

pub const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;

/// Kept distinct from HTTP/auth/schema errors for the launcher's single local
/// startup decision. It never includes a URL, credentials or remote body.
#[derive(Debug)]
pub struct NetworkUnavailable {
    mutation: bool,
    connection_refused: bool,
}
impl NetworkUnavailable {
    pub fn connection_refused(&self) -> bool {
        self.connection_refused
    }
}
impl std::fmt::Display for NetworkUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Сеть недоступна или сервер не ответил вовремя{}",
            if self.mutation {
                "; исход изменяющего запроса неизвестен"
            } else {
                ""
            }
        )
    }
}
impl std::error::Error for NetworkUnavailable {}

/// Private transport data for an owned media adapter; deliberately no Debug/Serialize.
pub struct MediaRequest {
    pub url: String,
    pub headers: Vec<String>,
}

#[derive(Clone)]
pub struct Client {
    base: Url,
    token: String,
    agent: ureq::Agent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Summary,
    Text,
    All,
}
impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Text => "text",
            Self::All => "all",
        }
    }
}

/// LLM endpoints may include /v1; daemon addresses intentionally cannot.
pub fn normalize_llm_server(value: &str) -> Result<String> {
    let url = Url::parse(value.trim()).context("Нужен HTTP(S) адрес LLM-сервера")?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && value.len() <= 2048,
        "Нужен HTTP(S) адрес без токена, логина, query или фрагмента"
    );
    Ok(url.to_string().trim_end_matches('/').into())
}
impl Client {
    pub fn new(address: &str, token: String) -> Result<Self> {
        let mut base = Url::parse(address).context("Адрес должен быть HTTP(S) URL")?;
        anyhow::ensure!(
            matches!(base.scheme(), "http" | "https") && base.host_str().is_some(),
            "Нужен HTTP(S) адрес"
        );
        anyhow::ensure!(
            base.username().is_empty()
                && base.password().is_none()
                && base.query().is_none()
                && base.fragment().is_none(),
            "Авторизация передаётся отдельно от адреса"
        );
        anyhow::ensure!(base.path() == "/", "Адрес должен указывать на корень API");
        anyhow::ensure!(!token.contains(['\r', '\n']), "Недопустимый токен");
        base.set_path("/");
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            // Release every socket after its response. Keeping idle connections
            // can occupy tiny-http parsers; asking it to half-close a Windows
            // socket before responding can abort small POSTs.
            .max_idle_connections(0)
            .timeout(Duration::from_secs(15))
            .build();
        Ok(Self { base, token, agent })
    }
    pub fn address(&self) -> &str {
        self.base.as_str()
    }
    pub fn reconnect(&self, address: &str, token: Option<String>) -> Result<Self> {
        Self::new(address, token.unwrap_or_else(|| self.token.clone()))
    }
    pub fn media(&self, session: &str) -> MediaRequest {
        MediaRequest {
            url: self
                .url(&["api", "sessions", session, "audio.wav"], &[])
                .to_string(),
            headers: if self.token.is_empty() {
                vec![]
            } else {
                vec![format!("Authorization: Bearer {}", self.token)]
            },
        }
    }
    pub fn playback(&self, session: &str) -> Result<PlaybackDoc> {
        self.get(&["api", "sessions", session, "playback"], &[])
    }
    pub fn media_snapshot(&self, session: &str, snapshot: Option<&str>) -> MediaRequest {
        let mut media = self.media(session);
        if let Some(snapshot) = snapshot {
            let mut url = Url::parse(&media.url).expect("client media URL");
            url.query_pairs_mut().append_pair("snapshot", snapshot);
            media.url = url.into();
        }
        media
    }
    pub fn transcript_document(&self, s: &str) -> Result<TranscriptDoc> {
        self.get(&["api", "sessions", s, "transcript"], &[])
    }
    pub fn url(&self, segments: &[&str], query: &[(&str, &str)]) -> Url {
        let mut u = self.base.clone();
        u.path_segments_mut()
            .expect("HTTP base URL")
            .clear()
            .extend(segments);
        if !query.is_empty() {
            u.query_pairs_mut().extend_pairs(query.iter().copied());
        }
        u
    }
    pub fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        segments: &[&str],
        query: &[(&str, &str)],
        body: Option<Value>,
        timeout: Duration,
    ) -> Result<T> {
        let url = self.url(segments, query);
        // The agent drops the connection after reading this response; do not
        // request an early server-side half-close while a POST is in flight.
        let mut request = self.agent.request(method, url.as_str()).timeout(timeout);
        if !self.token.is_empty() {
            request = request.set("Authorization", &format!("Bearer {}", self.token));
        }
        let result = match body {
            Some(v) => {
                let bytes = serde_json::to_vec(&v)?;
                anyhow::ensure!(
                    bytes.len() <= MAX_REQUEST_BYTES,
                    "JSON запроса превышает HTTP-лимит 1 MiB; сократите материал или инструкцию"
                );
                request
                    .set("Content-Type", "application/json")
                    .send_bytes(&bytes)
            }
            None => request.call(),
        };
        let response = match result {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let mut bytes = Vec::new();
                r.into_reader().take(4096).read_to_end(&mut bytes).ok();
                let message = serde_json::from_slice::<Value>(&bytes)
                    .ok()
                    .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_owned));
                let reason = match code {
                    401 => "нет доступа — задайте токен",
                    403 => "доступ запрещён",
                    503 => "сервер занят",
                    _ => "ошибка запроса",
                };
                bail!(
                    "HTTP {code}: {reason}{}",
                    message
                        .map(|m| {
                            let m = if self.token.is_empty() {
                                m
                            } else {
                                m.replace(&self.token, "[скрыто]")
                            };
                            format!(": {}", safe_text(&m))
                        })
                        .unwrap_or_default()
                );
            }
            Err(error) => {
                use std::error::Error;
                let mut source = error.source();
                let mut connection_refused = false;
                while let Some(cause) = source {
                    connection_refused |= cause
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|io| io.kind() == std::io::ErrorKind::ConnectionRefused);
                    source = cause.source();
                }
                return Err(NetworkUnavailable {
                    mutation: method != "GET",
                    connection_refused,
                }
                .into());
            }
        };
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("Чтение ответа API")?;
        anyhow::ensure!(
            bytes.len() as u64 <= MAX_RESPONSE_BYTES,
            "Ответ API превышает лимит 4 MiB"
        );
        serde_json::from_slice(&bytes).map_err(|e| {
            anyhow::anyhow!(
                "Несовместимый ответ API: {:?} JSON, строка {}, колонка {}",
                e.classify(),
                e.line(),
                e.column()
            )
        })
    }
    fn get<T: DeserializeOwned>(&self, path: &[&str], q: &[(&str, &str)]) -> Result<T> {
        self.request("GET", path, q, None, Duration::from_secs(15))
    }
    fn post<T: DeserializeOwned>(&self, path: &[&str], q: &[(&str, &str)], v: Value) -> Result<T> {
        self.request("POST", path, q, Some(v), Duration::from_secs(15))
    }
    pub fn health(&self) -> Result<Health> {
        self.health_timeout(Duration::from_secs(15))
    }
    pub fn health_timeout(&self, timeout: Duration) -> Result<Health> {
        self.request("GET", &["api", "health"], &[], None, timeout)
    }
    /// Only literal loopback and localhost qualify for automatic local startup.
    /// Remote names resolving to a local IP do not grant that authority.
    pub fn loopback_addresses(&self) -> Vec<std::net::SocketAddr> {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
        let port = self.base.port_or_known_default().unwrap_or(80);
        match self.base.host() {
            Some(url::Host::Ipv4(ip)) if ip.is_loopback() => {
                vec![SocketAddr::new(IpAddr::V4(ip), port)]
            }
            Some(url::Host::Ipv6(ip)) if ip.is_loopback() => {
                vec![SocketAddr::new(IpAddr::V6(ip), port)]
            }
            Some(url::Host::Domain("localhost")) => vec![
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
                SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
            ],
            _ => vec![],
        }
    }
    pub fn local_http_bind(&self) -> Option<String> {
        (self.base.scheme() == "http")
            .then(|| self.loopback_addresses().first().map(ToString::to_string))
            .flatten()
    }
    pub fn journal(
        &self,
        session: &str,
        after: Option<&str>,
        before: Option<&str>,
    ) -> Result<JournalPage> {
        let mut q = vec![("limit", "200")];
        if let Some(c) = after.filter(|s| !s.is_empty()) {
            q.push(("after", c));
        }
        if let Some(c) = before.filter(|s| !s.is_empty()) {
            q.push(("before", c));
        }
        self.get(&["api", "sessions", session, "journal"], &q)
    }
    pub fn sessions(&self) -> Result<Vec<Session>> {
        self.get(&["api", "sessions"], &[("limit", "50")])
    }
    pub fn record(&self) -> Result<RecordState> {
        self.get(&["api", "record"], &[])
    }
    pub fn jobs(&self) -> Result<Jobs> {
        self.get(&["api", "jobs"], &[])
    }
    pub fn langs(&self) -> Result<Vec<String>> {
        self.get::<LanguagesDoc>(&["api", "langs"], &[])
            .map(|d| d.langs)
    }
    pub fn slots(&self) -> Result<Vec<String>> {
        self.get::<SlotsDoc>(&["api", "slots"], &[])
            .map(|d| d.slots)
    }
    pub fn queue(&self) -> Result<Vec<QueueItem>> {
        self.get::<QueueDoc>(&["api", "queue"], &[])
            .map(|d| d.queue)
    }
    pub fn transcript(&self, s: &str) -> Result<Vec<TranscriptLine>> {
        self.get::<TranscriptDoc>(&["api", "sessions", s, "transcript"], &[])
            .map(|d| d.lines)
    }
    pub fn summary(&self, s: &str) -> Result<Summary> {
        self.get(&["api", "sessions", s, "summary"], &[])
    }
    pub fn readable(&self, s: &str) -> Result<ReadableDoc> {
        self.get(&["api", "sessions", s, "processed"], &[])
    }
    pub fn progress(&self, s: &str) -> Result<Progress> {
        self.get(&["api", "sessions", s, "progress"], &[])
    }
    pub fn versions(&self, s: &str) -> Result<Vec<Version>> {
        self.get::<VersionsDoc>(&["api", "sessions", s, "versions"], &[])
            .map(|d| d.versions)
    }
    pub fn speakers(&self, s: &str) -> Result<Vec<Participant>> {
        self.get::<SpeakersDoc>(&["api", "sessions", s, "speakers"], &[])
            .map(|d| d.speakers)
    }
    pub fn sources(&self, s: &str) -> Result<Vec<SourceName>> {
        self.get::<SourcesDoc>(&["api", "sessions", s, "sources"], &[])
            .map(|d| d.sources)
    }
    pub fn search(&self, q: &str, mode: &str) -> Result<Vec<Hit>> {
        self.get(
            &["api", "search"],
            &[("q", q), ("mode", mode), ("limit", "25")],
        )
    }
    pub fn ask(&self, question: &str) -> Result<Answer> {
        self.request(
            "POST",
            &["api", "ask"],
            &[],
            Some(json!({"question":question})),
            Duration::from_secs(180),
        )
    }
    pub fn asks(&self) -> Result<Vec<AskRecord>> {
        self.get::<AsksDoc>(&["api", "asks"], &[]).map(|d| d.asks)
    }
    pub fn get_ask(&self, id: &str) -> Result<AskRecord> {
        self.get(&["api", "asks", id], &[])
    }
    pub fn create_ask(&self, request: &AskRequest) -> Result<AskRecord> {
        anyhow::ensure!(
            matches!(request.provider.as_str(), "ollama" | "claude"),
            "Выберите провайдера явно"
        );
        self.request(
            "POST",
            &["api", "asks"],
            &[],
            Some(serde_json::to_value(request)?),
            Duration::from_secs(15),
        )
    }
    pub fn delete_ask(&self, id: &str) -> Result<Reply> {
        self.request(
            "DELETE",
            &["api", "asks", id],
            &[],
            None,
            Duration::from_secs(15),
        )
    }
    pub fn note(&self, text: &str, slot: &str) -> Result<Reply> {
        let body = if slot.trim().is_empty() {
            json!({"text":text})
        } else {
            json!({"text":text,"slot":slot})
        };
        self.post(&["api", "notes"], &[], body)
    }
    pub fn route(&self, text: &str) -> Result<Vec<RouteHint>> {
        self.request::<RouteDoc>(
            "POST",
            &["api", "route"],
            &[],
            Some(json!({"text":text})),
            Duration::from_secs(180),
        )
        .map(|d| d.suggestions)
    }
    pub fn slot_notes(&self) -> Result<Vec<SlotNotes>> {
        self.get::<NotesDoc>(&["api", "slots", "notes"], &[("limit", "100")])
            .map(|d| d.slots)
    }
    pub fn delete_note(&self, slot: &str, note: &StoredNote) -> Result<Reply> {
        self.request(
            "DELETE",
            &["api", "slots", "notes"],
            &[],
            Some(json!({"slot":slot,"raw":note.raw,"source":note.source})),
            Duration::from_secs(15),
        )
    }
    pub fn start_recording(&self, title: &str) -> Result<Reply> {
        self.post(&["api", "record", "start"], &[], json!({"title":title}))
    }
    pub fn stop_recording(&self) -> Result<Reply> {
        self.post(&["api", "record", "stop"], &[], json!({}))
    }
    pub fn ingest(&self, url: &str) -> Result<Reply> {
        self.post(&["api", "ingest"], &[], json!({"url":url}))
    }
    pub fn retry(&self, s: &str) -> Result<Reply> {
        self.post(&["api", "sessions", s, "retry"], &[], json!({}))
    }
    pub fn cancel(&self, s: &str) -> Result<Reply> {
        self.post(&["api", "sessions", s, "cancel"], &[], json!({}))
    }
    pub fn recook(&self, s: &str, scope: Scope) -> Result<Reply> {
        self.post(
            &["api", "sessions", s, "recook"],
            &[("scope", scope.as_str())],
            json!({}),
        )
    }
    pub fn delete_session(&self, s: &str) -> Result<Reply> {
        self.request(
            "DELETE",
            &["api", "sessions", s],
            &[],
            Some(json!({"confirm":s})),
            Duration::from_secs(15),
        )
    }
    pub fn set_best(&self, s: &str, id: u32) -> Result<BestReply> {
        self.post(&["api", "sessions", s, "best"], &[], json!({"id":id}))
    }
    pub fn set_lang(&self, s: &str, lang: &str) -> Result<Reply> {
        self.post(&["api", "sessions", s, "lang"], &[], json!({"lang":lang}))
    }
    pub fn name_speaker(&self, s: &str, speaker: &str, name: &str) -> Result<Reply> {
        self.post(
            &["api", "sessions", s, "speakers"],
            &[],
            json!({"speaker":speaker,"name":name}),
        )
    }
    pub fn name_source(&self, s: &str, id: u8, name: &str) -> Result<Reply> {
        self.post(
            &["api", "sessions", s, "sources"],
            &[],
            json!({"source_id":id,"name":name}),
        )
    }
    pub fn confirm(&self, s: &str, artifact: &str) -> Result<Reply> {
        self.post(
            &["api", "sessions", s, "confirm"],
            &[],
            json!({"artifact":artifact}),
        )
    }
    pub fn settings(&self) -> Result<SettingsDoc> {
        self.get(&["api", "settings"], &[])
    }
    pub fn save_settings(&self, set: BTreeMap<String, Option<String>>) -> Result<Reply> {
        self.post(&["api", "settings"], &[], json!({"set":set}))
    }
    pub fn peaks(&self, s: &str, buckets: usize) -> Result<Peaks> {
        let media = self.playback(s)?;
        if media.partial.is_some() && media.snapshot.is_none() {
            return Ok(Peaks::default());
        }
        self.peaks_snapshot(s, buckets, media.snapshot.as_deref())
    }
    pub fn peaks_snapshot(&self, s: &str, buckets: usize, snapshot: Option<&str>) -> Result<Peaks> {
        let count = buckets.min(320).to_string();
        let mut query = vec![("buckets", count.as_str())];
        if let Some(snapshot) = snapshot {
            query.push(("snapshot", snapshot));
        }
        self.request(
            "GET",
            &["api", "sessions", s, "peaks"],
            &query,
            None,
            Duration::from_secs(120),
        )
    }
}

/// Display text is never allowed to introduce terminal control sequences (including OSC).
pub fn safe_text(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == '\n'
                || c == '\t'
                || (!c.is_control()
                    && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
            {
                c
            } else {
                '�'
            }
        })
        .collect()
}
/// A label occupies one terminal line; body text retains its explicit line breaks.
pub fn safe_label(text: &str) -> String {
    safe_text(text).replace(['\n', '\t'], " ")
}

macro_rules! document {
    ($name:ident, $field:ident, $item:ty) => {
        #[derive(serde::Deserialize)]
        struct $name {
            $field: Vec<$item>,
        }
    };
}
document!(QueueDoc, queue, QueueItem);
document!(VersionsDoc, versions, Version);
document!(SpeakersDoc, speakers, Participant);
document!(SourcesDoc, sources, SourceName);
document!(AsksDoc, asks, AskRecord);
document!(NotesDoc, slots, SlotNotes);
document!(LanguagesDoc, langs, String);
document!(SlotsDoc, slots, String);
document!(RouteDoc, suggestions, RouteHint);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_cannot_change_api_origin_or_query() {
        let c = Client::new("http://localhost:3017", "".into()).unwrap();
        let u = c.url(
            &["api", "sessions", "a /?=#тест", "transcript"],
            &[("q", "&token=no")],
        );
        assert_eq!(u.host_str(), Some("localhost"));
        assert!(u.as_str().contains("a%20%2F%3F=%23"));
        assert_eq!(u.query_pairs().next().unwrap().1, "&token=no");
    }
    #[test]
    fn tokens_are_separate_and_control_text_is_inert() {
        assert!(Client::new("http://user:password@localhost", String::new()).is_err());
        assert!(Client::new("http://localhost", "x\r\nHeader: bad".into()).is_err());
        assert!(!safe_text("\x1b]52;c;secret\x07").contains(['\x1b', '\x07']));
        let settings: SettingsDoc =
            serde_json::from_value(json!({"extra":[{"key":"NEW_TOKEN","value":"hidden"}]}))
                .unwrap();
        assert!(!serde_json::to_string(&settings).unwrap().contains("hidden"));
    }
}
