//! Loudness metering (Loudness Meter, Loudness Radar Meter) and Automatic Pitch Correction.

use crate::dsp::Processor;
use std::sync::atomic::{AtomicU32, Ordering};

fn store(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

fn load(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

/// Latest readings of the loudness meters (momentary, short-term, integrated LUFS, true peak dB).
static LOUD_M: AtomicU32 = AtomicU32::new(0xc2c8_0000); // -100.0
static LOUD_S: AtomicU32 = AtomicU32::new(0xc2c8_0000);
static LOUD_I: AtomicU32 = AtomicU32::new(0xc2c8_0000);
static LOUD_PK: AtomicU32 = AtomicU32::new(0xc2c8_0000);
/// Latest correction of the automatic pitch correction, in cents.
static PITCH_CENTS: AtomicU32 = AtomicU32::new(0);

/// (momentary, short-term, integrated, peak) as last measured while playing.
pub fn loudness_readings() -> (f32, f32, f32, f32) {
    (load(&LOUD_M), load(&LOUD_S), load(&LOUD_I), load(&LOUD_PK))
}

/// Clear held loudness and true-peak readings when playback starts on a new source.
pub fn reset_loudness() {
    store(&LOUD_M, -100.0);
    store(&LOUD_S, -100.0);
    store(&LOUD_I, -100.0);
    store(&LOUD_PK, -100.0);
}

pub fn pitch_correction_cents() -> f32 {
    load(&PITCH_CENTS)
}

/// Passes audio through unchanged and measures its loudness (K-weighting approximated by a
/// high-pass at 60 Hz and a +4 dB shelf above 2 kHz; 400 ms momentary, 3 s short-term windows,
/// gated integrated loudness).
pub struct LoudnessMeter {
    rate: f32,
    hp: Vec<(f32, f32)>,
    sh: Vec<(f32, f32)>,
    m: f64,
    s: f64,
    sum: f64,
    blocks: u64,
    acc: f64,
    acc_n: usize,
    pk: f32,
}

impl LoudnessMeter {
    pub fn new(rate: f32) -> Self {
        LoudnessMeter {
            rate: rate.max(1.0),
            hp: vec![],
            sh: vec![],
            m: 0.0,
            s: 0.0,
            sum: 0.0,
            blocks: 0,
            acc: 0.0,
            acc_n: 0,
            pk: 0.0,
        }
    }
}

impl Processor for LoudnessMeter {
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let nch = buf.len();
        self.hp.resize(nch, (0.0, 0.0));
        self.sh.resize(nch, (0.0, 0.0));
        let n = buf.first().map(|c| c.len()).unwrap_or(0);
        let a_hp = (-2.0 * std::f32::consts::PI * 60.0 / self.rate).exp();
        let a_sh = (-2.0 * std::f32::consts::PI * 2000.0 / self.rate).exp();
        let km = (-1.0 / (0.4 * self.rate as f64)).exp();
        let ks = (-1.0 / (3.0 * self.rate as f64)).exp();
        let block = (self.rate as usize / 10).max(1);
        for i in 0..n {
            let mut e = 0.0f64;
            for (c, ch) in buf.iter().enumerate() {
                let x = ch[i];
                self.pk = self.pk.max(x.abs());
                // one-pole high-pass, then a gentle high shelf
                let (px, py) = self.hp[c];
                let y = a_hp * (py + x - px);
                self.hp[c] = (x, y);
                let (lp, _) = self.sh[c];
                let low = lp + (1.0 - a_sh) * (y - lp);
                self.sh[c] = (low, 0.0);
                let z = y + 0.585 * (y - low);
                e += (z as f64) * (z as f64);
            }
            self.m = km * self.m + (1.0 - km) * e;
            self.s = ks * self.s + (1.0 - ks) * e;
            self.acc += e;
            self.acc_n += 1;
            if self.acc_n >= block {
                let ms = self.acc / self.acc_n as f64;
                // absolute gate at -70 LUFS
                if ms > 1e-7 {
                    self.sum += ms;
                    self.blocks += 1;
                }
                self.acc = 0.0;
                self.acc_n = 0;
            }
        }
        let lufs = |ms: f64| {
            if ms > 1e-12 {
                (-0.691 + 10.0 * ms.log10()) as f32
            } else {
                -100.0
            }
        };
        store(&LOUD_M, lufs(self.m));
        store(&LOUD_S, lufs(self.s));
        store(
            &LOUD_I,
            if self.blocks > 0 {
                lufs(self.sum / self.blocks as f64)
            } else {
                -100.0
            },
        );
        store(&LOUD_PK, crate::audio::db(self.pk));
        self.pk *= 0.97;
    }
}

/// Automatic pitch correction: finds the pitch of the sound, picks the nearest note of the
/// chosen scale and key, and moves the pitch towards it at the attack speed.
pub struct AutoPitch {
    rate: f32,
    scale: u8,
    key: u8,
    attack: f32,
    sensitivity: f32,
    ref_ch: u8,
    a4: f32,
    win: usize,
    hist: Vec<f32>,
    cents: f32,
    target: f32,
    // pitch shifter: a delay line read by two heads that fade in and out
    line: Vec<Vec<f32>>,
    pos: usize,
    phase: f32,
    since: usize,
}

impl AutoPitch {
    pub fn new(
        rate: f32,
        scale: u8,
        key: u8,
        attack: f32,
        sensitivity: f32,
        ref_ch: u8,
        fft: u32,
        a4: f32,
    ) -> Self {
        AutoPitch {
            rate: rate.max(1.0),
            scale,
            key: key % 12,
            attack: attack.clamp(1.0, 10.0),
            sensitivity: sensitivity.clamp(0.0, 200.0),
            ref_ch,
            a4: a4.clamp(410.0, 470.0),
            win: (fft as usize).clamp(512, 8192),
            hist: vec![],
            cents: 0.0,
            target: 0.0,
            line: vec![],
            pos: 0,
            phase: 0.0,
            since: 0,
        }
    }

    /// Pitch of the last `win` samples in Hz (difference-function method), if clear enough.
    fn detect(&self) -> Option<f32> {
        let x = &self.hist;
        let n = x.len();
        if n < self.win {
            return None;
        }
        let x = &x[n - self.win..];
        let rms = (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        if rms < 0.003 {
            return None;
        }
        let min_lag = (self.rate / 1000.0) as usize;
        let max_lag = ((self.rate / 60.0) as usize).min(self.win / 2);
        let w = self.win / 2;
        let mut d = vec![0.0f32; max_lag + 1];
        for (lag, dv) in d.iter_mut().enumerate().take(max_lag + 1).skip(1) {
            let mut s = 0.0;
            for i in 0..w {
                let e = x[i] - x[i + lag];
                s += e * e;
            }
            *dv = s;
        }
        // cumulative mean normalised difference
        let mut run = 0.0;
        let mut best: Option<usize> = None;
        let thr = 0.05 + 0.4 * (self.sensitivity / 200.0);
        for lag in 1..=max_lag {
            run += d[lag];
            let nd = if run > 0.0 {
                d[lag] * lag as f32 / run
            } else {
                1.0
            };
            if lag >= min_lag && nd < thr {
                // walk to the local minimum
                let mut l = lag;
                while l + 1 <= max_lag && d[l + 1] < d[l] {
                    l += 1;
                }
                best = Some(l);
                break;
            }
        }
        best.map(|l| self.rate / l as f32)
    }

    /// Correction in cents that brings `hz` to the nearest note of the scale.
    pub fn correction(&self, hz: f32) -> f32 {
        let midi = 69.0 + 12.0 * (hz / self.a4).log2();
        let allowed: &[u8] = match self.scale {
            0 => &[0, 2, 4, 5, 7, 9, 11],
            1 => &[0, 2, 3, 5, 7, 8, 10],
            _ => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        };
        let base = midi.floor() as i32;
        let mut best = f32::MAX;
        for cand in (base - 2)..=(base + 2) {
            let pc = (cand - self.key as i32).rem_euclid(12) as u8;
            if allowed.contains(&pc) {
                let diff = cand as f32 - midi;
                if diff.abs() < best.abs() {
                    best = diff;
                }
            }
        }
        if best == f32::MAX { 0.0 } else { best * 100.0 }
    }
}

impl Processor for AutoPitch {
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let nch = buf.len();
        if nch == 0 {
            return;
        }
        let n = buf[0].len();
        let size = 2048usize;
        if self.line.len() != nch {
            self.line = vec![vec![0.0; size]; nch];
        }
        let hop = self.win / 4;
        // attack: 1 slow .. 10 fast
        let k = 0.0005 * self.attack * self.attack;
        for i in 0..n {
            let r = match (self.ref_ch, nch) {
                (1, 2..) => buf[1][i],
                (0, _) => buf[0][i],
                _ => buf.iter().map(|c| c[i]).sum::<f32>() / nch as f32,
            };
            self.hist.push(r);
            self.since += 1;
            if self.since >= hop {
                self.since = 0;
                self.target = match self.detect() {
                    Some(hz) => self.correction(hz),
                    None => 0.0,
                };
                let keep = self.win * 2;
                if self.hist.len() > keep {
                    let cut = self.hist.len() - keep;
                    self.hist.drain(..cut);
                }
            }
            self.cents += (self.target - self.cents) * k;
            let ratio = 2f32.powf(self.cents / 1200.0);
            // two read heads half a window apart, each faded by a sine window
            self.phase = (self.phase + (1.0 - ratio) / size as f32).rem_euclid(1.0);
            let p2 = (self.phase + 0.5).rem_euclid(1.0);
            let g1 = (std::f32::consts::PI * self.phase).sin().powi(2);
            let g2 = (std::f32::consts::PI * p2).sin().powi(2);
            for c in 0..nch {
                self.line[c][self.pos] = buf[c][i];
                let read = |ph: f32, line: &Vec<f32>| {
                    let d = ph * (size as f32 - 2.0) + 1.0;
                    let rp = (self.pos as f32 - d).rem_euclid(size as f32);
                    let a = rp.floor() as usize % size;
                    let b = (a + 1) % size;
                    let f = rp - rp.floor();
                    line[a] * (1.0 - f) + line[b] * f
                };
                buf[c][i] = g1 * read(self.phase, &self.line[c]) + g2 * read(p2, &self.line[c]);
            }
            self.pos = (self.pos + 1) % size;
        }
        store(&PITCH_CENTS, self.cents);
    }
}

/// Pitch (Hz) of a short stretch of sound, or None when it has no clear pitch. `sensitivity`
/// 0..200: higher accepts less clear pitches. Range about 50 Hz .. 1.5 kHz.
pub fn detect_pitch(x: &[f32], rate: f32, sensitivity: f32) -> Option<f32> {
    let n = x.len();
    if n < 64 || rate <= 0.0 {
        return None;
    }
    let rms = (x.iter().map(|v| v * v).sum::<f32>() / n as f32).sqrt();
    if rms < 0.003 {
        return None;
    }
    let min_lag = ((rate / 1500.0) as usize).max(2);
    let max_lag = ((rate / 50.0) as usize).min(n / 2);
    if max_lag <= min_lag {
        return None;
    }
    let w = n - max_lag;
    let mut d = vec![0.0f32; max_lag + 1];
    for (lag, dv) in d.iter_mut().enumerate().skip(1) {
        let mut s = 0.0;
        for i in 0..w {
            let e = x[i] - x[i + lag];
            s += e * e;
        }
        *dv = s;
    }
    let thr = 0.05 + 0.4 * (sensitivity.clamp(0.0, 200.0) / 200.0);
    let mut run = 0.0;
    for lag in 1..=max_lag {
        run += d[lag];
        let nd = if run > 0.0 {
            d[lag] * lag as f32 / run
        } else {
            1.0
        };
        if lag >= min_lag && nd < thr {
            let mut l = lag;
            while l < max_lag && d[l + 1] < d[l] {
                l += 1;
            }
            // parabolic interpolation around the minimum
            let (a, b, c) = (d[l - 1], d[l], d[(l + 1).min(max_lag)]);
            let den = a - 2.0 * b + c;
            let off = if den.abs() > 1e-12 {
                0.5 * (a - c) / den
            } else {
                0.0
            };
            return Some(rate / (l as f32 + off.clamp(-0.5, 0.5)));
        }
    }
    None
}

/// Graphic Phase Shifter: turns the phase of each frequency by the curve's angle (short-time
/// Fourier transform, 75% overlap; latency one FFT frame).
pub struct PhaseCurve {
    n: usize,
    hop: usize,
    rot: Vec<rustfft::num_complex::Complex<f32>>,
    channel: u8,
    win: Vec<f32>,
    inp: Vec<Vec<f32>>,
    outq: Vec<std::collections::VecDeque<f32>>,
    acc: Vec<Vec<f32>>,
    fwd: std::sync::Arc<dyn rustfft::Fft<f32>>,
    inv: std::sync::Arc<dyn rustfft::Fft<f32>>,
    points: Vec<(f32, f32)>,
    rate_set: f32,
}

impl PhaseCurve {
    pub fn new(points: Vec<(f32, f32)>, channel: u8, fft: u32) -> Self {
        let n = (fft as usize).clamp(256, 16384).next_power_of_two();
        let mut planner = rustfft::FftPlanner::<f32>::new();
        let win = (0..n)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
            .collect();
        PhaseCurve {
            n,
            hop: n / 4,
            rot: vec![],
            channel,
            win,
            inp: vec![],
            outq: vec![],
            acc: vec![],
            fwd: planner.plan_fft_forward(n),
            inv: planner.plan_fft_inverse(n),
            points,
            rate_set: 0.0,
        }
    }

    /// The angle (degrees) the curve gives frequency `f` (straight lines between points).
    pub fn angle_at(points: &[(f32, f32)], f: f32) -> f32 {
        match points.len() {
            0 => 0.0,
            1 => points[0].1,
            _ => {
                if f <= points[0].0 {
                    return points[0].1;
                }
                for w in points.windows(2) {
                    if f <= w[1].0 {
                        let t = (f.max(1.0).ln() - w[0].0.max(1.0).ln())
                            / (w[1].0.max(1.0).ln() - w[0].0.max(1.0).ln()).max(1e-6);
                        return w[0].1 + (w[1].1 - w[0].1) * t.clamp(0.0, 1.0);
                    }
                }
                points[points.len() - 1].1
            }
        }
    }

    pub fn set_rate(&mut self, rate: f32) {
        if (self.rate_set - rate).abs() < 0.5 {
            return;
        }
        self.rate_set = rate;
        let n = self.n;
        self.rot = (0..=n / 2)
            .map(|j| {
                let f = j as f32 * rate / n as f32;
                let a = Self::angle_at(&self.points, f).to_radians();
                rustfft::num_complex::Complex::new(a.cos(), a.sin())
            })
            .collect();
    }
}

impl Processor for PhaseCurve {
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        // the processor does not know the rate; 48 kHz is assumed for the curve's frequencies
        // until told otherwise (racks run at the output rate)
        if self.rate_set == 0.0 {
            self.set_rate(48000.0);
        }
        let nch = buf.len();
        let n = self.n;
        if self.inp.len() != nch {
            self.inp = vec![vec![]; nch];
            self.outq = (0..nch)
                .map(|_| std::iter::repeat(0.0).take(n).collect())
                .collect();
            self.acc = vec![vec![0.0; n]; nch];
        }
        let len = buf.first().map(|c| c.len()).unwrap_or(0);
        for c in 0..nch {
            let active =
                self.channel == 0 || (self.channel == 1 && c == 0) || (self.channel == 2 && c == 1);
            for i in 0..len {
                self.inp[c].push(buf[c][i]);
                if self.inp[c].len() == n {
                    let mut fr: Vec<rustfft::num_complex::Complex<f32>> = self.inp[c]
                        .iter()
                        .zip(&self.win)
                        .map(|(x, w)| rustfft::num_complex::Complex::new(x * w, 0.0))
                        .collect();
                    if active {
                        self.fwd.process(&mut fr);
                        for j in 0..=n / 2 {
                            fr[j] *= self.rot[j];
                        }
                        for j in 1..n / 2 {
                            fr[n - j] = fr[j].conj();
                        }
                        self.inv.process(&mut fr);
                        for v in fr.iter_mut() {
                            *v /= n as f32;
                        }
                    }
                    // overlap-add with the synthesis window; hann^2 at 75% overlap sums to 1.5
                    for k in 0..n {
                        self.acc[c][k] += fr[k].re * self.win[k] / 1.5;
                    }
                    for k in 0..self.hop {
                        self.outq[c].push_back(self.acc[c][k]);
                    }
                    self.acc[c].drain(..self.hop);
                    self.acc[c].extend(std::iter::repeat(0.0).take(self.hop));
                    self.inp[c].drain(..self.hop);
                }
                buf[c][i] = self.outq[c].pop_front().unwrap_or(0.0);
            }
        }
    }
}

/// Automatic Phase Correction (as a rack effect): finds how far the right channel lags or
/// leads the left (cross-correlation over recent blocks) and delays the earlier one to match.
pub struct PhaseCorrection {
    max: usize,
    hist: [Vec<f32>; 2],
    lag: i64,
    line: [std::collections::VecDeque<f32>; 2],
    count: usize,
}

impl PhaseCorrection {
    pub fn new(rate: f32, max_ms: f32) -> Self {
        let max = ((max_ms.clamp(0.1, 20.0) / 1000.0) * rate) as usize;
        PhaseCorrection {
            max: max.max(1),
            hist: [vec![], vec![]],
            lag: 0,
            line: [Default::default(), Default::default()],
            count: 0,
        }
    }

    /// Lag (in samples) of the right channel behind the left that best lines them up.
    pub fn best_lag(l: &[f32], r: &[f32], max: usize) -> i64 {
        let n = l.len().min(r.len());
        if n <= 2 * max {
            return 0;
        }
        let mut best = (0i64, f32::MIN);
        for lag in -(max as i64)..=(max as i64) {
            let mut s = 0.0f32;
            for i in max..n - max {
                let j = (i as i64 + lag) as usize;
                s += l[i] * r[j];
            }
            if s > best.1 {
                best = (lag, s);
            }
        }
        best.0
    }
}

impl Processor for PhaseCorrection {
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        if buf.len() < 2 {
            return;
        }
        let n = buf[0].len();
        for c in 0..2 {
            self.hist[c].extend_from_slice(&buf[c]);
            let keep = 8192;
            if self.hist[c].len() > keep {
                let cut = self.hist[c].len() - keep;
                self.hist[c].drain(..cut);
            }
        }
        self.count += n;
        if self.count >= 4096 {
            self.count = 0;
            self.lag = Self::best_lag(&self.hist[0], &self.hist[1], self.max);
        }
        // delay the channel that comes first by the lag
        let (lead, d) = if self.lag > 0 {
            (0usize, self.lag as usize)
        } else {
            (1usize, (-self.lag) as usize)
        };
        for c in 0..2 {
            let delay = if c == lead { d } else { 0 };
            let line = &mut self.line[c];
            for s in buf[c].iter_mut() {
                line.push_back(*s);
                while line.len() > delay + 1 {
                    line.pop_front();
                }
                *s = if line.len() > delay { line[0] } else { 0.0 };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::{self, Wave};

    #[test]
    fn pitch_correction_finds_the_nearest_scale_note() {
        let p = AutoPitch::new(48000.0, 2, 0, 5.0, 100.0, 2, 2048, 440.0);
        // 450 Hz is about +39 cents over A4: pulled down
        let c = p.correction(450.0);
        assert!(c < -30.0 && c > -45.0, "{c}");
        assert!(p.correction(440.0).abs() < 0.5);
        // C major has no C#: 277 Hz (C#4) goes to C or D
        let maj = AutoPitch::new(48000.0, 0, 0, 5.0, 100.0, 2, 2048, 440.0);
        assert!(maj.correction(277.18).abs() > 90.0);
    }

    #[test]
    fn auto_pitch_detects_and_keeps_level() {
        let d = dsp::gen_tone(48000, 1, 1.0, 452.0, Wave::Sine, -12.0);
        let mut p = AutoPitch::new(48000.0, 2, 0, 10.0, 100.0, 0, 2048, 440.0);
        let mut buf = vec![d.ch[0].clone()];
        p.process(&mut buf);
        assert!(p.detect().map(|f| (f - 452.0).abs() < 5.0).unwrap_or(false));
        assert!(p.cents < -10.0, "moving towards A4: {}", p.cents);
        let rms = (buf[0][24000..].iter().map(|v| v * v).sum::<f32>() / 24000.0).sqrt();
        assert!(rms > 0.1, "{rms}");
    }

    #[test]
    fn loudness_meter_reads_a_tone() {
        let d = dsp::gen_tone(48000, 2, 1.0, 1000.0, Wave::Sine, -20.0);
        let mut m = LoudnessMeter::new(48000.0);
        let mut buf = d.ch.clone();
        m.process(&mut buf);
        assert_eq!(buf, d.ch);
        let (mo, _, i, pk) = loudness_readings();
        assert!(
            mo > -30.0 && mo < -10.0 && i > -30.0 && pk > -21.0,
            "{mo} {i} {pk}"
        );
    }
}
