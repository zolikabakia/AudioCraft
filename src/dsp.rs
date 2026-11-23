//! Effects: definitions, parameter UIs, real-time processors and offline processes.

use crate::audio::{AudioData, db, undb};
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

// ---------------------------------------------------------------- filters

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterKind {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
}

impl FilterKind {
    pub const ALL: [FilterKind; 7] = [
        FilterKind::LowPass,
        FilterKind::HighPass,
        FilterKind::BandPass,
        FilterKind::Notch,
        FilterKind::Peak,
        FilterKind::LowShelf,
        FilterKind::HighShelf,
    ];
    pub fn name(self) -> &'static str {
        match self {
            FilterKind::LowPass => "Low Pass",
            FilterKind::HighPass => "High Pass",
            FilterKind::BandPass => "Band Pass",
            FilterKind::Notch => "Notch",
            FilterKind::Peak => "Peak",
            FilterKind::LowShelf => "Low Shelf",
            FilterKind::HighShelf => "High Shelf",
        }
    }
    pub fn uses_gain(self) -> bool {
        matches!(
            self,
            FilterKind::Peak | FilterKind::LowShelf | FilterKind::HighShelf
        )
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    pub fn new(kind: FilterKind, rate: f32, freq: f32, q: f32, gain_db: f32) -> Self {
        let freq = freq.clamp(10.0, rate * 0.49);
        let q = q.max(0.05);
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / rate;
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / (2.0 * q);
        let (b0, b1, b2, a0, a1, a2) = match kind {
            FilterKind::LowPass => (
                (1.0 - cs) / 2.0,
                1.0 - cs,
                (1.0 - cs) / 2.0,
                1.0 + alpha,
                -2.0 * cs,
                1.0 - alpha,
            ),
            FilterKind::HighPass => (
                (1.0 + cs) / 2.0,
                -(1.0 + cs),
                (1.0 + cs) / 2.0,
                1.0 + alpha,
                -2.0 * cs,
                1.0 - alpha,
            ),
            FilterKind::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            FilterKind::Notch => (1.0, -2.0 * cs, 1.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            FilterKind::Peak => (
                1.0 + alpha * a,
                -2.0 * cs,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cs,
                1.0 - alpha / a,
            ),
            FilterKind::LowShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cs + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cs),
                    a * ((a + 1.0) - (a - 1.0) * cs - s),
                    (a + 1.0) + (a - 1.0) * cs + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cs),
                    (a + 1.0) + (a - 1.0) * cs - s,
                )
            }
            FilterKind::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cs + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cs),
                    a * ((a + 1.0) + (a - 1.0) * cs - s),
                    (a + 1.0) - (a - 1.0) * cs + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cs),
                    (a + 1.0) - (a - 1.0) * cs - s,
                )
            }
        };
        Biquad {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Magnitude response at frequency f.
    pub fn response(&self, f: f32, rate: f32) -> f32 {
        let w = 2.0 * PI * f / rate;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        ((nr * nr + ni * ni) / (dr * dr + di * di).max(1e-20)).sqrt()
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct BqState {
    z1: f32,
    z2: f32,
}

impl BqState {
    #[inline]
    pub(crate) fn run(&mut self, f: &Biquad, x: f32) -> f32 {
        let y = f.b0 * x + self.z1;
        self.z1 = f.b1 * x - f.a1 * y + self.z2;
        self.z2 = f.b2 * x - f.a2 * y;
        y
    }
}

/// Split audio into complementary bands at the given crossover frequencies (Hz). Each crossover
/// uses a fourth-order Linkwitz-Riley pair: two Butterworth low-pass sections and two high-pass
/// sections, so the bands add back up to roughly the original level and phase.
///
/// Frequencies are sorted, de-duplicated and limited to a safe range below Nyquist. A file with
/// no usable crossovers (or no audio) is returned as a single band, so callers always get data.
pub fn split_bands(d: &AudioData, crossovers: &[f32]) -> Vec<AudioData> {
    let frames = d.frames();
    if frames == 0 || d.channels() == 0 || d.rate < 40 {
        return vec![d.clone()];
    }
    let nyquist = d.rate as f32 * 0.45;
    if nyquist <= 20.0 {
        return vec![d.clone()];
    }
    let mut xs: Vec<f32> = crossovers
        .iter()
        .copied()
        .filter(|f| f.is_finite() && *f > 0.0)
        .map(|f| f.clamp(20.0, nyquist))
        .collect();
    xs.sort_by(|a, b| a.total_cmp(b));
    xs.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    if xs.is_empty() {
        return vec![d.clone()];
    }
    let q = std::f32::consts::FRAC_1_SQRT_2;
    let rate = d.rate as f32;
    let mut out = Vec::with_capacity(xs.len() + 1);
    for band in 0..=xs.len() {
        let mut filters: Vec<Biquad> = Vec::new();
        // Middle and top bands get a high-pass at the lower crossover.
        if band > 0 {
            for _ in 0..2 {
                filters.push(Biquad::new(
                    FilterKind::HighPass,
                    rate,
                    xs[band - 1],
                    q,
                    0.0,
                ));
            }
        }
        // Bottom and middle bands get a low-pass at the upper crossover.
        if band < xs.len() {
            for _ in 0..2 {
                filters.push(Biquad::new(FilterKind::LowPass, rate, xs[band], q, 0.0));
            }
        }
        let mut data = AudioData::new(d.rate, d.channels(), frames);
        for (src, dst) in d.ch.iter().zip(data.ch.iter_mut()) {
            let mut state = vec![BqState::default(); filters.len()];
            for (i, sample) in src.iter().enumerate() {
                let mut v = *sample;
                for (s, f) in state.iter_mut().zip(filters.iter()) {
                    v = s.run(f, v);
                }
                dst[i] = v;
            }
        }
        out.push(data);
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    pub kind: FilterKind,
    pub freq: f32,
    pub gain_db: f32,
    pub q: f32,
    pub on: bool,
}

/// A readable name for an effect parameter path: "bands.1.gain_db" -> "Band 2 Gain (dB)".
pub fn param_label(path: &str) -> String {
    let mut words: Vec<String> = vec![];
    let parts: Vec<&str> = path.split('.').collect();
    for (i, p) in parts.iter().enumerate() {
        if let Ok(n) = p.parse::<usize>() {
            if let Some(w) = words.last_mut() {
                let singular = w.strip_suffix('s').unwrap_or(w).to_string();
                *w = format!("{singular} {}", n + 1);
            }
            continue;
        }
        let last = i + 1 == parts.len();
        let (base, unit) = if last {
            if let Some(b) = p.strip_suffix("_ms") {
                (b.to_string(), " (ms)")
            } else if let Some(b) = p.strip_suffix("_db") {
                (b.to_string(), " (dB)")
            } else if let Some(b) = p.strip_suffix("_hz") {
                (b.to_string(), " (Hz)")
            } else if let Some(b) = p.strip_suffix("_deg") {
                (b.to_string(), " (\u{b0})")
            } else {
                (p.to_string(), "")
            }
        } else {
            (p.to_string(), "")
        };
        let mut w: String = base
            .split('_')
            .filter(|s| !s.is_empty())
            .map(|s| {
                let mut c = s.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        w.push_str(unit);
        words.push(w);
    }
    words.join(" ")
}

/// A sensible automation range for a parameter, from its name (widened to hold `v`).
pub fn param_range(path: &str, v: f32) -> (f32, f32) {
    let last = path.rsplit('.').next().unwrap_or(path);
    let (lo, hi): (f32, f32) = if last.ends_with("_ms") {
        (0.0, 2000.0)
    } else if last.ends_with("_db")
        || last == "threshold"
        || last == "makeup"
        || last == "input_boost"
    {
        (-60.0, 24.0)
    } else if last.ends_with("_hz")
        || last == "freq"
        || last.starts_with('f') && last.len() <= 2
        || last.starts_with('x') && last.len() == 2
    {
        (20.0, 20000.0)
    } else if last == "q" {
        (0.1, 30.0)
    } else if last == "ratio" {
        (1.0, 30.0)
    } else if last == "rate" {
        (0.01, 20.0)
    } else if last.ends_with("_deg") {
        (-180.0, 180.0)
    } else if last == "width" {
        (0.0, 3.0)
    } else if matches!(last, "ll" | "lr" | "rl" | "rr") {
        (-100.0, 100.0)
    } else if last == "a4" {
        (400.0, 480.0)
    } else if matches!(last, "sensitivity" | "attack") {
        (0.0, 100.0)
    } else {
        (0.0, 1.0)
    };
    (lo.min(v), hi.max(v))
}

pub fn default_eq_bands() -> Vec<EqBand> {
    crate::equi::standard_bands()
}

// ---------------------------------------------------------------- effect definitions

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Amplify {
        gain_db: f32,
    },
    ParametricEq {
        bands: Vec<EqBand>,
        master_db: f32,
    },
    Filter {
        kind: FilterKind,
        freq: f32,
        q: f32,
        gain_db: f32,
    },
    Compressor {
        threshold: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup: f32,
    },
    HardLimiter {
        ceiling_db: f32,
        input_boost: f32,
        release_ms: f32,
    },
    NoiseGate {
        threshold: f32,
        attack_ms: f32,
        release_ms: f32,
        floor_db: f32,
    },
    Delay {
        time_ms: f32,
        feedback: f32,
        mix: f32,
    },
    Reverb {
        room: f32,
        damping: f32,
        width: f32,
        mix: f32,
    },
    Chorus {
        rate: f32,
        depth_ms: f32,
        mix: f32,
    },
    Flanger {
        rate: f32,
        depth_ms: f32,
        feedback: f32,
        mix: f32,
    },
    Distortion {
        drive_db: f32,
        mix: f32,
    },
    DeHum {
        freq: f32,
        harmonics: u32,
        q: f32,
    },
    StereoExpander {
        width: f32,
    },
    /// A SoundCraft built-in or a hosted CLAP/VST3 plug-in.
    Plugin {
        id: String,
        name: String,
        params: Vec<(String, f32)>,
        #[serde(default)]
        state: Option<Vec<u8>>,
    },
    /// An effect from an imported session that AudioCraft doesn't have yet: kept by name with
    /// its stored settings; audio passes through unchanged.
    Imported {
        name: String,
        params: Vec<(String, f32)>,
    },
    Phaser {
        stages: u32,
        rate: f32,
        depth: f32,
        feedback: f32,
        mix: f32,
    },
    ChorusFlanger {
        flanger: bool,
        rate: f32,
        depth_ms: f32,
        feedback: f32,
        mix: f32,
    },
    AnalogDelay {
        time_ms: f32,
        feedback: f32,
        drive: f32,
        spread_ms: f32,
        tone_hz: f32,
        mix: f32,
    },
    Echo {
        left_ms: f32,
        right_ms: f32,
        feedback: f32,
        tone_hz: f32,
        mix: f32,
    },
    DeEsser {
        freq: f32,
        threshold: f32,
        reduction: f32,
    },
    Multiband {
        x1: f32,
        x2: f32,
        thr_low: f32,
        thr_mid: f32,
        thr_high: f32,
        ratio: f32,
        makeup: f32,
    },
    DynamicsProcessing {
        gate_db: f32,
        comp_db: f32,
        ratio: f32,
        ceiling_db: f32,
        makeup: f32,
    },
    TubeCompressor {
        threshold: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        drive: f32,
        makeup: f32,
    },
    SpeechLeveler {
        target_db: f32,
        max_gain_db: f32,
        gate_db: f32,
        speed: f32,
    },
    ChannelMixer {
        ll: f32,
        lr: f32,
        rl: f32,
        rr: f32,
    },
    GraphicEq {
        gains: Vec<f32>,
        master_db: f32,
    },
    NotchFilter {
        freqs: Vec<f32>,
        depth_db: f32,
        q: f32,
    },
    ScientificFilter {
        kind: u8,
        order: u32,
        f1: f32,
        f2: f32,
        gain_db: f32,
    },
    FftFilter {
        points: Vec<(f32, f32)>,
    },
    StudioReverb {
        full: bool,
        decay: f32,
        predelay_ms: f32,
        damping: f32,
        width: f32,
        early: f32,
        dry: f32,
        wet: f32,
    },
    ConvolutionReverb {
        kind: u8,
        mix: f32,
    },
    GuitarSuite {
        comp: f32,
        drive: f32,
        tone_hz: f32,
        cab_hz: f32,
        mix: f32,
    },
    Mastering {
        low_db: f32,
        mid_db: f32,
        high_db: f32,
        exciter: f32,
        width: f32,
        maximize_db: f32,
    },
    VocalEnhancer {
        mode: u8,
    },
    CenterChannel {
        extract: bool,
        level_db: f32,
        lo_hz: f32,
        hi_hz: f32,
    },
    /// Graphic Phase Shifter: a phase curve (Hz, degrees) applied per frequency; the older fixed
    /// left/right shift is used when the curve is empty. Channel 0 both, 1 left, 2 right.
    PhaseShifter {
        left_deg: f32,
        right_deg: f32,
        freq: f32,
        #[serde(default)]
        points: Vec<(f32, f32)>,
        #[serde(default = "yes_log")]
        log: bool,
        #[serde(default)]
        range360: bool,
        #[serde(default)]
        channel: u8,
        #[serde(default = "fft_4096")]
        fft: u32,
    },
    /// 0 Adaptive Noise Reduction, 1 DeNoise, 2 Hiss Reduction, 3 DeReverb.
    SpectralRepair {
        mode: u8,
        amount: f32,
        reduce_db: f32,
        cutoff_hz: f32,
    },
    ClickRemover {
        sensitivity: f32,
    },
    /// Gate, compressor and limiter in one (Amplitude and Compression > Dynamics).
    Dynamics {
        gate_db: f32,
        comp_db: f32,
        ratio: f32,
        limit_db: f32,
        makeup: f32,
    },
    /// Measures loudness while audio passes unchanged (radar: the circular display).
    LoudnessMeter {
        radar: bool,
        target: f32,
    },
    SurroundReverb {
        decay: f32,
        predelay_ms: f32,
        damping: f32,
        center: f32,
        dry: f32,
        wet: f32,
    },
    /// Scale 0 major, 1 minor, 2 chromatic; key 0 = C; reference channel 0 left, 1 right, 2 both.
    AutoPitch {
        scale: u8,
        key: u8,
        attack: f32,
        sensitivity: f32,
        ref_channel: u8,
        fft: u32,
        a4: f32,
    },
    /// Lines the channels up in time (rack version of the process).
    PhaseCorrection {
        max_ms: f32,
    },
}

impl Effect {
    pub fn name(&self) -> &str {
        match self {
            Effect::Plugin { name, .. } => name.as_str(),
            Effect::Imported { name, .. } => name.as_str(),
            Effect::Phaser { .. } => "Phaser",
            Effect::ChorusFlanger { .. } => "Chorus/Flanger",
            Effect::AnalogDelay { .. } => "Analog Delay",
            Effect::Echo { .. } => "Echo",
            Effect::DeEsser { .. } => "DeEsser",
            Effect::Multiband { .. } => "Multiband Compressor",
            Effect::DynamicsProcessing { .. } => "Dynamics Processing",
            Effect::TubeCompressor { .. } => "Tube-modeled Compressor",
            Effect::SpeechLeveler { .. } => "Speech Volume Leveler",
            Effect::ChannelMixer { .. } => "Channel Mixer",
            Effect::GraphicEq { .. } => "Graphic Equalizer",
            Effect::NotchFilter { .. } => "Notch Filter",
            Effect::ScientificFilter { .. } => "Scientific Filter",
            Effect::FftFilter { .. } => "FFT Filter",
            Effect::StudioReverb { full, .. } => {
                if *full {
                    "Full Reverb"
                } else {
                    "Studio Reverb"
                }
            }
            Effect::ConvolutionReverb { .. } => "Convolution Reverb",
            Effect::GuitarSuite { .. } => "Guitar Suite",
            Effect::Mastering { .. } => "Mastering",
            Effect::VocalEnhancer { .. } => "Vocal Enhancer",
            Effect::CenterChannel { .. } => "Center Channel Extractor",
            Effect::PhaseShifter { .. } => "Graphic Phase Shifter",
            Effect::SpectralRepair { mode, .. } => match mode {
                1 => "DeNoise",
                2 => "Hiss Reduction",
                3 => "DeReverb",
                _ => "Adaptive Noise Reduction",
            },
            Effect::ClickRemover { .. } => "Automatic Click Remover",
            Effect::Dynamics { .. } => "Dynamics",
            Effect::LoudnessMeter { radar, .. } => {
                if *radar {
                    "Loudness Radar Meter"
                } else {
                    "Loudness Meter"
                }
            }
            Effect::SurroundReverb { .. } => "Surround Reverb",
            Effect::AutoPitch { .. } => "Automatic Pitch Correction",
            Effect::PhaseCorrection { .. } => "Automatic Phase Correction",
            Effect::Amplify { .. } => "Amplify",
            Effect::ParametricEq { .. } => "Parametric Equalizer",
            Effect::Filter { .. } => "Simple Filter",
            Effect::Compressor { .. } => "Single-band Compressor",
            Effect::HardLimiter { .. } => "Hard Limiter",
            Effect::NoiseGate { .. } => "Noise Gate",
            Effect::Delay { .. } => "Delay",
            Effect::Reverb { .. } => "Reverb",
            Effect::Chorus { .. } => "Chorus",
            Effect::Flanger { .. } => "Flanger",
            Effect::Distortion { .. } => "Distortion",
            Effect::DeHum { .. } => "DeHummer",
            Effect::StereoExpander { .. } => "Stereo Expander",
        }
    }

    /// The effect's automatable parameters: (path, label, value, min, max). Every continuous
    /// setting is found from the effect's own stored fields (including nested ones such as EQ
    /// bands, "bands.1.gain_db"); whole-number and on/off settings are left out. Plug-in
    /// parameters are listed by name.
    /// Whether automating this effect's settings keeps its sound (tails, filter memory) while
    /// they move. The few that don't restart on each change; the automation menu says so.
    pub fn glides_when_automated(&self) -> bool {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        if matches!(self, Effect::Plugin { .. } | Effect::Imported { .. }) {
            return true;
        }
        static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
        let key = self.name().to_string();
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(v) = cache.lock().ok().and_then(|c| c.get(&key).copied()) {
            return v;
        }
        let mut a = self.make(48000.0);
        let b = self.make(48000.0);
        let v = a.retune(b.as_ref());
        if let Ok(mut c) = cache.lock() {
            c.insert(key, v);
        }
        v
    }

    pub fn auto_params(&self) -> Vec<(String, String, f32, f32, f32)> {
        let mut out = vec![];
        if let Effect::Plugin { params, .. } | Effect::Imported { params, .. } = self {
            for (i, (n, v)) in params.iter().enumerate() {
                out.push((format!("p.{i}"), n.clone(), *v, v.min(0.0), v.max(1.0)));
            }
            return out;
        }
        let Ok(serde_json::Value::Object(top)) = serde_json::to_value(self) else {
            return out;
        };
        let Some((_, body)) = top.into_iter().next() else {
            return out;
        };
        fn walk(v: &serde_json::Value, path: &str, out: &mut Vec<(String, String, f32, f32, f32)>) {
            match v {
                serde_json::Value::Object(m) => {
                    for (k, x) in m {
                        let p = if path.is_empty() {
                            k.clone()
                        } else {
                            format!("{path}.{k}")
                        };
                        walk(x, &p, out);
                    }
                }
                serde_json::Value::Array(a) if a.len() <= 32 => {
                    for (i, x) in a.iter().enumerate() {
                        walk(x, &format!("{path}.{i}"), out);
                    }
                }
                serde_json::Value::Number(n) if n.is_f64() => {
                    let val = n.as_f64().unwrap_or(0.0) as f32;
                    let (lo, hi) = param_range(path, val);
                    out.push((path.to_string(), param_label(path), val, lo, hi));
                }
                _ => {}
            }
        }
        walk(&body, "", &mut out);
        out
    }

    /// Set one parameter found by `auto_params` (clamped to its range). False when the path
    /// isn't one of this effect's.
    pub fn set_auto_param(&mut self, path: &str, v: f32) -> bool {
        if !v.is_finite() {
            return false;
        }
        if let Effect::Plugin { params, .. } | Effect::Imported { params, .. } = self {
            let Some(i) = path
                .strip_prefix("p.")
                .and_then(|s| s.parse::<usize>().ok())
            else {
                return false;
            };
            return match params.get_mut(i) {
                Some(p) => {
                    p.1 = v;
                    true
                }
                None => false,
            };
        }
        let Ok(mut top) = serde_json::to_value(&*self) else {
            return false;
        };
        let Some(body) = top.as_object_mut().and_then(|m| m.values_mut().next()) else {
            return false;
        };
        let mut cur = body;
        for part in path.split('.') {
            let next = match cur {
                serde_json::Value::Object(m) => m.get_mut(part),
                serde_json::Value::Array(a) => {
                    part.parse::<usize>().ok().and_then(|i| a.get_mut(i))
                }
                _ => None,
            };
            match next {
                Some(n) => cur = n,
                None => return false,
            }
        }
        if !cur.is_f64() {
            return false;
        }
        let (lo, hi) = param_range(path, v);
        let Some(num) = serde_json::Number::from_f64(v.clamp(lo, hi.max(lo)) as f64) else {
            return false;
        };
        *cur = serde_json::Value::Number(num);
        match serde_json::from_value::<Effect>(top) {
            Ok(e) => {
                *self = e;
                true
            }
            Err(_) => false,
        }
    }

    pub fn category(&self) -> &'static str {
        match self {
            Effect::Amplify { .. } | Effect::Compressor { .. } | Effect::HardLimiter { .. } => {
                "Amplitude and Compression"
            }
            Effect::ParametricEq { .. } | Effect::Filter { .. } => "Filter and EQ",
            Effect::NoiseGate { .. } | Effect::DeHum { .. } => "Noise Reduction / Restoration",
            Effect::Delay { .. } => "Delay and Echo",
            Effect::Reverb { .. } => "Reverb",
            Effect::Chorus { .. } | Effect::Flanger { .. } => "Modulation",
            Effect::Distortion { .. } => "Special",
            Effect::StereoExpander { .. } => "Stereo Imagery",
            Effect::Imported { .. } => "Imported",
            Effect::DeEsser { .. }
            | Effect::Multiband { .. }
            | Effect::DynamicsProcessing { .. }
            | Effect::TubeCompressor { .. }
            | Effect::SpeechLeveler { .. }
            | Effect::ChannelMixer { .. } => "Amplitude and Compression",
            Effect::AnalogDelay { .. } | Effect::Echo { .. } => "Delay and Echo",
            Effect::GraphicEq { .. }
            | Effect::NotchFilter { .. }
            | Effect::ScientificFilter { .. }
            | Effect::FftFilter { .. } => "Filter and EQ",
            Effect::Phaser { .. } | Effect::ChorusFlanger { .. } => "Modulation",
            Effect::SpectralRepair { .. } | Effect::ClickRemover { .. } => {
                "Noise Reduction / Restoration"
            }
            Effect::StudioReverb { .. } | Effect::ConvolutionReverb { .. } => "Reverb",
            Effect::GuitarSuite { .. }
            | Effect::Mastering { .. }
            | Effect::VocalEnhancer { .. } => "Special",
            Effect::CenterChannel { .. } | Effect::PhaseShifter { .. } => "Stereo Imagery",
            Effect::Dynamics { .. } => "Amplitude and Compression",
            Effect::LoudnessMeter { .. } => "Special",
            Effect::SurroundReverb { .. } => "Reverb",
            Effect::AutoPitch { .. } => "Time and Pitch",
            Effect::PhaseCorrection { .. } => "Noise Reduction / Restoration",
            Effect::Plugin { id, .. } => {
                if crate::plugins::is_hosted(id) {
                    if id.starts_with("clap:") {
                        "CLAP"
                    } else {
                        "VST3"
                    }
                } else {
                    "SoundCraft"
                }
            }
        }
    }

    pub fn all_defaults() -> Vec<Effect> {
        vec![
            Effect::Amplify { gain_db: 0.0 },
            Effect::Compressor {
                threshold: -20.0,
                ratio: 4.0,
                attack_ms: 10.0,
                release_ms: 100.0,
                makeup: 0.0,
            },
            Effect::HardLimiter {
                ceiling_db: -0.3,
                input_boost: 0.0,
                release_ms: 50.0,
            },
            Effect::ParametricEq {
                bands: default_eq_bands(),
                master_db: 0.0,
            },
            Effect::Filter {
                kind: FilterKind::HighPass,
                freq: 100.0,
                q: 0.707,
                gain_db: 0.0,
            },
            Effect::NoiseGate {
                threshold: -45.0,
                attack_ms: 2.0,
                release_ms: 120.0,
                floor_db: -60.0,
            },
            Effect::DeHum {
                freq: 50.0,
                harmonics: 4,
                q: 30.0,
            },
            Effect::Delay {
                time_ms: 350.0,
                feedback: 0.35,
                mix: 0.35,
            },
            Effect::Reverb {
                room: 0.6,
                damping: 0.5,
                width: 1.0,
                mix: 0.25,
            },
            Effect::Chorus {
                rate: 0.8,
                depth_ms: 6.0,
                mix: 0.5,
            },
            Effect::Flanger {
                rate: 0.25,
                depth_ms: 3.0,
                feedback: 0.5,
                mix: 0.5,
            },
            Effect::Distortion {
                drive_db: 12.0,
                mix: 1.0,
            },
            Effect::StereoExpander { width: 1.5 },
            Effect::Phaser {
                stages: 8,
                rate: 0.4,
                depth: 0.8,
                feedback: 0.4,
                mix: 0.5,
            },
            Effect::ChorusFlanger {
                flanger: false,
                rate: 0.6,
                depth_ms: 5.0,
                feedback: 0.2,
                mix: 0.5,
            },
            Effect::AnalogDelay {
                time_ms: 300.0,
                feedback: 0.45,
                drive: 0.4,
                spread_ms: 15.0,
                tone_hz: 3500.0,
                mix: 0.35,
            },
            Effect::Echo {
                left_ms: 250.0,
                right_ms: 375.0,
                feedback: 0.5,
                tone_hz: 6000.0,
                mix: 0.35,
            },
            Effect::DeEsser {
                freq: 6500.0,
                threshold: -30.0,
                reduction: 10.0,
            },
            Effect::Multiband {
                x1: 200.0,
                x2: 3000.0,
                thr_low: -18.0,
                thr_mid: -18.0,
                thr_high: -18.0,
                ratio: 3.0,
                makeup: 2.0,
            },
            Effect::DynamicsProcessing {
                gate_db: -55.0,
                comp_db: -20.0,
                ratio: 3.0,
                ceiling_db: -0.5,
                makeup: 0.0,
            },
            Effect::TubeCompressor {
                threshold: -20.0,
                ratio: 3.0,
                attack_ms: 10.0,
                release_ms: 120.0,
                drive: 0.4,
                makeup: 2.0,
            },
            Effect::SpeechLeveler {
                target_db: -20.0,
                max_gain_db: 12.0,
                gate_db: -50.0,
                speed: 0.4,
            },
            Effect::ChannelMixer {
                ll: 100.0,
                lr: 0.0,
                rl: 0.0,
                rr: 100.0,
            },
            Effect::GraphicEq {
                gains: vec![0.0; 10],
                master_db: 0.0,
            },
            Effect::NotchFilter {
                freqs: vec![1000.0],
                depth_db: -30.0,
                q: 12.0,
            },
            Effect::ScientificFilter {
                kind: 0,
                order: 6,
                f1: 8000.0,
                f2: 12000.0,
                gain_db: 0.0,
            },
            Effect::FftFilter {
                points: vec![(20.0, 0.0), (500.0, 0.0), (5000.0, 0.0), (20000.0, 0.0)],
            },
            Effect::StudioReverb {
                full: false,
                decay: 0.6,
                predelay_ms: 20.0,
                damping: 0.5,
                width: 1.0,
                early: 0.4,
                dry: 0.8,
                wet: 0.3,
            },
            Effect::StudioReverb {
                full: true,
                decay: 0.75,
                predelay_ms: 35.0,
                damping: 0.45,
                width: 1.0,
                early: 0.8,
                dry: 0.75,
                wet: 0.35,
            },
            Effect::ConvolutionReverb { kind: 1, mix: 0.3 },
            Effect::GuitarSuite {
                comp: 0.5,
                drive: 0.6,
                tone_hz: 3500.0,
                cab_hz: 5000.0,
                mix: 1.0,
            },
            Effect::Mastering {
                low_db: 1.5,
                mid_db: 0.0,
                high_db: 1.5,
                exciter: 0.3,
                width: 1.1,
                maximize_db: 4.0,
            },
            Effect::VocalEnhancer { mode: 0 },
            Effect::CenterChannel {
                extract: false,
                level_db: -40.0,
                lo_hz: 120.0,
                hi_hz: 8000.0,
            },
            Effect::PhaseShifter {
                left_deg: 0.0,
                right_deg: 0.0,
                freq: 1000.0,
                points: vec![(20.0, 0.0), (20000.0, 0.0)],
                log: true,
                range360: false,
                channel: 0,
                fft: 4096,
            },
            Effect::SpectralRepair {
                mode: 0,
                amount: 0.8,
                reduce_db: 18.0,
                cutoff_hz: 0.0,
            },
            Effect::SpectralRepair {
                mode: 1,
                amount: 0.6,
                reduce_db: 12.0,
                cutoff_hz: 0.0,
            },
            Effect::SpectralRepair {
                mode: 2,
                amount: 0.8,
                reduce_db: 18.0,
                cutoff_hz: 4000.0,
            },
            Effect::SpectralRepair {
                mode: 3,
                amount: 0.7,
                reduce_db: 12.0,
                cutoff_hz: 0.0,
            },
            Effect::ClickRemover { sensitivity: 0.5 },
            Effect::Dynamics {
                gate_db: -60.0,
                comp_db: -18.0,
                ratio: 3.0,
                limit_db: -0.5,
                makeup: 0.0,
            },
            Effect::LoudnessMeter {
                radar: false,
                target: -23.0,
            },
            Effect::LoudnessMeter {
                radar: true,
                target: -23.0,
            },
            Effect::SurroundReverb {
                decay: 0.6,
                predelay_ms: 25.0,
                damping: 0.5,
                center: 0.5,
                dry: 0.8,
                wet: 0.35,
            },
            Effect::AutoPitch {
                scale: 1,
                key: 3,
                attack: 7.0,
                sensitivity: 66.0,
                ref_channel: 0,
                fft: 4096,
                a4: 440.0,
            },
            Effect::PhaseCorrection { max_ms: 5.0 },
        ]
    }

    /// Draw the parameter controls. Returns true if something changed.
    pub fn ui(&mut self, ui: &mut egui::Ui, rate: f32) -> bool {
        let mut ch = false;
        match self {
            Effect::Amplify { gain_db } => {
                let gain = *gain_db;
                let ticks = crate::fx_ui::db_ticks(-40.0, 24.0, 8.0);
                crate::fx_ui::graph(
                    ui,
                    120.0,
                    -40.0,
                    24.0,
                    -40.0,
                    24.0,
                    |_| gain,
                    &ticks,
                    &ticks,
                    &[],
                    true,
                );
                ch |= crate::fx_ui::bank(ui, "Gain", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Gain",
                            &mut *gain_db,
                            -40.0,
                            24.0,
                            " dB",
                            false,
                            0.0,
                        );
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("Level change")
                                    .small()
                                    .color(crate::theme::text_dim()),
                            );
                            b |= slider(ui, "Precise", &mut *gain_db, -40.0..=24.0, " dB", false);
                        });
                        b
                    })
                });
            }
            Effect::ParametricEq { bands, master_db } => {
                ch |= crate::equi::parametric(ui, bands, master_db, rate);
            }
            Effect::Filter {
                kind,
                freq,
                q,
                gain_db,
            } => {
                let biquad = Biquad::new(*kind, rate, *freq, *q, *gain_db);
                crate::equi::response_view(ui, &[biquad], rate, 24.0, 0.0);
                egui::ComboBox::from_label("Type")
                    .selected_text(kind.name())
                    .show_ui(ui, |ui| {
                        for k in FilterKind::ALL {
                            ch |= ui.selectable_value(kind, k, k.name()).changed();
                        }
                    });
                let uses_gain = kind.uses_gain();
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Frequency",
                            &mut *freq,
                            20.0,
                            20000.0,
                            " Hz",
                            true,
                            100.0,
                        );
                        b |= crate::fx_ui::knob(ui, "Q", &mut *q, 0.1, 20.0, "", true, 0.707);
                        if uses_gain {
                            b |= crate::fx_ui::knob(
                                ui,
                                "Gain",
                                &mut *gain_db,
                                -30.0,
                                30.0,
                                " dB",
                                false,
                                0.0,
                            );
                        }
                        b
                    })
                });
            }
            Effect::Compressor {
                threshold,
                ratio,
                attack_ms,
                release_ms,
                makeup,
            } => {
                let (th, rt) = (*threshold, (*ratio).max(1.0));
                let ticks = crate::fx_ui::db_ticks(-60.0, 0.0, 12.0);
                crate::fx_ui::graph(
                    ui,
                    132.0,
                    -60.0,
                    0.0,
                    -60.0,
                    0.0,
                    |x| if x > th { th + (x - th) / rt } else { x },
                    &ticks,
                    &ticks,
                    &[(
                        th,
                        "Threshold".into(),
                        egui::Color32::from_rgb(230, 180, 70),
                    )],
                    true,
                );
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Threshold",
                            &mut *threshold,
                            -60.0,
                            0.0,
                            " dB",
                            false,
                            -20.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Ratio",
                            &mut *ratio,
                            1.0,
                            30.0,
                            ":1",
                            true,
                            4.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Attack",
                            &mut *attack_ms,
                            0.1,
                            200.0,
                            " ms",
                            true,
                            10.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Release",
                            &mut *release_ms,
                            5.0,
                            2000.0,
                            " ms",
                            true,
                            100.0,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    crate::fx_ui::knob(
                        ui,
                        "Output Gain",
                        &mut *makeup,
                        -20.0,
                        30.0,
                        " dB",
                        false,
                        0.0,
                    )
                });
            }
            Effect::HardLimiter {
                ceiling_db,
                input_boost,
                release_ms,
            } => {
                let ceil = *ceiling_db;
                let ticks = crate::fx_ui::db_ticks(-48.0, 0.0, 12.0);
                crate::fx_ui::graph(
                    ui,
                    132.0,
                    -48.0,
                    0.0,
                    -48.0,
                    0.0,
                    |x| x.min(ceil),
                    &ticks,
                    &ticks,
                    &[(
                        ceil,
                        "Ceiling".into(),
                        egui::Color32::from_rgb(230, 180, 70),
                    )],
                    true,
                );
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Maximum Amplitude",
                            &mut *ceiling_db,
                            -24.0,
                            0.0,
                            " dB",
                            false,
                            -0.3,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Input Boost",
                            &mut *input_boost,
                            0.0,
                            24.0,
                            " dB",
                            false,
                            0.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Release Time",
                            &mut *release_ms,
                            1.0,
                            500.0,
                            " ms",
                            true,
                            50.0,
                        );
                        b
                    })
                });
            }
            Effect::NoiseGate {
                threshold,
                attack_ms,
                release_ms,
                floor_db,
            } => {
                let (th, floor) = (*threshold, *floor_db);
                let ticks = crate::fx_ui::db_ticks(-90.0, 0.0, 15.0);
                crate::fx_ui::graph(
                    ui,
                    132.0,
                    -90.0,
                    0.0,
                    -90.0,
                    0.0,
                    |x| {
                        if x < th {
                            floor.max(-90.0)
                        } else if x < th + 6.0 {
                            floor + (x - th) / 6.0 * (th + 6.0 - floor)
                        } else {
                            x
                        }
                    },
                    &ticks,
                    &ticks,
                    &[(
                        th,
                        "Threshold".into(),
                        egui::Color32::from_rgb(230, 180, 70),
                    )],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Threshold",
                            &mut *threshold,
                            -90.0,
                            0.0,
                            " dB",
                            false,
                            -45.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Attack",
                            &mut *attack_ms,
                            0.1,
                            100.0,
                            " ms",
                            true,
                            2.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Release",
                            &mut *release_ms,
                            5.0,
                            2000.0,
                            " ms",
                            true,
                            120.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Floor",
                            &mut *floor_db,
                            -90.0,
                            0.0,
                            " dB",
                            false,
                            -60.0,
                        );
                        b
                    })
                });
            }
            Effect::Delay {
                time_ms,
                feedback,
                mix,
            } => {
                let (tm, fb) = ((*time_ms).max(1.0), *feedback);
                let ticks = vec![
                    (0.0, "0".into()),
                    (250.0, "250".into()),
                    (500.0, "500".into()),
                    (750.0, "750".into()),
                    (1000.0, "1000 ms".into()),
                ];
                let yticks = vec![(0.0, "0".into()), (0.5, "0.5".into()), (1.0, "1".into())];
                let marks = vec![
                    (tm.min(1000.0), "1st".into(), crate::fx_ui::VALUE),
                    (
                        (tm * 2.0).min(1000.0),
                        "2nd".into(),
                        crate::theme::time_blue(),
                    ),
                ];
                crate::fx_ui::graph(
                    ui,
                    128.0,
                    0.0,
                    1000.0,
                    0.0,
                    1.0,
                    |x| {
                        let k = (x / tm).floor();
                        if k < 1.0 {
                            1.0
                        } else {
                            fb.clamp(0.0, 0.97).powf(k - 1.0)
                        }
                    },
                    &ticks,
                    &yticks,
                    &marks,
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Delay", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Delay Time",
                            &mut *time_ms,
                            1.0,
                            2000.0,
                            " ms",
                            true,
                            350.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Feedback",
                            &mut *feedback,
                            0.0,
                            0.95,
                            "",
                            false,
                            0.35,
                        );
                        b |= crate::fx_ui::knob(ui, "Mix", &mut *mix, 0.0, 1.0, "", false, 0.35);
                        b
                    })
                });
            }
            Effect::Reverb {
                room,
                damping,
                width,
                mix,
            } => {
                let (sz, damp) = (*room, *damping);
                let ticks = vec![
                    (0.0, "0".into()),
                    (0.25, "0.25".into()),
                    (0.5, "0.5".into()),
                    (0.75, "0.75".into()),
                    (1.0, "1 s".into()),
                ];
                let yticks = vec![(0.0, "0".into()), (0.5, "0.5".into()), (1.0, "1".into())];
                crate::fx_ui::graph(
                    ui,
                    128.0,
                    0.0,
                    1.0,
                    0.0,
                    1.0,
                    |x| (-x * (1.0 + sz * 7.0) * (1.0 + damp * 0.5)).exp(),
                    &ticks,
                    &yticks,
                    &[(0.04, "Early".into(), crate::theme::time_blue())],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Room Size",
                            &mut *room,
                            0.0,
                            1.0,
                            "",
                            false,
                            0.6,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Damping",
                            &mut *damping,
                            0.0,
                            1.0,
                            "",
                            false,
                            0.5,
                        );
                        b |= crate::fx_ui::knob(ui, "Width", &mut *width, 0.0, 1.0, "", false, 1.0);
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    crate::fx_ui::knob(ui, "Mix", &mut *mix, 0.0, 1.0, "", false, 0.25)
                });
            }
            Effect::Chorus {
                rate: r,
                depth_ms,
                mix,
            } => {
                let sp = *r;
                let ticks = vec![
                    (0.0, "0".into()),
                    (0.25, "0.25".into()),
                    (0.5, "0.5".into()),
                    (0.75, "0.75".into()),
                    (1.0, "1 s".into()),
                ];
                let yticks = vec![(-1.0, "-1".into()), (0.0, "0".into()), (1.0, "1".into())];
                crate::fx_ui::graph(
                    ui,
                    120.0,
                    0.0,
                    1.0,
                    -1.0,
                    1.0,
                    |x| (x * sp * std::f32::consts::TAU).sin(),
                    &ticks,
                    &yticks,
                    &[],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Modulation", |ui| {
                    ui.horizontal(|ui| {
                        let mut b =
                            crate::fx_ui::knob(ui, "Speed", &mut *r, 0.05, 10.0, " Hz", true, 0.8);
                        b |= crate::fx_ui::knob(
                            ui,
                            "Depth",
                            &mut *depth_ms,
                            0.5,
                            20.0,
                            " ms",
                            false,
                            6.0,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    crate::fx_ui::knob(ui, "Mix", &mut *mix, 0.0, 1.0, "", false, 0.5)
                });
            }
            Effect::Flanger {
                rate: r,
                depth_ms,
                feedback,
                mix,
            } => {
                let (sp, dep, fb) = (*r, *depth_ms, *feedback);
                let ticks = vec![
                    (0.0, "0".into()),
                    (0.25, "0.25".into()),
                    (0.5, "0.5".into()),
                    (0.75, "0.75".into()),
                    (1.0, "1 s".into()),
                ];
                let yticks = vec![(-1.0, "-1".into()), (0.0, "0".into()), (1.0, "1".into())];
                crate::fx_ui::graph(
                    ui,
                    120.0,
                    0.0,
                    1.0,
                    -1.0,
                    1.0,
                    |x| (x * sp * std::f32::consts::TAU).sin(),
                    &ticks,
                    &yticks,
                    &[],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Modulation", |ui| {
                    ui.horizontal(|ui| {
                        let mut b =
                            crate::fx_ui::knob(ui, "Speed", &mut *r, 0.05, 10.0, " Hz", true, 0.25);
                        b |= crate::fx_ui::knob(
                            ui,
                            "Depth",
                            &mut *depth_ms,
                            0.1,
                            10.0,
                            " ms",
                            false,
                            3.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Feedback",
                            &mut *feedback,
                            0.0,
                            0.95,
                            "",
                            false,
                            0.5,
                        );
                        let _ = (dep, fb);
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    crate::fx_ui::knob(ui, "Mix", &mut *mix, 0.0, 1.0, "", false, 0.5)
                });
            }
            Effect::Distortion { drive_db, mix } => {
                let drive = undb(*drive_db).max(0.01);
                let norm = drive.tanh().max(1e-3);
                let ticks = vec![
                    (-1.0, "-1".into()),
                    (-0.5, "-0.5".into()),
                    (0.0, "0".into()),
                    (0.5, "0.5".into()),
                    (1.0, "1".into()),
                ];
                crate::fx_ui::graph(
                    ui,
                    140.0,
                    -1.0,
                    1.0,
                    -1.0,
                    1.0,
                    |x| (drive * x).tanh() / norm,
                    &ticks,
                    &ticks,
                    &[],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    crate::fx_ui::knob(ui, "Drive", &mut *drive_db, 0.0, 40.0, " dB", false, 12.0)
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    crate::fx_ui::knob(ui, "Mix", &mut *mix, 0.0, 1.0, "", false, 1.0)
                });
            }
            Effect::DeHum { freq, harmonics, q } => {
                let bank = (1..=(*harmonics).max(1).min(10))
                    .filter_map(|h| {
                        let f = *freq * h as f32;
                        (f > 10.0 && f < rate * 0.45)
                            .then(|| Biquad::new(FilterKind::Notch, rate, f, *q, 0.0))
                    })
                    .collect::<Vec<_>>();
                crate::equi::response_view(ui, &bank, rate, 48.0, 0.0);
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = ui.radio_value(freq, 50.0, "50 Hz").changed();
                        b |= ui.radio_value(freq, 60.0, "60 Hz").changed();
                        ui.add_space(10.0);
                        b |= ui
                            .add(crate::theme::RSlider::new(harmonics, 1..=10).text("Harmonics"))
                            .changed();
                        b |= crate::fx_ui::knob(ui, "Q", &mut *q, 5.0, 100.0, "", true, 30.0);
                        b
                    })
                });
            }
            Effect::StereoExpander { width } => {
                ch |= crate::fx_ui::bank(ui, "Stereo Image", |ui| {
                    crate::fx_ui::knob(ui, "Width", &mut *width, 0.0, 3.0, "x", false, 1.5)
                });
            }
            Effect::Plugin { id, params, .. } => {
                ch |= crate::plugins::params_ui(ui, id, params);
            }
            Effect::Imported { name, params } => {
                ui.colored_label(egui::Color32::from_rgb(235, 180, 80), format!("\u{201c}{name}\u{201d} came from an imported session. AudioCraft doesn't have this effect yet, so audio passes through unchanged. Its stored settings are kept and saved back."));
                egui::Grid::new(ui.next_auto_id())
                    .num_columns(2)
                    .striped(true)
                    .show(ui, |ui| {
                        for (k, v) in params.iter() {
                            ui.label(k);
                            ui.label(format!("{v:.4}"));
                            ui.end_row();
                        }
                    });
            }
            Effect::Phaser {
                stages,
                rate: r,
                depth,
                feedback,
                mix,
            } => {
                let sp = *r;
                let ticks = vec![
                    (0.0, "0".into()),
                    (0.25, "0.25".into()),
                    (0.5, "0.5".into()),
                    (0.75, "0.75".into()),
                    (1.0, "1 s".into()),
                ];
                let yticks = vec![(-1.0, "-1".into()), (0.0, "0".into()), (1.0, "1".into())];
                crate::fx_ui::graph(
                    ui,
                    120.0,
                    0.0,
                    1.0,
                    -1.0,
                    1.0,
                    |x| (x * sp * std::f32::consts::TAU).sin(),
                    &ticks,
                    &yticks,
                    &[],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = ui
                            .add(
                                crate::theme::RSlider::new(stages, 2..=12)
                                    .step_by(2.0)
                                    .text("Stages"),
                            )
                            .changed();
                        b |= crate::fx_ui::knob(ui, "Speed", &mut *r, 0.02, 8.0, " Hz", true, 0.4);
                        b |= crate::fx_ui::knob(ui, "Depth", &mut *depth, 0.0, 1.0, "", false, 0.8);
                        b |= crate::fx_ui::knob(
                            ui,
                            "Feedback",
                            &mut *feedback,
                            -0.9,
                            0.9,
                            "",
                            false,
                            0.4,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    crate::fx_ui::knob(ui, "Mix", &mut *mix, 0.0, 1.0, "", false, 0.5)
                });
            }
            Effect::ChorusFlanger {
                flanger,
                rate: r,
                depth_ms,
                feedback,
                mix,
            } => {
                let sp = *r;
                let ticks = vec![
                    (0.0, "0".into()),
                    (0.25, "0.25".into()),
                    (0.5, "0.5".into()),
                    (0.75, "0.75".into()),
                    (1.0, "1 s".into()),
                ];
                let yticks = vec![(-1.0, "-1".into()), (0.0, "0".into()), (1.0, "1".into())];
                crate::fx_ui::graph(
                    ui,
                    120.0,
                    0.0,
                    1.0,
                    -1.0,
                    1.0,
                    |x| (x * sp * std::f32::consts::TAU).sin(),
                    &ticks,
                    &yticks,
                    &[],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Type", |ui| {
                    let mut b = ui.radio_value(flanger, false, "Chorus").changed();
                    b |= ui.radio_value(flanger, true, "Flanger").changed();
                    b
                });
                ch |= crate::fx_ui::bank(ui, "Modulation", |ui| {
                    ui.horizontal(|ui| {
                        let mut b =
                            crate::fx_ui::knob(ui, "Speed", &mut *r, 0.02, 8.0, " Hz", true, 0.6);
                        b |= crate::fx_ui::knob(
                            ui,
                            "Width",
                            &mut *depth_ms,
                            0.2,
                            20.0,
                            " ms",
                            false,
                            5.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Feedback",
                            &mut *feedback,
                            0.0,
                            0.95,
                            "",
                            false,
                            0.2,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    crate::fx_ui::knob(ui, "Mix", &mut *mix, 0.0, 1.0, "", false, 0.5)
                });
            }
            Effect::AnalogDelay {
                time_ms,
                feedback,
                drive,
                spread_ms,
                tone_hz,
                mix,
            } => {
                let (tm, fb) = ((*time_ms).max(1.0), *feedback);
                let ticks = crate::fx_ui::db_ticks(0.0, 1000.0, 250.0);
                let yticks = vec![(0.0, "0".into()), (0.5, "0.5".into()), (1.0, "1".into())];
                crate::fx_ui::graph(
                    ui,
                    128.0,
                    0.0,
                    1000.0,
                    0.0,
                    1.0,
                    |x| {
                        let k = (x / tm).floor();
                        if k < 1.0 {
                            1.0
                        } else {
                            fb.clamp(0.0, 0.97).powf(k - 1.0)
                        }
                    },
                    &ticks,
                    &yticks,
                    &[(tm.min(1000.0), "Delay".into(), crate::fx_ui::VALUE)],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Delay", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Delay",
                            &mut *time_ms,
                            1.0,
                            4000.0,
                            " ms",
                            true,
                            300.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Feedback",
                            &mut *feedback,
                            0.0,
                            0.97,
                            "",
                            false,
                            0.45,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Spread",
                            &mut *spread_ms,
                            0.0,
                            100.0,
                            " ms",
                            false,
                            15.0,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Tape Character", |ui| {
                    ui.horizontal(|ui| {
                        let mut b =
                            crate::fx_ui::knob(ui, "Trash", &mut *drive, 0.0, 1.0, "", false, 0.4);
                        b |= crate::fx_ui::knob(
                            ui,
                            "Tone",
                            &mut *tone_hz,
                            300.0,
                            16000.0,
                            " Hz",
                            true,
                            3500.0,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    crate::fx_ui::knob(ui, "Mix", &mut *mix, 0.0, 1.0, "", false, 0.35)
                });
            }
            Effect::Echo {
                left_ms,
                right_ms,
                feedback,
                tone_hz,
                mix,
            } => {
                let (lt, rt) = ((*left_ms).max(1.0), (*right_ms).max(1.0));
                let ticks = crate::fx_ui::db_ticks(0.0, 1000.0, 250.0);
                let yticks = vec![(0.0, "0".into()), (0.5, "0.5".into()), (1.0, "1".into())];
                crate::fx_ui::graph(
                    ui,
                    128.0,
                    0.0,
                    1000.0,
                    0.0,
                    1.0,
                    |x| {
                        let a: f32 = if x < lt { 1.0 } else { 0.0 };
                        let b: f32 = if x < rt { 0.7 } else { 0.0 };
                        a.max(b * (*feedback).clamp(0.0, 0.95))
                    },
                    &ticks,
                    &yticks,
                    &[
                        (lt.min(1000.0), "L".into(), crate::fx_ui::VALUE),
                        (rt.min(1000.0), "R".into(), crate::theme::time_blue()),
                    ],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Echoes", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Left Delay",
                            &mut *left_ms,
                            1.0,
                            5000.0,
                            " ms",
                            true,
                            250.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Right Delay",
                            &mut *right_ms,
                            1.0,
                            5000.0,
                            " ms",
                            true,
                            375.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Feedback",
                            &mut *feedback,
                            0.0,
                            0.95,
                            "",
                            false,
                            0.5,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Tone", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Echo Tone",
                            &mut *tone_hz,
                            300.0,
                            16000.0,
                            " Hz",
                            true,
                            6000.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Echo Level",
                            &mut *mix,
                            0.0,
                            1.0,
                            "",
                            false,
                            0.35,
                        );
                        b
                    })
                });
            }
            Effect::DeEsser {
                freq,
                threshold,
                reduction,
            } => {
                let fc = *freq;
                let ticks = crate::fx_ui::db_ticks(0.0, 16000.0, 4000.0);
                let yticks = crate::fx_ui::db_ticks(-40.0, 0.0, 10.0);
                crate::fx_ui::graph(
                    ui,
                    120.0,
                    0.0,
                    16000.0,
                    -40.0,
                    0.0,
                    |x| -24.0 * (-(x - fc).powi(2) / (2.0 * 2500.0f32.powi(2))).exp(),
                    &ticks,
                    &yticks,
                    &[(fc, "Sibilance".into(), crate::theme::time_blue())],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Center Frequency",
                            &mut *freq,
                            2000.0,
                            16000.0,
                            " Hz",
                            true,
                            6500.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Threshold",
                            &mut *threshold,
                            -60.0,
                            0.0,
                            " dB",
                            false,
                            -30.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Max Reduction",
                            &mut *reduction,
                            0.0,
                            40.0,
                            " dB",
                            false,
                            10.0,
                        );
                        b
                    })
                });
            }
            Effect::Multiband {
                x1,
                x2,
                thr_low,
                thr_mid,
                thr_high,
                ratio,
                makeup,
            } => {
                let (t1, t2, t3, rt) = (*thr_low, *thr_mid, *thr_high, (*ratio).max(1.0));
                let ticks = crate::fx_ui::db_ticks(-60.0, 0.0, 12.0);
                crate::fx_ui::graph(
                    ui,
                    132.0,
                    -60.0,
                    0.0,
                    -60.0,
                    0.0,
                    |x| {
                        let th = if x < 250.0 {
                            t1
                        } else if x < 4000.0 {
                            t2
                        } else {
                            t3
                        };
                        if x > th { th + (x - th) / rt } else { x }
                    },
                    &ticks,
                    &ticks,
                    &[
                        (t1, "Low".into(), crate::theme::time_blue()),
                        (t2, "Mid".into(), crate::fx_ui::VALUE),
                        (t3, "High".into(), egui::Color32::from_rgb(230, 180, 70)),
                    ],
                    true,
                );
                ch |= crate::fx_ui::bank(ui, "Crossovers", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Low / Mid",
                            &mut *x1,
                            40.0,
                            2000.0,
                            " Hz",
                            true,
                            200.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Mid / High",
                            &mut *x2,
                            500.0,
                            16000.0,
                            " Hz",
                            true,
                            3000.0,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Thresholds", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Low",
                            &mut *thr_low,
                            -60.0,
                            0.0,
                            " dB",
                            false,
                            -18.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Mid",
                            &mut *thr_mid,
                            -60.0,
                            0.0,
                            " dB",
                            false,
                            -18.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "High",
                            &mut *thr_high,
                            -60.0,
                            0.0,
                            " dB",
                            false,
                            -18.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Ratio",
                            &mut *ratio,
                            1.0,
                            20.0,
                            ":1",
                            true,
                            3.0,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    crate::fx_ui::knob(
                        ui,
                        "Output Gain",
                        &mut *makeup,
                        -12.0,
                        24.0,
                        " dB",
                        false,
                        2.0,
                    )
                });
            }
            Effect::DynamicsProcessing {
                gate_db,
                comp_db,
                ratio,
                ceiling_db,
                makeup,
            } => {
                let (g, c, lim, rt) = (*gate_db, *comp_db, *ceiling_db, (*ratio).max(1.0));
                let ticks = crate::fx_ui::db_ticks(-90.0, 0.0, 15.0);
                crate::fx_ui::graph(
                    ui,
                    132.0,
                    -90.0,
                    0.0,
                    -90.0,
                    0.0,
                    |x| {
                        let v = if x < g {
                            -90.0
                        } else if x > c {
                            c + (x - c) / rt
                        } else {
                            x
                        };
                        v.min(lim)
                    },
                    &ticks,
                    &ticks,
                    &[
                        (g, "Gate".into(), crate::theme::time_blue()),
                        (c, "Comp".into(), crate::fx_ui::VALUE),
                        (lim, "Limit".into(), egui::Color32::from_rgb(230, 180, 70)),
                    ],
                    true,
                );
                ch |= crate::fx_ui::bank(ui, "Dynamics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Expander Below",
                            &mut *gate_db,
                            -90.0,
                            -10.0,
                            " dB",
                            false,
                            -55.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Compress Above",
                            &mut *comp_db,
                            -60.0,
                            0.0,
                            " dB",
                            false,
                            -20.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Ratio",
                            &mut *ratio,
                            1.0,
                            20.0,
                            ":1",
                            true,
                            3.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Limit At",
                            &mut *ceiling_db,
                            -24.0,
                            0.0,
                            " dB",
                            false,
                            -0.5,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    crate::fx_ui::knob(
                        ui,
                        "Make-up Gain",
                        &mut *makeup,
                        -12.0,
                        24.0,
                        " dB",
                        false,
                        0.0,
                    )
                });
            }
            Effect::TubeCompressor {
                threshold,
                ratio,
                attack_ms,
                release_ms,
                drive,
                makeup,
            } => {
                let (th, rt) = (*threshold, (*ratio).max(1.0));
                let ticks = crate::fx_ui::db_ticks(-60.0, 0.0, 12.0);
                crate::fx_ui::graph(
                    ui,
                    132.0,
                    -60.0,
                    0.0,
                    -60.0,
                    0.0,
                    |x| if x > th { th + (x - th) / rt } else { x },
                    &ticks,
                    &ticks,
                    &[(
                        th,
                        "Threshold".into(),
                        egui::Color32::from_rgb(230, 180, 70),
                    )],
                    true,
                );
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Threshold",
                            &mut *threshold,
                            -60.0,
                            0.0,
                            " dB",
                            false,
                            -20.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Ratio",
                            &mut *ratio,
                            1.0,
                            30.0,
                            ":1",
                            true,
                            3.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Attack",
                            &mut *attack_ms,
                            0.1,
                            200.0,
                            " ms",
                            true,
                            10.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Release",
                            &mut *release_ms,
                            5.0,
                            2000.0,
                            " ms",
                            true,
                            120.0,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Tube", |ui| {
                    ui.horizontal(|ui| {
                        let mut b =
                            crate::fx_ui::knob(ui, "Drive", &mut *drive, 0.0, 1.0, "", false, 0.4);
                        b |= crate::fx_ui::knob(
                            ui,
                            "Output Gain",
                            &mut *makeup,
                            -12.0,
                            24.0,
                            " dB",
                            false,
                            2.0,
                        );
                        b
                    })
                });
            }
            Effect::SpeechLeveler {
                target_db,
                max_gain_db,
                gate_db,
                speed,
            } => {
                let (target, gate) = (*target_db, *gate_db);
                let ticks = crate::fx_ui::db_ticks(-60.0, 0.0, 12.0);
                crate::fx_ui::graph(
                    ui,
                    120.0,
                    -60.0,
                    0.0,
                    -60.0,
                    0.0,
                    |x| {
                        if x < gate {
                            x
                        } else {
                            x + (target - x).clamp(-*max_gain_db, *max_gain_db).min(0.0)
                        }
                    },
                    &ticks,
                    &ticks,
                    &[
                        (gate, "Gate".into(), crate::theme::time_blue()),
                        (target, "Target".into(), crate::fx_ui::VALUE),
                    ],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Leveling", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Target Level",
                            &mut *target_db,
                            -40.0,
                            -6.0,
                            " dB",
                            false,
                            -20.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Max Gain",
                            &mut *max_gain_db,
                            0.0,
                            30.0,
                            " dB",
                            false,
                            12.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Noise Gate",
                            &mut *gate_db,
                            -80.0,
                            -20.0,
                            " dB",
                            false,
                            -50.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Response",
                            &mut *speed,
                            0.05,
                            1.0,
                            "",
                            false,
                            0.4,
                        );
                        b
                    })
                });
            }
            Effect::ChannelMixer { ll, lr, rl, rr } => {
                ch |= crate::fx_ui::bank(ui, "New Left Channel", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "from Left",
                            &mut *ll,
                            -100.0,
                            100.0,
                            " %",
                            false,
                            100.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "from Right",
                            &mut *lr,
                            -100.0,
                            100.0,
                            " %",
                            false,
                            0.0,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "New Right Channel", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "from Left",
                            &mut *rl,
                            -100.0,
                            100.0,
                            " %",
                            false,
                            0.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "from Right",
                            &mut *rr,
                            -100.0,
                            100.0,
                            " %",
                            false,
                            100.0,
                        );
                        b
                    })
                });
            }
            Effect::GraphicEq { gains, master_db } => {
                {
                    let bank = crate::fx2::graphic_bank(rate, gains, *master_db);
                    let (f, g) = bank.parts();
                    crate::equi::response_view(ui, f, rate, 24.0, g);
                }
                ui.horizontal(|ui| {
                    for n in [10usize, 20, 30] {
                        if ui
                            .selectable_label(gains.len() == n, format!("{n} Bands"))
                            .clicked()
                            && gains.len() != n
                        {
                            *gains = vec![0.0; n];
                            ch = true;
                        }
                    }
                    if ui.button("Flat").clicked() {
                        gains.iter_mut().for_each(|g| *g = 0.0);
                        ch = true;
                    }
                });
                let freqs = crate::fx2::graphic_freqs(gains.len());
                egui::ScrollArea::horizontal()
                    .id_salt(ui.next_auto_id())
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (g, f) in gains.iter_mut().zip(freqs.iter()) {
                                ui.vertical(|ui| {
                                    ch |= ui
                                        .add(
                                            crate::theme::RSlider::new(g, -20.0..=20.0)
                                                .vertical()
                                                .show_value(false),
                                        )
                                        .changed();
                                    let l = if *f >= 1000.0 {
                                        format!("{:.0}k", f / 1000.0)
                                    } else {
                                        format!("{f:.0}")
                                    };
                                    ui.label(egui::RichText::new(l).small());
                                    ui.label(
                                        egui::RichText::new(format!("{:+.0}", g))
                                            .small()
                                            .color(crate::theme::time_blue()),
                                    );
                                });
                            }
                        });
                    });
                ch |= slider(ui, "Range Gain", master_db, -20.0..=20.0, " dB", false);
            }
            Effect::NotchFilter { freqs, depth_db, q } => {
                {
                    let f: Vec<Biquad> = freqs
                        .iter()
                        .filter(|f| **f > 10.0 && **f < rate * 0.45)
                        .map(|f| Biquad::new(FilterKind::Peak, rate, *f, *q, depth_db.min(0.0)))
                        .collect();
                    crate::equi::response_view(ui, &f, rate, 48.0, 0.0);
                }
                let mut remove = None;
                for (i, f) in freqs.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let sp = *f * 0.01;
                        ch |= ui
                            .add(
                                egui::DragValue::new(f)
                                    .range(20.0..=20000.0)
                                    .speed(sp)
                                    .suffix(" Hz")
                                    .max_decimals(0),
                            )
                            .changed();
                        if ui.small_button("\u{2715}").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    freqs.remove(i);
                    ch = true;
                }
                if freqs.len() < 6 && ui.button("Add Notch").clicked() {
                    freqs.push(
                        freqs
                            .last()
                            .map(|f| (f * 2.0).min(18000.0))
                            .unwrap_or(1000.0),
                    );
                    ch = true;
                }
                ch |= slider(ui, "Notch Depth", depth_db, -60.0..=0.0, " dB", false);
                ch |= slider(ui, "Narrowness (Q)", q, 1.0..=60.0, "", true);
            }
            Effect::ScientificFilter {
                kind,
                order,
                f1,
                f2,
                gain_db,
            } => {
                {
                    let bank = crate::fx2::scientific_bank(rate, *kind, *order, *f1, *f2, *gain_db);
                    let (f, g) = bank.parts();
                    crate::equi::response_view(ui, f, rate, 48.0, g);
                }
                ui.horizontal(|ui| {
                    for (k, n) in [
                        (0u8, "Low Pass"),
                        (1, "High Pass"),
                        (2, "Band Pass"),
                        (3, "Band Stop"),
                    ] {
                        ch |= ui.selectable_value(kind, k, n).changed();
                    }
                });
                ch |= ui
                    .add(
                        crate::theme::RSlider::new(order, 2..=16)
                            .step_by(2.0)
                            .text("Order (Butterworth)"),
                    )
                    .changed();
                ch |= slider(
                    ui,
                    if *kind >= 2 { "Low Cutoff" } else { "Cutoff" },
                    f1,
                    20.0..=20000.0,
                    " Hz",
                    true,
                );
                if *kind >= 2 {
                    ch |= slider(ui, "High Cutoff", f2, 20.0..=20000.0, " Hz", true);
                }
                ch |= slider(ui, "Gain", gain_db, -24.0..=24.0, " dB", false);
            }
            Effect::FftFilter { points } => {
                ui.label(egui::RichText::new("Drag the points; double-click the curve to add one, double-click a point to remove it.").small().color(crate::theme::text_dim()));
                ch |= curve_editor(ui, points, (20.0, 20000.0), (-60.0, 20.0), true);
            }
            Effect::Dynamics {
                gate_db,
                comp_db,
                ratio,
                limit_db,
                makeup,
            } => {
                let (g, c, lim, rt) = (*gate_db, *comp_db, *limit_db, (*ratio).max(1.0));
                let ticks = crate::fx_ui::db_ticks(-80.0, 0.0, 10.0);
                crate::fx_ui::graph(
                    ui,
                    132.0,
                    -80.0,
                    0.0,
                    -80.0,
                    0.0,
                    |x| {
                        let v = if x < g {
                            -80.0
                        } else if x > c {
                            c + (x - c) / rt
                        } else {
                            x
                        };
                        v.min(lim)
                    },
                    &ticks,
                    &ticks,
                    &[
                        (g, "Gate".into(), crate::theme::time_blue()),
                        (c, "Comp".into(), crate::fx_ui::VALUE),
                        (lim, "Limit".into(), egui::Color32::from_rgb(230, 180, 70)),
                    ],
                    true,
                );
                ch |= crate::fx_ui::bank(ui, "AutoGate", |ui| {
                    crate::fx_ui::knob(
                        ui,
                        "Threshold",
                        &mut *gate_db,
                        -80.0,
                        0.0,
                        " dB",
                        false,
                        -60.0,
                    )
                });
                ch |= crate::fx_ui::bank(ui, "Compressor", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Threshold",
                            &mut *comp_db,
                            -60.0,
                            0.0,
                            " dB",
                            false,
                            -18.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Ratio",
                            &mut *ratio,
                            1.0,
                            30.0,
                            ":1",
                            true,
                            3.0,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Limiter", |ui| {
                    crate::fx_ui::knob(
                        ui,
                        "Threshold",
                        &mut *limit_db,
                        -30.0,
                        0.0,
                        " dB",
                        false,
                        -0.5,
                    )
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    crate::fx_ui::knob(
                        ui,
                        "Make-up Gain",
                        &mut *makeup,
                        0.0,
                        24.0,
                        " dB",
                        false,
                        0.0,
                    )
                });
            }
            Effect::LoudnessMeter { radar, target } => {
                let (m, s, i, pk) = crate::fx3::loudness_readings();
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
                ch |= slider(ui, "Target Loudness", target, -36.0..=-6.0, " LUFS", false);
                if *radar {
                    // the radar: short-term loudness as a ring around the target
                    let (r, p) = ui.allocate_painter(
                        egui::vec2(ui.available_width().min(320.0), 220.0),
                        egui::Sense::hover(),
                    );
                    let c = r.rect.center();
                    let rad = r.rect.height().min(r.rect.width()) * 0.45;
                    p.circle_filled(c, rad, egui::Color32::from_rgb(16, 22, 30));
                    for k in 1..=6 {
                        p.circle_stroke(
                            c,
                            rad * k as f32 / 6.0,
                            egui::Stroke::new(1.0, crate::theme::gray(50)),
                        );
                    }
                    let lvl = |v: f32| ((v - (*target - 30.0)) / 40.0).clamp(0.0, 1.0) * rad;
                    p.circle_stroke(
                        c,
                        lvl(*target),
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(80, 200, 120)),
                    );
                    p.circle_filled(
                        c,
                        lvl(s),
                        egui::Color32::from_rgba_unmultiplied(80, 160, 255, 70),
                    );
                    p.circle_stroke(
                        c,
                        lvl(m),
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(230, 200, 80)),
                    );
                    p.text(
                        c,
                        egui::Align2::CENTER_CENTER,
                        format!("{i:.1} LUFS"),
                        egui::FontId::proportional(18.0),
                        egui::Color32::WHITE,
                    );
                } else {
                    for (n, v) in [
                        ("Momentary", m),
                        ("Short-term", s),
                        ("Integrated", i),
                        ("True Peak", pk),
                    ] {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [110.0, 18.0],
                                egui::Label::new(
                                    egui::RichText::new(n).color(crate::theme::text_dim()),
                                ),
                            );
                            let (r, _) = ui
                                .allocate_exact_size(egui::vec2(200.0, 12.0), egui::Sense::hover());
                            ui.painter().rect_filled(r, 0.0, crate::theme::gray(25));
                            let f = ((v + 60.0) / 60.0).clamp(0.0, 1.0);
                            let col = if v > *target + 1.0 {
                                egui::Color32::from_rgb(230, 80, 60)
                            } else {
                                egui::Color32::from_rgb(80, 200, 120)
                            };
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    r.min,
                                    egui::vec2(r.width() * f, r.height()),
                                ),
                                0.0,
                                col,
                            );
                            let tx =
                                r.left() + ((*target + 60.0) / 60.0).clamp(0.0, 1.0) * r.width();
                            ui.painter().line_segment(
                                [
                                    egui::pos2(tx, r.top() - 2.0),
                                    egui::pos2(tx, r.bottom() + 2.0),
                                ],
                                egui::Stroke::new(1.0, egui::Color32::WHITE),
                            );
                            ui.label(
                                egui::RichText::new(if v <= -99.0 {
                                    "--".to_string()
                                } else {
                                    format!("{v:.1}")
                                })
                                .color(crate::theme::time_blue()),
                            );
                        });
                    }
                }
                ui.label(
                    egui::RichText::new(
                        "Readings come from the audio playing through this effect.",
                    )
                    .small()
                    .color(crate::theme::text_dim()),
                );
            }
            Effect::SurroundReverb {
                decay,
                predelay_ms,
                damping,
                center,
                dry,
                wet,
            } => {
                let (dec, damp) = (*decay, *damping);
                let ticks = vec![
                    (0.0, "0".into()),
                    (0.25, "0.25".into()),
                    (0.5, "0.5".into()),
                    (0.75, "0.75".into()),
                    (1.0, "1 s".into()),
                ];
                let yticks = vec![(0.0, "0".into()), (0.5, "0.5".into()), (1.0, "1".into())];
                crate::fx_ui::graph(
                    ui,
                    128.0,
                    0.0,
                    1.0,
                    0.0,
                    1.0,
                    |x| (-x * (1.0 + dec * 7.0) * (1.0 + damp * 0.5)).exp(),
                    &ticks,
                    &yticks,
                    &[],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b =
                            crate::fx_ui::knob(ui, "Decay", &mut *decay, 0.0, 1.0, "", false, 0.6);
                        b |= crate::fx_ui::knob(
                            ui,
                            "Pre-delay",
                            &mut *predelay_ms,
                            0.0,
                            200.0,
                            " ms",
                            false,
                            25.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "High Frequency Cut",
                            &mut *damping,
                            0.0,
                            1.0,
                            "",
                            false,
                            0.5,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Center",
                            &mut *center,
                            0.0,
                            1.0,
                            "",
                            false,
                            0.5,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    ui.horizontal(|ui| {
                        let mut b =
                            crate::fx_ui::knob(ui, "Dry", &mut *dry, 0.0, 1.0, "", false, 0.8);
                        b |= crate::fx_ui::knob(ui, "Wet", &mut *wet, 0.0, 1.0, "", false, 0.35);
                        b
                    })
                });
            }
            Effect::AutoPitch {
                scale,
                key,
                attack,
                sensitivity,
                ref_channel,
                fft,
                a4,
            } => {
                const KEYS: [&str; 12] = [
                    "C", "C#/Db", "D", "D#/Eb", "E", "F", "F#/Gb", "G", "G#/Ab", "A", "A#/Bb", "B",
                ];
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        egui::Grid::new("autopitch")
                            .num_columns(2)
                            .spacing([10.0, 8.0])
                            .show(ui, |ui| {
                                let lab = |ui: &mut egui::Ui, t: &str| {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(t)
                                                    .color(crate::theme::text_dim()),
                                            )
                                        },
                                    );
                                };
                                lab(ui, "Scale:");
                                egui::ComboBox::from_id_salt("ap_scale")
                                    .width(140.0)
                                    .selected_text(
                                        ["Major", "Minor", "Chromatic"][(*scale as usize).min(2)],
                                    )
                                    .show_ui(ui, |ui| {
                                        for (i, n) in
                                            ["Major", "Minor", "Chromatic"].iter().enumerate()
                                        {
                                            ch |= ui.selectable_value(scale, i as u8, *n).changed();
                                        }
                                    });
                                ui.end_row();
                                lab(ui, "Key:");
                                egui::ComboBox::from_id_salt("ap_key")
                                    .width(140.0)
                                    .selected_text(KEYS[(*key as usize) % 12])
                                    .show_ui(ui, |ui| {
                                        for (i, n) in KEYS.iter().enumerate() {
                                            ch |= ui.selectable_value(key, i as u8, *n).changed();
                                        }
                                    });
                                ui.end_row();
                                lab(ui, "Attack:");
                                ch |= scale_slider(
                                    ui,
                                    attack,
                                    1.0..=10.0,
                                    &[
                                        (1.0, "Slow"),
                                        (2.0, "2"),
                                        (4.0, "4"),
                                        (6.0, "6"),
                                        (8.0, "8"),
                                        (10.0, "Fast"),
                                    ],
                                    "",
                                    0,
                                );
                                ui.end_row();
                                lab(ui, "Sensitivity:");
                                ch |= scale_slider(
                                    ui,
                                    sensitivity,
                                    0.0..=200.0,
                                    &[(50.0, "50"), (100.0, "100"), (150.0, "150"), (200.0, "200")],
                                    "",
                                    0,
                                );
                                ui.end_row();
                                lab(ui, "Reference Channel:");
                                egui::ComboBox::from_id_salt("ap_ref")
                                    .width(190.0)
                                    .selected_text(
                                        ["Left", "Right", "Both"][(*ref_channel as usize).min(2)],
                                    )
                                    .show_ui(ui, |ui| {
                                        for (i, n) in ["Left", "Right", "Both"].iter().enumerate() {
                                            ch |= ui
                                                .selectable_value(ref_channel, i as u8, *n)
                                                .changed();
                                        }
                                    });
                                ui.end_row();
                                lab(ui, "FFT Size:");
                                egui::ComboBox::from_id_salt("ap_fft")
                                    .width(140.0)
                                    .selected_text(format!("{fft}"))
                                    .show_ui(ui, |ui| {
                                        for n in [1024u32, 2048, 4096, 8192] {
                                            ch |= ui
                                                .selectable_value(fft, n, format!("{n}"))
                                                .changed();
                                        }
                                    });
                                ui.end_row();
                                lab(ui, "Calibration A4:");
                                ch |= ui
                                    .add(
                                        egui::DragValue::new(a4)
                                            .range(410.0..=470.0)
                                            .speed(0.1)
                                            .suffix(" Hz"),
                                    )
                                    .changed();
                                ui.end_row();
                            });
                    });
                    ui.add_space(30.0);
                    // correction meter, -100..+100 cents
                    ui.vertical(|ui| {
                        let cents = crate::fx3::pitch_correction_cents();
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(100));
                        ui.label(
                            egui::RichText::new(format!("Correction: {cents:.1} cents"))
                                .color(crate::theme::text_dim()),
                        );
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(70.0, 210.0), egui::Sense::hover());
                        let bar = egui::Rect::from_min_max(
                            egui::pos2(r.left() + 40.0, r.top() + 4.0),
                            egui::pos2(r.left() + 62.0, r.bottom() - 4.0),
                        );
                        let p = ui.painter();
                        p.rect_filled(bar, 0.0, crate::theme::gray(40));
                        for k in [-100.0f32, -50.0, 0.0, 50.0, 100.0] {
                            let y = bar.center().y - k / 100.0 * bar.height() * 0.5;
                            p.text(
                                egui::pos2(bar.left() - 6.0, y),
                                egui::Align2::RIGHT_CENTER,
                                format!("{k:.0}"),
                                egui::FontId::proportional(10.0),
                                crate::theme::text_dim(),
                            );
                            p.line_segment(
                                [egui::pos2(bar.right(), y), egui::pos2(bar.right() + 4.0, y)],
                                egui::Stroke::new(1.0, crate::theme::text_dim()),
                            );
                        }
                        let y = bar.center().y
                            - cents.clamp(-100.0, 100.0) / 100.0 * bar.height() * 0.5;
                        p.line_segment(
                            [egui::pos2(bar.left(), y), egui::pos2(bar.right(), y)],
                            egui::Stroke::new(2.0, egui::Color32::from_rgb(230, 50, 40)),
                        );
                    });
                });
            }
            Effect::PhaseCorrection { max_ms } => {
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    crate::fx_ui::knob(
                        ui,
                        "Maximum Shift",
                        &mut *max_ms,
                        0.1,
                        20.0,
                        " ms",
                        false,
                        5.0,
                    )
                });
                ui.label(
                    egui::RichText::new(
                        "Finds how far one channel lags the other and lines them up.",
                    )
                    .small()
                    .color(crate::theme::text_dim()),
                );
            }
            Effect::StudioReverb {
                full,
                decay,
                predelay_ms,
                damping,
                width,
                early,
                dry,
                wet,
            } => {
                let _ = full;
                let (dec, pre, damping_v) = (*decay, *predelay_ms, *damping);
                let ticks = vec![
                    (0.0, "0".into()),
                    (0.25, "0.25".into()),
                    (0.5, "0.5".into()),
                    (0.75, "0.75".into()),
                    (1.0, "1 s".into()),
                ];
                let yticks = vec![(0.0, "0".into()), (0.5, "0.5".into()), (1.0, "1".into())];
                crate::fx_ui::graph(
                    ui,
                    128.0,
                    0.0,
                    1.0,
                    0.0,
                    1.0,
                    |x| (-x * (1.0 + dec * 7.0) * (1.0 + damping_v * 0.5)).exp(),
                    &ticks,
                    &yticks,
                    &[(pre / 200.0, "Pre-delay".into(), crate::theme::time_blue())],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Characteristics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b =
                            crate::fx_ui::knob(ui, "Decay", &mut *decay, 0.0, 1.0, "", false, 0.6);
                        b |= crate::fx_ui::knob(
                            ui,
                            "Pre-delay",
                            &mut *predelay_ms,
                            0.0,
                            200.0,
                            " ms",
                            false,
                            20.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Early Reflections",
                            &mut *early,
                            0.0,
                            1.0,
                            "",
                            false,
                            0.4,
                        );
                        b |= crate::fx_ui::knob(ui, "Width", &mut *width, 0.0, 1.0, "", false, 1.0);
                        b |= crate::fx_ui::knob(
                            ui,
                            "High Frequency Cut",
                            &mut *damping,
                            0.0,
                            1.0,
                            "",
                            false,
                            0.5,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    ui.horizontal(|ui| {
                        let mut b =
                            crate::fx_ui::knob(ui, "Dry", &mut *dry, 0.0, 1.0, "", false, 0.8);
                        b |= crate::fx_ui::knob(ui, "Wet", &mut *wet, 0.0, 1.0, "", false, 0.3);
                        b
                    })
                });
            }
            Effect::ConvolutionReverb { kind, mix } => {
                ch |= crate::fx_ui::bank(ui, "Impulse", |ui| {
                    let mut b = egui::ComboBox::from_id_salt("conv_ir")
                        .width(220.0)
                        .selected_text(
                            [
                                "Small Room",
                                "Medium Hall",
                                "Large Hall",
                                "Plate",
                                "Cathedral",
                            ]
                            .get(*kind as usize)
                            .copied()
                            .unwrap_or("?"),
                        )
                        .show_ui(ui, |ui| {
                            let mut ch = false;
                            for (k, n) in [
                                "Small Room",
                                "Medium Hall",
                                "Large Hall",
                                "Plate",
                                "Cathedral",
                            ]
                            .iter()
                            .enumerate()
                            {
                                ch |= ui.selectable_value(kind, k as u8, *n).changed();
                            }
                            ch
                        })
                        .inner
                        .unwrap_or(false);
                    ui.add_space(12.0);
                    b |= crate::fx_ui::knob(ui, "Mix", &mut *mix, 0.0, 1.0, "", false, 0.3);
                    b
                });
                ui.label(
                    egui::RichText::new(
                        "Adds about 85 ms of delay while playing live; processing is exact.",
                    )
                    .small()
                    .color(crate::theme::text_dim()),
                );
            }
            Effect::GuitarSuite {
                comp,
                drive,
                tone_hz,
                cab_hz,
                mix,
            } => {
                let d = *drive;
                let ticks = vec![
                    (-1.0, "-1".into()),
                    (-0.5, "-0.5".into()),
                    (0.0, "0".into()),
                    (0.5, "0.5".into()),
                    (1.0, "1".into()),
                ];
                let norm = (d + 0.01).tanh().max(1e-3);
                crate::fx_ui::graph(
                    ui,
                    140.0,
                    -1.0,
                    1.0,
                    -1.0,
                    1.0,
                    |x| ((d + 0.01) * x).tanh() / norm,
                    &ticks,
                    &ticks,
                    &[],
                    false,
                );
                ch |= crate::fx_ui::bank(ui, "Amplifier", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Compressor",
                            &mut *comp,
                            0.0,
                            1.0,
                            "",
                            false,
                            0.5,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Distortion",
                            &mut *drive,
                            0.0,
                            1.0,
                            "",
                            false,
                            0.6,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Tone",
                            &mut *tone_hz,
                            300.0,
                            8000.0,
                            " Hz",
                            true,
                            3500.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Cabinet",
                            &mut *cab_hz,
                            1500.0,
                            12000.0,
                            " Hz",
                            true,
                            5000.0,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Output Level", |ui| {
                    crate::fx_ui::knob(ui, "Mix", &mut *mix, 0.0, 1.0, "", false, 1.0)
                });
            }
            Effect::Mastering {
                low_db,
                mid_db,
                high_db,
                exciter,
                width,
                maximize_db,
            } => {
                ch |= crate::fx_ui::bank(ui, "Equalizer", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Low Shelf",
                            &mut *low_db,
                            -12.0,
                            12.0,
                            " dB",
                            false,
                            1.5,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Peak (1.5 kHz)",
                            &mut *mid_db,
                            -12.0,
                            12.0,
                            " dB",
                            false,
                            0.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "High Shelf",
                            &mut *high_db,
                            -12.0,
                            12.0,
                            " dB",
                            false,
                            1.5,
                        );
                        b
                    })
                });
                ch |= crate::fx_ui::bank(ui, "Stereo and Dynamics", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            "Exciter",
                            &mut *exciter,
                            0.0,
                            1.0,
                            "",
                            false,
                            0.3,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Widener",
                            &mut *width,
                            0.0,
                            2.0,
                            "x",
                            false,
                            1.1,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Loudness Maximizer",
                            &mut *maximize_db,
                            0.0,
                            12.0,
                            " dB",
                            false,
                            4.0,
                        );
                        b
                    })
                });
            }
            Effect::VocalEnhancer { mode } => {
                ch |= crate::fx_ui::bank(ui, "Voice", |ui| {
                    let mut b = ui.radio_value(mode, 0, "Male").changed();
                    b |= ui.radio_value(mode, 1, "Female").changed();
                    b |= ui.radio_value(mode, 2, "Music").changed();
                    b
                });
            }
            Effect::CenterChannel {
                extract,
                level_db,
                lo_hz,
                hi_hz,
            } => {
                ch |= crate::fx_ui::bank(ui, "Mode", |ui| {
                    let mut b = ui.radio_value(extract, true, "Extract Center").changed();
                    b |= ui.radio_value(extract, false, "Remove Center").changed();
                    b
                });
                ch |= crate::fx_ui::bank(ui, "Frequency Range", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            if *extract {
                                "Center Level"
                            } else {
                                "Center Reduction"
                            },
                            &mut *level_db,
                            -60.0,
                            6.0,
                            " dB",
                            false,
                            -40.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Low Frequency",
                            &mut *lo_hz,
                            20.0,
                            2000.0,
                            " Hz",
                            true,
                            120.0,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "High Frequency",
                            &mut *hi_hz,
                            1000.0,
                            20000.0,
                            " Hz",
                            true,
                            8000.0,
                        );
                        b
                    })
                });
            }
            Effect::PhaseShifter {
                left_deg,
                right_deg,
                freq,
                points,
                log,
                range360,
                channel,
                fft,
            } => {
                if points.len() < 2 {
                    // an older setting: start the curve from its single shift
                    let d = (*right_deg - *left_deg).clamp(-180.0, 180.0);
                    *points = vec![(20.0, d), (20000.0, d)];
                    let _ = freq;
                    ch = true;
                }
                let range = if *range360 { 360.0 } else { 180.0 };
                ch |= crate::equi::phase_curve_editor(ui, points, *log, range, rate * 0.5);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 190.0).max(0.0));
                    let mut spline = false;
                    ui.add_enabled(false, egui::Checkbox::new(&mut spline, "Spline Curves"));
                    if ui
                        .button("\u{27f2}")
                        .on_hover_text("Reset to a flat curve")
                        .clicked()
                    {
                        *points = vec![(20.0, 0.0), (20000.0, 0.0)];
                        ch = true;
                    }
                });
                egui::Grid::new("phase_opts")
                    .num_columns(2)
                    .spacing([10.0, 8.0])
                    .show(ui, |ui| {
                        let lab = |ui: &mut egui::Ui, t: &str| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(egui::RichText::new(t).color(crate::theme::text_dim()))
                                },
                            );
                        };
                        lab(ui, "Frequency Scale (Horizontal Axis):");
                        ui.horizontal(|ui| {
                            ch |= ui.radio_value(log, false, "Linear").changed();
                            ch |= ui.radio_value(log, true, "Logarithmic").changed();
                        });
                        ui.end_row();
                        lab(ui, "Range (Vertical Axis):");
                        ui.horizontal(|ui| {
                            ch |= ui.radio_value(range360, false, "180\u{b0}").changed();
                            ch |= ui.radio_value(range360, true, "360\u{b0}").changed();
                        });
                        ui.end_row();
                        lab(ui, "Channel:");
                        egui::ComboBox::from_id_salt("phase_ch")
                            .width(110.0)
                            .selected_text(["Both", "Left", "Right"][(*channel as usize).min(2)])
                            .show_ui(ui, |ui| {
                                for (i, n) in ["Both", "Left", "Right"].iter().enumerate() {
                                    ch |= ui.selectable_value(channel, i as u8, *n).changed();
                                }
                            });
                        ui.end_row();
                        lab(ui, "FFT Size:");
                        egui::ComboBox::from_id_salt("phase_fft")
                            .width(110.0)
                            .selected_text(format!("{fft}"))
                            .show_ui(ui, |ui| {
                                for n in [1024u32, 2048, 4096, 8192] {
                                    ch |= ui.selectable_value(fft, n, format!("{n}")).changed();
                                }
                            });
                        ui.end_row();
                    });
                if !*range360 {
                    for p in points.iter_mut() {
                        p.1 = p.1.clamp(-180.0, 180.0);
                    }
                }
            }
            Effect::SpectralRepair {
                mode,
                amount,
                reduce_db,
                cutoff_hz,
            } => {
                ch |= crate::fx_ui::bank(ui, "Noise Reduction", |ui| {
                    ui.horizontal(|ui| {
                        let mut b = crate::fx_ui::knob(
                            ui,
                            if *mode == 3 {
                                "Amount"
                            } else {
                                "Noise Reduction"
                            },
                            &mut *amount,
                            0.0,
                            2.0,
                            "",
                            false,
                            0.8,
                        );
                        b |= crate::fx_ui::knob(
                            ui,
                            "Reduce By (max)",
                            &mut *reduce_db,
                            1.0,
                            60.0,
                            " dB",
                            false,
                            18.0,
                        );
                        if *mode == 2 {
                            b |= crate::fx_ui::knob(
                                ui,
                                "Above",
                                &mut *cutoff_hz,
                                500.0,
                                16000.0,
                                " Hz",
                                true,
                                4000.0,
                            );
                        }
                        b
                    })
                });
                ui.label(egui::RichText::new("Learns the noise as it plays (no noise print needed). Adds about 40 ms of delay while playing live.").small().color(crate::theme::text_dim()));
            }
            Effect::ClickRemover { sensitivity } => {
                ch |= crate::fx_ui::bank(ui, "Detection", |ui| {
                    crate::fx_ui::knob(
                        ui,
                        "Sensitivity",
                        &mut *sensitivity,
                        0.05,
                        1.0,
                        "",
                        false,
                        0.5,
                    )
                });
            }
        }
        ch
    }

    pub fn make(&self, rate: f32) -> Box<dyn Processor> {
        match self.clone() {
            Effect::Amplify { gain_db } => Box::new(Gain { g: undb(gain_db) }),
            Effect::ParametricEq { bands, master_db } => {
                let filters: Vec<Biquad> = bands
                    .iter()
                    .filter(|b| b.on)
                    .map(|b| Biquad::new(b.kind, rate, b.freq, b.q, b.gain_db))
                    .collect();
                Box::new(FilterChain {
                    filters,
                    state: vec![],
                    gain: undb(master_db),
                })
            }
            Effect::Filter {
                kind,
                freq,
                q,
                gain_db,
            } => Box::new(FilterChain {
                filters: vec![Biquad::new(kind, rate, freq, q, gain_db)],
                state: vec![],
                gain: 1.0,
            }),
            Effect::DeHum { freq, harmonics, q } => {
                let filters = (1..=harmonics)
                    .map(|h| h as f32 * freq)
                    .filter(|f| *f < rate * 0.45)
                    .map(|f| Biquad::new(FilterKind::Notch, rate, f, q, 0.0))
                    .collect();
                Box::new(FilterChain {
                    filters,
                    state: vec![],
                    gain: 1.0,
                })
            }
            Effect::Compressor {
                threshold,
                ratio,
                attack_ms,
                release_ms,
                makeup,
            } => Box::new(Dynamics {
                mode: DynMode::Compress { threshold, ratio },
                att: coef(attack_ms, rate),
                rel: coef(release_ms, rate),
                env: 0.0,
                gain: 1.0,
                makeup: undb(makeup),
            }),
            Effect::NoiseGate {
                threshold,
                attack_ms,
                release_ms,
                floor_db,
            } => Box::new(Dynamics {
                mode: DynMode::Gate {
                    threshold,
                    floor: undb(floor_db),
                },
                att: coef(attack_ms, rate),
                rel: coef(release_ms, rate),
                env: 0.0,
                gain: 1.0,
                makeup: 1.0,
            }),
            Effect::HardLimiter {
                ceiling_db,
                input_boost,
                release_ms,
            } => Box::new(Limiter {
                ceiling: undb(ceiling_db),
                boost: undb(input_boost),
                rel: coef(release_ms, rate),
                gain: 1.0,
            }),
            Effect::Delay {
                time_ms,
                feedback,
                mix,
            } => Box::new(DelayFx {
                len: ((time_ms / 1000.0) * rate).max(1.0) as usize,
                fb: feedback,
                mix,
                lines: vec![],
                pos: 0,
            }),
            Effect::Reverb {
                room,
                damping,
                width,
                mix,
            } => Box::new(Freeverb::new(rate, room, damping, width, mix)),
            Effect::Chorus {
                rate: r,
                depth_ms,
                mix,
            } => Box::new(ModDelay::new(rate, r, 15.0, depth_ms, 0.0, mix)),
            Effect::Flanger {
                rate: r,
                depth_ms,
                feedback,
                mix,
            } => Box::new(ModDelay::new(rate, r, 0.5, depth_ms, feedback, mix)),
            Effect::Distortion { drive_db, mix } => Box::new(Distort {
                drive: undb(drive_db),
                mix,
            }),
            Effect::StereoExpander { width } => Box::new(Width { w: width }),
            Effect::Plugin {
                id, params, state, ..
            } => Box::new(crate::plugins::PluginProc::new(
                &id,
                &params,
                state.as_deref(),
                rate,
            )),
            Effect::Imported { .. } => Box::new(Gain { g: 1.0 }),
            Effect::Phaser {
                stages,
                rate: r,
                depth,
                feedback,
                mix,
            } => Box::new(crate::fx2::Phaser::new(
                rate, stages, r, depth, feedback, mix,
            )),
            Effect::ChorusFlanger {
                flanger,
                rate: r,
                depth_ms,
                feedback,
                mix,
            } => {
                if flanger {
                    Box::new(ModDelay::new(
                        rate,
                        r,
                        0.5,
                        depth_ms.min(10.0),
                        feedback,
                        mix,
                    ))
                } else {
                    Box::new(ModDelay::new(rate, r, 15.0, depth_ms, feedback * 0.5, mix))
                }
            }
            Effect::AnalogDelay {
                time_ms,
                feedback,
                drive,
                spread_ms,
                tone_hz,
                mix,
            } => Box::new(crate::fx2::AnalogDelay::new(
                rate, time_ms, feedback, drive, spread_ms, mix, tone_hz,
            )),
            Effect::Echo {
                left_ms,
                right_ms,
                feedback,
                tone_hz,
                mix,
            } => Box::new(crate::fx2::AnalogDelay::new(
                rate,
                left_ms,
                feedback,
                0.0,
                right_ms - left_ms,
                mix,
                tone_hz,
            )),
            Effect::DeEsser {
                freq,
                threshold,
                reduction,
            } => Box::new(crate::fx2::DeEsser::new(rate, freq, threshold, reduction)),
            Effect::Multiband {
                x1,
                x2,
                thr_low,
                thr_mid,
                thr_high,
                ratio,
                makeup,
            } => Box::new(crate::fx2::Multiband::new(
                rate,
                x1,
                x2,
                [thr_low, thr_mid, thr_high],
                [ratio; 3],
                makeup,
            )),
            Effect::DynamicsProcessing {
                gate_db,
                comp_db,
                ratio,
                ceiling_db,
                makeup,
            } => Box::new(crate::fx2::DynamicsProc::new(
                rate, gate_db, comp_db, ratio, ceiling_db, makeup,
            )),
            Effect::TubeCompressor {
                threshold,
                ratio,
                attack_ms,
                release_ms,
                drive,
                makeup,
            } => Box::new(crate::fx2::TubeComp::new(
                rate, threshold, ratio, attack_ms, release_ms, drive, makeup,
            )),
            Effect::SpeechLeveler {
                target_db,
                max_gain_db,
                gate_db,
                speed,
            } => Box::new(crate::fx2::Leveler::new(
                rate,
                target_db,
                max_gain_db,
                gate_db,
                speed,
            )),
            Effect::ChannelMixer { ll, lr, rl, rr } => {
                Box::new(crate::fx2::ChannelMixer::new(ll, lr, rl, rr))
            }
            Effect::GraphicEq { gains, master_db } => {
                Box::new(crate::fx2::graphic_bank(rate, &gains, master_db))
            }
            Effect::NotchFilter { freqs, depth_db, q } => {
                let f = freqs
                    .iter()
                    .filter(|f| **f > 10.0 && **f < rate * 0.45)
                    .map(|f| Biquad::new(FilterKind::Peak, rate, *f, q, depth_db.min(0.0)))
                    .collect();
                Box::new(crate::fx2::Bank::new(f, 0.0))
            }
            Effect::ScientificFilter {
                kind,
                order,
                f1,
                f2,
                gain_db,
            } => Box::new(crate::fx2::scientific_bank(
                rate, kind, order, f1, f2, gain_db,
            )),
            Effect::FftFilter { points } => Box::new(crate::fx2::Fir::design(rate, &points, 1023)),
            Effect::StudioReverb {
                decay,
                predelay_ms,
                damping,
                width,
                early,
                dry,
                wet,
                ..
            } => Box::new(crate::fx2::StudioReverb::new(
                rate,
                decay,
                predelay_ms,
                damping,
                width,
                early,
                dry,
                wet,
            )),
            Effect::ConvolutionReverb { kind, mix } => Box::new(crate::fx2::Convolver::new(
                crate::fx2::synthetic_ir(rate, kind),
                mix,
            )),
            Effect::GuitarSuite {
                comp,
                drive,
                tone_hz,
                cab_hz,
                mix,
            } => {
                let mut chain: Vec<Box<dyn Processor>> = vec![];
                if comp > 0.0 {
                    chain.push(Box::new(crate::fx2::TubeComp::new(
                        rate,
                        -10.0 - comp * 25.0,
                        1.5 + comp * 6.0,
                        5.0,
                        100.0,
                        0.0,
                        comp * 8.0,
                    )));
                }
                chain.push(Box::new(crate::fx2::Drive {
                    k: 1.0 + drive * 30.0,
                }));
                chain.push(Box::new(crate::fx2::Bank::new(
                    vec![
                        Biquad::new(
                            FilterKind::Peak,
                            rate,
                            tone_hz.clamp(200.0, rate * 0.4),
                            0.8,
                            4.0,
                        ),
                        Biquad::new(FilterKind::HighPass, rate, 90.0, 0.707, 0.0),
                        Biquad::new(
                            FilterKind::LowPass,
                            rate,
                            cab_hz.clamp(1000.0, rate * 0.45),
                            0.9,
                            0.0,
                        ),
                    ],
                    -drive * 9.0,
                )));
                let _ = mix;
                Box::new(crate::fx2::Series(chain))
            }
            Effect::Mastering {
                low_db,
                mid_db,
                high_db,
                exciter,
                width,
                maximize_db,
            } => Box::new(crate::fx2::Series(vec![
                Box::new(crate::fx2::Bank::new(
                    vec![
                        Biquad::new(FilterKind::LowShelf, rate, 120.0, 0.707, low_db),
                        Biquad::new(FilterKind::Peak, rate, 1500.0, 0.8, mid_db),
                        Biquad::new(
                            FilterKind::HighShelf,
                            rate,
                            8000.0_f32.min(rate * 0.4),
                            0.707,
                            high_db,
                        ),
                    ],
                    0.0,
                )),
                Box::new(crate::fx2::Exciter::new(rate, exciter)),
                Box::new(crate::fx2::Widen { w: width }),
                Box::new(crate::fx2::Maximizer {
                    gain: undb(maximize_db),
                    ceiling: undb(-0.3),
                }),
            ])),
            Effect::VocalEnhancer { mode } => {
                // 0 male, 1 female, 2 music: clarity EQ plus de-essing
                let (lo, pres, air) = match mode {
                    1 => (220.0, 3500.0, 3.0),
                    2 => (150.0, 2500.0, 1.5),
                    _ => (160.0, 2800.0, 2.0),
                };
                Box::new(crate::fx2::Series(vec![
                    Box::new(crate::fx2::Bank::new(
                        vec![
                            Biquad::new(FilterKind::HighPass, rate, 70.0, 0.707, 0.0),
                            Biquad::new(FilterKind::Peak, rate, lo, 1.0, -2.0),
                            Biquad::new(FilterKind::Peak, rate, pres, 1.0, air),
                        ],
                        0.0,
                    )),
                    Box::new(crate::fx2::DeEsser::new(rate, 6500.0, -28.0, 8.0)),
                ]))
            }
            Effect::CenterChannel {
                extract,
                level_db,
                lo_hz,
                hi_hz,
            } => Box::new(crate::fx2::CenterChannel::new(
                rate, extract, level_db, lo_hz, hi_hz,
            )),
            Effect::PhaseShifter {
                left_deg,
                right_deg,
                freq,
                points,
                channel,
                fft,
                ..
            } => {
                if points.len() >= 2 {
                    let mut pc = crate::fx3::PhaseCurve::new(points, channel, fft);
                    pc.set_rate(rate);
                    Box::new(pc)
                } else {
                    Box::new(crate::fx2::PhaseShift::new(rate, left_deg, right_deg, freq))
                }
            }
            Effect::SpectralRepair {
                mode,
                amount,
                reduce_db,
                cutoff_hz,
            } => {
                let m = match mode {
                    2 => crate::fx2::SpecMode::Hiss,
                    3 => crate::fx2::SpecMode::DeReverb,
                    _ => crate::fx2::SpecMode::Adaptive,
                };
                Box::new(crate::fx2::Spectral::new(
                    rate, m, amount, reduce_db, cutoff_hz,
                ))
            }
            Effect::ClickRemover { sensitivity } => {
                Box::new(crate::fx2::ClickRemover::new(sensitivity))
            }
            Effect::Dynamics {
                gate_db,
                comp_db,
                ratio,
                limit_db,
                makeup,
            } => Box::new(crate::fx2::DynamicsProc::new(
                rate, gate_db, comp_db, ratio, limit_db, makeup,
            )),
            Effect::LoudnessMeter { .. } => Box::new(crate::fx3::LoudnessMeter::new(rate)),
            Effect::SurroundReverb {
                decay,
                predelay_ms,
                damping,
                center,
                dry,
                wet,
            } => Box::new(crate::fx2::StudioReverb::new(
                rate,
                decay,
                predelay_ms,
                damping,
                1.0 - center * 0.5,
                0.5,
                dry,
                wet,
            )),
            Effect::PhaseCorrection { max_ms } => {
                Box::new(crate::fx3::PhaseCorrection::new(rate, max_ms))
            }
            Effect::AutoPitch {
                scale,
                key,
                attack,
                sensitivity,
                ref_channel,
                fft,
                a4,
            } => Box::new(crate::fx3::AutoPitch::new(
                rate,
                scale,
                key,
                attack,
                sensitivity,
                ref_channel,
                fft,
                a4,
            )),
        }
    }
}

fn yes_log() -> bool {
    true
}

fn fft_4096() -> u32 {
    4096
}

fn coef(ms: f32, rate: f32) -> f32 {
    (-1.0 / ((ms.max(0.01) / 1000.0) * rate)).exp()
}

/// An effect parameter row in the classic effect-window style: the label right-aligned, a long
/// slider with its scale written above it, and the value in blue (type or drag it) with its
/// unit to the right.
pub fn slider(
    ui: &mut egui::Ui,
    label: &str,
    v: &mut f32,
    r: std::ops::RangeInclusive<f32>,
    suffix: &str,
    log: bool,
) -> bool {
    let (lo, hi) = (*r.start(), *r.end());
    let labels = scale_labels(lo, hi, log);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(150.0, 30.0),
            egui::Layout::right_to_left(egui::Align::Max),
            |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("{label}:")).color(crate::theme::text_dim()),
                    )
                    .truncate(),
                );
            },
        );
        let mut ch = track_slider(ui, v, lo, hi, log, &labels);
        ch |= value_box(ui, v, lo, hi, suffix, log);
        ch
    })
    .inner
}

/// A slider with labels of your own on its scale (Slow .. Fast and so on), for grid rows.
pub fn scale_slider(
    ui: &mut egui::Ui,
    v: &mut f32,
    r: std::ops::RangeInclusive<f32>,
    labels: &[(f32, &str)],
    suffix: &str,
    _decimals: usize,
) -> bool {
    let (lo, hi) = (*r.start(), *r.end());
    let labels: Vec<(f32, String)> = labels.iter().map(|(p, t)| (*p, t.to_string())).collect();
    ui.horizontal(|ui| {
        let mut ch = track_slider(ui, v, lo, hi, false, &labels);
        ch |= value_box(ui, v, lo, hi, suffix, false);
        ch
    })
    .inner
}

/// A labelled row (label right-aligned) with a slider carrying your own scale labels.
pub fn scale_slider_labeled(
    ui: &mut egui::Ui,
    label: &str,
    v: &mut f32,
    r: std::ops::RangeInclusive<f32>,
    labels: &[(f32, &str)],
    suffix: &str,
) -> bool {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(150.0, 30.0),
            egui::Layout::right_to_left(egui::Align::Max),
            |ui| {
                ui.label(egui::RichText::new(format!("{label}:")).color(crate::theme::text_dim()));
            },
        );
        scale_slider(ui, v, r, labels, suffix, 0)
    })
    .inner
}

/// A titled box around a set of controls ("Characteristics", "Output Level"...). Kept for the
/// effects that still build their controls from sliders; it uses the same frame as the knob banks.
pub fn group(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(4.0);
    egui::Frame::new()
        .fill(crate::theme::gray(29))
        .stroke(egui::Stroke::new(1.0, crate::theme::gray(54)))
        .corner_radius(crate::theme::ROUND)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .color(crate::theme::gray(196))
                    .strong(),
            );
            ui.add_space(2.0);
            add(ui);
        });
}

fn nice_step(span: f32) -> f32 {
    let raw = (span / 5.0).max(1e-6);
    let mag = 10f32.powf(raw.log10().floor());
    let n = raw / mag;
    let k = if n < 1.5 {
        1.0
    } else if n < 3.0 {
        2.0
    } else if n < 7.0 {
        5.0
    } else {
        10.0
    };
    k * mag
}

fn fmt_scale(v: f32) -> String {
    if v.abs() >= 1000.0 && (v / 1000.0).fract().abs() < 1e-3 {
        format!("{}k", v / 1000.0)
    } else if v.fract().abs() < 1e-4 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// Round values to write over a slider's scale.
fn scale_labels(lo: f32, hi: f32, log: bool) -> Vec<(f32, String)> {
    if hi <= lo {
        return vec![];
    }
    if log && lo > 0.0 {
        let mut out = vec![];
        let mut dec = 10f32.powf(lo.log10().floor());
        while dec <= hi * 1.0001 {
            for m in [1.0f32, 2.0, 5.0] {
                let v = dec * m;
                if v >= lo * 0.9999 && v <= hi * 1.0001 {
                    out.push((v, fmt_scale(v)));
                }
            }
            dec *= 10.0;
        }
        return out;
    }
    let step = nice_step(hi - lo);
    let mut v = (lo / step).ceil() * step;
    let mut out = vec![];
    while v <= hi + step * 1e-3 {
        out.push((v, fmt_scale(if v.abs() < step * 1e-3 { 0.0 } else { v })));
        v += step;
    }
    out
}

/// The slider track: thin line, scale ticks and numbers above, round handle. Drag or click.
fn track_slider(
    ui: &mut egui::Ui,
    v: &mut f32,
    lo: f32,
    hi: f32,
    log: bool,
    labels: &[(f32, String)],
) -> bool {
    let w = (ui.available_width() - 110.0).clamp(160.0, 320.0);
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 30.0), egui::Sense::click_and_drag());
    let line_y = r.bottom() - 8.0;
    let (x0, x1) = (r.left() + 8.0, r.right() - 8.0);
    let to_t = |x: f32| -> f32 {
        if log && lo > 0.0 {
            ((x.max(lo)).ln() - lo.ln()) / (hi.ln() - lo.ln()).max(1e-6)
        } else {
            (x - lo) / (hi - lo).max(1e-9)
        }
    };
    let from_t = |t: f32| -> f32 {
        let t = t.clamp(0.0, 1.0);
        if log && lo > 0.0 {
            (lo.ln() + t * (hi.ln() - lo.ln())).exp()
        } else {
            lo + t * (hi - lo)
        }
    };
    let mut changed = false;
    if resp.dragged() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let nv = from_t((p.x - x0) / (x1 - x0).max(1.0));
            if (nv - *v).abs() > f32::EPSILON {
                *v = nv;
                changed = true;
            }
        }
    }
    if resp.double_clicked() {
        // back to the middle of the range is rarely wanted; leave the value, the box types it
    }
    let p = ui.painter();
    let dim = crate::theme::text_dim();
    p.line_segment(
        [egui::pos2(x0, line_y), egui::pos2(x1, line_y)],
        egui::Stroke::new(2.0, crate::theme::gray(95)),
    );
    // minor ticks between labels, labels above the major ticks
    let n_minor = 20;
    for i in 0..=n_minor {
        let x = x0 + (x1 - x0) * i as f32 / n_minor as f32;
        p.line_segment(
            [egui::pos2(x, line_y - 4.0), egui::pos2(x, line_y - 1.0)],
            egui::Stroke::new(1.0, crate::theme::gray(80)),
        );
    }
    let mut last_x = f32::MIN;
    for (val, text) in labels {
        let x = x0 + to_t(*val).clamp(0.0, 1.0) * (x1 - x0);
        p.line_segment(
            [egui::pos2(x, line_y - 7.0), egui::pos2(x, line_y - 1.0)],
            egui::Stroke::new(1.0, dim),
        );
        if x - last_x > 22.0 {
            p.text(
                egui::pos2(x, line_y - 9.0),
                egui::Align2::CENTER_BOTTOM,
                text,
                egui::FontId::proportional(10.0),
                dim,
            );
            last_x = x;
        }
    }
    let hx = x0 + to_t(*v).clamp(0.0, 1.0) * (x1 - x0);
    let hot = resp.hovered() || resp.dragged();
    p.circle_filled(
        egui::pos2(hx, line_y),
        7.0,
        crate::theme::gray(if hot { 70 } else { 45 }),
    );
    p.circle_stroke(
        egui::pos2(hx, line_y),
        7.0,
        egui::Stroke::new(2.0, crate::theme::gray(if hot { 235 } else { 200 })),
    );
    changed
}

/// The blue value at the end of a slider row, with its unit; drag or type to change it.
fn value_box(ui: &mut egui::Ui, v: &mut f32, lo: f32, hi: f32, suffix: &str, log: bool) -> bool {
    let span = (hi - lo).abs();
    let decimals = if span <= 2.0 {
        2
    } else if span <= 20.0 {
        1
    } else {
        0
    };
    let speed = if log {
        (*v).abs().max(1.0) * 0.005
    } else {
        span / 300.0
    };
    let r = ui
        .scope(|ui| {
            ui.visuals_mut().override_text_color = Some(crate::theme::time_blue());
            let wv = &mut ui.visuals_mut().widgets;
            wv.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
            wv.inactive.bg_stroke = egui::Stroke::NONE;
            ui.add(
                egui::DragValue::new(v)
                    .range(lo..=hi)
                    .speed(speed)
                    .max_decimals(decimals)
                    .min_decimals(decimals),
            )
        })
        .inner;
    let unit = suffix.trim();
    if !unit.is_empty() {
        ui.label(egui::RichText::new(unit).color(crate::theme::text()));
    }
    r.changed()
}

// ---------------------------------------------------------------- slots

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectSlot {
    pub effect: Effect,
    pub enabled: bool,
    pub mix: f32,
}

impl EffectSlot {
    pub fn new(effect: Effect) -> Self {
        Self {
            effect,
            enabled: true,
            mix: 1.0,
        }
    }
}

/// A chain of processors built from slots.
pub struct Chain {
    procs: Vec<(Box<dyn Processor>, f32)>,
    dry: Vec<Vec<f32>>,
    slots: Vec<EffectSlot>,
    rate: f32,
}

impl Default for Chain {
    fn default() -> Self {
        Chain {
            procs: vec![],
            dry: vec![],
            slots: vec![],
            rate: 48000.0,
        }
    }
}

impl Chain {
    /// Build a chain. Call on the UI thread: hosted plug-ins must be created there.
    pub fn new(slots: &[EffectSlot], rate: f32) -> Self {
        let en: Vec<EffectSlot> = slots.iter().filter(|s| s.enabled).cloned().collect();
        Chain {
            procs: en
                .iter()
                .map(|s| (s.effect.make(rate), s.mix.clamp(0.0, 1.0)))
                .collect(),
            dry: vec![],
            slots: en,
            rate,
        }
    }
    /// Bring the chain in line with slots, keeping unchanged processors (and their state).
    pub fn update(&mut self, slots: &[EffectSlot], rate: f32) {
        let en: Vec<&EffectSlot> = slots.iter().filter(|s| s.enabled).collect();
        if (rate - self.rate).abs() > 0.5 {
            *self = Chain::new(slots, rate);
            return;
        }
        if en.len() != self.procs.len() {
            // an effect was added, removed or bypassed: keep the running processors of the
            // others (a plug-in keeps its instance, sound and settings), build only new ones
            let mut old: Vec<Option<(Box<dyn Processor>, f32)>> = std::mem::take(&mut self.procs)
                .into_iter()
                .map(Some)
                .collect();
            let old_slots = std::mem::take(&mut self.slots);
            let same = |a: &EffectSlot, b: &EffectSlot| match (&a.effect, &b.effect) {
                (Effect::Plugin { id: x, .. }, Effect::Plugin { id: y, .. }) => x == y,
                (x, y) => std::mem::discriminant(x) == std::mem::discriminant(y),
            };
            for s in &en {
                let pick = (0..old.len())
                    .find(|&j| old[j].is_some() && old_slots[j] == **s)
                    .or_else(|| {
                        (0..old.len()).find(|&j| old[j].is_some() && same(&old_slots[j], s))
                    });
                let proc = match pick.and_then(|j| old[j].take().map(|p| (j, p))) {
                    Some((j, (mut p, _))) => {
                        if old_slots[j] != **s && !p.update(&s.effect) {
                            let fresh = s.effect.make(rate);
                            if !p.retune(fresh.as_ref()) {
                                p = fresh;
                            }
                        }
                        p
                    }
                    None => s.effect.make(rate),
                };
                self.procs.push((proc, s.mix.clamp(0.0, 1.0)));
                self.slots.push((*s).clone());
            }
            return;
        }
        for (i, s) in en.iter().enumerate() {
            if self.slots[i] == **s {
                continue;
            }
            let same_kind =
                std::mem::discriminant(&self.slots[i].effect) == std::mem::discriminant(&s.effect);
            if !(same_kind && self.procs[i].0.update(&s.effect)) {
                let fresh = s.effect.make(rate);
                if !(same_kind && self.procs[i].0.retune(fresh.as_ref())) {
                    self.procs[i].0 = fresh;
                }
            }
            self.procs[i].1 = s.mix.clamp(0.0, 1.0);
            self.slots[i] = (*s).clone();
        }
    }
    /// Forget what the effects still hold (reverb and delay tails, filter memory), so audio
    /// from before a stop or a jump of the playhead can never be heard again. Built-in effects
    /// start over fresh; plug-ins (whose inner state is their own) are run on silence until
    /// their tail has died away, and that output is thrown away. UI thread.
    pub fn flush(&mut self) {
        let rate = self.rate;
        let mut quiet: Vec<usize> = vec![];
        for (i, s) in self.slots.iter().enumerate() {
            if matches!(s.effect, Effect::Plugin { .. }) {
                quiet.push(i);
            } else if let Some(p) = self.procs.get_mut(i) {
                p.0 = s.effect.make(rate);
            }
        }
        for i in quiet {
            let Some((p, _)) = self.procs.get_mut(i) else {
                continue;
            };
            let total = p.tail().clamp(rate as usize / 2, rate as usize * 4);
            let mut buf = vec![vec![0.0f32; 1024]; 2];
            let mut done = 0;
            while done < total {
                for c in buf.iter_mut() {
                    c.iter_mut().for_each(|v| *v = 0.0);
                }
                p.process(&mut buf);
                done += 1024;
            }
        }
    }
    pub fn is_empty(&self) -> bool {
        self.procs.is_empty()
    }
    /// The processor running the `nth` enabled slot, when that slot is `slot`.
    /// Take `slot` as the settings of running effect `nth` without sending anything to its
    /// processor: for edits the processor already has (made in its own plug-in window), so the
    /// next `update` finds nothing to change. Only for the same plug-in.
    pub fn adopt_slot(&mut self, nth: usize, slot: &EffectSlot) -> bool {
        let same = match (self.slots.get(nth).map(|s| &s.effect), &slot.effect) {
            (Some(Effect::Plugin { id: a, .. }), Effect::Plugin { id: b, .. }) => a == b,
            _ => false,
        };
        if same {
            self.slots[nth] = slot.clone();
        }
        same
    }

    pub fn proc_for(&mut self, nth: usize, slot: &EffectSlot) -> Option<&mut Box<dyn Processor>> {
        let same = match (self.slots.get(nth).map(|s| &s.effect), &slot.effect) {
            (Some(Effect::Plugin { id: a, .. }), Effect::Plugin { id: b, .. }) => a == b,
            _ => false,
        };
        if same {
            self.procs.get_mut(nth).map(|p| &mut p.0)
        } else {
            None
        }
    }
    pub fn process(&mut self, buf: &mut [Vec<f32>]) {
        if self.procs.is_empty() {
            return;
        }
        // broken samples (NaN, infinity: a damaged float file, a misbehaving plug-in) would
        // stick in filter and reverb memory and silence everything after them; they go in
        // and come out as silence instead
        let clean = |buf: &mut [Vec<f32>]| {
            for c in buf.iter_mut() {
                for s in c.iter_mut() {
                    if !s.is_finite() {
                        *s = 0.0;
                    }
                }
            }
        };
        clean(buf);
        self.process_inner(buf);
        clean(buf);
    }

    fn process_inner(&mut self, buf: &mut [Vec<f32>]) {
        for (p, mix) in self.procs.iter_mut() {
            if *mix >= 0.999 {
                p.process(buf);
            } else {
                self.dry.resize(buf.len(), vec![]);
                for (d, b) in self.dry.iter_mut().zip(buf.iter()) {
                    d.clear();
                    d.extend_from_slice(b);
                }
                p.process(buf);
                let m = *mix;
                for (d, b) in self.dry.iter().zip(buf.iter_mut()) {
                    for (x, y) in d.iter().zip(b.iter_mut()) {
                        *y = *x * (1.0 - m) + *y * m;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- processors

/// Lets a processor be recognised by its concrete type (for [`Processor::retune`]).
pub trait AsAny {
    fn as_any(&self) -> &dyn std::any::Any;
}
impl<T: std::any::Any> AsAny for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub trait Processor: Send + AsAny {
    /// Process planar audio in place. Every channel has the same length.
    fn process(&mut self, buf: &mut [Vec<f32>]);
    /// Take new settings without rebuilding (keeps tails and plug-in state). False = rebuild.
    fn update(&mut self, _e: &Effect) -> bool {
        false
    }
    /// Take the settings of `fresh` (the same effect just built with new settings) and keep
    /// this one's sound: reverb and delay tails, filter memory, envelope followers. This is
    /// what makes automated effect settings glide: rebuilding on every change emptied a
    /// reverb, so it fell silent for as long as its settings moved. False = rebuild.
    fn retune(&mut self, _fresh: &dyn Processor) -> bool {
        false
    }
    /// A hosted plug-in's own editor and state reader, driving this very (playing) instance.
    /// UI thread. `None` for built-in effects.
    fn live_editor(
        &mut self,
    ) -> Option<(
        Box<dyn soundcraft_dsp::PluginEditor>,
        Option<Box<dyn soundcraft_dsp::PluginStateReader>>,
    )> {
        None
    }
    /// The instance already holds this saved state (read from it), so a slot that now stores it
    /// must not load it again.
    fn adopt_state(&mut self, _s: &[u8]) {}
    /// How many samples the effect keeps ringing after the input stops.
    fn tail(&self) -> usize {
        0
    }
}

struct Gain {
    g: f32,
}
impl Processor for Gain {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Gain>() else {
            return false;
        };
        self.g = f.g;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        for c in buf.iter_mut() {
            for s in c.iter_mut() {
                *s *= self.g;
            }
        }
    }
}

struct FilterChain {
    filters: Vec<Biquad>,
    state: Vec<Vec<BqState>>,
    gain: f32,
}
impl Processor for FilterChain {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<FilterChain>() else {
            return false;
        };
        if f.filters.len() != self.filters.len() {
            return false;
        }
        self.filters.clone_from(&f.filters);
        self.gain = f.gain;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        if self.state.len() < buf.len() {
            self.state
                .resize(buf.len(), vec![BqState::default(); self.filters.len()]);
        }
        for (ci, c) in buf.iter_mut().enumerate() {
            let st = &mut self.state[ci];
            for s in c.iter_mut() {
                let mut x = *s;
                for (f, z) in self.filters.iter().zip(st.iter_mut()) {
                    x = z.run(f, x);
                }
                *s = x * self.gain;
            }
        }
    }
}

#[derive(Clone, Copy)]
enum DynMode {
    Compress { threshold: f32, ratio: f32 },
    Gate { threshold: f32, floor: f32 },
}

struct Dynamics {
    mode: DynMode,
    att: f32,
    rel: f32,
    env: f32,
    gain: f32,
    makeup: f32,
}
impl Processor for Dynamics {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Dynamics>() else {
            return false;
        };
        self.mode = f.mode;
        self.att = f.att;
        self.rel = f.rel;
        self.makeup = f.makeup;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = buf.first().map(|c| c.len()).unwrap_or(0);
        for i in 0..n {
            let lvl = buf.iter().map(|c| c[i].abs()).fold(0.0f32, f32::max);
            // peak envelope follower
            let k = if lvl > self.env { self.att } else { self.rel };
            self.env = lvl + k * (self.env - lvl);
            let env_db = db(self.env);
            let target = match self.mode {
                DynMode::Compress { threshold, ratio } => {
                    if env_db > threshold {
                        undb((threshold + (env_db - threshold) / ratio) - env_db)
                    } else {
                        1.0
                    }
                }
                DynMode::Gate { threshold, floor } => {
                    if env_db > threshold {
                        1.0
                    } else {
                        floor
                    }
                }
            };
            let k2 = if target < self.gain {
                self.att
            } else {
                self.rel
            };
            self.gain = target + k2 * (self.gain - target);
            let g = self.gain * self.makeup;
            for c in buf.iter_mut() {
                c[i] *= g;
            }
        }
    }
}

struct Limiter {
    ceiling: f32,
    boost: f32,
    rel: f32,
    gain: f32,
}
impl Processor for Limiter {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Limiter>() else {
            return false;
        };
        self.ceiling = f.ceiling;
        self.boost = f.boost;
        self.rel = f.rel;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = buf.first().map(|c| c.len()).unwrap_or(0);
        for i in 0..n {
            let lvl = buf
                .iter()
                .map(|c| (c[i] * self.boost).abs())
                .fold(0.0f32, f32::max);
            let need = if lvl * self.gain > self.ceiling {
                self.ceiling / lvl.max(1e-9)
            } else {
                1.0
            };
            if need < self.gain {
                self.gain = need;
            } else {
                self.gain = need + self.rel * (self.gain - need);
            }
            for c in buf.iter_mut() {
                c[i] = (c[i] * self.boost * self.gain).clamp(-self.ceiling, self.ceiling);
            }
        }
    }
}

struct DelayFx {
    len: usize,
    fb: f32,
    mix: f32,
    lines: Vec<Vec<f32>>,
    pos: usize,
}
impl Processor for DelayFx {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<DelayFx>() else {
            return false;
        };
        self.fb = f.fb;
        self.mix = f.mix;
        if f.len != self.len {
            // a new delay time: the line grows or shrinks, what it holds keeps playing
            self.len = f.len.max(1);
            for l in self.lines.iter_mut() {
                l.resize(self.len, 0.0);
            }
            self.pos %= self.len;
        }
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        if self.lines.len() < buf.len() {
            self.lines.resize(buf.len(), vec![0.0; self.len]);
        }
        let n = buf.first().map(|c| c.len()).unwrap_or(0);
        let mut pos = self.pos;
        for i in 0..n {
            for (c, line) in buf.iter_mut().zip(self.lines.iter_mut()) {
                let d = line[pos];
                let x = c[i];
                line[pos] = x + d * self.fb;
                c[i] = x + d * self.mix;
            }
            pos = (pos + 1) % self.len;
        }
        self.pos = pos;
    }
}

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}
impl Comb {
    fn run(&mut self, x: f32, fb: f32, damp: f32) -> f32 {
        let y = self.buf[self.idx];
        self.store = y * (1.0 - damp) + self.store * damp;
        self.buf[self.idx] = x + self.store * fb;
        self.idx = (self.idx + 1) % self.buf.len();
        y
    }
}
struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}
impl Allpass {
    fn run(&mut self, x: f32) -> f32 {
        let b = self.buf[self.idx];
        let y = -x + b;
        self.buf[self.idx] = x + b * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        y
    }
}

pub(crate) struct Freeverb {
    combs: [Vec<Comb>; 2],
    alls: [Vec<Allpass>; 2],
    fb: f32,
    damp: f32,
    width: f32,
    mix: f32,
}
impl Freeverb {
    pub(crate) fn new(rate: f32, room: f32, damping: f32, width: f32, mix: f32) -> Self {
        let sc = rate / 44100.0;
        let ct = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        let at = [556, 441, 341, 225];
        let mk = |spread: usize| {
            (
                ct.iter()
                    .map(|&t| Comb {
                        buf: vec![0.0; (((t + spread) as f32) * sc).max(1.0) as usize],
                        idx: 0,
                        store: 0.0,
                    })
                    .collect::<Vec<_>>(),
                at.iter()
                    .map(|&t| Allpass {
                        buf: vec![0.0; (((t + spread) as f32) * sc).max(1.0) as usize],
                        idx: 0,
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let (c0, a0) = mk(0);
        let (c1, a1) = mk(23);
        Freeverb {
            combs: [c0, c1],
            alls: [a0, a1],
            fb: 0.7 + room * 0.28,
            damp: damping * 0.4,
            width,
            mix,
        }
    }
}
impl Processor for Freeverb {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Freeverb>() else {
            return false;
        };
        self.fb = f.fb;
        self.damp = f.damp;
        self.width = f.width;
        self.mix = f.mix;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = buf.first().map(|c| c.len()).unwrap_or(0);
        let nch = buf.len();
        let wet1 = self.mix * (self.width / 2.0 + 0.5);
        let wet2 = self.mix * ((1.0 - self.width) / 2.0);
        for i in 0..n {
            let input = buf.iter().map(|c| c[i]).sum::<f32>() * 0.015 / nch as f32 * 2.0;
            let mut outs = [0.0f32; 2];
            for s in 0..2 {
                let mut acc = 0.0;
                for c in self.combs[s].iter_mut() {
                    acc += c.run(input, self.fb, self.damp);
                }
                for a in self.alls[s].iter_mut() {
                    acc = a.run(acc);
                }
                outs[s] = acc;
            }
            if nch == 1 {
                buf[0][i] =
                    buf[0][i] * (1.0 - self.mix * 0.5) + (outs[0] + outs[1]) * 0.5 * self.mix;
            } else {
                for (ci, c) in buf.iter_mut().enumerate() {
                    let (a, b) = if ci % 2 == 0 {
                        (outs[0], outs[1])
                    } else {
                        (outs[1], outs[0])
                    };
                    c[i] = c[i] * (1.0 - self.mix * 0.5) + a * wet1 + b * wet2;
                }
            }
        }
    }
}

struct ModDelay {
    rate: f32,
    lfo_hz: f32,
    base: f32,
    depth: f32,
    fb: f32,
    mix: f32,
    phase: f32,
    lines: Vec<Vec<f32>>,
    pos: usize,
}
impl ModDelay {
    fn new(rate: f32, lfo_hz: f32, base_ms: f32, depth_ms: f32, fb: f32, mix: f32) -> Self {
        ModDelay {
            rate,
            lfo_hz,
            base: base_ms / 1000.0 * rate,
            depth: depth_ms / 1000.0 * rate,
            fb,
            mix,
            phase: 0.0,
            lines: vec![],
            pos: 0,
        }
    }
}
impl Processor for ModDelay {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<ModDelay>() else {
            return false;
        };
        if (f.rate - self.rate).abs() > 0.5 {
            return false;
        }
        self.lfo_hz = f.lfo_hz;
        self.base = f.base;
        self.depth = f.depth;
        self.fb = f.fb;
        self.mix = f.mix;
        // the line is sized for the deepest sweep; keep it in step with the new settings
        let len = (self.base + self.depth + 4.0) as usize + 1;
        for l in self.lines.iter_mut() {
            l.resize(len, 0.0);
        }
        self.pos %= len.max(1);
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let len = (self.base + self.depth + 4.0) as usize + 1;
        if self.lines.len() < buf.len() {
            self.lines.resize(buf.len(), vec![0.0; len]);
        }
        let n = buf.first().map(|c| c.len()).unwrap_or(0);
        let inc = 2.0 * PI * self.lfo_hz / self.rate;
        for i in 0..n {
            for (ci, (c, line)) in buf.iter_mut().zip(self.lines.iter_mut()).enumerate() {
                let ph = self.phase + ci as f32 * PI * 0.5;
                let d = self.base + self.depth * (0.5 + 0.5 * ph.sin());
                let rp = (self.pos as f32 - d).rem_euclid(len as f32);
                let i0 = rp.floor() as usize % len;
                let i1 = (i0 + 1) % len;
                let fr = rp - rp.floor();
                let delayed = line[i0] * (1.0 - fr) + line[i1] * fr;
                let x = c[i];
                line[self.pos] = x + delayed * self.fb;
                c[i] = x * (1.0 - self.mix * 0.5) + delayed * self.mix;
            }
            self.pos = (self.pos + 1) % len;
            self.phase = (self.phase + inc) % (2.0 * PI);
        }
    }
}

struct Distort {
    drive: f32,
    mix: f32,
}
impl Processor for Distort {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Distort>() else {
            return false;
        };
        self.drive = f.drive;
        self.mix = f.mix;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let norm = 1.0 / self.drive.tanh().max(1e-6);
        for c in buf.iter_mut() {
            for s in c.iter_mut() {
                let w = (*s * self.drive).tanh() * norm;
                *s = *s * (1.0 - self.mix) + w * self.mix;
            }
        }
    }
}

struct Width {
    w: f32,
}
impl Processor for Width {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Width>() else {
            return false;
        };
        self.w = f.w;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        if buf.len() < 2 {
            return;
        }
        let (l, r) = buf.split_at_mut(1);
        for (a, b) in l[0].iter_mut().zip(r[0].iter_mut()) {
            let m = (*a + *b) * 0.5;
            let s = (*a - *b) * 0.5 * self.w;
            *a = m + s;
            *b = m - s;
        }
    }
}

// ---------------------------------------------------------------- offline processes

/// Run a chain over frames [a,b) of the data.
pub fn apply_chain(d: &mut AudioData, a: usize, b: usize, slots: &[EffectSlot]) {
    let mut chain = Chain::new(slots, d.rate as f32);
    if chain.is_empty() {
        return;
    }
    let (a, b) = d.span(a, b);
    let mut pos = a;
    let block = 4096;
    let mut tmp: Vec<Vec<f32>> = vec![vec![]; d.channels()];
    while pos < b {
        let e = (pos + block).min(b);
        for (t, c) in tmp.iter_mut().zip(d.ch.iter()) {
            t.clear();
            t.extend_from_slice(&c[pos..e]);
        }
        chain.process(&mut tmp);
        for (t, c) in tmp.iter().zip(d.ch.iter_mut()) {
            c[pos..e].copy_from_slice(t);
        }
        pos = e;
    }
}

pub fn normalize(d: &mut AudioData, a: usize, b: usize, target_db: f32) {
    let (a, b) = d.span(a, b);
    let a = a.min(b);
    let peak =
        d.ch.iter()
            .flat_map(|c| c[a..b].iter())
            .fold(0.0f32, |m, s| m.max(s.abs()));
    if peak <= 1e-9 {
        return;
    }
    let g = undb(target_db) / peak;
    for c in d.ch.iter_mut() {
        for s in c[a..b].iter_mut() {
            *s *= g;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FadeCurve {
    Linear,
    Logarithmic,
    Cosine,
}

pub fn fade_gain(x: f32, curve: FadeCurve) -> f32 {
    let x = x.clamp(0.0, 1.0);
    match curve {
        FadeCurve::Linear => x,
        FadeCurve::Logarithmic => x * x,
        FadeCurve::Cosine => 0.5 - 0.5 * (PI * x).cos(),
    }
}

pub fn fade(d: &mut AudioData, a: usize, b: usize, fade_in: bool, curve: FadeCurve) {
    let (a, b) = d.span(a, b);
    let n = (b - a).max(1) as f32;
    for c in d.ch.iter_mut() {
        for (i, s) in c[a..b].iter_mut().enumerate() {
            let x = i as f32 / n;
            *s *= fade_gain(if fade_in { x } else { 1.0 - x }, curve);
        }
    }
}

pub fn invert(d: &mut AudioData, a: usize, b: usize) {
    let (a, b) = d.span(a, b);
    for c in d.ch.iter_mut() {
        for s in c[a..b].iter_mut() {
            *s = -*s;
        }
    }
}

pub fn reverse(d: &mut AudioData, a: usize, b: usize) {
    let (a, b) = d.span(a, b);
    for c in d.ch.iter_mut() {
        c[a..b].reverse();
    }
}

pub fn silence(d: &mut AudioData, a: usize, b: usize) {
    let (a, b) = d.span(a, b);
    for c in d.ch.iter_mut() {
        for s in c[a..b].iter_mut() {
            *s = 0.0;
        }
    }
}

pub fn remove_dc(d: &mut AudioData, a: usize, b: usize) {
    let (a, b) = d.span(a, b);
    if b <= a {
        return;
    }
    for c in d.ch.iter_mut() {
        let mean = c[a..b].iter().map(|v| *v as f64).sum::<f64>() / (b - a) as f64;
        for s in c[a..b].iter_mut() {
            *s -= mean as f32;
        }
    }
}

/// Change speed (and pitch) of [a,b) by `ratio` (>1 faster).
pub fn change_speed(d: &AudioData, a: usize, b: usize, ratio: f64) -> AudioData {
    let seg = d.slice(a, b);
    // pretend the segment was recorded at rate*ratio, then convert back to the file rate
    let virt_rate = ((d.rate as f64) * ratio).round().max(1.0) as u32;
    let mut s2 = seg.clone();
    s2.rate = virt_rate;
    let mut r = crate::audio::resample(&s2, d.rate);
    r.rate = d.rate;
    let rest = d.without(a, b);
    rest.inserted(a, &r)
}

pub fn match_loudness(d: &mut AudioData, a: usize, b: usize, target_db: f32) {
    let (a, b) = d.span(a, b);
    if b <= a {
        return;
    }
    let ms: f64 =
        d.ch.iter()
            .flat_map(|c| c[a..b].iter())
            .map(|v| (*v as f64) * (*v as f64))
            .sum::<f64>()
            / ((b - a) * d.channels()) as f64;
    if ms <= 1e-12 {
        return;
    }
    let cur = 10.0 * ms.log10() as f32 - 0.691;
    let g = undb(target_db - cur);
    for c in d.ch.iter_mut() {
        for s in c[a..b].iter_mut() {
            *s = (*s * g).clamp(-1.0, 1.0);
        }
    }
}

// ---------------------------------------------------------------- FFT helpers

pub fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos())
        .collect()
}

/// Magnitude spectrum (linear) of one window starting at `start`.
pub struct Spectrum {
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    win: Vec<f32>,
    buf: Vec<rustfft::num_complex::Complex<f32>>,
    pub n: usize,
}

impl Spectrum {
    pub fn new(n: usize) -> Self {
        let mut planner = rustfft::FftPlanner::new();
        Spectrum {
            fft: planner.plan_fft_forward(n),
            win: hann(n),
            buf: vec![Default::default(); n],
            n,
        }
    }
    /// With a chosen window: 0 Blackman-Harris, 1 Hann, 2 Hamming, 3 Blackman, 4 rectangular.
    pub fn with_window(n: usize, window: u8) -> Self {
        let mut s = Spectrum::new(n);
        let m = (n.max(2) - 1) as f32;
        let tau = std::f32::consts::TAU;
        s.win = (0..n)
            .map(|i| {
                let x = i as f32 / m;
                match window {
                    0 => {
                        0.35875 - 0.48829 * (tau * x).cos() + 0.14128 * (2.0 * tau * x).cos()
                            - 0.01168 * (3.0 * tau * x).cos()
                    }
                    2 => 0.54 - 0.46 * (tau * x).cos(),
                    3 => 0.42 - 0.5 * (tau * x).cos() + 0.08 * (2.0 * tau * x).cos(),
                    4 => 1.0,
                    _ => 0.5 - 0.5 * (tau * x).cos(),
                }
            })
            .collect();
        s
    }
    pub fn mags(&mut self, src: &[f32], center: isize, out: &mut Vec<f32>) {
        let n = self.n;
        let start = center - n as isize / 2;
        for i in 0..n {
            let idx = start + i as isize;
            let v = if idx >= 0 && (idx as usize) < src.len() {
                src[idx as usize]
            } else {
                0.0
            };
            self.buf[i] = rustfft::num_complex::Complex::new(v * self.win[i], 0.0);
        }
        self.fft.process(&mut self.buf);
        out.clear();
        // a full-scale sine reads 1.0 whatever the window (divide by the window's sum)
        let norm = 2.0 / self.win.iter().sum::<f32>().max(1e-6);
        out.extend(self.buf[..n / 2].iter().map(|c| c.norm() * norm));
    }
}

/// Averaged spectrum (dB) over [a,b) of channel samples.
pub fn average_spectrum(src: &[f32], a: usize, b: usize, n: usize) -> Vec<f32> {
    let mut sp = Spectrum::new(n);
    let mut acc = vec![0.0f32; n / 2];
    let mut tmp = Vec::new();
    let mut count = 0;
    let b = b.min(src.len());
    let mut pos = a as isize + n as isize / 2;
    let hop = (n / 2).max(1) as isize;
    loop {
        sp.mags(src, pos, &mut tmp);
        for (x, y) in acc.iter_mut().zip(tmp.iter()) {
            *x += y * y;
        }
        count += 1;
        pos += hop;
        if pos + (n as isize / 2) > b as isize || count > 2000 {
            break;
        }
    }
    acc.iter().map(|v| db((v / count as f32).sqrt())).collect()
}

#[derive(Clone, Debug)]
pub struct NoiseProfile {
    pub n: usize,
    pub mags: Vec<Vec<f32>>,
    pub rate: u32,
}

pub fn capture_noise(d: &AudioData, a: usize, b: usize) -> NoiseProfile {
    let n = 2048;
    let mut sp = Spectrum::new(n);
    let mut tmp = Vec::new();
    let (a, b) = d.span(a, b);
    let mags =
        d.ch.iter()
            .map(|c| {
                let mut acc = vec![0.0f32; n / 2];
                let mut count = 0usize;
                let mut pos = a as isize + n as isize / 2;
                while pos <= b as isize - n as isize / 2 || count == 0 {
                    sp.mags(c, pos, &mut tmp);
                    for (x, y) in acc.iter_mut().zip(tmp.iter()) {
                        *x += *y;
                    }
                    count += 1;
                    pos += (n / 4) as isize;
                    if count > 4000 {
                        break;
                    }
                }
                acc.iter().map(|v| v / count as f32).collect()
            })
            .collect();
    NoiseProfile {
        n,
        mags,
        rate: d.rate,
    }
}

/// Spectral subtraction noise reduction over [a,b).
pub fn noise_reduce(
    d: &mut AudioData,
    a: usize,
    b: usize,
    prof: &NoiseProfile,
    amount: f32,
    reduce_db: f32,
) {
    use rustfft::num_complex::Complex;
    let n = prof.n;
    let hop = n / 4;
    let mut planner = rustfft::FftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(n);
    let inv = planner.plan_fft_inverse(n);
    let win = hann(n);
    let floor = undb(-reduce_db.abs());
    let (a, b) = d.span(a, b);
    if b <= a {
        return;
    }
    let len = b - a;
    let nchan = d.channels();
    if prof.mags.is_empty() {
        return;
    }
    // a profile captured at another sample rate: read it by frequency, not by bin number
    let ratio = if prof.rate > 0 {
        d.rate as f32 / prof.rate as f32
    } else {
        1.0
    };
    for ci in 0..nchan {
        let raw = &prof.mags[ci.min(prof.mags.len() - 1)];
        let rescaled: Vec<f32>;
        let noise: &Vec<f32> = if (ratio - 1.0).abs() > 1e-4 && !raw.is_empty() {
            rescaled = (0..raw.len())
                .map(|k| raw[((k as f32 * ratio).round() as usize).min(raw.len() - 1)])
                .collect();
            &rescaled
        } else {
            raw
        };
        let src: Vec<f32> = d.ch[ci][a..b].to_vec();
        let mut out = vec![0.0f32; len + n];
        let mut wsum = vec![0.0f32; len + n];
        let mut buf = vec![Complex::new(0.0f32, 0.0); n];
        let mut prev_gain = vec![1.0f32; n / 2 + 1];
        let mut pos: isize = -(n as isize) + hop as isize;
        while pos < len as isize {
            for i in 0..n {
                let idx = pos + i as isize;
                let v = if idx >= 0 && (idx as usize) < len {
                    src[idx as usize]
                } else {
                    0.0
                };
                buf[i] = Complex::new(v * win[i], 0.0);
            }
            fwd.process(&mut buf);
            let norm = 2.0 / (n as f32 * 0.5);
            for k in 0..=n / 2 {
                let mag = buf[k].norm() * norm;
                let nm = noise[k.min(noise.len() - 1)] * (1.0 + amount * 1.5);
                let mut g = if mag > 1e-12 {
                    ((mag - nm * amount.max(0.01)) / mag).max(0.0)
                } else {
                    0.0
                };
                g = g.max(floor);
                // temporal smoothing reduces musical noise
                g = 0.6 * g + 0.4 * prev_gain[k];
                prev_gain[k] = g;
                buf[k] *= g;
                if k > 0 && k < n / 2 {
                    buf[n - k] *= g;
                }
            }
            inv.process(&mut buf);
            for i in 0..n {
                let idx = pos + i as isize;
                if idx >= 0 && (idx as usize) < len {
                    out[idx as usize] += buf[i].re / n as f32 * win[i];
                    wsum[idx as usize] += win[i] * win[i];
                }
            }
            pos += hop as isize;
        }
        for i in 0..len {
            d.ch[ci][a + i] = if wsum[i] > 1e-6 {
                out[i] / wsum[i]
            } else {
                src[i]
            };
        }
    }
}

// ---------------------------------------------------------------- generators

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wave {
    Sine,
    Square,
    Triangle,
    Sawtooth,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoiseColor {
    White,
    Pink,
    Brown,
}

pub fn gen_tone(
    rate: u32,
    channels: usize,
    secs: f32,
    freq: f32,
    wave: Wave,
    amp_db: f32,
) -> AudioData {
    let n = (secs * rate as f32) as usize;
    let amp = undb(amp_db);
    let mut d = AudioData::new(rate, channels, n);
    for i in 0..n {
        let ph = (i as f32 * freq / rate as f32).fract();
        let v = match wave {
            Wave::Sine => (2.0 * PI * ph).sin(),
            Wave::Square => {
                if ph < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Triangle => 4.0 * (ph - 0.5).abs() - 1.0,
            Wave::Sawtooth => 2.0 * ph - 1.0,
        } * amp;
        for c in d.ch.iter_mut() {
            c[i] = v;
        }
    }
    d
}

pub fn gen_noise(
    rate: u32,
    channels: usize,
    secs: f32,
    color: NoiseColor,
    amp_db: f32,
) -> AudioData {
    let n = (secs * rate as f32) as usize;
    let amp = undb(amp_db);
    let mut d = AudioData::new(rate, channels, n);
    let mut seed: u64 = 0x9E3779B97F4A7C15;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        ((seed >> 11) as f64 / (1u64 << 53) as f64) as f32 * 2.0 - 1.0
    };
    for c in d.ch.iter_mut() {
        let (mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6) =
            (0f32, 0f32, 0f32, 0f32, 0f32, 0f32, 0f32);
        let mut brown = 0.0f32;
        for s in c.iter_mut() {
            let w = rnd();
            let v = match color {
                NoiseColor::White => w,
                NoiseColor::Pink => {
                    b0 = 0.99886 * b0 + w * 0.0555179;
                    b1 = 0.99332 * b1 + w * 0.0750759;
                    b2 = 0.96900 * b2 + w * 0.1538520;
                    b3 = 0.86650 * b3 + w * 0.3104856;
                    b4 = 0.55000 * b4 + w * 0.5329522;
                    b5 = -0.7616 * b5 - w * 0.0168980;
                    let p = b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362;
                    b6 = w * 0.115926;
                    p * 0.11
                }
                NoiseColor::Brown => {
                    brown = (brown + w * 0.02).clamp(-1.0, 1.0) * 0.998;
                    brown * 3.0
                }
            };
            *s = v.clamp(-1.0, 1.0) * amp;
        }
    }
    d
}

// ---------------------------------------------------------------- statistics

pub fn stats(d: &AudioData, a: usize, b: usize) -> Vec<(String, Vec<String>)> {
    let (a, b) = d.span(a, b);
    let n = b.saturating_sub(a).max(1);
    let win = (d.rate as usize / 20).max(1);
    let mut rows: Vec<(String, Vec<String>)> = vec![
        ("Peak Amplitude".into(), vec![]),
        ("Possibly Clipped Samples".into(), vec![]),
        ("DC Offset".into(), vec![]),
        ("Minimum Sample Value".into(), vec![]),
        ("Maximum Sample Value".into(), vec![]),
        ("Total RMS Amplitude".into(), vec![]),
        ("Maximum RMS Amplitude".into(), vec![]),
        ("Minimum RMS Amplitude".into(), vec![]),
        ("Average RMS Amplitude".into(), vec![]),
        ("Integrated Loudness (approx.)".into(), vec![]),
    ];
    for c in &d.ch {
        let s = &c[a.min(c.len())..b];
        let (mn, mx) = s
            .iter()
            .fold((0.0f32, 0.0f32), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        let peak = mn.abs().max(mx.abs());
        let clipped = s.iter().filter(|v| v.abs() >= 0.9999).count();
        let mean = s.iter().map(|v| *v as f64).sum::<f64>() / n as f64;
        let ms = s.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / n as f64;
        let mut rmax = 0.0f64;
        let mut rmin = f64::MAX;
        let mut rsum = 0.0f64;
        let mut cnt = 0;
        for w in s.chunks(win) {
            let r = (w.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / w.len() as f64).sqrt();
            rmax = rmax.max(r);
            rmin = rmin.min(r);
            rsum += r;
            cnt += 1;
        }
        if cnt == 0 {
            rmin = 0.0;
        }
        let fdb = |v: f64| {
            if v <= 1e-9 {
                "-inf dB".to_string()
            } else {
                format!("{:.2} dB", 20.0 * v.log10())
            }
        };
        rows[0].1.push(fdb(peak as f64));
        rows[1].1.push(format!("{clipped}"));
        rows[2].1.push(format!("{:.4} %", mean * 100.0));
        rows[3].1.push(format!("{mn:.4}"));
        rows[4].1.push(format!("{mx:.4}"));
        rows[5].1.push(fdb(ms.sqrt()));
        rows[6].1.push(fdb(rmax));
        rows[7].1.push(fdb(rmin));
        rows[8]
            .1
            .push(fdb(if cnt > 0 { rsum / cnt as f64 } else { 0.0 }));
        rows[9].1.push(if ms > 1e-12 {
            format!("{:.1} LUFS", 10.0 * ms.log10() - 0.691)
        } else {
            "-inf".into()
        });
    }
    rows
}

// ---------------------------------------------------------------- curved fades

/// Fade with a power curve: gain = x^p (p < 1 fast start, p > 1 slow start).
pub fn fade_pow(d: &mut AudioData, a: usize, b: usize, fade_in: bool, p: f32) {
    let (a, b) = d.span(a, b);
    if b <= a {
        return;
    }
    let n = (b - a) as f32;
    for c in d.ch.iter_mut() {
        for (i, s) in c[a..b].iter_mut().enumerate() {
            let x = i as f32 / n;
            let x = if fade_in { x } else { 1.0 - x };
            *s *= x.clamp(0.0, 1.0).powf(p);
        }
    }
}

// ---------------------------------------------------------------- time stretch (WSOLA)

/// Stretch planar audio by `alpha` (output length = input length * alpha) keeping pitch.
pub fn wsola(src: &AudioData, alpha: f64) -> AudioData {
    let n_in = src.frames();
    let rate = src.rate;
    if n_in == 0 || (alpha - 1.0).abs() < 1e-6 {
        return src.clone();
    }
    let frame = ((rate as f64 * 0.04) as usize).max(256) & !1;
    let hop = frame / 2;
    let tol = frame / 4;
    let n_out = (n_in as f64 * alpha).round() as usize;
    let win = hann(frame);
    // mono guide signal, decimated for a cheap coarse search
    let mono: Vec<f32> = (0..n_in)
        .map(|i| src.ch.iter().map(|c| c[i]).sum::<f32>() / src.channels() as f32)
        .collect();
    let dec = 8usize;
    let small: Vec<f32> = mono
        .chunks(dec)
        .map(|w| w.iter().sum::<f32>() / w.len() as f32)
        .collect();
    let corr = |sig: &[f32], a: usize, b: usize, len: usize, step: usize| -> f32 {
        let mut s = 0.0f32;
        let mut i = 0;
        while i < len {
            let x = sig.get(a + i).copied().unwrap_or(0.0);
            let y = sig.get(b + i).copied().unwrap_or(0.0);
            s += x * y;
            i += step;
        }
        s
    };
    let mut out = vec![vec![0.0f32; n_out + frame]; src.channels()];
    let mut wsum = vec![0.0f32; n_out + frame];
    let mut prev: usize = 0;
    let mut k = 0usize;
    loop {
        let op = k * hop;
        if op >= n_out {
            break;
        }
        let nominal = (op as f64 / alpha) as isize;
        let pos = if k == 0 {
            0usize
        } else {
            let target = prev + hop; // natural continuation of the previous frame
            let lo = (nominal - tol as isize).max(0) as usize;
            let hi = ((nominal + tol as isize).max(0) as usize).min(n_in.saturating_sub(1));
            // coarse search on decimated signal
            let mut best = lo;
            let mut bv = f32::MIN;
            let mut cand = lo / dec;
            while cand * dec <= hi {
                let v = corr(&small, target / dec, cand, frame / dec, 1);
                if v > bv {
                    bv = v;
                    best = cand * dec;
                }
                cand += 1;
            }
            // refine around the coarse best
            let r0 = best.saturating_sub(dec).max(lo);
            let r1 = (best + dec).min(hi);
            let mut fine = best;
            let mut fv = f32::MIN;
            for c2 in r0..=r1 {
                let v = corr(&mono, target, c2, frame, 2);
                if v > fv {
                    fv = v;
                    fine = c2;
                }
            }
            fine
        };
        for (ci, ch) in src.ch.iter().enumerate() {
            for i in 0..frame {
                let v = ch.get(pos + i).copied().unwrap_or(0.0);
                out[ci][op + i] += v * win[i];
            }
        }
        for i in 0..frame {
            wsum[op + i] += win[i];
        }
        prev = pos;
        k += 1;
    }
    for c in out.iter_mut() {
        for (i, s) in c.iter_mut().enumerate() {
            let w = wsum[i];
            if w > 1e-3 {
                *s /= w;
            }
        }
        c.truncate(n_out);
    }
    AudioData { rate, ch: out }
}

/// Replace [a,b) with a time-stretched version (`alpha` = new length / old length).
pub fn stretch_range(d: &AudioData, a: usize, b: usize, alpha: f64) -> AudioData {
    let seg = wsola(&d.slice(a, b), alpha);
    d.without(a, b).inserted(a, &seg)
}

/// Shift the pitch of [a,b) by `semis` semitones while keeping its length.
pub fn pitch_range(d: &AudioData, a: usize, b: usize, semis: f64) -> AudioData {
    let f = 2f64.powf(semis / 12.0);
    let seg = d.slice(a, b);
    let len = seg.frames();
    let stretched = wsola(&seg, f);
    let mut tmp = stretched;
    tmp.rate = ((d.rate as f64) * f).round().max(1.0) as u32;
    let mut res = crate::audio::resample(&tmp, d.rate);
    res.rate = d.rate;
    for c in res.ch.iter_mut() {
        c.resize(len, 0.0);
    }
    d.without(a, b).inserted(a, &res)
}

// ---------------------------------------------------------------- rack settings shared by file, track and clip racks

/// The controls around a rack's slots: the power switch for all its effects, input/output
/// gain, dry/wet mix and (tracks) whether the rack sits before or after the fader.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RackOpts {
    pub power: bool,
    pub in_db: f32,
    pub out_db: f32,
    /// 0 = dry, 1 = wet.
    pub mix: f32,
    pub pre_fader: bool,
    /// Show the Input/Output/Mix controls in the panel.
    pub show_io: bool,
}

impl Default for RackOpts {
    fn default() -> Self {
        RackOpts {
            power: true,
            in_db: 0.0,
            out_db: 0.0,
            mix: 1.0,
            pre_fader: true,
            show_io: true,
        }
    }
}

impl RackOpts {
    /// True when the rack would not change the audio at all.
    pub fn is_neutral(&self, chain_empty: bool) -> bool {
        !self.power || (chain_empty && self.in_db.abs() < 1e-6 && self.out_db.abs() < 1e-6)
    }
}

/// Run a rack: input gain, the effect chain, output gain, then the dry/wet mix.
pub fn run_rack(chain: &mut Chain, o: &RackOpts, buf: &mut [Vec<f32>], dry: &mut Vec<Vec<f32>>) {
    if o.is_neutral(chain.is_empty()) {
        return;
    }
    let gi = undb(o.in_db);
    let go = undb(o.out_db);
    let mix = o.mix.clamp(0.0, 1.0);
    let blend = mix < 0.999;
    if blend {
        dry.resize(buf.len(), vec![]);
        for (d, b) in dry.iter_mut().zip(buf.iter()) {
            d.clear();
            d.extend_from_slice(b);
        }
    }
    if (gi - 1.0).abs() > 1e-6 {
        for c in buf.iter_mut() {
            for s in c.iter_mut() {
                *s *= gi;
            }
        }
    }
    chain.process(buf);
    for (ci, c) in buf.iter_mut().enumerate() {
        for (i, s) in c.iter_mut().enumerate() {
            let w = *s * go;
            *s = if blend {
                dry[ci][i] * (1.0 - mix) + w * mix
            } else {
                w
            };
        }
    }
}

/// Editable curve of points in data space (frequency or time on x). Drag a point to move it,
/// double-click empty space to add one, double-click a point to remove it (two always stay).
pub fn curve_editor(
    ui: &mut egui::Ui,
    points: &mut Vec<(f32, f32)>,
    xr: (f32, f32),
    yr: (f32, f32),
    log_x: bool,
) -> bool {
    use egui::{Color32, Stroke, pos2};
    let w = ui.available_width().clamp(220.0, 620.0);
    let (resp, painter) = ui.allocate_painter(egui::vec2(w, 170.0), egui::Sense::click_and_drag());
    let r = resp.rect;
    painter.rect_filled(r, 0.0, Color32::from_rgb(18, 22, 26));
    let (x0, x1) = xr;
    let (y0, y1) = yr;
    let lx = |x: f32| if log_x { x.max(1e-3).ln() } else { x };
    let to_px = move |p: (f32, f32)| {
        let tx = (lx(p.0) - lx(x0)) / (lx(x1) - lx(x0)).max(1e-6);
        let ty = (p.1 - y0) / (y1 - y0).max(1e-6);
        pos2(
            r.left() + tx.clamp(0.0, 1.0) * r.width(),
            r.bottom() - ty.clamp(0.0, 1.0) * r.height(),
        )
    };
    let from_px = move |q: egui::Pos2| {
        let tx = ((q.x - r.left()) / r.width()).clamp(0.0, 1.0);
        let ty = ((r.bottom() - q.y) / r.height()).clamp(0.0, 1.0);
        let x = if log_x {
            (lx(x0) + tx * (lx(x1) - lx(x0))).exp()
        } else {
            x0 + tx * (x1 - x0)
        };
        (x, y0 + ty * (y1 - y0))
    };
    let grid = Stroke::new(1.0, crate::theme::gray(45));
    for k in 1..4 {
        let y = r.top() + r.height() * k as f32 / 4.0;
        painter.line_segment([pos2(r.left(), y), pos2(r.right(), y)], grid);
    }
    let zero = to_px((x0, 0.0f32.clamp(y0.min(y1), y0.max(y1))));
    painter.line_segment(
        [pos2(r.left(), zero.y), pos2(r.right(), zero.y)],
        Stroke::new(1.0, crate::theme::gray(80)),
    );
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut changed = false;
    let id = resp.id.with("curve_drag");
    let mut drag: Option<usize> = ui.data(|d| d.get_temp(id)).flatten();
    let near = |q: egui::Pos2, pts: &[(f32, f32)]| {
        pts.iter()
            .enumerate()
            .map(|(i, p)| (i, to_px(*p).distance(q)))
            .filter(|(_, d)| *d < 10.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    if resp.drag_started() {
        drag = ui
            .input(|i| i.pointer.press_origin())
            .and_then(|q| near(q, points));
    }
    if resp.dragged() {
        if let (Some(i), Some(q)) = (drag, resp.interact_pointer_pos()) {
            if i < points.len() {
                points[i] = from_px(q);
                changed = true;
            }
        }
    }
    if resp.drag_stopped() {
        drag = None;
    }
    if resp.double_clicked() {
        if let Some(q) = resp.interact_pointer_pos() {
            match near(q, points) {
                Some(i) if points.len() > 2 => {
                    points.remove(i);
                }
                Some(_) => {}
                None => points.push(from_px(q)),
            }
            changed = true;
        }
    }
    ui.data_mut(|d| d.insert_temp(id, drag));
    let mut sorted = points.clone();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut line: Vec<egui::Pos2> = vec![pos2(r.left(), to_px(sorted[0]).y)];
    line.extend(sorted.iter().map(|p| to_px(*p)));
    line.push(pos2(r.right(), to_px(sorted[sorted.len() - 1]).y));
    painter.add(egui::Shape::line(
        line,
        Stroke::new(2.0, Color32::from_rgb(80, 220, 140)),
    ));
    for p in sorted.iter() {
        painter.circle_filled(to_px(*p), 5.0, Color32::from_rgb(45, 140, 235));
    }
    if let Some(h) = resp.hover_pos() {
        let (x, y) = from_px(h);
        painter.text(
            r.right_top() + egui::vec2(-6.0, 4.0),
            egui::Align2::RIGHT_TOP,
            format!("{x:.0}  {y:+.1}"),
            egui::FontId::monospace(10.0),
            crate::theme::gray(180),
        );
    }
    changed
}
