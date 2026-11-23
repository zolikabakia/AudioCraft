//! Vocal-tract speech synthesis ("Vocal Tract" tab of Generate Speech).
//!
//! A physical model in the tradition of Pink Trombone (Neil Thapen, MIT licence) and the
//! research it builds on: an LF-model glottal source (Fant, Liljencrants and Lin; shape set from
//! a single tenseness value via Rd) drives a 44-section Kelly-Lochbaum waveguide of the vocal
//! tract with a 28-section nasal branch at the velum. Tongue, constriction and velum targets
//! move smoothly between the sounds of a short phonetic script, fricatives add turbulence noise
//! at the constriction, and plosives close the tract and release it. This is AudioCraft's own
//! implementation of the published model; no code was copied.

use crate::audio::AudioData;
use std::f32::consts::PI;

/// Everything the person sets on the Vocal Tract tab.
#[derive(Clone, Debug, PartialEq)]
pub struct TractParams {
    /// Voice pitch in Hz.
    pub pitch: f32,
    /// 0 = breathy, 1 = pressed.
    pub tenseness: f32,
    /// Vibrato depth in semitones.
    pub vibrato: f32,
    /// Where the tongue body sits (12 = back, 29 = front) and how open it leaves the tract.
    pub tongue_index: f32,
    pub tongue_diameter: f32,
    /// An extra narrowing (lips, teeth, palate). Diameter 3 or more means none.
    pub constriction_index: f32,
    pub constriction_diameter: f32,
    /// Velum opening: 0.01 closed (oral), 0.4 open (nasal).
    pub velum: f32,
    /// Length of a held sound when the script is empty, in seconds.
    pub seconds: f32,
    /// A phonetic script, e.g. "hello ai am a trombone". Empty = hold the sound set above.
    pub script: String,
    /// Speaking speed (1 = normal).
    pub speed: f32,
    /// Pitch falls over a phrase like a spoken sentence (semitones); a question rises at the end.
    pub intonation: f32,
    /// Size of the voice: above 1 a shorter tract (higher formants: women, children), below 1
    /// a longer one. The pitch stays where it is.
    pub formant: f32,
    /// Whisper: breath noise instead of a voiced tone.
    pub whisper: bool,
}

/// Ready voices for the Vocal Tract tab: (name, pitch Hz, tenseness, vibrato, intonation,
/// formant, whisper).
pub const VOICES: [(&str, f32, f32, f32, f32, f32, bool); 8] = [
    ("Man", 110.0, 0.6, 0.12, 3.0, 1.0, false),
    ("Woman", 205.0, 0.55, 0.15, 4.0, 1.17, false),
    ("Child", 290.0, 0.5, 0.2, 5.0, 1.33, false),
    ("Deep", 78.0, 0.65, 0.08, 2.5, 0.9, false),
    ("Elder", 125.0, 0.35, 0.55, 3.0, 0.97, false),
    ("Breathy", 140.0, 0.12, 0.12, 3.0, 1.05, false),
    ("Robot", 110.0, 0.95, 0.0, 0.0, 1.0, false),
    ("Whisper", 150.0, 0.3, 0.0, 2.0, 1.05, true),
];

impl TractParams {
    /// Take on one of the ready `VOICES` (the words and the held shape stay).
    pub fn set_voice(&mut self, i: usize) {
        if let Some(&(_, pitch, tense, vib, inton, formant, whisper)) = VOICES.get(i) {
            self.pitch = pitch;
            self.tenseness = tense;
            self.vibrato = vib;
            self.intonation = inton;
            self.formant = formant;
            self.whisper = whisper;
        }
    }
}

impl Default for TractParams {
    fn default() -> Self {
        TractParams {
            pitch: 120.0,
            tenseness: 0.6,
            vibrato: 0.15,
            tongue_index: 12.9,
            tongue_diameter: 2.43,
            constriction_index: 36.0,
            constriction_diameter: 3.5,
            velum: 0.01,
            seconds: 1.5,
            script: "hello, i am a vocal tract".into(),
            speed: 1.0,
            intonation: 3.0,
            formant: 1.0,
            whisper: false,
        }
    }
}

const N: usize = 44;
const NOSE: usize = 28;
const NOSE_START: usize = N - NOSE + 1;
const BLADE_START: usize = 10;
const TIP_START: usize = 32;
const LIP_START: usize = 39;
/// The model's tongue-curve offset: without it the tongue widens the tube where it should
/// narrow it, and every vowel comes out the same.
const GRID_OFFSET: f32 = 1.7;
/// The tube is tuned for this rate (two half-steps per sample); other output rates are
/// resampled, so vowels keep their resonances whatever rate is asked for.
const MODEL_RATE: f32 = 48000.0;

/// One articulation target.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Target {
    tongue: (f32, f32),
    constriction: (f32, f32),
    velum: f32,
    voiced: f32,
    noise: f32,
    secs: f32,
}

const OPEN: (f32, f32) = (40.0, 3.5);

fn vowel(c: char) -> Option<(f32, f32)> {
    Some(match c {
        'a' => (13.5, 2.6),  // "ah"
        'e' => (22.0, 2.4),  // "eh"
        'i' => (27.5, 2.05), // "ee"
        'o' => (14.0, 2.1),  // "oh"
        'u' => (19.5, 2.0),  // "oo" (with rounded lips below)
        'y' => (27.0, 2.0),
        'w' => (19.5, 2.0),
        'r' => (19.0, 2.3),
        'l' => (24.0, 2.5),
        'h' => (16.0, 2.9),
        _ => return None,
    })
}

/// English spelling to the simple letters the targets understand: digraphs become one sound
/// (ee, oo, ou, ai, ng, ph, ck, qu, tion, igh), c is soft before e/i/y and hard elsewhere, x is
/// "ks", a silent final e is dropped and a final y after a consonant sounds like "ee".
pub fn spell(text: &str) -> String {
    let mut out = String::new();
    for (wi, word) in text.split(' ').enumerate() {
        if wi > 0 {
            out.push(' ');
        }
        let mut w = word.to_string();
        for (a, b) in [
            ("tion", "shun"),
            ("igh", "ai"),
            ("ph", "f"),
            ("wh", "w"),
            ("ck", "k"),
            ("qu", "kw"),
            ("ee", "i"),
            ("ea", "i"),
            ("oo", "u"),
            ("ou", "au"),
            ("ow", "au"),
            ("ai", "ei"),
            ("ay", "ei"),
            ("oy", "oi"),
            ("ng", "N"),
            ("x", "ks"),
        ] {
            w = w.replace(a, b);
        }
        // c: soft before e, i, y; "ch" stays for the ch sound
        let cs: Vec<char> = w.chars().collect();
        let mut v = String::new();
        for (i, c) in cs.iter().enumerate() {
            let next = cs.get(i + 1).copied();
            if *c == 'c' && next != Some('h') {
                v.push(if matches!(next, Some('e' | 'i' | 'y')) {
                    's'
                } else {
                    'k'
                });
            } else {
                v.push(*c);
            }
        }
        let letters: Vec<char> = v.chars().collect();
        let is_vowel = |c: char| "aeiouy".contains(c);
        let n = letters.len();
        let mut v: String = letters.iter().collect();
        // a silent final e ("make", "voice")
        if n > 3
            && letters[n - 1] == 'e'
            && !is_vowel(letters[n - 2])
            && letters[n - 2].is_alphabetic()
        {
            v.pop();
        }
        // a final y after a consonant sounds like "ee" ("happy")
        let letters: Vec<char> = v.chars().collect();
        let n = letters.len();
        if n > 1 && letters[n - 1] == 'y' && !is_vowel(letters[n - 2]) {
            v.pop();
            v.push('i');
        }
        out.push_str(&v);
    }
    out
}

/// Turn a short script into articulation targets. Letters map to rough English sounds;
/// spaces and punctuation are pauses.
fn script_targets(text: &str, speed: f32) -> Vec<Target> {
    let speed = speed.clamp(0.25, 4.0);
    let v = |tongue: (f32, f32), secs: f32| Target {
        tongue,
        constriction: OPEN,
        velum: 0.01,
        voiced: 1.0,
        noise: 0.0,
        secs: secs / speed,
    };
    let mut out: Vec<Target> = Vec::new();
    let mut last_tongue = (13.0f32, 2.6f32);
    let lower = spell(&text.to_lowercase());
    let chars: Vec<char> = lower.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let pair = next.map(|n| [c, n]);
        let mut t = match (c, pair) {
            ('s', Some(['s', 'h'])) | ('c', Some(['c', 'h'])) => {
                i += 1;
                Target {
                    tongue: last_tongue,
                    constriction: (33.0, 0.45),
                    velum: 0.01,
                    voiced: 0.0,
                    noise: 1.0,
                    secs: 0.13 / speed,
                }
            }
            ('t', Some(['t', 'h'])) => {
                i += 1;
                Target {
                    tongue: last_tongue,
                    constriction: (38.0, 0.5),
                    velum: 0.01,
                    voiced: 0.3,
                    noise: 0.6,
                    secs: 0.1 / speed,
                }
            }
            ('s', _) | ('c', _) | ('x', _) => Target {
                tongue: last_tongue,
                constriction: (36.5, 0.35),
                velum: 0.01,
                voiced: 0.0,
                noise: 1.0,
                secs: 0.12 / speed,
            },
            ('z', _) => Target {
                tongue: last_tongue,
                constriction: (36.5, 0.4),
                velum: 0.01,
                voiced: 0.8,
                noise: 0.6,
                secs: 0.11 / speed,
            },
            ('f', _) => Target {
                tongue: last_tongue,
                constriction: (41.0, 0.45),
                velum: 0.01,
                voiced: 0.0,
                noise: 0.9,
                secs: 0.11 / speed,
            },
            ('v', _) => Target {
                tongue: last_tongue,
                constriction: (41.0, 0.5),
                velum: 0.01,
                voiced: 0.8,
                noise: 0.5,
                secs: 0.09 / speed,
            },
            ('m', _) => Target {
                tongue: last_tongue,
                constriction: (41.0, 0.0),
                velum: 0.4,
                voiced: 1.0,
                noise: 0.0,
                secs: 0.1 / speed,
            },
            // "ng": the back of the tongue closes against the soft palate, through the nose
            ('N', _) => Target {
                tongue: (20.0, 0.0),
                constriction: (22.0, 0.0),
                velum: 0.4,
                voiced: 1.0,
                noise: 0.0,
                secs: 0.1 / speed,
            },
            ('n', _) => Target {
                tongue: last_tongue,
                constriction: (36.0, 0.0),
                velum: 0.4,
                voiced: 1.0,
                noise: 0.0,
                secs: 0.09 / speed,
            },
            ('g', _) | ('k', _) | ('q', _) => Target {
                tongue: (20.0, 0.0),
                constriction: (22.0, 0.0),
                velum: 0.01,
                voiced: if c == 'g' { 0.7 } else { 0.0 },
                noise: 0.0,
                secs: 0.07 / speed,
            },
            ('d', _) | ('t', _) => Target {
                tongue: last_tongue,
                constriction: (36.0, 0.0),
                velum: 0.01,
                voiced: if c == 'd' { 0.7 } else { 0.0 },
                noise: 0.0,
                secs: 0.06 / speed,
            },
            ('b', _) | ('p', _) => Target {
                tongue: last_tongue,
                constriction: (41.0, 0.0),
                velum: 0.01,
                voiced: if c == 'b' { 0.7 } else { 0.0 },
                noise: 0.0,
                secs: 0.06 / speed,
            },
            ('j', _) => Target {
                tongue: (27.0, 2.0),
                constriction: (33.0, 0.5),
                velum: 0.01,
                voiced: 0.8,
                noise: 0.5,
                secs: 0.09 / speed,
            },
            ('h', _) => Target {
                tongue: last_tongue,
                constriction: OPEN,
                velum: 0.01,
                voiced: 0.0,
                noise: 0.0,
                secs: 0.08 / speed,
            },
            (c, _) if vowel(c).is_some() => {
                let tg = vowel(c).unwrap_or(last_tongue);
                last_tongue = tg;
                let mut t = v(
                    tg,
                    if matches!(c, 'w' | 'y' | 'r' | 'l') {
                        0.08
                    } else {
                        0.17
                    },
                );
                if c == 'u' || c == 'o' || c == 'w' {
                    t.constriction = (41.0, if c == 'o' { 1.6 } else { 1.0 });
                }
                if c == 'r' {
                    t.constriction = (31.0, 1.3);
                }
                if c == 'l' {
                    t.constriction = (35.0, 1.0);
                }
                t
            }
            _ => Target {
                tongue: last_tongue,
                constriction: OPEN,
                velum: 0.01,
                voiced: 0.0,
                noise: 0.0,
                secs: if c == ' ' { 0.09 } else { 0.25 } / speed,
            },
        };
        // a release burst after a stop
        let stop = t.constriction.1 == 0.0 && t.velum < 0.1;
        if stop && t.secs > 0.0 {
            out.push(t);
            t = Target {
                constriction: (t.constriction.0, 0.6),
                voiced: t.voiced,
                noise: 0.8,
                secs: 0.03 / speed,
                ..t
            };
        }
        out.push(t);
        i += 1;
    }
    out
}

/// The LF glottal pulse shape for one tenseness value (Rd parameterisation).
#[derive(Clone, Copy, Debug)]
struct Lf {
    alpha: f32,
    e0: f32,
    epsilon: f32,
    shift: f32,
    delta: f32,
    te: f32,
    omega: f32,
}

impl Lf {
    fn new(tenseness: f32) -> Lf {
        let rd = (3.0 * (1.0 - tenseness)).clamp(0.5, 2.7);
        let ra = -0.01 + 0.048 * rd;
        let rk = 0.224 + 0.118 * rd;
        let rg = (rk / 4.0) * (0.5 + 1.2 * rk) / (0.11 * rd - ra * (0.5 + 1.2 * rk));
        let ta = ra;
        let tp = 1.0 / (2.0 * rg);
        let te = tp + tp * rk;
        let epsilon = 1.0 / ta;
        let shift = (-epsilon * (1.0 - te)).exp();
        let delta = 1.0 - shift;
        let rhs = ((1.0 / epsilon) * (shift - 1.0) + (1.0 - te) * shift) / delta;
        let lower = -(te - tp) / 2.0 + rhs;
        let upper = -lower;
        let omega = PI / tp;
        let s = (omega * te).sin();
        let y = -PI * s * upper / (tp * 2.0);
        let z = y.max(1e-6).ln();
        let alpha = z / (tp / 2.0 - te);
        let e0 = -1.0 / (s * (alpha * te).exp());
        Lf {
            alpha,
            e0,
            epsilon,
            shift,
            delta,
            te,
            omega,
        }
    }

    fn at(&self, t: f32) -> f32 {
        if t > self.te {
            (-(-self.epsilon * (t - self.te)).exp() + self.shift) / self.delta
        } else {
            self.e0 * (self.alpha * t).exp() * (self.omega * t).sin()
        }
    }
}

struct Tract {
    diameter: [f32; N],
    rest: [f32; N],
    r: [f32; N],
    l: [f32; N],
    refl: [f32; N + 1],
    jr: [f32; N + 1],
    jl: [f32; N + 1],
    nose_d: [f32; NOSE],
    nr: [f32; NOSE],
    nl: [f32; NOSE],
    nrefl: [f32; NOSE + 1],
    njr: [f32; NOSE + 1],
    njl: [f32; NOSE + 1],
    refl_left: f32,
    refl_right: f32,
    refl_nose: f32,
}

impl Tract {
    fn new() -> Tract {
        let mut rest = [1.5f32; N];
        for (i, d) in rest.iter_mut().enumerate() {
            *d = if (i as f32) < 7.0 * N as f32 / 44.0 - 0.5 {
                0.6
            } else if (i as f32) < 12.0 * N as f32 / 44.0 {
                1.1
            } else {
                1.5
            };
        }
        let mut nose_d = [0.0f32; NOSE];
        for (i, d) in nose_d.iter_mut().enumerate() {
            let x = 2.0 * (i as f32 / NOSE as f32);
            let v = if x < 1.0 {
                0.4 + 1.6 * x
            } else {
                0.5 + 1.5 * (2.0 - x)
            };
            *d = v.min(1.9);
        }
        let mut t = Tract {
            diameter: rest,
            rest,
            r: [0.0; N],
            l: [0.0; N],
            refl: [0.0; N + 1],
            jr: [0.0; N + 1],
            jl: [0.0; N + 1],
            nose_d,
            nr: [0.0; NOSE],
            nl: [0.0; NOSE],
            nrefl: [0.0; NOSE + 1],
            njr: [0.0; NOSE + 1],
            njl: [0.0; NOSE + 1],
            refl_left: 0.0,
            refl_right: 0.0,
            refl_nose: 0.0,
        };
        t.calc_nose_reflections();
        t.calc_reflections();
        t
    }

    /// Shape the tract for a tongue position and an extra constriction.
    fn shape(&mut self, tongue: (f32, f32), con: (f32, f32), velum: f32) -> [f32; N] {
        let mut target = self.rest;
        let (ti, td) = tongue;
        if td > 0.0 {
            for (i, d) in target
                .iter_mut()
                .enumerate()
                .take(LIP_START)
                .skip(BLADE_START)
            {
                let t = 1.1 * PI * (ti - i as f32) / (TIP_START - BLADE_START) as f32;
                let fixed = 2.0 + (td - 2.0) / 1.5;
                let mut curve = (1.5 - fixed + GRID_OFFSET) * t.cos();
                if i == BLADE_START - 2 || i == LIP_START - 1 {
                    curve *= 0.8;
                }
                if i == BLADE_START || i == LIP_START - 2 {
                    curve *= 0.94;
                }
                *d = 1.5 - curve;
            }
        } else {
            // the back of the tongue closes against the palate (k, g)
            for (i, d) in target.iter_mut().enumerate() {
                if (i as f32 - ti).abs() < 2.5 {
                    *d = 0.0;
                }
            }
        }
        let (ci, cd) = con;
        if cd < 3.0 {
            let width = if ci < 25.0 {
                10.0
            } else if ci >= TIP_START as f32 {
                5.0
            } else {
                10.0 - 5.0 * (ci - 25.0) / (TIP_START as f32 - 25.0)
            };
            for (i, d) in target.iter_mut().enumerate() {
                let rel = (i as f32 - ci).abs() - 0.5;
                let shrink = if rel <= 0.0 {
                    0.0
                } else if rel > width {
                    1.0
                } else {
                    0.5 * (1.0 - (PI * rel / width).cos())
                };
                if cd < *d {
                    *d = cd + (*d - cd) * shrink;
                }
            }
        }
        self.nose_d[0] = velum.clamp(0.0, 0.5);
        target
    }

    fn calc_reflections(&mut self) {
        let mut a = [0.0f32; N];
        for (x, d) in a.iter_mut().zip(self.diameter.iter()) {
            *x = d * d;
        }
        for i in 1..N {
            self.refl[i] = if a[i] == 0.0 {
                0.999
            } else {
                (a[i - 1] - a[i]) / (a[i - 1] + a[i])
            };
        }
        let sum = a[NOSE_START] + a[NOSE_START + 1] + self.nose_d[0] * self.nose_d[0];
        let sum = sum.max(1e-6);
        self.refl_left = (2.0 * a[NOSE_START] - sum) / sum;
        self.refl_right = (2.0 * a[NOSE_START + 1] - sum) / sum;
        self.refl_nose = (2.0 * self.nose_d[0] * self.nose_d[0] - sum) / sum;
    }

    fn calc_nose_reflections(&mut self) {
        let a: Vec<f32> = self.nose_d.iter().map(|d| d * d).collect();
        for i in 1..NOSE {
            self.nrefl[i] = (a[i - 1] - a[i]) / (a[i - 1] + a[i]).max(1e-6);
        }
    }

    /// One half-sample step; returns (lip output, nose output).
    fn step(&mut self, glottal: f32, noise: f32, noise_at: f32) -> (f32, f32) {
        // turbulence enters at the narrowest point
        if noise != 0.0 {
            let i = noise_at.floor().clamp(1.0, (N - 2) as f32) as usize;
            let frac = noise_at - i as f32;
            let w0 = noise * (1.0 - frac) * 0.5;
            let w1 = noise * frac * 0.5;
            self.r[i] += w0;
            self.l[i] += w0;
            self.r[i + 1] += w1;
            self.l[i + 1] += w1;
        }
        self.jr[0] = self.l[0] * 0.75 + glottal;
        self.jl[N] = self.r[N - 1] * -0.85;
        for i in 1..N {
            let w = self.refl[i] * (self.r[i - 1] + self.l[i]);
            self.jr[i] = self.r[i - 1] - w;
            self.jl[i] = self.l[i] + w;
        }
        let i = NOSE_START;
        self.jl[i] =
            self.refl_left * self.r[i - 1] + (1.0 + self.refl_left) * (self.nl[0] + self.l[i]);
        self.jr[i] =
            self.refl_right * self.l[i] + (1.0 + self.refl_right) * (self.r[i - 1] + self.nl[0]);
        self.njr[0] =
            self.refl_nose * self.nl[0] + (1.0 + self.refl_nose) * (self.l[i] + self.r[i - 1]);
        for i in 0..N {
            self.r[i] = (self.jr[i] * 0.999).clamp(-8.0, 8.0);
            self.l[i] = (self.jl[i + 1] * 0.999).clamp(-8.0, 8.0);
        }
        let lip = self.r[N - 1];
        self.njl[NOSE] = self.nr[NOSE - 1] * -0.85;
        for i in 1..NOSE {
            let w = self.nrefl[i] * (self.nr[i - 1] + self.nl[i]);
            self.njr[i] = self.nr[i - 1] - w;
            self.njl[i] = self.nl[i] + w;
        }
        for i in 0..NOSE {
            self.nr[i] = (self.njr[i] * 0.999).clamp(-8.0, 8.0);
            self.nl[i] = (self.njl[i + 1] * 0.999).clamp(-8.0, 8.0);
        }
        (lip, self.nr[NOSE - 1])
    }

    /// The narrowest point of the oral tract and its diameter (for fricative noise).
    fn narrowest(&self) -> (f32, f32) {
        let mut best = (0usize, f32::MAX);
        for (i, d) in self.diameter.iter().enumerate().skip(BLADE_START) {
            if *d < best.1 {
                best = (i, *d);
            }
        }
        (best.0 as f32, best.1)
    }
}

/// A small, deterministic noise source (the result is the same every time).
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// The diameter profile for drawing the tract (oral sections, then the nasal branch).
pub fn profile(p: &TractParams) -> (Vec<f32>, Vec<f32>) {
    let mut t = Tract::new();
    let shape = t.shape(
        (p.tongue_index, p.tongue_diameter),
        (p.constriction_index, p.constriction_diameter),
        p.velum,
    );
    (shape.to_vec(), t.nose_d.to_vec())
}

/// Render the voice: the held sound when the script is empty, else the script.
pub fn render(p: &TractParams, rate: u32) -> Result<AudioData, String> {
    let rate = rate.clamp(8000, 192000);
    let f = p.formant.clamp(0.7, 1.5);
    // the tube has a fixed number of sections, so running it faster makes it shorter (formants
    // rise by `f`) while timing and pitch, which are in seconds and Hz, stay where they are
    let internal = (MODEL_RATE * f).round() as u32;
    let d = render_at(p, internal)?;
    if internal == rate {
        return Ok(d);
    }
    let mut out = crate::audio::resample(&d, rate);
    out.rate = rate;
    Ok(out)
}

fn render_at(p: &TractParams, rate: u32) -> Result<AudioData, String> {
    let rate = rate.clamp(8000, 240000);
    let held = Target {
        tongue: (
            p.tongue_index.clamp(10.0, 30.0),
            p.tongue_diameter.clamp(1.5, 3.6),
        ),
        constriction: (
            p.constriction_index.clamp(10.0, 43.0),
            p.constriction_diameter.clamp(0.0, 3.6),
        ),
        velum: p.velum.clamp(0.0, 0.5),
        voiced: 1.0,
        noise: if p.constriction_diameter < 1.0 {
            0.7
        } else {
            0.0
        },
        secs: p.seconds.clamp(0.05, 60.0),
    };
    let mut targets = if p.script.trim().is_empty() {
        vec![held]
    } else {
        script_targets(&p.script, p.speed)
    };
    if targets.is_empty() {
        return Err("Type something for the vocal tract to say.".into());
    }
    // a little silence at both ends
    targets.insert(
        0,
        Target {
            voiced: 0.0,
            secs: 0.05,
            ..targets[0]
        },
    );
    let last = targets[targets.len() - 1];
    targets.push(Target {
        voiced: 0.0,
        noise: 0.0,
        secs: 0.12,
        ..last
    });
    let total: f32 = targets.iter().map(|t| t.secs).sum();
    if total > 120.0 {
        return Err("Please keep the vocal tract text under two minutes.".into());
    }
    let n = (total * rate as f32) as usize;
    let mut out = vec![0.0f32; n];
    let mut tract = Tract::new();
    let mut noise = Noise(0x1234_5678);
    let lf = Lf::new(p.tenseness.clamp(0.0, 1.0));
    let mut phase = 0.0f32;
    let dt = 1.0 / rate as f32;
    let (mut cur_t, mut cur_c, mut cur_v, mut voiced, mut fric) = (
        targets[0].tongue,
        targets[0].constriction,
        targets[0].velum,
        0.0f32,
        0.0f32,
    );
    let mut seg = 0usize;
    let mut seg_end = targets[0].secs;
    let mut target_shape = tract.shape(cur_t, cur_c, cur_v);
    let mut lp = 0.0f32;
    for (k, o) in out.iter_mut().enumerate() {
        let time = k as f32 * dt;
        while time > seg_end && seg + 1 < targets.len() {
            seg += 1;
            seg_end += targets[seg].secs;
        }
        let tg = targets[seg];
        // glide toward the target (about 30 ms), recomputing the shape every 64 samples
        let glide = 1.0 - (-dt / 0.03).exp();
        cur_t.0 += (tg.tongue.0 - cur_t.0) * glide;
        cur_t.1 += (tg.tongue.1 - cur_t.1) * glide;
        cur_c.0 += (tg.constriction.0 - cur_c.0) * glide * 2.0;
        cur_c.1 += (tg.constriction.1 - cur_c.1) * glide * 2.0;
        cur_v += (tg.velum - cur_v) * glide;
        voiced += (tg.voiced - voiced) * (1.0 - (-dt / 0.015).exp());
        fric += (tg.noise - fric) * (1.0 - (-dt / 0.01).exp());
        if k % 64 == 0 {
            let tongue = if tg.tongue.1 == 0.0 { tg.tongue } else { cur_t };
            let con = if tg.constriction.1 == 0.0 && cur_c.1 < 0.2 {
                (tg.constriction.0, 0.0)
            } else {
                cur_c
            };
            target_shape = tract.shape(tongue, con, cur_v);
        }
        for i in 0..N {
            // the tract moves toward its shape, a closure a little faster
            let d = target_shape[i] - tract.diameter[i];
            tract.diameter[i] += d * if target_shape[i] < tract.diameter[i] {
                0.02
            } else {
                0.012
            };
            if tract.diameter[i] < 0.0 {
                tract.diameter[i] = 0.0;
            }
        }
        tract.calc_reflections();
        // glottis: pitch with vibrato, phrase intonation and a little natural wobble
        let wobble = (time * 4.07 * 2.0 * PI).sin() * 0.02 + (time * 2.15 * 2.0 * PI).sin() * 0.03;
        let frac = time / total.max(0.1);
        let fall = if p.script.trim().is_empty() {
            0.0
        } else if p.script.trim_end().ends_with('?') {
            // a question: a gentle fall, then a rise on the last words
            if frac < 0.7 {
                -p.intonation * frac * 0.5
            } else {
                -p.intonation * 0.35 + (frac - 0.7) / 0.3 * p.intonation * 1.6
            }
        } else {
            -p.intonation * frac
        };
        let semis = p.vibrato * (time * 6.0 * 2.0 * PI).sin() + wobble + fall;
        let f0 = p.pitch.clamp(40.0, 900.0) * 2f32.powf(semis / 12.0);
        phase += f0 * dt;
        if phase >= 1.0 {
            phase -= 1.0;
        }
        let pulse = lf.at(phase);
        let aspiration = noise.next()
            * 0.2
            * (1.0 - p.tenseness.clamp(0.0, 1.0).sqrt())
            * (0.2 + 0.8 * (phase < lf.te) as u8 as f32);
        let glottal = if p.whisper {
            noise.next() * 0.09 * voiced + noise.next() * 0.02 * (1.0 - voiced) * fric
        } else {
            (pulse * 0.08 + aspiration * 0.25) * voiced
                + noise.next() * 0.02 * (1.0 - voiced) * fric
        };
        let (at, dia) = tract.narrowest();
        let turb = if fric > 0.01 && dia < 1.2 {
            noise.next() * fric * 0.6 * (1.2 - dia.max(0.0)).min(1.0)
        } else {
            0.0
        };
        // band-limit the turbulence a little so it hisses instead of crackling
        lp += (turb - lp) * 0.6;
        let (l1, n1) = tract.step(glottal, lp, at + 0.5);
        let (l2, n2) = tract.step(glottal, lp, at + 0.5);
        *o = (l1 + l2 + n1 + n2) * 0.5;
    }
    // remove DC and normalise to -1 dBFS
    let mean = out.iter().sum::<f32>() / out.len().max(1) as f32;
    let mut prev_in = 0.0f32;
    let mut prev_out = 0.0f32;
    for s in out.iter_mut() {
        let x = *s - mean;
        let y = x - prev_in + 0.995 * prev_out;
        prev_in = x;
        prev_out = y;
        *s = y;
    }
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if !peak.is_finite() || peak < 1e-6 {
        return Err(
            "The vocal tract made no sound with these settings (try opening the constriction)."
                .into(),
        );
    }
    let g = 0.89 / peak;
    for s in out.iter_mut() {
        *s *= g;
    }
    Ok(AudioData {
        rate,
        ch: vec![out],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spelling_rules_and_voices() {
        assert_eq!(spell("phone"), "fon");
        assert_eq!(spell("sing"), "siN");
        assert_eq!(spell("city cat"), "siti kat");
        assert_eq!(spell("quick"), "kwik");
        assert_eq!(spell("nation"), "nashun");
        assert_eq!(spell("happy"), "happi");
        for i in 0..VOICES.len() {
            let mut p = TractParams {
                script: "is it working?".into(),
                ..Default::default()
            };
            p.set_voice(i);
            let d = render(&p, 16000).unwrap();
            assert!(d.frames() > 1000, "{}", VOICES[i].0);
            assert!(d.ch[0].iter().all(|v| v.is_finite()), "{}", VOICES[i].0);
        }
    }

    #[test]
    fn a_held_vowel_is_voiced_and_bounded() {
        let p = TractParams {
            script: String::new(),
            seconds: 0.5,
            ..Default::default()
        };
        let d = render(&p, 22050).expect("render");
        assert_eq!(d.channels(), 1);
        assert!(d.duration() > 0.5 && d.duration() < 0.8);
        assert!(d.ch[0].iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        // voiced: the signal repeats at about the pitch period
        let x = &d.ch[0][4000..8000];
        let period = (22050.0 / 120.0) as usize;
        let corr: f32 = x
            .iter()
            .zip(x.iter().skip(period))
            .map(|(a, b)| a * b)
            .sum();
        assert!(corr > 0.0, "a held vowel should be periodic");
    }

    #[test]
    fn a_script_renders_and_extreme_settings_do_not_explode() {
        let d = render(&TractParams::default(), 16000).expect("script");
        assert!(d.duration() > 1.0);
        let wild = TractParams {
            pitch: 9999.0,
            tenseness: 5.0,
            constriction_diameter: -3.0,
            tongue_diameter: 99.0,
            script: "sssh ptk mmm".into(),
            ..Default::default()
        };
        let d = render(&wild, 16000).expect("clamped");
        assert!(d.ch[0].iter().all(|s| s.is_finite()));
        assert!(
            render(
                &TractParams {
                    script: "a".repeat(5000),
                    ..Default::default()
                },
                16000
            )
            .is_err()
        );
    }

    #[test]
    fn the_profile_follows_the_tongue() {
        let back = profile(&TractParams {
            tongue_index: 13.0,
            ..Default::default()
        })
        .0;
        let front = profile(&TractParams {
            tongue_index: 28.0,
            ..Default::default()
        })
        .0;
        assert_eq!(back.len(), N);
        assert_ne!(back, front);
        let closed = profile(&TractParams {
            constriction_index: 41.0,
            constriction_diameter: 0.0,
            ..Default::default()
        })
        .0;
        assert!(
            closed[41] < 0.05,
            "a lip closure closes the tract: {}",
            closed[41]
        );
    }
}

#[cfg(test)]
mod formant_probe {
    use super::*;

    /// Prints the strongest spectral peaks of held vowels (run by hand with --ignored --nocapture).
    #[test]
    #[ignore]
    fn print_vowel_formants() {
        for (name, ti, td, ci, cd) in [
            ("ah", 13.5, 2.6, 40.0, 3.5),
            ("ee", 27.5, 2.05, 40.0, 3.5),
            ("oo", 19.5, 2.0, 41.0, 1.0),
            ("eh", 22.0, 2.4, 40.0, 3.5),
        ] {
            let p = TractParams {
                tongue_index: ti,
                tongue_diameter: td,
                constriction_index: ci,
                constriction_diameter: cd,
                script: String::new(),
                seconds: 1.0,
                vibrato: 0.0,
                pitch: 100.0,
                ..Default::default()
            };
            let d = render(&p, 16000).unwrap();
            let x = &d.ch[0][4000..12000];
            let n = 8192;
            let mut buf: Vec<rustfft::num_complex::Complex<f32>> = (0..n)
                .map(|i| {
                    let w = 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos();
                    // pre-emphasis flattens the voice's natural tilt so formants show
                    let v = x.get(i).copied().unwrap_or(0.0)
                        - 0.95 * x.get(i.wrapping_sub(1)).copied().unwrap_or(0.0);
                    rustfft::num_complex::Complex::new(v * w, 0.0)
                })
                .collect();
            rustfft::FftPlanner::new()
                .plan_fft_forward(n)
                .process(&mut buf);
            let mag: Vec<f32> = buf[..n / 2].iter().map(|c| c.norm()).collect();
            // smooth over 250 Hz to see the envelope, not the harmonics
            let hz = 16000.0 / n as f32;
            let w = (120.0 / hz) as usize;
            let env: Vec<f32> = (0..mag.len())
                .map(|i| {
                    let a = i.saturating_sub(w);
                    let b = (i + w).min(mag.len());
                    mag[a..b].iter().sum::<f32>() / (b - a) as f32
                })
                .collect();
            let mut peaks = vec![];
            for i in 1..env.len() - 1 {
                if env[i] > env[i - 1]
                    && env[i] >= env[i + 1]
                    && (i as f32 * hz) > 150.0
                    && (i as f32 * hz) < 4000.0
                {
                    peaks.push(((i as f32 * hz) as i32, env[i]));
                }
            }
            peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            let mut top: Vec<i32> = vec![];
            for (f, _) in &peaks {
                if top.iter().all(|t| (t - f).abs() >= 300) && *f > 200 {
                    top.push(*f);
                }
                if top.len() == 3 {
                    break;
                }
            }
            top.sort();
            let rms = (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
            println!("{name}: rms {rms:.4} peaks {top:?}");
        }
    }
}
