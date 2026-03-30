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
use crossbeam_channel::{Sender, TrySendError};

use crate::events::UiMsg;

pub const SAMPLE_RATE: u32 = 16000;
pub const CHUNK_FRAMES: usize = 512;

pub struct PcmChunk {
    pub source_id: u8,
    pub samples: Vec<i16>,
}

/// RMS → 0..1 для шкалы из 8 блоков (как client-reliable: `rms/32768*12`, min 1.0).
pub fn pcm_level_i16(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let rms: f64 = (samples.iter().map(|&s| (s as f64).powi(2)).sum::<f64>()
        / samples.len() as f64)
        .sqrt();
    (rms / 32768.0 * 12.0).min(1.0) as f32
}

/// Строка для отображения устройства (имя + производитель + id) — как в client-reliable.
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
        // CoCreateInstance(MMDeviceEnumerator) падает без CoInitializeEx на этом потоке;
        // loopback в отдельном потоке вызывает initialize_mta, главный поток TUI — нет.
        let _ = wasapi::initialize_mta();
        let enumerator = match wasapi::DeviceEnumerator::new() {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = ?e, "WASAPI DeviceEnumerator (список loopback)");
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

/// Источники для захвата «системного» звука (не Windows).
///
/// **macOS (CPAL ≥ 0.17, Sequoia+):** loopback — это не отдельный вход в списке, а `build_input_stream`
/// на **output**-устройстве (динамики). Поэтому здесь перечисляются **выходы** с `supports_output()`;
/// приоритет — устройства **без** физического входа (`!supports_input()`), иначе CPAL откроет микрофон, а не tap.
///
/// **Linux:** виртуальные входы monitor (PipeWire/Pulse), подстрока `monitor` в имени.
///
/// Порядок перечисления может слегка меняться — в конфиг лучше сохранять `device.id()`.
#[cfg(all(not(windows), target_os = "macos"))]
pub fn list_loopback_capture_devices() -> Vec<(cpal::Device, String)> {
    let host = cpal::default_host();
    let Ok(iter) = host.output_devices() else {
        return Vec::new();
    };
    // Не отбрасывать устройство, если description() временно пустой — иначе список «loopback» может
    // стать пустым при живых выходах (редко, но на отдельных сборках/OS встречалось).
    let outs: Vec<(cpal::Device, String)> = iter
        .map(|dev| {
            let name = dev
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| {
                    dev.id()
                        .map(|id| format!("(имя недоступно) {id}"))
                        .unwrap_or_else(|_| "(устройство без имени)".into())
                });
            (dev, name)
        })
        .collect();
    // Предпочитаем чистые выходы: иначе CPAL на combo-устройстве откроет микрофон, а не tap с выхода.
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

/// Если `list_loopback_capture_devices` пуста при `--list-devices` на macOS — краткая подсказка.
#[cfg(all(not(windows), target_os = "macos"))]
pub fn macos_loopback_empty_hint() -> Option<String> {
    let host = cpal::default_host();
    match host.output_devices() {
        Err(e) => Some(format!(
            "cpal не смог перечислить выходы: {e}. Обычно это не про «имя устройства», а окружение (sandbox, нет GUI-сессии) или ограничения доступа. Для записи системного звука на macOS 14.6+ у терминала/IDE часто нужно разрешение «Микрофон» (Настройки → Конфиденциальность и безопасность)."
        )),
        Ok(iter) => {
            let n = iter.count();
            if n == 0 {
                Some(
                    "CoreAudio вернул 0 выходов (при этом входы могут быть видны). Проверьте: не SSH без аудио-сессии, не обрезанная VM; встроенные динамики в «Звук» включены.".into(),
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

/// Стабильный идентификатор CPAL для сохранения в конфиг (не зависит от языка ОС).
pub fn device_id_save_token(dev: &cpal::Device) -> Option<String> {
    dev.id().ok().map(|id| id.to_string())
}

fn resolve_input_device_by_id_str(id_str: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    let id = DeviceId::from_str(id_str.trim())
        .with_context(|| format!("Invalid CPAL device id: {id_str}"))?;
    let dev = host
        .device_by_id(&id)
        .with_context(|| format!("No device for id {id_str} (отключено или другое имя хоста)"))?;
    anyhow::ensure!(
        dev.supports_input(),
        "Устройство {id_str} не поддерживает ввод (input)"
    );
    Ok(dev)
}

/// Для loopback: на macOS id относится к **выходу** (динамики), `supports_input` не требуется.
#[cfg(not(windows))]
fn resolve_device_by_id_for_loopback(id_str: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    let id = DeviceId::from_str(id_str.trim())
        .with_context(|| format!("Invalid CPAL device id: {id_str}"))?;
    host.device_by_id(&id)
        .with_context(|| format!("No device for id {id_str} (отключено или другое имя хоста)"))
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
            .with_context(|| format!("micidx: ожидалось число, получено {rest}"))?;
        return collect_input_devices()
            .into_iter()
            .nth(idx)
            .map(|(d, _)| d)
            .with_context(|| format!("micidx:{idx} — нет такого индекса в списке микрофонов"));
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
            .with_context(|| format!("lbidx: ожидалось число, получено {rest}"))?;
        return list_loopback_capture_devices()
            .into_iter()
            .nth(idx)
            .map(|(d, _)| d)
            .with_context(|| format!("lbidx:{idx} — нет такого loopback-входа"));
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

    // Не вызываем resolve_mic: на macOS микрофонные входы — отдельные устройства; подстрочное
    // совпадение имени могло бы открыть микрофон как «sys», что семантически неверно.
    anyhow::bail!(
        "Loopback device '{q}' not found. Use CPAL id from `--list-devices`, or `lbidx:N`, or `default-output`. \
         macOS: id — выход (динамики); Linux: обычно monitor-вход с подстрокой \"monitor\" в имени."
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

fn resample(src: &[f32], src_rate: u32) -> Vec<f32> {
    if src_rate == SAMPLE_RATE {
        return src.to_vec();
    }
    let ratio = src_rate as f64 / SAMPLE_RATE as f64;
    let out_len = (src.len() as f64 / ratio).ceil() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = src[idx.min(src.len() - 1)];
            let b = src[(idx + 1).min(src.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

/// Capture from a cpal input device (mic). Resamples to 16 kHz mono.
/// При `reload_gen`: выход из потока, если счётчик стал ≠ `reload_snapshot` (смена устройства из TUI).
pub fn mic_capture(
    device: cpal::Device,
    source_id: u8,
    tx: Sender<PcmChunk>,
    running: Arc<AtomicBool>,
    level_ui: Option<Sender<UiMsg>>,
    reload_gen: Option<Arc<AtomicU64>>,
    reload_snapshot: u64,
) -> Result<()> {
    // CPAL macOS loopback: output-only device → default_output_config + build_input_stream (см. examples/record_wav.rs).
    let supported = if device.supports_input() {
        device
            .default_input_config()
            .context("default_input_config")?
    } else {
        device
            .default_output_config()
            .context("default_output_config (захват с выхода / loopback)")?
    };
    let native_rate = supported.sample_rate();
    let native_channels = supported.channels();
    let config = cpal::StreamConfig {
        channels: native_channels,
        sample_rate: native_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    let mut pcm_buf: Vec<i16> = Vec::with_capacity(CHUNK_FRAMES * 4);
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
            let resampled = resample(&mono, native_rate);
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
                match tx2.try_send(PcmChunk {
                    source_id,
                    samples: chunk,
                }) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
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
        16, 16,
        &wasapi::SampleType::Int,
        SAMPLE_RATE as usize, 1, None,
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
            match tx.try_send(PcmChunk {
                source_id: 1,
                samples: pcm,
            }) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
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
