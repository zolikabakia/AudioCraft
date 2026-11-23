//! Filter and EQ editors drawn in the style of a classic parametric EQ window: a frequency
//! response graph (log Hz axis along the bottom, dB scale on the right, a master gain slider on
//! the left) with labelled band handles, and a band table underneath.

use crate::audio::db;
use crate::dsp::{Biquad, EqBand, FilterKind};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};

const CURVE: Color32 = Color32::from_rgb(92, 112, 255);
#[allow(non_snake_case)]
fn GRID() -> Color32 {
    crate::theme::gray(52)
}
#[allow(non_snake_case)]
fn GRID_MAJOR() -> Color32 {
    crate::theme::gray(78)
}
#[allow(non_snake_case)]
fn LABEL() -> Color32 {
    crate::theme::gray(150)
}
const VALUE: Color32 = Color32::from_rgb(78, 168, 255);

/// The nine-band layout: HP, low shelf, five peaks, high shelf, LP.
pub fn standard_bands() -> Vec<EqBand> {
    let b = |kind, freq, q, on| EqBand {
        kind,
        freq,
        gain_db: 0.0,
        q,
        on,
    };
    vec![
        b(FilterKind::HighPass, 40.0, 0.707, false),
        b(FilterKind::LowShelf, 40.0, 0.707, true),
        b(FilterKind::Peak, 50.0, 2.0, true),
        b(FilterKind::Peak, 200.0, 2.0, true),
        b(FilterKind::Peak, 800.0, 2.0, true),
        b(FilterKind::Peak, 3200.0, 2.0, true),
        b(FilterKind::Peak, 12800.0, 2.0, true),
        b(FilterKind::HighShelf, 18000.0, 0.707, true),
        b(FilterKind::LowPass, 18000.0, 0.707, false),
    ]
}

/// Column label for a band ("HP", "L", "1".., "H", "LP").
fn band_label(bands: &[EqBand], i: usize) -> String {
    match bands[i].kind {
        FilterKind::HighPass => "HP".into(),
        FilterKind::LowPass => "LP".into(),
        FilterKind::LowShelf => "L".into(),
        FilterKind::HighShelf => "H".into(),
        _ => {
            let n = bands[..=i]
                .iter()
                .filter(|b| {
                    !matches!(
                        b.kind,
                        FilterKind::HighPass
                            | FilterKind::LowPass
                            | FilterKind::LowShelf
                            | FilterKind::HighShelf
                    )
                })
                .count();
            format!("{n}")
        }
    }
}

struct Axes {
    r: Rect,
    lo: f32,
    hi: f32,
    range: f32,
}

impl Axes {
    fn x(&self, f: f32) -> f32 {
        self.r.left()
            + ((f.max(1.0).ln() - self.lo.ln()) / (self.hi.ln() - self.lo.ln())) * self.r.width()
    }
    fn f(&self, x: f32) -> f32 {
        (self.lo.ln()
            + ((x - self.r.left()) / self.r.width()).clamp(0.0, 1.0)
                * (self.hi.ln() - self.lo.ln()))
        .exp()
    }
    fn y(&self, d: f32) -> f32 {
        self.r.center().y - (d / self.range).clamp(-1.0, 1.0) * (self.r.height() * 0.5)
    }
    fn d(&self, y: f32) -> f32 {
        ((self.r.center().y - y) / (self.r.height() * 0.5)).clamp(-1.0, 1.0) * self.range
    }
}

/// Grid, Hz labels along the bottom and dB labels on the right.
fn draw_axes(p: &egui::Painter, ax: &Axes, outer: Rect) {
    p.rect_filled(outer, 0.0, crate::theme::gray(24));
    p.rect_filled(ax.r, 0.0, crate::theme::gray(30));
    let mut f = 10.0f32;
    while f <= 30_000.0 {
        for m in 1..10 {
            let fr = f * m as f32;
            if fr < ax.lo || fr > ax.hi {
                continue;
            }
            let x = ax.x(fr);
            let major = m == 1;
            p.line_segment(
                [pos2(x, ax.r.top()), pos2(x, ax.r.bottom())],
                Stroke::new(1.0, if major { GRID_MAJOR() } else { GRID() }),
            );
            let show = matches!(m, 1 | 2 | 3 | 4 | 5 | 7) || fr < 100.0;
            if show {
                let t = if fr >= 1000.0 {
                    format!("{}k", fr / 1000.0)
                } else {
                    format!("{fr}")
                };
                p.text(
                    pos2(x, ax.r.bottom() + 3.0),
                    Align2::CENTER_TOP,
                    t,
                    FontId::proportional(if major { 10.5 } else { 9.5 }),
                    if major { Color32::WHITE } else { LABEL() },
                );
            }
        }
        f *= 10.0;
    }
    p.text(
        pos2(ax.r.left() + 2.0, ax.r.bottom() + 3.0),
        Align2::LEFT_TOP,
        "Hz",
        FontId::proportional(9.5),
        LABEL(),
    );
    let step = if ax.range > 40.0 { 12.0 } else { 5.0 };
    let mut d = -ax.range;
    while d <= ax.range + 0.01 {
        let y = ax.y(d);
        p.line_segment(
            [pos2(ax.r.left(), y), pos2(ax.r.right(), y)],
            Stroke::new(1.0, if d.abs() < 0.01 { GRID_MAJOR() } else { GRID() }),
        );
        p.line_segment(
            [pos2(ax.r.right(), y), pos2(ax.r.right() + 4.0, y)],
            Stroke::new(1.0, LABEL()),
        );
        p.text(
            pos2(ax.r.right() + 7.0, y),
            Align2::LEFT_CENTER,
            format!("{d:.0}"),
            FontId::proportional(9.5),
            LABEL(),
        );
        d += step;
    }
    p.text(
        pos2(ax.r.right() + 7.0, ax.r.top() - 9.0),
        Align2::LEFT_CENTER,
        "dB",
        FontId::proportional(9.5),
        LABEL(),
    );
}

fn response_points(ax: &Axes, filters: &[Biquad], rate: f32, gain_db: f32) -> Vec<Pos2> {
    let steps = (ax.r.width() as usize).max(2);
    (0..steps)
        .map(|i| {
            let x = ax.r.left() + i as f32;
            let f = ax.f(x).min(rate * 0.499);
            let mag: f32 = filters.iter().map(|b| b.response(f, rate)).product();
            pos2(x, ax.y(db(mag) + gain_db))
        })
        .collect()
}

/// A read-only frequency response graph (Scientific, Notch and Graphic filters).
pub fn response_view(
    ui: &mut egui::Ui,
    filters: &[Biquad],
    rate: f32,
    range_db: f32,
    gain_db: f32,
) {
    let w = ui.available_width().clamp(260.0, 720.0);
    let (outer, _) = ui.allocate_exact_size(vec2(w, 170.0), Sense::hover());
    let ax = Axes {
        r: Rect::from_min_max(outer.min + vec2(8.0, 12.0), outer.max - vec2(34.0, 20.0)),
        lo: 20.0,
        hi: (rate * 0.5).min(30_000.0),
        range: range_db,
    };
    let p = ui.painter_at(outer);
    draw_axes(&p, &ax, outer);
    p.add(egui::Shape::line(
        response_points(&ax, filters, rate, gain_db),
        Stroke::new(2.0, CURVE),
    ));
}

/// The phase curve of the Graphic Phase Shifter: degrees against frequency on a black grid, the
/// degree scale on the right, the frequency scale along the bottom. Click to add a point, drag
/// to move one, double-click a point to remove it (the end points stay). Returns true on change.
pub fn phase_curve_editor(
    ui: &mut egui::Ui,
    points: &mut Vec<(f32, f32)>,
    log: bool,
    range: f32,
    nyq: f32,
) -> bool {
    let w = ui.available_width().clamp(360.0, 760.0);
    let (outer, resp) = ui.allocate_exact_size(vec2(w, 360.0), Sense::click_and_drag());
    let r = Rect::from_min_max(outer.min + vec2(8.0, 8.0), outer.max - vec2(46.0, 22.0));
    let p = ui.painter_at(outer);
    p.rect_filled(outer, 0.0, crate::theme::gray(24));
    p.rect_filled(r, 0.0, Color32::BLACK);
    let (lo, hi) = (20.0f32, nyq.clamp(1000.0, 22050.0));
    let tx = |f: f32| {
        if log {
            (f.max(lo).ln() - lo.ln()) / (hi.ln() - lo.ln())
        } else {
            (f - lo) / (hi - lo)
        }
    };
    let x_of = |f: f32| r.left() + tx(f).clamp(0.0, 1.0) * r.width();
    let f_of = |x: f32| {
        let t = ((x - r.left()) / r.width()).clamp(0.0, 1.0);
        if log {
            (lo.ln() + t * (hi.ln() - lo.ln())).exp()
        } else {
            lo + t * (hi - lo)
        }
    };
    let y_of = |d: f32| r.center().y - (d / range).clamp(-1.0, 1.0) * r.height() * 0.5;
    let d_of = |y: f32| ((r.center().y - y) / (r.height() * 0.5)).clamp(-1.0, 1.0) * range;
    // frequency grid and labels
    let mut dec = 10.0f32;
    while dec <= hi {
        for m in 1..10 {
            let f = dec * m as f32;
            if f < lo || f > hi {
                continue;
            }
            let x = x_of(f);
            let major = m == 1;
            p.line_segment(
                [pos2(x, r.top()), pos2(x, r.bottom())],
                Stroke::new(1.0, if major { GRID_MAJOR() } else { GRID() }),
            );
            let show = major || matches!(m, 2 | 3 | 4 | 5 | 6 | 8) && (log || m % 2 == 0);
            if show {
                let t = if f >= 1000.0 {
                    format!("{}k", f / 1000.0)
                } else {
                    format!("{f}")
                };
                p.text(
                    pos2(x, r.bottom() + 3.0),
                    Align2::CENTER_TOP,
                    t,
                    FontId::proportional(if major { 10.5 } else { 9.5 }),
                    if major { Color32::WHITE } else { LABEL() },
                );
            }
        }
        dec *= 10.0;
    }
    p.text(
        pos2(r.left() + 1.0, r.bottom() + 3.0),
        Align2::LEFT_TOP,
        "Hz",
        FontId::proportional(9.5),
        LABEL(),
    );
    // degree grid and labels on the right
    let step = if range > 200.0 { 40.0 } else { 20.0 };
    let mut d = -range;
    while d <= range + 0.1 {
        let y = y_of(d);
        p.line_segment(
            [pos2(r.left(), y), pos2(r.right(), y)],
            Stroke::new(1.0, if d.abs() < 0.1 { GRID_MAJOR() } else { GRID() }),
        );
        p.line_segment(
            [pos2(r.right(), y), pos2(r.right() + 4.0, y)],
            Stroke::new(1.0, LABEL()),
        );
        p.text(
            pos2(r.right() + 7.0, y),
            Align2::LEFT_CENTER,
            format!("{d:.0}\u{b0}"),
            FontId::proportional(9.5),
            LABEL(),
        );
        d += step;
    }
    // keep two end points and the order
    if points.len() < 2 {
        *points = vec![(lo, 0.0), (hi, 0.0)];
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut changed = false;
    let did = resp.id.with("phase_drag");
    let mut drag: Option<usize> = ui.data(|d| d.get_temp(did)).flatten();
    let near = |q: Pos2, pts: &[(f32, f32)]| {
        pts.iter()
            .enumerate()
            .map(|(i, pt)| (i, pos2(x_of(pt.0), y_of(pt.1)).distance(q)))
            .filter(|(_, d)| *d < 9.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    if resp.drag_started() {
        drag = ui.input(|i| i.pointer.press_origin()).and_then(|q| {
            near(q, points).or_else(|| {
                if r.contains(q) {
                    points.push((f_of(q.x), d_of(q.y)));
                    points.sort_by(|a, b| a.0.total_cmp(&b.0));
                    near(q, points)
                } else {
                    None
                }
            })
        });
    }
    if resp.dragged() {
        if let (Some(i), Some(q)) = (drag, resp.interact_pointer_pos()) {
            if i < points.len() {
                let last = points.len() - 1;
                // the end points stay at the ends; the others move between their neighbours
                let f = if i == 0 {
                    lo
                } else if i == last {
                    hi
                } else {
                    f_of(q.x).clamp(points[i - 1].0 + 0.1, points[i + 1].0 - 0.1)
                };
                points[i] = (f, (d_of(q.y)).round());
                changed = true;
            }
        }
    }
    if resp.drag_stopped() {
        drag = None;
    }
    if resp.clicked() {
        if let Some(q) = resp.interact_pointer_pos().filter(|q| r.contains(*q)) {
            if near(q, points).is_none() {
                points.push((f_of(q.x), d_of(q.y).round()));
                points.sort_by(|a, b| a.0.total_cmp(&b.0));
                changed = true;
            }
        }
    }
    if resp.double_clicked() {
        if let Some(i) = resp.interact_pointer_pos().and_then(|q| near(q, points)) {
            if i > 0 && i + 1 < points.len() {
                points.remove(i);
                changed = true;
            }
        }
    }
    ui.data_mut(|d| d.insert_temp(did, drag));
    let line: Vec<Pos2> = points
        .iter()
        .map(|(f, d)| pos2(x_of(*f), y_of(*d)))
        .collect();
    p.add(egui::Shape::line(line.clone(), Stroke::new(2.0, CURVE)));
    for q in line {
        p.rect_filled(
            Rect::from_center_size(q, vec2(6.0, 6.0)),
            0.0,
            Color32::WHITE,
        );
    }
    if let Some(h) = resp.hover_pos().filter(|h| r.contains(*h)) {
        p.text(
            r.left_top() + vec2(6.0, 4.0),
            Align2::LEFT_TOP,
            format!("{:.0} Hz   {:+.0}\u{b0}", f_of(h.x), d_of(h.y)),
            FontId::monospace(10.0),
            crate::theme::gray(200),
        );
    }
    changed
}

/// The parametric EQ editor. Returns true when something changed.
pub fn parametric(
    ui: &mut egui::Ui,
    bands: &mut Vec<EqBand>,
    master_db: &mut f32,
    rate: f32,
) -> bool {
    let mut changed = false;
    if bands.is_empty() {
        *bands = standard_bands();
        changed = true;
    }
    let range_id = ui.make_persistent_id("eq_range");
    let mut range: f32 = ui.data(|d| d.get_temp(range_id)).unwrap_or(15.0);
    let w = ui.available_width().clamp(420.0, 760.0);
    let (outer, resp) = ui.allocate_exact_size(vec2(w, 300.0), Sense::click_and_drag());
    // master gain slider on the left, graph to the right
    let gain_col = Rect::from_min_max(outer.min, pos2(outer.left() + 70.0, outer.bottom()));
    let ax = Axes {
        r: Rect::from_min_max(
            pos2(gain_col.right() + 10.0, outer.top() + 14.0),
            pos2(outer.right() - 34.0, outer.bottom() - 22.0),
        ),
        lo: 20.0,
        hi: (rate * 0.5).min(30_000.0),
        range,
    };
    let p = ui.painter_at(outer);
    draw_axes(
        &p,
        &ax,
        Rect::from_min_max(pos2(gain_col.right(), outer.top()), outer.max),
    );
    // master gain (vertical, -80..+40 like a console fader scale)
    {
        let gr = Rect::from_min_max(
            pos2(gain_col.center().x - 3.0, ax.r.top() + 8.0),
            pos2(gain_col.center().x + 3.0, ax.r.bottom() - 18.0),
        );
        p.text(
            pos2(gain_col.center().x, outer.top() + 4.0),
            Align2::CENTER_TOP,
            "Gain",
            FontId::proportional(11.5),
            crate::theme::gray(200),
        );
        p.rect_filled(gr, 0.0, crate::theme::gray(18));
        let gy = |g: f32| gr.bottom() - ((g + 80.0) / 120.0).clamp(0.0, 1.0) * gr.height();
        for g in [40.0f32, 20.0, 0.0, -20.0, -40.0, -60.0, -80.0] {
            let y = gy(g);
            p.line_segment(
                [pos2(gr.left() - 6.0, y), pos2(gr.left() - 2.0, y)],
                Stroke::new(1.0, LABEL()),
            );
            p.text(
                pos2(gr.left() - 8.0, y),
                Align2::RIGHT_CENTER,
                format!("{g:.0}"),
                FontId::proportional(9.0),
                LABEL(),
            );
        }
        let mid = ui.interact(
            gr.expand2(vec2(10.0, 4.0)),
            ui.scope_id().with("eq_master"),
            Sense::click_and_drag(),
        );
        if mid.dragged() {
            if let Some(q) = mid.interact_pointer_pos() {
                *master_db =
                    (-80.0 + ((gr.bottom() - q.y) / gr.height()).clamp(0.0, 1.0) * 120.0).round();
                changed = true;
            }
        }
        if mid.double_clicked() {
            *master_db = 0.0;
            changed = true;
        }
        let ky = gy(*master_db);
        p.circle_filled(pos2(gr.center().x, ky), 6.0, crate::theme::gray(200));
        p.circle_stroke(
            pos2(gr.center().x, ky),
            6.0,
            Stroke::new(1.0, crate::theme::gray(60)),
        );
        p.text(
            pos2(gain_col.center().x, outer.bottom() - 4.0),
            Align2::CENTER_BOTTOM,
            format!("{:.0} dB", master_db),
            FontId::proportional(12.0),
            VALUE,
        );
    }
    // response curve
    let filters: Vec<Biquad> = bands
        .iter()
        .filter(|b| b.on)
        .map(|b| Biquad::new(b.kind, rate, b.freq, b.q, b.gain_db))
        .collect();
    p.add(egui::Shape::line(
        response_points(&ax, &filters, rate, *master_db),
        Stroke::new(2.0, CURVE),
    ));
    // band handles: squares on the curve's own band line, labelled above
    let handle_pos = |b: &EqBand| {
        pos2(
            ax.x(b.freq),
            ax.y(
                if matches!(
                    b.kind,
                    FilterKind::HighPass
                        | FilterKind::LowPass
                        | FilterKind::Notch
                        | FilterKind::BandPass
                ) {
                    0.0
                } else {
                    b.gain_db
                },
            ),
        )
    };
    let drag_id = resp.id.with("eq_band");
    let mut dragging: Option<usize> = ui.data(|d| d.get_temp(drag_id)).flatten();
    if resp.drag_started() {
        dragging = ui.input(|i| i.pointer.press_origin()).and_then(|q| {
            bands
                .iter()
                .enumerate()
                .filter(|(_, b)| b.on)
                .map(|(i, b)| (i, handle_pos(b).distance(q)))
                .filter(|(_, d)| *d < 12.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        });
    }
    if resp.dragged() {
        if let (Some(i), Some(q)) = (dragging, resp.interact_pointer_pos()) {
            if let Some(b) = bands.get_mut(i) {
                b.freq = ax.f(q.x).clamp(10.0, rate * 0.49).round();
                if b.kind.uses_gain() {
                    b.gain_db = (ax.d(q.y) * 10.0).round() / 10.0;
                }
                changed = true;
            }
        }
    }
    if resp.drag_stopped() {
        dragging = None;
    }
    ui.data_mut(|d| d.insert_temp(drag_id, dragging));
    for i in 0..bands.len() {
        if !bands[i].on {
            continue;
        }
        let hp = handle_pos(&bands[i]);
        let r = Rect::from_center_size(hp, vec2(7.0, 7.0));
        p.rect_filled(r, 0.0, Color32::WHITE);
        p.text(
            hp - vec2(0.0, 12.0),
            Align2::CENTER_CENTER,
            band_label(bands, i),
            FontId::proportional(11.0),
            Color32::WHITE,
        );
    }
    if let Some(h) = resp.hover_pos() {
        if ax.r.contains(h) {
            p.text(
                ax.r.left_top() + vec2(6.0, 4.0),
                Align2::LEFT_TOP,
                format!("{:.0} Hz   {:+.1} dB", ax.f(h.x), ax.d(h.y)),
                FontId::monospace(10.0),
                crate::theme::gray(190),
            );
        }
    }
    ui.add_space(6.0);
    // band table: Frequency / Gain / Q, then the band buttons
    egui::Grid::new(ui.next_auto_id())
        .num_columns(bands.len() + 1)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Frequency").color(LABEL()));
            for b in bands.iter_mut() {
                let col = if b.on { VALUE } else { crate::theme::gray(90) };
                let sp = (b.freq * 0.01).max(0.5);
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut b.freq)
                            .range(10.0..=22_000.0)
                            .speed(sp)
                            .max_decimals(0)
                            .suffix(" Hz")
                            .custom_formatter(|v, _| {
                                if v >= 1000.0 {
                                    format!("{:.1}k", v / 1000.0)
                                } else {
                                    format!("{v:.0}")
                                }
                            }),
                    )
                    .on_hover_text("Drag or type the frequency")
                    .changed();
                let _ = col;
            }
            ui.end_row();
            ui.label(egui::RichText::new("Gain").color(LABEL()));
            for b in bands.iter_mut() {
                if b.kind.uses_gain() {
                    changed |= ui
                        .add_enabled(
                            b.on,
                            egui::DragValue::new(&mut b.gain_db)
                                .range(-range..=range)
                                .speed(0.1)
                                .max_decimals(1)
                                .suffix(" dB"),
                        )
                        .changed();
                } else {
                    // HP/LP: slope instead of gain
                    ui.label(
                        egui::RichText::new("12dB/Oct")
                            .small()
                            .color(crate::theme::gray(if b.on { 170 } else { 90 })),
                    );
                }
            }
            ui.end_row();
            ui.label(egui::RichText::new("Q / Width").color(LABEL()));
            for b in bands.iter_mut() {
                changed |= ui
                    .add_enabled(
                        b.on,
                        egui::DragValue::new(&mut b.q)
                            .range(0.1..=40.0)
                            .speed(0.01)
                            .max_decimals(2),
                    )
                    .changed();
            }
            ui.end_row();
            ui.label(egui::RichText::new("Band").color(LABEL()));
            for i in 0..bands.len() {
                let label = band_label(bands, i);
                let on = bands[i].on;
                let btn = egui::Button::new(
                    egui::RichText::new(label)
                        .color(if on { VALUE } else { crate::theme::gray(150) })
                        .strong(),
                )
                .min_size(vec2(36.0, 22.0))
                .fill(crate::theme::gray(if on { 40 } else { 30 }))
                .stroke(Stroke::new(1.0, crate::theme::gray(70)));
                if ui
                    .add(btn)
                    .on_hover_text("Turn this band on or off")
                    .clicked()
                {
                    bands[i].on = !on;
                    changed = true;
                }
            }
            ui.end_row();
        });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Range:").color(LABEL()));
        if ui.radio(range < 20.0, "30 dB").clicked() {
            range = 15.0;
        }
        if ui.radio(range > 20.0, "96 dB").clicked() {
            range = 48.0;
        }
        ui.add_space(12.0);
        if ui
            .button("Reset")
            .on_hover_text("Flat response, standard bands")
            .clicked()
        {
            *bands = standard_bands();
            *master_db = 0.0;
            changed = true;
        }
    });
    ui.data_mut(|d| d.insert_temp(range_id, range));
    changed
}
