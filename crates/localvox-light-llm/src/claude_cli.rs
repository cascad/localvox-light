//! Claude Code as a SUBPROCESS provider — the safe way to use a Claude subscription.
//!
//! WHY A SUBPROCESS AND NOT AN HTTP FLAVOR. There is no official SDK that reaches a Pro/Max
//! subscription without the Claude Code runtime, and there is no way to use a ChatGPT/Claude
//! *subscription* through the plain API (that is an API key, billed per token). The one path
//! Anthropic sanctions for a subscription is the official `claude` binary in non-interactive
//! mode (`claude -p`), which draws from the plan's own limits. So we invoke THAT — the same tool
//! the person already logged in — exactly as `yt-dlp` and `localvox-process` are invoked.
//!
//! WHAT WE DELIBERATELY DO NOT DO. We never lift the subscription's OAuth token out and call
//! `api.anthropic.com` with it, and we never drive the web UI in a browser. Both are the patterns
//! Anthropic actively bans (measured: third-party tools carrying the OAuth token were cut off with
//! «This credential is only authorized for use with Claude Code»). Invoking the official binary is
//! not that: the request to Anthropic comes from Claude Code itself.
//!
//! TWO NON-OBVIOUS INVOCATION FACTS, both verified against the Claude Code docs:
//!   * NO `--bare`. Bare mode is faster but «skips OAuth and keychain reads» — it REQUIRES an API
//!     key and therefore cannot use the subscription. We must run without it.
//!   * NEUTRAL working directory. Without `--bare`, `claude -p` auto-discovers `CLAUDE.md`, MCP
//!     servers and hooks from the cwd. Run from the repo and it would inherit our project context
//!     (and behave differently on every machine). We run it in a dedicated empty directory so the
//!     answer depends only on the flags we pass.
//!
//! PROXY. Claude Code is Node/undici and honours `HTTPS_PROXY` / `HTTP_PROXY` / `NO_PROXY`
//! natively; a TLS-inspecting proxy additionally needs `NODE_EXTRA_CA_CERTS`. «Работать через
//! указываемый прокси при запуске» = we put those into the CHILD's environment at spawn time. The
//! owner configures one `LOCALVOX_*` variable (single-`.env` policy); it is mapped to the standard
//! env vars here, at the edge.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// How to invoke the official `claude` CLI. All entropy (proxy, paths, model) is filled at the
/// composition root — see [`ClaudeCliConfig::from_env`]; the rest of this module is deterministic.
#[derive(Clone, Debug)]
pub struct ClaudeCliConfig {
    /// The `claude` executable. `claude` by default (resolved on `PATH`); an absolute path when
    /// the owner installed it somewhere non-standard.
    pub binary: String,
    /// `--model` (e.g. `sonnet`, `opus`). `None` — the plan's default model, whatever the CLI picks.
    pub model: Option<String>,
    /// HTTP(S) proxy for the child, e.g. `http://127.0.0.1:8080`. Mapped to `HTTPS_PROXY` and
    /// `HTTP_PROXY`. `None` — no proxy injected (the child still inherits any ambient one).
    pub proxy: Option<String>,
    /// Hosts that must bypass the proxy — mapped to `NO_PROXY`.
    pub no_proxy: Option<String>,
    /// Extra CA bundle for a TLS-inspecting proxy — mapped to `NODE_EXTRA_CA_CERTS`. Without it,
    /// such a proxy makes the child fail with an opaque SSL error.
    pub ca_certs: Option<PathBuf>,
    /// A long-lived OAuth token from `claude setup-token` (one year), mapped to
    /// `CLAUDE_CODE_OAUTH_TOKEN`. `None` — the child uses the credentials the person left with
    /// `/login` (macOS Keychain / `~/.claude/.credentials.json`), which is the normal case when
    /// Claude Code is already installed and signed in on this machine. The token is a SECRET: it
    /// must never be echoed back through the settings API.
    pub oauth_token: Option<String>,
    /// Hard ceiling on one call. A subscription request that wedges must not hold a slot forever.
    pub timeout: Duration,
    /// The NEUTRAL directory the child runs in (see the module docs). Created if missing.
    pub workdir: PathBuf,
    pub process_scope: localvox_light_process::Scope,
}

impl ClaudeCliConfig {
    /// Read the configuration from the environment. The ONE place that touches `std::env`.
    ///
    /// Only `LOCALVOX_*` names, so the settings screen (which may write these through the API) can
    /// never be used to set `PATH` or another process-wide variable — the same boundary as the
    /// rest of the config.
    pub fn from_env() -> Self {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        ClaudeCliConfig {
            process_scope: if get(localvox_light_process::CONTAINED_ENV).as_deref() == Some("1") {
                localvox_light_process::Scope::Inherit
            } else {
                localvox_light_process::Scope::Tree
            },
            binary: get("LOCALVOX_CLAUDE_BIN").unwrap_or_else(|| "claude".into()),
            model: get("LOCALVOX_CLAUDE_MODEL"),
            proxy: get("LOCALVOX_CLAUDE_PROXY"),
            no_proxy: get("LOCALVOX_CLAUDE_NO_PROXY"),
            ca_certs: get("LOCALVOX_CLAUDE_CA").map(PathBuf::from),
            oauth_token: get("LOCALVOX_CLAUDE_OAUTH_TOKEN"),
            timeout: Duration::from_secs(
                get("LOCALVOX_CLAUDE_TIMEOUT_SEC")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(300),
            ),
            workdir: get("LOCALVOX_CLAUDE_WORKDIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("localvox-claude-cwd")),
        }
    }
}

/// The exact process to run — program, arguments, environment overrides and cwd. Built by a PURE
/// function ([`build_spec`]) so the whole invocation can be asserted in a test without a `claude`
/// binary anywhere near the machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub program: String,
    pub args: Vec<String>,
    /// Environment variables ADDED to the child (not the full environment).
    pub envs: Vec<(String, String)>,
    /// Environment variables REMOVED from the child. `ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN`
    /// always go here: in `-p` mode Claude Code uses an API key whenever one is present in the
    /// environment (auth precedence), so a stray key would silently divert this subscription
    /// provider onto paid, per-token API billing. We strip them so «via the subscription» stays true.
    pub env_remove: Vec<String>,
    pub cwd: PathBuf,
}

/// Turn a config and a prompt into the concrete command. Deterministic: same inputs → same spec,
/// including argument ORDER (the prompt must sit right after `-p`), which is why the prompt lives
/// here rather than being tacked on at spawn time.
pub fn build_spec(cfg: &ClaudeCliConfig, prompt: &str) -> Spec {
    // `-p <prompt>` — non-interactive with the instruction; `--output-format json` — a parseable
    // envelope with `.result` and the per-call cost. NO `--bare`: it would disable the subscription
    // auth we depend on.
    let mut args = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--output-format".to_string(),
        "json".to_string(),
    ];
    if let Some(m) = &cfg.model {
        args.push("--model".to_string());
        args.push(m.clone());
    }

    let mut envs: Vec<(String, String)> = Vec::new();
    if let Some(p) = &cfg.proxy {
        // Both cases: undici prefers the uppercase names, but some layers read the lowercase ones.
        for k in ["HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"] {
            envs.push((k.to_string(), p.clone()));
        }
    }
    if let Some(np) = &cfg.no_proxy {
        envs.push(("NO_PROXY".to_string(), np.clone()));
        envs.push(("no_proxy".to_string(), np.clone()));
    }
    if let Some(ca) = &cfg.ca_certs {
        envs.push((
            "NODE_EXTRA_CA_CERTS".to_string(),
            ca.to_string_lossy().into_owned(),
        ));
    }
    // A long-lived token, if the owner set one up for an unattended daemon. When absent, the child
    // falls back to the stored `/login` credentials — the normal case here (Claude Code is already
    // signed in on this machine).
    if let Some(tok) = &cfg.oauth_token {
        envs.push(("CLAUDE_CODE_OAUTH_TOKEN".to_string(), tok.clone()));
    }

    Spec {
        program: cfg.binary.clone(),
        args,
        envs,
        // Guarantee the subscription is what answers — never a stray API key (see the field docs).
        env_remove: vec![
            "ANTHROPIC_API_KEY".to_string(),
            "ANTHROPIC_AUTH_TOKEN".to_string(),
        ],
        cwd: cfg.workdir.clone(),
    }
}

/// One answer from the CLI.
#[derive(Clone, Debug)]
pub struct Answer {
    /// The model's reply text — the `.result` field of the JSON envelope.
    pub text: String,
    /// What the call cost, if the CLI reported it (`.total_cost_usd`). Kept for provenance and so
    /// an operator can see spend per request without the usage dashboard.
    pub cost_usd: Option<f64>,
}

/// The relevant fields of `claude -p --output-format json`. Lenient on purpose: the envelope
/// carries session id, usage and more, and gains fields between versions — we read only what we
/// need and ignore the rest.
#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    total_cost_usd: Option<f64>,
    #[serde(default)]
    is_error: Option<bool>,
    #[serde(default)]
    subtype: Option<String>,
}

/// Parse the JSON envelope. Pure — tested without invoking anything.
pub fn parse_answer(stdout: &str) -> Result<Answer> {
    let env: Envelope = serde_json::from_str(stdout.trim())
        .with_context(|| format!("claude did not return JSON (got: {})", first_line(stdout)))?;
    if env.is_error.unwrap_or(false) {
        bail!(
            "claude reported an error ({})",
            env.subtype.unwrap_or_else(|| "unknown".into())
        );
    }
    let text = env.result.unwrap_or_default().trim().to_string();
    if text.is_empty() {
        bail!("claude returned an empty result");
    }
    Ok(Answer {
        text,
        cost_usd: env.total_cost_usd,
    })
}

fn first_line(s: &str) -> String {
    s.trim().lines().next().unwrap_or("").chars().take(200).collect()
}

/// Ask Claude. `prompt` is the instruction (goes to `-p`); `content` is the material to work on
/// (a file's text, a fetched page, pasted text) and is piped to stdin — the docs' own pattern
/// (`cat file | claude -p '…'`). `content` up to ~10 MB; larger callers should summarise first.
///
/// The shared supervisor bounds the call and owns its descendants. File-backed stdio allows a
/// large input and response without pipe deadlocks or detached drain threads.
pub fn run(cfg: &ClaudeCliConfig, prompt: &str, content: Option<&str>) -> Result<Answer> {
    use localvox_light_process::{Limits, Termination};
    let spec = build_spec(cfg, prompt);
    std::fs::create_dir_all(&spec.cwd)
        .with_context(|| format!("creating the claude work dir {}", spec.cwd.display()))?;
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).current_dir(&spec.cwd).envs(spec.envs);
    for key in &spec.env_remove {
        cmd.env_remove(key);
    }
    let mut limits = Limits::new(cfg.timeout);
    limits.scope = cfg.process_scope;
    let output = localvox_light_process::run(
        cmd,
        content.map(str::as_bytes),
        limits,
        || false,
        |pid, elapsed| {
            tracing::info!(
                pid,
                elapsed_sec = elapsed.as_secs(),
                "claude: waiting for response"
            )
        },
    )
    .with_context(|| format!("launching `{}` (LOCALVOX_CLAUDE_BIN)", spec.program))?;
    match output.termination {
        Termination::Exited(status) if status.success() => {
            parse_answer(&String::from_utf8_lossy(&output.stdout))
        }
        Termination::Exited(status) => bail!(
            "claude exited with {status}: {}",
            first_line(&String::from_utf8_lossy(&output.stderr))
        ),
        Termination::TimedOut => bail!("claude did not answer within {}s", cfg.timeout.as_secs()),
        Termination::OutputLimit => bail!(
            "claude exceeded the {} byte output limit",
            limits.output_bytes
        ),
        Termination::Cancelled => bail!("claude cancelled"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ClaudeCliConfig {
        ClaudeCliConfig {
            binary: "claude".into(),
            model: None,
            proxy: None,
            no_proxy: None,
            ca_certs: None,
            oauth_token: None,
            timeout: Duration::from_secs(300),
            workdir: PathBuf::from("/tmp/x"),
            process_scope: localvox_light_process::Scope::Tree,
        }
    }

    /// The invocation must be non-interactive, JSON, with the prompt right after `-p`, and — the
    /// load-bearing detail — must NOT carry `--bare`, which would silently drop us off the
    /// subscription onto an API key.
    #[test]
    fn spec_is_headless_json_prompt_ordered_and_never_bare() {
        let s = build_spec(&cfg(), "суммируй это");
        assert_eq!(s.args[0], "-p");
        assert_eq!(s.args[1], "суммируй это", "prompt must sit right after -p");
        assert_eq!(s.args.windows(2).find(|w| w[0] == "--output-format").map(|w| &w[1]), Some(&"json".to_string()));
        assert!(!s.args.iter().any(|a| a == "--bare"), "--bare disables subscription auth");
    }

    /// A proxy in the config becomes the standard Node/undici env vars ON THE CHILD — this is the
    /// whole of «работать через указываемый прокси при запуске».
    #[test]
    fn proxy_maps_to_child_env() {
        let mut c = cfg();
        c.proxy = Some("http://127.0.0.1:8080".into());
        c.no_proxy = Some("localhost".into());
        let s = build_spec(&c, "x");
        let has = |k: &str, v: &str| s.envs.iter().any(|(a, b)| a == k && b == v);
        assert!(has("HTTPS_PROXY", "http://127.0.0.1:8080"));
        assert!(has("HTTP_PROXY", "http://127.0.0.1:8080"));
        assert!(has("NO_PROXY", "localhost"));
    }

    /// No proxy configured → we add nothing (the child keeps whatever ambient proxy exists).
    #[test]
    fn no_proxy_config_adds_no_env() {
        assert!(build_spec(&cfg(), "x").envs.is_empty());
    }

    /// The subscription must be what answers: a stray API key in the environment would, in `-p`
    /// mode, silently divert the call onto paid API billing — so we always strip those two vars.
    #[test]
    fn api_key_vars_are_always_stripped_from_the_child() {
        let s = build_spec(&cfg(), "x");
        assert!(s.env_remove.contains(&"ANTHROPIC_API_KEY".to_string()));
        assert!(s.env_remove.contains(&"ANTHROPIC_AUTH_TOKEN".to_string()));
    }

    /// A configured long-lived token is handed to the child as `CLAUDE_CODE_OAUTH_TOKEN`; without
    /// one, we add nothing and the child uses the stored `/login` credentials.
    #[test]
    fn oauth_token_maps_to_child_env_only_when_set() {
        assert!(!build_spec(&cfg(), "x")
            .envs
            .iter()
            .any(|(k, _)| k == "CLAUDE_CODE_OAUTH_TOKEN"));
        let mut c = cfg();
        c.oauth_token = Some("sk-oauth-xyz".into());
        let s = build_spec(&c, "x");
        assert!(s
            .envs
            .iter()
            .any(|(k, v)| k == "CLAUDE_CODE_OAUTH_TOKEN" && v == "sk-oauth-xyz"));
    }

    /// A model is passed through as `--model <name>`.
    #[test]
    fn model_is_forwarded() {
        let mut c = cfg();
        c.model = Some("opus".into());
        let s = build_spec(&c, "x");
        assert_eq!(
            s.args.windows(2).find(|w| w[0] == "--model").map(|w| &w[1]),
            Some(&"opus".to_string())
        );
    }

    #[test]
    fn parses_result_and_cost() {
        let a = parse_answer(r#"{"result":"  привет  ","total_cost_usd":0.0123,"session_id":"x"}"#).unwrap();
        assert_eq!(a.text, "привет");
        assert_eq!(a.cost_usd, Some(0.0123));
    }

    /// A REAL `claude -p --output-format json` envelope (v2.1.220, trimmed), captured live on the
    /// owner's machine. Locks the actual shape into the suite and proves the parser stays lenient
    /// against the pile of metadata fields the CLI emits around `result`.
    #[test]
    fn parses_the_real_cli_envelope() {
        let real = r#"{"is_error":false,"duration_api_ms":3761,"num_turns":1,"stop_reason":"end_turn","session_id":"f0040339","total_cost_usd":0.187204,"usage":{"input_tokens":2,"output_tokens":4},"permission_denials":[],"subtype":"success","api_error_status":null,"result":"OK","type":"result"}"#;
        let a = parse_answer(real).unwrap();
        assert_eq!(a.text, "OK");
        assert_eq!(a.cost_usd, Some(0.187204));
    }

    /// An error envelope is a failure, not an empty answer — and it names the subtype.
    #[test]
    fn error_envelope_is_an_error() {
        let e = parse_answer(r#"{"is_error":true,"subtype":"rate_limit"}"#).unwrap_err();
        assert!(e.to_string().contains("rate_limit"), "{e}");
    }

    /// Non-JSON (e.g. the CLI printed a human error) is reported as such, with a snippet.
    #[test]
    fn non_json_is_reported() {
        let e = parse_answer("Error: not logged in").unwrap_err();
        assert!(e.to_string().contains("did not return JSON"), "{e}");
    }

    /// An empty result is a failure — a blank answer must never look like success.
    #[test]
    fn empty_result_fails() {
        assert!(parse_answer(r#"{"result":"   "}"#).is_err());
    }
}
