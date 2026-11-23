//! Offline ("process") operations on a range of frames.

use crate::audio::{AudioData, undb};
use std::f32::consts::PI;

/// Gain at relative position x (0..1) from a curve of (x 0..1, value) points.
pub fn curve_at(points: &[(f32, f32)], x: f32) -> f32 {
    let mut p = points.to_vec();
    p.sort_by(|a, b| a.0.total_cmp(&b.0));
    match p.len() {
        0 => 1.0,
        1 => p[0].1,
        _ => {
            if x <= p[0].0 {
                return p[0].1;
            }
            for w in p.windows(2) {
                if x <= w[1].0 {
                    let t = (x - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
                    return w[0].1 + (w[1].1 - w[0].1) * t;
                }
            }
            p[p.len() - 1].1
        }
    }
}

/// Fade Envelope: amplitude 0..1 (x in 0..1 across the range). Gain Envelope: dB values.
pub fn envelope(d: &mut AudioData, a: usize, b: usize, points: &[(f32, f32)], db: bool) {
    let (a, b) = d.span(a, b);
    let n = (b.saturating_sub(a)).max(1) as f32;
    for c in d.ch.iter_mut() {
        for (i, s) in c[a..b].iter_mut().enumerate() {
            let v = curve_at(points, i as f32 / n);
            *s *= if db { undb(v) } else { v.clamp(0.0, 4.0) };
        }
    }
}

/// Spans of [a,b) quieter than `thr_db` for at least `min_s` seconds, as frame ranges.
pub fn quiet_spans(
    d: &AudioData,
    a: usize,
    b: usize,
    thr_db: f32,
    min_s: f32,
) -> Vec<(usize, usize)> {
    let (a, b) = d.span(a, b);
    let win = (d.rate as usize / 100).max(1);
    let thr = undb(thr_db);
    let min = (min_s * d.rate as f32) as usize;
    let mut out = vec![];
    let mut start: Option<usize> = None;
    let mut pos = a;
    while pos < b {
        let e = (pos + win).min(b);
        let rms = (d
            .ch
            .iter()
            .map(|c| c[pos..e].iter().map(|v| v * v).sum::<f32>())
            .sum::<f32>()
            / ((e - pos) * d.channels()).max(1) as f32)
            .sqrt();
        match (rms < thr, start) {
            (true, None) => start = Some(pos),
            (false, Some(s)) => {
                if pos - s >= min {
                    out.push((s, pos));
                }
                start = None;
            }
            _ => {}
        }
        pos = e;
    }
    if let Some(s) = start {
        if b - s >= min {
            out.push((s, b));
        }
    }
    out
}

/// Delete Silence: remove quiet spans, keeping `keep_s` of each so the cuts don't sound abrupt.
pub fn delete_silence(
    d: &AudioData,
    a: usize,
    b: usize,
    thr_db: f32,
    min_s: f32,
    keep_s: f32,
) -> (AudioData, usize) {
    let spans = quiet_spans(d, a, b, thr_db, min_s);
    let keep = (keep_s * d.rate as f32) as usize;
    let mut out = d.clone();
    let mut removed = 0;
    for (s, e) in spans.iter().rev() {
        let half = keep / 2;
        let cs = (s + half).min(*e);
        let ce = e.saturating_sub(half).max(cs);
        if ce > cs {
            out = out.without(cs, ce);
            removed += ce - cs;
        }
    }
    (out, removed)
}

/// Mark Audio: start times (seconds) of sound after quiet spans.
pub fn mark_audio(d: &AudioData, a: usize, b: usize, thr_db: f32, min_s: f32) -> Vec<f64> {
    let spans = quiet_spans(d, a, b, thr_db, min_s);
    let mut marks = vec![];
    let r = d.rate as f64;
    if spans.first().map(|s| s.0 > a).unwrap_or(true) {
        marks.push(a as f64 / r);
    }
    for (_, e) in spans {
        if e < b.min(d.frames()) {
            marks.push(e as f64 / r);
        }
    }
    marks
}

/// Automatic Phase Correction: find the time offset between L and R (up to `max_ms`) by cross
/// correlation and shift the right channel to line up. Returns the shift in samples.
pub fn phase_correct(d: &mut AudioData, a: usize, b: usize, max_ms: f32) -> isize {
    if d.channels() < 2 {
        return 0;
    }
    let (a, b) = d.span(a, b);
    let maxlag = ((max_ms / 1000.0) * d.rate as f32) as isize;
    let len = (b - a).min(d.rate as usize * 4);
    let (l, r) = (&d.ch[0][a..a + len], &d.ch[1][a..a + len]);
    let mut best = (0isize, f32::MIN);
    for lag in -maxlag..=maxlag {
        let mut s = 0.0f32;
        let mut i = maxlag.max(0) as usize;
        while i + (maxlag as usize) < len {
            let j = (i as isize + lag) as usize;
            s += l[i] * r[j];
            i += 2;
        }
        if s > best.1 {
            best = (lag, s);
        }
    }
    let lag = best.0;
    if lag != 0 {
        let src = d.ch[1].clone();
        for i in a..b {
            let j = i as isize + lag;
            d.ch[1][i] = if j >= 0 && (j as usize) < src.len() {
                src[j as usize]
            } else {
                0.0
            };
        }
    }
    lag
}

/// Pitch Bender: pitch (and speed) moves from `start_st` to `end_st` semitones across the range.
pub fn pitch_bend(d: &AudioData, a: usize, b: usize, start_st: f32, end_st: f32) -> AudioData {
    let (a, b) = d.span(a, b);
    let seg = d.slice(a, b);
    let n = seg.frames();
    if n < 2 {
        return d.clone();
    }
    let mut out = AudioData::new(d.rate, d.channels(), 0);
    let mut pos = 0.0f64;
    while pos < (n - 1) as f64 {
        let f = (pos / n as f64) as f32;
        let ratio = 2f64.powf(((start_st + (end_st - start_st) * f) / 12.0) as f64);
        let i = pos as usize;
        let fr = (pos - i as f64) as f32;
        for (oc, sc) in out.ch.iter_mut().zip(seg.ch.iter()) {
            let v = sc[i] * (1.0 - fr) + sc[(i + 1).min(n - 1)] * fr;
            oc.push(v);
        }
        pos += ratio;
    }
    d.without(a, b).inserted(a, &out)
}

/// DTMF tones for a dial string (0-9, *, #, A-D; ',' is a pause).
pub fn dtmf(
    rate: u32,
    channels: usize,
    digits: &str,
    tone_ms: f32,
    pause_ms: f32,
    db: f32,
) -> AudioData {
    let rows = [697.0, 770.0, 852.0, 941.0];
    let cols = [1209.0, 1336.0, 1477.0, 1633.0];
    let key = |c: char| -> Option<(f32, f32)> {
        let layout = ["123A", "456B", "789C", "*0#D"];
        for (ri, row) in layout.iter().enumerate() {
            if let Some(ci) = row.find(c.to_ascii_uppercase()) {
                return Some((rows[ri], cols[ci]));
            }
        }
        None
    };
    let amp = undb(db) * 0.5;
    let tn = (tone_ms.max(10.0) / 1000.0 * rate as f32) as usize;
    let pn = (pause_ms.max(0.0) / 1000.0 * rate as f32) as usize;
    let mut v: Vec<f32> = vec![];
    for c in digits.chars() {
        match key(c) {
            Some((f1, f2)) => {
                for i in 0..tn {
                    let t = i as f32 / rate as f32;
                    let edge = ((i.min(tn - i) as f32) / (rate as f32 * 0.003)).min(1.0);
                    v.push(((2.0 * PI * f1 * t).sin() + (2.0 * PI * f2 * t).sin()) * amp * edge);
                }
                v.extend(std::iter::repeat(0.0).take(pn));
            }
            None if c == ',' => v.extend(std::iter::repeat(0.0).take(pn * 3 + tn)),
            None => {}
        }
    }
    AudioData {
        rate,
        ch: vec![v; channels.max(1)],
    }
}

/// Auto Heal: rebuild a short selection from the audio around it (crossfading a copy of what
/// precedes it into a copy of what follows), removing clicks, pops and short noises.
pub fn auto_heal(d: &mut AudioData, a: usize, b: usize) -> bool {
    let n = d.frames();
    let b = b.min(n);
    let len = b.saturating_sub(a);
    if len == 0 || a < len || b + len > n {
        return false;
    }
    for c in d.ch.iter_mut() {
        let before: Vec<f32> = c[a - len..a].to_vec();
        let after: Vec<f32> = c[b..b + len].to_vec();
        for i in 0..len {
            let t = (i as f32 + 0.5) / len as f32;
            let g = 0.5 - 0.5 * (PI * t).cos();
            c[a + i] = before[i] * (1.0 - g) + after[i] * g;
        }
        // smooth the joins
        for k in 0..8.min(len / 2) {
            let w = k as f32 / 8.0;
            c[a + k] = c[a + k] * w + c[a - 1] * (1.0 - w);
            c[b - 1 - k] = c[b - 1 - k] * w + c[b] * (1.0 - w);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_silence_and_mark_audio() {
        let r = 48000;
        let mut d = AudioData::new(r, 1, r as usize * 3);
        for i in 0..r as usize {
            d.ch[0][i] = 0.5 * (i as f32 * 0.05).sin();
            d.ch[0][2 * r as usize + i] = 0.5 * (i as f32 * 0.05).sin();
        }
        let (o, removed) = delete_silence(&d, 0, d.frames(), -50.0, 0.3, 0.1);
        assert!(removed > 40_000 && o.frames() < d.frames());
        let m = mark_audio(&d, 0, d.frames(), -50.0, 0.3);
        assert_eq!(m.len(), 2);
        assert!((m[1] - 2.0).abs() < 0.02);
    }

    #[test]
    fn envelopes_and_heal() {
        let mut d = AudioData {
            rate: 48000,
            ch: vec![vec![1.0; 1000]],
        };
        envelope(&mut d, 0, 1000, &[(0.0, 0.0), (1.0, 1.0)], false);
        assert!(d.ch[0][0] < 0.01 && d.ch[0][999] > 0.98);
        let mut g = AudioData {
            rate: 48000,
            ch: vec![vec![1.0; 1000]],
        };
        envelope(&mut g, 0, 1000, &[(0.0, -6.0), (1.0, -6.0)], true);
        assert!((g.ch[0][500] - 0.501).abs() < 0.01);
        let mut h = AudioData {
            rate: 48000,
            ch: vec![vec![0.1; 3000]],
        };
        h.ch[0][1500] = 1.0;
        assert!(auto_heal(&mut h, 1490, 1510));
        assert!(h.ch[0][1500].abs() < 0.2);
    }

    #[test]
    fn phase_correction_finds_the_offset() {
        let n = 48000;
        let src: Vec<f32> = (0..n + 100)
            .map(|i| ((i as f32 * 0.013).sin() + (i as f32 * 0.031).sin()) * 0.3)
            .collect();
        let mut d = AudioData {
            rate: 48000,
            ch: vec![src[20..20 + n].to_vec(), src[..n].to_vec()],
        };
        let lag = phase_correct(&mut d, 0, n, 2.0);
        assert_eq!(lag, 20);
    }

    #[test]
    fn dtmf_and_bend_lengths() {
        let t = dtmf(8000, 1, "12#", 100.0, 50.0, -6.0);
        assert_eq!(t.frames(), 3 * (800 + 400));
        let d = AudioData {
            rate: 8000,
            ch: vec![vec![0.1; 8000]],
        };
        let up = pitch_bend(&d, 0, 8000, 12.0, 12.0);
        assert!((up.frames() as i64 - 4000).abs() < 5);
    }
}
