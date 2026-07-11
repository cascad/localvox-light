//! Log-mel spectrogram preprocessor.
//!
//! Совместим с `gigaam.preprocess.FeatureExtractor` / NeMo `FilterbankFeatures`
//! при следующих параметрах (см. [`MelConfig::GIGAAM_V3`]):
//! * sample_rate = 16000, n_mels = 64
//! * n_fft = win_length = 320, hop_length = 160
//! * mel_scale = HTK, mel_norm = null, center = false
//! * окно — периодический Hann (PyTorch default)

use realfft::num_complex::Complex;
use realfft::RealFftPlanner;

/// Параметры mel-спектрограммы.
#[derive(Clone, Debug)]
pub struct MelConfig {
    pub sample_rate: u32,
    pub n_mels: usize,
    pub n_fft: usize,
    pub win_length: usize,
    pub hop_length: usize,
    /// `false` — окна выровнены от первого сэмпла (NeMo `center=false`).
    /// `true` — librosa-style центрирование (padding `n_fft/2` слева/справа). Пока не реализовано.
    pub center: bool,
    /// HTK-формула шкалы mel (`2595 * log10(1 + f/700)`).
    pub use_htk: bool,
    /// Epsilon под `ln(mel + eps)` (стабилизирует логарифм).
    pub log_eps: f32,
}

impl MelConfig {
    /// Параметры всего семейства GigaAM v3 (одни и те же для `v3_ctc`, `v3_e2e_ctc`, `v3_rnnt` и т.п.).
    pub const GIGAAM_V3: Self = Self {
        sample_rate: 16_000,
        n_mels: 64,
        n_fft: 320,
        win_length: 320,
        hop_length: 160,
        center: false,
        use_htk: true,
        log_eps: 1e-10,
    };
}

/// Результат: log-mel-спектрограмма в row-major (frame-major) укладке.
#[derive(Clone, Debug)]
pub struct MelSpectrogram {
    /// `[n_frames * n_mels]`. Для frame t индекс m: `data[t * n_mels + m]`.
    pub data: Vec<f32>,
    pub n_frames: usize,
    pub n_mels: usize,
}

/// Вычислить log-mel спектрограмму над PCM 16 kHz mono f32.
///
/// Реализация: пер-фреймное окно Hann (периодическое) → real-FFT → power-spectrum
/// → HTK-mel filterbank (без нормализации) → `ln(mel + eps)`.
///
/// Возвращает пустой `data` (с `n_frames = 0`), если входных сэмплов меньше окна.
pub fn log_mel_spectrogram(samples: &[f32], cfg: &MelConfig) -> MelSpectrogram {
    assert!(!cfg.center, "MelConfig::center=true пока не поддержан");
    assert!(
        cfg.win_length <= cfg.n_fft,
        "win_length должен быть <= n_fft"
    );

    if samples.len() < cfg.win_length {
        return MelSpectrogram {
            data: Vec::new(),
            n_frames: 0,
            n_mels: cfg.n_mels,
        };
    }

    let n_frames = (samples.len() - cfg.win_length) / cfg.hop_length + 1;
    let window = hann_window_periodic(cfg.win_length);
    let filterbank = mel_filterbank_htk(cfg);

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(cfg.n_fft);
    let mut buf_in: Vec<f32> = fft.make_input_vec();
    let mut buf_out: Vec<Complex<f32>> = fft.make_output_vec();

    let n_bins = cfg.n_fft / 2 + 1;
    let mut out = Vec::with_capacity(n_frames * cfg.n_mels);

    for frame in 0..n_frames {
        let start = frame * cfg.hop_length;
        for i in 0..cfg.win_length {
            buf_in[i] = samples[start + i] * window[i];
        }
        for x in buf_in.iter_mut().skip(cfg.win_length) {
            *x = 0.0;
        }
        fft.process(&mut buf_in, &mut buf_out)
            .expect("realfft forward");

        for mel in 0..cfg.n_mels {
            let filter = &filterbank[mel];
            let mut energy = 0.0f32;
            for k in 0..n_bins {
                let c = buf_out[k];
                let p = c.re * c.re + c.im * c.im;
                energy += filter[k] * p;
            }
            out.push((energy + cfg.log_eps).ln());
        }
    }

    MelSpectrogram {
        data: out,
        n_frames,
        n_mels: cfg.n_mels,
    }
}

fn hann_window_periodic(n: usize) -> Vec<f32> {
    let two_pi = std::f32::consts::TAU;
    (0..n)
        .map(|i| 0.5 - 0.5 * (two_pi * i as f32 / n as f32).cos())
        .collect()
}

#[inline]
fn hz_to_mel_htk(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

#[inline]
fn mel_to_hz_htk(mel: f32) -> f32 {
    700.0 * (10.0f32.powf(mel / 2595.0) - 1.0)
}

fn mel_filterbank_htk(cfg: &MelConfig) -> Vec<Vec<f32>> {
    assert!(cfg.use_htk, "Slaney-шкала пока не поддержана");
    let n_bins = cfg.n_fft / 2 + 1;
    let f_min = 0.0f32;
    let f_max = cfg.sample_rate as f32 / 2.0;
    let mel_min = hz_to_mel_htk(f_min);
    let mel_max = hz_to_mel_htk(f_max);

    let n_pts = cfg.n_mels + 2;
    let mut hz_pts = Vec::with_capacity(n_pts);
    for i in 0..n_pts {
        let mel = mel_min + (mel_max - mel_min) * (i as f32) / ((cfg.n_mels + 1) as f32);
        hz_pts.push(mel_to_hz_htk(mel));
    }

    let bin_hz: Vec<f32> = (0..n_bins)
        .map(|k| k as f32 * cfg.sample_rate as f32 / cfg.n_fft as f32)
        .collect();

    let mut bank = Vec::with_capacity(cfg.n_mels);
    for m in 0..cfg.n_mels {
        let left = hz_pts[m];
        let center = hz_pts[m + 1];
        let right = hz_pts[m + 2];
        let mut filter = vec![0.0f32; n_bins];
        for k in 0..n_bins {
            let f = bin_hz[k];
            if f >= left && f <= center && center > left {
                filter[k] = (f - left) / (center - left);
            } else if f > center && f <= right && right > center {
                filter[k] = (right - f) / (right - center);
            }
        }
        bank.push(filter);
    }
    bank
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_zero_frames() {
        let cfg = MelConfig::GIGAAM_V3;
        let r = log_mel_spectrogram(&[], &cfg);
        assert_eq!(r.n_frames, 0);
        assert_eq!(r.n_mels, cfg.n_mels);
        assert!(r.data.is_empty());
    }

    #[test]
    fn frame_count_matches_formula() {
        let cfg = MelConfig::GIGAAM_V3;
        let samples = vec![0.0f32; 16_000]; // 1 секунда
        let r = log_mel_spectrogram(&samples, &cfg);
        let expected = (samples.len() - cfg.win_length) / cfg.hop_length + 1;
        assert_eq!(r.n_frames, expected);
        assert_eq!(r.data.len(), expected * cfg.n_mels);
    }

    #[test]
    fn sine_1khz_peaks_near_expected_mel_bin() {
        let cfg = MelConfig::GIGAAM_V3;
        let sr = cfg.sample_rate as f32;
        let freq = 1000.0f32;
        let n = 16_000;
        let samples: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin())
            .collect();

        let r = log_mel_spectrogram(&samples, &cfg);
        assert!(r.n_frames > 10, "должны быть фреймы");
        let mid = r.n_frames / 2;
        let row = &r.data[mid * cfg.n_mels..(mid + 1) * cfg.n_mels];

        let mut argmax = 0usize;
        let mut best = row[0];
        for (i, &v) in row.iter().enumerate().skip(1) {
            if v > best {
                best = v;
                argmax = i;
            }
        }

        // Ожидаемый центр: mel-bin, чья центральная частота ближе всего к 1 кГц.
        let mel_min = hz_to_mel_htk(0.0);
        let mel_max = hz_to_mel_htk(sr / 2.0);
        let expected_mel = (0..cfg.n_mels)
            .min_by(|&a, &b| {
                let fa = mel_to_hz_htk(
                    mel_min + (mel_max - mel_min) * ((a + 1) as f32) / ((cfg.n_mels + 1) as f32),
                );
                let fb = mel_to_hz_htk(
                    mel_min + (mel_max - mel_min) * ((b + 1) as f32) / ((cfg.n_mels + 1) as f32),
                );
                (fa - freq).abs().partial_cmp(&(fb - freq).abs()).unwrap()
            })
            .unwrap();

        assert!(
            (argmax as i32 - expected_mel as i32).abs() <= 1,
            "argmax {argmax} далеко от ожидаемого {expected_mel}"
        );
    }
}
