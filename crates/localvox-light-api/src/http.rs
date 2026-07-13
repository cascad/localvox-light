//! The HTTP part of F6: a local JSON API over the archive — the foundation for the
//! PWA (F7 tier 1) and for external scripts. A synchronous stack (tiny-http), by
//! default it binds only 127.0.0.1; for LAN — an explicit `--bind 0.0.0.0:…` and a
//! mandatory token.
//!
//! Routes:
//!   GET  /api/health
//!   GET  /api/sessions?limit=20
//!   GET  /api/sessions/{name}/transcript      (json; ?format=text — plain text)
//!   GET  /api/sessions/{name}/summary         (markdown)
//!   GET  /api/sessions/{name}/processed       (markdown)
//!   GET  /api/sessions/{name}/clip?source=0&start=12&dur=20   (audio/wav — player)
//!   GET  /api/search?q=...&limit=10
//!   POST /api/notes {"text": "...", "slot": "идеи"?}
//!   POST /api/route {"text": "..."}   → LLM slot hint (F5)
//!   POST /api/sessions/{name}/recook  → throw away the derivatives and cook anew
//!   POST /api/session/finish          → close the current session (the cook picks it up)
//!   GET  /api/slots                   → the list of configured slots for notes
//!   GET  /api/sessions/{name}/versions → transcript versions
//!   POST /api/sessions/{name}/best {"id": 3} → make a version the working one
//!   POST /api/sessions/{name}/lang {"lang": "en"|"auto"} → recording language (+re-cook)
//!   GET  /            — the embedded PWA page of the archive (F7 tier 1; it carries
//!                       no data, so it is served without authorization — the data is
//!                       behind /api/*)
//!
//! Authorization: only the `Authorization: Bearer <token>` header (we do not accept
//! the token in the query — it settles in the browser history and in proxy logs);
//! with no token configured, access is allowed only from loopback addresses.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tiny_http::{Header, Method, Response, Server};

use crate::archive::{render_transcript_text, Archive};

/// Ceiling for the POST body: notes, not file uploads.
const MAX_BODY_BYTES: usize = 1 << 20;
/// Ceiling on concurrent requests: a local API, not the public web.
const MAX_INFLIGHT: usize = 32;
/// A separate ceiling on concurrent LLM routings (/api/route): an expensive call MUST
/// NOT take up the whole shared budget and freeze the cheap endpoints.
const MAX_ROUTE_INFLIGHT: usize = 4;

static ROUTE_INFLIGHT: AtomicUsize = AtomicUsize::new(0);

/// RAII counter of concurrent /api/route calls: `Drop` frees the slot.
struct RouteGuard;
impl RouteGuard {
    fn try_acquire() -> Option<Self> {
        if ROUTE_INFLIGHT.fetch_add(1, Ordering::AcqRel) >= MAX_ROUTE_INFLIGHT {
            ROUTE_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
            None
        } else {
            Some(Self)
        }
    }
}
impl Drop for RouteGuard {
    fn drop(&mut self) {
        ROUTE_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct HttpConfig {
    pub bind: String,
    pub token: Option<String>,
}

/// A synchronous bind — the "port is busy" error is visible to the caller right away,
/// not in the log of a background thread.
pub fn bind(addr: &str) -> Result<Server> {
    Server::http(addr).map_err(|e| anyhow::anyhow!("bind {addr}: {e}"))
}

pub fn run_http(archive: Arc<Archive>, cfg: HttpConfig) -> Result<()> {
    let server = bind(&cfg.bind)?;
    serve(server, archive, cfg)
}

/// The serving loop: a thread per request, so that a slow client does not block the
/// others; the number of concurrent ones is bounded (otherwise 503).
pub fn serve(server: Server, archive: Arc<Archive>, cfg: HttpConfig) -> Result<()> {
    tracing::info!(
        "HTTP API: http://{} (token: {})",
        cfg.bind,
        if cfg.token.is_some() {
            "enabled"
        } else {
            "none — localhost only"
        }
    );
    let cfg = Arc::new(cfg);
    let inflight = Arc::new(AtomicUsize::new(0));
    for request in server.incoming_requests() {
        if inflight.load(Ordering::Relaxed) >= MAX_INFLIGHT {
            respond_json(request, 503, json!({"error": "занято, повторите позже"}));
            continue;
        }
        inflight.fetch_add(1, Ordering::Relaxed);
        let archive = Arc::clone(&archive);
        let cfg = Arc::clone(&cfg);
        let inflight = Arc::clone(&inflight);
        std::thread::Builder::new()
            .name("http-req".into())
            .spawn(move || {
                handle_request(&archive, &cfg, request);
                inflight.fetch_sub(1, Ordering::Relaxed);
            })
            .ok();
    }
    Ok(())
}

/// The embedded web page of the archive (search/sessions/notes from a phone).
pub const WEBAPP_HTML: &str = include_str!("webapp.html");

/// The PWA manifest: the page installs onto the home screen as an application.
/// The icon is an inline SVG in a data: URI (zero external files, CSP img-src data: ok).
const WEBAPP_MANIFEST: &str = r##"{
  "name": "localvox — архив",
  "short_name": "localvox",
  "start_url": "/",
  "scope": "/",
  "display": "standalone",
  "background_color": "#161719",
  "theme_color": "#3b6ea5",
  "icons": [
    {
      "src": "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 192 192'%3E%3Crect width='192' height='192' rx='36' fill='%233b6ea5'/%3E%3Cpath d='M96 40a20 20 0 0 1 20 20v36a20 20 0 0 1-40 0V60a20 20 0 0 1 20-20zm44 56a44 44 0 0 1-36 43.3V156h-16v-16.7A44 44 0 0 1 52 96h14a30 30 0 0 0 60 0z' fill='white'/%3E%3C/svg%3E",
      "sizes": "192x192",
      "type": "image/svg+xml",
      "purpose": "any maskable"
    }
  ]
}"##;

/// CSP + anti-sniff + no-cache for the static page: the only inline script is ours,
/// but connections go only to our own origin (the token from localStorage MUST NOT
/// leak outward), and the cache does not stick to an old version after a daemon
/// update.
const WEBAPP_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; \
style-src 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; \
media-src 'self' blob:; manifest-src 'self'; base-uri 'none'; form-action 'none'; \
frame-ancestors 'none'";

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

/// Is the host from the Host/Origin header a loopback one? Protects the default
/// token-less configuration from DNS rebinding (a re-bound Host is no longer loopback)
/// and from cross-origin CSRF (a foreign Origin). Not needed when a token is set: the
/// token IS the control there, and the Host is a LAN address.
fn host_is_loopback(h: &str) -> bool {
    let hostport = h.split("://").nth(1).unwrap_or(h); // Origin carries the scheme
    let host = hostport
        .rsplit_once(':')
        .map(|(a, _)| a)
        .unwrap_or(hostport);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host == "127.0.0.1" || host == "::1" || host.eq_ignore_ascii_case("localhost")
}

fn handle_request(archive: &Archive, cfg: &HttpConfig, mut request: tiny_http::Request) {
    let url = request.url().to_string();
    let method = request.method().clone();

    // The PWA manifest: "add to home screen" (F7). No data, no auth.
    if method == Method::Get && split_url(&url).0 == "/manifest.webmanifest" {
        let response = Response::from_string(WEBAPP_MANIFEST)
            .with_status_code(200)
            .with_header(header(
                "Content-Type",
                "application/manifest+json; charset=utf-8",
            ))
            .with_header(header("Cache-Control", "no-cache"));
        let _ = request.respond(response);
        return;
    }

    // Static content without authorization: it holds no secrets, the data is behind
    // /api/*.
    if method == Method::Get && matches!(split_url(&url).0, "/" | "/app") {
        let response = Response::from_string(WEBAPP_HTML)
            .with_status_code(200)
            .with_header(header("Content-Type", "text/html; charset=utf-8"))
            .with_header(header("Content-Security-Policy", WEBAPP_CSP))
            .with_header(header("X-Content-Type-Options", "nosniff"))
            .with_header(header("Cache-Control", "no-cache"));
        let _ = request.respond(response);
        return;
    }

    let is_local = request
        .remote_addr()
        .map(|a| a.ip().is_loopback())
        .unwrap_or(false);
    let auth_header = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.as_str().to_string());

    // The default (no token) trusts loopback — but then Host/Origin MUST be loopback
    // too, otherwise a third-party site, through the victim's browser (CSRF / DNS
    // rebinding), writes notes and reads the archive. With a token this check is not
    // applied: there the Host is a LAN address, and the token is what protects.
    if cfg.token.is_none() {
        let host = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Host"))
            .map(|h| h.value.as_str().to_string());
        let origin = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Origin"))
            .map(|h| h.value.as_str().to_string());
        let host_ok = host.as_deref().is_some_and(host_is_loopback);
        let origin_ok = origin.as_deref().map(host_is_loopback).unwrap_or(true);
        if !host_ok || !origin_ok {
            respond_json(request, 403, json!({"error": "forbidden origin/host"}));
            return;
        }
    }

    // Authorization BEFORE reading the body: someone else's POST will not make us
    // buffer gigabytes.
    if !authorized(cfg, auth_header.as_deref(), is_local) {
        respond_json(
            request,
            401,
            json!({"error": "unauthorized (нужен токен или localhost)"}),
        );
        return;
    }

    // The binary route of the audio clip (the player) — not JSON.
    // GET /api/sessions/{name}/clip?source=0&start=12.5&dur=20
    if method == Method::Get {
        let (path, query) = split_url(&url);
        if let Some(rest) = path.strip_prefix("/api/sessions/") {
            if let Some((name, "clip")) = rest.split_once('/') {
                // `source` is not specified — we play the MIX of both tracks:
                // listening to half a dialogue is pointless. An explicit `source=0|1`
                // remains for the line player: there we know whose line we are showing.
                let source = query_param(query, "source").and_then(|v| v.parse::<u8>().ok());
                let start = query_param(query, "start")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0);
                let dur = query_param(query, "dur")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30.0);
                match archive.audio_clip(name, source, start, dur) {
                    Ok(bytes) => {
                        let response = Response::from_data(bytes).with_header(
                            Header::from_bytes(&b"Content-Type"[..], &b"audio/wav"[..]).unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        tracing::warn!("clip {name}: {msg}");
                        // the chunk name/source/range are not a secret; we return the
                        // reason
                        respond_json(request, 404, json!({"error": msg}));
                    }
                }
                return;
            }
        }
    }

    let mut body = String::new();
    if method == Method::Post {
        use std::io::Read as _;
        let declared_too_big = request.body_length().is_some_and(|n| n > MAX_BODY_BYTES);
        if !declared_too_big {
            let _ = request
                .as_reader()
                .take(MAX_BODY_BYTES as u64 + 1)
                .read_to_string(&mut body);
        }
        if declared_too_big || body.len() > MAX_BODY_BYTES {
            respond_json(request, 413, json!({"error": "тело больше 1 МиБ"}));
            return;
        }
    }

    let (status, payload) = respond(
        archive,
        cfg,
        &method,
        &url,
        auth_header.as_deref(),
        is_local,
        &body,
    );
    respond_json(request, status, payload);
}

fn respond_json(request: tiny_http::Request, status: u16, payload: Value) {
    let response = Response::from_string(payload.to_string())
        .with_status_code(status)
        .with_header(
            Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .unwrap(),
        );
    let _ = request.respond(response);
}

fn authorized(cfg: &HttpConfig, auth_header: Option<&str>, is_local: bool) -> bool {
    match &cfg.token {
        Some(t) => {
            let expected = format!("Bearer {t}");
            auth_header.is_some_and(|h| ct_eq(h.as_bytes(), expected.as_bytes()))
        }
        None => is_local,
    }
}

/// Comparison without an early exit: the timing does not give away the position of
/// the first mismatch (we do not hide the token's length — that is accepted practice).
/// Against timing brute-force on LAN binds.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The pure router (tested without sockets): (status, json).
pub fn respond(
    archive: &Archive,
    cfg: &HttpConfig,
    method: &Method,
    url: &str,
    auth_header: Option<&str>,
    is_local: bool,
    body: &str,
) -> (u16, Value) {
    let (path, query) = split_url(url);

    if !authorized(cfg, auth_header, is_local) {
        return (
            401,
            json!({"error": "unauthorized (нужен токен или localhost)"}),
        );
    }

    let result: Result<Value> = (|| {
        match (method, path) {
            (Method::Get, "/api/health") => {
                Ok(json!({"ok": true, "version": env!("CARGO_PKG_VERSION")}))
            }
            // Visibility of the autopilot: what is cooking / how many are queued (WP-C13)
            (Method::Get, "/api/jobs") => Ok(serde_json::to_value(archive.jobs_status())?),
            (Method::Get, "/api/sessions") => {
                let limit = query_param(query, "limit")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(20);
                Ok(serde_json::to_value(archive.list_sessions(limit))?)
            }
            (Method::Get, "/api/search") => {
                let q = query_param(query, "q").context("нужен параметр q")?;
                let limit = query_param(query, "limit")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10);
                // lexical (default) | semantic | hybrid — semantics requires Ollama
                let mode = query_param(query, "mode").unwrap_or_default();
                Ok(serde_json::to_value(
                    archive.search_mode(&q, limit, &mode)?,
                )?)
            }
            (Method::Post, "/api/notes") => {
                let parsed: Value = serde_json::from_str(body).context("тело — JSON")?;
                let text = parsed
                    .get("text")
                    .and_then(Value::as_str)
                    .context("нужно поле text")?;
                let slot = parsed.get("slot").and_then(Value::as_str);
                let dest = archive.append_note(slot, text)?;
                // the destination path is a local detail, we do not hand it outward
                if is_local {
                    Ok(json!({"ok": true, "dest": dest}))
                } else {
                    Ok(json!({"ok": true}))
                }
            }
            (Method::Post, "/api/route") => {
                // F5: "where do I file this?" — an LLM slot hint for the note's text.
                // A separate small limit: LLM calls MUST NOT eat the shared inflight
                // budget and freeze the cheap endpoints (health/search).
                let _guard = RouteGuard::try_acquire()
                    .context("занято: слишком много LLM-запросов, повторите")?;
                let parsed: Value = serde_json::from_str(body).context("тело — JSON")?;
                let text = parsed
                    .get("text")
                    .and_then(Value::as_str)
                    .context("нужно поле text")?;
                let suggestions = archive.route(text)?;
                // reason is free-form LLM text (it may reflect the slot's description);
                // outward we hand only the names, locally — with the reasons.
                if is_local {
                    Ok(json!({"suggestions": suggestions}))
                } else {
                    Ok(json!({"suggestions": suggestions.iter()
                        .map(|s| json!({"slot": s.slot})).collect::<Vec<_>>()}))
                }
            }
            // F4: a question to the archive. The answer is built ONLY from the found
            // fragments and carries citations to them; if nothing was found, the model
            // is not called at all.
            //
            // The same limit as for routing, and for the same reason: LLM calls MUST
            // NOT eat the shared inflight budget and freeze the cheap endpoints.
            (Method::Post, "/api/ask") => {
                let _guard = RouteGuard::try_acquire()
                    .context("занято: слишком много LLM-запросов, повторите")?;
                let parsed: Value = serde_json::from_str(body).context("тело — JSON")?;
                let question = parsed
                    .get("question")
                    .and_then(Value::as_str)
                    .context("нужно поле question")?;
                Ok(serde_json::to_value(archive.ask(question)?)?)
            }
            // "The model lied — re-cook it." Throws away the derived artifacts and
            // puts the session up for cooking anew. The audio is not touched:
            // everything is recreated from it, so throwing away a derivative is not a
            // loss.
            (Method::Get, "/api/slots") => Ok(json!({"slots": archive.slots()})),
            // Transcript versions: what it was cooked with, when, which is the working
            // one right now.
            (Method::Get, p) if p.ends_with("/versions") => {
                let name = p
                    .strip_prefix("/api/sessions/")
                    .and_then(|r| r.strip_suffix("/versions"))
                    .context("не найдено")?;
                Ok(json!({"versions": archive.versions(name)?}))
            }
            // Make a version the working one — the summary, the search and the export
            // are computed from it.
            (Method::Post, p) if p.ends_with("/best") => {
                let name = p
                    .strip_prefix("/api/sessions/")
                    .and_then(|r| r.strip_suffix("/best"))
                    .context("не найдено")?;
                let parsed: Value = serde_json::from_str(body).context("тело — JSON")?;
                let id = parsed
                    .get("id")
                    .and_then(Value::as_u64)
                    .context("нужно поле id")? as u32;
                archive.set_best(name, id)?;
                Ok(json!({"ok": true, "best": id}))
            }
            // "Finish the session" — to get a summary one no longer has to kill the
            // daemon.
            (Method::Post, "/api/session/finish") => {
                Ok(json!({"ok": true, "session": archive.finish_session()?}))
            }
            // "This is a meeting": close the current recording and start a new one,
            // with a title. Auto-detection will never see a face-to-face stand-up —
            // that is why there is a button.
            (Method::Post, "/api/session/meeting") => {
                let parsed: Value = serde_json::from_str(body).unwrap_or(json!({}));
                let title = parsed.get("title").and_then(Value::as_str).unwrap_or("");
                Ok(json!({"ok": true, "msg": archive.start_meeting(title)?}))
            }
            (Method::Post, p) if p.ends_with("/recook") => {
                let rest = p
                    .strip_prefix("/api/sessions/")
                    .and_then(|r| r.strip_suffix("/recook"))
                    .context("не найдено")?;
                Ok(json!({"ok": true, "msg": archive.recook(rest)?}))
            }
            // The languages we can REALLY recognize (the model is on disk).
            (Method::Get, "/api/langs") => Ok(json!({"langs": archive.langs()})),
            // The voices we know by name. Shared across the whole archive.
            (Method::Get, "/api/speakers") => {
                Ok(json!({"speakers": archive.known_speakers()}))
            }
            // Forget a voice: a person MUST be able to erase biometrics.
            (Method::Post, "/api/speakers/forget") => {
                let parsed: Value = serde_json::from_str(body).context("тело — JSON")?;
                let name = parsed
                    .get("name")
                    .and_then(Value::as_str)
                    .context("нужно поле name")?;
                Ok(json!({"ok": true, "msg": archive.forget_speaker(name)?}))
            }
            // «Участник 2 — это Иван»: the voice is remembered, the lines are
            // re-labelled, the summary is rebuilt. The audio is not re-cooked — it has
            // not changed.
            (Method::Post, p) if p.ends_with("/speakers") => {
                let session = p
                    .strip_prefix("/api/sessions/")
                    .and_then(|r| r.strip_suffix("/speakers"))
                    .context("не найдено")?;
                let parsed: Value = serde_json::from_str(body).context("тело — JSON")?;
                let label = parsed
                    .get("speaker")
                    .and_then(Value::as_str)
                    .context("нужно поле speaker (кого переименовываем: «Участник 2»)")?;
                let name = parsed
                    .get("name")
                    .and_then(Value::as_str)
                    .context("нужно поле name (как его зовут)")?;
                Ok(json!({"ok": true, "msg": archive.name_speaker(session, label, name)?}))
            }
            // The recording's language. Changing the language devalues everything
            // derived (the language is part of the recipe), so the session goes
            // straight to a re-cook.
            (Method::Post, p) if p.ends_with("/lang") => {
                let name = p
                    .strip_prefix("/api/sessions/")
                    .and_then(|r| r.strip_suffix("/lang"))
                    .context("не найдено")?;
                let parsed: Value = serde_json::from_str(body).context("тело — JSON")?;
                let raw = parsed
                    .get("lang")
                    .and_then(Value::as_str)
                    .context("нужно поле lang («auto» или код языка: ru, en…)")?;
                let lang = (!raw.eq_ignore_ascii_case("auto")).then_some(raw);
                Ok(json!({"ok": true, "msg": archive.set_lang(name, lang)?}))
            }
            (Method::Get, p) => {
                // /api/sessions/{name}/{artifact}
                let rest = p.strip_prefix("/api/sessions/").context("не найдено")?;
                let (name, artifact) = rest.split_once('/').context("не найдено")?;
                // the name arrives URL-encoded only in exotic cases — our names are
                // ASCII+digits
                match artifact {
                    "transcript" => {
                        let doc = archive.transcript(name)?;
                        if query_param(query, "format").as_deref() == Some("text") {
                            Ok(
                                json!({"text": render_transcript_text(&doc, std::path::Path::new(""))}),
                            )
                        } else {
                            Ok(serde_json::to_value(doc)?)
                        }
                    }
                    // A verified result and a draft not confirmed by the recording are
                    // DIFFERENT files and different tabs: an invention MUST NOT look
                    // like the summary.
                    "summary" => Ok(json!({"markdown": archive.artifact(name, "summary.md")?})),
                    "summary-unverified" => {
                        Ok(json!({"markdown": archive.artifact(name, "summary.unverified.md")?}))
                    }
                    "processed" => Ok(json!({"markdown": archive.artifact(name, "processed.md")?})),
                    "processed-unverified" => {
                        Ok(json!({"markdown": archive.artifact(name, "processed.unverified.md")?}))
                    }
                    // Who spoke in this recording. Empty means the voices were not
                    // counted (no model) or not found: both reasons are honest.
                    "speakers" => Ok(json!({"speakers": archive.speakers(name)?})),
                    _ => anyhow::bail!("не найдено"),
                }
            }
            _ => anyhow::bail!("не найдено"),
        }
    })();

    match result {
        Ok(v) => (200, v),
        Err(e) => {
            let msg = format!("{e:#}");
            let code = if msg.contains("не найдено") || msg.contains("не найдена")
            {
                404
            } else if msg.contains("конфиг слотов") {
                500 // a server misconfiguration (no slots.toml) — not the client's fault
            } else if msg.contains("занято") || msg.contains("embed") {
                // overload (Route/SemanticGuard) or an unreachable Ollama («embed
                // через …», «разбор ответа /api/embed») — not the client's fault:
                // 503, not a 400 "bad request"
                503
            } else {
                400
            };
            // The full error chain goes into the log and to local clients; to remote
            // ones — without paths, slot names and other file-system details.
            tracing::warn!("API {method} {path}: {msg}");
            let public = if is_local {
                msg
            } else if code == 404 {
                "не найдено".to_string()
            } else if code == 500 {
                "слоты не настроены на сервере (slots.toml)".to_string()
            } else if code == 503 {
                "сервис временно недоступен, повторите позже".to_string()
            } else {
                "некорректный запрос".to_string()
            };
            (code, json!({"error": public}))
        }
    }
}

fn split_url(url: &str) -> (&str, &str) {
    match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url, ""),
    }
}

fn query_param(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        // pairs without "=" (bare flags, empties from "&&") are simply skipped
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if k == key {
            return Some(percent_decode(v));
        }
    }
    None
}

/// A minimal percent-decode (+ for space, %XX bytes, UTF-8).
fn percent_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut it = s.bytes();
    while let Some(b) = it.next() {
        match b {
            b'+' => bytes.push(b' '),
            b'%' => {
                let h = it.next().unwrap_or(b'0');
                let l = it.next().unwrap_or(b'0');
                let hex = [h, l];
                let v = u8::from_str_radix(std::str::from_utf8(&hex).unwrap_or("0"), 16)
                    .unwrap_or(b'%');
                bytes.push(v);
            }
            other => bytes.push(other),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn archive(dir: &Path) -> Archive {
        let session = dir.join("sessions/20260711_http");
        fs::create_dir_all(&session).unwrap();
        fs::write(session.join("summary.md"), "## Решения\nвсё хорошо\n").unwrap();
        Archive::new(dir.to_path_buf())
    }

    fn cfg(token: Option<&str>) -> HttpConfig {
        HttpConfig {
            bind: "127.0.0.1:0".into(),
            token: token.map(String::from),
        }
    }

    #[test]
    fn health_and_sessions_from_localhost_without_token() {
        let dir = tempfile::tempdir().unwrap();
        let a = archive(dir.path());
        let (code, v) = respond(&a, &cfg(None), &Method::Get, "/api/health", None, true, "");
        assert_eq!(code, 200);
        assert_eq!(v["ok"], true);
        let (code, v) = respond(
            &a,
            &cfg(None),
            &Method::Get,
            "/api/sessions",
            None,
            true,
            "",
        );
        assert_eq!(code, 200);
        assert_eq!(v[0]["name"], "20260711_http");
        assert_eq!(v[0]["has_summary"], true);
    }

    #[test]
    fn token_required_for_remote_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let a = archive(dir.path());
        // without a token, non-localhost — refused
        let (code, _) = respond(&a, &cfg(None), &Method::Get, "/api/health", None, false, "");
        assert_eq!(code, 401);
        // with a token: a wrong one — refused, the right one (header or query) — ok
        let c = cfg(Some("s3cret"));
        let (code, _) = respond(
            &a,
            &c,
            &Method::Get,
            "/api/health",
            Some("Bearer wrong"),
            false,
            "",
        );
        assert_eq!(code, 401);
        let (code, _) = respond(
            &a,
            &c,
            &Method::Get,
            "/api/health",
            Some("Bearer s3cret"),
            false,
            "",
        );
        assert_eq!(code, 200);
        // the token in the query is no longer accepted (it settles in history/logs)
        let (code, _) = respond(
            &a,
            &c,
            &Method::Get,
            "/api/health?token=s3cret",
            None,
            false,
            "",
        );
        assert_eq!(code, 401);
    }

    #[test]
    fn remote_errors_are_sanitized_local_are_detailed() {
        let dir = tempfile::tempdir().unwrap();
        let a = archive(dir.path());
        let c = cfg(Some("s3cret"));
        let (code, v) = respond(
            &a,
            &c,
            &Method::Get,
            "/api/sessions/nope/summary",
            Some("Bearer s3cret"),
            false,
            "",
        );
        assert_eq!(code, 404);
        assert_eq!(v["error"], "не найдено");
        let (code, v) = respond(
            &a,
            &cfg(None),
            &Method::Get,
            "/api/sessions/nope/summary",
            None,
            true,
            "",
        );
        assert_eq!(code, 404);
        assert!(v["error"].as_str().unwrap().contains("nope"));
    }

    #[test]
    fn webapp_is_embedded_and_routed() {
        assert!(WEBAPP_HTML.contains("localvox — архив"));
        // static routing goes by the clean path, the query does not get in the way
        assert_eq!(split_url("/app?x=1").0, "/app");
        assert_eq!(split_url("/").0, "/");
    }

    #[test]
    fn host_loopback_check_matches_only_local() {
        assert!(host_is_loopback("127.0.0.1:3017"));
        assert!(host_is_loopback("localhost:3017"));
        assert!(host_is_loopback("[::1]:3017"));
        assert!(host_is_loopback("http://127.0.0.1:3017")); // Origin with a scheme
        assert!(!host_is_loopback("evil.com"));
        assert!(!host_is_loopback("192.168.1.5:3017"));
        assert!(!host_is_loopback("http://evil.com"));
    }

    #[test]
    fn ct_eq_matches_semantics_of_equality() {
        assert!(ct_eq(b"Bearer s3cret", b"Bearer s3cret"));
        assert!(!ct_eq(b"Bearer s3cret", b"Bearer s3crex"));
        assert!(!ct_eq(b"short", b"longer"));
    }

    #[test]
    fn query_param_skips_pairs_without_eq() {
        assert_eq!(query_param("debug&q=x", "q").as_deref(), Some("x"));
        assert_eq!(query_param("&&q=x", "q").as_deref(), Some("x"));
        assert_eq!(query_param("debug", "q"), None);
    }

    #[test]
    fn summary_endpoint_and_404() {
        let dir = tempfile::tempdir().unwrap();
        let a = archive(dir.path());
        let (code, v) = respond(
            &a,
            &cfg(None),
            &Method::Get,
            "/api/sessions/20260711_http/summary",
            None,
            true,
            "",
        );
        assert_eq!(code, 200);
        assert!(v["markdown"].as_str().unwrap().contains("всё хорошо"));
        let (code, _) = respond(
            &a,
            &cfg(None),
            &Method::Get,
            "/api/sessions/nope/summary",
            None,
            true,
            "",
        );
        assert_eq!(code, 404);
    }

    #[test]
    fn note_post_via_slots() {
        let dir = tempfile::tempdir().unwrap();
        let a = archive(dir.path());
        let vault = dir.path().join("v.md");
        let slots = dir.path().join("slots.toml");
        fs::write(
            &slots,
            format!(
                "[slots.\"идеи\"]\npath = \"{}\"\ntemplate = \"- {{{{text}}}}\"\ndefault = true\n",
                vault.display().to_string().replace('\\', "/")
            ),
        )
        .unwrap();
        let _env = crate::SLOTS_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("LOCALVOX_SLOTS_CONFIG", &slots);
        let (code, v) = respond(
            &a,
            &cfg(None),
            &Method::Post,
            "/api/notes",
            None,
            true,
            r#"{"text": "из http"}"#,
        );
        std::env::remove_var("LOCALVOX_SLOTS_CONFIG");
        assert_eq!(code, 200, "{v}");
        assert_eq!(fs::read_to_string(&vault).unwrap(), "- из http\n");
    }

    #[test]
    fn search_query_is_percent_decoded() {
        assert_eq!(
            percent_decode("%D0%B8%D0%B4%D0%B5%D0%B8+%D1%82%D0%B5%D1%81%D1%82"),
            "идеи тест"
        );
    }
}
