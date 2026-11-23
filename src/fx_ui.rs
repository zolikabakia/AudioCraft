//! The shared visual language of the effect windows: instrument knobs, framed control banks and
//! the small response/transfer graphs drawn by the effect editors. This module only maps values
//! and paints them; all audio state stays in `Effect` and `EffectSlot`.

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, pos2, vec2};

#[allow(non_snake_case)]
fn PANEL() -> Color32 {
    crate::theme::gray(29)
}
#[allow(non_snake_case)]
fn PLOT() -> Color32 {
    crate::theme::gray(16)
}
#[allow(non_snake_case)]
fn GRID() -> Color32 {
    crate::theme::gray(48)
}
#[allow(non_snake_case)]
fn GRID_MAJOR() -> Color32 {
    crate::theme::gray(72)
}
#[allow(non_snake_case)]
fn LABEL() -> Color32 {
    crate::theme::gray(150)
}
#[allow(non_snake_case)]
fn KNOB() -> Color32 {
    crate::theme::gray(70)
}
const ARC: Color32 = Color32::from_rgb(96, 158, 210);
pub const VALUE: Color32 = Color32::from_rgb(78, 168, 255);
const CURVE: Color32 = Color32::from_rgb(92, 112, 255);

/// Map a value onto 0..1, optionally on a logarithmic scale. Degenerate ranges return 0 instead
/// of panicking, so a bad saved preset can never take the window down.
pub fn normalize(v: f32, lo: f32, hi: f32, log: bool) -> f32 {
    if !v.is_finite() || !lo.is_finite() || !hi.is_finite() || hi <= lo {
        return 0.0;
    }
    let t = if log && lo > 0.0 && hi > 0.0 {
        (v.max(lo).min(hi).ln() - lo.ln()) / (hi.ln() - lo.ln()).max(1e-9)
    } else {
        (v - lo) / (hi - lo)
    };
    t.clamp(0.0, 1.0)
}

/// The inverse of `normalize`, with the same guard against crossed bounds.
pub fn denormalize(t: f32, lo: f32, hi: f32, log: bool) -> f32 {
    if !lo.is_finite() || !hi.is_finite() || hi <= lo {
        return lo;
    }
    let t = t.clamp(0.0, 1.0);
    if log && lo > 0.0 && hi > 0.0 {
        (lo.ln() + t * (hi.ln() - lo.ln())).exp()
    } else {
        lo + t * (hi - lo)
    }
}

/// The blue numeric readout used under knobs and beside sliders.
pub fn format_value(v: f32, suffix: &str) -> String {
    if !v.is_finite() {
        return format!("--{suffix}");
    }
    if v.abs() >= 1000.0 {
        format!("{:.2}k{suffix}", v / 1000.0)
    } else if v.fract().abs() < 0.01 {
        format!("{v:.0}{suffix}")
    } else if v.abs() < 10.0 {
        format!("{v:.2}{suffix}")
    } else {
        format!("{v:.1}{suffix}")
    }
}

/// Evenly spaced dB axis labels ("-60", "-48" ... "0") for response graphs.
pub fn db_ticks(lo: f32, hi: f32, step: f32) -> Vec<(f32, String)> {
    let mut out = Vec::new();
    if !lo.is_finite() || !hi.is_finite() || hi <= lo || !step.is_finite() || step <= 0.0 {
        return out;
    }
    let mut v = lo;
    while v <= hi + step * 1e-3 {
        out.push((v, format!("{v:.0}")));
        v += step;
    }
    out
}

/// A round control like the ones in Audition's effect windows. Drag up/down to change, hold Shift
/// for fine steps, double-click to go back to `default`. Returns true if the value changed.
pub fn knob(
    ui: &mut egui::Ui,
    label: &str,
    v: &mut f32,
    lo: f32,
    hi: f32,
    suffix: &str,
    log: bool,
    default: f32,
) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(66.0, 76.0), Sense::click_and_drag());
    let mut changed = false;
    if resp.dragged() {
        let dy = ui.input(|i| i.pointer.delta().y);
        let fine = ui.input(|i| i.modifiers.shift);
        let span = if fine { 900.0 } else { 180.0 };
        let nv = denormalize(normalize(*v, lo, hi, log) - dy / span, lo, hi, log);
        if nv.is_finite() && (nv - *v).abs() > 1e-6 {
            *v = nv;
            changed = true;
        }
    }
    if resp.double_clicked() {
        let d = default.max(lo).min(hi);
        if d.is_finite() && (d - *v).abs() > 1e-6 {
            *v = d;
            changed = true;
        }
    }
    let p = ui.painter();
    let c = pos2(r.center().x, r.top() + 27.0);
    let radius = 21.0;
    let t = normalize(*v, lo, hi, log);
    let arc = |upto: f32, rad: f32| -> Vec<Pos2> {
        (0..=28)
            .map(|i| {
                let a = (135.0 + upto * 270.0 * i as f32 / 28.0).to_radians();
                c + vec2(a.cos() * rad, a.sin() * rad)
            })
            .collect()
    };
    p.add(egui::Shape::line(
        arc(1.0, radius),
        Stroke::new(3.0, crate::theme::gray(46)),
    ));
    if t > 0.0 {
        p.add(egui::Shape::line(arc(t, radius), Stroke::new(3.0, ARC)));
    }
    p.circle_filled(c, radius - 5.0, KNOB());
    p.circle_stroke(c, radius - 5.0, Stroke::new(1.0, crate::theme::gray(104)));
    let a = (135.0 + t * 270.0).to_radians();
    let dir = vec2(a.cos(), a.sin());
    p.line_segment(
        [c + dir * 6.0, c + dir * (radius - 7.0)],
        Stroke::new(2.0, crate::theme::gray(235)),
    );
    p.text(
        pos2(c.x, r.bottom() - 16.0),
        Align2::CENTER_CENTER,
        format_value(*v, suffix),
        FontId::proportional(11.0),
        VALUE,
    );
    p.text(
        pos2(c.x, r.bottom() - 3.0),
        Align2::CENTER_BOTTOM,
        label,
        FontId::proportional(10.0),
        LABEL(),
    );
    resp.on_hover_text(format!(
        "{label}: {}   drag to adjust, Shift for fine, double-click to reset",
        format_value(*v, suffix)
    ));
    changed
}

/// Lets a bank end in a bool, a widget response, or a nested layout that returns either.
pub trait FxChanged {
    fn fx_changed(self) -> bool;
}
impl FxChanged for bool {
    fn fx_changed(self) -> bool {
        self
    }
}
impl FxChanged for egui::Response {
    fn fx_changed(self) -> bool {
        self.changed()
    }
}
impl<R: FxChanged> FxChanged for egui::InnerResponse<R> {
    fn fx_changed(self) -> bool {
        self.inner.fx_changed()
    }
}
impl FxChanged for () {
    fn fx_changed(self) -> bool {
        false
    }
}

/// A framed group of knobs (or other controls), with the small caption Audition puts at the top
/// of each section. Returns whether any control inside changed.
pub fn bank<R: FxChanged>(
    ui: &mut egui::Ui,
    title: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> bool {
    ui.add_space(4.0);
    egui::Frame::new()
        .fill(PANEL())
        .stroke(Stroke::new(1.0, crate::theme::gray(54)))
        .corner_radius(crate::theme::ROUND)
        .inner_margin(egui::Margin {
            left: 10,
            right: 10,
            top: 6,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.label(RichText::new(title).color(crate::theme::gray(196)).strong());
            ui.add_space(2.0);
            add(ui).fx_changed()
        })
        .inner
}

/// A graph on a dark grid: `curve(x)` is the plotted line, `ticks` are axis labels and `markers`
/// are named vertical guides (threshold, crossover, delay time ...). `unity` draws y = x when
/// both axes are decibels.
#[allow(clippy::too_many_arguments)]
pub fn graph(
    ui: &mut egui::Ui,
    height: f32,
    x_min: f32,
    x_max: f32,
    y_min: f32,
    y_max: f32,
    curve: impl Fn(f32) -> f32,
    x_ticks: &[(f32, String)],
    y_ticks: &[(f32, String)],
    markers: &[(f32, String, Color32)],
    unity: bool,
) {
    let w = ui.available_width().clamp(240.0, 760.0);
    let (outer, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
    let r = Rect::from_min_max(outer.min + vec2(8.0, 8.0), outer.max - vec2(40.0, 19.0));
    if r.width() <= 1.0 || r.height() <= 1.0 || x_max <= x_min || y_max <= y_min {
        return;
    }
    let x_of = |x: f32| r.left() + ((x - x_min) / (x_max - x_min)).clamp(0.0, 1.0) * r.width();
    let y_of = |y: f32| r.bottom() - ((y - y_min) / (y_max - y_min)).clamp(0.0, 1.0) * r.height();
    let p = ui.painter_at(outer);
    p.rect_filled(outer, 0.0, PANEL());
    p.rect_filled(r, 0.0, PLOT());
    for (x, text) in x_ticks {
        let x = x_of(*x);
        p.line_segment(
            [pos2(x, r.top()), pos2(x, r.bottom())],
            Stroke::new(1.0, GRID()),
        );
        p.text(
            pos2(x, r.bottom() + 3.0),
            Align2::CENTER_TOP,
            text,
            FontId::proportional(9.5),
            LABEL(),
        );
    }
    for (y, text) in y_ticks {
        let y = y_of(*y);
        p.line_segment(
            [pos2(r.left(), y), pos2(r.right(), y)],
            Stroke::new(1.0, GRID()),
        );
        p.line_segment(
            [pos2(r.right(), y), pos2(r.right() + 4.0, y)],
            Stroke::new(1.0, GRID_MAJOR()),
        );
        p.text(
            pos2(r.right() + 7.0, y),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(9.5),
            LABEL(),
        );
    }
    if unity {
        let line: Vec<Pos2> = (0..=64)
            .map(|i| {
                let x = x_min + (x_max - x_min) * i as f32 / 64.0;
                pos2(x_of(x), y_of(x))
            })
            .collect();
        p.add(egui::Shape::line(
            line,
            Stroke::new(1.0, crate::theme::gray(86)),
        ));
    }
    for (x, text, col) in markers {
        let x = x_of(*x);
        p.line_segment(
            [pos2(x, r.top()), pos2(x, r.bottom())],
            Stroke::new(1.0, *col),
        );
        p.text(
            pos2(x + 3.0, r.top() + 2.0),
            Align2::LEFT_TOP,
            text,
            FontId::proportional(9.5),
            *col,
        );
    }
    let n = (r.width().max(2.0) as usize).min(2048);
    let mut pts = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let x = x_min + (x_max - x_min) * i as f32 / n as f32;
        let y = curve(x);
        if y.is_finite() {
            pts.push(pos2(x_of(x), y_of(y)));
        }
    }
    p.add(egui::Shape::line(pts, Stroke::new(2.0, CURVE)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knob_value_mapping_round_trips() {
        for (lo, hi, log) in [
            (-60.0f32, 24.0f32, false),
            (20.0, 20000.0, true),
            (1.0, 20.0, false),
        ] {
            for t in [0.0, 0.25, 0.5, 0.9, 1.0] {
                let v = denormalize(t, lo, hi, log);
                assert!(
                    (normalize(v, lo, hi, log) - t).abs() < 0.002,
                    "{lo}..{hi} log={log} t={t}"
                );
            }
        }
    }

    #[test]
    fn degenerate_ranges_do_not_panic() {
        assert_eq!(normalize(5.0, 10.0, 0.0, true), 0.0);
        assert_eq!(denormalize(0.5, 10.0, 0.0, true), 10.0);
        assert!(db_ticks(10.0, 0.0, 6.0).is_empty());
    }
}
