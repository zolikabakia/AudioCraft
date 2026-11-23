//! Waveform editor view: navigator, ruler, channel lanes, spectral display, HUD.

use crate::app::{App, Tool};
use crate::audio::{self, AudioData, Peaks};
use crate::dsp::{self, Effect, EffectSlot};
use crate::spectral::{SpecOp, SpecShape};
use crate::theme::{self, Icon};
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use std::sync::Arc;

#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub enum WaveDrag {
    #[default]
    None,
    Select {
        anchor: f64,
    },
    Scrub,
    Nav {
        grab: f64,
    },
    Marker {
        idx: usize,
    },
}

pub struct DragState {
    pub drag: WaveDrag,
    pub amp_zoom: f32,
    pub hud_gain: f32,
    pub fade_len: f64,
    pub fade_p: f32,
    pub fade_grab_y: f32,
    pub fade_which: u8,
    /// Frequency range shown by the spectral display (0 = the whole range).
    pub spec_lo: f32,
    pub spec_hi: f32,
    /// Paintbrush / spot healing brush diameter in pixels.
    pub brush_px: f32,
    /// Paintbrush opacity, percent (how strongly painted cells are selected).
    pub brush_opacity: f32,
    /// Picture of the current spectral selection: (what it shows, texture).
    pub sel_tex: Option<(u64, egui::TextureHandle)>,
    /// Background pitch analysis of the current file: (file, result when done: hop, pitch).
    pub pitch_job: Option<(usize, PitchSlot)>,
    /// The pitch display's own spectrogram picture.
    pub pitch_bg: SpecCache,
    pub spec_drag: SpecDrag,
    /// Gain on the spectral selection's HUD knob (applied on release).
    pub spec_gain: f32,
    /// Share of the editor's height for the waveform (above the spectral displays) and for the
    /// pitch display (below the frequency display), set by dragging the borders.
    pub split_wave: f32,
    pub split_pitch: f32,
    /// Where a right-drag zoom on the frequency scale started.
    pub spec_rzoom: Option<f32>,
    /// Range of the spectral pitch display (0: the Preferences' range).
    pub pitch_lo: f32,
    pub pitch_hi: f32,
}

/// Where a background pitch analysis leaves its result.
pub type PitchSlot = Arc<std::sync::Mutex<Option<(f64, Vec<Option<f32>>)>>>;

/// What a drag on the spectral display is doing.
#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub enum SpecDrag {
    #[default]
    None,
    /// Drawing a new marquee from this corner (time, Hz).
    Make {
        t: f64,
        f: f32,
    },
    /// Moving the selection; the last pointer position (time, Hz).
    Move {
        t: f64,
        f: f32,
    },
    /// Stretching marquee edges: bit 1 left, 2 right, 4 bottom (low Hz), 8 top (high Hz).
    Resize {
        edges: u8,
    },
    Lasso,
    Brush,
    Heal,
}

impl Default for DragState {
    fn default() -> Self {
        DragState {
            drag: WaveDrag::None,
            amp_zoom: 1.0,
            hud_gain: 0.0,
            fade_len: 0.0,
            fade_p: 1.0,
            fade_grab_y: 0.0,
            fade_which: 0,
            spec_lo: 0.0,
            spec_hi: 0.0,
            brush_px: 24.0,
            brush_opacity: 100.0,
            sel_tex: None,
            pitch_job: None,
            pitch_bg: SpecCache::default(),
            spec_drag: SpecDrag::None,
            spec_gain: 0.0,
            split_wave: 0.35,
            split_pitch: 0.45,
            spec_rzoom: None,
            pitch_lo: 0.0,
            pitch_hi: 0.0,
        }
    }
}

/// Frequency axis of a spectral display: `lo`..`hi` Hz from the bottom to the top, linear
/// (evenly spaced Hz, the original's default for the Spectral Frequency Display) or logarithmic
/// (evenly spaced octaves; the Spectral Pitch Display is always logarithmic, like a keyboard).
#[derive(Clone, Copy, Debug)]
pub struct FreqAxis {
    pub lo: f32,
    pub hi: f32,
    pub nyq: f32,
    pub log: bool,
}

/// The smallest frequency a logarithmic scale shows.
pub const SPEC_MIN_HZ: f32 = 16.0;

impl FreqAxis {
    /// The range actually drawn (a logarithmic scale cannot reach 0 Hz).
    fn ends(&self) -> (f32, f32) {
        if self.log {
            let a = self.lo.max(SPEC_MIN_HZ);
            (a, self.hi.max(a * 1.01))
        } else {
            (self.lo, self.hi.max(self.lo + 1e-3))
        }
    }
    /// The bottom of the scale's possible range.
    fn floor(&self) -> f32 {
        if self.log { SPEC_MIN_HZ } else { 0.0 }
    }
    /// Fraction up the lane (0 bottom .. 1 top) of frequency `f` (outside 0..1 when off screen).
    pub fn frac(&self, f: f32) -> f32 {
        let (a, b) = self.ends();
        if self.log {
            (f.max(1.0).ln() - a.ln()) / (b.ln() - a.ln()).max(1e-6)
        } else {
            (f - a) / (b - a)
        }
    }
    /// The frequency at a fraction up the lane (the inverse of `frac`), kept within 0..Nyquist.
    pub fn freq(&self, frac: f32) -> f32 {
        let (a, b) = self.ends();
        let f = if self.log {
            (a.ln() + frac * (b.ln() - a.ln())).exp()
        } else {
            a + frac * (b - a)
        };
        f.clamp(0.0, self.nyq.max(b))
    }
    pub fn y(&self, lane: Rect, f: f32) -> f32 {
        lane.bottom() - self.frac(f) * lane.height()
    }
    pub fn f_at(&self, lane: Rect, y: f32) -> f32 {
        self.freq((lane.bottom() - y) / lane.height().max(1.0))
    }
    /// The range after zooming by `k` (below 1 zooms in) around the point `pf` up the lane,
    /// which stays where it is. Never past the ends, never narrower than a semitone (log) or
    /// 60 Hz (linear).
    pub fn zoomed(&self, pf: f32, k: f32) -> (f32, f32) {
        let (a, b) = self.ends();
        let top = self.nyq.max(1.0);
        if self.log {
            let (la, lb) = (a.ln(), b.ln());
            let c = la + pf * (lb - la);
            let full = top.ln() - self.floor().ln();
            let span = ((lb - la) * k).clamp(1.06f32.ln(), full);
            let mut na = c - pf * span;
            na = na.clamp(self.floor().ln(), top.ln() - span);
            (na.exp(), (na + span).exp().min(top))
        } else {
            let c = a + pf * (b - a);
            let span = ((b - a) * k).clamp(60.0f32.min(top), top);
            let mut na = c - pf * span;
            na = na.clamp(0.0, top - span);
            (na, (na + span).min(top))
        }
    }
    /// The range moved by `s` lanes' heights (positive: up to higher frequencies), keeping its
    /// size on screen and stopping at either end.
    pub fn scrolled(&self, s: f32) -> (f32, f32) {
        let (a, b) = self.ends();
        let top = self.nyq.max(1.0);
        if self.log {
            let (la, lb) = (a.ln(), b.ln());
            let span = lb - la;
            let na =
                (la + s * span).clamp(self.floor().ln(), (top.ln() - span).max(self.floor().ln()));
            (na.exp(), (na + span).exp().min(top))
        } else {
            let span = b - a;
            let na = (a + s * span).clamp(0.0, (top - span).max(0.0));
            (na, (na + span).min(top))
        }
    }
}

/// How a spectrogram picture is made: FFT size, the decibel range shown, the window and the
/// colours (0: the Spectral Frequency Display's, 1: the Spectral Pitch Display's).
#[derive(Clone, Copy, Debug)]
pub struct SpecParams {
    pub n: usize,
    pub db_range: f32,
    pub window: u8,
    pub palette: u8,
}

/// MIDI note number of a frequency (A4 = 440 Hz = 69).
pub fn midi_of(f: f32) -> f32 {
    69.0 + 12.0 * (f.max(1e-3) / 440.0).log2()
}

pub fn note_hz(m: f32) -> f32 {
    440.0 * 2f32.powf((m - 69.0) / 12.0)
}

/// A note's name on the pitch scale: "C#/Db4", and the A's with their frequency, "A4 (440)".
pub fn note_label(m: i32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#/Db", "D", "D#/Eb", "E", "F", "F#/Gb", "G", "G#/Ab", "A", "A#/Bb", "B",
    ];
    let name = NAMES[m.rem_euclid(12) as usize];
    let oct = m.div_euclid(12) - 1;
    if m.rem_euclid(12) == 9 {
        let hz = note_hz(m as f32);
        if (hz - hz.round()).abs() < 0.01 {
            format!("{name}{oct} ({:.0})", hz)
        } else {
            format!("{name}{oct} ({hz:.1})")
        }
    } else {
        format!("{name}{oct}")
    }
}

/// The Spectral Pitch Display's colours: dark plum, maroon, red, orange, then a pale gold for
/// the loudest harmonics.
fn pitch_color(v: f32) -> Color32 {
    let stops: [(f32, [f32; 3]); 7] = [
        (0.0, [12.0, 4.0, 18.0]),
        (0.22, [52.0, 10.0, 40.0]),
        (0.42, [112.0, 18.0, 36.0]),
        (0.6, [175.0, 48.0, 22.0]),
        (0.76, [212.0, 112.0, 26.0]),
        (0.9, [196.0, 170.0, 70.0]),
        (1.0, [240.0, 228.0, 150.0]),
    ];
    let v = v.clamp(0.0, 1.0);
    for w in stops.windows(2) {
        let (a, ca) = w[0];
        let (b, cb) = w[1];
        if v <= b {
            let t = (v - a) / (b - a);
            return Color32::from_rgb(
                (ca[0] + (cb[0] - ca[0]) * t) as u8,
                (ca[1] + (cb[1] - ca[1]) * t) as u8,
                (ca[2] + (cb[2] - ca[2]) * t) as u8,
            );
        }
    }
    Color32::WHITE
}

/// Spectrogram pictures worked out on a background thread, so zooming and scrolling stay
/// smooth: while a new picture is being made, the last one is stretched to the new view, and
/// at most one job runs at a time (a view that changes again meanwhile is made next).
#[derive(Default)]
pub struct SpecCache {
    /// The pictures on screen: what they were made for and the time range they cover.
    shown: Option<(u64, (f64, f64), Vec<egui::TextureHandle>)>,
    /// The job being worked on: its key, time range and the slot its result lands in.
    job: Option<(
        u64,
        (f64, f64),
        Arc<std::sync::Mutex<Option<Vec<egui::ColorImage>>>>,
    )>,
}

impl SpecCache {
    /// Ask for the pictures of `key` (covering `view`); `make` builds them off the UI thread.
    /// Returns whether the shown pictures are already the wanted ones.
    pub fn want(
        &mut self,
        ctx: &egui::Context,
        name: &str,
        key: u64,
        view: (f64, f64),
        make: impl FnOnce() -> Vec<egui::ColorImage> + Send + 'static,
    ) -> bool {
        if self.shown.as_ref().map(|s| s.0) == Some(key) {
            return true;
        }
        // a finished job is shown right away (even an older view: closer than nothing)
        let done = self.job.as_ref().and_then(|(k, v, slot)| {
            slot.lock()
                .ok()
                .and_then(|mut g| g.take())
                .map(|imgs| (*k, *v, imgs))
        });
        if let Some((k, v, imgs)) = done {
            let tex = imgs
                .into_iter()
                .enumerate()
                .map(|(i, img)| {
                    ctx.load_texture(format!("{name}{i}"), img, egui::TextureOptions::LINEAR)
                })
                .collect();
            self.shown = Some((k, v, tex));
            self.job = None;
            if k == key {
                return true;
            }
        }
        if self.job.is_none() {
            let slot = Arc::new(std::sync::Mutex::new(None));
            let out = slot.clone();
            let c = ctx.clone();
            std::thread::spawn(move || {
                let imgs = make();
                if let Ok(mut g) = out.lock() {
                    *g = Some(imgs);
                }
                c.request_repaint();
            });
            self.job = Some((key, view, slot));
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
        false
    }

    /// Draw picture `i` into `lane` for the current view, stretched from the range it was made for.
    pub fn draw(&self, p: &egui::Painter, i: usize, lane: Rect, view: (f64, f64)) {
        let Some((_, (c0, c1), tex)) = &self.shown else {
            return;
        };
        let Some(t) = tex.get(i) else { return };
        let span = (view.1 - view.0).max(1e-12);
        let x0 = lane.left() + ((c0 - view.0) / span) as f32 * lane.width();
        let x1 = lane.left() + ((c1 - view.0) / span) as f32 * lane.width();
        if !(x0.is_finite() && x1.is_finite()) || x1 <= x0 {
            return;
        }
        let r = Rect::from_min_max(pos2(x0, lane.top()), pos2(x1, lane.bottom()));
        p.with_clip_rect(lane.intersect(p.clip_rect())).image(
            t.id(),
            r,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

pub fn ruler_ticks(span: f64, width: f32) -> (f64, f64) {
    // choose major step so labels are ~90px apart
    let target = span * 90.0 / width.max(1.0) as f64;
    let steps = [
        0.0001, 0.0002, 0.0005, 0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0,
        5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0,
    ];
    let major = *steps.iter().find(|s| **s >= target).unwrap_or(&3600.0);
    (major, major / 5.0)
}

pub fn fmt_ruler(t: f64, major: f64) -> String {
    if major >= 1.0 {
        let m = (t / 60.0).floor() as i64;
        let s = t - m as f64 * 60.0;
        format!("{}:{:02}", m, s.round() as i64)
    } else if major >= 0.01 {
        format!("{:.2}", t)
    } else {
        format!("{:.4}", t)
    }
}

/// Draw a time ruler in `r` for view (v0,v1).
pub fn draw_ruler(p: &egui::Painter, r: Rect, v0: f64, v1: f64) {
    p.rect_filled(r, 0.0, theme::ruler());
    let span = (v1 - v0).max(1e-9);
    let (major, minor) = ruler_ticks(span, r.width());
    let x_of = |t: f64| r.left() + ((t - v0) / span) as f32 * r.width();
    let mut t = (v0 / minor).floor() * minor;
    while t <= v1 {
        let x = x_of(t);
        let is_major = ((t / major).round() * major - t).abs() < minor * 0.01;
        let h = if is_major { 9.0 } else { 4.0 };
        p.line_segment(
            [pos2(x, r.bottom() - h), pos2(x, r.bottom())],
            Stroke::new(1.0, crate::theme::gray(if is_major { 150 } else { 95 })),
        );
        if is_major {
            p.text(
                pos2(x + 3.0, r.top() + 2.0),
                egui::Align2::LEFT_TOP,
                fmt_ruler(t, major),
                FontId::proportional(10.0),
                theme::text_dim(),
            );
        }
        t += minor;
    }
    p.line_segment(
        [r.left_bottom(), r.right_bottom()],
        Stroke::new(1.0, crate::theme::gray(20)),
    );
}

fn spectro_color(v: f32) -> Color32 {
    // dark -> blue -> purple -> red -> orange -> yellow -> white
    let stops: [(f32, [f32; 3]); 7] = [
        (0.0, [0.0, 0.0, 0.0]),
        (0.18, [10.0, 10.0, 80.0]),
        (0.38, [100.0, 20.0, 140.0]),
        (0.58, [200.0, 40.0, 70.0]),
        (0.76, [245.0, 140.0, 30.0]),
        (0.9, [255.0, 230.0, 90.0]),
        (1.0, [255.0, 255.0, 240.0]),
    ];
    let v = v.clamp(0.0, 1.0);
    for w in stops.windows(2) {
        let (a, ca) = w[0];
        let (b, cb) = w[1];
        if v <= b {
            let t = (v - a) / (b - a);
            return Color32::from_rgb(
                (ca[0] + (cb[0] - ca[0]) * t) as u8,
                (ca[1] + (cb[1] - ca[1]) * t) as u8,
                (ca[2] + (cb[2] - ca[2]) * t) as u8,
            );
        }
    }
    Color32::WHITE
}

/// A spectrogram picture of channel `ch` (None: all channels mixed) over the view, `w` x `h`
/// pixels, on `axis`. Columns are worked out at most about a thousand at a time (the picture is
/// stretched smoothly), and a row that spans several FFT bins shows the loudest of them, so
/// the high end of a logarithmic scale doesn't alias.
#[allow(clippy::too_many_arguments)]
pub fn spectrogram_image(
    d: &AudioData,
    ch: Option<usize>,
    v0: f64,
    v1: f64,
    w: usize,
    h: usize,
    axis: FreqAxis,
    sp: SpecParams,
) -> egui::ColorImage {
    let n = sp.n.next_power_of_two().clamp(256, 32768);
    let cols = w.min(if n > 4096 { 768 } else { 1024 }).max(1);
    let mut spec = dsp::Spectrum::with_window(n, sp.window);
    let mut mags = Vec::new();
    let mut img = egui::ColorImage::filled(
        [cols, h],
        if sp.palette == 1 {
            pitch_color(0.0)
        } else {
            Color32::BLACK
        },
    );
    let r = d.rate as f64;
    if d.channels() == 0 || d.frames() == 0 {
        return img;
    }
    // the samples needed (with half a window either side), mixed to mono when asked
    let a0 = ((v0 * r) as isize - n as isize).max(0) as usize;
    let b0 = (((v1 * r) as isize + n as isize).max(0) as usize).min(d.frames());
    let mixed: Vec<f32>;
    let (src, base): (&[f32], usize) = match ch {
        Some(c) => (&d.ch[c.min(d.channels() - 1)], 0),
        None => {
            let k = 1.0 / d.channels() as f32;
            mixed = (a0..b0.max(a0))
                .map(|i| d.ch.iter().map(|c| c[i]).sum::<f32>() * k)
                .collect();
            (&mixed, a0)
        }
    };
    let bins = n / 2;
    let hz_per_bin = (r / n as f64) as f32;
    // each row's bin range (top and bottom edge) on the frequency axis
    let rows: Vec<(f32, f32)> = (0..h)
        .map(|y| {
            let top = axis.freq(1.0 - y as f32 / h as f32) / hz_per_bin;
            let bot = axis.freq(1.0 - (y as f32 + 1.0) / h as f32) / hz_per_bin;
            (bot.min(top), top.max(bot))
        })
        .collect();
    let range = sp.db_range.max(10.0);
    for x in 0..cols {
        let t = v0 + (x as f64 + 0.5) / cols as f64 * (v1 - v0);
        let center = (t * r) as isize - base as isize;
        if center + (base as isize) < 0 || (center + base as isize) as usize >= d.frames() {
            continue;
        }
        spec.mags(src, center, &mut mags);
        if mags.len() < bins {
            continue;
        }
        for (y, (b_lo, b_hi)) in rows.iter().enumerate() {
            let m = if b_hi - b_lo >= 1.5 {
                let i0 = (b_lo.ceil() as usize).min(bins - 1);
                let i1 = (b_hi.floor() as usize).min(bins - 1);
                mags[i0..=i1.max(i0)].iter().fold(0.0f32, |a, v| a.max(*v))
            } else {
                let b = 0.5 * (b_lo + b_hi);
                let i = (b.floor().max(0.0) as usize).min(bins - 2);
                let fr = (b - i as f32).clamp(0.0, 1.0);
                mags[i] * (1.0 - fr) + mags[i + 1] * fr
            };
            let v = ((audio::db(m) + range) / range).clamp(0.0, 1.0);
            img.pixels[y * cols + x] = if sp.palette == 1 {
                pitch_color(v.powf(1.15))
            } else {
                spectro_color(v.powf(1.3))
            };
        }
    }
    img
}

/// Draw the min/max waveform of channel `c` of `d` in `lane`.
pub fn draw_wave(
    p: &egui::Painter,
    lane: Rect,
    d: &AudioData,
    peaks: &Peaks,
    c: usize,
    v0: f64,
    v1: f64,
    amp: f32,
    color: Color32,
) {
    let span = (v1 - v0).max(1e-9);
    let rate = d.rate as f64;
    let w = lane.width().max(1.0) as usize;
    let mid = lane.center().y;
    let half = lane.height() * 0.5 - 1.0;
    let fpp = span * rate / w as f64; // frames per pixel
    let yv = |v: f32| mid - (v * amp).clamp(-1.0, 1.0) * half;
    if fpp < 1.5 {
        // draw sample polyline
        let a = ((v0 * rate).floor() as i64 - 1).max(0) as usize;
        let b = (((v1 * rate).ceil() as i64) + 1)
            .min(d.frames() as i64)
            .max(0) as usize;
        let mut pts = Vec::with_capacity(b.saturating_sub(a));
        for i in a..b {
            let t = i as f64 / rate;
            let x = lane.left() + ((t - v0) / span) as f32 * lane.width();
            pts.push(pos2(x, yv(d.ch[c][i])));
        }
        if pts.len() >= 2 {
            p.add(egui::Shape::line(pts.clone(), Stroke::new(1.3, color)));
        }
        if fpp < 0.34 {
            // a dot on every sample (the snap targets at this zoom)
            let s = if fpp < 0.12 { 5.0 } else { 4.0 };
            for q in pts {
                p.rect_filled(Rect::from_center_size(q, vec2(s, s)), 0.0, color);
            }
        }
        return;
    }
    // work in physical pixels so columns line up with the screen grid (no banding)
    let ppp = p.pixels_per_point().max(0.5);
    let w = (lane.width() * ppp).max(1.0) as usize;
    let left = (lane.left() * ppp).round() / ppp;
    let colw = 1.0 / ppp;
    let mut rects = Vec::with_capacity(w);
    for x in 0..w {
        let t0 = v0 + x as f64 * span / w as f64;
        let t1 = t0 + span / w as f64;
        let fa = (t0 * rate) as i64;
        let fb = (t1 * rate).ceil() as i64;
        if fb <= 0 || fa >= d.frames() as i64 {
            continue;
        }
        if let Some((lo, hi)) = peaks.range(d, c, fa.max(0) as usize, fb.max(0) as usize) {
            let y0 = yv(hi);
            let y1 = yv(lo).max(y0 + colw);
            let xx = left + x as f32 * colw;
            rects.push(egui::Shape::rect_filled(
                Rect::from_min_max(pos2(xx, y0), pos2(xx + colw, y1)),
                0.0,
                color,
            ));
        }
    }
    p.extend(rects);
}

impl App {
    pub fn waveform_view(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        let p = ui.painter_at(full);
        p.rect_filled(full, 0.0, theme::wave_bg());
        let Some(fid) = self.current else {
            self.empty_editor(ui, full);
            return;
        };
        if self.file(fid).is_none() {
            self.current = None;
            return;
        }
        // file switcher strip
        let strip = Rect::from_min_size(full.min, vec2(full.width(), 24.0));
        p.rect_filled(strip, 0.0, crate::theme::gray(33));
        {
            let mut sui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(strip.shrink2(vec2(6.0, 1.0)))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            let names: Vec<(u64, String)> =
                self.files.iter().map(|f| (f.id, f.name.clone())).collect();
            let cur_name = self.cur().map(|f| f.name.clone()).unwrap_or_default();
            let mut sel = fid;
            egui::ComboBox::from_id_salt("file_switch")
                .selected_text(cur_name)
                .width(220.0)
                .show_ui(&mut sui, |ui| {
                    for (id, n) in &names {
                        ui.selectable_value(&mut sel, *id, n);
                    }
                });
            if sel != fid {
                self.stop();
                self.current = Some(sel);
                return;
            }
            let hud = self.drag_state.hud_gain;
            sui.add_space(12.0);
            sui.label(egui::RichText::new("HUD").color(theme::text_dim()).small());
            let mut g = hud;
            let kr = theme::knob(
                &mut sui,
                &mut g,
                -24.0,
                24.0,
                20.0,
                "Adjust amplitude: drag, release to apply",
            );
            self.drag_state.hud_gain = g;
            sui.label(
                egui::RichText::new(format!("{:+.1} dB", g))
                    .color(theme::time_blue())
                    .monospace(),
            );
            if kr.drag_stopped() && g.abs() > 0.05 {
                self.process(&format!("Amplify {:+.1} dB", g), |d, a, b| {
                    dsp::apply_chain(d, a, b, &[EffectSlot::new(Effect::Amplify { gain_db: g })]);
                });
                self.drag_state.hud_gain = 0.0;
            }
            sui.add_space(12.0);
            let mut spec = self.spectral;
            if theme::icon_button(
                &mut sui,
                Icon::Spectral,
                spec,
                "Spectral Frequency Display (Shift+D)",
            )
            .clicked()
            {
                spec = !spec;
            }
            self.spectral = spec;
        }

        let (data, peaks_ok) = {
            let f = self.cur_mut().unwrap();
            f.refresh_peaks();
            (f.data.clone(), true)
        };
        let _ = peaks_ok;
        let dur = data.duration().max(1e-6);
        let nav = Rect::from_min_size(pos2(full.left(), strip.bottom()), vec2(full.width(), 30.0));
        // a wider scale column while the pitch display shows note names
        let scale_w = if self.pitch_display { 66.0 } else { 44.0 };
        let ruler = Rect::from_min_max(
            pos2(full.left(), nav.bottom()),
            pos2(full.right() - scale_w, nav.bottom() + 22.0),
        );
        let lanes_all = Rect::from_min_max(
            pos2(full.left(), ruler.bottom()),
            pos2(full.right() - scale_w, full.bottom()),
        );
        let scale = Rect::from_min_max(pos2(lanes_all.right(), ruler.top()), full.max);
        // The editor stacks the waveform, the Spectral Frequency Display and the Spectral Pitch
        // Display (each shown by its button), like the original; the borders between them can be
        // dragged (the waveform can be closed all the way, leaving a spectral display alone).
        let gap = 5.0;
        let total_h = lanes_all.height().max(1.0);
        let (show_spec, show_pitch) = (self.spectral, self.pitch_display);
        let fw = self.drag_state.split_wave.clamp(0.0, 0.9);
        let fp = self.drag_state.split_pitch.clamp(0.1, 0.9);
        let hw = if show_spec || show_pitch {
            (total_h * fw).floor()
        } else {
            total_h
        };
        let wave_area =
            Rect::from_min_max(lanes_all.min, pos2(lanes_all.right(), lanes_all.top() + hw));
        let rest_top = wave_area.bottom() + if show_spec || show_pitch { gap } else { 0.0 };
        let (spec_area, pitch_area) = match (show_spec, show_pitch) {
            (false, false) => (None, None),
            (true, false) => (
                Some(Rect::from_min_max(
                    pos2(lanes_all.left(), rest_top),
                    lanes_all.max,
                )),
                None,
            ),
            (false, true) => (
                None,
                Some(Rect::from_min_max(
                    pos2(lanes_all.left(), rest_top),
                    lanes_all.max,
                )),
            ),
            (true, true) => {
                let hp = ((lanes_all.bottom() - rest_top) * fp).floor();
                let split = lanes_all.bottom() - hp;
                (
                    Some(Rect::from_min_max(
                        pos2(lanes_all.left(), rest_top),
                        pos2(lanes_all.right(), split - gap),
                    )),
                    Some(Rect::from_min_max(
                        pos2(lanes_all.left(), split),
                        lanes_all.max,
                    )),
                )
            }
        };
        // the dividers: waveform | spectral display(s), spectral | pitch (dragged further down,
        // after the editor's own click area, so they sit on top of it)
        let mut split_bars: Vec<(Rect, u8)> = vec![];
        if show_spec || show_pitch {
            split_bars.push((
                Rect::from_min_max(
                    pos2(lanes_all.left(), wave_area.bottom()),
                    pos2(scale.right(), wave_area.bottom() + gap),
                ),
                1,
            ));
        }
        if let (Some(sa), Some(_)) = (spec_area, pitch_area) {
            split_bars.push((
                Rect::from_min_max(
                    pos2(lanes_all.left(), sa.bottom()),
                    pos2(scale.right(), sa.bottom() + gap),
                ),
                2,
            ));
        }

        let (mut view, mut selection, mut cursor) = {
            let f = self.cur().unwrap();
            (f.view, f.selection, f.cursor)
        };
        if view.1 <= view.0 {
            view = (0.0, dur);
        }
        let span = view.1 - view.0;
        let v0 = view.0;
        let x_of = move |t: f64| lanes_all.left() + ((t - v0) / span) as f32 * lanes_all.width();
        let srate = data.rate.max(1) as f64;
        // times snap to the file's sample points (exact at close zoom)
        let t_of = move |x: f32| {
            let t = v0 + ((x - lanes_all.left()) / lanes_all.width()) as f64 * span;
            (t * srate).round() / srate
        };

        // ---------- navigator
        p.rect_filled(nav, 0.0, crate::theme::gray(24));
        {
            let f = self.cur().unwrap();
            // the whole file, drawn like the editor's own waveform: one lane per channel and
            // one column per screen pixel (not per point), from the same peak data
            let nch = data.channels().max(1);
            let lh = (nav.height() - 2.0) / nch as f32;
            for c in 0..data.channels() {
                let lane = Rect::from_min_size(
                    pos2(nav.left(), nav.top() + 1.0 + lh * c as f32),
                    vec2(nav.width(), lh),
                );
                draw_wave(
                    &p,
                    lane,
                    &data,
                    &f.peaks,
                    c,
                    0.0,
                    dur,
                    1.0,
                    Color32::from_rgb(28, 160, 160),
                );
            }
            let vx0 = nav.left() + (view.0 / dur) as f32 * nav.width();
            let vx1 = nav.left() + (view.1 / dur) as f32 * nav.width();
            let vr = Rect::from_min_max(
                pos2(vx0, nav.top() + 1.0),
                pos2(vx1.max(vx0 + 3.0), nav.bottom() - 1.0),
            );
            p.rect_filled(vr, 0.0, crate::theme::gray_a(255, 30));
            p.rect_stroke(
                vr,
                0.0,
                Stroke::new(1.0, crate::theme::gray(170)),
                egui::StrokeKind::Inside,
            );
            let cx = nav.left() + (cursor / dur) as f32 * nav.width();
            p.line_segment(
                [pos2(cx, nav.top()), pos2(cx, nav.bottom())],
                Stroke::new(1.0, theme::playhead()),
            );
            let nresp = ui.interact(nav, ui.scope_id().with("nav"), Sense::click_and_drag());
            if let Some(pp) = nresp.interact_pointer_pos() {
                let t = ((pp.x - nav.left()) / nav.width()) as f64 * dur;
                if nresp.drag_started() {
                    self.drag_state.drag = WaveDrag::Nav {
                        grab: if t >= view.0 && t <= view.1 {
                            t - view.0
                        } else {
                            span / 2.0
                        },
                    };
                }
                if nresp.dragged() {
                    if let WaveDrag::Nav { grab } = self.drag_state.drag {
                        let a = (t - grab).clamp(0.0, (dur - span).max(0.0));
                        view = (a, a + span);
                    }
                }
                if nresp.clicked() {
                    let a = (t - span / 2.0).clamp(0.0, (dur - span).max(0.0));
                    view = (a, a + span);
                }
            }
            if nresp.drag_stopped() {
                self.drag_state.drag = WaveDrag::None;
            }
        }

        // ---------- ruler
        draw_ruler(&p, ruler, view.0, view.1);
        let rresp = ui.interact(ruler, ui.scope_id().with("ruler"), Sense::click_and_drag());
        let markers = self.cur().unwrap().markers.clone();
        // marker flags
        let mut marker_hit: Option<usize> = None;
        for (i, m) in markers.iter().enumerate() {
            let x = x_of(m.time);
            if x < ruler.left() - 50.0 || x > ruler.right() {
                continue;
            }
            let flag = Rect::from_min_size(pos2(x, ruler.top() + 11.0), vec2(9.0, 10.0));
            p.add(egui::Shape::convex_polygon(
                vec![
                    flag.left_top(),
                    flag.right_top() + vec2(0.0, 5.0),
                    flag.left_bottom(),
                ],
                theme::marker(),
                Stroke::NONE,
            ));
            p.text(
                pos2(x + 10.0, ruler.top() + 12.0),
                egui::Align2::LEFT_TOP,
                &m.name,
                FontId::proportional(9.5),
                theme::marker(),
            );
            if m.duration > 0.0 {
                let x2 = x_of(m.time + m.duration);
                p.line_segment(
                    [
                        pos2(x, ruler.bottom() - 1.0),
                        pos2(x2, ruler.bottom() - 1.0),
                    ],
                    Stroke::new(2.0, theme::marker()),
                );
            }
            if let Some(hp) = rresp.hover_pos() {
                if (hp.x - x).abs() < 6.0 && hp.y > ruler.top() + 9.0 {
                    marker_hit = Some(i);
                }
            }
        }
        if rresp.drag_started() {
            self.drag_state.drag = match marker_hit {
                Some(i) => WaveDrag::Marker { idx: i },
                None => WaveDrag::Scrub,
            };
        }
        if let Some(pp) = rresp.interact_pointer_pos() {
            let t = t_of(pp.x).clamp(0.0, dur);
            match self.drag_state.drag {
                WaveDrag::Scrub if rresp.dragged() => {
                    cursor = t;
                    // keep scrolling while the playhead is dragged past an edge
                    let edge = 16.0;
                    if pp.x > ruler.right() - edge && view.1 < dur {
                        let d =
                            (span * 0.02 * (1.0 + ((pp.x - ruler.right() + edge) / edge) as f64))
                                .min(dur - view.1);
                        view = (view.0 + d, view.1 + d);
                        cursor = cursor.max(view.1 - span * 0.02).min(dur);
                        ui.ctx().request_repaint();
                    } else if pp.x < ruler.left() + edge && view.0 > 0.0 {
                        let d =
                            (span * 0.02 * (1.0 + ((ruler.left() + edge - pp.x) / edge) as f64))
                                .min(view.0);
                        view = (view.0 - d, view.1 - d);
                        cursor = cursor.min(view.0 + span * 0.02).max(0.0);
                        ui.ctx().request_repaint();
                    }
                }
                WaveDrag::Marker { idx } if rresp.dragged() => {
                    if let Some(m) = self.cur_mut().unwrap().markers.get_mut(idx) {
                        m.time = t;
                    }
                }
                _ => {}
            }
            if rresp.clicked() {
                cursor = match marker_hit {
                    Some(i) => markers[i].time,
                    None => t,
                };
            }
        }
        if matches!(self.drag_state.drag, WaveDrag::Scrub)
            && (rresp.drag_started() || rresp.dragged())
        {
            self.scrub_to(Some(cursor));
        }
        if rresp.drag_stopped() {
            if let WaveDrag::Marker { .. } = self.drag_state.drag {
                self.cur_mut()
                    .unwrap()
                    .markers
                    .sort_by(|a, b| a.time.total_cmp(&b.time));
            }
            self.drag_state.drag = WaveDrag::None;
            self.scrub_to(None);
        }
        if let Some(i) = marker_hit {
            rresp.clone().on_hover_text(format!(
                "{} — drag to move, right-click to delete",
                markers[i].name
            ));
            if rresp.secondary_clicked() {
                self.cur_mut().unwrap().markers.remove(i);
            }
        }

        // ---------- lanes
        let chs = data.channels().max(1);
        let amp = self.drag_state.amp_zoom;
        let lane_h = wave_area.height() / chs as f32;
        p.rect_filled(scale, 0.0, crate::theme::gray(28));
        p.text(
            pos2(scale.center().x, ruler.center().y),
            egui::Align2::CENTER_CENTER,
            if wave_area.height() >= 8.0 { "dB" } else { "" },
            FontId::proportional(10.0),
            theme::text_dim(),
        );
        for c in 0..if wave_area.height() >= 8.0 { chs } else { 0 } {
            let lane = Rect::from_min_size(
                pos2(wave_area.left(), wave_area.top() + c as f32 * lane_h),
                vec2(wave_area.width(), lane_h),
            );
            // selection background
            if let Some((a, b)) = selection {
                let xa = x_of(a).max(lane.left());
                let xb = x_of(b).min(lane.right());
                if xb > xa {
                    p.rect_filled(
                        Rect::from_min_max(pos2(xa, lane.top()), pos2(xb, lane.bottom())),
                        0.0,
                        theme::wave_sel_bg(),
                    );
                }
            }
            // grid and dB scale: labels never overlap each other or leave their lane, however
            // far the amplitude is zoomed or however short the lane is
            let mid = lane.center().y;
            let half = lane.height() * 0.5 - 1.0;
            let sp = p.with_clip_rect(Rect::from_min_max(
                pos2(lane.left(), lane.top()),
                pos2(scale.right(), lane.bottom()),
            ));
            let mut placed: Vec<f32> = vec![mid];
            sp.text(
                pos2(scale.right() - 4.0, mid),
                egui::Align2::RIGHT_CENTER,
                "-\u{221e}",
                FontId::proportional(9.0),
                theme::text_dim(),
            );
            for (dbv, label) in [
                (0.0f32, "0"),
                (-3.0, "-3"),
                (-6.0, "-6"),
                (-9.0, "-9"),
                (-12.0, "-12"),
                (-18.0, "-18"),
                (-24.0, "-24"),
                (-30.0, "-30"),
                (-36.0, "-36"),
                (-48.0, "-48"),
            ] {
                let a = audio::undb(dbv) * amp;
                if a > 1.0 {
                    continue;
                }
                for sgn in [-1.0f32, 1.0] {
                    let y = mid + sgn * a * half;
                    if y < lane.top() + 6.0
                        || y > lane.bottom() - 6.0
                        || placed.iter().any(|q| (q - y).abs() < 11.0)
                    {
                        continue;
                    }
                    placed.push(y);
                    p.line_segment(
                        [pos2(lane.left(), y), pos2(lane.right(), y)],
                        Stroke::new(1.0, crate::theme::gray_a(255, 10)),
                    );
                    sp.text(
                        pos2(scale.right() - 4.0, y),
                        egui::Align2::RIGHT_CENTER,
                        label,
                        FontId::proportional(9.0),
                        theme::text_dim(),
                    );
                }
            }
            let label = if chs == 2 {
                if c == 0 { "L" } else { "R" }
            } else if chs == 1 {
                "M"
            } else {
                ""
            };
            sp.text(
                pos2(scale.left() + 4.0, lane.top() + 4.0),
                egui::Align2::LEFT_TOP,
                if label.is_empty() {
                    format!("{}", c + 1)
                } else {
                    label.to_string()
                },
                FontId::proportional(11.0),
                theme::text(),
            );
            // the red zero (-inf dB) reference line runs under the waveform
            let zy = lane.center().y;
            p.line_segment(
                [pos2(lane.left(), zy), pos2(lane.right(), zy)],
                Stroke::new(1.0, theme::zero_line()),
            );
            {
                let f = self.cur().unwrap();
                draw_wave(
                    &p.with_clip_rect(lane),
                    lane,
                    &data,
                    &f.peaks,
                    c,
                    view.0,
                    view.1,
                    amp,
                    theme::wave(),
                );
            }
            if c + 1 < chs {
                p.line_segment(
                    [
                        pos2(lane.left(), lane.bottom()),
                        pos2(scale.right(), lane.bottom()),
                    ],
                    Stroke::new(1.0, crate::theme::gray(45)),
                );
            }
        }

        // ---------- spectral frequency display
        let mut freq_wheel = false;
        let nyq = data.rate as f32 / 2.0;
        let log_scale = self.prefs.spec_log_scale;
        // spec_hi 0 means "the whole range" (0 Hz to the top)
        if self.drag_state.spec_hi <= 0.0
            || self.drag_state.spec_hi <= self.drag_state.spec_lo
            || self.drag_state.spec_hi > nyq
            || self.drag_state.spec_lo < 0.0
        {
            self.drag_state.spec_lo = 0.0;
            self.drag_state.spec_hi = nyq.max(100.0);
        }
        let axis = FreqAxis {
            lo: self.drag_state.spec_lo,
            hi: self.drag_state.spec_hi,
            nyq,
            log: log_scale,
        };
        let spec_params = SpecParams {
            n: self.prefs.spec_resolution.clamp(256, 16384) as usize,
            db_range: self.prefs.spec_db_range.clamp(20.0, 240.0),
            window: self.prefs.spec_window,
            palette: 0,
        };
        let spec_lanes: Vec<Rect> = match spec_area {
            Some(sa) => {
                let lh = sa.height() / chs as f32;
                (0..chs)
                    .map(|c| {
                        Rect::from_min_size(
                            pos2(sa.left(), sa.top() + c as f32 * lh),
                            vec2(sa.width(), lh),
                        )
                    })
                    .collect()
            }
            None => vec![],
        };
        if let Some(sa) = spec_area.filter(|sa| sa.height() >= 8.0) {
            let w = (sa.width() as usize).clamp(16, 4096);
            let lh = sa.height() / chs as f32;
            let h = (lh as usize).clamp(16, 1024);
            let mut hk = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(
                &(
                    Arc::as_ptr(&data) as usize,
                    view.0.to_bits(),
                    view.1.to_bits(),
                    w,
                    h,
                    axis.lo.to_bits(),
                    axis.hi.to_bits(),
                    axis.log,
                    spec_params.n,
                    spec_params.db_range.to_bits(),
                    spec_params.window,
                    chs,
                ),
                &mut hk,
            );
            let key = std::hash::Hasher::finish(&hk);
            {
                let (d, v) = (data.clone(), view);
                self.spec_cache.want(ui.ctx(), "spec", key, view, move || {
                    (0..chs)
                        .map(|c| spectrogram_image(&d, Some(c), v.0, v.1, w, h, axis, spec_params))
                        .collect()
                });
            }
            let spec_sel = self.cur().and_then(|f| f.spec_sel.clone());
            // free-form selections (lasso, paintbrush, inverse) are drawn as a smooth picture of
            // how strongly each pixel is selected, rebuilt only when the selection or view changes
            let sel_img: Option<egui::TextureId> = match (&spec_sel, spec_lanes.first()) {
                (Some(shape), Some(l0)) if !matches!(shape, SpecShape::Rect { .. }) => {
                    let (iw, ih) = (
                        (l0.width() as usize).clamp(1, 4096),
                        (l0.height() as usize).clamp(1, 2048),
                    );
                    let mut hsh = std::collections::hash_map::DefaultHasher::new();
                    std::hash::Hash::hash(
                        &(
                            shape.fingerprint(),
                            view.0.to_bits(),
                            view.1.to_bits(),
                            iw,
                            ih,
                            axis.lo.to_bits(),
                            axis.hi.to_bits(),
                            axis.log,
                        ),
                        &mut hsh,
                    );
                    let k = std::hash::Hasher::finish(&hsh);
                    if self.drag_state.sel_tex.as_ref().map(|(kk, _)| *kk) != Some(k) {
                        let img = selection_image(shape, iw, ih, view, axis);
                        let tex =
                            ui.ctx()
                                .load_texture("spec_sel", img, egui::TextureOptions::LINEAR);
                        self.drag_state.sel_tex = Some((k, tex));
                    }
                    self.drag_state.sel_tex.as_ref().map(|(_, t)| t.id())
                }
                _ => None,
            };
            for (c, lane) in spec_lanes.iter().enumerate() {
                let lane = *lane;
                self.spec_cache.draw(&p, c, lane, view);
                if spec_sel.is_none() {
                    if let Some((a, b)) = selection {
                        let xa = x_of(a).max(lane.left());
                        let xb = x_of(b).min(lane.right());
                        if xb > xa {
                            p.rect_filled(
                                Rect::from_min_max(pos2(xa, lane.top()), pos2(xb, lane.bottom())),
                                0.0,
                                crate::theme::gray_a(255, 40),
                            );
                        }
                    }
                }
                if let Some(shape) = &spec_sel {
                    if let Some(tex) = sel_img {
                        p.with_clip_rect(lane).image(
                            tex,
                            lane,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    draw_spec_sel(&p.with_clip_rect(lane), lane, shape, &x_of, axis);
                }
                if c + 1 < chs {
                    p.line_segment(
                        [
                            pos2(lane.left(), lane.bottom()),
                            pos2(scale.right(), lane.bottom()),
                        ],
                        Stroke::new(1.0, crate::theme::gray(60)),
                    );
                }
                // frequency scale: round values, never crowded (top down)
                let sp = p.with_clip_rect(Rect::from_min_max(
                    pos2(scale.left(), lane.top()),
                    pos2(scale.right(), lane.bottom()),
                ));
                let mut last_y = f32::MIN;
                let mut marks: Vec<f32> = vec![
                    20.0, 30.0, 40.0, 50.0, 60.0, 80.0, 100.0, 150.0, 200.0, 300.0, 400.0, 500.0,
                    600.0, 800.0, 1000.0, 1500.0, 2000.0, 3000.0, 4000.0, 5000.0, 6000.0, 8000.0,
                    10000.0, 12000.0, 14000.0, 16000.0, 18000.0, 20000.0, 22000.0, 24000.0,
                    30000.0, 40000.0, 48000.0,
                ];
                marks.retain(|hz| *hz >= axis.lo && *hz <= axis.hi);
                marks.sort_by(|a, b| b.total_cmp(a));
                for hz in marks {
                    let y = axis.y(lane, hz);
                    if y > lane.bottom() - 5.0 || y < lane.top() + 19.0 || (y - last_y) < 13.0 {
                        continue;
                    }
                    last_y = y;
                    let label = if hz >= 1000.0 {
                        format!("{}k", hz / 1000.0)
                    } else {
                        format!("{hz:.0}")
                    };
                    let major = hz == 100.0 || hz == 1000.0 || hz == 10000.0;
                    sp.line_segment(
                        [pos2(scale.left(), y), pos2(scale.left() + 4.0, y)],
                        Stroke::new(1.0, theme::text_dim()),
                    );
                    sp.text(
                        pos2(scale.right() - 4.0, y),
                        egui::Align2::RIGHT_CENTER,
                        label,
                        FontId::proportional(9.0),
                        if major {
                            theme::text()
                        } else {
                            theme::text_dim()
                        },
                    );
                }
                sp.text(
                    pos2(scale.right() - 4.0, lane.top() + 6.0),
                    egui::Align2::RIGHT_CENTER,
                    "Hz",
                    FontId::proportional(9.0),
                    theme::text_dim(),
                );
            }
            let hz_scale = Rect::from_min_max(
                pos2(scale.left(), sa.top()),
                pos2(scale.right(), sa.bottom()),
            );
            let lane_of = |y: f32| {
                spec_lanes
                    .iter()
                    .copied()
                    .find(|l| y >= l.top() && y <= l.bottom())
                    .or(spec_lanes.first().copied())
            };
            // the wheel over the frequency scale (or Alt+wheel over the display) zooms the
            // frequencies around the pointer, keeping the one under it in place
            if let Some(hp) = ui.ctx().pointer_hover_pos() {
                let (sy, zd, alt) =
                    ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta(), i.modifiers.alt));
                let over_scale = hz_scale.contains(hp) && ui.rect_contains_pointer(hz_scale);
                let over_spec = sa.contains(hp) && ui.rect_contains_pointer(sa) && alt;
                if over_scale || over_spec {
                    let k = if zd != 1.0 {
                        1.0 / zd
                    } else if sy.abs() > 0.1 {
                        (0.996f32).powf(sy * self.prefs.zoom_factor.clamp(0.25, 4.0))
                    } else {
                        1.0
                    };
                    if k != 1.0 {
                        if let Some(lane) = lane_of(hp.y) {
                            let pf = (lane.bottom() - hp.y) / lane.height().max(1.0);
                            let (lo, hi) = axis.zoomed(pf, k);
                            self.drag_state.spec_lo = lo;
                            self.drag_state.spec_hi = hi;
                        }
                    }
                    freq_wheel = true;
                }
            }
            let sresp = ui.interact(
                hz_scale,
                ui.scope_id().with("hzscale"),
                Sense::click_and_drag(),
            );
            let lane_h = spec_lanes
                .first()
                .map(|l| l.height())
                .unwrap_or(1.0)
                .max(1.0);
            // drag the scale (or middle-drag on the display) to move through the frequencies
            let mut dy = 0.0f32;
            if sresp.dragged_by(egui::PointerButton::Primary) {
                dy = sresp.drag_delta().y;
            }
            let (mid, md) = ui.input(|i| (i.pointer.middle_down(), i.pointer.delta()));
            if mid
                && ui
                    .ctx()
                    .pointer_hover_pos()
                    .map(|h| sa.contains(h))
                    .unwrap_or(false)
            {
                dy = md.y;
            }
            if dy != 0.0 {
                let (lo, hi) = axis.scrolled(dy / lane_h);
                self.drag_state.spec_lo = lo;
                self.drag_state.spec_hi = hi;
            }
            // right-drag on the scale zooms to the frequencies dragged over
            if sresp.drag_started_by(egui::PointerButton::Secondary) {
                self.drag_state.spec_rzoom = ui.input(|i| i.pointer.press_origin()).map(|o| o.y);
            }
            if let (Some(y0), Some(pp)) =
                (self.drag_state.spec_rzoom, ui.ctx().pointer_latest_pos())
            {
                let (a, b) = (y0.min(pp.y), y0.max(pp.y));
                p.rect_filled(
                    Rect::from_min_max(pos2(sa.left(), a), pos2(scale.right(), b)),
                    0.0,
                    Color32::from_rgba_unmultiplied(120, 170, 255, 40),
                );
                if sresp.drag_stopped() || !ui.input(|i| i.pointer.secondary_down()) {
                    if b - a > 4.0 {
                        if let Some(lane) = lane_of(y0) {
                            let f1 = axis.f_at(lane, a.clamp(lane.top(), lane.bottom()));
                            let f0 = axis.f_at(lane, b.clamp(lane.top(), lane.bottom()));
                            if f1 - f0 > 20.0 {
                                self.drag_state.spec_lo = f0.max(0.0);
                                self.drag_state.spec_hi = f1.min(nyq);
                            }
                        }
                    }
                    self.drag_state.spec_rzoom = None;
                }
            }
            if sresp.hovered() || sresp.dragged() {
                ui.ctx().set_cursor_icon(if sresp.dragged() {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::ResizeVertical
                });
            }
            let sresp = sresp.on_hover_text("Wheel: zoom frequency \u{2022} drag: scroll \u{2022} right-drag: zoom to a range \u{2022} double-click: whole range \u{2022} right-click: scale");
            if sresp.double_clicked() {
                self.drag_state.spec_lo = 0.0;
                self.drag_state.spec_hi = 0.0;
            }
            let mut log_choice = self.prefs.spec_log_scale;
            sresp.context_menu(|ui| {
                if ui.button("Zoom In").clicked() {
                    let (lo, hi) = axis.zoomed(0.5, 0.5);
                    self.drag_state.spec_lo = lo;
                    self.drag_state.spec_hi = hi;
                    ui.close();
                }
                if ui.button("Zoom Out").clicked() {
                    let (lo, hi) = axis.zoomed(0.5, 2.0);
                    self.drag_state.spec_lo = lo;
                    self.drag_state.spec_hi = hi;
                    ui.close();
                }
                if ui.button("Zoom Out Full").clicked() {
                    self.drag_state.spec_lo = 0.0;
                    self.drag_state.spec_hi = 0.0;
                    ui.close();
                }
                ui.separator();
                if theme::radio_item(ui, true, !log_choice, "Linear", "").clicked() {
                    log_choice = false;
                    ui.close();
                }
                if theme::radio_item(ui, true, log_choice, "Logarithmic", "").clicked() {
                    log_choice = true;
                    ui.close();
                }
            });
            if log_choice != self.prefs.spec_log_scale {
                self.prefs.spec_log_scale = log_choice;
                crate::prefs::save(&self.prefs);
            }
        }

        // ---------- spectral pitch display: the pitch of the sound over time, on a musical scale
        if let Some(pa) = pitch_area.filter(|pa| pa.height() >= 8.0) {
            // the pitch of the whole file is worked out once, in the background
            let key = Arc::as_ptr(&data) as usize;
            if self
                .drag_state
                .pitch_job
                .as_ref()
                .map(|j| j.0 != key)
                .unwrap_or(true)
            {
                let slot: PitchSlot = Arc::new(std::sync::Mutex::new(None));
                let out = slot.clone();
                let d = data.clone();
                std::thread::spawn(move || {
                    let r = analyse_pitch(&d);
                    if let Ok(mut g) = out.lock() {
                        *g = Some(r);
                    }
                });
                self.drag_state.pitch_job = Some((key, slot));
            }
            let track: Option<(f64, Vec<Option<f32>>)> = self
                .drag_state
                .pitch_job
                .as_ref()
                .and_then(|j| j.1.lock().ok().and_then(|g| g.clone()));
            if track.is_none() {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200));
            }
            let (dlo, dhi) = (
                self.prefs.pitch_min_hz.clamp(16.0, nyq * 0.5),
                self.prefs.pitch_max_hz.clamp(40.0, nyq),
            );
            if self.drag_state.pitch_hi <= 0.0
                || self.drag_state.pitch_hi <= self.drag_state.pitch_lo * 1.2
                || self.drag_state.pitch_hi > nyq
            {
                self.drag_state.pitch_lo = dlo;
                self.drag_state.pitch_hi = dhi.max(dlo * 2.0);
            }
            let pax = FreqAxis {
                lo: self.drag_state.pitch_lo,
                hi: self.drag_state.pitch_hi,
                nyq,
                log: true,
            };
            let pparams = SpecParams {
                n: self.prefs.pitch_resolution.clamp(1024, 32768) as usize,
                db_range: self.prefs.pitch_db_range.clamp(20.0, 200.0),
                window: 0,
                palette: 1,
            };
            let w = (pa.width() as usize).clamp(16, 2048);
            let h = (pa.height() as usize).clamp(16, 1024);
            let mut bh = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(
                &(
                    key,
                    view.0.to_bits(),
                    view.1.to_bits(),
                    w,
                    h,
                    pax.lo.to_bits(),
                    pax.hi.to_bits(),
                    pparams.n,
                    pparams.db_range.to_bits(),
                ),
                &mut bh,
            );
            let bkey = std::hash::Hasher::finish(&bh);
            {
                let (d, v) = (data.clone(), view);
                self.drag_state
                    .pitch_bg
                    .want(ui.ctx(), "pitch_bg", bkey, view, move || {
                        vec![spectrogram_image(&d, None, v.0, v.1, w, h, pax, pparams)]
                    });
            }
            self.drag_state.pitch_bg.draw(&p, 0, pa, view);
            let pc = p.with_clip_rect(pa);
            let yf = |f: f32| pax.y(pa, f);
            // a faint line on every semitone, and the note names on the scale (as many as fit)
            let m_lo = midi_of(pax.lo).ceil() as i32;
            let m_hi = midi_of(pax.hi).floor() as i32;
            let px_per_semi = pa.height() / (midi_of(pax.hi) - midi_of(pax.lo)).max(1.0);
            let step = if px_per_semi * 2.0 >= 13.0 {
                2
            } else if px_per_semi * 4.0 >= 13.0 {
                4
            } else if px_per_semi * 6.0 >= 13.0 {
                6
            } else {
                12
            };
            let sp = p.with_clip_rect(Rect::from_min_max(
                pos2(scale.left(), pa.top()),
                pos2(scale.right(), pa.bottom()),
            ));
            p.rect_filled(
                Rect::from_min_max(
                    pos2(scale.left(), pa.top()),
                    pos2(scale.right(), pa.bottom()),
                ),
                0.0,
                crate::theme::gray(28),
            );
            for m in m_lo..=m_hi {
                let y = yf(note_hz(m as f32));
                if px_per_semi >= 3.0 {
                    pc.line_segment(
                        [pos2(pa.left(), y), pos2(pa.right(), y)],
                        Stroke::new(
                            1.0,
                            Color32::from_rgba_unmultiplied(
                                255,
                                255,
                                255,
                                if m.rem_euclid(12) == 0 { 30 } else { 12 },
                            ),
                        ),
                    );
                }
                sp.line_segment(
                    [
                        pos2(scale.left(), y),
                        pos2(
                            scale.left()
                                + if (m - 69).rem_euclid(step) == 0 {
                                    5.0
                                } else {
                                    3.0
                                },
                            y,
                        ),
                    ],
                    Stroke::new(1.0, theme::text_dim()),
                );
                if (m - 69).rem_euclid(step) == 0 && y > pa.top() + 16.0 && y < pa.bottom() - 4.0 {
                    sp.text(
                        pos2(scale.left() + 8.0, y),
                        egui::Align2::LEFT_CENTER,
                        note_label(m),
                        FontId::proportional(9.5),
                        theme::text(),
                    );
                }
            }
            sp.text(
                pos2(scale.left() + 8.0, pa.top() + 7.0),
                egui::Align2::LEFT_CENTER,
                "Hz",
                FontId::proportional(9.5),
                theme::text_dim(),
            );
            // the detected pitch: a bright blue line (breaks where there is no clear pitch)
            match &track {
                None => {
                    pc.text(
                        pa.left_top() + vec2(8.0, 6.0),
                        egui::Align2::LEFT_TOP,
                        "Analyzing pitch...",
                        FontId::proportional(11.0),
                        theme::text_dim(),
                    );
                }
                Some((hop, tr)) => {
                    let i0 = ((view.0 / hop).floor().max(0.0)) as usize;
                    let i1 = ((view.1 / hop).ceil() as usize + 1).min(tr.len());
                    let step = ((i1.saturating_sub(i0)) / (pa.width() as usize).max(1)).max(1);
                    let blue = Stroke::new(2.0, Color32::from_rgb(70, 90, 255));
                    let mut seg: Vec<Pos2> = vec![];
                    let mut i = i0;
                    while i < i1 {
                        match tr[i] {
                            Some(hz) => {
                                let q = pos2(x_of(i as f64 * hop), yf(hz));
                                // a jump of more than a few semitones starts a new piece
                                if let Some(last) = seg.last() {
                                    if (last.y - q.y).abs() > px_per_semi * 4.0 + 2.0 {
                                        if seg.len() > 1 {
                                            pc.add(egui::Shape::line(
                                                std::mem::take(&mut seg),
                                                blue,
                                            ));
                                        }
                                        seg.clear();
                                    }
                                }
                                seg.push(q);
                            }
                            None => {
                                if seg.len() > 1 {
                                    pc.add(egui::Shape::line(std::mem::take(&mut seg), blue));
                                }
                                seg.clear();
                            }
                        }
                        i += step;
                    }
                    if seg.len() > 1 {
                        pc.add(egui::Shape::line(seg, blue));
                    }
                }
            }
            if let Some(hp) = ui.ctx().pointer_hover_pos().filter(|h| pa.contains(*h)) {
                let f = pax.f_at(pa, hp.y);
                let m = midi_of(f);
                let near = m.round() as i32;
                let cents = ((m - near as f32) * 100.0).round() as i32;
                let txt = format!(
                    "{}  {:+} cents  {f:.1} Hz",
                    note_label(near).split(' ').next().unwrap_or(""),
                    cents
                );
                let tr = Rect::from_min_size(pa.left_top() + vec2(6.0, 4.0), vec2(190.0, 16.0));
                pc.rect_filled(tr, 0.0, Color32::from_black_alpha(150));
                pc.text(
                    tr.left_center() + vec2(4.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    txt,
                    FontId::monospace(10.0),
                    crate::theme::gray(225),
                );
            }
            // its own scale: wheel zooms (around the pointer), drag scrolls, right-click menu
            let nscale = Rect::from_min_max(
                pos2(scale.left(), pa.top()),
                pos2(scale.right(), pa.bottom()),
            );
            let nresp = ui.interact(
                nscale,
                ui.scope_id().with("pitchscale"),
                Sense::click_and_drag(),
            );
            if let Some(hp) = ui.ctx().pointer_hover_pos() {
                let (sy, alt) = ui.input(|i| (i.smooth_scroll_delta.y, i.modifiers.alt));
                if (nscale.contains(hp) && ui.rect_contains_pointer(nscale))
                    || (pa.contains(hp) && alt && ui.rect_contains_pointer(pa))
                {
                    if sy.abs() > 0.1 {
                        let k = (0.996f32).powf(sy * self.prefs.zoom_factor.clamp(0.25, 4.0));
                        let pf = (pa.bottom() - hp.y) / pa.height().max(1.0);
                        let (lo, hi) = pax.zoomed(pf, k);
                        if hi / lo.max(1.0) > 1.5 {
                            self.drag_state.pitch_lo = lo.max(16.0);
                            self.drag_state.pitch_hi = hi;
                        }
                    }
                    freq_wheel = true;
                }
            }
            if nresp.dragged_by(egui::PointerButton::Primary) {
                let (lo, hi) = pax.scrolled(nresp.drag_delta().y / pa.height().max(1.0));
                if lo >= 16.0 {
                    self.drag_state.pitch_lo = lo;
                    self.drag_state.pitch_hi = hi;
                }
            }
            if nresp.hovered() || nresp.dragged() {
                ui.ctx().set_cursor_icon(if nresp.dragged() {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::ResizeVertical
                });
            }
            let nresp = nresp.on_hover_text("Wheel: zoom pitch \u{2022} drag: scroll \u{2022} double-click: default range \u{2022} right-click: zoom");
            if nresp.double_clicked() {
                self.drag_state.pitch_hi = 0.0;
            }
            nresp.context_menu(|ui| {
                if ui.button("Zoom In").clicked() {
                    let (lo, hi) = pax.zoomed(0.5, 0.5);
                    self.drag_state.pitch_lo = lo;
                    self.drag_state.pitch_hi = hi;
                    ui.close();
                }
                if ui.button("Zoom Out").clicked() {
                    let (lo, hi) = pax.zoomed(0.5, 2.0);
                    self.drag_state.pitch_lo = lo.max(16.0);
                    self.drag_state.pitch_hi = hi;
                    ui.close();
                }
                if ui.button("Zoom Out Full").clicked() {
                    self.drag_state.pitch_hi = 0.0;
                    ui.close();
                }
            });
        }

        // ---------- markers & playhead overlay
        for m in &markers {
            let x = x_of(m.time);
            if x >= lanes_all.left() && x <= lanes_all.right() {
                let mut y = lanes_all.top();
                while y < lanes_all.bottom() {
                    p.line_segment(
                        [pos2(x, y), pos2(x, (y + 4.0).min(lanes_all.bottom()))],
                        Stroke::new(1.0, theme::marker()),
                    );
                    y += 8.0;
                }
            }
        }

        // ---------- interaction on lanes
        let resp = ui.interact(
            lanes_all,
            ui.scope_id().with("lanes"),
            Sense::click_and_drag(),
        );
        // the borders between the waveform and the spectral displays (on top of the lanes)
        for (r, which) in split_bars {
            p.rect_filled(r, 0.0, crate::theme::gray(48));
            p.line_segment(
                [
                    pos2(r.center().x - 12.0, r.center().y),
                    pos2(r.center().x + 12.0, r.center().y),
                ],
                Stroke::new(1.0, crate::theme::gray(120)),
            );
            let bresp = ui.interact(
                r.expand2(vec2(0.0, 2.0)),
                ui.scope_id().with(("editor_split", which)),
                Sense::click_and_drag(),
            );
            if bresp.hovered() || bresp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if let (true, Some(pp)) = (bresp.dragged(), bresp.interact_pointer_pos()) {
                if which == 1 {
                    self.drag_state.split_wave =
                        ((pp.y - lanes_all.top()) / total_h).clamp(0.0, 0.9);
                } else {
                    let below = (lanes_all.bottom() - rest_top).max(1.0);
                    self.drag_state.split_pitch =
                        ((lanes_all.bottom() - pp.y) / below).clamp(0.1, 0.9);
                }
            }
            if bresp.double_clicked() {
                if which == 1 {
                    self.drag_state.split_wave = 0.35;
                } else {
                    self.drag_state.split_pitch = 0.45;
                }
            }
        }
        let shift = ui.input(|i| i.modifiers.shift);
        let mut spec_sel = self.cur().and_then(|f| f.spec_sel.clone());
        let mut spec_apply: Option<(SpecOp, &'static str)> = None;
        // the spectral tools act on the spectral display; anywhere else the pointer selects time
        let tool = self.tool;
        let in_spec = |q: Pos2| spec_area.map(|s| s.contains(q)).unwrap_or(false);
        let spec_tool = tool.is_spectral() && spec_area.is_some();
        let origin = ui.input(|i| i.pointer.press_origin());
        // a drag that began on the spectral display stays spectral until it ends (the press origin is gone on release)
        let spec_mode = spec_tool
            && (origin.map(in_spec).unwrap_or(false)
                || self.drag_state.spec_drag != SpecDrag::None);
        let lane_at = |y: f32| -> Option<Rect> {
            spec_lanes
                .iter()
                .copied()
                .find(|l| y >= l.top() - 0.5 && y <= l.bottom() + 0.5)
        };
        let tf = |q: Pos2| -> Option<(f64, f32, Rect)> {
            let sa = spec_area?;
            let y = q.y.clamp(sa.top(), sa.bottom() - 0.5);
            let l = lane_at(y).or_else(|| spec_lanes.first().copied())?;
            Some((
                t_of(q.x).clamp(0.0, dur),
                axis.f_at(l, y.clamp(l.top(), l.bottom())),
                l,
            ))
        };
        // marquee edges under the pointer: 1 left, 2 right, 4 bottom, 8 top
        let marquee_edges = |sel: &Option<SpecShape>, q: Pos2| -> u8 {
            let Some(SpecShape::Rect { .. }) = sel else {
                return 0;
            };
            let Some(l) = lane_at(q.y) else { return 0 };
            let (t0, t1, f0, f1) = sel.as_ref().map(|s| s.bounds()).unwrap_or_default();
            let r =
                Rect::from_min_max(pos2(x_of(t0), axis.y(l, f1)), pos2(x_of(t1), axis.y(l, f0)));
            if !r.expand(5.0).contains(q) {
                return 0;
            }
            let mut e = 0;
            if (q.x - r.left()).abs() < 5.0 {
                e |= 1;
            }
            if (q.x - r.right()).abs() < 5.0 {
                e |= 2;
            }
            if (q.y - r.bottom()).abs() < 5.0 {
                e |= 4;
            }
            if (q.y - r.top()).abs() < 5.0 {
                e |= 8;
            }
            e
        };
        let opacity = self.drag_state.brush_opacity;
        let brush_shape = |t: f64, f: f32, l: Rect, px: f32| -> SpecShape {
            let rt = (px as f64 / 2.0) / lanes_all.width().max(1.0) as f64 * span;
            // the brush's radius in octaves where it is (a circle on screen on either scale)
            let rf = (px / 2.0) / l.height().max(1.0);
            let up = axis.freq((axis.frac(f) + rf).min(2.0)).max(1.0);
            let ro = (up / f.max(1.0)).log2().abs().max(0.005);
            SpecShape::Brush {
                pts: vec![(t, f)],
                rt,
                ro,
                opacity: opacity / 100.0,
            }
        };
        if spec_tool {
            if let Some(hp) = resp.hover_pos().filter(|q| in_spec(*q)) {
                let inside = tf(hp)
                    .map(|(t, f, _)| spec_sel.as_ref().map(|s| s.contains(t, f)).unwrap_or(false))
                    .unwrap_or(false);
                let e = if tool == Tool::Marquee {
                    marquee_edges(&spec_sel, hp)
                } else {
                    0
                };
                let icon = match (self.drag_state.spec_drag, e) {
                    (SpecDrag::Move { .. }, _) => egui::CursorIcon::Grabbing,
                    (_, 1 | 2) => egui::CursorIcon::ResizeHorizontal,
                    (_, 4 | 8) => egui::CursorIcon::ResizeVertical,
                    (_, 5 | 10) => egui::CursorIcon::ResizeNeSw,
                    (_, 6 | 9) => egui::CursorIcon::ResizeNwSe,
                    _ if inside && matches!(tool, Tool::Marquee | Tool::Lasso) => {
                        egui::CursorIcon::Grab
                    }
                    _ => egui::CursorIcon::Crosshair,
                };
                ui.ctx().set_cursor_icon(icon);
                if matches!(tool, Tool::Brush | Tool::SpotHeal) {
                    let col = if tool == Tool::SpotHeal {
                        Color32::from_rgb(255, 200, 120)
                    } else {
                        Color32::from_rgb(120, 255, 160)
                    };
                    p.circle_stroke(hp, self.drag_state.brush_px / 2.0, Stroke::new(1.0, col));
                }
            }
        }
        if spec_mode {
            if resp.drag_started_by(egui::PointerButton::Primary) {
                if let Some((t, f, l)) = origin.and_then(tf) {
                    let inside = spec_sel.as_ref().map(|s| s.contains(t, f)).unwrap_or(false);
                    self.drag_state.spec_drag = match tool {
                        Tool::Marquee => {
                            let e = origin.map(|o| marquee_edges(&spec_sel, o)).unwrap_or(0);
                            if e != 0 {
                                SpecDrag::Resize { edges: e }
                            } else if inside {
                                SpecDrag::Move { t, f }
                            } else {
                                spec_sel = Some(SpecShape::Rect {
                                    t0: t,
                                    t1: t,
                                    f0: f,
                                    f1: f,
                                });
                                SpecDrag::Make { t, f }
                            }
                        }
                        Tool::Lasso => {
                            if inside {
                                SpecDrag::Move { t, f }
                            } else {
                                spec_sel = Some(SpecShape::Lasso(vec![(t, f)]));
                                SpecDrag::Lasso
                            }
                        }
                        Tool::Brush => {
                            match (&mut spec_sel, shift) {
                                // Shift paints more onto the current selection
                                (Some(SpecShape::Brush { pts, .. }), true) => pts.push((t, f)),
                                _ => {
                                    spec_sel = Some(brush_shape(t, f, l, self.drag_state.brush_px))
                                }
                            }
                            SpecDrag::Brush
                        }
                        _ => {
                            spec_sel = Some(brush_shape(t, f, l, self.drag_state.brush_px));
                            SpecDrag::Heal
                        }
                    };
                    selection = None;
                }
            }
            if resp.dragged_by(egui::PointerButton::Primary) {
                if let Some((t, f, _)) = resp.interact_pointer_pos().and_then(tf) {
                    match (self.drag_state.spec_drag, &mut spec_sel) {
                        (
                            SpecDrag::Make { t: t0, f: f0 },
                            Some(SpecShape::Rect {
                                t0: a,
                                t1: b,
                                f0: lo,
                                f1: hi,
                            }),
                        ) => {
                            *a = t0.min(t);
                            *b = t0.max(t);
                            *lo = f0.min(f);
                            *hi = f0.max(f);
                        }
                        (
                            SpecDrag::Resize { edges },
                            Some(SpecShape::Rect {
                                t0: a,
                                t1: b,
                                f0: lo,
                                f1: hi,
                            }),
                        ) => {
                            if edges & 1 != 0 {
                                *a = t.min(*b);
                            }
                            if edges & 2 != 0 {
                                *b = t.max(*a);
                            }
                            if edges & 4 != 0 {
                                *lo = f.min(*hi);
                            }
                            if edges & 8 != 0 {
                                *hi = f.max(*lo);
                            }
                        }
                        (SpecDrag::Move { t: lt, f: lf }, Some(s)) => {
                            // the grabbed point stays under the pointer; at the top or bottom of
                            // the range the selection stops there instead of drifting off it
                            let f_now = if self.prefs.spec_move_keep_octaves {
                                // proportional: every frequency is multiplied by the same factor,
                                // so the selection keeps its octaves and its size changes in
                                // proportion to where it is (bigger higher up on the linear
                                // scale; the same height on screen on the logarithmic scale)
                                let k = s.move_proportional(t - lt, f / lf.max(1.0), 1.0, nyq);
                                lf.max(1.0) * k
                            } else {
                                // the selection keeps its Hz width
                                let (fmin, fmax) = s.freq_extent();
                                let df = (f - lf).clamp(1.0 - fmin, (nyq - fmax).max(1.0 - fmin));
                                s.translate(t - lt, df);
                                lf + df
                            };
                            self.drag_state.spec_drag = SpecDrag::Move { t, f: f_now };
                        }

                        (SpecDrag::Lasso, Some(SpecShape::Lasso(pts))) => {
                            let far = pts
                                .last()
                                .map(|(pt, pf)| {
                                    (x_of(*pt) - x_of(t)).abs()
                                        + (pf.log2() - f.log2()).abs() * 20.0
                                        > 2.0
                                })
                                .unwrap_or(true);
                            if far {
                                pts.push((t, f));
                            }
                        }
                        (
                            SpecDrag::Brush | SpecDrag::Heal,
                            Some(SpecShape::Brush { pts, rt, .. }),
                        ) => {
                            let gap = (*rt * 0.4).max(1e-6);
                            let far = pts
                                .last()
                                .map(|(pt, pf)| {
                                    (pt - t).abs() > gap || (pf.log2() - f.log2()).abs() > 0.05
                                })
                                .unwrap_or(true);
                            if far {
                                pts.push((t, f));
                            }
                        }
                        _ => {}
                    }
                    ui.ctx().request_repaint();
                }
            }
            if resp.drag_stopped() {
                if self.drag_state.spec_drag == SpecDrag::Heal {
                    spec_apply = Some((SpecOp::Heal, "Spot Healing Brush"));
                } else if let Some(s) = &spec_sel {
                    if s.is_empty() {
                        spec_sel = None;
                    } else {
                        // the time range of the spectral selection is what plays
                        let (t0, t1, _, _) = s.bounds();
                        selection = Some((t0.max(0.0), t1.min(dur)));
                        cursor = t0.max(0.0);
                    }
                }
                if self.drag_state.spec_drag != SpecDrag::Heal {
                    self.drag_state.spec_drag = SpecDrag::None;
                }
            }
            if resp.clicked() {
                // a click on the selection keeps it; a click outside clears it
                if let Some((t, f, _)) = resp.interact_pointer_pos().and_then(tf) {
                    let inside = spec_sel.as_ref().map(|s| s.contains(t, f)).unwrap_or(false);
                    if !inside {
                        spec_sel = None;
                        selection = None;
                        cursor = t;
                    }
                }
                self.drag_state.spec_drag = SpecDrag::None;
            }
        }
        let edge_hit = |px: f32| -> Option<f64> {
            let (a, b) = selection?;
            if (x_of(a) - px).abs() < 5.0 {
                Some(b)
            } else if (x_of(b) - px).abs() < 5.0 {
                Some(a)
            } else {
                None
            }
        };
        if let Some(hp) = resp.hover_pos().filter(|q| !(spec_tool && in_spec(*q))) {
            if edge_hit(hp.x).is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            } else {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            }
        }
        if !spec_mode && resp.drag_started_by(egui::PointerButton::Primary) {
            // a new time selection replaces the previous one (and any spectral selection);
            // Shift+drag or dragging an edge extends it instead
            spec_sel = None;
            // start from where the button went down, not where the drag was first noticed
            if let Some(pp) = origin.or(resp.interact_pointer_pos()) {
                let t = t_of(pp.x).clamp(0.0, dur);
                let anchor = if let Some(other) = edge_hit(pp.x) {
                    other
                } else if shift {
                    match selection {
                        Some((a, b)) => {
                            if (t - a).abs() < (t - b).abs() {
                                b
                            } else {
                                a
                            }
                        }
                        None => cursor,
                    }
                } else {
                    t
                };
                self.drag_state.drag = WaveDrag::Select { anchor };
            }
        }
        if !spec_mode && resp.dragged_by(egui::PointerButton::Primary) {
            if let (WaveDrag::Select { anchor }, Some(pp)) =
                (self.drag_state.drag, resp.interact_pointer_pos())
            {
                let t = t_of(pp.x).clamp(0.0, dur);
                let (a, b) = if t < anchor { (t, anchor) } else { (anchor, t) };
                selection = if b - a > 0.0 { Some((a, b)) } else { None };
                cursor = a;
                // edge auto-scroll
                let edge = 20.0;
                if pp.x > lanes_all.right() - edge && view.1 < dur {
                    let d = span * 0.02;
                    let d = d.min(dur - view.1);
                    view = (view.0 + d, view.1 + d);
                } else if pp.x < lanes_all.left() + edge && view.0 > 0.0 {
                    let d = (span * 0.02).min(view.0);
                    view = (view.0 - d, view.1 - d);
                }
                ui.ctx().request_repaint();
            }
        }
        if resp.drag_stopped() {
            self.drag_state.drag = WaveDrag::None;
        }
        if !spec_mode && resp.clicked() {
            if let Some(pp) = resp.interact_pointer_pos() {
                let t = t_of(pp.x).clamp(0.0, dur);
                spec_sel = None;
                if shift {
                    let (a, b) = match selection {
                        Some((a, b)) => (a.min(t), b.max(t)),
                        None => (cursor.min(t), cursor.max(t)),
                    };
                    selection = Some((a, b));
                } else {
                    selection = None;
                    cursor = t;
                }
            }
        }
        if !spec_mode && resp.double_clicked() {
            selection = Some(view);
            cursor = view.0;
        }
        // middle button drag pans the view
        if resp.hovered() || rresp.hovered() {
            let (mid, dx) = ui.input(|i| (i.pointer.middle_down(), i.pointer.delta().x));
            if mid {
                let d = -(dx as f64) / lanes_all.width().max(1.0) as f64 * span;
                let a = (view.0 + d).clamp(0.0, (dur - span).max(0.0));
                view = (a, a + span);
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }
        // mouse wheel zoom / pan
        if resp.hovered() && !freq_wheel {
            let (scroll, zoom_d) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            let hp = resp.hover_pos().unwrap_or(lanes_all.center());
            let tc = t_of(hp.x);
            let mut factor = 1.0;
            if zoom_d != 1.0 {
                factor = 1.0 / zoom_d as f64;
            } else if scroll.y.abs() > 0.1 {
                factor = (0.996f64).powf(scroll.y as f64);
            }
            if factor != 1.0 {
                let w = (span * factor)
                    .max((10.0 / data.rate as f64).min(dur))
                    .min(dur);
                let a = (tc - (tc - view.0) * (w / span)).clamp(0.0, (dur - w).max(0.0));
                view = (a, a + w);
            }
            if scroll.x.abs() > 0.1 {
                let d = -(scroll.x as f64) / lanes_all.width() as f64 * span;
                let a = (view.0 + d).clamp(0.0, (dur - span).max(0.0));
                view = (a, a + span);
            }
        }
        let has_spec = spec_sel.is_some();
        resp.context_menu(|ui| {
            self.editor_context_menu(ui, selection.is_some() || has_spec, has_spec)
        });

        // ---------- fade handles (drag inward: horizontal = length, vertical = curve)
        // The fades stay where they are let go and can be dragged again while they are the
        // latest edit (one undo step holds both); a handle never leaves the visible area.
        let stored = self
            .cur()
            .and_then(|f| f.live_fades())
            .map(|(_, f)| f)
            .unwrap_or([(0.0, 1.0), (0.0, 1.0)]);
        let mut fade_apply: Option<[(f64, f32); 2]> = None;
        for which in [1u8, 2u8] {
            let is_in = which == 1;
            let k = if is_in { 0 } else { 1 };
            let active = self.drag_state.fade_which == which;
            let (len, pw) = if active {
                (self.drag_state.fade_len, self.drag_state.fade_p)
            } else {
                stored[k]
            };
            let raw_x = if is_in {
                x_of(len) + 7.0
            } else {
                x_of(dur - len) - 7.0
            };
            let hx = raw_x
                .min(lanes_all.right() - 6.0)
                .max(lanes_all.left() + 6.0);
            if !active && (raw_x < lanes_all.left() - 1.0 || raw_x > lanes_all.right() + 1.0) {
                // scrolled out of view (and not being dragged): nothing to grab here
                continue;
            }
            let hr = Rect::from_center_size(pos2(hx, wave_area.top() + 8.0), vec2(10.0, 10.0));
            let hresp = ui.interact(
                hr.expand(3.0),
                ui.scope_id().with(("fadeh", which)),
                Sense::drag(),
            );
            let col = if hresp.hovered() || active {
                Color32::WHITE
            } else {
                crate::theme::gray(185)
            };
            p.rect_filled(hr, 0.0, col);
            p.rect_stroke(
                hr,
                0.0,
                Stroke::new(1.0, crate::theme::gray(40)),
                egui::StrokeKind::Inside,
            );
            let hresp = hresp.on_hover_text(if is_in {
                "Fade In: drag right (up/down changes the curve)"
            } else {
                "Fade Out: drag left (up/down changes the curve)"
            });
            if hresp.drag_started() {
                self.drag_state.fade_which = which;
                self.drag_state.fade_len = stored[k].0;
                self.drag_state.fade_p = stored[k].1;
                // remember where the grab began so the curve changes relative to it
                self.drag_state.fade_grab_y =
                    hresp.interact_pointer_pos().map(|p| p.y).unwrap_or(0.0)
                        - 45.0 * stored[k].1.max(0.2).log2();
            }
            if active {
                if let Some(pp) = ui.ctx().pointer_latest_pos() {
                    let t = t_of(pp.x).clamp(0.0, dur);
                    self.drag_state.fade_len = if is_in { t } else { dur - t };
                    let dy = pp.y - self.drag_state.fade_grab_y;
                    self.drag_state.fade_p = 2f32.powf(dy / 45.0).clamp(0.2, 5.0);
                }
            }
            if len > 0.0 {
                // the fade's curve over the faded part
                let (t0, t1) = if is_in { (0.0, len) } else { (dur - len, dur) };
                let steps = 48;
                let mut pts = vec![];
                for i in 0..=steps {
                    let fr = i as f32 / steps as f32;
                    let t = t0 + (t1 - t0) * fr as f64;
                    let x = if is_in { fr } else { 1.0 - fr };
                    pts.push(pos2(
                        x_of(t),
                        wave_area.bottom() - x.powf(pw) * wave_area.height(),
                    ));
                }
                let shade = Rect::from_min_max(
                    pos2(x_of(t0).max(lanes_all.left()), wave_area.top()),
                    pos2(x_of(t1).min(lanes_all.right()), wave_area.bottom()),
                );
                if shade.width() > 0.0 {
                    p.rect_filled(
                        shade,
                        0.0,
                        Color32::from_rgba_unmultiplied(0, 0, 0, if active { 70 } else { 40 }),
                    );
                }
                p.with_clip_rect(lanes_all).add(egui::Shape::line(
                    pts,
                    Stroke::new(
                        if active { 2.0 } else { 1.2 },
                        Color32::from_rgb(255, 225, 90),
                    ),
                ));
                if active {
                    p.text(
                        pos2(
                            hx + if is_in { 10.0 } else { -10.0 },
                            wave_area.top() + 20.0,
                        ),
                        if is_in {
                            egui::Align2::LEFT_TOP
                        } else {
                            egui::Align2::RIGHT_TOP
                        },
                        format!(
                            "{} {}",
                            if is_in { "Fade In" } else { "Fade Out" },
                            audio::fmt_time(len)
                        ),
                        FontId::proportional(11.0),
                        Color32::WHITE,
                    );
                }
            }
            // let go anywhere (even off the handle) applies the fade
            if active && !ui.input(|i| i.pointer.primary_down()) {
                let mut f2 = stored;
                f2[k] = (
                    if self.drag_state.fade_len > 0.0005 {
                        self.drag_state.fade_len
                    } else {
                        0.0
                    },
                    self.drag_state.fade_p,
                );
                if f2 != stored {
                    fade_apply = Some(f2);
                }
                self.drag_state.fade_which = 0;
                self.drag_state.fade_len = 0.0;
            }
        }
        if let Some(fades) = fade_apply {
            let label = if fades[0].0 > 0.0 && fades[1].0 > 0.0 {
                "Fade In/Out"
            } else if fades[0].0 > 0.0 {
                "Fade In"
            } else {
                "Fade Out"
            };
            if let Some(f) = self.cur_mut() {
                f.set_fades(fades, label);
            }
        }
        // the take being recorded, growing from where recording began
        if let Some((at, None)) = self.recording_at {
            self.draw_live_take(&p.with_clip_rect(lanes_all), lanes_all, at, &x_of);
        }

        // playhead
        let px = x_of(cursor);
        if px >= lanes_all.left() - 1.0 && px <= lanes_all.right() + 1.0 {
            p.line_segment(
                [pos2(px, ruler.top()), pos2(px, lanes_all.bottom())],
                Stroke::new(1.0, theme::playhead()),
            );
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(px - 6.0, ruler.top()),
                    pos2(px + 6.0, ruler.top()),
                    pos2(px, ruler.top() + 8.0),
                ],
                theme::playhead(),
                Stroke::NONE,
            ));
        }
        // selection time tooltip
        if let (WaveDrag::Select { .. }, Some((a, b))) = (self.drag_state.drag, selection) {
            let txt = format!(
                "{}  \u{2192}  {}   ({})",
                self.fmt(a),
                self.fmt(b),
                self.fmt(b - a)
            );
            p.text(
                pos2(lanes_all.left() + 8.0, lanes_all.bottom() - 8.0),
                egui::Align2::LEFT_BOTTOM,
                txt,
                FontId::monospace(11.0),
                Color32::WHITE,
            );
        }

        // spectral selection HUD: a small gain knob riding on the selection's top-left corner;
        // drag it and release to raise or lower just the selected frequencies
        if let (Some(shape), Some(first)) = (&spec_sel, spec_lanes.first()) {
            if self.drag_state.spec_drag == SpecDrag::None
                || matches!(self.drag_state.spec_drag, SpecDrag::Move { .. })
            {
                let (t0, _, _, f1) = shape.bounds();
                let x = x_of(t0).clamp(first.left() + 2.0, first.right() - 150.0);
                let y = (axis.y(*first, f1) - 28.0).clamp(first.top() + 2.0, first.bottom() - 26.0);
                let hr = Rect::from_min_size(pos2(x, y), vec2(148.0, 24.0));
                p.rect_filled(hr, 0.0, crate::theme::gray_a(40, 215));
                let mut hui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(hr.shrink2(vec2(6.0, 1.0)))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                {
                    // rising bars: the volume mark
                    let (br, _) = hui.allocate_exact_size(vec2(16.0, 18.0), Sense::hover());
                    for i in 0..5 {
                        let x = br.left() + 1.0 + i as f32 * 3.0;
                        let h = 2.0 + i as f32 * 2.2;
                        hui.painter().line_segment(
                            [
                                pos2(x, br.center().y + 5.0),
                                pos2(x, br.center().y + 5.0 - h),
                            ],
                            Stroke::new(1.0, theme::text_dim()),
                        );
                    }
                }
                let mut g = self.drag_state.spec_gain;
                let k = theme::knob(
                    &mut hui,
                    &mut g,
                    -40.0,
                    20.0,
                    20.0,
                    "Spectral selection gain: drag, release to apply",
                );
                let gtxt = format!("{:+.1} dB", g);
                let hv = theme::hot_value(
                    &mut hui,
                    &mut g,
                    -40.0,
                    20.0,
                    0.1,
                    0.0,
                    &gtxt,
                    "Spectral selection gain: drag, release to apply",
                );
                self.drag_state.spec_gain = (g * 10.0).round() / 10.0;
                if (k.drag_stopped() || hv.drag_stopped()) && self.drag_state.spec_gain.abs() > 0.05
                {
                    spec_apply = Some((SpecOp::Gain(self.drag_state.spec_gain), "Spectral Gain"));
                    self.drag_state.spec_gain = 0.0;
                }
                if hui
                    .small_button("\u{2715}")
                    .on_hover_text("Deselect (Esc)")
                    .clicked()
                {
                    spec_sel = None;
                }
            }
        }

        // write back
        let playing = self.is_playing();
        let f = self.cur_mut().unwrap();
        f.view = view;
        f.selection = selection;
        f.spec_sel = spec_sel;
        let moved = (f.cursor - cursor).abs() > 1e-9;
        if moved {
            f.cursor = cursor;
        }
        if moved && playing {
            self.set_cursor(cursor);
        }
        if let Some((op, label)) = spec_apply {
            self.spectral_apply(op, label);
            if op == SpecOp::Heal {
                // the healing brush leaves no selection behind
                if let Some(f) = self.cur_mut() {
                    f.spec_sel = None;
                    f.selection = None;
                }
                self.drag_state.spec_drag = SpecDrag::None;
            }
        }
        if let Some(c) = self.pending_cmd.take() {
            let ctx = ui.ctx().clone();
            match c {
                "zoomsel" => self.zoom_selection(),
                "selview" => {
                    if let Some(f) = self.cur_mut() {
                        f.selection = Some(f.view);
                        f.spec_sel = None;
                    }
                }
                _ => self.run(c, &ctx),
            }
        }
    }

    /// The editor's right-click menu (Waveform Editor and spectral display).
    fn editor_context_menu(&mut self, ui: &mut egui::Ui, has_sel: bool, has_spec: bool) {
        let ctx = ui.ctx().clone();
        let item = |ui: &mut egui::Ui, enabled: bool, label: &str, sc: &str| -> bool {
            let r = ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(sc));
            if r.clicked() {
                ui.close();
                true
            } else {
                false
            }
        };
        if item(ui, true, "Select All", "Ctrl+A") {
            self.pending_cmd = Some("all");
        }
        if item(ui, has_sel, "Deselect All", "Ctrl+Shift+A") {
            self.pending_cmd = Some("none");
        }
        if item(ui, true, "Select Current View Time", "") {
            self.pending_cmd = Some("selview");
        }
        // the opposite frequencies over the same time range as the spectral selection
        if item(ui, has_spec, "Select Inverse", "") {
            let _ = self.exec("spectral.invert", &ctx);
        }
        ui.separator();
        if item(ui, has_sel, "Save Selection As...", "Ctrl+Alt+S") {
            self.copy();
            self.paste_new();
            self.run("saveas", &ctx);
        }
        ui.menu_button("Insert into Multitrack", |ui| {
            let has_session = self.session.is_some();
            if ui
                .add_enabled(
                    has_session,
                    egui::Button::new(
                        self.session
                            .as_ref()
                            .map(|s| s.name.clone())
                            .unwrap_or_else(|| "(no session)".into()),
                    ),
                )
                .clicked()
            {
                self.insert_current_into_multitrack(false);
                ui.close();
            }
            if ui.button("New Multitrack Session...").clicked() {
                self.insert_current_into_multitrack(true);
                ui.close();
            }
        });
        ui.separator();
        if item(ui, true, "Capture Noise Print", "Shift+P") {
            let _ = self.exec("process.capture_noise_print", &ctx);
        }
        item(ui, false, "Learn Sound Model", "");
        ui.separator();
        {
            let ctx = ui.ctx().clone();
            self.clipboard_menu(ui, &ctx);
        }
        if item(ui, has_sel, "Cut", "Ctrl+X") {
            self.pending_cmd = Some("cut");
        }
        if item(ui, has_sel, "Copy", "Ctrl+C") {
            self.pending_cmd = Some("copy");
        }
        if item(ui, has_sel, "Copy To New", "Alt+Shift+C") {
            self.copy();
            self.paste_new();
        }
        if item(ui, self.clipboard.is_some(), "Paste", "Ctrl+V") {
            self.pending_cmd = Some("paste");
        }
        if item(ui, self.clipboard.is_some(), "Mix Paste...", "Ctrl+Shift+V") {
            self.pending_cmd = Some("mixpaste");
        }
        if item(ui, has_sel, "Delete", "Delete") {
            self.pending_cmd = Some("delete");
        }
        ui.separator();
        if item(ui, has_sel && !has_spec, "Crop", "Ctrl+T") {
            self.pending_cmd = Some("crop");
        }
        if item(ui, has_sel, "Silence", "") {
            if has_spec {
                let _ = self.exec("spectral.silence", &ctx);
            } else {
                let _ = self.exec("process.silence", &ctx);
            }
        }
        if item(ui, has_sel, "Auto Heal Selection", "Ctrl+U") {
            self.pending_cmd = Some("autoheal");
        }
        ui.separator();
        ui.menu_button("Marker", |ui| {
            if ui
                .add(egui::Button::new("Add Marker").shortcut_text("M"))
                .clicked()
            {
                self.pending_cmd = Some("marker");
                ui.close();
            }
            if ui.button("Show Markers Panel").clicked() {
                let _ = self.exec("window.markers", &ctx);
                ui.close();
            }
        });
        if item(ui, true, "Convert Sample Type...", "Shift+T") {
            let _ = self.exec("edit.convert", &ctx);
        }
        if item(ui, true, "Interpret Sample Rate...", "") {
            let _ = self.exec("edit.interpret_sample_rate", &ctx);
        }
        let stereo = self.cur().map(|f| f.data.channels() > 1).unwrap_or(false);
        if item(ui, stereo, "Extract Channels to Mono Files", "") {
            self.extract_channels();
        }
        if item(ui, true, "Frequency Band Splitter...", "") {
            let _ = self.exec("edit.frequency_band_splitter", &ctx);
        }
    }

    fn empty_editor(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = ui.painter_at(r);
        let c = r.center();
        paint_drop_hint(&p, c);
        p.text(
            c + vec2(0.0, 40.0),
            egui::Align2::CENTER_CENTER,
            "Drop audio files here, or use File \u{203a} Open",
            FontId::proportional(14.0),
            theme::text_dim(),
        );
        p.text(
            c + vec2(0.0, 62.0),
            egui::Align2::CENTER_CENTER,
            "Ctrl+N creates a new file \u{2022} Shift+Space records",
            FontId::proportional(11.5),
            crate::theme::gray(100),
        );
        let br = Rect::from_center_size(c + vec2(0.0, 98.0), vec2(220.0, 26.0));
        let mut bui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(br)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        if bui.button("Open File...").clicked() {
            self.open_dialog();
        }
        if bui.button("New File...").clicked() {
            let ctx = ui.ctx().clone();
            self.run("new", &ctx);
        }
        if bui.button("Record").clicked() {
            self.toggle_record();
        }
    }
}

/// The pitch (Hz) of the whole file every `hop` seconds: (hop, pitch or None when unvoiced).
/// Mono mix, low-passed and decimated to about 8 kHz, 40 ms windows; isolated octave jumps and
/// one-frame blips are smoothed out.
pub fn analyse_pitch(d: &AudioData) -> (f64, Vec<Option<f32>>) {
    let hop = 0.01;
    let rate = d.rate.max(1) as usize;
    let dec = (rate / 8000).max(1);
    let r = rate as f32 / dec as f32;
    // mono, averaged over each decimation block (a crude low-pass)
    let n = d.frames() / dec;
    let chs = d.channels().max(1);
    let mono: Vec<f32> = (0..n)
        .map(|i| {
            let mut s = 0.0;
            for k in 0..dec {
                for c in &d.ch {
                    s += c[i * dec + k];
                }
            }
            s / (dec * chs) as f32
        })
        .collect();
    let win = (r * 0.04) as usize;
    let step = ((r as f64) * hop) as usize;
    let mut out: Vec<Option<f32>> = vec![];
    if win < 64 || step == 0 {
        return (hop, out);
    }
    let mut i = 0;
    while i + win <= mono.len() {
        out.push(crate::fx3::detect_pitch(&mono[i..i + win], r, 70.0));
        i += step;
    }
    // smoothing: a 5-frame median on voiced runs, and drop lone voiced frames
    let midi: Vec<Option<f32>> = out
        .iter()
        .map(|f| f.map(|hz| 69.0 + 12.0 * (hz / 440.0).log2()))
        .collect();
    let mut sm = midi.clone();
    for k in 0..midi.len() {
        let Some(_) = midi[k] else { continue };
        let mut w: Vec<f32> = midi[k.saturating_sub(2)..(k + 3).min(midi.len())]
            .iter()
            .flatten()
            .copied()
            .collect();
        if w.len() < 2 {
            sm[k] = None;
            continue;
        }
        w.sort_by(|a, b| a.total_cmp(b));
        sm[k] = Some(w[w.len() / 2]);
    }
    let hop_real = step as f64 / r as f64;
    (
        hop_real,
        sm.into_iter()
            .map(|m| m.map(|v| 440.0 * 2f32.powf((v - 69.0) / 12.0)))
            .collect(),
    )
}

/// Picture of how strongly each pixel of a spectral lane is selected (smooth, anti-aliased:
/// each pixel averages 2x2 samples). Used for lasso, paintbrush and inverted selections.
fn selection_image(
    shape: &SpecShape,
    w: usize,
    h: usize,
    view: (f64, f64),
    axis: FreqAxis,
) -> egui::ColorImage {
    let mut img = egui::ColorImage::filled([w, h], Color32::TRANSPARENT);
    let span = (view.1 - view.0).max(1e-9);
    let (t0, t1, _, _) = shape.bounds();
    let x0 = (((t0 - view.0) / span) * w as f64).floor().max(0.0) as usize;
    let x1 = ((((t1 - view.0) / span) * w as f64).ceil().max(0.0) as usize).min(w);
    if x1 <= x0 {
        return img;
    }
    // weights on a half-pixel grid (stamped, so long brush strokes stay fast), then each pixel
    // averages its 2x2 samples for smooth, anti-aliased edges
    let rows: Vec<f32> = (0..h * 2)
        .map(|r| axis.freq(1.0 - (r as f32 + 0.5) / (h * 2) as f32))
        .collect();
    let cols: Vec<f64> = (x0 * 2..x1 * 2)
        .map(|c| view.0 + (c as f64 + 0.5) / (w * 2) as f64 * span)
        .collect();
    let g = shape.grid_weights(&cols, &rows);
    let nr = rows.len();
    for x in x0..x1 {
        let c = (x - x0) * 2;
        for y in 0..h {
            let r = y * 2;
            let wgt =
                (g[c * nr + r] + g[c * nr + r + 1] + g[(c + 1) * nr + r] + g[(c + 1) * nr + r + 1])
                    * 0.25;
            if wgt > 0.0 {
                let a = (wgt * 95.0) as u8;
                img.pixels[y * w + x] = Color32::from_rgba_unmultiplied(110, 250, 140, a);
            }
        }
    }
    img
}

/// Outline of a spectral selection over one spectral lane (the fill of free-form selections
/// comes from `selection_image`): a translucent green box with a cyan dashed edge for marquees.
fn draw_spec_sel(
    p: &egui::Painter,
    lane: Rect,
    shape: &SpecShape,
    x_of: &impl Fn(f64) -> f32,
    axis: FreqAxis,
) {
    let fill = Color32::from_rgba_unmultiplied(110, 250, 140, 80);
    let edge = Stroke::new(1.0, Color32::from_rgb(80, 220, 220));
    let dashed = |a: Pos2, b: Pos2| {
        let len = a.distance(b);
        let n = (len / 6.0).ceil().max(1.0) as usize;
        for i in (0..n).step_by(2) {
            let u0 = i as f32 / n as f32;
            let u1 = ((i + 1) as f32 / n as f32).min(1.0);
            p.line_segment([a + (b - a) * u0, a + (b - a) * u1], edge);
        }
    };
    let to_screen = |(t, f): (f64, f32)| pos2(x_of(t), axis.y(lane, f));
    let rect_of = |s: &SpecShape| {
        let (t0, t1, f0, f1) = s.bounds();
        Rect::from_min_max(
            pos2(x_of(t0), axis.y(lane, f1)),
            pos2(x_of(t1), axis.y(lane, f0)),
        )
    };
    let dashed_rect = |r: Rect| {
        dashed(r.left_top(), r.right_top());
        dashed(r.right_top(), r.right_bottom());
        dashed(r.right_bottom(), r.left_bottom());
        dashed(r.left_bottom(), r.left_top());
    };
    match shape {
        SpecShape::Rect { .. } => {
            let r = rect_of(shape);
            p.rect_filled(r, 0.0, fill);
            dashed_rect(r);
            for c in [
                r.left_top(),
                r.right_top(),
                r.left_bottom(),
                r.right_bottom(),
            ] {
                p.rect_filled(
                    Rect::from_center_size(c, vec2(5.0, 5.0)),
                    0.0,
                    Color32::from_rgb(80, 220, 220),
                );
            }
        }
        SpecShape::Lasso(pts) => {
            let sp: Vec<Pos2> = pts.iter().map(|q| to_screen(*q)).collect();
            for i in 0..sp.len() {
                let j = (i + 1) % sp.len();
                if j == 0 && sp.len() < 3 {
                    break;
                }
                dashed(sp[i], sp[j]);
            }
        }
        SpecShape::Brush { .. } => {}
        SpecShape::Inverse { inner, .. } => {
            // the hole the inverse leaves, and its time range
            if let SpecShape::Rect { .. } = **inner {
                dashed_rect(rect_of(inner));
            }
            let r = rect_of(shape);
            dashed(pos2(r.left(), lane.top()), pos2(r.left(), lane.bottom()));
            dashed(pos2(r.right(), lane.top()), pos2(r.right(), lane.bottom()));
        }
    }
}

fn paint_drop_hint(p: &egui::Painter, c: Pos2) {
    // a smooth waveform: sampled every half pixel (so the curves never break into straight
    // pieces), swelling in the middle, with a soft glow under a crisp line
    let w = 200.0f32;
    let n = (w * 2.0) as usize;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let u = i as f32 / n as f32;
            let s = u * 2.0 - 1.0;
            let env = (1.0 - s * s).max(0.0).powf(1.4);
            let a = 22.0 * env * (u * 26.0).sin() * (0.65 + 0.35 * (u * 7.0).cos());
            pos2(c.x - w / 2.0 + u * w, c.y + a)
        })
        .collect();
    p.add(egui::Shape::line(
        pts.clone(),
        Stroke::new(6.0, Color32::from_rgba_unmultiplied(46, 214, 170, 28)),
    ));
    p.add(egui::Shape::line(
        pts,
        Stroke::new(2.0, Color32::from_rgb(46, 190, 150)),
    ));
}
