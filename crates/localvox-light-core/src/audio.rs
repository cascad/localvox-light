//! Audio capture: microphone and loopback (system sound).
//! Sends 16 kHz mono i16 PCM chunks to the pipeline via crossbeam channel.

#[cfg(windows)]
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use std::str::FromStr;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::DeviceId;
use crossbeam_channel::Sender;

use crate::events::UiMsg;

pub const SAMPLE_RATE: u32 = 16000;
pub const CHUNK_FRAMES: usize = 512;

/// How many samples of each source actually ARRIVED from the device and went into the
/// channel.
///
/// Counted HERE, at the entrance — and that is the whole point. Loss of a recording is a
/// question about the DEVICE: did the sound reach us at all. Everything past this line is
/// our own queue, and a queue is a delay, not a loss.
///
/// The integrity watchdog used to count on the consumer side, and it lied: when the cook
/// saturated the cores, the consumer fell behind, the audio piled up in the (unbounded)
/// channel — and the watchdog shouted «RECORDING LOSS» about audio that was safely sitting
/// in memory. A warning that cries wolf is worse than no warning: it stops being read.
///
/// A process-global counter is honest here: one process records exactly one workspace (the
/// single-instance lock guarantees it).
static CAPTURED: [std::sync::atomic::AtomicU64; 2] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// Samples that came from the device for this source since the process started.
pub fn captured(source_id: u8) -> u64 {
    CAPTURED
        .get(source_id as usize)
        .map(|c| c.load(std::sync::atomic::Ordering::Relaxed))
        .unwrap_or(0)
}

fn count_captured(source_id: u8, n: usize) {
    if let Some(c) = CAPTURED.get(source_id as usize) {
        c.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
    }
}

pub struct PcmChunk {
    pub source_id: u8,
    pub samples: Vec<i16>,
}

/// RMS → 0..1 for an 8-block meter (as in client-reliable: `rms/32768*12`, min 1.0).
pub fn pcm_level_i16(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let rms: f64 =
        (samples.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / samples.len() as f64).sqrt();
    (rms / 32768.0 * 12.0).min(1.0) as f32
}

/// Display line for a device (name + manufacturer + id) — as in client-reliable.
pub fn format_device_display(dev: &cpal::Device, name: &str, extra: &str) -> String {
    let id_str = dev
        .id()
        .map(|id| format!("{id}"))
        .unwrap_or_else(|_| "?".into());
    let mut parts = vec![name.to_string()];
    if let Ok(desc) = dev.description() {
        if let Some(mfr) = desc.manufacturer() {
            let mfr = mfr.trim();
            if !mfr.is_empty() && mfr != name {
                parts.push(mfr.to_string());
            }
        }
    }
    parts.push(format!("id:{id_str}"));
    if !extra.is_empty() {
        parts.push(extra.to_string());
    }
    parts.join(" | ")
}

pub fn collect_input_devices() -> Vec<(cpal::Device, String)> {
    let host = cpal::default_host();
    host.input_devices()
        .unwrap_or_else(|_| panic!("input_devices"))
        .filter_map(|dev| {
            let name = dev.description().ok()?.name().to_string();
            #[cfg(target_os = "macos")]
            if name.contains("Cpal loopback") || name.contains("cpal output recorder") {
                return None;
            }
            Some((dev, name))
        })
        .collect()
}

fn fallback_output_list_default_only() -> Vec<(usize, String)> {
    vec![(0, "default-output".to_string())]
}

pub fn list_output_device_names() -> Vec<(usize, String)> {
    #[cfg(windows)]
    {
        // CoCreateInstance(MMDeviceEnumerator) fails without CoInitializeEx on this thread;
        // loopback in its own thread calls initialize_mta, the TUI main thread does not.
        let _ = wasapi::initialize_mta();
        let enumerator = match wasapi::DeviceEnumerator::new() {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = ?e, "WASAPI DeviceEnumerator (loopback list)");
                return fallback_output_list_default_only();
            }
        };
        let collection = match enumerator.get_device_collection(&wasapi::Direction::Render) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = ?e, "WASAPI EnumAudioEndpoints Render");
                return fallback_output_list_default_only();
            }
        };
        let mut list: Vec<(usize, String)> = collection
            .into_iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let dev = r.ok()?;
                dev.get_friendlyname().ok().map(|n| (i + 1, n))
            })
            .collect();
        list.insert(0, (0, "default-output".to_string()));
        list
    }
    #[cfg(not(windows))]
    {
        let devices = match cpal::default_host().output_devices() {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = ?e, "cpal output_devices");
                return fallback_output_list_default_only();
            }
        };
        let mut list: Vec<(usize, String)> = devices
            .enumerate()
            .filter_map(|(i, dev)| {
                dev.description()
                    .ok()
                    .map(|d| (i + 1, d.name().to_string()))
            })
            .collect();
        list.insert(0, (0, "default-output".to_string()));
        list
    }
}

/// Sources for capturing "system" sound (non-Windows).
///
/// **macOS (CPAL ≥ 0.17, Sequoia+):** loopback is not a separate input in the list, it is
/// `build_input_stream` on an **output** device (speakers). So what is enumerated here are
/// **outputs** with `supports_output()`; priority goes to devices **without** a physical
/// input (`!supports_input()`), otherwise CPAL opens the microphone rather than the tap.
///
/// **Linux:** virtual monitor inputs (PipeWire/Pulse), the substring `monitor` in the name.
///
/// The enumeration order may shift slightly — better to store `device.id()` in the config.
#[cfg(all(not(windows), target_os = "macos"))]
pub fn list_loopback_capture_devices() -> Vec<(cpal::Device, String)> {
    let host = cpal::default_host();
    let Ok(iter) = host.output_devices() else {
        return Vec::new();
    };
    // Do not drop a device if description() is temporarily empty — otherwise the "loopback"
    // list can come out empty while outputs are alive (rare, but seen on some builds/OSes).
    let outs: Vec<(cpal::Device, String)> = iter
        .map(|dev| {
            let name = dev
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| {
                    dev.id()
                        .map(|id| format!("(name unavailable) {id}"))
                        .unwrap_or_else(|_| "(unnamed device)".into())
                });
            (dev, name)
        })
        .collect();
    // Prefer pure outputs: otherwise on a combo device CPAL opens the microphone, not the
    // tap from the output.
    let output_only: Vec<(cpal::Device, String)> = outs
        .iter()
        .filter(|(d, _)| !d.supports_input())
        .cloned()
        .collect();
    if !output_only.is_empty() {
        return output_only;
    }
    outs
}

/// If `list_loopback_capture_devices` is empty under `--list-devices` on macOS — a short hint.
#[cfg(all(not(windows), target_os = "macos"))]
pub fn macos_loopback_empty_hint() -> Option<String> {
    let host = cpal::default_host();
    match host.output_devices() {
        Err(e) => Some(format!(
            "cpal could not enumerate outputs: {e}. Usually this is not about the \"device name\" but about the environment (sandbox, no GUI session) or access restrictions. To record system audio on macOS 14.6+, the terminal/IDE often needs the \"Microphone\" permission (Settings → Privacy & Security)."
        )),
        Ok(iter) => {
            let n = iter.count();
            if n == 0 {
                Some(
                    "CoreAudio returned 0 outputs (inputs may still be visible). Check: not SSH without an audio session, not a stripped-down VM; the built-in speakers are enabled in \"Sound\".".into(),
                )
            } else {
                None
            }
        }
    }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn list_loopback_capture_devices() -> Vec<(cpal::Device, String)> {
    let host = cpal::default_host();
    let Ok(iter) = host.input_devices() else {
        return Vec::new();
    };
    iter.filter_map(|dev| {
        let name = dev.description().ok()?.name().to_string();
        let n = name.to_lowercase();
        if n.contains("monitor") {
            Some((dev, name))
        } else {
            None
        }
    })
    .collect()
}

/// Stable CPAL identifier to store in the config (independent of the OS language).
pub fn device_id_save_token(dev: &cpal::Device) -> Option<String> {
    dev.id().ok().map(|id| id.to_string())
}

/// One pickable device: what to WRITE into the config, and what to SHOW.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceChoice {
    /// The value that goes into `.env`. For inputs this is the STABLE ID, not an index and not a
    /// name: indices shift the moment anything is plugged in, and a headset that reconnects can
    /// come back under a slightly different name — either would silently point the recording at
    /// somebody else's microphone.
    pub value: String,
    /// What the person reads.
    pub label: String,
    /// True for the entry that means «whatever the system default is». It is a real choice, and
    /// often the right one: it follows the headset the person actually plugs in.
    pub is_default: bool,
}

/// A short, stable fragment of a device id — just enough to tell two same-named devices apart.
///
/// The LAST characters, not the first: a WASAPI id looks like
/// `wasapi:{0.0.1.00000000}.{4029964e-…}`, and everything up to the second brace is identical
/// across devices. A prefix would print the same thing twice and solve nothing.
fn id_tail(value: &str) -> String {
    let core: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let n = core.chars().count();
    core.chars().skip(n.saturating_sub(6)).collect()
}

/// Microphones, for the settings screen.
///
/// Same pair `print_devices()` prints in the CLI — the stable id plus the human name. Enumerated on
/// request rather than cached: devices come and go while the daemon runs, and a cached list would
/// offer a microphone that is no longer there.
pub fn input_device_choices() -> Vec<DeviceChoice> {
    let mut out = vec![DeviceChoice {
        value: "default".into(),
        label: "по умолчанию (системный)".into(),
        is_default: true,
    }];
    let found = collect_input_devices();
    // Windows hands out several inputs literally called «Микрофон» — measured on the owner's
    // machine, two of them. Their ids differ, so the CHOICE is unambiguous, but the list is not:
    // picking between two identical lines is a coin toss. Only the ambiguous ones get the
    // manufacturer appended — decorating every entry would add noise to answer a question nobody
    // asked.
    let mut seen: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (_, name) in &found {
        *seen.entry(name.as_str()).or_insert(0) += 1;
    }
    for (dev, name) in &found {
        let ambiguous = seen.get(name.as_str()).copied().unwrap_or(0) > 1;
        let maker = dev
            .description()
            .ok()
            .and_then(|d| d.manufacturer().map(str::to_string))
            .filter(|m| !m.trim().is_empty());
        // No stable id — the device is still offered under its name, which is what the resolver
        // falls back to anyway. Dropping it would hide a working microphone.
        let value = device_id_save_token(dev).unwrap_or_else(|| name.clone());
        let label = match (ambiguous, maker) {
            (false, _) => name.clone(),
            (true, Some(m)) => format!("{name} — {m}"),
            // Nothing readable tells them apart: on this machine WASAPI reports no manufacturer,
            // and the two «Микрофон» differ only by GUID. So a piece of that GUID goes on screen.
            // Ugly, and still the right answer — it belongs to the DEVICE. Numbering them «(1)»
            // and «(2)» would read better and lie: enumeration order is not guaranteed, so the
            // number could point at the other microphone tomorrow, which is the exact failure the
            // stable id was chosen to prevent.
            (true, None) => format!("{name} · {}", id_tail(&value)),
        };
        out.push(DeviceChoice {
            value,
            label,
            is_default: false,
        });
    }
    out
}

/// Outputs whose sound can be captured (loopback).
///
/// Here the NAME is the value: that is what the loopback resolver matches on, and index 0 is
/// literally called `default-output`.
pub fn output_device_choices() -> Vec<DeviceChoice> {
    list_output_device_names()
        .into_iter()
        .map(|(i, name)| DeviceChoice {
            is_default: i == 0,
            label: if i == 0 {
                "по умолчанию (то, что звучит в колонках)".into()
            } else {
                name.clone()
            },
            value: name,
        })
        .collect()
}

fn resolve_input_device_by_id_str(id_str: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    let id = DeviceId::from_str(id_str.trim())
        .with_context(|| format!("Invalid CPAL device id: {id_str}"))?;
    let dev = host
        .device_by_id(&id)
        .with_context(|| format!("No device for id {id_str} (unplugged, or a different host name)"))?;
    anyhow::ensure!(
        dev.supports_input(),
        "Device {id_str} does not support input"
    );
    Ok(dev)
}

/// For loopback: on macOS the id refers to an **output** (speakers), `supports_input` is not
/// required.
#[cfg(not(windows))]
fn resolve_device_by_id_for_loopback(id_str: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    let id = DeviceId::from_str(id_str.trim())
        .with_context(|| format!("Invalid CPAL device id: {id_str}"))?;
    host.device_by_id(&id)
        .with_context(|| format!("No device for id {id_str} (unplugged, or a different host name)"))
}

/// Does this setting FOLLOW the system default, or is it pinned to a device the human chose?
///
/// A pinned device is a decision, and we must never second-guess it: if someone said "record from
/// this mixer", they meant that mixer, even when Windows thinks otherwise. Following makes sense
/// only where the human said "whatever the system uses".
pub fn follows_default(query: &str) -> bool {
    let q = query.trim();
    q.is_empty() || q.eq_ignore_ascii_case("default") || q.eq_ignore_ascii_case("default-output")
}

/// Identity of the CURRENT default playback device — the one the system sends sound to now.
///
/// THIS IS THE THING THAT MOVES UNDER OUR FEET. The loopback stream binds to a device ONCE, when
/// it starts. You join a meeting, a headset connects, Windows makes it the default, the call app
/// follows it — and our stream keeps listening to the old, now-idle endpoint. WASAPI loopback
/// sends no packets when nothing plays there, so we dutifully pad with silence and record nothing
/// at all, without a single complaint. Measured on the owner's recording (16.07.2026): the other
/// side was audible for two minutes, then 27 minutes of silence on a full-length track.
///
/// `None` — we could not ask; the caller must treat that as "unknown", not as "changed".
#[cfg(windows)]
pub fn default_render_id() -> Option<String> {
    // COM is per-thread; the watcher calls this from its own thread. Repeat calls are harmless.
    let _ = wasapi::initialize_mta();
    let enumerator = wasapi::DeviceEnumerator::new().ok()?;
    let dev = enumerator
        .get_default_device(&wasapi::Direction::Render)
        .ok()?;
    dev.get_id().ok()
}

#[cfg(not(windows))]
pub fn default_render_id() -> Option<String> {
    cpal::default_host().default_output_device()?.name().ok()
}

/// Identity of the current default microphone — it moves for the same reasons (a headset is one
/// device: reconnect it and both the mic and the speakers change under you).
pub fn default_capture_id() -> Option<String> {
    cpal::default_host().default_input_device()?.name().ok()
}

pub fn resolve_mic(query: &str) -> Result<cpal::Device> {
    let q = query.trim();
    if q.eq_ignore_ascii_case("default") {
        return cpal::default_host()
            .default_input_device()
            .context("No default input device");
    }
    if DeviceId::from_str(q).is_ok() {
        return resolve_input_device_by_id_str(q);
    }
    if let Some(rest) = q.strip_prefix("micidx:") {
        let idx: usize = rest
            .parse()
            .with_context(|| format!("micidx: a number was expected, got {rest}"))?;
        return collect_input_devices()
            .into_iter()
            .nth(idx)
            .map(|(d, _)| d)
            .with_context(|| format!("micidx:{idx} — no such index in the microphone list"));
    }
    let devices = collect_input_devices();
    if let Ok(idx) = q.parse::<usize>() {
        return devices
            .into_iter()
            .nth(idx)
            .map(|(d, _)| d)
            .context(format!("Input device index {idx} not found"));
    }
    let needle = q.to_lowercase();
    devices
        .into_iter()
        .find(|(_, name)| name.to_lowercase().contains(&needle))
        .map(|(d, _)| d)
        .context(format!("Input device '{query}' not found"))
}

#[cfg(not(windows))]
fn resolve_loopback_input(query: &str) -> Result<cpal::Device> {
    let q = query.trim();

    if q.eq_ignore_ascii_case("default-output") || q.eq_ignore_ascii_case("default") {
        #[cfg(target_os = "macos")]
        if let Some(dev) = cpal::default_host().default_output_device() {
            return Ok(dev);
        }
        let lb = list_loopback_capture_devices();
        if let Some((d, _)) = lb.into_iter().next() {
            return Ok(d);
        }
        anyhow::bail!(
            "No loopback capture device for default output. On macOS, check output devices / permissions; on Linux, need a *monitor* input; or disable loopback (--no-loopback)."
        );
    }

    if let Some(rest) = q.strip_prefix("lbidx:") {
        let idx: usize = rest
            .parse()
            .with_context(|| format!("lbidx: a number was expected, got {rest}"))?;
        return list_loopback_capture_devices()
            .into_iter()
            .nth(idx)
            .map(|(d, _)| d)
            .with_context(|| format!("lbidx:{idx} — no such loopback input"));
    }

    if DeviceId::from_str(q).is_ok() {
        return resolve_device_by_id_for_loopback(q);
    }

    if let Ok(idx) = q.parse::<usize>() {
        let outputs = list_output_device_names();
        if let Some((_, out_name)) = outputs.iter().find(|(i, _)| *i == idx) {
            let lb = list_loopback_capture_devices();
            if out_name.eq_ignore_ascii_case("default-output") {
                if let Some((d, _)) = lb.into_iter().next() {
                    return Ok(d);
                }
            } else {
                let needle = out_name.to_lowercase();
                if let Some((d, _)) = lb
                    .into_iter()
                    .find(|(_, n)| n.to_lowercase().contains(&needle))
                {
                    return Ok(d);
                }
            }
        }
    }

    let needle = q.to_lowercase();
    let lb = list_loopback_capture_devices();
    if let Some((d, _)) = lb
        .into_iter()
        .find(|(_, n)| n.to_lowercase().contains(&needle))
    {
        return Ok(d);
    }

    // We do not call resolve_mic: on macOS microphone inputs are separate devices; a substring
    // name match could open the microphone as "sys", which is semantically wrong.
    anyhow::bail!(
        "Loopback device '{q}' not found. Use CPAL id from `--list-devices`, or `lbidx:N`, or `default-output`. \
         macOS: the id is an output (speakers); Linux: usually a monitor input with the substring \"monitor\" in its name."
    );
}

fn to_mono(data: &[f32], channels: u16) -> Vec<f32> {
    if channels == 1 {
        return data.to_vec();
    }
    let ch = channels as usize;
    data.chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Microphone resampler → 16 kHz, WITH MEMORY between calls.
///
/// **What was wrong.** The previous code interpolated linearly and WITHOUT A FILTER:
///
/// * **aliasing.** At 48 → 16 kHz everything above 8 kHz folds back into the audible
///   range and lands on top of the voice. That is exactly the "artifacts in the
///   background", and it is the same thing that ruins ASR: the model hears what was
///   never there.
/// * **a discontinuity at the seam.** Every callback restarted the interpolation from
///   position zero — clicks were born on the buffer boundaries.
///
/// This is a job for a library, not for twenty lines eyeballed by hand: `rubato` computes
/// the filter via FFT and keeps the tail between blocks.
struct Downsampler {
    inner: rubato::FftFixedIn<f32>,
    /// The input accumulates: the resampler needs a fixed-length block.
    pending: Vec<f32>,
    chunk_in: usize,
}

impl Downsampler {
    /// The input block. 1024 frames at 48 kHz is 21 ms: the latency is unnoticeable, while
    /// the FFT works at a length where it is efficient.
    const CHUNK_IN: usize = 1024;

    fn new(src_rate: u32) -> Option<Self> {
        if src_rate == SAMPLE_RATE {
            return None; // nothing to convert
        }
        match rubato::FftFixedIn::<f32>::new(
            src_rate as usize,
            SAMPLE_RATE as usize,
            Self::CHUNK_IN,
            1, // sub_chunks
            1, // mono
        ) {
            Ok(inner) => Some(Self {
                inner,
                pending: Vec::with_capacity(Self::CHUNK_IN * 2),
                chunk_in: Self::CHUNK_IN,
            }),
            Err(e) => {
                // Silently handing back the raw stream is not an option: it would go into
                // the file at somebody else's rate and the recording would play at the
                // wrong speed.
                tracing::error!("resampler {src_rate}→{SAMPLE_RATE} Hz was not created: {e}");
                None
            }
        }
    }

    /// Returns as many 16 kHz samples as are already ready. The remainder is kept until the
    /// next call — no seams appear.
    fn push(&mut self, src: &[f32]) -> Vec<f32> {
        use rubato::Resampler as _;
        self.pending.extend_from_slice(src);
        let mut out = Vec::new();
        while self.pending.len() >= self.chunk_in {
            let block: Vec<f32> = self.pending.drain(..self.chunk_in).collect();
            match self.inner.process(&[block], None) {
                Ok(mut res) => {
                    if let Some(ch) = res.pop() {
                        out.extend(ch);
                    }
                }
                Err(e) => tracing::warn!("resampling: {e}"),
            }
        }
        out
    }
}

/// Capture from a cpal input device (mic). Resamples to 16 kHz mono.
/// With `reload_gen`: leave the thread if the counter becomes ≠ `reload_snapshot`
/// (device changed from the TUI).
pub fn mic_capture(
    device: cpal::Device,
    source_id: u8,
    tx: Sender<PcmChunk>,
    running: Arc<AtomicBool>,
    level_ui: Option<Sender<UiMsg>>,
    reload_gen: Option<Arc<AtomicU64>>,
    reload_snapshot: u64,
) -> Result<()> {
    // CPAL macOS loopback: output-only device → default_output_config + build_input_stream
    // (see examples/record_wav.rs).
    let supported = if device.supports_input() {
        device
            .default_input_config()
            .context("default_input_config")?
    } else {
        device
            .default_output_config()
            .context("default_output_config (capture from an output / loopback)")?
    };
    let native_rate = supported.sample_rate();
    let native_channels = supported.channels();
    let config = cpal::StreamConfig {
        channels: native_channels,
        sample_rate: native_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    let mut pcm_buf: Vec<i16> = Vec::with_capacity(CHUNK_FRAMES * 4);
    // The resampler lives BETWEEN callback invocations — otherwise every seam clicks.
    let mut down = Downsampler::new(native_rate);
    let tx2 = tx.clone();
    let running2 = running.clone();
    let level2 = level_ui.clone();

    let stream = device.build_input_stream(
        &config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            if !running2.load(Ordering::Relaxed) {
                return;
            }
            let mono = to_mono(data, native_channels);
            let resampled = match down.as_mut() {
                Some(d) => d.push(&mono),
                None => mono, // the device already gives 16 kHz
            };
            let samples: Vec<i16> = resampled
                .iter()
                .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                .collect();
            pcm_buf.extend_from_slice(&samples);
            while pcm_buf.len() >= CHUNK_FRAMES {
                let chunk: Vec<i16> = pcm_buf.drain(..CHUNK_FRAMES).collect();
                if let Some(ref u) = level2 {
                    let level = pcm_level_i16(&chunk);
                    let _ = u.send(UiMsg::AudioLevel { source_id, level });
                }
                // WE SEND, WE DO NOT TRY. There used to be a `try_send` here, and on a full
                // queue a piece of the recording was SILENTLY DROPPED.
                //
                // That threw away the one thing that is unrecoverable. The transcript, the
                // summary, the index — all of it can be recreated FROM THE AUDIO. The audio
                // cannot be recreated from anything.
                //
                // Acceptance 13.07.2026, live archive: under load (cook + LLM saturating the
                // cores) the consumer starved, the 33-second queue filled up — and HALF the
                // time got recorded. The file came out twice as short as reality: it played
                // twice as fast, clicked at the seams, and the ASR was fed time-compressed
                // mush. There was nothing to learn about it from.
                //
                // The channel is now unbounded: 32 KB/s per source — even a minute-long
                // stall is 2 MB. Losing the recording is incomparably more expensive.
                count_captured(source_id, chunk.len());
                if tx2
                    .send(PcmChunk {
                        source_id,
                        samples: chunk,
                    })
                    .is_err()
                {
                    return; // the receiver is gone — nowhere to write
                }
            }
        },
        |err| tracing::warn!("audio stream error: {err}"),
        None,
    )?;
    stream.play()?;
    while running.load(Ordering::Relaxed) {
        if let Some(ref rg) = reload_gen {
            if rg.load(Ordering::SeqCst) != reload_snapshot {
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = stream.pause();
    drop(stream);
    Ok(())
}

/// Capture system audio via WASAPI loopback (Windows).
#[cfg(windows)]
pub fn loopback_capture(
    device_query: &str,
    tx: Sender<PcmChunk>,
    running: Arc<AtomicBool>,
    level_ui: Option<Sender<UiMsg>>,
    reload_gen: Option<Arc<AtomicU64>>,
    reload_snapshot: u64,
) -> Result<()> {
    wasapi::initialize_mta()
        .ok()
        .context("COM init failed in loopback thread")?;

    let device = resolve_output_device_wasapi(device_query)?;
    let mut audio_client = device
        .get_iaudioclient()
        .map_err(|e| anyhow::anyhow!("get_iaudioclient: {e:?}"))?;

    let desired_format = wasapi::WaveFormat::new(
        16,
        16,
        &wasapi::SampleType::Int,
        SAMPLE_RATE as usize,
        1,
        None,
    );
    let (_, min_time) = audio_client
        .get_device_period()
        .map_err(|e| anyhow::anyhow!("get_device_period: {e:?}"))?;

    audio_client
        .initialize_client(
            &desired_format,
            &wasapi::Direction::Capture,
            &wasapi::StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: min_time,
            },
        )
        .map_err(|e| anyhow::anyhow!("initialize_client loopback: {e:?}"))?;

    let h_event = audio_client
        .set_get_eventhandle()
        .map_err(|e| anyhow::anyhow!("set_get_eventhandle: {e:?}"))?;
    let capture_client = audio_client
        .get_audiocaptureclient()
        .map_err(|e| anyhow::anyhow!("get_audiocaptureclient: {e:?}"))?;
    audio_client
        .start_stream()
        .map_err(|e| anyhow::anyhow!("start_stream: {e:?}"))?;

    let blockalign = desired_format.get_blockalign() as usize;
    let chunk_bytes = CHUNK_FRAMES * blockalign;
    let mut sample_queue: VecDeque<u8> = VecDeque::with_capacity(chunk_bytes * 8);

    // SILENCE IS ALSO A RECORDING, and it must occupy its real time.
    //
    // WASAPI loopback gives NOTHING while not a single application is playing sound — not
    // silent buffers, simply no packets at all. Without padding, the system-audio track then
    // becomes SHORTER than reality: 126 seconds of the world turn into 119 seconds of the
    // file. Everything that comes later stands in the wrong place relative to the microphone,
    // and the integrity watchdog honestly screams «RECORDING LOSS» — it is right, the audio
    // really is missing.
    //
    // So we top the track up with silence to real time. A pause in a conversation is a fact
    // of the conversation; a recording in which pauses are cut out is a lie about it.
    let started = std::time::Instant::now();
    let mut emitted: u64 = 0;
    // We only top up when we have fallen behind by more than a quarter of a second: the
    // device breathes unevenly, and fighting it over every 10 ms would only add jitter.
    const PAD_AFTER_SAMPLES: u64 = SAMPLE_RATE as u64 / 4;

    while running.load(Ordering::Relaxed) {
        if let Some(ref rg) = reload_gen {
            if rg.load(Ordering::SeqCst) != reload_snapshot {
                break;
            }
        }
        capture_client
            .read_from_device_to_deque(&mut sample_queue)
            .map_err(|e| anyhow::anyhow!("read_from_device: {e:?}"))?;

        while sample_queue.len() >= chunk_bytes {
            let mut pcm = Vec::with_capacity(CHUNK_FRAMES);
            for _ in 0..CHUNK_FRAMES {
                let lo = sample_queue.pop_front().unwrap();
                let hi = sample_queue.pop_front().unwrap();
                pcm.push(i16::from_le_bytes([lo, hi]));
            }
            if let Some(ref u) = level_ui {
                let level = pcm_level_i16(&pcm);
                let _ = u.send(UiMsg::AudioLevel {
                    source_id: 1,
                    level,
                });
            }
            // See the comment in mic_capture: we NEVER drop audio.
            emitted += pcm.len() as u64;
            count_captured(1, pcm.len());
            if tx
                .send(PcmChunk {
                    source_id: 1,
                    samples: pcm,
                })
                .is_err()
            {
                break; // the receiver is gone
            }
        }
        // Nothing is playing in the system — top the track up with silence so that its time
        // matches the world's. Otherwise the pause simply falls out of the recording.
        let expected =
            (started.elapsed().as_secs_f64() * f64::from(SAMPLE_RATE)) as u64;
        if expected > emitted + PAD_AFTER_SAMPLES {
            let mut missing = expected - emitted;
            while missing > 0 {
                let n = missing.min(CHUNK_FRAMES as u64) as usize;
                emitted += n as u64;
                missing -= n as u64;
                count_captured(1, n);
                if tx
                    .send(PcmChunk {
                        source_id: 1,
                        samples: vec![0i16; n],
                    })
                    .is_err()
                {
                    break;
                }
            }
        }
        if h_event.wait_for_event(100).is_err() {}
    }

    audio_client
        .stop_stream()
        .map_err(|e| anyhow::anyhow!("stop_stream: {e:?}"))?;
    Ok(())
}

#[cfg(windows)]
fn resolve_output_device_wasapi(query: &str) -> Result<wasapi::Device> {
    let enumerator =
        wasapi::DeviceEnumerator::new().map_err(|e| anyhow::anyhow!("DeviceEnumerator: {e:?}"))?;
    if query.eq_ignore_ascii_case("default-output") || query.eq_ignore_ascii_case("default") {
        return enumerator
            .get_default_device(&wasapi::Direction::Render)
            .map_err(|e| anyhow::anyhow!("get_default_device: {e:?}"));
    }
    if let Ok(idx) = query.parse::<usize>() {
        if idx == 0 {
            return enumerator
                .get_default_device(&wasapi::Direction::Render)
                .map_err(|e| anyhow::anyhow!("get_default_device: {e:?}"));
        }
        let collection = enumerator
            .get_device_collection(&wasapi::Direction::Render)
            .map_err(|e| anyhow::anyhow!("get_device_collection: {e:?}"))?;
        return collection
            .into_iter()
            .nth(idx - 1)
            .context(format!("Output device index {idx} not found"))?
            .map_err(|e| anyhow::anyhow!("device error: {e:?}"));
    }
    let needle = query.to_lowercase();
    let collection = enumerator
        .get_device_collection(&wasapi::Direction::Render)
        .map_err(|e| anyhow::anyhow!("get_device_collection: {e:?}"))?;
    for dev_result in collection.into_iter() {
        let dev = dev_result.map_err(|e| anyhow::anyhow!("device error: {e:?}"))?;
        if dev
            .get_friendlyname()
            .unwrap_or_default()
            .to_lowercase()
            .contains(&needle)
        {
            return Ok(dev);
        }
    }
    anyhow::bail!("Output device '{query}' not found")
}

#[cfg(not(windows))]
pub fn loopback_capture(
    device_query: &str,
    tx: Sender<PcmChunk>,
    running: Arc<AtomicBool>,
    level_ui: Option<Sender<UiMsg>>,
    reload_gen: Option<Arc<AtomicU64>>,
    reload_snapshot: u64,
) -> Result<()> {
    let device = resolve_loopback_input(device_query)?;
    mic_capture(
        device,
        1,
        tx,
        running,
        level_ui,
        reload_gen,
        reload_snapshot,
    )
}

#[cfg(test)]
mod device_choice_tests {
    use super::*;

    /// Two devices with the same name must not produce two identical lines on screen — picking
    /// between them would be a coin toss. Measured on the owner's machine: Windows reports two
    /// inputs both called «Микрофон», with no manufacturer, differing only by GUID.
    ///
    /// The tail, because a WASAPI id starts with an identical `wasapi:{0.0.1.00000000}.` on every
    /// device: a prefix would print the same six characters twice and solve nothing.
    #[test]
    fn same_named_devices_are_told_apart_by_the_end_of_their_id() {
        let a = "wasapi:{0.0.1.00000000}.{4029964e-6b79-4452-9b74-df7f32c2599e}";
        let b = "wasapi:{0.0.1.00000000}.{b1f75504-1cef-418d-9b90-e5097f957553}";
        assert_ne!(id_tail(a), id_tail(b), "two microphones would look identical");
        assert_eq!(id_tail(a).chars().count(), 6);
    }

    /// A short or odd id must not panic or come back empty — the label still has to say something.
    #[test]
    fn a_short_id_still_yields_a_tail() {
        assert_eq!(id_tail("abc"), "abc");
        assert_eq!(id_tail(""), "");
        // Non-ASCII ids are stripped to what can be read aloud; the point is only distinguishing.
        assert!(id_tail("устройство-42").ends_with("42"));
    }
}

#[cfg(test)]
mod resample_tests {
    use super::*;

    /// Signal energy at frequency `f` (a plain correlation with a sine and a cosine).
    fn energy_at(samples: &[f32], f: f64, rate: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (n, &s) in samples.iter().enumerate() {
            let a = 2.0 * std::f64::consts::PI * f * n as f64 / rate;
            re += f64::from(s) * a.cos();
            im += f64::from(s) * a.sin();
        }
        (re * re + im * im).sqrt() / samples.len() as f64
    }

    fn sine(freq: f64, rate: f64, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / rate).sin() as f32 * 0.5)
            .collect()
    }

    /// A continuous sine, cut into blocks. CONTINUOUS is the whole point: generating every
    /// block from zero phase means tearing the signal at the seams, and then we would be
    /// measuring our own clicks rather than the filter.
    fn blocks(freq: f64, rate: f64, block: usize, count: usize) -> Vec<Vec<f32>> {
        let all = sine(freq, rate, block * count);
        all.chunks(block).map(<[f32]>::to_vec).collect()
    }

    /// ALIASING — that very "dirt in the background".
    ///
    /// A 12 kHz tone at 48 kHz lies ABOVE the Nyquist frequency for 16 kHz (8 kHz). Without
    /// a filter it does not disappear, it FOLDS back: 16000 − 12000 = 4 kHz — right into the
    /// middle of the voice range, on top of the speech. The previous linear interpolation did
    /// exactly that, and it ruined not only the listening but the ASR too.
    #[test]
    fn a_tone_above_nyquist_is_filtered_out_not_folded_onto_the_voice() {
        let mut d = Downsampler::new(48_000).expect("resampler 48k→16k");
        let mut out = Vec::new();
        // several blocks — this also checks that the state lives between calls
        for b in blocks(12_000.0, 48_000.0, 1024, 8) {
            out.extend(d.push(&b));
        }
        assert!(out.len() > 2000, "the resampler produced nothing: {}", out.len());

        let ghost = energy_at(&out, 4_000.0, 16_000.0); // where the tone would have folded to
        let full = out.iter().map(|s| f64::from(*s).abs()).sum::<f64>() / out.len() as f64;
        assert!(
            ghost < 0.01 && full < 0.05,
            "the 12 kHz tone folded into the audible range: ghost at 4 kHz = {ghost:.4}, \
             mean amplitude = {full:.4}"
        );
    }

    /// And the voice gets through. The filter must not mute the very thing all of this is for.
    #[test]
    fn a_voice_frequency_survives_the_downsample() {
        let mut d = Downsampler::new(48_000).unwrap();
        let mut out = Vec::new();
        for b in blocks(1_000.0, 48_000.0, 1024, 8) {
            out.extend(d.push(&b));
        }
        // The resampler emits the first block with the filter's latency — measure on the tail.
        let tail = &out[out.len() / 2..];
        let kept = energy_at(tail, 1_000.0, 16_000.0);
        assert!(kept > 0.15, "the 1 kHz tone (speech) is lost: {kept:.4}");
    }

    /// Duration MUST be preserved: as much time went in, as much must come out.
    /// Otherwise the recording plays at the wrong speed (that is exactly the bug the channel
    /// had).
    #[test]
    fn time_is_preserved_one_to_one() {
        let mut d = Downsampler::new(48_000).unwrap();
        let mut out = 0usize;
        // 48000 samples = exactly 1 second
        for b in blocks(440.0, 48_000.0, 1024, 48_000 / 1024) {
            out += d.push(&b).len();
        }
        let sec = out as f64 / f64::from(SAMPLE_RATE);
        assert!(
            (sec - 1.0).abs() < 0.05,
            "a second of input produced {sec:.3} s of output — the recording will drift in time"
        );
    }
}
