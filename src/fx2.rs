//! More effects: modulation, delays, dynamics, filters, reverbs, special, stereo imagery and
//! restoration. All are original textbook DSP; every processor is realtime-safe (no allocation in
//! steady state beyond first use) and never panics on odd parameter values.

use crate::audio::undb;
use crate::dsp::{Biquad, BqState, FilterKind, Freeverb, Processor};
use std::f32::consts::PI;

fn ensure<T: Clone>(v: &mut Vec<T>, n: usize, init: T) {
    if v.len() < n {
        v.resize(n, init);
    }
}

fn frames(buf: &[Vec<f32>]) -> usize {
    buf.first().map(|c| c.len()).unwrap_or(0)
}

fn coef(ms: f32, rate: f32) -> f32 {
    (-1.0 / ((ms.max(0.01) / 1000.0) * rate)).exp()
}

// ---------------------------------------------------------------- modulation

/// Swept all-pass phaser.
pub struct Phaser {
    rate: f32,
    lfo: f32,
    inc: f32,
    stages: usize,
    depth: f32,
    fb: f32,
    mix: f32,
    lo: f32,
    hi: f32,
    state: Vec<(Vec<f32>, f32)>,
}

impl Phaser {
    pub fn new(rate: f32, stages: u32, lfo_hz: f32, depth: f32, feedback: f32, mix: f32) -> Self {
        Phaser {
            rate,
            lfo: 0.0,
            inc: 2.0 * PI * lfo_hz.max(0.01) / rate,
            stages: (stages.clamp(2, 12) as usize) & !1,
            depth: depth.clamp(0.0, 1.0),
            fb: feedback.clamp(-0.95, 0.95),
            mix: mix.clamp(0.0, 1.0),
            lo: 200.0,
            hi: 4000.0,
            state: vec![],
        }
    }
}

impl Processor for Phaser {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Phaser>() else {
            return false;
        };
        if f.stages != self.stages {
            return false;
        }
        self.inc = f.inc;
        self.depth = f.depth;
        self.fb = f.fb;
        self.mix = f.mix;
        self.lo = f.lo;
        self.hi = f.hi;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = frames(buf);
        ensure(&mut self.state, buf.len(), (vec![0.0; 12], 0.0));
        for i in 0..n {
            for (ci, c) in buf.iter_mut().enumerate() {
                let ph = self.lfo + ci as f32 * PI * 0.5;
                let sweep = 0.5 + 0.5 * ph.sin() * self.depth;
                let f = self.lo * (self.hi / self.lo).powf(sweep);
                let t = (PI * f / self.rate).tan();
                let a = (t - 1.0) / (t + 1.0);
                let (z, last) = &mut self.state[ci];
                let x = c[i];
                let mut y = x + *last * self.fb;
                for s in z.iter_mut().take(self.stages) {
                    let out = a * y + *s;
                    *s = y - a * out;
                    y = out;
                }
                *last = y;
                c[i] = x * (1.0 - self.mix * 0.5) + y * self.mix * 0.5 * 2.0 * 0.5;
            }
            self.lfo = (self.lfo + self.inc) % (2.0 * PI);
        }
    }
}

// ---------------------------------------------------------------- delays

/// Delay with a darkening, saturating feedback path (tape/analog character) and stereo spread.
pub struct AnalogDelay {
    len: [usize; 2],
    fb: f32,
    mix: f32,
    drive: f32,
    lines: Vec<Vec<f32>>,
    pos: Vec<usize>,
    lp: Vec<BqState>,
    filt: Biquad,
}

impl AnalogDelay {
    pub fn new(
        rate: f32,
        time_ms: f32,
        feedback: f32,
        drive: f32,
        spread_ms: f32,
        mix: f32,
        tone_hz: f32,
    ) -> Self {
        let l = ((time_ms.max(1.0) / 1000.0) * rate) as usize;
        let r = (((time_ms + spread_ms).max(1.0) / 1000.0) * rate) as usize;
        AnalogDelay {
            len: [l.max(1), r.max(1)],
            fb: feedback.clamp(0.0, 0.97),
            mix: mix.clamp(0.0, 1.0),
            drive: drive.max(0.0),
            lines: vec![],
            pos: vec![],
            lp: vec![],
            filt: Biquad::new(FilterKind::LowPass, rate, tone_hz.max(200.0), 0.707, 0.0),
        }
    }
}

impl Processor for AnalogDelay {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<AnalogDelay>() else {
            return false;
        };
        self.fb = f.fb;
        self.mix = f.mix;
        self.drive = f.drive;
        self.filt = f.filt;
        if f.len != self.len {
            self.len = f.len;
            for (ci, l) in self.lines.iter_mut().enumerate() {
                l.resize(self.len[ci % 2].max(1), 0.0);
                if let Some(p) = self.pos.get_mut(ci) {
                    *p %= l.len();
                }
            }
        }
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = frames(buf);
        for ci in self.lines.len()..buf.len() {
            self.lines.push(vec![0.0; self.len[ci % 2]]);
        }
        ensure(&mut self.pos, buf.len(), 0);
        ensure(&mut self.lp, buf.len(), BqState::default());
        let k = 1.0 + self.drive * 4.0;
        let norm = 1.0 / k.tanh().max(1e-3);
        for (ci, c) in buf.iter_mut().enumerate() {
            let line = &mut self.lines[ci];
            let len = line.len();
            for s in c.iter_mut().take(n) {
                let p = self.pos[ci];
                let d = line[p];
                let fed = self.lp[ci].run(&self.filt, *s + d * self.fb);
                line[p] = if self.drive > 0.0 {
                    (fed * k).tanh() * norm
                } else {
                    fed
                };
                *s = *s * (1.0 - self.mix * 0.5) + d * self.mix;
                self.pos[ci] = (p + 1) % len;
            }
        }
    }
}

// ---------------------------------------------------------------- dynamics

fn env_follow(env: &mut f32, x: f32, att: f32, rel: f32) {
    let k = if x > *env { att } else { rel };
    *env = x + k * (*env - x);
}

/// De-esser: detects energy in the sibilance band and turns that band down.
pub struct DeEsser {
    det: Biquad,
    split_lo: Biquad,
    thr: f32,
    max_red: f32,
    att: f32,
    rel: f32,
    env: f32,
    st: Vec<(BqState, BqState)>,
}

impl DeEsser {
    pub fn new(rate: f32, freq: f32, threshold_db: f32, reduction_db: f32) -> Self {
        DeEsser {
            det: Biquad::new(
                FilterKind::BandPass,
                rate,
                freq.clamp(2000.0, rate * 0.45),
                1.2,
                0.0,
            ),
            split_lo: Biquad::new(
                FilterKind::LowPass,
                rate,
                (freq * 0.75).clamp(1500.0, rate * 0.45),
                0.707,
                0.0,
            ),
            thr: undb(threshold_db),
            max_red: undb(-reduction_db.abs()),
            att: coef(1.0, rate),
            rel: coef(60.0, rate),
            env: 0.0,
            st: vec![],
        }
    }
}

impl Processor for DeEsser {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<DeEsser>() else {
            return false;
        };
        self.det = f.det;
        self.split_lo = f.split_lo;
        self.thr = f.thr;
        self.max_red = f.max_red;
        self.att = f.att;
        self.rel = f.rel;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = frames(buf);
        ensure(
            &mut self.st,
            buf.len(),
            (BqState::default(), BqState::default()),
        );
        for i in 0..n {
            let mut lvl = 0.0f32;
            for (ci, c) in buf.iter().enumerate() {
                lvl = lvl.max(self.st[ci].0.run(&self.det, c[i]).abs());
            }
            env_follow(&mut self.env, lvl, self.att, self.rel);
            let g = if self.env > self.thr {
                (self.thr / self.env).max(self.max_red)
            } else {
                1.0
            };
            for (ci, c) in buf.iter_mut().enumerate() {
                let low = self.st[ci].1.run(&self.split_lo, c[i]);
                let high = c[i] - low;
                c[i] = low + high * g;
            }
        }
    }
}

/// Feed-forward compressor core shared by several effects (dB domain, peak detector).
#[derive(Clone)]
pub struct Comp {
    thr: f32,
    ratio: f32,
    att: f32,
    rel: f32,
    env: f32,
    gain: f32,
}

impl Comp {
    /// The settings of `o`, keeping this compressor's level and gain.
    fn take_settings(&mut self, o: &Comp) {
        self.thr = o.thr;
        self.ratio = o.ratio;
        self.att = o.att;
        self.rel = o.rel;
    }

    pub fn new(rate: f32, thr_db: f32, ratio: f32, att_ms: f32, rel_ms: f32) -> Self {
        Comp {
            thr: thr_db,
            ratio: ratio.max(1.0),
            att: coef(att_ms, rate),
            rel: coef(rel_ms, rate),
            env: 0.0,
            gain: 1.0,
        }
    }
    #[inline]
    pub fn gain_for(&mut self, level: f32) -> f32 {
        env_follow(&mut self.env, level, self.att, self.rel);
        let d = crate::audio::db(self.env);
        let target = if d > self.thr {
            undb((self.thr + (d - self.thr) / self.ratio) - d)
        } else {
            1.0
        };
        let k = if target < self.gain {
            self.att
        } else {
            self.rel
        };
        self.gain = target + k * (self.gain - target);
        self.gain
    }
}

/// Three-band compressor with 2nd-order crossovers (bands sum back flat when untouched).
pub struct Multiband {
    lp1: Biquad,
    lp2: Biquad,
    st: Vec<[BqState; 2]>,
    comps: [Comp; 3],
    makeup: f32,
}

impl Multiband {
    pub fn new(
        rate: f32,
        x1: f32,
        x2: f32,
        thr: [f32; 3],
        ratio: [f32; 3],
        makeup_db: f32,
    ) -> Self {
        let x1 = x1.clamp(40.0, rate * 0.4);
        let x2 = x2.clamp(x1 * 1.5, rate * 0.45);
        Multiband {
            lp1: Biquad::new(FilterKind::LowPass, rate, x1, 0.707, 0.0),
            lp2: Biquad::new(FilterKind::LowPass, rate, x2, 0.707, 0.0),
            st: vec![],
            comps: [
                Comp::new(rate, thr[0], ratio[0], 20.0, 200.0),
                Comp::new(rate, thr[1], ratio[1], 10.0, 150.0),
                Comp::new(rate, thr[2], ratio[2], 5.0, 100.0),
            ],
            makeup: undb(makeup_db),
        }
    }
}

impl Processor for Multiband {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Multiband>() else {
            return false;
        };
        self.lp1 = f.lp1;
        self.lp2 = f.lp2;
        for (c, o) in self.comps.iter_mut().zip(f.comps.iter()) {
            c.take_settings(o);
        }
        self.makeup = f.makeup;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = frames(buf);
        ensure(&mut self.st, buf.len(), [BqState::default(); 2]);
        let nc = buf.len();
        let mut bands = [[0.0f32; 3]; 8];
        for i in 0..n {
            let mut lv = [0.0f32; 3];
            for ci in 0..nc.min(8) {
                let x = buf[ci][i];
                let low = self.st[ci][0].run(&self.lp1, x);
                let lowmid = self.st[ci][1].run(&self.lp2, x);
                let b = [low, lowmid - low, x - lowmid];
                for k in 0..3 {
                    lv[k] = lv[k].max(b[k].abs());
                }
                bands[ci] = b;
            }
            let g = [
                self.comps[0].gain_for(lv[0]),
                self.comps[1].gain_for(lv[1]),
                self.comps[2].gain_for(lv[2]),
            ];
            for ci in 0..nc.min(8) {
                let b = bands[ci];
                buf[ci][i] = (b[0] * g[0] + b[1] * g[1] + b[2] * g[2]) * self.makeup;
            }
        }
    }
}

/// Dynamics processing: an expander/gate below one threshold, compression above another, then a
/// brick-wall ceiling.
pub struct DynamicsProc {
    gate_thr: f32,
    comp: Comp,
    ceiling: f32,
    env: f32,
    gg: f32,
    att: f32,
    rel: f32,
    makeup: f32,
}

impl DynamicsProc {
    pub fn new(
        rate: f32,
        gate_db: f32,
        comp_db: f32,
        ratio: f32,
        ceiling_db: f32,
        makeup_db: f32,
    ) -> Self {
        DynamicsProc {
            gate_thr: gate_db,
            comp: Comp::new(rate, comp_db, ratio, 8.0, 150.0),
            ceiling: undb(ceiling_db),
            env: 0.0,
            gg: 1.0,
            att: coef(2.0, rate),
            rel: coef(120.0, rate),
            makeup: undb(makeup_db),
        }
    }
}

impl Processor for DynamicsProc {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<DynamicsProc>() else {
            return false;
        };
        self.gate_thr = f.gate_thr;
        self.comp.take_settings(&f.comp);
        self.ceiling = f.ceiling;
        self.att = f.att;
        self.rel = f.rel;
        self.makeup = f.makeup;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = frames(buf);
        for i in 0..n {
            let lvl = buf.iter().map(|c| c[i].abs()).fold(0.0f32, f32::max);
            env_follow(&mut self.env, lvl, self.att, self.rel);
            // 1:3 downward expansion below the gate threshold
            let d = crate::audio::db(self.env);
            let target = if d < self.gate_thr {
                undb(((d - self.gate_thr) * 2.0).max(-80.0))
            } else {
                1.0
            };
            let k = if target < self.gg { self.att } else { self.rel };
            self.gg = target + k * (self.gg - target);
            let g = self.gg * self.comp.gain_for(lvl) * self.makeup;
            for c in buf.iter_mut() {
                c[i] = (c[i] * g).clamp(-self.ceiling, self.ceiling);
            }
        }
    }
}

/// Compressor followed by gentle tube-style asymmetric saturation.
pub struct TubeComp {
    comp: Comp,
    drive: f32,
    makeup: f32,
}

impl TubeComp {
    pub fn new(
        rate: f32,
        thr: f32,
        ratio: f32,
        att: f32,
        rel: f32,
        drive: f32,
        makeup_db: f32,
    ) -> Self {
        TubeComp {
            comp: Comp::new(rate, thr, ratio, att, rel),
            drive: drive.clamp(0.0, 1.0),
            makeup: undb(makeup_db),
        }
    }
}

impl Processor for TubeComp {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<TubeComp>() else {
            return false;
        };
        self.comp.take_settings(&f.comp);
        self.drive = f.drive;
        self.makeup = f.makeup;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = frames(buf);
        let k = 1.0 + self.drive * 3.0;
        for i in 0..n {
            let lvl = buf.iter().map(|c| c[i].abs()).fold(0.0f32, f32::max);
            let g = self.comp.gain_for(lvl) * self.makeup;
            for c in buf.iter_mut() {
                let x = c[i] * g;
                let y = if self.drive > 0.0 {
                    let s = x * k;
                    // asymmetric soft clip: positive side slightly harder (even harmonics)
                    let t = if s >= 0.0 {
                        s.tanh()
                    } else {
                        (s * 0.9).tanh() / 0.9
                    };
                    t / k.tanh().max(1e-3) * (1.0 - self.drive * 0.2) + x * self.drive * 0.2
                } else {
                    x
                };
                c[i] = y;
            }
        }
    }
}

/// Speech volume leveler: slow automatic gain towards a target level, frozen in pauses.
pub struct Leveler {
    target: f32,
    max_gain: f32,
    gate: f32,
    rms: f32,
    gain: f32,
    a_rms: f32,
    a_gain: f32,
}

impl Leveler {
    pub fn new(rate: f32, target_db: f32, max_gain_db: f32, gate_db: f32, speed: f32) -> Self {
        let s = speed.clamp(0.05, 1.0);
        Leveler {
            target: undb(target_db),
            max_gain: undb(max_gain_db.max(0.0)),
            gate: undb(gate_db),
            rms: 0.0,
            gain: 1.0,
            a_rms: coef(300.0, rate),
            a_gain: coef(1500.0 / s, rate),
        }
    }
}

impl Processor for Leveler {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Leveler>() else {
            return false;
        };
        self.target = f.target;
        self.max_gain = f.max_gain;
        self.gate = f.gate;
        self.a_rms = f.a_rms;
        self.a_gain = f.a_gain;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = frames(buf);
        for i in 0..n {
            let p = buf.iter().map(|c| c[i] * c[i]).sum::<f32>() / buf.len().max(1) as f32;
            self.rms = p + self.a_rms * (self.rms - p);
            let r = self.rms.sqrt();
            if r > self.gate {
                let want = (self.target / r.max(1e-6)).clamp(1.0 / self.max_gain, self.max_gain);
                self.gain = want + self.a_gain * (self.gain - want);
            }
            for c in buf.iter_mut() {
                c[i] = (c[i] * self.gain).clamp(-1.0, 1.0);
            }
        }
    }
}

// ---------------------------------------------------------------- stereo

/// 2x2 channel matrix (percent of each input in each output).
pub struct ChannelMixer {
    m: [[f32; 2]; 2],
}

impl ChannelMixer {
    pub fn new(ll: f32, lr: f32, rl: f32, rr: f32) -> Self {
        ChannelMixer {
            m: [[ll / 100.0, lr / 100.0], [rl / 100.0, rr / 100.0]],
        }
    }
}

impl Processor for ChannelMixer {
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        if buf.len() < 2 {
            return;
        }
        let n = frames(buf);
        let (a, b) = buf.split_at_mut(1);
        for i in 0..n {
            let (l, r) = (a[0][i], b[0][i]);
            a[0][i] = l * self.m[0][0] + r * self.m[0][1];
            b[0][i] = l * self.m[1][0] + r * self.m[1][1];
        }
    }
}

/// Center channel extractor: keep or remove what is identical in both channels, in a band.
pub struct CenterChannel {
    extract: bool,
    level: f32,
    bp_lo: Biquad,
    bp_hi: Biquad,
    st: [BqState; 2],
}

impl CenterChannel {
    pub fn new(rate: f32, extract: bool, level_db: f32, lo_hz: f32, hi_hz: f32) -> Self {
        CenterChannel {
            extract,
            level: undb(level_db),
            bp_lo: Biquad::new(
                FilterKind::HighPass,
                rate,
                lo_hz.clamp(20.0, rate * 0.4),
                0.707,
                0.0,
            ),
            bp_hi: Biquad::new(
                FilterKind::LowPass,
                rate,
                hi_hz.clamp(lo_hz + 50.0, rate * 0.45),
                0.707,
                0.0,
            ),
            st: [BqState::default(); 2],
        }
    }
}

impl Processor for CenterChannel {
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        if buf.len() < 2 {
            return;
        }
        let n = frames(buf);
        let (a, b) = buf.split_at_mut(1);
        for i in 0..n {
            let (l, r) = (a[0][i], b[0][i]);
            let mid = (l + r) * 0.5;
            let hp = self.st[0].run(&self.bp_lo, mid);
            let band = self.st[1].run(&self.bp_hi, hp);
            if self.extract {
                let side = (l - r) * 0.5;
                // keep the centre band, attenuate everything else
                let keep = band * self.level;
                a[0][i] = keep + (mid - band + side) * 0.1;
                b[0][i] = keep + (mid - band - side) * 0.1;
            } else {
                // remove (or turn down) the centre band
                let cut = band * (1.0 - self.level.min(1.0));
                a[0][i] = l - cut;
                b[0][i] = r - cut;
            }
        }
    }
}

/// Phase shifter: a first-order all-pass per channel tuned so the shift at `freq` is `deg`.
pub struct PhaseShift {
    a: [f32; 2],
    z: [(f32, f32); 2],
}

impl PhaseShift {
    pub fn new(rate: f32, left_deg: f32, right_deg: f32, freq: f32) -> Self {
        let coef_for = |deg: f32| {
            let phi = (deg.clamp(-179.0, 179.0)).to_radians();
            let w = 2.0 * PI * freq.clamp(20.0, rate * 0.45) / rate;
            // first-order all-pass phase: -2*atan(((1-a)/(1+a))*tan(w/2)); solve for a
            let t = (w / 2.0).tan();
            let k = (-phi / 2.0).tan() / t.max(1e-6);
            ((1.0 - k) / (1.0 + k)).clamp(-0.999, 0.999)
        };
        PhaseShift {
            a: [coef_for(left_deg), coef_for(right_deg)],
            z: [(0.0, 0.0); 2],
        }
    }
}

impl Processor for PhaseShift {
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        for (ci, c) in buf.iter_mut().enumerate().take(2) {
            let a = self.a[ci];
            if a.abs() < 1e-6 && ci < 2 {
                continue;
            }
            let (mut x1, mut y1) = self.z[ci];
            for s in c.iter_mut() {
                let y = a * *s + x1 - a * y1;
                x1 = *s;
                y1 = y;
                *s = y;
            }
            self.z[ci] = (x1, y1);
        }
    }
}

// ---------------------------------------------------------------- filters

/// Bank of biquads applied in series (graphic EQ, notch filter, scientific filter).
pub struct Bank {
    f: Vec<Biquad>,
    st: Vec<Vec<BqState>>,
    gain: f32,
}

impl Bank {
    pub fn new(f: Vec<Biquad>, gain_db: f32) -> Self {
        Bank {
            f,
            st: vec![],
            gain: undb(gain_db),
        }
    }
    /// The filters (for drawing a response graph) and the output gain in dB.
    pub fn parts(&self) -> (&[Biquad], f32) {
        (&self.f, crate::audio::db(self.gain))
    }
}

impl Processor for Bank {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Bank>() else {
            return false;
        };
        if f.f.len() != self.f.len() {
            return false;
        }
        self.f.clone_from(&f.f);
        self.gain = f.gain;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        ensure(
            &mut self.st,
            buf.len(),
            vec![BqState::default(); self.f.len()],
        );
        for (ci, c) in buf.iter_mut().enumerate() {
            let st = &mut self.st[ci];
            for s in c.iter_mut() {
                let mut x = *s;
                for (f, z) in self.f.iter().zip(st.iter_mut()) {
                    x = z.run(f, x);
                }
                *s = x * self.gain;
            }
        }
    }
}

/// Graphic EQ centre frequencies for 10, 20 or 30 bands (octave, 1/2 and 1/3 octave).
pub fn graphic_freqs(bands: usize) -> Vec<f32> {
    let (n, step) = match bands {
        30 => (30, 1.0 / 3.0),
        20 => (20, 0.5),
        _ => (10, 1.0),
    };
    (0..n)
        .map(|i| 31.25 * 2f32.powf(i as f32 * step))
        .filter(|f| *f < 20_500.0)
        .collect()
}

pub fn graphic_bank(rate: f32, gains: &[f32], master_db: f32) -> Bank {
    let freqs = graphic_freqs(gains.len());
    let q = match gains.len() {
        30 => 4.3,
        20 => 2.9,
        _ => 1.4,
    };
    let f = freqs
        .iter()
        .zip(gains.iter())
        .filter(|(fr, g)| g.abs() > 0.01 && **fr < rate * 0.45)
        .map(|(fr, g)| Biquad::new(FilterKind::Peak, rate, *fr, q, *g))
        .collect();
    Bank::new(f, master_db)
}

/// Butterworth Q values for an even order (cascade of 2nd-order sections).
fn butterworth_qs(order: u32) -> Vec<f32> {
    let n = (order.clamp(2, 16) / 2 * 2) as usize;
    (0..n / 2)
        .map(|k| 1.0 / (2.0 * ((2 * k + 1) as f32 * PI / (2 * n) as f32).sin()))
        .collect()
}

/// Scientific filter: Butterworth low/high/band pass or band stop of a given order.
pub fn scientific_bank(rate: f32, kind: u8, order: u32, f1: f32, f2: f32, gain_db: f32) -> Bank {
    let qs = butterworth_qs(order);
    let f1 = f1.clamp(10.0, rate * 0.45);
    let f2 = f2.clamp(f1 + 10.0, rate * 0.46);
    let mut f = vec![];
    for q in &qs {
        match kind {
            0 => f.push(Biquad::new(FilterKind::LowPass, rate, f1, *q, 0.0)),
            1 => f.push(Biquad::new(FilterKind::HighPass, rate, f1, *q, 0.0)),
            2 => {
                f.push(Biquad::new(FilterKind::HighPass, rate, f1, *q, 0.0));
                f.push(Biquad::new(FilterKind::LowPass, rate, f2, *q, 0.0));
            }
            _ => {}
        }
    }
    if kind == 3 {
        // band stop: notches spread across the band
        let centre = (f1 * f2).sqrt();
        let q = centre / (f2 - f1).max(1.0);
        for _ in 0..(qs.len().max(1)) {
            f.push(Biquad::new(
                FilterKind::Notch,
                rate,
                centre,
                q.max(0.1),
                0.0,
            ));
        }
    }
    Bank::new(f, gain_db)
}

/// Linear-phase FIR filter designed from a drawn response (frequency-sampling method).
pub struct Fir {
    taps: Vec<f32>,
    hist: Vec<Vec<f32>>,
    pos: Vec<usize>,
}

impl Fir {
    /// `points`: (frequency Hz, gain dB), interpolated on a log-frequency axis.
    pub fn design(rate: f32, points: &[(f32, f32)], len: usize) -> Fir {
        let n = len.clamp(64, 4096) | 1;
        let half = n / 2;
        let gain_at = |f: f32| -> f32 {
            if points.is_empty() {
                return 1.0;
            }
            let mut p = points.to_vec();
            p.sort_by(|a, b| a.0.total_cmp(&b.0));
            if f <= p[0].0 {
                return undb(p[0].1);
            }
            for w in p.windows(2) {
                if f <= w[1].0 {
                    let t = ((f.max(1.0)).ln() - w[0].0.max(1.0).ln())
                        / (w[1].0.max(1.0).ln() - w[0].0.max(1.0).ln()).max(1e-6);
                    return undb(w[0].1 + (w[1].1 - w[0].1) * t.clamp(0.0, 1.0));
                }
            }
            undb(p[p.len() - 1].1)
        };
        let m = 512;
        let mags: Vec<f32> = (0..=m)
            .map(|k| gain_at(k as f32 / m as f32 * rate * 0.5))
            .collect();
        let mut taps = vec![0.0f32; n];
        for (i, t) in taps.iter_mut().enumerate() {
            let x = i as f32 - half as f32;
            let mut acc = mags[0];
            for k in 1..m {
                acc += 2.0 * mags[k] * (PI * k as f32 * x / m as f32).cos();
            }
            acc += mags[m] * (PI * x).cos();
            let w = 0.42 - 0.5 * (2.0 * PI * i as f32 / (n - 1) as f32).cos()
                + 0.08 * (4.0 * PI * i as f32 / (n - 1) as f32).cos();
            *t = acc / (2.0 * m as f32) * w;
        }
        Fir {
            taps,
            hist: vec![],
            pos: vec![],
        }
    }
}

impl Processor for Fir {
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = self.taps.len();
        ensure(&mut self.hist, buf.len(), vec![0.0; n]);
        ensure(&mut self.pos, buf.len(), 0);
        for (ci, c) in buf.iter_mut().enumerate() {
            let h = &mut self.hist[ci];
            let mut p = self.pos[ci];
            for s in c.iter_mut() {
                h[p] = *s;
                let mut acc = 0.0f32;
                let mut idx = p;
                for t in self.taps.iter() {
                    acc += t * h[idx];
                    idx = if idx == 0 { n - 1 } else { idx - 1 };
                }
                *s = acc;
                p = (p + 1) % n;
            }
            self.pos[ci] = p;
        }
    }
    fn tail(&self) -> usize {
        self.taps.len()
    }
}

// ---------------------------------------------------------------- reverbs

/// Studio reverb: pre-delay, early reflections and a damped diffuse tail.
pub struct StudioReverb {
    pre: Vec<Vec<f32>>,
    pre_len: usize,
    pos: usize,
    taps: Vec<(usize, f32)>,
    early: f32,
    tail: Freeverb,
    wet: f32,
    dry: f32,
    scratch: Vec<Vec<f32>>,
}

impl StudioReverb {
    pub fn new(
        rate: f32,
        decay: f32,
        predelay_ms: f32,
        damping: f32,
        width: f32,
        early: f32,
        dry: f32,
        wet: f32,
    ) -> Self {
        let pre_len = ((predelay_ms.clamp(0.0, 500.0) / 1000.0) * rate) as usize + 1;
        let taps = [7.0, 11.0, 17.0, 23.0, 31.0, 41.0]
            .iter()
            .enumerate()
            .map(|(i, ms)| {
                (
                    ((ms * (1.0 + decay * 0.5) / 1000.0) * rate) as usize,
                    0.7f32.powi(i as i32 + 1),
                )
            })
            .collect();
        StudioReverb {
            pre: vec![],
            pre_len: pre_len + 2400,
            pos: 0,
            taps,
            early: early.clamp(0.0, 1.0),
            tail: Freeverb::new(
                rate,
                decay.clamp(0.0, 1.0),
                damping.clamp(0.0, 1.0),
                width.clamp(0.0, 1.0),
                1.0,
            ),
            wet: wet.clamp(0.0, 1.0),
            dry: dry.clamp(0.0, 1.0),
            scratch: vec![],
        }
        .with_pre(pre_len)
    }
    fn with_pre(mut self, pre_len: usize) -> Self {
        self.taps.insert(0, (pre_len, 1.0));
        self
    }
}

impl Processor for StudioReverb {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<StudioReverb>() else {
            return false;
        };
        if f.pre_len != self.pre_len || f.taps.len() != self.taps.len() {
            return false;
        }
        self.taps.clone_from(&f.taps);
        self.early = f.early;
        self.wet = f.wet;
        self.dry = f.dry;
        self.tail.retune(&f.tail)
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = frames(buf);
        let len = self.pre_len + 4000;
        ensure(&mut self.pre, buf.len(), vec![0.0; len]);
        self.scratch.resize(buf.len(), vec![]);
        let base = self.pos;
        for (ci, c) in buf.iter().enumerate() {
            let line = &mut self.pre[ci];
            let s = &mut self.scratch[ci];
            s.clear();
            let mut p = base;
            for x in c.iter().take(n) {
                line[p] = *x;
                // pre-delayed signal feeds the tail; early reflections are added on top
                let rd = |d: usize| line[(p + len - d.min(len - 1)) % len];
                let mut e = 0.0;
                for (d, g) in self.taps.iter().skip(1) {
                    e += rd(*d + self.taps[0].0) * g;
                }
                s.push(rd(self.taps[0].0) + e * self.early * 0.3);
                p = (p + 1) % len;
            }
        }
        self.pos = (base + n) % len;
        let mut wet = std::mem::take(&mut self.scratch);
        self.tail.process(&mut wet);
        for (c, w) in buf.iter_mut().zip(wet.iter()) {
            for (x, y) in c.iter_mut().zip(w.iter()) {
                *x = *x * self.dry + y * self.wet * 2.0;
            }
        }
        self.scratch = wet;
    }
    fn tail(&self) -> usize {
        96_000
    }
}

/// Synthetic impulse responses for the convolution reverb (decaying filtered noise).
pub fn synthetic_ir(rate: f32, kind: u8) -> Vec<Vec<f32>> {
    let (secs, bright) = match kind {
        0 => (0.5, 0.6),  // small room
        1 => (1.6, 0.5),  // medium hall
        2 => (3.5, 0.45), // large hall
        3 => (2.2, 0.8),  // plate
        _ => (6.0, 0.35), // cathedral
    };
    let n = (secs * rate) as usize;
    let mut seed = 0x2545F4914F6CDD1Du64.wrapping_add(kind as u64);
    let mut out = vec![vec![0.0f32; n]; 2];
    for ch in out.iter_mut() {
        let mut lp = 0.0f32;
        for (i, s) in ch.iter_mut().enumerate() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let w = ((seed >> 11) as f64 / (1u64 << 53) as f64) as f32 * 2.0 - 1.0;
            let t = i as f32 / rate;
            // darker as it decays
            let a = bright * (-t * 1.5).exp() + 0.05;
            lp += a * (w - lp);
            *s = lp * (-6.9 * t / secs).exp();
        }
    }
    // normalise energy
    let e: f32 = out
        .iter()
        .flat_map(|c| c.iter())
        .map(|v| v * v)
        .sum::<f32>()
        .sqrt()
        .max(1e-6);
    for c in out.iter_mut() {
        for s in c.iter_mut() {
            *s /= e;
        }
    }
    out
}

/// FFT convolution reverb (overlap-add with a fixed input block). Adds `block` samples latency.
pub struct Convolver {
    ir_f: Vec<Vec<rustfft::num_complex::Complex<f32>>>,
    nfft: usize,
    block: usize,
    fwd: std::sync::Arc<dyn rustfft::Fft<f32>>,
    inv: std::sync::Arc<dyn rustfft::Fft<f32>>,
    inbuf: Vec<Vec<f32>>,
    overlap: Vec<Vec<f32>>,
    outq: Vec<std::collections::VecDeque<f32>>,
    mix: f32,
    dryq: Vec<std::collections::VecDeque<f32>>,
    work: Vec<rustfft::num_complex::Complex<f32>>,
}

impl Convolver {
    pub fn new(ir: Vec<Vec<f32>>, mix: f32) -> Self {
        use rustfft::num_complex::Complex;
        let block = 4096usize;
        let irlen = ir.iter().map(|c| c.len()).max().unwrap_or(1).max(1);
        let nfft = (block + irlen).next_power_of_two();
        let mut planner = rustfft::FftPlanner::new();
        let fwd = planner.plan_fft_forward(nfft);
        let inv = planner.plan_fft_inverse(nfft);
        let ir_f = ir
            .iter()
            .map(|c| {
                let mut v: Vec<Complex<f32>> = c.iter().map(|x| Complex::new(*x, 0.0)).collect();
                v.resize(nfft, Complex::new(0.0, 0.0));
                fwd.process(&mut v);
                v
            })
            .collect();
        Convolver {
            ir_f,
            nfft,
            block,
            fwd,
            inv,
            inbuf: vec![],
            overlap: vec![],
            outq: vec![],
            mix: mix.clamp(0.0, 1.0),
            dryq: vec![],
            work: vec![],
        }
    }
}

impl Processor for Convolver {
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        use rustfft::num_complex::Complex;
        if self.ir_f.is_empty() {
            return;
        }
        let nc = buf.len();
        ensure(&mut self.inbuf, nc, vec![]);
        ensure(&mut self.overlap, nc, vec![0.0; self.nfft]);
        ensure(&mut self.outq, nc, std::collections::VecDeque::new());
        ensure(&mut self.dryq, nc, std::collections::VecDeque::new());
        for ci in 0..nc {
            if self.outq[ci].is_empty() && self.dryq[ci].is_empty() {
                // prime the latency so output never runs dry
                self.outq[ci].extend(std::iter::repeat(0.0).take(self.block));
                self.dryq[ci].extend(std::iter::repeat(0.0).take(self.block));
            }
            let irc = ci % self.ir_f.len();
            for s in buf[ci].iter_mut() {
                self.inbuf[ci].push(*s);
                self.dryq[ci].push_back(*s);
                if self.inbuf[ci].len() == self.block {
                    self.work.clear();
                    self.work
                        .extend(self.inbuf[ci].iter().map(|x| Complex::new(*x, 0.0)));
                    self.work.resize(self.nfft, Complex::new(0.0, 0.0));
                    self.fwd.process(&mut self.work);
                    for (w, h) in self.work.iter_mut().zip(self.ir_f[irc].iter()) {
                        *w *= *h;
                    }
                    self.inv.process(&mut self.work);
                    let scale = 1.0 / self.nfft as f32;
                    let ov = &mut self.overlap[ci];
                    for (o, w) in ov.iter_mut().zip(self.work.iter()) {
                        *o += w.re * scale;
                    }
                    let ready: Vec<f32> = ov.drain(0..self.block).collect();
                    ov.resize(self.nfft, 0.0);
                    self.outq[ci].extend(ready);
                    self.inbuf[ci].clear();
                }
                let wet = self.outq[ci].pop_front().unwrap_or(0.0);
                let dry = self.dryq[ci].pop_front().unwrap_or(0.0);
                *s = dry * (1.0 - self.mix) + wet * self.mix;
            }
        }
    }
    fn tail(&self) -> usize {
        self.nfft
    }
}

// ---------------------------------------------------------------- restoration (STFT)

/// Spectral processor modes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SpecMode {
    /// Tracks the noise floor continuously and subtracts it.
    Adaptive,
    /// Like Adaptive but only above `cutoff` (hiss).
    Hiss,
    /// Estimates the late reverberation from earlier frames and subtracts it.
    DeReverb,
}

/// Overlap-add STFT engine with per-bin gain rules (latency: one frame).
pub struct Spectral {
    mode: SpecMode,
    n: usize,
    hop: usize,
    win: Vec<f32>,
    fwd: std::sync::Arc<dyn rustfft::Fft<f32>>,
    inv: std::sync::Arc<dyn rustfft::Fft<f32>>,
    amount: f32,
    floor: f32,
    cut_bin: usize,
    ch: Vec<SpecCh>,
    work: Vec<rustfft::num_complex::Complex<f32>>,
}

#[derive(Clone)]
struct SpecCh {
    input: Vec<f32>,
    fill: usize,
    acc: Vec<f32>,
    outq: std::collections::VecDeque<f32>,
    noise: Vec<f32>,
    hist: Vec<f32>,
    prev_gain: Vec<f32>,
    frames: u64,
}

impl Spectral {
    pub fn new(rate: f32, mode: SpecMode, amount: f32, reduce_db: f32, cutoff_hz: f32) -> Self {
        let n = 2048;
        let mut planner = rustfft::FftPlanner::new();
        Spectral {
            mode,
            n,
            hop: n / 4,
            win: crate::dsp::hann(n),
            fwd: planner.plan_fft_forward(n),
            inv: planner.plan_fft_inverse(n),
            amount: amount.clamp(0.0, 2.0),
            floor: undb(-reduce_db.abs()),
            cut_bin: ((cutoff_hz / (rate * 0.5)) * (n / 2) as f32).clamp(0.0, (n / 2) as f32)
                as usize,
            ch: vec![],
            work: vec![],
        }
    }

    fn frame(&mut self, ci: usize) {
        use rustfft::num_complex::Complex;
        let n = self.n;
        let half = n / 2;
        let c = &mut self.ch[ci];
        self.work.clear();
        self.work.extend(
            c.input
                .iter()
                .zip(self.win.iter())
                .map(|(x, w)| Complex::new(x * w, 0.0)),
        );
        self.fwd.process(&mut self.work);
        c.frames += 1;
        let warm = c.frames < 8;
        for k in 0..=half {
            let mag = self.work[k].norm();
            let g = match self.mode {
                SpecMode::Adaptive | SpecMode::Hiss => {
                    // minimum-tracking noise estimate: falls fast, rises slowly
                    let nz = &mut c.noise[k];
                    if warm || mag < *nz {
                        *nz = if warm { mag } else { 0.7 * *nz + 0.3 * mag };
                    } else {
                        *nz *= 1.0035;
                    }
                    if self.mode == SpecMode::Hiss && k < self.cut_bin {
                        1.0
                    } else if mag > 1e-12 {
                        ((mag - *nz * self.amount * 2.0) / mag).max(self.floor)
                    } else {
                        self.floor
                    }
                }
                SpecMode::DeReverb => {
                    // late reverb ~ smoothed past energy; keep what rises above it
                    let h = &mut c.hist[k];
                    let late = *h * 0.85;
                    *h = 0.92 * *h + 0.08 * mag;
                    if mag > 1e-12 {
                        ((mag - late * self.amount) / mag).max(self.floor)
                    } else {
                        1.0
                    }
                }
            };
            let g = 0.5 * g + 0.5 * c.prev_gain[k];
            c.prev_gain[k] = g;
            self.work[k] *= g;
            if k > 0 && k < half {
                self.work[n - k] *= g;
            }
        }
        self.inv.process(&mut self.work);
        let norm = 1.0 / (n as f32 * 1.5);
        for i in 0..n {
            c.acc[i] += self.work[i].re * self.win[i] * norm;
        }
        let out: Vec<f32> = c.acc.drain(0..self.hop).collect();
        c.acc.resize(n, 0.0);
        c.outq.extend(out);
    }
}

impl Processor for Spectral {
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let n = self.n;
        let half = n / 2;
        while self.ch.len() < buf.len() {
            let mut q = std::collections::VecDeque::new();
            q.extend(std::iter::repeat(0.0).take(n));
            self.ch.push(SpecCh {
                input: vec![0.0; n],
                fill: 0,
                acc: vec![0.0; n],
                outq: q,
                noise: vec![0.0; half + 1],
                hist: vec![0.0; half + 1],
                prev_gain: vec![1.0; half + 1],
                frames: 0,
            });
        }
        for ci in 0..buf.len() {
            for i in 0..buf[ci].len() {
                let x = buf[ci][i];
                {
                    let c = &mut self.ch[ci];
                    c.input.rotate_left(1);
                    c.input[n - 1] = x;
                    c.fill += 1;
                }
                if self.ch[ci].fill >= self.hop {
                    self.ch[ci].fill = 0;
                    self.frame(ci);
                }
                buf[ci][i] = self.ch[ci].outq.pop_front().unwrap_or(0.0);
            }
        }
    }
    fn tail(&self) -> usize {
        self.n * 2
    }
}

// ---------------------------------------------------------------- clicks

/// Click/pop remover: finds samples far off a local prediction and redraws them smoothly.
pub struct ClickRemover {
    sens: f32,
    hist: Vec<Vec<f32>>,
}

impl ClickRemover {
    pub fn new(sensitivity: f32) -> Self {
        ClickRemover {
            sens: sensitivity.clamp(0.05, 1.0),
            hist: vec![],
        }
    }
}

/// Repair clicks in one channel in place. Returns how many samples were fixed.
pub fn declick(x: &mut [f32], sensitivity: f32) -> usize {
    let n = x.len();
    if n < 16 {
        return 0;
    }
    // residual of a 2nd-order linear prediction
    let mut res = vec![0.0f32; n];
    for i in 2..n {
        res[i] = x[i] - (2.0 * x[i - 1] - x[i - 2]);
    }
    let win = 256usize;
    let mut fixed = 0;
    let k = 4.0 + (1.0 - sensitivity) * 16.0;
    let mut i = 2;
    while i < n - 2 {
        let a = i.saturating_sub(win / 2);
        let b = (i + win / 2).min(n);
        let mut s = res[a..b].iter().map(|v| v.abs()).sum::<f32>() / (b - a) as f32;
        s = s.max(1e-4);
        if res[i].abs() > k * s {
            // extent of the click
            let mut j = i;
            while j < n - 2 && j - i < 48 && res[j].abs() > k * s * 0.3 {
                j += 1;
            }
            let l = i.saturating_sub(2);
            let r = (j + 2).min(n - 1);
            let (y0, y1) = (x[l], x[r]);
            for t in l + 1..r {
                let f = (t - l) as f32 / (r - l) as f32;
                let f = f * f * (3.0 - 2.0 * f);
                x[t] = y0 + (y1 - y0) * f;
            }
            fixed += r - l - 1;
            i = r + 1;
        } else {
            i += 1;
        }
    }
    fixed
}

impl Processor for ClickRemover {
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        ensure(&mut self.hist, buf.len(), vec![0.0; 64]);
        for (ci, c) in buf.iter_mut().enumerate() {
            // include a little history so clicks at block edges are found
            let mut tmp: Vec<f32> = self.hist[ci].clone();
            tmp.extend_from_slice(c);
            declick(&mut tmp, self.sens);
            let h = self.hist[ci].len();
            c.copy_from_slice(&tmp[h..]);
            let keep = tmp.len().saturating_sub(64);
            self.hist[ci] = tmp[keep..].to_vec();
        }
    }
}

// ---------------------------------------------------------------- chains of the above

/// Several processors in series (Guitar Suite, Mastering, Vocal Enhancer...).
pub struct Series(pub Vec<Box<dyn Processor>>);

impl Processor for Series {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Series>() else {
            return false;
        };
        // a chain of effects glides when every link does (same chain, new settings)
        if f.0.len() != self.0.len() {
            return false;
        }
        let mut ok = true;
        for (a, b) in self.0.iter_mut().zip(f.0.iter()) {
            ok &= a.retune(b.as_ref());
        }
        ok
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        for p in self.0.iter_mut() {
            p.process(buf);
        }
    }
    fn tail(&self) -> usize {
        self.0.iter().map(|p| p.tail()).sum()
    }
}

/// Simple drive stage (used in Guitar Suite).
pub struct Drive {
    pub k: f32,
}

impl Processor for Drive {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Drive>() else {
            return false;
        };
        self.k = f.k;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let norm = 1.0 / self.k.tanh().max(1e-3);
        for c in buf.iter_mut() {
            for s in c.iter_mut() {
                *s = (*s * self.k).tanh() * norm;
            }
        }
    }
}

/// Harmonic exciter: adds a little saturated high band.
pub struct Exciter {
    hp: Biquad,
    st: Vec<BqState>,
    amount: f32,
}

impl Exciter {
    pub fn new(rate: f32, amount: f32) -> Self {
        Exciter {
            hp: Biquad::new(
                FilterKind::HighPass,
                rate,
                3000.0_f32.min(rate * 0.4),
                0.707,
                0.0,
            ),
            st: vec![],
            amount: amount.clamp(0.0, 1.0),
        }
    }
}

impl Processor for Exciter {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Exciter>() else {
            return false;
        };
        self.hp = f.hp;
        self.amount = f.amount;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        ensure(&mut self.st, buf.len(), BqState::default());
        for (ci, c) in buf.iter_mut().enumerate() {
            for s in c.iter_mut() {
                let h = self.st[ci].run(&self.hp, *s);
                *s += (h * 4.0).tanh() * 0.25 * self.amount;
            }
        }
    }
}

/// Mid/side width control.
pub struct Widen {
    pub w: f32,
}

impl Processor for Widen {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Widen>() else {
            return false;
        };
        self.w = f.w;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        if buf.len() < 2 {
            return;
        }
        let (a, b) = buf.split_at_mut(1);
        for (l, r) in a[0].iter_mut().zip(b[0].iter_mut()) {
            let m = (*l + *r) * 0.5;
            let s = (*l - *r) * 0.5 * self.w;
            *l = m + s;
            *r = m - s;
        }
    }
}

/// Look-ahead-free loudness maximizer: gain then a soft ceiling.
pub struct Maximizer {
    pub gain: f32,
    pub ceiling: f32,
}

impl Processor for Maximizer {
    fn retune(&mut self, fresh: &dyn Processor) -> bool {
        let Some(f) = fresh.as_any().downcast_ref::<Maximizer>() else {
            return false;
        };
        self.gain = f.gain;
        self.ceiling = f.ceiling;
        true
    }
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        for c in buf.iter_mut() {
            for s in c.iter_mut() {
                let x = *s * self.gain / self.ceiling;
                *s = (if x.abs() < 0.8 {
                    x
                } else {
                    x.signum() * (0.8 + 0.2 * ((x.abs() - 0.8) / 0.2).tanh())
                }) * self.ceiling;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(n: usize, hz: f32) -> Vec<Vec<f32>> {
        vec![
            (0..n)
                .map(|i| (2.0 * PI * hz * i as f32 / 48000.0).sin() * 0.5)
                .collect();
            2
        ]
    }

    fn finite(b: &[Vec<f32>]) -> bool {
        b.iter().flat_map(|c| c.iter()).all(|v| v.is_finite())
    }

    #[test]
    fn processors_are_finite_and_bounded() {
        let r = 48000.0;
        let mut ps: Vec<Box<dyn Processor>> = vec![
            Box::new(Phaser::new(r, 8, 0.5, 0.8, 0.5, 0.5)),
            Box::new(AnalogDelay::new(r, 200.0, 0.6, 0.5, 20.0, 0.4, 3000.0)),
            Box::new(DeEsser::new(r, 6000.0, -30.0, 12.0)),
            Box::new(Multiband::new(r, 200.0, 3000.0, [-20.0; 3], [3.0; 3], 0.0)),
            Box::new(DynamicsProc::new(r, -50.0, -20.0, 4.0, -0.5, 0.0)),
            Box::new(TubeComp::new(r, -20.0, 3.0, 10.0, 100.0, 0.6, 2.0)),
            Box::new(Leveler::new(r, -20.0, 12.0, -50.0, 0.5)),
            Box::new(ChannelMixer::new(50.0, 50.0, 50.0, 50.0)),
            Box::new(CenterChannel::new(r, true, 0.0, 100.0, 8000.0)),
            Box::new(PhaseShift::new(r, 90.0, -90.0, 1000.0)),
            Box::new(graphic_bank(r, &[6.0; 10], 0.0)),
            Box::new(scientific_bank(r, 2, 8, 300.0, 3000.0, 0.0)),
            Box::new(Fir::design(
                r,
                &[(100.0, -20.0), (1000.0, 0.0), (8000.0, -20.0)],
                511,
            )),
            Box::new(StudioReverb::new(r, 0.6, 20.0, 0.5, 1.0, 0.5, 0.8, 0.3)),
            Box::new(Convolver::new(synthetic_ir(r, 0), 0.3)),
            Box::new(Spectral::new(r, SpecMode::Adaptive, 1.0, 20.0, 0.0)),
            Box::new(Spectral::new(r, SpecMode::DeReverb, 1.0, 12.0, 0.0)),
            Box::new(ClickRemover::new(0.5)),
        ];
        for p in ps.iter_mut() {
            let mut b = tone(9000, 440.0);
            p.process(&mut b);
            assert!(finite(&b));
            assert!(b.iter().flat_map(|c| c.iter()).all(|v| v.abs() < 8.0));
        }
    }

    #[test]
    fn graphic_eq_cuts_its_band() {
        let mut g = graphic_bank(
            48000.0,
            &[0.0, 0.0, 0.0, 0.0, 0.0, -24.0, 0.0, 0.0, 0.0, 0.0],
            0.0,
        );
        let mut b = tone(48000, 1000.0);
        g.process(&mut b);
        let pk = b[0][24000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(pk < 0.1, "1 kHz band cut: {pk}");
    }

    #[test]
    fn declick_repairs_a_spike() {
        let mut x: Vec<f32> = (0..4000).map(|i| (i as f32 * 0.01).sin() * 0.3).collect();
        x[2000] = 0.95;
        x[2001] = -0.9;
        let fixed = declick(&mut x, 0.6);
        assert!(fixed > 0);
        assert!(x[2000].abs() < 0.4 && x[2001].abs() < 0.4);
    }

    #[test]
    fn convolver_keeps_length_and_delays_by_one_block() {
        let ir = vec![vec![1.0f32, 0.0, 0.0], vec![1.0, 0.0, 0.0]];
        let mut c = Convolver::new(ir, 1.0);
        let mut b = vec![
            (0..10000)
                .map(|i| if i == 10 { 1.0 } else { 0.0 })
                .collect::<Vec<f32>>();
            2
        ];
        c.process(&mut b);
        assert_eq!(b[0].len(), 10000);
        let at = b[0].iter().position(|v| v.abs() > 0.5).unwrap_or(0);
        assert_eq!(at, 10 + 4096);
    }
}
