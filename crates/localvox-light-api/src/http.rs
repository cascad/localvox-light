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
use rust_embed::Embed;
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

/// The application: the built frontend (`ui/dist`), embedded into the binary. It is
/// the same code that runs in the desktop shell, in the browser and on a phone over
/// the LAN — the shell only opens a window onto this origin.
///
/// If `ui/dist` is absent, build.rs puts a page there that says how to build it: a
/// missing frontend must not break `cargo build` for someone who has no Node.
#[derive(Embed)]
#[folder = "../../ui/dist"]
struct WebApp;

/// The previous single-file page. Kept at `/legacy` until the new UI reaches parity —
/// removing the only working interface before its replacement is proven is how you end
/// up with no interface at all.
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

/// CSP for the app: scripts and styles are our own files and nothing else. Connections
/// go only to our own origin — the token lives in localStorage and MUST NOT leak
/// outward. `style-src` keeps 'unsafe-inline' because React writes style attributes;
/// scripts have no such loophole.
const APP_CSP: &str = "default-src 'none'; script-src 'self'; \
style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; \
media-src 'self' blob:; manifest-src 'self'; base-uri 'none'; form-action 'none'; \
frame-ancestors 'none'";

/// The legacy single-file page: its script IS inline, so it needs its own CSP.
const WEBAPP_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; \
style-src 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; \
media-src 'self' blob:; manifest-src 'self'; base-uri 'none'; form-action 'none'; \
frame-ancestors 'none'";

/// Content type by extension. A wrong type on a module script means the browser
/// refuses to run it, and the app shows a blank page with no error anywhere.
fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, e)| e) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("json") => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

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

/// `Range: bytes=START-END` → `(start, end_inclusive?)`. Only a single range is honoured — that is
/// all a media element ever sends. Anything unparseable means "the whole file".
fn parse_range(request: &tiny_http::Request) -> Option<(u64, Option<u64>)> {
    let raw = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Range"))
        .map(|h| h.value.as_str().to_string())?;
    let spec = raw.trim().strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let start: u64 = a.trim().parse().ok()?;
    let end = b.trim();
    let end = if end.is_empty() { None } else { Some(end.parse().ok()?) };
    Some((start, end))
}

/// Serve the recording's WAV, honouring Range so the browser seeks natively.
fn serve_audio_wav(archive: &Archive, request: tiny_http::Request, name: &str) {
    let range = parse_range(&request);
    let partial = range.is_some();
    match archive.audio_wav(name, range) {
        Ok(slice) => {
            let status = if partial { 206 } else { 200 };
            let mut response = Response::from_data(slice.bytes)
                .with_status_code(status)
                .with_header(header("Content-Type", "audio/wav"))
                // Without this the browser will not even try to seek by range — it re-downloads
                // from the start on every seek, which is exactly the lag we are removing.
                .with_header(header("Accept-Ranges", "bytes"));
            if partial {
                response = response.with_header(header(
                    "Content-Range",
                    &format!("bytes {}-{}/{}", slice.start, slice.end.saturating_sub(1), slice.total),
                ));
            }
            let _ = request.respond(response);
        }
        Err(e) => {
            let msg = format!("{e:#}");
            tracing::warn!("audio {name}: {msg}");
            respond_json(request, 404, json!({"error": msg}));
        }
    }
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
    if method == Method::Get {
        let path = split_url(&url).0;

        // The app. Asset names carry a content hash, so they may be cached forever;
        // index.html must not be, or a daemon update leaves the browser on the old one.
        let asset = match path {
            "/" | "/app" => Some("index.html"),
            p if p.starts_with("/assets/") => Some(p.trim_start_matches('/')),
            _ => None,
        };
        if let Some(name) = asset {
            if let Some(file) = WebApp::get(name) {
                let immutable = name != "index.html";
                let response = Response::from_data(file.data.into_owned())
                    .with_status_code(200)
                    .with_header(header("Content-Type", content_type(name)))
                    .with_header(header("Content-Security-Policy", APP_CSP))
                    .with_header(header("X-Content-Type-Options", "nosniff"))
                    .with_header(header(
                        "Cache-Control",
                        if immutable { "public, max-age=31536000, immutable" } else { "no-cache" },
                    ));
                let _ = request.respond(response);
                return;
            }
        }

        // The old page while the new one is being finished.
        if path == "/legacy" {
            let response = Response::from_string(WEBAPP_HTML)
                .with_status_code(200)
                .with_header(header("Content-Type", "text/html; charset=utf-8"))
                .with_header(header("Content-Security-Policy", WEBAPP_CSP))
                .with_header(header("X-Content-Type-Options", "nosniff"))
                .with_header(header("Cache-Control", "no-cache"));
            let _ = request.respond(response);
            return;
        }
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

    // The full recording as one streamable WAV — the player hands this straight to an <audio>
    // element, which then seeks NATIVELY via HTTP Range: instant, exact, fetching only the bytes
    // around the target. Handled here, before the header-only auth, because an <audio> element
    // sends no Authorization header — it authenticates by loopback, or by a `?token=` the client
    // appends (a local media stream; the query-token trade-off is scoped to this one route).
    if method == Method::Get {
        let (path, query) = split_url(&url);
        if let Some(rest) = path.strip_prefix("/api/sessions/") {
            if let Some((name, "audio.wav")) = rest.split_once('/') {
                let q_token = query_param(query, "token");
                let ok = match &cfg.token {
                    Some(t) => q_token.as_deref() == Some(t.as_str())
                        || auth_header.as_deref() == Some(&format!("Bearer {t}")),
                    None => is_local,
                };
                if !ok {
                    respond_json(request, 401, json!({"error": "unauthorized"}));
                    return;
                }
                serve_audio_wav(archive, request, name);
                return;
            }
        }
    }

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
    // DELETE carries a body too: the session name is echoed back in it as confirmation.
    if method == Method::Post || method == Method::Delete {
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
            // The queue itself: what is cooking now and what comes after it, in order. Counts
            // answer «is it working»; only a list answers «on what, and what is next».
            (Method::Get, "/api/queue") => Ok(json!({"queue": archive.queue()})),
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
            // «Спросить у LLM про файл/текст» — CREATE the request (Pending) and return at once. The
            // model call happens in the daemon's ask-worker, so no RouteGuard and no long wait here:
            // this endpoint is now instant, and the answer is polled from GET /api/asks/{id}.
            (Method::Post, "/api/asks") => {
                let req: crate::archive::AskRequest =
                    serde_json::from_str(body).context("тело — JSON {text, prompt?, provider?}")?;
                Ok(serde_json::to_value(archive.create_ask(req)?)?)
            }
            (Method::Get, "/api/asks") => Ok(json!({ "asks": archive.list_asks() })),
            (Method::Get, p) if p.starts_with("/api/asks/") => {
                let id = p.strip_prefix("/api/asks/").context("не найдено")?;
                archive.get_ask(id)
            }
            // Delete one request entirely. Unlike a session (whose audio is irreplaceable and needs
            // an echoed confirm), an ask is re-runnable — the two-step confirm in the UI is enough.
            (Method::Delete, p) if p.starts_with("/api/asks/") => {
                let id = p.strip_prefix("/api/asks/").context("не найдено")?;
                Ok(json!({ "ok": true, "deleted": archive.delete_ask(id)? }))
            }
            // "The model lied — re-cook it." Throws away the derived artifacts and
            // puts the session up for cooking anew. The audio is not touched:
            // everything is recreated from it, so throwing away a derivative is not a
            // loss.
            // What the session's audio sources are called. Not speakers: «which input», and the
            // name matters most for a session that arrived by link, where source 0 is not the owner.
            (Method::Get, p) if p.ends_with("/sources") => {
                let name = p
                    .strip_prefix("/api/sessions/")
                    .and_then(|r| r.strip_suffix("/sources"))
                    .context("не найдено")?;
                Ok(json!({ "sources": archive.sources(name)? }))
            }
            (Method::Post, p) if p.ends_with("/sources") => {
                let name = p
                    .strip_prefix("/api/sessions/")
                    .and_then(|r| r.strip_suffix("/sources"))
                    .context("не найдено")?;
                let parsed: Value = serde_json::from_str(body).context("тело — JSON")?;
                let source_id = parsed
                    .get("source_id")
                    .and_then(Value::as_u64)
                    .context("нужно поле source_id")? as u8;
                // An empty name is a REQUEST, not a mistake: it means «back to the default».
                let new_name = parsed.get("name").and_then(Value::as_str).unwrap_or("");
                Ok(json!({"ok": true, "msg": archive.name_source(name, source_id, new_name)?}))
            }
            (Method::Get, "/api/slots") => Ok(json!({"slots": archive.slots()})),
            // What is actually IN the slots. Without this the app takes dictation and then never
            // mentions the note again — the only way to check it landed was a file manager.
            (Method::Get, p) if p.starts_with("/api/slots/notes") => {
                let (_, query) = split_url(p);
                let limit = query_param(query, "limit")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(50)
                    .clamp(1, 500);
                Ok(json!({ "slots": archive.slot_notes(limit) }))
            }
            // Remove a note. DELETE, not POST: this destroys something in the person's own vault,
            // and the method should say so. The note is named by its text and its file — never by a
            // position in a list, which the file's own editor may have shifted already.
            (Method::Delete, "/api/slots/notes") => {
                let parsed: Value = serde_json::from_str(body).context("тело — JSON")?;
                let slot = parsed
                    .get("slot")
                    .and_then(Value::as_str)
                    .context("нужно поле slot")?;
                // `raw` — the line as it is in the file, not the cleaned text the screen shows.
                let raw = parsed
                    .get("raw")
                    .and_then(Value::as_str)
                    .context("нужно поле raw")?;
                let source = parsed
                    .get("source")
                    .and_then(Value::as_str)
                    .context("нужно поле source")?;
                archive.delete_slot_note(slot, raw, source)?;
                Ok(json!({"ok": true, "deleted": raw}))
            }
            // The shape of the recording for the player's waveform. Real, not drawn: people aim
            // at the loud stretch by it, and a wave that does not match the sound sends them to
            // the wrong place while looking just as trustworthy.
            (Method::Get, p) if p.ends_with("/peaks") => {
                let name = p
                    .strip_prefix("/api/sessions/")
                    .and_then(|r| r.strip_suffix("/peaks"))
                    .context("не найдено")?;
                let buckets = query_param(query, "buckets")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(160);
                archive.peaks(name, buckets)
            }
            // Where the session is RIGHT NOW: the chain of stages, each with its state, its
            // time and — if it broke — its reason.
            (Method::Get, p) if p.ends_with("/progress") => {
                let name = p
                    .strip_prefix("/api/sessions/")
                    .and_then(|r| r.strip_suffix("/progress"))
                    .context("не найдено")?;
                archive.progress(name)
            }
            // Transcript versions: what it was cooked with, when, which is the working
            // one right now.
            (Method::Get, p) if p.ends_with("/versions") => {
                let name = p
                    .strip_prefix("/api/sessions/")
                    .and_then(|r| r.strip_suffix("/versions"))
                    .context("не найдено")?;
                Ok(json!({"versions": archive.versions(name)?}))
            }
            // Delete the session entirely — audio and all derivatives. The ONLY irreversible
            // action here: everywhere else the audio is kept and the rest is recomputable. The
            // body echoes the session name back as confirmation; the guard against deleting a live
            // recording lives in the archive.
            (Method::Delete, p) if p.starts_with("/api/sessions/") => {
                let name = p.strip_prefix("/api/sessions/").context("не найдено")?;
                let parsed: Value = serde_json::from_str(body).unwrap_or(json!({}));
                let confirm = parsed.get("confirm").and_then(Value::as_str).unwrap_or("");
                Ok(json!({"ok": true, "deleted": archive.delete_session(name, confirm)?}))
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
            // Is a recording running right now, and can one be started at all. Without this
            // the app would offer a "record" button while the engine is not even alive — and
            // the press would go nowhere.
            (Method::Get, "/api/record") => Ok(serde_json::to_value(archive.record_state())?),
            // Start recording. Nothing reaches the disk before this — but the last minutes
            // are held in memory and go INTO the session, so a conversation that began
            // before the button is not lost.
            (Method::Post, "/api/record/start") => {
                let parsed: Value = serde_json::from_str(body).unwrap_or(json!({}));
                let title = parsed.get("title").and_then(Value::as_str).unwrap_or("");
                Ok(json!({"ok": true, "msg": archive.start_recording(title)?}))
            }
            // Stop recording — the cook picks the session up at once.
            (Method::Post, "/api/record/stop") => {
                Ok(json!({"ok": true, "session": archive.stop_recording()?}))
            }
            // The settings screen. ONE config file — `.env`: defaults live in the code, `.env`
            // overrides them, and this endpoint edits `.env`. There is no second config file, on
            // purpose: two sources of truth for one fact drift apart the first time either moves.
            //
            // We report what is IN THE FILE, not what is in the daemon's environment. Those differ
            // the moment someone saves — the process keeps the value it started with — and showing
            // the live environment would make a saved change look like it had not been saved.
            (Method::Get, "/api/settings") => {
                let path = localvox_light_core::env_file::resolve_path();
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let in_file = localvox_light_core::env_file::parse(&content);
                let catalogue: Vec<Value> = localvox_light_core::settings::CATALOGUE
                    .iter()
                    .map(|s| {
                        let set = in_file.get(s.key);
                        // Devices are PICKED, not typed. A name entered by hand is a name that can
                        // be misspelled, and the recording then silently opens a different
                        // microphone — or none. Enumerated live: devices come and go.
                        let options: Option<Value> = match s.key {
                            "LOCALVOX_LIGHT_MIC" => {
                                Some(json!(localvox_light_core::audio::input_device_choices()))
                            }
                            "LOCALVOX_LIGHT_LOOPBACK_DEVICE" => {
                                Some(json!(localvox_light_core::audio::output_device_choices()))
                            }
                            // The summary provider is a real choice, so a dropdown — like «Спросить».
                            // Empty selects the default (the local model in the hint).
                            "LOCALVOX_SUMMARY_PROVIDER" => Some(json!([
                                {"value": "claude", "label": "Claude (подписка)"},
                                {"value": "ollama", "label": "Локальная (Ollama)"},
                            ])),
                            _ => None,
                        };
                        json!({
                            "key": s.key,
                            "group": s.group,
                            "label": s.label,
                            "hint": s.hint,
                            "kind": s.kind,
                            // When it starts working. The screen says this BEFORE the save, per
                            // field: «требуется перезапуск» over the whole page was a lie in both
                            // directions — it made live settings look dead and hid the sleeping ones.
                            "applies": s.applies,
                            "options": options,
                            // A secret is never echoed back — only whether it is set. A token that
                            // travels to the screen on every poll is a token in every log and cache
                            // between here and there.
                            "value": if s.secret() { None } else { set.cloned() },
                            "set": set.is_some(),
                        })
                    })
                    .collect();
                // Everything else the file already holds — the raw area. The owner's `.env` carries
                // far more than the catalogue, and a screen that hid it would be lying about what
                // is in effect.
                let known: std::collections::BTreeSet<&str> =
                    localvox_light_core::settings::CATALOGUE.iter().map(|s| s.key).collect();
                let extra: Vec<Value> = in_file
                    .iter()
                    .filter(|(k, _)| !known.contains(k.as_str()))
                    .map(|(k, v)| json!({ "key": k, "value": v }))
                    .collect();
                Ok(json!({
                    "path": path.to_string_lossy(),
                    "exists": path.is_file(),
                    "settings": catalogue,
                    "extra": extra,
                }))
            }
            // Save. The write is surgical — one line per key, every comment and untouched line
            // preserved (`env_file::apply`). A null value unsets the key: it is commented out and
            // the code default takes over again.
            (Method::Post, "/api/settings") => {
                let parsed: Value = serde_json::from_str(body).context("тело — JSON")?;
                let obj = parsed
                    .get("set")
                    .and_then(Value::as_object)
                    .context("нужно поле set — объект {ключ: значение}")?;
                let mut changes: Vec<(String, Option<String>)> = Vec::new();
                for (k, v) in obj {
                    if !localvox_light_core::settings::writable(k) {
                        anyhow::bail!("нельзя писать ключ «{k}»: только LOCALVOX_* и RUST_LOG");
                    }
                    let value = match v {
                        Value::Null => None,
                        Value::String(s) if s.trim().is_empty() => None,
                        Value::String(s) => Some(s.to_string()),
                        Value::Bool(b) => Some(if *b { "on".into() } else { "off".into() }),
                        Value::Number(n) => Some(n.to_string()),
                        other => anyhow::bail!("значение «{k}» должно быть строкой: {other}"),
                    };
                    changes.push((k.clone(), value));
                }
                let path = localvox_light_core::env_file::save(&changes)?;
                // The file is the record; the process is what is running. Both, in that order:
                // if the write fails there is nothing to apply, and a value applied but not
                // saved would vanish on the next start with no trace of why.
                let need_restart = localvox_light_core::settings::apply_live(&changes);
                let msg = if need_restart.is_empty() {
                    "Сохранено и применено".to_string()
                } else {
                    // Named, not counted. «Некоторые настройки требуют перезапуска» sends a
                    // person to compare the whole screen against their memory.
                    format!(
                        "Сохранено. Применится после перезапуска: {}",
                        need_restart.join(", ")
                    )
                };
                Ok(json!({
                    "ok": true,
                    "path": path.to_string_lossy(),
                    "saved": changes.len(),
                    "need_restart": need_restart,
                    "msg": msg,
                }))
            }
            // A link → a session. The session appears in the archive AT ONCE, empty, carrying
            // the stages of its own arrival: a link that vanishes for ten minutes with nothing
            // to look at is indistinguishable from a link that was dropped.
            (Method::Post, "/api/ingest") => {
                let parsed: Value = serde_json::from_str(body).unwrap_or(json!({}));
                let url = parsed
                    .get("url")
                    .and_then(Value::as_str)
                    .context("нужно поле url")?;
                Ok(json!({"ok": true, "session": archive.ingest(url)?}))
            }
            (Method::Post, p) if p.ends_with("/recook") => {
                let rest = p
                    .strip_prefix("/api/sessions/")
                    .and_then(|r| r.strip_suffix("/recook"))
                    .context("не найдено")?;
                // `?scope=summary|text|all` — the tab the person pressed on. Absent → full redo,
                // the old behaviour, so an older client keeps working.
                let scope = crate::archive::RecookScope::from_tab(
                    query_param(query, "scope").as_deref().unwrap_or_default(),
                );
                Ok(json!({"ok": true, "msg": archive.recook(rest, scope)?}))
            }
            // «Всё верно»: человек снимает пометку сомнения, и его слово запоминается.
            //
            // Кнопка в интерфейсе была, а этого адреса — нет: она молча стучалась в никуда.
            (Method::Post, p) if p.ends_with("/confirm") => {
                let name = p
                    .strip_prefix("/api/sessions/")
                    .and_then(|r| r.strip_suffix("/confirm"))
                    .context("не найдено")?;
                let parsed: Value = serde_json::from_str(body).unwrap_or(json!({}));
                let artifact = parsed
                    .get("artifact")
                    .and_then(Value::as_str)
                    .context("нужно поле artifact")?;
                Ok(json!({"ok": true, "msg": archive.confirm(name, artifact)?}))
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
                    // Blocks, not a page: each claim carries the seconds it came from, so the
                    // client can offer to play them. `?format=md` still returns the document.
                    "summary" => {
                        if query_param(query, "format").as_deref() == Some("md") {
                            document(archive.artifact(name, "summary.md")?)
                        } else {
                            archive.summary_blocks(name)
                        }
                    }
                    "summary-unverified" => {
                        document(archive.artifact(name, "summary.unverified.md")?)
                    }
                    // The readable text is DATA. It goes out as lines — who, when, the wording,
                    // and the recognizer's own words wherever the cleanup changed something — so
                    // that a client draws it however it likes and a person can check the model
                    // instead of trusting it. `?format=md` renders the markdown for those who
                    // want a document (export, the clipboard, a script).
                    "processed" => {
                        let dir = archive.session_dir(name)?;
                        // Missing artifact — through the archive, so the answer is the one that
                        // names the command that creates it («… localvox-process <сессия>
                        // --cleanup») instead of a bare «file not found».
                        if !localvox_light_core::readable::exists(&dir) {
                            archive.artifact(name, localvox_light_core::readable::FILE)?;
                        }
                        let lines = localvox_light_core::readable::lines(&dir)?;
                        if query_param(query, "format").as_deref() == Some("md") {
                            Ok(json!({"markdown": localvox_light_core::readable::render_markdown(&lines)}))
                        } else {
                            let r = localvox_light_core::readable::load(&dir)?;
                            Ok(json!({
                                "lines": lines,
                                "version_id": r.version_id,
                                "provenance": r.provenance,
                                "omitted": r.omitted,
                                "rejected": r.rejected,
                            }))
                        }
                    }
                    "processed-unverified" => {
                        document(archive.artifact(name, "processed.unverified.md")?)
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

/// A derived document, with its provenance header taken off the text and handed over as a fact.
///
/// The header stays in the FILE — it is the only record of what produced this text, and a person
/// reading the file outside the app must still find it. It is off the `markdown` because a
/// comment carrying our own bookkeeping has no business inside a document someone forwards to
/// colleagues: it used to render as the document's first paragraph and travelled with every copy.
fn document(text: String) -> anyhow::Result<serde_json::Value> {
    let (prov, body) = localvox_light_core::provenance::split(&text);
    Ok(json!({"markdown": body, "provenance": prov}))
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
        // With the provenance header a real summary carries — the endpoint must hand it over as a
        // fact, not as the document's first paragraph.
        fs::write(
            session.join("summary.md"),
            "<!-- localvox: summary-ru | модель qwen3.5:9b | глоссарий: 0 замен | 2026-07-11T10:00:00+03:00 -->\n\n## Решения\n* всё хорошо [[0]]\n",
        )
        .unwrap();
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

    /// The app must be INSIDE the binary. If the embedded folder is empty, the daemon
    /// starts and serves a blank page — and nothing anywhere says why.
    #[test]
    fn app_is_embedded() {
        let index = WebApp::get("index.html").expect("ui/dist/index.html is not embedded");
        assert!(!index.data.is_empty());
    }

    /// A module script served as text/plain is refused by the browser, and the app shows
    /// an empty page with no error to be found.
    #[test]
    fn assets_get_their_content_type() {
        assert_eq!(content_type("assets/index-a1b2.js"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("assets/index-a1b2.css"), "text/css; charset=utf-8");
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
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
        // Blocks by default: a claim is a thing with a source, not a line of a page.
        let blocks = v["blocks"].as_array().expect("no blocks");
        assert_eq!(blocks[0]["kind"], "heading");
        assert_eq!(blocks[0]["text"], "Решения");
        assert_eq!(blocks[1]["kind"], "bullet");
        // The citation marker is out of the text — it is provenance, not something to read.
        assert_eq!(blocks[1]["text"], "всё хорошо");
        assert!(
            !v.to_string().contains("<!--"),
            "the bookkeeping comment travelled inside the document"
        );
        assert_eq!(v["provenance"]["model"], "qwen3.5:9b");
        assert_eq!(v["provenance"]["template"], "summary-ru");

        // `?format=md` still hands over the document, markers and all — that is the file.
        let (code, doc) = respond(
            &a,
            &cfg(None),
            &Method::Get,
            "/api/sessions/20260711_http/summary?format=md",
            None,
            true,
            "format=md",
        );
        assert_eq!(code, 200);
        assert!(doc["markdown"].as_str().unwrap().contains("всё хорошо"));
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
