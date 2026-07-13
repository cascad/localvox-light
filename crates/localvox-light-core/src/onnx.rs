//! The shared setup of the ONNX Runtime — one per process.
//!
//! **Why this exists.** The `ort` crate creates the ONNX Runtime environment with the
//! VERBOSE logging level and offloads the filtering onto `tracing`, that is, onto us. As a
//! result, on every inference the log was showered with the internal bookkeeping of
//! somebody else's allocator: «Reserving memory in BFCArena for Cpu size: 24», «Allocated
//! memory at 000001D0…», «GraphTransformer … modified» — thousands of lines per single
//! cook.
//!
//! Filtering that in the log subscriber is possible, but that is treating the symptom: the
//! lines have already been produced, have gone through FFI, have been formatted and only
//! then thrown away. And catching them by substrings («BFCArena», «Allocated memory») is a
//! blacklist that will fall behind the next version of the library forever.
//!
//! Here is the treatment of the cause: the ONNX Runtime gets ITS OWN logging level and
//! simply does not produce what we did not ask for. Errors (a model that did not load, a
//! tensor shape that did not match) arrive at warning and above — they get through, and
//! they must get through.
//!
//! `LOCALVOX_ONNX_LOG=info|verbose` brings it all back: silence must not turn into an
//! inability to look.

use std::sync::Once;

static INIT: Once = Once::new();

/// Set up the ONNX Runtime. Idempotent: it is called from every point where a session is
/// created (ASR, NER, diarization) and does the work exactly once.
pub fn init() {
    INIT.call_once(|| {
        let level = match std::env::var("LOCALVOX_ONNX_LOG")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "verbose" | "trace" => ort::logging::LogLevel::Verbose,
            "info" => ort::logging::LogLevel::Info,
            "error" => ort::logging::LogLevel::Error,
            _ => ort::logging::LogLevel::Warning,
        };
        match ort::environment::current() {
            Ok(env) => env.set_log_level(level),
            // We could not set it up — that is no reason not to work: the models will load
            // anyway, the log will just be chattier. But we must say so.
            Err(e) => tracing::warn!("ONNX Runtime: the logging level was not set ({e})"),
        }
    });
}
