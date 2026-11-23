//! Spectral selections (marquee, lasso, paintbrush) over the spectral frequency display, and the
//! edits made through them: gain on just those time/frequency cells, silence, and healing (the
//! selected cells take the level of the sound around them).

use crate::audio::AudioData;
use rustfft::{FftPlanner, num_complex::Complex};

/// A region of the time x frequency plane. Times in seconds, frequencies in Hz.
#[derive(Clone, Debug, PartialEq)]
pub enum SpecShape {
    Rect {
        t0: f64,
        t1: f64,
        f0: f32,
        f1: f32,
    },
    /// Closed polygon of (time, Hz) points.
    Lasso(Vec<(f64, f32)>),
    /// Paint strokes: centres (time, Hz), radius in seconds and in octaves, and how strongly the
    /// painted cells are selected (0..1; edges fade out softly).
    Brush {
        pts: Vec<(f64, f32)>,
        rt: f64,
        ro: f32,
        opacity: f32,
    },
    /// Everything in the inner shape's time range except the inner shape (up to `top` Hz).
    Inverse {
        inner: Box<SpecShape>,
        top: f32,
    },
}

fn oct(f: f32) -> f32 {
    f.max(1.0).log2()
}

impl SpecShape {
    /// Time and frequency bounds (t0, t1, f0, f1).
    pub fn bounds(&self) -> (f64, f64, f32, f32) {
        match self {
            SpecShape::Rect { t0, t1, f0, f1 } => {
                (t0.min(*t1), t0.max(*t1), f0.min(*f1), f0.max(*f1))
            }
            SpecShape::Lasso(p) => {
                let mut b = (f64::MAX, f64::MIN, f32::MAX, f32::MIN);
                for (t, f) in p {
                    b = (b.0.min(*t), b.1.max(*t), b.2.min(*f), b.3.max(*f));
                }
                if p.is_empty() {
                    (0.0, 0.0, 0.0, 0.0)
                } else {
                    b
                }
            }
            SpecShape::Brush { pts, rt, ro, .. } => {
                let mut b = (f64::MAX, f64::MIN, f32::MAX, f32::MIN);
                for (t, f) in pts {
                    let k = 2f32.powf(*ro);
                    b = (
                        b.0.min(t - rt),
                        b.1.max(t + rt),
                        b.2.min(f / k),
                        b.3.max(f * k),
                    );
                }
                if pts.is_empty() {
                    (0.0, 0.0, 0.0, 0.0)
                } else {
                    (b.0.max(0.0), b.1, b.2, b.3)
                }
            }
            SpecShape::Inverse { inner, top } => {
                let (t0, t1, _, _) = inner.bounds();
                (t0, t1, 0.0, *top)
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        let (t0, t1, f0, f1) = self.bounds();
        match self {
            SpecShape::Lasso(p) => p.len() < 3 || t1 <= t0,
            SpecShape::Inverse { inner, .. } => {
                t1 <= t0 || matches!(**inner, SpecShape::Lasso(ref p) if p.len() < 3)
            }
            _ => t1 <= t0 || f1 <= f0,
        }
    }

    /// How strongly the cell at (t, f) is selected: 0 (not at all) to 1.
    pub fn weight(&self, t: f64, f: f32) -> f32 {
        match self {
            SpecShape::Rect { .. } => {
                let (t0, t1, f0, f1) = self.bounds();
                if t >= t0 && t <= t1 && f >= f0 && f <= f1 {
                    1.0
                } else {
                    0.0
                }
            }
            SpecShape::Lasso(p) => {
                // even-odd rule in (time, octave) coordinates
                if p.len() < 3 {
                    return 0.0;
                }
                let (x, y) = (t, oct(f) as f64);
                let mut inside = false;
                let mut j = p.len() - 1;
                for i in 0..p.len() {
                    let (xi, yi) = (p[i].0, oct(p[i].1) as f64);
                    let (xj, yj) = (p[j].0, oct(p[j].1) as f64);
                    if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                        inside = !inside;
                    }
                    j = i;
                }
                if inside { 1.0 } else { 0.0 }
            }
            SpecShape::Brush {
                pts,
                rt,
                ro,
                opacity,
            } => {
                let mut w = 0.0f32;
                for (pt, pf) in pts {
                    let a = (t - pt) / rt.max(1e-6);
                    let b = ((oct(f) - oct(*pf)) / ro.max(1e-4)) as f64;
                    let d = (a * a + b * b).sqrt() as f32;
                    // full inside 75% of the radius, then a soft edge
                    let k = if d <= 0.75 {
                        1.0
                    } else if d < 1.0 {
                        (1.0 - d) / 0.25
                    } else {
                        0.0
                    };
                    w = w.max(k);
                    if w >= 1.0 {
                        break;
                    }
                }
                w * opacity.clamp(0.0, 1.0)
            }
            SpecShape::Inverse { inner, top } => {
                let (t0, t1, _, _) = inner.bounds();
                if t >= t0 && t <= t1 && f <= *top {
                    1.0 - inner.weight(t, f)
                } else {
                    0.0
                }
            }
        }
    }

    /// Weights on a grid: `times` ascending, `freqs` ascending or descending; the result is
    /// row-major by time (`[ti * freqs.len() + fi]`). Paint strokes are stamped dab by dab over
    /// just the cells each dab covers, so long strokes stay quick.
    pub fn grid_weights(&self, times: &[f64], freqs: &[f32]) -> Vec<f32> {
        let nf = freqs.len();
        let mut out = vec![0.0f32; times.len() * nf];
        match self {
            SpecShape::Brush {
                pts,
                rt,
                ro,
                opacity,
            } => {
                let octs: Vec<f32> = freqs.iter().map(|f| oct(*f)).collect();
                let asc = nf < 2 || freqs[0] <= freqs[nf - 1];
                let op = opacity.clamp(0.0, 1.0);
                for (pt, pf) in pts {
                    let t0 = times.partition_point(|t| *t < pt - rt);
                    let t1 = times.partition_point(|t| *t <= pt + rt);
                    let po = oct(*pf);
                    let (lo, hi) = (po - ro, po + ro);
                    let (f0, f1) = if asc {
                        (
                            octs.partition_point(|o| *o < lo),
                            octs.partition_point(|o| *o <= hi),
                        )
                    } else {
                        (
                            octs.partition_point(|o| *o > hi),
                            octs.partition_point(|o| *o >= lo),
                        )
                    };
                    for ti in t0..t1 {
                        let a = (times[ti] - pt) / rt.max(1e-6);
                        for fi in f0..f1.max(f0) {
                            let b = ((octs[fi] - po) / ro.max(1e-4)) as f64;
                            let d = (a * a + b * b).sqrt() as f32;
                            let k = (if d <= 0.75 {
                                1.0
                            } else if d < 1.0 {
                                (1.0 - d) / 0.25
                            } else {
                                0.0
                            }) * op;
                            let cell = &mut out[ti * nf + fi];
                            if k > *cell {
                                *cell = k;
                            }
                        }
                    }
                }
            }
            SpecShape::Inverse { inner, top } => {
                let w = inner.grid_weights(times, freqs);
                let (t0, t1, _, _) = inner.bounds();
                for (ti, t) in times.iter().enumerate() {
                    if *t < t0 || *t > t1 {
                        continue;
                    }
                    for (fi, f) in freqs.iter().enumerate() {
                        if *f <= *top {
                            out[ti * nf + fi] = 1.0 - w[ti * nf + fi];
                        }
                    }
                }
            }
            _ => {
                let (t0, t1, _, _) = self.bounds();
                for (ti, t) in times.iter().enumerate() {
                    if *t < t0 || *t > t1 {
                        continue;
                    }
                    for (fi, f) in freqs.iter().enumerate() {
                        out[ti * nf + fi] = self.weight(*t, *f);
                    }
                }
            }
        }
        out
    }

    /// A cheap fingerprint of the shape (for knowing when a drawing of it is out of date).
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let mut put = |t: f64, f: f32| {
            t.to_bits().hash(&mut h);
            f.to_bits().hash(&mut h);
        };
        match self {
            SpecShape::Rect { t0, t1, f0, f1 } => {
                put(*t0, *f0);
                put(*t1, *f1);
            }
            SpecShape::Lasso(p) => p.iter().for_each(|(t, f)| put(*t, *f)),
            SpecShape::Brush {
                pts,
                rt,
                ro,
                opacity,
            } => {
                put(*rt, *ro);
                put(0.0, *opacity);
                pts.iter().for_each(|(t, f)| put(*t, *f));
            }
            SpecShape::Inverse { inner, top } => {
                put(-1.0, *top);
                put(inner.fingerprint() as f64, 0.0);
            }
        }
        h.finish()
    }

    pub fn contains(&self, t: f64, f: f32) -> bool {
        self.weight(t, f) > 0.0
    }

    /// Move by `dt` seconds and `df` Hz. Every frequency moves by the same number of Hz, so on
    /// the logarithmic display a selection shrinks as it goes up and grows as it comes down.
    pub fn translate(&mut self, dt: f64, df: f32) {
        let df = if df.is_finite() { df } else { 0.0 };
        match self {
            SpecShape::Rect { t0, t1, f0, f1 } => {
                let df = df.max(1.0 - f0.min(*f1));
                *t0 += dt;
                *t1 += dt;
                *f0 += df;
                *f1 += df;
            }
            SpecShape::Lasso(p) | SpecShape::Brush { pts: p, .. } => {
                let lo = p.iter().map(|q| q.1).fold(f32::MAX, f32::min);
                let df = if lo.is_finite() { df.max(1.0 - lo) } else { df };
                for (t, f) in p.iter_mut() {
                    *t += dt;
                    *f += df;
                }
            }
            SpecShape::Inverse { inner, .. } => inner.translate(dt, df),
        }
    }

    /// Move proportionally: by `dt` seconds and every frequency times `k` (so the selection keeps
    /// its octaves and its Hz range grows as it goes up and shrinks as it comes down, on either
    /// scale). `k` is limited so the whole selection stays inside `lo`..`hi`; returns the factor
    /// used, so the caller can keep the grabbed point under the pointer.
    pub fn move_proportional(&mut self, dt: f64, k: f32, lo: f32, hi: f32) -> f32 {
        let (fmin, fmax) = self.freq_extent();
        let mut k = if k.is_finite() && k > 0.0 { k } else { 1.0 };
        if fmin > 0.0 && fmax > 0.0 && fmax / fmin <= hi / lo.max(1e-6) {
            k = k.clamp(lo / fmin, (hi / fmax).max(lo / fmin));
        } else {
            k = 1.0;
        }
        self.translate_with(dt, &|x| x * k, 0.0, f32::INFINITY);
        k
    }

    /// The lowest and highest frequency the shape reaches.
    pub fn freq_extent(&self) -> (f32, f32) {
        match self {
            SpecShape::Rect { f0, f1, .. } => (f0.min(*f1), f0.max(*f1)),
            SpecShape::Lasso(p) => p
                .iter()
                .fold((f32::MAX, 0.0f32), |(a, b), q| (a.min(q.1), b.max(q.1))),
            SpecShape::Brush { pts, ro, .. } => {
                let s = 2f32.powf(ro.max(0.0));
                pts.iter().fold((f32::MAX, 0.0f32), |(a, b), q| {
                    (a.min(q.1 / s), b.max(q.1 * s))
                })
            }
            SpecShape::Inverse { inner, .. } => inner.freq_extent(),
        }
    }

    /// Move by `dt` seconds and map every frequency through `map` (a move along the display's
    /// scale). When any frequency would leave `lo`..`hi` the frequencies stay put (time still moves).
    pub fn translate_with(&mut self, dt: f64, map: &dyn Fn(f32) -> f32, lo: f32, hi: f32) {
        let ok = |f: f32| {
            let g = map(f);
            g.is_finite() && g >= lo && g <= hi
        };
        let fits = match self {
            SpecShape::Rect { f0, f1, .. } => ok(*f0) && ok(*f1),
            SpecShape::Lasso(p) | SpecShape::Brush { pts: p, .. } => p.iter().all(|q| ok(q.1)),
            SpecShape::Inverse { .. } => true,
        };
        match self {
            SpecShape::Rect { t0, t1, f0, f1 } => {
                *t0 += dt;
                *t1 += dt;
                if fits {
                    *f0 = map(*f0);
                    *f1 = map(*f1);
                }
            }
            SpecShape::Lasso(p) | SpecShape::Brush { pts: p, .. } => {
                for (t, f) in p.iter_mut() {
                    *t += dt;
                    if fits {
                        *f = map(*f);
                    }
                }
            }
            SpecShape::Inverse { inner, .. } => inner.translate_with(dt, map, lo, hi),
        }
    }

    /// The opposite selection over the same time range.

    pub fn inverted(self, top: f32) -> SpecShape {
        match self {
            SpecShape::Inverse { inner, .. } => *inner,
            s => SpecShape::Inverse {
                inner: Box::new(s),
                top,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecOp {
    /// Change the level of the selected cells by this many dB (-inf for silence).
    Gain(f32),
    /// Replace the selected cells with the level of the sound on either side of them.
    Heal,
}

const N: usize = 2048;
const HOP: usize = 512;

/// Apply `op` to the cells of `shape` in the channels marked in `chans` (all when empty).
/// Cells outside the shape come back unchanged.
pub fn apply(d: &mut AudioData, shape: &SpecShape, chans: &[bool], op: SpecOp) {
    if shape.is_empty() || d.frames() == 0 {
        return;
    }
    let rate = d.rate.max(1) as f64;
    let (t0, t1, _, _) = shape.bounds();
    let frames = d.frames();
    let a = ((t0 * rate) as i64 - N as i64).max(0) as usize;
    let b = (((t1 * rate) as i64 + N as i64).max(0) as usize).min(frames);
    if b <= a {
        return;
    }
    let win: Vec<f32> = (0..N)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N as f32).cos())
        .collect();
    let mut planner = FftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(N);
    let inv = planner.plan_fft_inverse(N);
    let bins = N / 2 + 1;
    // frames start N before the region so every sample in it is fully covered
    let start = a as i64 - N as i64;
    let n_fr = ((b as i64 - start) as usize).div_ceil(HOP) + 1;
    // which cells are selected (shared by all channels)
    let times: Vec<f64> = (0..n_fr)
        .map(|k| (start + (k * HOP) as i64 + (N / 2) as i64) as f64 / rate)
        .collect();
    let freqs: Vec<f32> = (0..bins)
        .map(|j| (j as f64 * rate / N as f64) as f32)
        .collect();
    let weights = shape.grid_weights(&times, &freqs);
    let mask: Vec<bool> = weights.iter().map(|w| *w > 0.01).collect();
    if !mask.iter().any(|m| *m) {
        return;
    }
    let gain = match op {
        SpecOp::Gain(db) if db <= -120.0 => 0.0,
        SpecOp::Gain(db) => 10f32.powf(db / 20.0),
        SpecOp::Heal => 1.0,
    };
    for (ci, ch) in d.ch.iter_mut().enumerate() {
        if !chans.is_empty() && !chans.get(ci).copied().unwrap_or(true) {
            continue;
        }
        let mut spec: Vec<Vec<Complex<f32>>> = Vec::with_capacity(n_fr);
        for k in 0..n_fr {
            let s0 = start + (k * HOP) as i64;
            let mut buf: Vec<Complex<f32>> = (0..N)
                .map(|i| {
                    let idx = s0 + i as i64;
                    let x = if idx >= 0 && (idx as usize) < ch.len() {
                        ch[idx as usize]
                    } else {
                        0.0
                    };
                    Complex::new(x * win[i], 0.0)
                })
                .collect();
            fwd.process(&mut buf);
            spec.push(buf);
        }
        match op {
            SpecOp::Gain(_) => {
                for k in 0..n_fr {
                    for j in 0..bins {
                        let w = weights[k * bins + j];
                        if w > 0.0 {
                            // a softly selected cell (brush edge, opacity) gets part of the change
                            spec[k][j] *= 1.0 + (gain - 1.0) * w;
                        }
                    }
                }
            }
            SpecOp::Heal => {
                // frames whose window still overlaps the selection carry the sound being removed;
                // widen each selected run by half a window so the reference levels come from
                // frames that are clear of it
                let pad = N / HOP / 2;
                let mut heal_mask = mask.clone();
                for j in 0..bins {
                    for k in 0..n_fr {
                        if mask[k * bins + j] {
                            for kk in k.saturating_sub(pad)..(k + pad + 1).min(n_fr) {
                                heal_mask[kk * bins + j] = true;
                            }
                        }
                    }
                }
                let mask = &heal_mask;
                for j in 0..bins {
                    let mags: Vec<f32> = (0..n_fr).map(|k| spec[k][j].norm()).collect();
                    let mut k = 0;
                    while k < n_fr {
                        if !mask[k * bins + j] {
                            k += 1;
                            continue;
                        }
                        let s = k;
                        while k < n_fr && mask[k * bins + j] {
                            k += 1;
                        }
                        let e = k; // masked run [s, e)
                        let left = if s > 0 { Some(mags[s - 1]) } else { None };
                        let right = if e < n_fr { Some(mags[e]) } else { None };
                        for (q, kk) in (s..e).enumerate() {
                            let fr = (q + 1) as f32 / (e - s + 1) as f32;
                            let target = match (left, right) {
                                (Some(l), Some(r)) => l + (r - l) * fr,
                                (Some(l), None) => l,
                                (None, Some(r)) => r,
                                (None, None) => 0.0,
                            };
                            let m = spec[kk][j].norm();
                            // inside the selection the strength follows its weight; the frames
                            // added around it are healed fully
                            let w = if weights[kk * bins + j] > 0.01 {
                                weights[kk * bins + j]
                            } else {
                                1.0
                            };
                            let healed = if m > 1e-12 { target.min(m) / m } else { 0.0 };
                            spec[kk][j] *= 1.0 + (healed - 1.0) * w;
                        }
                    }
                }
            }
        }
        // keep the spectrum Hermitian so the result is real
        let mut acc = vec![0.0f32; (n_fr - 1) * HOP + N];
        let mut wsum = vec![0.0f32; acc.len()];
        for (k, s) in spec.iter_mut().enumerate() {
            for j in 1..N / 2 {
                s[N - j] = s[j].conj();
            }
            inv.process(s);
            for i in 0..N {
                acc[k * HOP + i] += s[i].re / N as f32 * win[i];
                wsum[k * HOP + i] += win[i] * win[i];
            }
        }
        for idx in a..b {
            let o = (idx as i64 - start) as usize;
            if o < acc.len() && wsum[o] > 1e-3 {
                ch[idx] = acc[o] / wsum[o];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::{self, Wave};

    fn level(d: &AudioData, a: usize, b: usize) -> f32 {
        (d.ch[0][a..b].iter().map(|x| x * x).sum::<f32>() / (b - a) as f32).sqrt()
    }

    #[test]
    fn shapes_contain_what_they_cover() {
        let r = SpecShape::Rect {
            t0: 1.0,
            t1: 2.0,
            f0: 100.0,
            f1: 1000.0,
        };
        assert!(r.contains(1.5, 500.0) && !r.contains(0.5, 500.0) && !r.contains(1.5, 2000.0));
        let l = SpecShape::Lasso(vec![
            (0.0, 100.0),
            (1.0, 100.0),
            (1.0, 1000.0),
            (0.0, 1000.0),
        ]);
        assert!(l.contains(0.5, 300.0) && !l.contains(1.5, 300.0));
        let b = SpecShape::Brush {
            pts: vec![(1.0, 1000.0)],
            rt: 0.1,
            ro: 0.5,
            opacity: 1.0,
        };
        assert!(b.contains(1.05, 1100.0) && !b.contains(1.3, 1000.0));
        let mut m = r.clone();
        // frequencies move by Hz, so the span in Hz stays the same
        m.translate(1.0, 500.0);
        assert_eq!(m.bounds(), (2.0, 3.0, 600.0, 1500.0));
        // the inverse covers the same time range but the other frequencies
        let inv = r.clone().inverted(24000.0);
        assert!(
            inv.contains(1.5, 5000.0) && !inv.contains(1.5, 500.0) && !inv.contains(0.5, 5000.0)
        );
        assert_eq!(inv.clone().inverted(24000.0), r);
        // brush edges are soft; opacity scales the selection
        let soft = SpecShape::Brush {
            pts: vec![(1.0, 1000.0)],
            rt: 0.1,
            ro: 0.5,
            opacity: 0.5,
        };
        assert!((soft.weight(1.0, 1000.0) - 0.5).abs() < 1e-6);
        assert!(soft.weight(1.09, 1000.0) > 0.0 && soft.weight(1.09, 1000.0) < 0.5);
    }

    #[test]
    fn spectral_gain_only_touches_the_selected_band() {
        // 200 Hz + 4 kHz; silence 3-6 kHz between 0.5 and 1.5 s
        let low = dsp::gen_tone(48000, 1, 2.0, 200.0, Wave::Sine, -12.0);
        let high = dsp::gen_tone(48000, 1, 2.0, 4000.0, Wave::Sine, -12.0);
        let d0 = low.mixed(0, &high, 1.0);
        let mut d = d0.clone();
        apply(
            &mut d,
            &SpecShape::Rect {
                t0: 0.5,
                t1: 1.5,
                f0: 3000.0,
                f1: 6000.0,
            },
            &[],
            SpecOp::Gain(-200.0),
        );
        let (a, b) = (36000, 60000); // 0.75..1.25 s
        let lo = level(&low, a, b);
        assert!(
            (level(&d, a, b) - lo).abs() < lo * 0.05,
            "only the low tone is left: {} vs {}",
            level(&d, a, b),
            lo
        );
        // outside the selection nothing changes
        let diff: f32 = d.ch[0][..12000]
            .iter()
            .zip(&d0.ch[0][..12000])
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max);
        assert!(diff < 1e-4, "untouched before the selection, diff {diff}");
    }

    #[test]
    fn heal_removes_a_short_burst() {
        let mut d = dsp::gen_tone(48000, 1, 1.0, 300.0, Wave::Sine, -20.0);
        let base = level(&d, 22000, 26000);
        for i in 24000..24200 {
            d.ch[0][i] += if i % 2 == 0 { 0.8 } else { -0.8 };
        }
        assert!(level(&d, 23800, 24400) > base * 3.0);
        apply(
            &mut d,
            &SpecShape::Rect {
                t0: 0.495,
                t1: 0.51,
                f0: 1000.0,
                f1: 24000.0,
            },
            &[],
            SpecOp::Heal,
        );
        assert!(
            level(&d, 23800, 24400) < base * 1.6,
            "burst healed: {} vs {}",
            level(&d, 23800, 24400),
            base
        );
    }

    #[test]
    fn empty_or_odd_selections_do_nothing() {
        let mut d = dsp::gen_tone(8000, 2, 0.1, 300.0, Wave::Sine, -20.0);
        let d0 = d.clone();
        apply(
            &mut d,
            &SpecShape::Lasso(vec![(0.0, 10.0)]),
            &[],
            SpecOp::Gain(-200.0),
        );
        apply(
            &mut d,
            &SpecShape::Rect {
                t0: 5.0,
                t1: 6.0,
                f0: 10.0,
                f1: 100.0,
            },
            &[],
            SpecOp::Heal,
        );
        assert_eq!(d.ch, d0.ch);
    }
}
