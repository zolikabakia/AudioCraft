//! Multitrack editor (track headers, clips, tools) and the Mixer view.

use crate::app::{App, Tool};
use crate::audio;
use crate::curves;
use crate::keys::{EnvTarget, KeyView, VKind};
use crate::theme::{self, Icon};
use crate::ui_wave::{draw_ruler, draw_wave, ruler_ticks};
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};

/// Every time a drag with Snapping on can stick to, given the person's snapping-target
/// settings (the magnet menu in the Multitrack Tools corner, Preferences > Multitrack).
/// `span`/`width` are the visible timeline span and lane width, so the time-display targets
/// are the ruler's labelled (major) marks; `view` is the visible time range.
pub fn collect_snap_targets(
    s: &crate::session::Session,
    p: &crate::prefs::Prefs,
    cursor: f64,
    span: f64,
    lanes_width: f32,
    view: (f64, f64),
) -> Vec<f64> {
    collect_snap_targets_except(s, p, cursor, span, lanes_width, view, None)
}

/// [`collect_snap_targets`] without the edges of clip `except` (the clip being dragged: its
/// own edges move with it). Every other clip's starts and ends count, on its own track and on
/// the tracks above and below, so a dragged clip or a trimmed edge lines up with any of them.
pub fn collect_snap_targets_except(
    s: &crate::session::Session,
    p: &crate::prefs::Prefs,
    cursor: f64,
    span: f64,
    lanes_width: f32,
    view: (f64, f64),
    except: Option<u64>,
) -> Vec<f64> {
    let mut v: Vec<f64> = Vec::new();
    if p.snap_session_start {
        v.push(0.0);
    }
    if p.snap_playhead {
        v.push(cursor);
    }
    for t in &s.tracks {
        for c in t.clips.iter().filter(|c| Some(c.id) != except) {
            if p.snap_clip_starts {
                v.push(c.start);
            }
            if p.snap_clip_ends {
                v.push(c.end());
            }
        }
    }
    if p.marker_snap {
        v.extend(s.markers.iter().map(|m| m.time));
    }
    if p.snap_time_display && lanes_width > 1.0 && span > 0.0 {
        let (major, _) = ruler_ticks(span, lanes_width);
        let mut t = (view.0 / major).floor() * major;
        // one tick past the right edge, so drags near the edges still snap
        while t <= view.1 + major {
            v.push(t);
            t += major;
        }
    }
    v
}

#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub enum MDrag {
    #[default]
    None,
    Move {
        id: u64,
        grab: f64,
        orig_start: f64,
        orig_track: usize,
    },
    TrimL {
        id: u64,
    },
    TrimR {
        id: u64,
    },
    FadeIn {
        id: u64,
    },
    FadeOut {
        id: u64,
    },
    Slip {
        id: u64,
        grab: f64,
        orig_offset: f64,
    },
    Env {
        id: u64,
        idx: usize,
    },
    PanEnv {
        id: u64,
        idx: usize,
    },
    MidPan {
        last: egui::Pos2,
    },
    TimeSel {
        anchor: f64,
    },
    Nav {
        grab: f64,
    },
    /// Stretching the time navigator's left / right handle.
    NavL,
    NavR,
    /// Moving the vertical navigator's window, or stretching its top / bottom handle.
    VNav {
        grab: f32,
    },
    VNavTop,
    VNavBot,
    /// Stretching a clip from its left / right top corner (global clip stretching).
    StretchL {
        id: u64,
        end: f64,
        orig_len: f64,
        orig_speed: f64,
    },
    StretchR {
        id: u64,
        orig_len: f64,
        orig_speed: f64,
    },
    /// Dragging a track by its colour strip to another place.
    TrackMove {
        from: usize,
    },
    /// Dragging the border under a track to change its height.
    TrackH {
        ti: usize,
        start_h: f32,
        start_y: f32,
    },
    /// Moving a volume-automation point (usize::MAX is the Master lane).
    AutoPoint {
        track: usize,
        idx: usize,
    },
    /// Dragging the border under an open automation lane to change its height.
    LaneH {
        ti: usize,
        start_h: f32,
        start_y: f32,
    },
    /// Moving a point just added to a clip's fade curve.
    FadePt {
        id: u64,
        out: bool,
        idx: usize,
    },
    /// Dragging a clip's volume icon (bottom left) up or down: its gain.
    ClipGain {
        id: u64,
        y0: f32,
        db0: f32,
    },
}

impl MDrag {
    /// A drag of a real object (a clip, a keyframe, a handle) that Shift can lock to the main
    /// 8 directions. View drags (the navigators, the marquee selection) are not locked.
    pub fn is_object_drag(self) -> bool {
        matches!(
            self,
            MDrag::Move { .. }
                | MDrag::TrimL { .. }
                | MDrag::TrimR { .. }
                | MDrag::FadeIn { .. }
                | MDrag::FadeOut { .. }
                | MDrag::Slip { .. }
                | MDrag::StretchL { .. }
                | MDrag::StretchR { .. }
                | MDrag::Env { .. }
                | MDrag::PanEnv { .. }
                | MDrag::FadePt { .. }
                | MDrag::ClipGain { .. }
                | MDrag::AutoPoint { .. }
        )
    }
}

#[derive(Default)]
pub struct MultiDrag {
    pub drag: MDrag,
    pub scroll_y: f32,
    pub changed: bool,
    /// Last frame's layout (clip area, track rows, view): where files from outside land.
    pub geom: Option<(Rect, Vec<Rect>, (f64, f64))>,
    /// Selected keyframes, a keyframe or handle drag, and whether a keyframe is under the pointer.
    pub key_sel: crate::keys::KeySel,
    /// The keyframe selection at the end of the last frame (to see when keyframes are picked).
    pub key_sel_seen: crate::keys::KeySel,
    /// The clip selection (main clip, all selected clips) at the last selection check.
    pub clip_sel_seen: (Option<u64>, Vec<u64>),
    /// The selected empty spans at the last selection check (they share the clip selection).
    pub gap_sel_seen: Vec<crate::session::GapSel>,
    pub key_drag: Option<crate::keys::KeyDrag>,
    pub key_hovered: bool,
    /// Where each keyframe was drawn last frame (for tests and hit checks).
    pub key_spots: Vec<(crate::keys::EnvTarget, usize, egui::Pos2)>,
    /// Where each Multitrack Tools button was drawn last frame (for tests and hit checks).
    pub tool_spots: Vec<(u8, Rect)>,
    /// The corner panel the Multitrack Tools sit in last frame (for tests and hit checks).
    pub tool_panel: Option<Rect>,
    /// Where the pre-render gauge was drawn last frame (tests).
    pub prerender_bar: Option<Rect>,
    /// The time a dragged clip edge (trim or stretch) is snapped to, for the snap line.
    pub edge_snap: Option<f64>,
    /// Where a fade-handle drag started (pointer y) and the fade's shape then.
    pub fade_start: (f32, f32),
    /// The clips a Move drag takes with it, recorded where each sat when the drag began:
    /// the whole selection when the grabbed clip is one of several picked, plus its
    /// group. Empty unless a Move drag is on.
    pub crew: Vec<(u64, f64, usize)>,
}

impl App {
    /// Where a file dropped from outside the app at `p` goes: (track, time). Over the clip area
    /// it is the track and time under the pointer (below the last track: a new track);
    /// elsewhere the selected track at the playhead.
    pub fn multitrack_drop_target(&self, p: Option<egui::Pos2>) -> (usize, f64) {
        let s = match self.session.as_ref() {
            Some(s) => s,
            None => return (0, 0.0),
        };
        if let (Some(p), Some((lanes, rows, view))) = (p, &self.multi_drag.geom) {
            if lanes.contains(p) {
                let t = (view.0
                    + ((p.x - lanes.left()) / lanes.width().max(1.0)) as f64 * (view.1 - view.0))
                    .max(0.0);
                let tk = rows
                    .iter()
                    .position(|r| p.y >= r.top() && p.y < r.bottom())
                    .unwrap_or(s.tracks.len());
                return (tk, t);
            }
        }
        (s.selected_track.min(s.tracks.len()), s.cursor)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    None,
    Body(u64),
    Header(u64),
    LeftEdge(u64),
    RightEdge(u64),
    FadeIn(u64),
    FadeOut(u64),
    EnvPoint(u64, usize),
    EnvLine(u64),
    PanPoint(u64, usize),
    PanLine(u64),
    StretchL(u64),
    StretchR(u64),
    /// On a clip's fade curve (false = fade-in, true = fade-out): click adds a shaping point.
    FadeLine(u64, bool),
    /// The volume icon in a clip's bottom-left corner: drag up/down for the clip's gain.
    Gain(u64),
}

/// Where a clip's volume icon sits: its bottom-left corner.
pub fn gain_icon_rect(cr: Rect) -> Rect {
    Rect::from_min_size(pos2(cr.left() + 6.0, cr.bottom() - 14.0), vec2(14.0, 10.0))
}

/// A fade shape after dragging its handle from `y0` to `y`: up bends it up (to +1), down
/// bends it down (to -1); 80 px from plain to the end.
pub fn fade_shape_drag(shape0: f32, y0: f32, y: f32) -> f32 {
    let v = shape0 + (y0 - y) / 80.0;
    if v.is_finite() {
        v.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// A clip's gain after dragging its volume icon from `y0` to `y`: a tenth of a dB per pixel,
/// up is louder, from -96 dB to +24 dB.
pub fn clip_gain_drag(db0: f32, y0: f32, y: f32) -> f32 {
    let v = ((db0 + (y0 - y) * 0.1) * 10.0).round() / 10.0;
    if v.is_finite() {
        v.clamp(-96.0, 24.0)
    } else {
        db0
    }
}

/// One click on a clip. Where the black mouse tool grabs — anywhere with the Move tool,
/// or any tool over a clip's top bar, the one grab zone every tool shares — Shift+click
/// selects every clip of the clicked clip's row and Ctrl+click toggles just that clip.
/// Elsewhere the tool keeps its own click, which selects the clip alone.
fn pick_clip(s: &mut crate::session::Session, id: u64, grab: bool, shift: bool, ctrl: bool) {
    if grab && shift {
        s.select_row_clips(id);
    } else {
        s.select_clips(id, grab && ctrl);
    }
}

/// The rect a clip's volume/pan keyframes are interactive in: the clip's full extent within
/// the track area, so keyframes at the clip's edges stay grabbable even when the clip is
/// scrolled so the edge is at or behind the visible clip area.
pub fn key_clip(cr: Rect, tracks_rect: Rect) -> Rect {
    cr.intersect(tracks_rect)
}

/// Whether the pointer is on a clip's left edge handle: within the usual 7px of the clip's
/// left edge, or — when the clip is scrolled so the edge is behind the visible area — within
/// 7px of the visible boundary, so the clip's length stays trimmable there.
pub fn on_left_edge(r: Rect, lanes: Rect, pp: Pos2) -> bool {
    (pp.x - r.left() < 7.0) || (lanes.left() - r.left() > 0.0 && pp.x < lanes.left() + 7.0)
}

/// Same for the right edge: when the clip extends past the visible right edge, its edge
/// handle stays reachable at the boundary.
pub fn on_right_edge(r: Rect, lanes: Rect, pp: Pos2) -> bool {
    (r.right() - pp.x < 7.0) || (r.right() - lanes.right() > 0.0 && pp.x > lanes.right() - 7.0)
}

/// Height of a clip-volume envelope point, on the original's scale: the clip's own level sits
/// 65% of the way up, the top is +6 dB and the bottom is silence (the same scale the session
/// files store, so lines look where the original draws them).
pub fn env_y(body: Rect, db: f32) -> f32 {
    let n = crate::sesx::env_key_value(db) as f32;
    body.bottom() - 3.0 - n * (body.height() - 6.0).max(1.0)
}

/// Pan envelope: centre line = centre, left (-1) up, right (+1) down.
pub fn pan_y(body: Rect, p: f32) -> f32 {
    body.center().y + p.clamp(-1.0, 1.0) * (body.height() * 0.5 - 3.0).max(1.0)
}

pub fn pan_at(body: Rect, y: f32) -> f32 {
    ((y - body.center().y) / (body.height() * 0.5 - 3.0).max(1.0)).clamp(-1.0, 1.0)
}

pub fn env_db_at(body: Rect, y: f32) -> f32 {
    let n = ((body.bottom() - 3.0 - y) / (body.height() - 6.0).max(1.0)).clamp(0.0, 1.0);
    ((crate::sesx::env_key_db(n as f64) * 10.0).round() / 10.0).clamp(-96.0, 6.1)
}

/// Screen y for a dB value on an automation lane: the same scale as clip volume lines (0 dB at
/// 65 % of the height, +6 dB at the top, silence at the bottom), like the original's lanes.
pub fn auto_y(r: Rect, db: f32) -> f32 {
    env_y(r, db)
}

/// dB value under a point on an automation lane.
pub fn auto_db_at(r: Rect, y: f32) -> f32 {
    env_db_at(r, y)
}

/// Clip volume line colour and pan line colour (the original's yellow and blue).
pub const VOL_COL: Color32 = Color32::from_rgb(235, 205, 60);
pub const PAN_COL: Color32 = Color32::from_rgb(90, 165, 255);

/// Screen points of an envelope's line across `r` (curves followed), held flat at both ends.
fn env_line(
    env: &[(f64, f32)],
    hs: &[curves::Handles],
    x_of: &dyn Fn(f64) -> f32,
    y_of: &dyn Fn(f32) -> f32,
    left: f32,
    right: f32,
    empty: f32,
) -> Vec<egui::Pos2> {
    let pts = curves::polyline(env, hs, false, 24);
    let first = pts.first().map(|p| p.1).unwrap_or(empty);
    let last = pts.last().map(|p| p.1).unwrap_or(empty);
    let mut out = vec![pos2(left, y_of(first))];
    for (t, v) in &pts {
        let x = x_of(*t);
        if x < left - 2000.0 || x > right + 2000.0 {
            continue;
        }
        out.push(pos2(x, y_of(*v)));
    }
    out.push(pos2(right, y_of(last)));
    out
}

/// Distance from `p` to a polyline.
fn dist_to_line(pts: &[egui::Pos2], p: egui::Pos2) -> f32 {
    let mut best = f32::MAX;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let l2 = ab.length_sq();
        let u = if l2 > 1e-6 {
            ((p - a).dot(ab) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        best = best.min((a + ab * u).distance(p));
    }
    best
}

/// The little disclosure chevron left of the automation-mode selector: closed points right,
/// open points down. Unlike a static label it can be clicked and has a tooltip.
fn automation_disclosure(ui: &mut egui::Ui, open: bool, tip: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::click());
    let col = if resp.hovered() {
        Color32::WHITE
    } else {
        theme::text_dim()
    };
    let p = ui.painter();
    let c = r.center();
    let st = Stroke::new(1.4, col);
    if open {
        // the open (down) chevron is as wide as the closed one is tall, and starts where the
        // closed one does, so it never reaches into the track's colour strip on the left
        let cx = c.x + 0.5;
        p.line_segment([pos2(cx - 4.0, c.y - 2.0), pos2(cx, c.y + 2.0)], st);
        p.line_segment([pos2(cx + 4.0, c.y - 2.0), pos2(cx, c.y + 2.0)], st);
    } else {
        p.line_segment([pos2(c.x - 2.0, c.y - 5.0), pos2(c.x + 3.0, c.y)], st);
        p.line_segment([pos2(c.x - 2.0, c.y + 5.0), pos2(c.x + 3.0, c.y)], st);
    }
    resp.on_hover_text(tip)
}

/// Draw one envelope of an automation lane (its keyframes come from the keyframe layer).
/// Returns the line's screen points, for clicking on it.
#[allow(clippy::too_many_arguments)]
fn paint_auto_env(
    p: &egui::Painter,
    r: Rect,
    env: &[(f64, f32)],
    hs: &[curves::Handles],
    x_of: &dyn Fn(f64) -> f32,
    pan: bool,
    editing: bool,
    read: bool,
) -> Vec<egui::Pos2> {
    if !r.is_positive() {
        return vec![];
    }
    let base = if pan { PAN_COL } else { VOL_COL };
    let col = if !read {
        crate::theme::gray(120)
    } else if editing {
        base
    } else {
        base.gamma_multiply(0.45)
    };
    let y_of = |v: f32| if pan { pan_y(r, v) } else { auto_y(r, v) };
    let pts = env_line(env, hs, x_of, &y_of, r.left(), r.right(), 0.0);
    p.add(egui::Shape::line(
        pts.clone(),
        Stroke::new(if editing { 1.5 } else { 1.0 }, col),
    ));
    pts
}

/// The open automation lane's background and its 0 dB / centre guide.
fn paint_lane_bg(p: &egui::Painter, r: Rect) {
    if !r.is_positive() {
        return;
    }
    p.rect_filled(r, 0.0, theme::auto_lane());
    p.line_segment(
        [r.left_top(), r.right_top()],
        Stroke::new(1.0, crate::theme::gray(20)),
    );
}

/// Add a shaping point on a clip's fade curve where it was clicked; returns its index.
fn fade_point_at(
    s: &mut crate::session::Session,
    eff: &std::collections::HashMap<u64, (f64, f64, bool, bool)>,
    id: u64,
    out: bool,
    t: f64,
    pp: egui::Pos2,
    hits: &[(Rect, u64, usize)],
) -> Option<usize> {
    let r = hits.iter().find(|h| h.1 == id)?.0;
    let body_top = r.top() + 15.0;
    let (efin, efout, _, _) = eff.get(&id).copied()?;
    let c = s.clip_mut(id)?;
    let (len, x) = if out {
        (efout, (c.end() - t) / efout.max(1e-9))
    } else {
        (efin, (t - c.start) / efin.max(1e-9))
    };
    if len <= 0.0 {
        return None;
    }
    let x = x.clamp(0.02, 0.98);
    let g = ((r.bottom() - pp.y) / (r.bottom() - body_top).max(1.0)).clamp(0.0, 1.0);
    let (pts, hs) = if out {
        (&mut c.curves.fout_pts, &mut c.curves.fout_h)
    } else {
        (&mut c.curves.fin_pts, &mut c.curves.fin_h)
    };
    Some(curves::insert_key(pts, hs, x, g))
}

fn clip_colors(c: [u8; 3]) -> (Color32, Color32, Color32) {
    let [r, g, b] = c;
    let bg = Color32::from_rgb(
        (r as f32 * 0.32) as u8,
        (g as f32 * 0.32) as u8,
        (b as f32 * 0.32) as u8,
    );
    let head = Color32::from_rgb(
        (r as f32 * 0.6) as u8,
        (g as f32 * 0.6) as u8,
        (b as f32 * 0.6) as u8,
    );
    let wave = Color32::from_rgb(
        r.saturating_add(60),
        g.saturating_add(60),
        b.saturating_add(60),
    );
    (bg, head, wave)
}

#[derive(Clone, Copy, PartialEq)]
enum HeadGlyph {
    Io,
    Fx,
    Sends,
    Eq,
    Metronome,
    Stretch,
    AutoScroll,
}

/// A toolbar toggle at a chosen size (the corner panel's rows are shorter than the old strip).
fn head_tool_sized(
    ui: &mut egui::Ui,
    g: HeadGlyph,
    on: bool,
    tip: &str,
    size: egui::Vec2,
    icon: f32,
) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    // every button has its own face (not the layers behind it)
    let bg = theme::control_fill(on, theme::selected(), &resp, true);
    p.rect_filled(r.shrink2(vec2(0.5, 0.0)), theme::ROUND as f32, bg);
    let col = if on || resp.hovered() {
        theme::strong()
    } else {
        theme::text()
    };
    let c = r.center();
    let ir = Rect::from_center_size(c, vec2(icon, icon));
    let def = match g {
        HeadGlyph::Fx => {
            p.text(
                c,
                egui::Align2::CENTER_CENTER,
                "fx",
                FontId::new(icon + 1.0, egui::FontFamily::Proportional),
                col,
            );
            None
        }
        HeadGlyph::Io => Some(&crate::lucide::IO),
        HeadGlyph::Sends => Some(&crate::lucide::SENDS),
        HeadGlyph::Eq => Some(&crate::lucide::EQ),
        HeadGlyph::Metronome => Some(&crate::lucide::METRONOME),
        HeadGlyph::Stretch => Some(&crate::lucide::STRETCH),
        HeadGlyph::AutoScroll => Some(&crate::lucide::AUTOSCROLL),
    };
    if let Some(d) = def {
        crate::lucide::draw(p, ir, d, col);
    }
    resp.on_hover_text(tip)
}

/// The snapping split button, in the same design style as the other drop-down controls: one
/// rounded face, the magnet on the left (toggles snapping) and a small arrow on the right
/// (opens the snapping-targets dropdown). Returns the magnet's and the arrow's responses.
fn snap_split_button(
    ui: &mut egui::Ui,
    on: bool,
    open: bool,
    snap_tip: &str,
    menu_tip: &str,
) -> (egui::Response, egui::Response) {
    let size = vec2(24.0, 22.0);
    let (r, hov) = ui.allocate_exact_size(size, egui::Sense::hover());
    let hover = hov.hovered();
    let menu_zone_w = 9.0;
    let toggle_r = Rect::from_min_max(r.min, pos2(r.right() - menu_zone_w, r.bottom()));
    let menu_r = Rect::from_min_max(pos2(r.right() - menu_zone_w, r.top()), r.max);
    let toggle = ui.interact(toggle_r, ui.id().with("snap_toggle"), egui::Sense::click());
    let menu = ui.interact(menu_r, ui.id().with("snap_menu"), egui::Sense::click());
    let p = ui.painter();
    // the same face the other corner buttons use (on state tinted, hover and press shown)
    let bg = if on {
        theme::selected()
    } else if toggle.is_pointer_button_down_on() || menu.is_pointer_button_down_on() {
        theme::pressed()
    } else if hover {
        theme::hover()
    } else {
        theme::control()
    };
    p.rect_filled(r.shrink2(vec2(0.5, 0.0)), theme::ROUND as f32, bg);
    let col = if on || hover {
        theme::strong()
    } else {
        theme::text()
    };
    let ir = Rect::from_center_size(pos2(toggle_r.center().x, r.center().y), vec2(12.0, 12.0));
    crate::lucide::draw(p, ir, &crate::lucide::MAGNET, col);
    let chev = Rect::from_center_size(
        pos2(r.right() - menu_zone_w * 0.5, r.center().y + 1.0),
        vec2(6.0, 6.0),
    );
    crate::lucide::draw(
        p,
        chev,
        &crate::lucide::CHEVRON_DOWN,
        if open { theme::strong() } else { col },
    );
    (
        toggle.on_hover_text(snap_tip),
        menu.on_hover_text(menu_tip),
    )
}

/// Width of the coloured strip at the left of each track header.
pub const COLOR_STRIP_W: f32 = 10.0;

/// Mute lights green, Solo yellow (like the original).
pub const MUTE_ON: Color32 = Color32::from_rgb(49, 173, 137);
pub const SOLO_ON: Color32 = Color32::from_rgb(230, 200, 60);

/// Dim small text in a header.
fn ui_dim(ui: &mut egui::Ui, t: &str) {
    ui.label(egui::RichText::new(t).small().color(theme::text_dim()));
}

/// The small icon left of a track's name: click to fold the track to one row (or back).
fn minimize_button(ui: &mut egui::Ui, minimized: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::click_and_drag());
    let col = if resp.hovered() {
        Color32::WHITE
    } else {
        theme::text_dim()
    };
    let p = ui.painter();
    // three short waves
    for k in 0..3 {
        let x = r.left() + 2.0 + k as f32 * 4.0;
        let c = r.center().y;
        p.line_segment([pos2(x, c - 3.0), pos2(x + 2.0, c)], Stroke::new(1.2, col));
        p.line_segment([pos2(x + 2.0, c), pos2(x, c + 3.0)], Stroke::new(1.2, col));
    }
    resp.on_hover_text(if minimized {
        "Restore the track"
    } else {
        "Minimize the track"
    })
}

/// Automation mode menu in the original's order (Off, Read, Write, Latch, Touch).
pub fn automation_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    mode: &mut u8,
) -> bool {
    let names = crate::session::AUTOMATION_MODES;
    let cur = names.get(*mode as usize).copied().unwrap_or("Read");
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .width((ui.available_width() - 4.0).max(40.0))
        .selected_text(cur)
        .show_ui(ui, |ui| {
            for i in [0usize, 1, 4, 2, 3] {
                if ui.selectable_label(*mode as usize == i, names[i]).clicked() {
                    *mode = i as u8;
                    changed = true;
                }
            }
        });
    changed
}

/// The Track EQ's response, small, for the EQ header view.
fn mini_eq(
    p: &egui::Painter,
    r: Rect,
    bands: &[crate::dsp::EqBand],
    master: f32,
    on: bool,
    rate: f32,
) {
    let filters: Vec<crate::dsp::Biquad> = bands
        .iter()
        .filter(|b| b.on)
        .map(|b| crate::dsp::Biquad::new(b.kind, rate, b.freq, b.q, b.gain_db))
        .collect();
    let (lo, hi) = (20f32.ln(), (rate * 0.5).min(22000.0).ln());
    let pts: Vec<egui::Pos2> = (0..=(r.width() as usize).max(2))
        .map(|i| {
            let x = r.left() + i as f32;
            let f = (lo + (i as f32 / r.width().max(1.0)) * (hi - lo)).exp();
            let m: f32 = filters.iter().map(|b| b.response(f, rate)).product();
            let d = (audio::db(m) + master).clamp(-15.0, 15.0);
            pos2(x, r.center().y - d / 15.0 * r.height() * 0.5)
        })
        .collect();
    p.add(egui::Shape::line(
        pts,
        Stroke::new(
            1.2,
            if on {
                Color32::from_rgb(92, 112, 255)
            } else {
                crate::theme::gray(150)
            },
        ),
    ));
}

/// Small square toggle with a letter (M / S / R).
fn letter_toggle(
    ui: &mut egui::Ui,
    on: &mut bool,
    letter: &str,
    on_col: Color32,
    tip: &str,
) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(20.0, 18.0), Sense::click());
    let enabled = ui.is_enabled();
    let bg = if !enabled {
        theme::own_or(crate::theme::gray(40), theme::panel())
    } else if *on {
        on_col
    } else if resp.hovered() {
        theme::own_or(crate::theme::gray(70), theme::hover())
    } else {
        theme::own_or(crate::theme::gray(52), theme::control())
    };
    ui.painter().rect_filled(r, theme::ROUND as f32, bg);
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        FontId::proportional(11.0),
        if !enabled {
            theme::own_or(
                crate::theme::gray(80),
                theme::text_dim().gamma_multiply(0.6),
            )
        } else if *on {
            Color32::BLACK
        } else {
            theme::text()
        },
    );
    if !enabled {
        resp.on_disabled_hover_text(format!("{tip}: choose an input first"));
        return false;
    }
    let clicked = resp.clicked();
    if clicked {
        *on = !*on;
    }
    resp.on_hover_text(tip);
    clicked
}

#[derive(Clone, Copy)]
enum Glyph {
    Volume,
    Pan,
}

/// The small volume (rising bars) and pan (bars both ways) marks in front of the track knobs.
fn small_glyph(ui: &mut egui::Ui, g: Glyph) {
    let (r, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
    let p = ui.painter();
    let col = theme::text_dim();
    let base = r.center().y + 5.0;
    match g {
        Glyph::Volume => {
            for i in 0..5 {
                let x = r.left() + 1.0 + i as f32 * 2.6;
                let h = 2.0 + i as f32 * 2.0;
                p.line_segment([pos2(x, base), pos2(x, base - h)], Stroke::new(1.0, col));
            }
        }
        Glyph::Pan => {
            for i in 0..5 {
                let x = r.left() + 1.0 + i as f32 * 2.6;
                let h = 2.0 + (i as f32 - 2.0).abs() * 3.0;
                p.line_segment([pos2(x, base), pos2(x, base - h)], Stroke::new(1.0, col));
            }
        }
    }
}

impl App {
    fn vertical_navigator(
        &mut self,
        ui: &mut egui::Ui,
        p: &egui::Painter,
        r: Rect,
        view_h: f32,
        total_h: f32,
    ) {
        p.rect_filled(r, 0.0, crate::theme::gray(24));
        let Some(s) = self.session.as_ref() else {
            return;
        };
        let total_h = total_h.max(1.0);
        let inner = r.shrink2(vec2(2.0, 2.0));
        let scale = inner.height() / total_h.max(view_h);
        // miniature: one bar per track in its colour
        let mut y = inner.top();
        for t in &s.tracks {
            let h = t.shown_height() * scale;
            let (_, head, _) = clip_colors(t.color);
            p.rect_filled(
                Rect::from_min_size(
                    pos2(inner.left() + 3.0, y + 0.5),
                    vec2(inner.width() - 6.0, (h - 1.0).max(1.0)),
                ),
                0.0,
                head.gamma_multiply(0.6),
            );
            y += h;
        }
        let win_top = inner.top() + self.multi_drag.scroll_y * scale;
        let win = Rect::from_min_max(
            pos2(inner.left(), win_top),
            pos2(
                inner.right(),
                (win_top + view_h * scale)
                    .min(inner.bottom())
                    .max(win_top + 8.0),
            ),
        );
        p.rect_filled(win, 0.0, crate::theme::gray_a(255, 16));
        p.rect_stroke(
            win,
            0.0,
            Stroke::new(1.0, crate::theme::gray(170)),
            egui::StrokeKind::Inside,
        );
        for gy in [win.top() + 2.5, win.bottom() - 2.5] {
            p.rect_filled(
                Rect::from_center_size(pos2(win.center().x, gy), vec2(win.width() - 6.0, 3.0)),
                0.0,
                crate::theme::gray(190),
            );
        }
        let resp = ui.interact(r, ui.scope_id().with("vnav"), Sense::click_and_drag());
        let near = |y: f32| -> Option<bool> {
            if (y - win.top()).abs() < 5.0 {
                Some(false)
            } else if (y - win.bottom()).abs() < 5.0 {
                Some(true)
            } else {
                None
            }
        };
        let mut height_k = 1.0f32;
        if let Some(hp) = resp.hover_pos() {
            if near(hp.y).is_some()
                || matches!(self.multi_drag.drag, MDrag::VNavTop | MDrag::VNavBot)
            {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            let sy = ui.input(|i| i.smooth_scroll_delta.y);
            if sy.abs() > 0.1 {
                height_k = (1.0025f32).powf(sy);
            }
        }
        if resp.drag_started() {
            let o = ui.input(|i| i.pointer.press_origin()).unwrap_or(r.center());
            self.multi_drag.drag = match near(o.y) {
                Some(false) => MDrag::VNavTop,
                Some(true) => MDrag::VNavBot,
                None => MDrag::VNav {
                    grab: if o.y >= win.top() && o.y <= win.bottom() {
                        o.y - win.top()
                    } else {
                        win.height() / 2.0
                    },
                },
            };
        }
        if resp.dragged() {
            let dy = resp.drag_delta().y;
            if let Some(pp) = resp.interact_pointer_pos() {
                match self.multi_drag.drag {
                    MDrag::VNav { grab } => {
                        self.multi_drag.scroll_y =
                            ((pp.y - grab - inner.top()) / scale.max(1e-6)).max(0.0)
                    }
                    // a taller window shows more tracks, so they get shorter
                    MDrag::VNavBot => height_k = win.height() / (win.height() + dy).max(8.0),
                    MDrag::VNavTop => height_k = win.height() / (win.height() - dy).max(8.0),
                    _ => {}
                }
            }
        }
        if resp.clicked() {
            if let Some(pp) = resp.interact_pointer_pos() {
                self.multi_drag.scroll_y =
                    ((pp.y - inner.top()) / scale.max(1e-6) - view_h / 2.0).max(0.0);
            }
        }
        // which edge of the visible window stays put while the tracks change height: the bottom
        // when the top grip is dragged (so it stretches upward), the top for the bottom grip,
        // the middle for the wheel
        let anchor = match self.multi_drag.drag {
            MDrag::VNavTop => 1.0,
            MDrag::VNavBot => 0.0,
            _ => 0.5,
        };
        if resp.drag_stopped() {
            self.multi_drag.drag = MDrag::None;
        }
        if resp.double_clicked() {
            // fit every track in the view
            if let Some(s) = self.session.as_mut() {
                let open: f32 = s
                    .tracks
                    .iter()
                    .filter(|t| !t.minimized)
                    .map(|t| t.height)
                    .sum::<f32>()
                    .max(1.0);
                let fixed: f32 = s
                    .tracks
                    .iter()
                    .filter(|t| t.minimized)
                    .map(|t| t.shown_height())
                    .sum();
                let k = ((view_h - fixed).max(40.0) / open).max(0.01);
                for t in s.tracks.iter_mut() {
                    t.height = (t.height * k).clamp(40.0, 600.0);
                }
            }
            self.multi_drag.scroll_y = 0.0;
            height_k = 1.0;
        }
        if (height_k - 1.0).abs() > 1e-4 {
            if let Some(s) = self.session.as_mut() {
                for t in s.tracks.iter_mut() {
                    t.height = (t.height * height_k).clamp(40.0, 600.0);
                }
            }
            let a = view_h * anchor;
            self.multi_drag.scroll_y = ((self.multi_drag.scroll_y + a) * height_k - a).max(0.0);
        }
        resp.on_hover_text("Drag to scroll the tracks; drag the top or bottom grip, or use the wheel here, to resize them; double-click to fit all tracks");
    }

    /// The Track EQ window (pencil in the EQ header view): the parametric EQ of one track.
    pub fn track_eq_windows(&mut self, ctx: &egui::Context) {
        let Some(id) = self.track_eq_window else {
            return;
        };
        let Some(s) = self.session.as_mut() else {
            self.track_eq_window = None;
            return;
        };
        let rate = s.rate as f32;
        let Some(t) = s.tracks.iter_mut().find(|t| t.id == id) else {
            self.track_eq_window = None;
            return;
        };
        let mut open = true;
        let mut changed = false;
        egui::Window::new(format!("Track EQ: {}", t.name))
            .id(egui::Id::new(("track_eq", id)))
            .open(&mut open)
            .resizable(false)
            .default_width(760.0)
            .show(ctx, |ui| {
                if t.eq.is_empty() {
                    t.eq = crate::equi::standard_bands();
                }
                changed |= crate::equi::parametric(ui, &mut t.eq, &mut t.eq_master, rate);
                ui.separator();
                ui.horizontal(|ui| {
                    let tip = if t.eq_on {
                        "Track EQ is on (click to bypass)"
                    } else {
                        "Track EQ is off (click to turn on)"
                    };
                    if letter_toggle(ui, &mut t.eq_on, "\u{23fb}", theme::power_on(), tip) {
                        changed = true;
                    }
                    ui.label(
                        egui::RichText::new(format!("Track: {}", t.name)).color(theme::text_dim()),
                    );
                });
            });
        if changed {
            s.touch();
        }
        if !open {
            self.track_eq_window = None;
            self.session_push("Track EQ");
        }
    }

    /// Recording inputs offered on the track headers.
    pub fn input_choices(&self) -> Vec<String> {
        let mut v = vec![
            "Default Stereo Input".to_string(),
            "Mono: Left".to_string(),
            "Mono: Right".to_string(),
        ];
        if self.engine.in_device_name != "(none)" {
            v.push(format!("Stereo: {}", self.engine.in_device_name));
        }
        v
    }
}

/// Shorten `text` with an ellipsis so it fits `max_w` points in the small button font, so a
/// long input or output name never spills out of the track header.
fn fit_text(ui: &egui::Ui, text: &str, max_w: f32) -> String {
    let font = egui::TextStyle::Small.resolve(ui.style());
    crate::panels::middle_ellipsis(ui.painter(), text, font, max_w.max(20.0))
}

/// Vertical fader in dB (-60..+6). Returns (changed, released).
fn fader(ui: &mut egui::Ui, rect: Rect, db: &mut f32) -> (bool, bool) {
    let resp = ui.interact(
        rect,
        ui.scope_id()
            .with(("fader", rect.min.x as i32, rect.min.y as i32)),
        Sense::click_and_drag(),
    );
    let to_pos = |d: f32| -> f32 {
        let d = d.clamp(-60.0, 6.0);
        // a gentle curve: more resolution near 0 dB
        let t = if d >= -12.0 {
            0.55 + (d + 12.0) / 18.0 * 0.45
        } else {
            (d + 60.0) / 48.0 * 0.55
        };
        rect.bottom() - 8.0 - t * (rect.height() - 16.0)
    };
    let from_pos = |y: f32| -> f32 {
        let t = ((rect.bottom() - 8.0 - y) / (rect.height() - 16.0)).clamp(0.0, 1.0);
        if t >= 0.55 {
            -12.0 + (t - 0.55) / 0.45 * 18.0
        } else {
            -60.0 + t / 0.55 * 48.0
        }
    };
    let mut changed = false;
    if resp.dragged() {
        if let Some(p) = resp.interact_pointer_pos() {
            *db = from_pos(p.y);
            changed = true;
        }
    }
    if resp.double_clicked() {
        *db = 0.0;
        changed = true;
    }
    let p = ui.painter();
    let groove = Rect::from_center_size(
        pos2(rect.center().x, rect.center().y),
        vec2(4.0, rect.height() - 12.0),
    );
    p.rect_filled(groove, 0.0, crate::theme::gray(15));
    for (d, l) in [
        (6.0, "+6"),
        (0.0, "0"),
        (-6.0, "-6"),
        (-12.0, "-12"),
        (-24.0, "-24"),
        (-48.0, "-48"),
    ] {
        let y = to_pos(d);
        p.line_segment(
            [pos2(rect.left() + 2.0, y), pos2(rect.left() + 8.0, y)],
            Stroke::new(1.0, theme::text_dim()),
        );
        p.text(
            pos2(rect.left() - 2.0, y),
            egui::Align2::RIGHT_CENTER,
            l,
            FontId::proportional(8.5),
            theme::text_dim(),
        );
    }
    let y = to_pos(*db);
    let cap = Rect::from_center_size(pos2(rect.center().x, y), vec2(rect.width().min(26.0), 12.0));
    p.rect_filled(
        cap,
        0.0,
        if resp.hovered() || resp.dragged() {
            crate::theme::gray(200)
        } else {
            crate::theme::gray(165)
        },
    );
    p.line_segment(
        [pos2(cap.left() + 2.0, y), pos2(cap.right() - 2.0, y)],
        Stroke::new(1.0, crate::theme::gray(40)),
    );
    (changed, resp.drag_stopped() || resp.double_clicked())
}

impl App {
    /// Keep one kind of selection, whichever was picked last: keyframes picked since the last
    /// check deselect every clip (their track stays the current one), and clips picked since
    /// then (a click, Select All, a menu) drop the keyframes. So a clip and a keyframe never
    /// look selected together, and Delete removes exactly what is highlighted.
    pub(crate) fn keys_take_selection(&mut self) {
        use crate::keys::EnvTarget as T;
        let keys_now = self.multi_drag.key_sel.clone();
        let clips_now = self
            .session
            .as_ref()
            .map(|s| (s.selected_clip, s.selected_clips.clone()))
            .unwrap_or_default();
        let gaps_now = self
            .session
            .as_ref()
            .map(|s| s.selected_gaps.clone())
            .unwrap_or_default();
        let keys_changed = keys_now != self.multi_drag.key_sel_seen;
        let clips_changed =
            clips_now != self.multi_drag.clip_sel_seen || gaps_now != self.multi_drag.gap_sel_seen;
        let keys_on = keys_now.target.is_some() && !keys_now.idx.is_empty();
        let clips_on = clips_now.0.is_some() || !clips_now.1.is_empty() || !gaps_now.is_empty();
        if keys_changed && keys_on {
            if let Some(s) = self.session.as_mut() {
                // the keyframe's own track stays the current track, but no clip stays selected
                if let Some(T::ClipVol(id) | T::ClipPan(id) | T::FadeIn(id) | T::FadeOut(id)) =
                    keys_now.target
                {
                    if let Some((ti, _)) = s.find_clip(id) {
                        s.selected_track = ti;
                    }
                }
                s.clear_selected_clips();
            }
        } else if clips_changed && clips_on && keys_on {
            self.multi_drag.key_sel.clear();
        }
        self.multi_drag.key_sel_seen = self.multi_drag.key_sel.clone();
        self.multi_drag.clip_sel_seen = self
            .session
            .as_ref()
            .map(|s| (s.selected_clip, s.selected_clips.clone()))
            .unwrap_or_default();
        self.multi_drag.gap_sel_seen = self
            .session
            .as_ref()
            .map(|s| s.selected_gaps.clone())
            .unwrap_or_default();
    }

    /// The Multitrack Tools back in the corner where they were: what the track headers show,
    /// then the global toggles (metronome, stretching, snapping with its target dropdown,
    /// auto-scroll), one row at the size the old tools strip used. The time-format selector
    /// lives in the Selection/View panel. The tools are pinned to the corner as their own top
    /// layer (Order::Middle, not movable), so however tall the tracks grow and wherever they
    /// scroll, nothing on the tracks can cover the buttons or take their clicks;
    /// View > Multitrack Tools hides the tool row again.
    fn multitrack_tools_corner(&mut self, ui: &mut egui::Ui, panel: Rect) {
        let mut open_prefs_multi = false;
        egui::Area::new(egui::Id::new("multitrack_tools"))
            .order(egui::Order::Middle)
            .movable(false)
            .fixed_pos(panel.min)
            .show(ui.ctx(), |ui| {
                ui.set_width(panel.width());
                if self.show_tools_window {
                    let row = Rect::from_min_max(
                        pos2(panel.left() + 2.0, panel.top() + 5.0),
                        pos2(panel.right() - 2.0, panel.bottom() - 5.0),
                    );
                    let mut r2 = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(row)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    r2.spacing_mut().item_spacing.x = 2.0;
                    let size = vec2(24.0, 22.0);
                    for (m, g, tip) in [
                        (0u8, HeadGlyph::Io, "Inputs/Outputs"),
                        (1, HeadGlyph::Fx, "Effects"),
                        (2, HeadGlyph::Sends, "Sends"),
                        (3, HeadGlyph::Eq, "EQ"),
                    ] {
                        let r = head_tool_sized(&mut r2, g, self.header_view == m, tip, size, 15.0);
                        self.multi_drag.tool_spots.push((m, r.rect));
                        if r.clicked() {
                            self.header_view = m;
                        }
                    }
                    let metro_on = self
                        .session
                        .as_ref()
                        .map(|s| s.metronome.on)
                        .unwrap_or(false);
                    let metro = head_tool_sized(
                        &mut r2,
                        HeadGlyph::Metronome,
                        metro_on,
                        "Toggle Metronome",
                        size,
                        15.0,
                    );
                    self.multi_drag.tool_spots.push((4, metro.rect));
                    if metro.clicked() {
                        if let Some(s) = self.session.as_mut() {
                            s.metronome.on = !metro_on;
                            s.touch();
                        }
                    }
                    let stretch = head_tool_sized(
                        &mut r2,
                        HeadGlyph::Stretch,
                        self.stretch_mode,
                        "Toggle Global Clip Stretching",
                        size,
                        15.0,
                    );
                    self.multi_drag.tool_spots.push((5, stretch.rect));
                    if stretch.clicked() {
                        self.stretch_mode = !self.stretch_mode;
                    }
                    // Snapping is a split button in the same design style as the other
                    // drop-downs: the magnet side toggles snapping, the small arrow side opens
                    // its target dropdown.
                    let (snap, targets) = snap_split_button(
                        &mut r2,
                        self.snapping,
                        self.snap_menu_open,
                        "Toggle Snapping",
                        "Snapping Targets",
                    );
                    self.multi_drag.tool_spots.push((6, snap.rect));
                    if snap.clicked() {
                        self.snapping = !self.snapping;
                    }
                    self.multi_drag.tool_spots.push((7, targets.rect));
                    // the snap targets are a real drop-down menu, the same kind (look, place,
                    // open/close behaviour) as every other drop-down in the app: it opens under
                    // the arrow, stays open while boxes are ticked, and closes on a click
                    // anywhere else or on the arrow again
                    let popup_id = egui::Id::new("snap_targets_popup");
                    egui::Popup::menu(&targets)
                        .id(popup_id)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .align(egui::RectAlign::BOTTOM_START)
                        .show(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(
                                egui::RichText::new("Snap to")
                                    .small()
                                    .color(theme::text_dim()),
                            );
                            ui.checkbox(
                                &mut self.prefs.snap_time_display,
                                "Time display (ruler marks)",
                            );
                            ui.checkbox(&mut self.prefs.snap_clip_starts, "Clip starts");
                            ui.checkbox(&mut self.prefs.snap_clip_ends, "Clip ends");
                            ui.checkbox(&mut self.prefs.marker_snap, "Markers");
                            ui.checkbox(&mut self.prefs.snap_playhead, "Playhead");
                            ui.checkbox(&mut self.prefs.snap_session_start, "Session start");
                            ui.separator();
                            if ui.button("Snapping Preferences...").clicked() {
                                open_prefs_multi = true;
                                ui.close();
                            }
                        });
                    self.snap_menu_open = egui::Popup::is_id_open(ui.ctx(), popup_id);
                    let auto = head_tool_sized(
                        &mut r2,
                        HeadGlyph::AutoScroll,
                        self.auto_scroll,
                        "Toggle Playback Auto-Scroll",
                        size,
                        15.0,
                    );
                    self.multi_drag.tool_spots.push((8, auto.rect));
                    if auto.clicked() {
                        self.auto_scroll = !self.auto_scroll;
                    }
                }
            });
        if open_prefs_multi {
            self.open_prefs("Multitrack");
        }
    }

    pub fn multitrack_view(&mut self, ui: &mut egui::Ui) {
        self.keys_take_selection();
        self.multi_drag.tool_spots.clear();
        let full = ui.max_rect();
        let p = ui.painter_at(full);
        p.rect_filled(full, 0.0, theme::panel());
        if self.session.is_none() {
            self.ensure_session();
        }
        let header_w = 222.0;
        // the vertical navigator runs down the right edge, beside the tracks
        let vnav_w = 16.0;
        let nav = Rect::from_min_size(full.min, vec2(full.width() - vnav_w, 18.0));
        let ruler = Rect::from_min_max(
            pos2(full.left() + header_w, nav.bottom()),
            pos2(full.right() - vnav_w, nav.bottom() + 22.0),
        );
        let master_h = 86.0;
        // the metronome, when shown, is a short row above all the tracks
        let metro_h = if self
            .session
            .as_ref()
            .map(|s| s.metronome.on)
            .unwrap_or(false)
        {
            46.0
        } else {
            0.0
        };
        let metro = Rect::from_min_max(
            pos2(full.left(), ruler.bottom()),
            pos2(full.right() - vnav_w, ruler.bottom() + metro_h),
        );
        let tracks_rect = Rect::from_min_max(
            pos2(full.left(), ruler.bottom() + metro_h),
            pos2(full.right() - vnav_w, full.bottom() - master_h),
        );
        let vnav = Rect::from_min_max(
            pos2(full.right() - vnav_w, tracks_rect.top()),
            pos2(full.right(), full.bottom() - master_h),
        );
        let lanes = Rect::from_min_max(
            pos2(full.left() + header_w, tracks_rect.top()),
            tracks_rect.max,
        );
        let master = Rect::from_min_max(pos2(full.left(), full.bottom() - master_h), full.max);

        let (mut view, mut cursor, mut selection) = {
            let s = self.session.as_ref().unwrap();
            (s.view, s.cursor, s.selection)
        };
        let sess_len = self.session.as_ref().unwrap().length();
        let total = (sess_len + 10.0).max(30.0).max(view.1);
        if view.1 <= view.0 {
            view = (0.0, 30.0);
        }
        let span = view.1 - view.0;

        // ---------- navigator
        p.rect_filled(nav, 0.0, crate::theme::gray(24));
        {
            let s = self.session.as_ref().unwrap();
            let nl = nav.left() + header_w;
            let nw = nav.width() - header_w;
            let n_tr = s.tracks.len().max(1) as f32;
            for (ti, t) in s.tracks.iter().enumerate() {
                let (_, head, _) = clip_colors(t.color);
                for c in &t.clips {
                    let x0 = nl + (c.start / total) as f32 * nw;
                    let x1 = nl + (c.end() / total) as f32 * nw;
                    let y0 = nav.top() + 2.0 + ti as f32 * (nav.height() - 4.0) / n_tr;
                    p.rect_filled(
                        Rect::from_min_max(
                            pos2(x0, y0),
                            pos2(
                                x1.max(x0 + 1.0),
                                y0 + ((nav.height() - 4.0) / n_tr).max(1.0),
                            ),
                        ),
                        0.0,
                        head,
                    );
                }
            }
            let vr = Rect::from_min_max(
                pos2(nl + (view.0 / total) as f32 * nw, nav.top() + 1.0),
                pos2(
                    (nl + (view.1 / total) as f32 * nw)
                        .max(nl + (view.0 / total) as f32 * nw + 6.0),
                    nav.bottom() - 1.0,
                ),
            );
            p.rect_filled(vr, 0.0, crate::theme::gray_a(255, 14));
            p.rect_stroke(
                vr,
                0.0,
                Stroke::new(1.0, crate::theme::gray(170)),
                egui::StrokeKind::Inside,
            );
            // grips at both ends: drag to stretch (zoom), drag the middle to move
            for x in [vr.left() + 2.5, vr.right() - 2.5] {
                p.rect_filled(
                    Rect::from_center_size(pos2(x, vr.center().y), vec2(3.0, vr.height() - 6.0)),
                    0.0,
                    crate::theme::gray(190),
                );
            }
            let nresp = ui.interact(
                Rect::from_min_max(pos2(nl, nav.top()), nav.max),
                ui.scope_id().with("mnav"),
                Sense::click_and_drag(),
            );
            let near_edge = |x: f32| -> Option<bool> {
                if (x - vr.left()).abs() < 6.0 {
                    Some(false)
                } else if (x - vr.right()).abs() < 6.0 {
                    Some(true)
                } else {
                    None
                }
            };
            if let Some(hp) = nresp.hover_pos() {
                if near_edge(hp.x).is_some()
                    || matches!(self.multi_drag.drag, MDrag::NavL | MDrag::NavR)
                {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                // the wheel over the navigator stretches the view around its centre
                let (sy, zd) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
                let f = if zd != 1.0 {
                    1.0 / zd as f64
                } else if sy.abs() > 0.1 {
                    (0.996f64).powf(sy as f64)
                } else {
                    1.0
                };
                if f != 1.0 {
                    let rate = self
                        .session
                        .as_ref()
                        .map(|s| s.rate)
                        .unwrap_or(48_000)
                        .max(1) as f64;
                    let c = (view.0 + view.1) * 0.5;
                    let w = (span * f).clamp(12.0 / rate, total.max(12.0 / rate));
                    let a = (c - w / 2.0).max(0.0);
                    view = (a, a + w);
                }
            }
            let min_w = 12.0
                / self
                    .session
                    .as_ref()
                    .map(|s| s.rate)
                    .unwrap_or(48_000)
                    .max(1) as f64;
            if let Some(pp) = nresp.interact_pointer_pos() {
                let t = (((pp.x - nl) / nw) as f64 * total).max(0.0);
                if nresp.drag_started() {
                    let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pp);
                    let ot = ((origin.x - nl) / nw) as f64 * total;
                    self.multi_drag.drag = match near_edge(origin.x) {
                        Some(false) => MDrag::NavL,
                        Some(true) => MDrag::NavR,
                        None => MDrag::Nav {
                            grab: if ot >= view.0 && ot <= view.1 {
                                ot - view.0
                            } else {
                                span / 2.0
                            },
                        },
                    };
                }
                if nresp.dragged() {
                    match self.multi_drag.drag {
                        MDrag::Nav { grab } => {
                            let a = (t - grab).clamp(0.0, (total - span).max(0.0));
                            view = (a, a + span);
                        }
                        MDrag::NavL => view.0 = t.min(view.1 - min_w).max(0.0),
                        MDrag::NavR => view.1 = t.max(view.0 + min_w),
                        _ => {}
                    }
                }
                if nresp.clicked() {
                    let a = (t - span / 2.0).clamp(0.0, (total - span).max(0.0));
                    view = (a, a + span);
                }
            }
            if nresp.drag_stopped() {
                self.multi_drag.drag = MDrag::None;
            }
            if nresp.double_clicked() {
                // show the whole session
                view = (0.0, (sess_len * 1.05).max(10.0));
            }
        }
        let span = view.1 - view.0;
        let v0 = view.0;
        let x_of = move |t: f64| lanes.left() + ((t - v0) / span) as f32 * lanes.width();
        let t_of = move |x: f32| v0 + ((x - lanes.left()) / lanes.width()) as f64 * span;

        // ---------- ruler
        draw_ruler(&p, ruler, view.0, view.1);
        // the pre-render gauge: a thin bar along the ruler's top edge (like a video editor's
        // render bar) whenever a track has Pre-render on. It follows the render window: faded
        // over the stretch the renders aim to hold (the whole session when it fits the cache
        // cap, otherwise around the playhead), filled where every pre-rendered track is
        // ready, with the moving edge marked; all in the theme's colours
        if let Some(g) = self.prerender_gauge() {
            let bar = Rect::from_min_max(ruler.min, pos2(ruler.right(), ruler.top() + 4.0));
            self.multi_drag.prerender_bar = Some(bar);
            let clip_x = |x: f32| x.clamp(bar.left(), bar.right());
            let span_rect = |a: f64, b: f64| {
                Rect::from_min_max(
                    pos2(clip_x(x_of(a)), bar.top()),
                    pos2(clip_x(x_of(b)), bar.bottom()),
                )
            };
            let w = span_rect(g.window.0, g.window.1);
            if w.width() > 0.0 {
                p.rect_filled(w, 0.0, theme::render_todo());
            }
            let d = span_rect(g.rendered.0, g.rendered.1);
            if d.width() > 0.0 {
                p.rect_filled(d, 0.0, theme::render_done());
            }
            let xr = x_of(g.rendered.1);
            if g.busy && xr >= bar.left() && xr <= bar.right() {
                p.rect_filled(
                    Rect::from_min_max(pos2(xr - 1.0, bar.top()), pos2(xr + 1.0, bar.bottom() + 1.0)),
                    0.0,
                    theme::render_edge(),
                );
            }
            let hover = ui.interact(
                Rect::from_min_max(bar.min, pos2(bar.right(), bar.bottom() + 2.0)),
                ui.scope_id().with("prerender_gauge"),
                Sense::hover(),
            );
            let tracks = format!(
                "{} track{}",
                g.tracks,
                if g.tracks == 1 { "" } else { "s" }
            );
            let fmt = self.time_format;
            let rate = self.rate();
            let when = format!(
                "{} - {}",
                fmt.format(g.rendered.0, rate),
                fmt.format(g.rendered.1, rate)
            );
            hover.on_hover_text(format!(
                "{}\nReady: {when}\nCache: {:.0} of {:.0} MB",
                if g.busy {
                    format!("Pre-rendering {tracks}: {:.0}%", g.progress * 100.0)
                } else {
                    format!("{tracks} pre-rendered")
                },
                g.megabytes,
                g.cap_megabytes
            ));
        } else {
            self.multi_drag.prerender_bar = None;
        }
        p.rect_filled(
            Rect::from_min_max(
                pos2(full.left(), ruler.top()),
                pos2(ruler.left(), ruler.bottom()),
            ),
            0.0,
            theme::ruler(),
        );
        // the corner left of the ruler: the Multitrack Tools pinned there as their own top
        // layer, so tracks growing and scrolling underneath can never cover the buttons or
        // take their clicks. The ruler's time-format selector lives in the Selection/View
        // panel.
        {
            let panel = Rect::from_min_max(
                pos2(full.left(), nav.top()),
                pos2(full.left() + header_w, ruler.bottom()),
            );
            p.rect_filled(panel, 0.0, theme::track_head());
            p.rect_filled(
                Rect::from_min_max(pos2(panel.right() - 2.0, panel.top()), panel.max),
                0.0,
                theme::track_divider(),
            );
            p.line_segment(
                [panel.left_bottom(), panel.right_bottom()],
                Stroke::new(1.0, crate::theme::gray(18)),
            );
            self.multi_drag.tool_panel = Some(panel);
            self.multitrack_tools_corner(ui, panel);
        }
        // metronome row
        if metro_h > 0.0 {
            let mhead = Rect::from_min_max(metro.min, pos2(lanes.left(), metro.bottom()));
            p.rect_filled(mhead, 0.0, Color32::from_rgb(34, 34, 40));
            p.rect_filled(
                Rect::from_min_max(pos2(lanes.left(), metro.top()), metro.max),
                0.0,
                crate::theme::gray(27),
            );
            p.rect_filled(
                Rect::from_min_size(mhead.min, vec2(4.0, mhead.height())),
                0.0,
                Color32::from_rgb(60, 70, 130),
            );
            p.line_segment(
                [
                    pos2(metro.left(), metro.bottom()),
                    pos2(metro.right(), metro.bottom()),
                ],
                Stroke::new(1.0, crate::theme::gray(12)),
            );
            // beat ticks across the lane
            if let Some(m) = self.session.as_ref().map(|s| s.metronome.clone()) {
                let beat = 60.0 / m.bpm.max(1.0) as f64;
                let first = (view.0 / beat).floor() as i64;
                let last = (view.1 / beat).ceil() as i64;
                if last - first < 2000 {
                    for b in first..=last {
                        let x = x_of(b as f64 * beat);
                        if x >= lanes.left() && x <= lanes.right() {
                            let bar = b.rem_euclid(m.beats.max(1) as i64) == 0;
                            p.line_segment(
                                [
                                    pos2(x, metro.bottom() - if bar { 16.0 } else { 8.0 }),
                                    pos2(x, metro.bottom() - 2.0),
                                ],
                                Stroke::new(1.0, crate::theme::gray(if bar { 150 } else { 90 })),
                            );
                        }
                    }
                }
            }
            let mut mui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(
                pos2(mhead.left() + 10.0, mhead.top() + 2.0),
                pos2(mhead.right() - 8.0, mhead.bottom()),
            )));
            mui.set_clip_rect(mhead);
            mui.spacing_mut().item_spacing.y = 2.0;
            if let Some(s) = self.session.as_mut() {
                let m = &mut s.metronome;
                let mut touched = false;
                mui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(egui::RichText::new("\u{25b3}  Metronome").color(theme::text()));
                    ui.add_space((ui.available_width() - 70.0).max(0.0));
                    if letter_toggle(ui, &mut m.mute, "M", MUTE_ON, "Mute the metronome") {
                        touched = true;
                    }
                    let tip = format!("Tempo {:.0} BPM, {}/4 (drag)", m.bpm, m.beats);
                    let mut b = m.bpm;
                    if theme::hot_value(
                        ui,
                        &mut b,
                        20.0,
                        300.0,
                        0.5,
                        120.0,
                        &format!("{:.0}", m.bpm),
                        &tip,
                    )
                    .changed()
                    {
                        m.bpm = b.round();
                        touched = true;
                    }
                });
                mui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    small_glyph(ui, Glyph::Volume);
                    let mut v = m.volume_db;
                    let k = theme::knob(ui, &mut v, -60.0, 15.0, 18.0, "Metronome volume");
                    let hv = theme::hot_value(
                        ui,
                        &mut v,
                        -60.0,
                        15.0,
                        0.1,
                        0.0,
                        &format!("{:+.0}", m.volume_db),
                        "Metronome volume",
                    );
                    if k.changed() || hv.changed() {
                        m.volume_db = v.round();
                        touched = true;
                    }
                    ui.add_space(8.0);
                    small_glyph(ui, Glyph::Pan);
                    let mut pn = m.pan * 100.0;
                    let k2 = theme::knob(ui, &mut pn, -100.0, 100.0, 18.0, "Metronome pan");
                    let hp = theme::hot_value(
                        ui,
                        &mut pn,
                        -100.0,
                        100.0,
                        0.5,
                        0.0,
                        &format!("{:.0}", m.pan * 100.0),
                        "Metronome pan",
                    );
                    if k2.changed() || hp.changed() {
                        m.pan = pn.round() / 100.0;
                        touched = true;
                    }
                });
                if touched {
                    s.touch();
                }
            }
        }
        {
            let s = self.session.as_ref().unwrap();
            for m in &s.markers {
                let x = x_of(m.time);
                if x >= ruler.left() && x <= ruler.right() {
                    p.add(egui::Shape::convex_polygon(
                        vec![
                            pos2(x, ruler.top() + 11.0),
                            pos2(x + 9.0, ruler.top() + 16.0),
                            pos2(x, ruler.top() + 21.0),
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
                }
            }
        }
        let rresp = ui.interact(ruler, ui.scope_id().with("mruler"), Sense::click_and_drag());
        let srate = self
            .session
            .as_ref()
            .map(|s| s.rate)
            .unwrap_or(48_000)
            .max(1) as f64;
        if let Some(pp) = rresp.interact_pointer_pos() {
            if rresp.clicked() || rresp.dragged() {
                // the playhead lands on sample points (exact at close zoom)
                cursor = ((t_of(pp.x).max(0.0) * srate).round() / srate).max(0.0);
            }
            // dragging the playhead past either edge keeps scrolling the view
            if rresp.dragged() {
                let edge = 16.0;
                if pp.x > ruler.right() - edge {
                    let d = span * 0.02 * (1.0 + ((pp.x - ruler.right() + edge) / edge) as f64);
                    view = (view.0 + d, view.1 + d);
                    cursor = cursor.max(view.1 - span * 0.02);
                    ui.ctx().request_repaint();
                } else if pp.x < ruler.left() + edge && view.0 > 0.0 {
                    let d = (span * 0.02 * (1.0 + ((ruler.left() + edge - pp.x) / edge) as f64))
                        .min(view.0);
                    view = (view.0 - d, view.1 - d);
                    cursor = cursor.min(view.0 + span * 0.02).max(0.0);
                    ui.ctx().request_repaint();
                }
            }
        }
        if rresp.drag_started() || rresp.dragged() {
            self.scrub_to(Some(cursor));
        } else if rresp.drag_stopped() {
            self.scrub_to(None);
        }

        // ---------- track headers & lanes
        let n_tracks = self.session.as_ref().unwrap().tracks.len();
        // empty spans picked with Shift+click (the whole row) or Ctrl+click (the one span);
        // kept by track id, so rows added or reordered around them do not move them
        let sel_gaps: Vec<crate::session::GapSel> = self
            .session
            .as_ref()
            .map(|s| s.selected_gaps.clone())
            .unwrap_or_default();
        let total_h: f32 = self
            .session
            .as_ref()
            .unwrap()
            .tracks
            .iter()
            .map(|t| t.shown_height())
            .sum();
        let max_scroll = (total_h - tracks_rect.height()).max(0.0);
        self.multi_drag.scroll_y = self.multi_drag.scroll_y.clamp(0.0, max_scroll);
        let mut y = tracks_rect.top() - self.multi_drag.scroll_y;
        let mut lane_rects: Vec<Rect> = vec![];
        // the clips' part of each row, and the automation lane under it when open
        let mut clip_rects: Vec<Rect> = vec![];
        let mut auto_rects: Vec<Option<Rect>> = vec![];
        let clip_painter = p.with_clip_rect(tracks_rect);
        let mut want_rack: Option<usize> = None;
        // (track, slot) of an effect to open from the Effects header view
        let mut want_slot: Option<(usize, usize)> = None;
        // Track EQ window to open (u64::MAX: the audio hardware settings)
        let mut want_eq: Option<u64> = None;
        // (track id, colour) to open the colour picker for; (track, drop y) of a dragged track
        let mut want_color: Option<(u64, [u8; 3])> = None;
        let mut track_drop: Option<(usize, f32)> = None;
        let mut hist: Option<&'static str> = None;
        let (track_meters, in_peaks) = {
            let e = self.engine.lock();
            (e.track_meters.clone(), e.in_peaks.clone())
        };
        for ti in 0..n_tracks {
            let (th, minimized, lane_h) = {
                let t = &self.session.as_ref().unwrap().tracks[ti];
                (t.shown_height(), t.minimized, t.lane_height())
            };
            let row = Rect::from_min_size(pos2(full.left(), y), vec2(full.width(), th));
            // the track's own controls sit beside its clips; the open lane gets its own header below
            let head = Rect::from_min_max(row.min, pos2(lanes.left(), row.bottom() - lane_h));
            let lane = Rect::from_min_max(pos2(lanes.left(), row.top()), row.max);
            lane_rects.push(lane);
            clip_rects.push(Rect::from_min_max(
                lane.min,
                pos2(lane.right(), row.bottom() - lane_h),
            ));
            auto_rects.push(if lane_h > 0.0 {
                Some(Rect::from_min_max(
                    pos2(lanes.left(), row.bottom() - lane_h),
                    row.max,
                ))
            } else {
                None
            });
            y += th;
            if row.bottom() < tracks_rect.top() || row.top() > tracks_rect.bottom() {
                continue;
            }
            let selected = {
                let s = self.session.as_ref().unwrap();
                s.selected_track == ti && !s.master_selected
            };
            clip_painter.rect_filled(
                Rect::from_min_max(row.min, pos2(lanes.left(), row.bottom())),
                0.0,
                if selected {
                    theme::track_head_sel()
                } else {
                    theme::track_head()
                },
            );
            clip_painter.rect_filled(
                lane,
                0.0,
                if ti % 2 == 0 {
                    theme::track_lane_a()
                } else {
                    theme::track_lane_b()
                },
            );
            // a hard divider keeps the track settings from blending into the timeline.
            // a solid 2px rect just inside the header stays crisp and cannot be hidden by a
            // playhead parked at time 0 (which used to paint over a thin stroked line).
            clip_painter.rect_filled(
                Rect::from_min_max(
                    pos2(lanes.left() - 2.0, row.top()),
                    pos2(lanes.left(), row.bottom()),
                ),
                0.0,
                theme::track_divider(),
            );
            clip_painter.line_segment(
                [
                    pos2(full.left(), row.bottom()),
                    pos2(full.right(), row.bottom()),
                ],
                Stroke::new(1.0, crate::theme::gray(12)),
            );
            // the row's selected empty spans, shaded like the time selection with a soft
            // edge (Shift+click took every span of the row, Ctrl+click the one clicked)
            {
                let area = clip_rects[ti];
                let trk = self.session.as_ref().unwrap().tracks[ti].id;
                for g in sel_gaps.iter().filter(|g| g.trk == trk) {
                    let ga = x_of(g.a).max(lanes.left());
                    let gb = x_of(g.b).min(lanes.right());
                    if gb <= ga {
                        continue;
                    }
                    let gr = Rect::from_min_max(
                        pos2(ga, area.top() + 2.0),
                        pos2(gb, area.bottom() - 2.0),
                    )
                    .intersect(tracks_rect);
                    if !gr.is_positive() {
                        continue;
                    }
                    clip_painter.rect_filled(gr, 0.0, crate::theme::gray_a(255, 26));
                    clip_painter.rect_stroke(
                        gr,
                        0.0,
                        Stroke::new(1.0, crate::theme::gray_a(255, 80)),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            let color = self.session.as_ref().unwrap().tracks[ti].color;
            let strip = Rect::from_min_size(head.min, vec2(COLOR_STRIP_W, row.height()));
            // The track keeps its own colour when selected: selection is already shown by the
            // lighter clip and track header, so recolouring the strip to orange was misleading.
            let strip_col =
                crate::theme::saturated(Color32::from_rgb(color[0], color[1], color[2]));
            clip_painter.rect_filled(strip, 0.0, strip_col);
            // per-track level meter: left and right channel side by side
            // an armed or monitored track meters its input (before and while recording)
            let lvl = self
                .track_input_level(ti, &in_peaks)
                .unwrap_or_else(|| track_meters.get(ti).copied().unwrap_or([0.0; 2]));
            let mr = Rect::from_min_max(
                pos2(head.right() - 13.0, head.top() + 5.0),
                pos2(head.right() - 3.0, head.bottom() - 5.0),
            );
            if !minimized {
                theme::vmeter(&clip_painter.with_clip_rect(tracks_rect), mr, &lvl);
            }
            let hr = head.shrink2(vec2(8.0, 4.0)).intersect(tracks_rect);
            if hr.height() < 10.0 {
                continue;
            }
            // the header's own click area goes first, so the buttons and knobs drawn on top of it
            // get their clicks (registered later = on top)
            let hresp = ui.interact(head, ui.scope_id().with(("thead", ti)), Sense::click());
            // the colour strip: an open hand says it can be dragged (to reorder the tracks);
            // double-click opens the colour picker
            let sresp = ui.interact(
                strip.intersect(tracks_rect),
                ui.scope_id().with(("tstrip", ti)),
                Sense::click_and_drag(),
            );
            if sresp.hovered() || sresp.dragged() {
                ui.ctx().set_cursor_icon(if sresp.dragged() {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::Grab
                });
            }
            if sresp.double_clicked() {
                let t = &self.session.as_ref().unwrap().tracks[ti];
                want_color = Some((t.id, t.color));
            }
            if sresp.drag_started() {
                self.multi_drag.drag = MDrag::TrackMove { from: ti };
            }
            let sresp =
                sresp.on_hover_text("Drag to move the track, double-click to change its colour");
            if sresp.drag_stopped() {
                if let (MDrag::TrackMove { from }, Some(pp)) =
                    (self.multi_drag.drag, sresp.interact_pointer_pos())
                {
                    track_drop = Some((from, pp.y));
                }
                self.multi_drag.drag = MDrag::None;
            }
            let mut hui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(
                pos2(head.left() + COLOR_STRIP_W + 5.0, head.top() + 4.0),
                pos2(head.right() - 16.0, head.bottom() - 2.0),
            )));
            hui.set_clip_rect(tracks_rect.intersect(head));
            let in_names = self.input_choices();
            let out_dev = self.engine.out_device_name.clone();
            let view_mode = self.header_view;
            let s_rate = self
                .session
                .as_ref()
                .map(|s| s.rate as f32)
                .unwrap_or(48000.0);
            let rack_open = &mut want_slot;
            let eq_open = &mut want_eq;
            let s = self.session.as_mut().unwrap();
            let tr = &mut s.tracks[ti];
            let mut touched = false;
            hui.spacing_mut().item_spacing.y = 3.0;
            // row 1: minimise, name, M S R I
            hui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let mb = minimize_button(ui, tr.minimized).on_hover_text(
                    "Minimise or restore the track (press and drag down to do the same to the tracks below)",
                );
                if theme::drag_toggle(ui, "track_minimize", &mb, &mut tr.minimized) {
                    touched = true;
                }
                let w = (ui.available_width() - 4.0 * 23.0).max(30.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut tr.name)
                        .desired_width(w)
                        .font(FontId::proportional(12.0)),
                );
                if r.changed() {
                    touched = true;
                }
                // Mute is green, Solo is yellow
                if letter_toggle(ui, &mut tr.mute, "M", MUTE_ON, "Mute") {
                    touched = true;
                    hist = Some("Mute Track");
                }
                if letter_toggle(ui, &mut tr.solo, "S", SOLO_ON, "Solo") {
                    touched = true;
                    hist = Some("Solo Track");
                }
                // arming needs an input; without one R is greyed out
                let can = tr.can_record();
                if !can {
                    tr.arm = false;
                }
                if ui
                    .add_enabled_ui(can, |ui| {
                        letter_toggle(
                            ui,
                            &mut tr.arm,
                            "R",
                            Color32::from_rgb(230, 80, 80),
                            "Arm for Record",
                        )
                    })
                    .inner
                {
                    touched = true;
                }
                if ui
                    .add_enabled_ui(can, |ui| {
                        letter_toggle(
                            ui,
                            &mut tr.io.monitor,
                            "I",
                            Color32::from_rgb(90, 160, 230),
                            "Monitor Input",
                        )
                    })
                    .inner
                {
                    touched = true;
                }
            });
            if !minimized {
                // row 2: volume and pan
                hui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    small_glyph(ui, Glyph::Volume);
                    let mut v = tr.volume_db;
                    let k = theme::knob(
                        ui,
                        &mut v,
                        -60.0,
                        15.0,
                        20.0,
                        "Volume (drag, double-click resets)",
                    );
                    let hv = theme::hot_value(
                        ui,
                        &mut v,
                        -60.0,
                        15.0,
                        0.1,
                        0.0,
                        &format!("{:+.1}", tr.volume_db),
                        "Volume: drag left/right, double-click for 0 dB",
                    );
                    if k.changed() || hv.changed() {
                        tr.volume_db = (v * 10.0).round() / 10.0;
                        touched = true;
                    }
                    if k.drag_stopped()
                        || hv.drag_stopped()
                        || k.double_clicked()
                        || hv.double_clicked()
                    {
                        hist = Some("Track Volume");
                    }
                    ui.add_space(8.0);
                    small_glyph(ui, Glyph::Pan);
                    let mut pn = tr.pan * 100.0;
                    let k2 = theme::knob(
                        ui,
                        &mut pn,
                        -100.0,
                        100.0,
                        20.0,
                        "Pan (drag, double-click centers)",
                    );
                    let pl = if tr.pan.abs() < 0.005 {
                        "0".to_string()
                    } else if tr.pan < 0.0 {
                        format!("L{:.0}", -tr.pan * 100.0)
                    } else {
                        format!("R{:.0}", tr.pan * 100.0)
                    };
                    let hp = theme::hot_value(
                        ui,
                        &mut pn,
                        -100.0,
                        100.0,
                        0.5,
                        0.0,
                        &pl,
                        "Pan: drag left/right, double-click centres",
                    );
                    if k2.changed() || hp.changed() {
                        tr.pan = pn.round() / 100.0;
                        touched = true;
                    }
                    if k2.drag_stopped()
                        || hp.drag_stopped()
                        || k2.double_clicked()
                        || hp.double_clicked()
                    {
                        hist = Some("Track Pan");
                    }
                    ui.add_space(4.0);
                    if ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(if tr.stereo {
                                    "((\u{2022}))"
                                } else {
                                    "(\u{2022})"
                                })
                                .small()
                                .color(theme::text_dim()),
                            )
                            .sense(Sense::click()),
                        )
                        .on_hover_text(if tr.stereo {
                            "Stereo track (click for mono)"
                        } else {
                            "Mono track (click for stereo)"
                        })
                        .clicked()
                    {
                        tr.stereo = !tr.stereo;
                        touched = true;
                        hist = Some("Track Channels");
                    }
                });
                // the middle: inputs/outputs, effects, sends or EQ (header toolbar choice)
                let show_auto = head.height() >= 118.0 || lane_h > 0.0;
                let body_h = (head.bottom() - 30.0 - hui.cursor().top()).max(0.0);
                if body_h >= 18.0 {
                    match view_mode {
                        1 => {
                            // effects: the rack's slots, each with its power switch
                            let rows = ((body_h / 20.0) as usize).max(1);
                            let used: Vec<(usize, String, bool)> = tr
                                .rack
                                .iter()
                                .enumerate()
                                .map(|(i, e)| (i, e.effect.name().to_string(), e.enabled))
                                .collect();
                            if used.is_empty() {
                                ui_dim(&mut hui, "No track effects (fx: open the rack)");
                                if hui.small_button("fx  Effects Rack").clicked() {
                                    want_rack = Some(ti);
                                }
                            }
                            for (i, name, on) in used.into_iter().take(rows) {
                                hui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    if letter_toggle(
                                        ui,
                                        &mut tr.rack[i].enabled,
                                        "\u{23fb}",
                                        theme::power_on(),
                                        if on {
                                            "Turn this effect off"
                                        } else {
                                            "Turn this effect on"
                                        },
                                    ) {
                                        touched = true;
                                    }
                                    let w = (ui.available_width() - 4.0).max(30.0);
                                    if ui
                                        .add(
                                            egui::Button::new(egui::RichText::new(&name).small())
                                                .min_size(vec2(w, 18.0)),
                                        )
                                        .on_hover_text("Open the effect's settings")
                                        .clicked()
                                    {
                                        *rack_open = Some((ti, i));
                                    }
                                });
                            }
                        }
                        2 => {
                            // sends: one send slot (busses route to it)
                            hui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 3.0;
                                if letter_toggle(
                                    ui,
                                    &mut tr.send.on,
                                    "\u{23fb}",
                                    theme::power_on(),
                                    "Send on/off",
                                ) {
                                    touched = true;
                                }
                                let w = (ui.available_width() - 4.0).max(30.0);
                                egui::containers::menu::MenuButton::from_button(
                                    egui::Button::new(
                                        egui::RichText::new(fit_text(
                                            ui,
                                            &format!("S1: {}", tr.send.target),
                                            w - 22.0,
                                        ))
                                        .small(),
                                    )
                                    .min_size(vec2(w, 18.0))
                                    .right_text("\u{203a}"),
                                )
                                .ui(ui, |ui| {
                                    if ui
                                        .selectable_label(tr.send.target == "None", "None")
                                        .clicked()
                                    {
                                        tr.send.target = "None".into();
                                        ui.close();
                                    }
                                    ui.add_enabled(
                                        false,
                                        egui::Button::new("Bus (add a bus track first)"),
                                    );
                                });
                            });
                            if body_h >= 40.0 {
                                hui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    let tip = if tr.send.pre_fader {
                                        "Pre-fader send (click for post-fader)"
                                    } else {
                                        "Post-fader send (click for pre-fader)"
                                    };
                                    if letter_toggle(
                                        ui,
                                        &mut tr.send.pre_fader,
                                        "\u{21b1}",
                                        Color32::from_rgb(170, 50, 50),
                                        tip,
                                    ) {
                                        touched = true;
                                    }
                                    small_glyph(ui, Glyph::Volume);
                                    let mut v = tr.send.level_db;
                                    let k =
                                        theme::knob(ui, &mut v, -96.0, 15.0, 20.0, "Send level");
                                    let txt = if v <= -95.9 {
                                        "-\u{221e}".to_string()
                                    } else {
                                        format!("{v:+.1}")
                                    };
                                    let hv = theme::hot_value(
                                        ui,
                                        &mut v,
                                        -96.0,
                                        15.0,
                                        0.2,
                                        -96.0,
                                        &txt,
                                        "Send level: drag left/right",
                                    );
                                    if k.changed() || hv.changed() {
                                        tr.send.level_db = v;
                                        touched = true;
                                    }
                                });
                            }
                        }
                        3 if body_h < 46.0 => {
                            // short track: power and an "EQ..." button on one row
                            hui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                if letter_toggle(
                                    ui,
                                    &mut tr.eq_on,
                                    "\u{23fb}",
                                    theme::power_on(),
                                    "Track EQ on/off",
                                ) {
                                    touched = true;
                                    hist = Some("Track EQ");
                                }
                                let w = (ui.available_width() - 4.0).max(40.0);
                                if ui
                                    .add(
                                        egui::Button::new(egui::RichText::new("EQ...").small())
                                            .corner_radius(crate::theme::ROUND)
                                            .min_size(vec2(w, 18.0)),
                                    )
                                    .on_hover_text("Open the Track EQ")
                                    .clicked()
                                {
                                    *eq_open = Some(tr.id);
                                }
                            });
                        }
                        3 => {
                            // EQ: power and pencil on the left, the response on the right
                            let top = hui.cursor().top();
                            let er = Rect::from_min_max(
                                pos2(head.left() + 8.0, top),
                                pos2(head.right() - 18.0, (top + body_h - 2.0).max(top + 18.0)),
                            );
                            let bcol =
                                Rect::from_min_max(er.min, pos2(er.left() + 24.0, er.bottom()));
                            let gr = Rect::from_min_max(pos2(bcol.right() + 2.0, er.top()), er.max);
                            let pnt = hui.painter().clone();
                            pnt.rect_filled(er, 0.0, crate::theme::gray(28));
                            pnt.rect_stroke(
                                gr,
                                0.0,
                                Stroke::new(1.0, crate::theme::gray(55)),
                                egui::StrokeKind::Inside,
                            );
                            let mut b1 = hui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(bcol.shrink(2.0))
                                    .layout(egui::Layout::top_down(egui::Align::Center)),
                            );
                            if letter_toggle(
                                &mut b1,
                                &mut tr.eq_on,
                                "\u{23fb}",
                                theme::power_on(),
                                "Track EQ on/off",
                            ) {
                                touched = true;
                                hist = Some("Track EQ");
                            }
                            if letter_toggle(
                                &mut b1,
                                &mut false.clone(),
                                "\u{270e}",
                                Color32::GRAY,
                                "Edit the Track EQ",
                            ) {
                                *eq_open = Some(tr.id);
                            }
                            if tr.eq.is_empty() {
                                tr.eq = crate::equi::standard_bands();
                            }
                            mini_eq(
                                &pnt.with_clip_rect(gr),
                                gr.shrink(2.0),
                                &tr.eq,
                                tr.eq_master,
                                tr.eq_on,
                                s_rate,
                            );
                            hui.allocate_rect(er, Sense::hover());
                        }
                        _ => {
                            // inputs and outputs
                            let io_h = crate::prefs::io_button_height();
                            hui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 3.0;
                                ui.label(egui::RichText::new("\u{2192}").color(theme::text_dim()))
                                    .on_hover_text("Input");
                                let w = (ui.available_width() - 30.0).max(40.0);
                                egui::containers::menu::MenuButton::from_button(
                                    egui::Button::new(
                                        egui::RichText::new(fit_text(ui, &tr.io.input, w - 22.0))
                                            .small(),
                                    )
                                    .min_size(vec2(w, io_h))
                                    .right_text("\u{203a}"),
                                )
                                .ui(ui, |ui| {
                                    if ui.selectable_label(tr.io.input == "None", "None").clicked()
                                    {
                                        tr.io.input = "None".into();
                                        tr.arm = false;
                                        touched = true;
                                        ui.close();
                                    }
                                    ui.separator();
                                    ui.menu_button("Stereo", |ui| {
                                        for n in in_names.iter().filter(|n| !n.starts_with("Mono"))
                                        {
                                            if ui.selectable_label(tr.io.input == *n, n).clicked() {
                                                tr.io.input = n.clone();
                                                touched = true;
                                                ui.close();
                                            }
                                        }
                                    });
                                    ui.menu_button("Mono", |ui| {
                                        for n in in_names.iter().filter(|n| n.starts_with("Mono")) {
                                            if ui.selectable_label(tr.io.input == *n, n).clicked() {
                                                tr.io.input = n.clone();
                                                touched = true;
                                                ui.close();
                                            }
                                        }
                                    });
                                    ui.separator();
                                    if ui.button("Audio Hardware...").clicked() {
                                        *eq_open = Some(u64::MAX);
                                        ui.close();
                                    }
                                });
                                if letter_toggle(
                                    ui,
                                    &mut tr.io.invert,
                                    "\u{d8}",
                                    Color32::from_rgb(200, 140, 230),
                                    "Invert Polarity",
                                ) {
                                    touched = true;
                                    hist = Some("Invert Polarity");
                                }
                            });
                            if body_h >= 40.0 {
                                hui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    ui.label(
                                        egui::RichText::new("\u{2190}").color(theme::text_dim()),
                                    )
                                    .on_hover_text("Output");
                                    let w = (ui.available_width() - 4.0).max(40.0);
                                    egui::containers::menu::MenuButton::from_button(
                                        egui::Button::new(
                                            egui::RichText::new(fit_text(
                                                ui,
                                                &tr.io.output,
                                                w - 22.0,
                                            ))
                                            .small(),
                                        )
                                        .min_size(vec2(w, io_h))
                                        .right_text("\u{203a}"),
                                    )
                                    .ui(ui, |ui| {
                                        for n in ["None", "Master"] {
                                            if ui.selectable_label(tr.io.output == n, n).clicked() {
                                                tr.io.output = n.into();
                                                touched = true;
                                                ui.close();
                                            }
                                            ui.separator();
                                        }
                                        ui.menu_button("Bus", |ui| {
                                            ui.add_enabled(
                                                false,
                                                egui::Button::new("No busses in this session"),
                                            );
                                        });
                                        ui.separator();
                                        ui.menu_button("Stereo", |ui| {
                                            let n = format!("Stereo: {out_dev}");
                                            if ui.selectable_label(tr.io.output == n, &n).clicked()
                                            {
                                                tr.io.output = n.clone();
                                                touched = true;
                                                ui.close();
                                            }
                                        });
                                        ui.menu_button("5.1", |ui| {
                                            ui.add_enabled(
                                                false,
                                                egui::Button::new("No 5.1 output"),
                                            );
                                        });
                                        ui.separator();
                                        if ui.button("Audio Hardware...").clicked() {
                                            *eq_open = Some(u64::MAX);
                                            ui.close();
                                        }
                                    });
                                });
                            }
                        }
                    }
                }
                if show_auto {
                    // automation mode sits at the bottom of the header
                    let ar = Rect::from_min_max(
                        pos2(head.left() + 8.0, head.bottom() - 26.0),
                        pos2(head.right() - 18.0, head.bottom() - 4.0),
                    );
                    let mut aui = hui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(ar)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    if automation_disclosure(
                        &mut aui,
                        tr.show_vol_env,
                        "Show or hide this track's automation lane (it opens under the track)",
                    )
                    .clicked()
                    {
                        tr.show_vol_env = !tr.show_vol_env;
                        if tr.show_vol_env {
                            tr.minimized = false;
                            if !tr.auto.show_vol && !tr.auto.show_pan {
                                tr.auto.show_vol = true;
                            }
                        }
                        touched = true;
                        hist = Some(if tr.show_vol_env {
                            "Show Automation Lane"
                        } else {
                            "Hide Automation Lane"
                        });
                    }
                    if automation_combo(&mut aui, ("automode", ti), &mut tr.io.automation) {
                        touched = true;
                    }
                }
                // the open lane's own header: which envelopes it shows, which one is edited, and
                // its keyframe tools (protect, clear, previous / add / next keyframe)
                if lane_h > 0.0 {
                    let lh = Rect::from_min_max(
                        pos2(head.left() + COLOR_STRIP_W + 5.0, head.bottom() + 6.0),
                        pos2(lanes.left() - 8.0, row.bottom() - 4.0),
                    );
                    let lclip = tracks_rect.intersect(lh.expand(3.0));
                    let mut lui = hui.new_child(egui::UiBuilder::new().max_rect(lh));
                    lui.set_clip_rect(lclip);
                    let w = lh.width();
                    egui::containers::menu::MenuButton::from_button(
                        egui::Button::new("Show Envelopes")
                            .min_size(vec2(w, 20.0))
                            .right_text("\u{203a}"),
                    )
                    .ui(&mut lui, |ui| {
                        if crate::lanes::show_envelopes_menu(ui, tr) {
                            touched = true;
                        }
                    });
                    if lh.height() >= 56.0 {
                        let sy = if lh.height() > 110.0 {
                            lh.center().y + 10.0
                        } else {
                            lh.top() + 38.0
                        };
                        let sr = Rect::from_min_max(
                            pos2(lh.left(), sy - 11.0),
                            pos2(lh.right(), sy + 11.0),
                        );
                        let mut sui = lui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(sr)
                                .layout(egui::Layout::right_to_left(egui::Align::Center)),
                        );
                        sui.set_clip_rect(lclip);
                        if crate::lanes::select_combo(&mut sui, ("autoparam", ti), tr) {
                            touched = true;
                        }
                        sui.label(egui::RichText::new("Select:").color(theme::text_dim()));
                    }
                    if lh.height() >= 84.0 {
                        let br = Rect::from_min_max(pos2(lh.left(), lh.bottom() - 20.0), lh.max);
                        let mut bui = lui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(br)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        bui.set_clip_rect(lclip);
                        bui.spacing_mut().item_spacing.x = 8.0;
                        let (tch, h) = crate::lanes::lane_tools(&mut bui, tr, &mut cursor);
                        if tch {
                            touched = true;
                        }
                        if h.is_some() {
                            hist = h;
                        }
                    }
                }
            }
            if touched {
                s.touch();
            }
            if hresp.clicked() {
                s.selected_track = ti;
                s.master_selected = false;
            }
            hresp.context_menu(|ui| {
                if ui.button("Add Stereo Track Below").clicked() {
                    let id = s.id();
                    let mut t = s.tracks[ti].clone();
                    t.id = id;
                    t.clips.clear();
                    t.rack.clear();
                    t.name = format!("Track {}", s.tracks.len() + 1);
                    s.tracks.insert(ti + 1, t);
                    hist = Some("Add Track");
                    ui.close();
                }
                if ui.button("Delete Track").clicked() {
                    s.tracks.remove(ti);
                    s.selected_track = 0;
                    hist = Some("Delete Track");
                    ui.close();
                }
                ui.separator();
                if ui
                    .button(if s.tracks.get(ti).map(|t| t.minimized).unwrap_or(false) {
                        "Restore Track"
                    } else {
                        "Minimize Track"
                    })
                    .clicked()
                {
                    if let Some(t) = s.tracks.get_mut(ti) {
                        t.minimized = !t.minimized;
                    }
                    ui.close();
                }
                for (h, l) in [(64.0, "Small"), (96.0, "Medium"), (150.0, "Large")] {
                    if ui.button(format!("Height: {l}")).clicked() {
                        if let Some(t) = s.tracks.get_mut(ti) {
                            t.height = h;
                            t.minimized = false;
                        }
                        ui.close();
                    }
                }
            });
            if self.session.as_ref().unwrap().tracks.len() != n_tracks {
                break;
            }
        }
        if let Some((id, color)) = want_color {
            self.dialog = Some(crate::app::Dialog::TrackColor { track: id, color });
        }
        if let Some((from, y)) = track_drop {
            // the track goes where it was dropped (between the rows around the pointer)
            let to = lane_rects
                .iter()
                .position(|r| y < r.center().y)
                .unwrap_or(lane_rects.len());
            if let Some(s) = self.session.as_mut() {
                if from < s.tracks.len() {
                    let to = if to > from { to - 1 } else { to }.min(s.tracks.len() - 1);
                    if to != from {
                        let t = s.tracks.remove(from);
                        s.tracks.insert(to, t);
                        s.selected_track = to;
                        s.touch();
                        hist = Some("Move Track");
                    }
                }
            }
        }
        // while a track is dragged, a line shows where it will land
        if let MDrag::TrackMove { .. } = self.multi_drag.drag {
            if let Some(pp) = ui.ctx().pointer_latest_pos() {
                let to = lane_rects
                    .iter()
                    .position(|r| pp.y < r.center().y)
                    .unwrap_or(lane_rects.len());
                let y = lane_rects
                    .get(to)
                    .map(|r| r.top())
                    .or(lane_rects.last().map(|r| r.bottom()))
                    .unwrap_or(tracks_rect.top());
                clip_painter.line_segment(
                    [pos2(full.left(), y), pos2(lanes.right(), y)],
                    Stroke::new(2.0, theme::accent()),
                );
            }
        }
        if let Some((ti, slot)) = want_slot {
            if let Some(s) = &mut self.session {
                s.selected_track = ti;
                s.master_selected = false;
            }
            self.rack_clip_tab = false;
            self.rack_window = Some((crate::rack::RackTarget::Track(ti), slot));
        }
        match want_eq {
            Some(u64::MAX) => self.dialog = Some(crate::app::Dialog::Preferences),
            Some(id) => self.track_eq_window = Some(id),
            None => {}
        }
        if let Some(ti) = want_rack {
            if let Some(s) = &mut self.session {
                s.selected_track = ti;
            }
            self.show_panel(crate::workspace::PanelId::EffectsRack);
        }
        let n_tracks = self.session.as_ref().unwrap().tracks.len();
        lane_rects.truncate(n_tracks);
        clip_rects.truncate(n_tracks);
        auto_rects.truncate(n_tracks);
        // every envelope drawn this frame, for the keyframe layer
        let mut key_views: Vec<KeyView> = vec![];
        let (kv_xl, kv_v0, kv_pps) = (lanes.left(), view.0, lanes.width() as f64 / span.max(1e-12));
        let direct_tool = self.tool == Tool::Direct;

        // ---------- clips
        let show_env = self.show_env;
        let show_pan = self.show_pan_env;
        let selected_clip = self.session.as_ref().unwrap().selected_clip;
        // the whole picked set is highlighted, not just the clip picked last
        let sel_clips: Vec<u64> = self
            .session
            .as_ref()
            .map(|s| s.selected_clips.clone())
            .unwrap_or_default();
        // the fades actually heard (auto crossfades included), by clip id
        let eff: std::collections::HashMap<u64, (f64, f64, bool, bool)> = self
            .session
            .as_ref()
            .map(|s| {
                s.tracks
                    .iter()
                    .flat_map(|t| {
                        t.clips
                            .iter()
                            .map(|c| c.id)
                            .zip(crate::session::effective_fades(t))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut hits: Vec<(Rect, u64, usize)> = vec![];
        let stretch_handles = self.stretch_mode;
        let drag_now = self.multi_drag.drag;
        // crossfade curves (both the fade-out and the fade-in over an overlap), drawn on top
        let mut xfade_lines: Vec<(Rect, Vec<egui::Pos2>)> = vec![];
        {
            let s = self.session.as_ref().unwrap();
            for (ti, t) in s.tracks.iter().enumerate() {
                let Some(lane) = clip_rects.get(ti) else {
                    continue;
                };
                let (bg, headc, wavec) = clip_colors(t.color);
                for c in &t.clips {
                    let x0 = x_of(c.start);
                    let x1 = x_of(c.end());
                    if x1 < lanes.left() || x0 > lanes.right() {
                        hits.push((Rect::NOTHING, c.id, ti));
                        continue;
                    }
                    let cr = Rect::from_min_max(
                        pos2(x0, lane.top() + 2.0),
                        pos2(x1.max(x0 + 2.0), lane.bottom() - 2.0),
                    );
                    hits.push((cr, c.id, ti));
                    let vis = cr.intersect(lanes).intersect(tracks_rect);
                    if !vis.is_positive() {
                        continue;
                    }
                    let cp = clip_painter.with_clip_rect(vis);
                    let sel = selected_clip == Some(c.id) || sel_clips.contains(&c.id);
                    // A selected clip keeps its track colour, only lighter.
                    let lighter = |c: Color32, k: f32| {
                        Color32::from_rgb(
                            c.r() + ((255 - c.r()) as f32 * k) as u8,
                            c.g() + ((255 - c.g()) as f32 * k) as u8,
                            c.b() + ((255 - c.b()) as f32 * k) as u8,
                        )
                    };
                    let body_col = if c.muted {
                        crate::theme::gray(40)
                    } else if sel {
                        lighter(bg, 0.22)
                    } else {
                        bg
                    };
                    let wave_col = if c.muted {
                        crate::theme::gray(90)
                    } else if sel {
                        lighter(wavec, 0.35)
                    } else {
                        wavec
                    };
                    cp.rect_filled(cr, 0.0, body_col);
                    let hb = Rect::from_min_max(cr.min, pos2(cr.right(), cr.top() + 15.0));
                    cp.rect_filled(hb, 0.0, if sel { lighter(headc, 0.22) } else { headc });
                    cp.text(
                        pos2(cr.left() + 5.0, hb.center().y),
                        egui::Align2::LEFT_CENTER,
                        &c.name,
                        FontId::proportional(10.5),
                        Color32::WHITE,
                    );
                    if c.gain_db.abs() > 0.05 {
                        cp.text(
                            pos2(cr.right() - 22.0, hb.center().y),
                            egui::Align2::RIGHT_CENTER,
                            format!("{:+.1} dB", c.gain_db),
                            FontId::proportional(9.5),
                            Color32::WHITE,
                        );
                    }
                    // stretched clips show their length as a share of the original
                    let pct = 100.0 / c.speed.max(1e-6);
                    if (pct - 100.0).abs() > 0.05 {
                        cp.text(
                            pos2(
                                cr.right() - if c.gain_db.abs() > 0.05 { 70.0 } else { 22.0 },
                                hb.center().y,
                            ),
                            egui::Align2::RIGHT_CENTER,
                            format!("{pct:.1}%"),
                            FontId::proportional(9.5),
                            Color32::from_rgb(255, 220, 120),
                        );
                    }
                    if stretch_handles {
                        for (corner, left) in [(cr.left_top(), true), (cr.right_top(), false)] {
                            let d = if left { 1.0 } else { -1.0 };
                            let tri = vec![
                                corner + vec2(d * 1.0, 1.0),
                                corner + vec2(d * 11.0, 1.0),
                                corner + vec2(d * 1.0, 11.0),
                            ];
                            cp.add(egui::Shape::convex_polygon(
                                tri,
                                crate::theme::gray(235),
                                Stroke::new(1.0, crate::theme::gray(30)),
                            ));
                        }
                    }
                    if let MDrag::StretchL { id, .. } | MDrag::StretchR { id, .. } = drag_now {
                        if id == c.id {
                            clip_painter.text(
                                pos2(cr.center().x, cr.center().y),
                                egui::Align2::CENTER_CENTER,
                                format!("{pct:.1}%"),
                                FontId::proportional(16.0),
                                Color32::WHITE,
                            );
                        }
                    }
                    // waveform
                    if let Some(f) = self.file(c.file_id) {
                        let body = Rect::from_min_max(pos2(cr.left(), hb.bottom()), cr.max);
                        let nch = f.data.channels().clamp(1, 2);
                        let h = body.height() / nch as f32;
                        let g = audio::undb(c.gain_db);
                        // map session time to source time: source = offset + (t - start)
                        let s0 = c.offset + (view.0 - c.start) * c.speed;
                        let s1 = c.offset + (view.1 - c.start) * c.speed;
                        let full_lane = Rect::from_min_max(
                            pos2(lanes.left(), body.top()),
                            pos2(lanes.right(), body.bottom()),
                        );
                        let wp = cp.with_clip_rect(vis.intersect(body));
                        for ch in 0..nch {
                            let lane_r = Rect::from_min_size(
                                pos2(full_lane.left(), full_lane.top() + ch as f32 * h),
                                vec2(full_lane.width(), h),
                            );
                            // the zero (-inf) line sits under the waveform
                            let zy = lane_r.center().y;
                            let zero_col = if sel && !c.muted {
                                crate::theme::gray(70)
                            } else {
                                theme::zero_line()
                            };
                            wp.line_segment(
                                [pos2(vis.left(), zy), pos2(vis.right(), zy)],
                                Stroke::new(1.0, zero_col),
                            );
                            draw_wave(&wp, lane_r, &f.data, &f.peaks, ch, s0, s1, g, wave_col);
                        }
                    }
                    // fades (effective: an overlap gives a balanced crossfade spanning it)
                    let (efin, efout, xin, xout) = eff
                        .get(&c.id)
                        .copied()
                        .unwrap_or((c.fade_in, c.fade_out, false, false));
                    let body_top = cr.top() + 15.0;
                    let cc = &c.curves;
                    let curve = |a: f64, b: f64, rising: bool, x: bool| -> Vec<egui::Pos2> {
                        let (pts, hs) = if rising {
                            (&cc.fin_pts, &cc.fin_h)
                        } else {
                            (&cc.fout_pts, &cc.fout_h)
                        };
                        let shape = if rising { cc.fin_shape } else { cc.fout_shape };
                        (0..=60)
                            .map(|k| {
                                let f = k as f64 / 60.0;
                                let g = curves::fade_gain(
                                    pts,
                                    hs,
                                    if rising { f } else { 1.0 - f },
                                    &|u| crate::session::shaped_fade(u, x, shape),
                                );
                                pos2(
                                    x_of(a + (b - a) * f),
                                    cr.bottom() - g * (cr.bottom() - body_top),
                                )
                            })
                            .collect()
                    };
                    let fcol = Color32::from_rgb(200, 170, 70);
                    // fade points: keyframes on the fade curve (fraction from the silent end, gain)
                    let fade_area = Rect::from_min_max(pos2(cr.left(), body_top), cr.max);
                    let fclip = Rect::from_min_max(
                        pos2(lanes.left(), body_top - 1.0),
                        pos2(lanes.right(), cr.bottom() + 1.0),
                    )
                    .intersect(tracks_rect);
                    if efin > 0.0 && (!cc.fin_pts.is_empty() || direct_tool) {
                        key_views.push(KeyView {
                            target: EnvTarget::FadeIn(c.id),
                            area: fade_area,
                            clip: fclip,
                            kind: VKind::Fade,
                            origin: c.start,
                            scale: efin,
                            voff: 0.0,
                            tmin: 0.0,
                            tmax: 1.0,
                            col: crate::theme::gray(235),
                            xl: kv_xl,
                            v0: kv_v0,
                            pps: kv_pps,
                            editable: true,
                        });
                    }
                    if efout > 0.0 && (!cc.fout_pts.is_empty() || direct_tool) {
                        key_views.push(KeyView {
                            target: EnvTarget::FadeOut(c.id),
                            area: fade_area,
                            clip: fclip,
                            kind: VKind::Fade,
                            origin: c.end(),
                            scale: -efout,
                            voff: 0.0,
                            tmin: 0.0,
                            tmax: 1.0,
                            col: crate::theme::gray(235),
                            xl: kv_xl,
                            v0: kv_v0,
                            pps: kv_pps,
                            editable: true,
                        });
                    }
                    if efin > 0.0 {
                        let pts = curve(c.start, c.start + efin, true, xin);
                        let mut shade = vec![pos2(cr.left(), body_top)];
                        shade.extend(pts.iter().copied());
                        cp.add(egui::Shape::Path(egui::epaint::PathShape {
                            points: shade,
                            closed: true,
                            fill: Color32::from_rgba_unmultiplied(
                                0,
                                0,
                                0,
                                if xin { 40 } else { 90 },
                            ),
                            stroke: Stroke::NONE.into(),
                        }));
                        if xin {
                            xfade_lines.push((
                                Rect::from_min_max(
                                    pos2(lanes.left(), body_top - 1.0),
                                    pos2(lanes.right(), cr.bottom() + 1.0),
                                )
                                .intersect(tracks_rect),
                                pts,
                            ));
                        } else {
                            cp.add(egui::Shape::line(pts, Stroke::new(1.0, fcol)));
                        }
                    }
                    if efout > 0.0 {
                        let pts = curve(c.end() - efout, c.end(), false, xout);
                        let mut shade = vec![pos2(cr.right(), body_top)];
                        shade.extend(pts.iter().rev().copied());
                        cp.add(egui::Shape::Path(egui::epaint::PathShape {
                            points: shade,
                            closed: true,
                            fill: Color32::from_rgba_unmultiplied(
                                0,
                                0,
                                0,
                                if xout { 40 } else { 90 },
                            ),
                            stroke: Stroke::NONE.into(),
                        }));
                        if xout {
                            // the next clip covers this part; draw it with the overlay pass
                            xfade_lines.push((
                                Rect::from_min_max(
                                    pos2(lanes.left(), body_top - 1.0),
                                    pos2(lanes.right(), cr.bottom() + 1.0),
                                )
                                .intersect(tracks_rect),
                                pts,
                            ));
                        } else {
                            cp.add(egui::Shape::line(pts, Stroke::new(1.0, fcol)));
                        }
                    }
                    // fade handles
                    let hi = pos2(x_of(c.start + efin).max(cr.left() + 5.0), body_top + 1.0);
                    let ho = pos2(x_of(c.end() - efout).min(cr.right() - 5.0), body_top + 1.0);
                    for hp in [hi, ho] {
                        cp.rect_filled(
                            Rect::from_center_size(hp + vec2(0.0, 4.0), vec2(10.0, 10.0)),
                            0.0,
                            crate::theme::gray(225),
                        );
                        cp.rect_stroke(
                            Rect::from_center_size(hp + vec2(0.0, 4.0), vec2(10.0, 10.0)),
                            0.0,
                            Stroke::new(1.0, crate::theme::gray(40)),
                            egui::StrokeKind::Inside,
                        );
                    }
                    // volume icon (bottom left): drag it up/down to make the clip louder/quieter
                    if cr.width() > 30.0 && cr.height() > 30.0 {
                        let gr = gain_icon_rect(cr);
                        let active = matches!(self.multi_drag.drag, MDrag::ClipGain { id, .. } if id == c.id);
                        let hover = ui
                            .ctx()
                            .pointer_hover_pos()
                            .map(|p| gr.expand(3.0).contains(p))
                            .unwrap_or(false);
                        let col = if active || hover {
                            Color32::WHITE
                        } else {
                            crate::theme::gray(205)
                        };
                        for k in 0..4 {
                            let h = 3.0 + k as f32 * 2.2;
                            let x = gr.left() + 1.0 + k as f32 * 3.2;
                            cp.rect_filled(
                                Rect::from_min_max(
                                    pos2(x, gr.bottom() - h),
                                    pos2(x + 2.0, gr.bottom()),
                                ),
                                0.0,
                                col,
                            );
                        }
                        if active || hover {
                            let txt = format!("{:+.1} dB", c.gain_db);
                            cp.text(
                                pos2(gr.right() + 5.0, gr.center().y),
                                egui::Align2::LEFT_CENTER,
                                txt,
                                FontId::proportional(11.0),
                                Color32::WHITE,
                            );
                        }
                    }
                    let kclip = key_clip(cr, tracks_rect);
                    if show_env {
                        let body = Rect::from_min_max(pos2(cr.left(), cr.top() + 15.0), cr.max);
                        let g = c.gain_db;
                        let pts = env_line(
                            &c.env,
                            &c.curves.env_h,
                            &|t| x_of(c.start + t),
                            &|v| env_y(body, v + g),
                            cr.left(),
                            cr.right(),
                            0.0,
                        );
                        cp.add(egui::Shape::line(pts, Stroke::new(1.2, VOL_COL)));
                        key_views.push(KeyView {
                            target: EnvTarget::ClipVol(c.id),
                            area: body,
                            clip: kclip,
                            kind: VKind::Vol,
                            origin: c.start,
                            scale: 1.0,
                            voff: g,
                            tmin: 0.0,
                            tmax: c.len,
                            col: VOL_COL,
                            xl: kv_xl,
                            v0: kv_v0,
                            pps: kv_pps,
                            editable: true,
                        });
                    }
                    if show_pan {
                        let body = Rect::from_min_max(pos2(cr.left(), cr.top() + 15.0), cr.max);
                        // a little see-through so the zero line under a centred pan line still shows
                        let pcol = Color32::from_rgba_unmultiplied(90, 165, 255, 190);
                        let pts = env_line(
                            &c.pan_env,
                            &c.curves.pan_h,
                            &|t| x_of(c.start + t),
                            &|v| pan_y(body, v),
                            cr.left(),
                            cr.right(),
                            0.0,
                        );
                        cp.add(egui::Shape::line(pts, Stroke::new(1.2, pcol)));
                        key_views.push(KeyView {
                            target: EnvTarget::ClipPan(c.id),
                            area: body,
                            clip: kclip,
                            kind: VKind::Pan,
                            origin: c.start,
                            scale: 1.0,
                            voff: 0.0,
                            tmin: 0.0,
                            tmax: c.len,
                            col: PAN_COL,
                            xl: kv_xl,
                            v0: kv_v0,
                            pps: kv_pps,
                            editable: true,
                        });
                    }
                    cp.rect_stroke(
                        cr,
                        0.0,
                        Stroke::new(
                            if sel { 1.5 } else { 1.0 },
                            if sel {
                                Color32::WHITE
                            } else {
                                crate::theme::gray(10)
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        for (clip, pts) in xfade_lines {
            let xp = clip_painter.with_clip_rect(clip);
            xp.add(egui::Shape::line(
                pts,
                Stroke::new(1.6, Color32::from_rgb(200, 180, 60)),
            ));
        }

        // ---------- automation lanes, opened under their tracks by the chevron left of Read
        // (track index, lane rect, the edited envelope's line on screen)
        let mut auto_lanes: Vec<(usize, Rect, Vec<egui::Pos2>)> = vec![];
        {
            let s = self.session.as_ref().unwrap();
            for (ti, t) in s.tracks.iter().enumerate() {
                let Some(Some(full_r)) = auto_rects.get(ti).copied() else {
                    continue;
                };
                let vis = full_r.intersect(tracks_rect);
                if !vis.is_positive() {
                    continue;
                }
                let lp = clip_painter.with_clip_rect(vis);
                paint_lane_bg(&lp, full_r);
                let r = full_r.shrink2(vec2(0.0, 3.0));
                let read = t.io.automation != 0;
                let a = &t.auto;
                let mut edit_line = vec![];
                // the extra parameters (Mute, rack, Track EQ, effect settings) under the main pair
                for (pi, prm) in a.params.iter().enumerate() {
                    if !prm.shown {
                        continue;
                    }
                    let editing = a.param == 2 && a.sel == prm.key;
                    let kind = crate::lanes::kind_of(prm);
                    let base = crate::lanes::param_color(pi);
                    let col = if !read {
                        crate::theme::gray(120)
                    } else if editing {
                        base
                    } else {
                        base.gamma_multiply(0.45)
                    };
                    let empty = crate::lanes::static_value(t, &prm.key).unwrap_or(prm.min);
                    let y_of = |v: f32| crate::keys::y_of(kind, r, v);
                    let line =
                        env_line(&prm.env, &prm.hs, &x_of, &y_of, r.left(), r.right(), empty);
                    lp.add(egui::Shape::line(
                        line.clone(),
                        Stroke::new(if editing { 1.5 } else { 1.0 }, col),
                    ));
                    if editing {
                        lp.text(
                            pos2(r.left() + 6.0, r.bottom() - 3.0),
                            egui::Align2::LEFT_BOTTOM,
                            &prm.label,
                            FontId::proportional(9.5),
                            col,
                        );
                        edit_line = line;
                    }
                    key_views.push(KeyView {
                        target: EnvTarget::TrackParam(t.id, pi as u32),
                        area: r,
                        clip: vis,
                        kind,
                        origin: 0.0,
                        scale: 1.0,
                        voff: 0.0,
                        tmin: 0.0,
                        tmax: f64::MAX / 4.0,
                        col: base,
                        xl: kv_xl,
                        v0: kv_v0,
                        pps: kv_pps,
                        editable: editing && !a.lock,
                    });
                }
                // the other envelope first (dimmed), the selected one on top
                for pan in [a.param != 1, a.param == 1] {
                    let shown = if pan { a.show_pan } else { a.show_vol };
                    if !shown {
                        continue;
                    }
                    let editing = a.param < 2 && (a.param == 1) == pan;
                    let (env, hs) = if pan {
                        (&a.pan_env, &a.pan_h)
                    } else {
                        (&t.vol_env, &a.vol_h)
                    };
                    let line = paint_auto_env(&lp, r, env, hs, &x_of, pan, editing, read);
                    if editing {
                        edit_line = line;
                    }
                    key_views.push(KeyView {
                        target: if pan {
                            EnvTarget::TrackPan(t.id)
                        } else {
                            EnvTarget::TrackVol(t.id)
                        },
                        area: r,
                        clip: vis,
                        kind: if pan { VKind::Pan } else { VKind::Vol },
                        origin: 0.0,
                        scale: 1.0,
                        voff: 0.0,
                        tmin: 0.0,
                        tmax: f64::MAX / 4.0,
                        col: if pan { PAN_COL } else { VOL_COL },
                        xl: kv_xl,
                        v0: kv_v0,
                        pps: kv_pps,
                        editable: editing && !a.lock,
                    });
                }
                if a.lock {
                    lp.text(
                        pos2(r.left() + 6.0, r.top() + 4.0),
                        egui::Align2::LEFT_TOP,
                        "\u{1f512} protected",
                        FontId::proportional(9.5),
                        theme::text_dim(),
                    );
                }
                auto_lanes.push((ti, full_r, edit_line));
            }
        }

        // ---------- time selection overlay
        if let Some((a, b)) = selection {
            let xa = x_of(a).max(lanes.left());
            let xb = x_of(b).min(lanes.right());
            if xb > xa {
                clip_painter.rect_filled(
                    Rect::from_min_max(pos2(xa, tracks_rect.top()), pos2(xb, tracks_rect.bottom())),
                    0.0,
                    crate::theme::gray_a(255, 28),
                );
                p.rect_filled(
                    Rect::from_min_max(pos2(xa, ruler.bottom() - 4.0), pos2(xb, ruler.bottom())),
                    0.0,
                    theme::accent(),
                );
            }
        }

        // ---------- interaction
        let resp = ui.interact(
            lanes.intersect(tracks_rect),
            ui.scope_id().with("mlanes"),
            Sense::click_and_drag(),
        );
        // the border under each track's clips makes the track taller or shorter; the border
        // under an open automation lane changes the lane's height
        for (ti, lr) in lane_rects.iter().enumerate() {
            let cb = clip_rects
                .get(ti)
                .map(|r| r.bottom())
                .unwrap_or(lr.bottom());
            let has_lane = matches!(auto_rects.get(ti), Some(Some(_)));
            for (lane_border, yb) in [(false, cb), (true, lr.bottom())] {
                if lane_border && !has_lane {
                    continue;
                }
                let zone = Rect::from_min_max(
                    pos2(full.left() + 4.0, yb - 3.0),
                    pos2(lanes.right(), yb + 3.0),
                )
                .intersect(tracks_rect);
                if !zone.is_positive() {
                    continue;
                }
                let zr = ui.interact(
                    zone,
                    ui.scope_id().with(("track_border", ti, lane_border)),
                    Sense::drag(),
                );
                if zr.hovered() || zr.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
                if zr.drag_started() {
                    let y0 = ui
                        .input(|i| i.pointer.press_origin())
                        .map(|p| p.y)
                        .unwrap_or(yb);
                    let t = self.session.as_ref().and_then(|s| s.tracks.get(ti));
                    self.multi_drag.drag = if lane_border {
                        MDrag::LaneH {
                            ti,
                            start_h: t.map(|t| t.auto.lane_h).unwrap_or(120.0),
                            start_y: y0,
                        }
                    } else {
                        let h = t
                            .map(|t| {
                                if t.minimized {
                                    crate::session::MIN_TRACK_H
                                } else {
                                    t.height
                                }
                            })
                            .unwrap_or(96.0);
                        MDrag::TrackH {
                            ti,
                            start_h: h,
                            start_y: y0,
                        }
                    };
                }
                if zr.dragged() {
                    if let (Some(pp), Some(s)) = (zr.interact_pointer_pos(), self.session.as_mut())
                    {
                        match self.multi_drag.drag {
                            MDrag::TrackH {
                                ti: dti,
                                start_h,
                                start_y,
                            } => {
                                if let Some(t) = s.tracks.get_mut(dti) {
                                    t.minimized = false;
                                    t.height = (start_h + pp.y - start_y).clamp(40.0, 600.0);
                                }
                            }
                            MDrag::LaneH {
                                ti: dti,
                                start_h,
                                start_y,
                            } => {
                                if let Some(t) = s.tracks.get_mut(dti) {
                                    t.auto.lane_h = (start_h + pp.y - start_y).clamp(40.0, 600.0);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if zr.drag_stopped() {
                    // a resized track is its own undo step, so undoing a later change keeps the height
                    if matches!(self.multi_drag.drag, MDrag::TrackH { .. }) {
                        self.session_push("Resize Track");
                    }
                    self.multi_drag.drag = MDrag::None;
                }
            }
        }

        // The automation lanes are registered after the clip area, so their clicks win (and the
        // keyframes, registered later still, win over the lanes). Like the original: click on the
        // selected envelope's line to add a keyframe there (drag to place it), click elsewhere
        // to deselect keyframes.
        let mut auto_hist: Option<&'static str> = None;
        let snap_rate = self
            .session
            .as_ref()
            .map(|s| s.rate as f64)
            .unwrap_or(48000.0)
            .max(1.0);
        let snap_time = |x: f32| -> f64 { (t_of(x).max(0.0) * snap_rate).round() / snap_rate };
        for (ti, full_r, line) in &auto_lanes {
            let r = full_r.shrink2(vec2(0.0, 3.0));
            let aresp = ui.interact(
                full_r.intersect(tracks_rect),
                ui.scope_id().with(("autolane", *ti)),
                Sense::click_and_drag(),
            );
            let (target, kind, locked) = {
                let t = &self.session.as_ref().unwrap().tracks[*ti];
                let (tg, k) = crate::lanes::edited(t);
                (tg, k, t.auto.lock)
            };
            let on_line = |p: egui::Pos2| !line.is_empty() && dist_to_line(line, p) < 6.0;
            if let Some(hp) = aresp.hover_pos() {
                ui.ctx().set_cursor_icon(if on_line(hp) && !locked {
                    egui::CursorIcon::Crosshair
                } else {
                    egui::CursorIcon::Default
                });
            }
            let value_at = |y: f32| crate::keys::v_at(kind, r, y);
            let add = |s: &mut crate::session::Session, pp: egui::Pos2| -> Option<usize> {
                let tr = s.tracks.get_mut(*ti)?;
                let i = crate::lanes::add_key(tr, snap_time(pp.x), value_at(pp.y));
                s.selected_track = *ti;
                s.master_selected = false;
                s.touch();
                Some(i)
            };
            if aresp.clicked() {
                if let Some(pp) = aresp.interact_pointer_pos() {
                    if on_line(pp) && !locked {
                        let i = self.session.as_mut().and_then(|s| add(s, pp));
                        if let Some(i) = i {
                            self.multi_drag.key_sel.only(target, i);
                        }
                        auto_hist = Some("Add Automation Keyframe");
                    } else {
                        self.multi_drag.key_sel.clear();
                        if let Some(s) = self.session.as_mut() {
                            s.selected_track = *ti;
                            s.master_selected = false;
                        }
                    }
                }
            }
            if aresp.drag_started() && !locked {
                if let Some(pp) = ui
                    .input(|i| i.pointer.press_origin())
                    .or(aresp.interact_pointer_pos())
                {
                    if on_line(pp) {
                        if let Some(idx) = self.session.as_mut().and_then(|s| add(s, pp)) {
                            self.multi_drag.key_sel.only(target, idx);
                            self.multi_drag.drag = MDrag::AutoPoint { track: *ti, idx };
                        }
                    }
                }
            }
            if aresp.dragged() {
                if let (Some(pp), MDrag::AutoPoint { track, idx }) =
                    (aresp.interact_pointer_pos(), self.multi_drag.drag)
                {
                    if track == *ti {
                        // Shift locks the point to the main 8 directions, like clips and keyframes
                        let pp = if ui.input(|i| i.modifiers.shift) {
                            ui.input(|i| i.pointer.press_origin())
                                .map(|o| crate::keys::snap_dir8(o, pp))
                                .unwrap_or(pp)
                        } else {
                            pp
                        };
                        if let Some(s) = self.session.as_mut() {
                            if let Some((e, _)) = crate::keys::env_mut(s, target) {
                                crate::workflows::move_auto_point(
                                    e,
                                    idx,
                                    snap_time(pp.x),
                                    value_at(pp.y),
                                );
                            }
                            s.touch();
                        }

                        auto_hist = Some("Edit Automation");
                    }
                }
            }
            if aresp.drag_stopped()
                && matches!(self.multi_drag.drag, MDrag::AutoPoint { track, .. } if track == *ti)
            {
                self.multi_drag.drag = MDrag::None;
            }
        }
        let stretch_mode = self.stretch_mode;
        let razor = self.tool == Tool::Razor;
        // the clip under a point (topmost), whatever part of it
        let clip_at = |pp: egui::Pos2| -> Option<u64> {
            hits.iter()
                .rev()
                .find(|(r, _, _)| r.contains(pp))
                .map(|(_, id, _)| *id)
        };
        let hit_test = |s: &crate::session::Session, pp: egui::Pos2| -> (Hit, Option<usize>) {
            let track = lane_rects
                .iter()
                .position(|r| r.contains(pp) || (pp.y >= r.top() && pp.y < r.bottom()));
            // on a fade curve (a crossfade's hidden fade-out too): click there to shape it
            if !razor {
                for (r, id, _) in hits.iter().rev() {
                    let Some(c) = s.find_clip(*id).map(|(t, ci)| &s.tracks[t].clips[ci]) else {
                        continue;
                    };
                    let (efin, efout, xin, xout) = eff
                        .get(id)
                        .copied()
                        .unwrap_or((c.fade_in, c.fade_out, false, false));
                    let body_top = r.top() + 15.0;
                    if pp.y < body_top - 2.0 || pp.y > r.bottom() {
                        continue;
                    }
                    let gy = |g: f32| r.bottom() - g * (r.bottom() - body_top);
                    // leave the fade handles at the top corners to their own drag
                    let near_handle = pp.y < r.top() + 30.0
                        && ((pp.x - x_of(c.start + efin).max(r.left() + 5.0)).abs() < 11.0
                            || (pp.x - x_of(c.end() - efout).min(r.right() - 5.0)).abs() < 11.0);
                    if near_handle {
                        continue;
                    }
                    if efin > 0.0 && pp.x >= x_of(c.start) && pp.x <= x_of(c.start + efin) {
                        let f = ((t_of(pp.x) - c.start) / efin).clamp(0.0, 1.0);
                        let g = curves::fade_gain(&c.curves.fin_pts, &c.curves.fin_h, f, &|u| {
                            crate::session::shaped_fade(u, xin, c.curves.fin_shape)
                        });
                        if (gy(g) - pp.y).abs() < 5.0 {
                            return (Hit::FadeLine(*id, false), track);
                        }
                    }
                    if efout > 0.0 && pp.x >= x_of(c.end() - efout) && pp.x <= x_of(c.end()) {
                        let f = ((c.end() - t_of(pp.x)) / efout).clamp(0.0, 1.0);
                        let g = curves::fade_gain(&c.curves.fout_pts, &c.curves.fout_h, f, &|u| {
                            crate::session::shaped_fade(u, xout, c.curves.fout_shape)
                        });
                        if (gy(g) - pp.y).abs() < 5.0 {
                            return (Hit::FadeLine(*id, true), track);
                        }
                    }
                }
            }
            for (r, id, _) in hits.iter().rev() {
                if !r.expand(4.0).contains(pp) {
                    continue;
                }
                let Some(c) = s.find_clip(*id).map(|(t, ci)| &s.tracks[t].clips[ci]) else {
                    continue;
                };
                // the razor cuts wherever it lands on a clip
                if razor && r.contains(pp) {
                    return (Hit::Body(*id), track);
                }
                // stretch handles on both top corners while global clip stretching is on
                if stretch_mode && pp.y < r.top() + 14.0 {
                    if pp.x - r.left() < 12.0 {
                        return (Hit::StretchL(*id), track);
                    }
                    if r.right() - pp.x < 12.0 {
                        return (Hit::StretchR(*id), track);
                    }
                }
                if gain_icon_rect(*r).expand(3.0).contains(pp) {
                    return (Hit::Gain(*id), track);
                }
                let (efin, efout, _, _) = eff
                    .get(id)
                    .copied()
                    .unwrap_or((c.fade_in, c.fade_out, false, false));
                let fi = x_of(c.start + efin).max(r.left() + 5.0);
                let fo = x_of(c.end() - efout).min(r.right() - 5.0);
                let hy = r.top() + 20.0;
                if (pp.x - fi).abs() < 9.0 && (pp.y - hy).abs() < 9.0 {
                    return (Hit::FadeIn(*id), track);
                }
                if (pp.x - fo).abs() < 9.0 && (pp.y - hy).abs() < 9.0 {
                    return (Hit::FadeOut(*id), track);
                }
                if show_pan && pp.y > r.top() + 15.0 {
                    let body = Rect::from_min_max(pos2(r.left(), r.top() + 15.0), r.max);
                    for (i, (et, ep)) in c.pan_env.iter().enumerate() {
                        if pos2(x_of(c.start + et), pan_y(body, *ep)).distance(pp) < 9.0 {
                            return (Hit::PanPoint(*id, i), track);
                        }
                    }
                }
                if show_env && pp.y > r.top() + 15.0 {
                    let body = Rect::from_min_max(pos2(r.left(), r.top() + 15.0), r.max);
                    for (i, (et, ed)) in c.env.iter().enumerate() {
                        if pos2(x_of(c.start + et), env_y(body, *ed + c.gain_db)).distance(pp) < 9.0
                        {
                            return (Hit::EnvPoint(*id, i), track);
                        }
                    }
                    let ly = env_y(
                        body,
                        crate::session::env_db(&c.env, t_of(pp.x) - c.start) + c.gain_db,
                    );
                    if (pp.y - ly).abs() < 6.0 && pp.x - r.left() >= 7.0 && r.right() - pp.x >= 7.0
                    {
                        return (Hit::EnvLine(*id), track);
                    }
                }
                if show_pan && pp.y > r.top() + 15.0 {
                    let body = Rect::from_min_max(pos2(r.left(), r.top() + 15.0), r.max);
                    let ly = pan_y(
                        body,
                        crate::session::env_db(&c.pan_env, t_of(pp.x) - c.start),
                    );
                    if (pp.y - ly).abs() < 6.0 && pp.x - r.left() >= 7.0 && r.right() - pp.x >= 7.0
                    {
                        return (Hit::PanLine(*id), track);
                    }
                }
                if on_left_edge(*r, lanes, pp) {
                    return (Hit::LeftEdge(*id), track);
                }
                if on_right_edge(*r, lanes, pp) {
                    return (Hit::RightEdge(*id), track);
                }
                if pp.y < r.top() + 15.0 {
                    return (Hit::Header(*id), track);
                }
                return (Hit::Body(*id), track);
            }
            (Hit::None, track)
        };
        if let Some(hp) = resp.hover_pos() {
            let (h, _) = hit_test(self.session.as_ref().unwrap(), hp);
            let icon = match (h, self.tool) {
                (Hit::StretchL(_) | Hit::StretchR(_), _) => egui::CursorIcon::ResizeColumn,
                (Hit::LeftEdge(_) | Hit::RightEdge(_), _) => egui::CursorIcon::ResizeHorizontal,
                // a fade handle drags both ways: sideways its length, up/down its curve
                (Hit::FadeIn(_) | Hit::FadeOut(_), _) => egui::CursorIcon::Move,
                (Hit::Gain(_), _) => egui::CursorIcon::ResizeVertical,
                (Hit::EnvPoint(..) | Hit::PanPoint(..), _) => egui::CursorIcon::Move,
                (Hit::FadeLine(..), _) => egui::CursorIcon::Crosshair,
                (Hit::PanLine(_), _) => egui::CursorIcon::Crosshair,
                (Hit::EnvLine(_), _) => egui::CursorIcon::Crosshair,
                (Hit::Body(_) | Hit::Header(_), Tool::Razor) => egui::CursorIcon::Crosshair,
                (Hit::Body(_) | Hit::Header(_), Tool::Move) | (Hit::Header(_), _) => {
                    egui::CursorIcon::Grab
                }
                (Hit::Body(_), Tool::Slip) => egui::CursorIcon::ResizeColumn,
                _ => egui::CursorIcon::Text,
            };
            ui.ctx().set_cursor_icon(icon);
        }
        let snap_px = self.prefs.snapping_distance();
        let snap_prefs = self.prefs.clone();
        let snapping = self.snapping;
        // the targets as the drag began this frame (the cursor and view may change below)
        let (snap_cursor, snap_view) = (cursor, view);
        // the dragged clip's own edges are left out by clip (not by position), so an edge that
        // has landed on another clip's edge, on any track, stays snapped to it
        let snap = |s: &crate::session::Session, t: f64, exclude: Option<u64>, len: f64| -> f64 {
            if !snapping {
                return t;
            }
            let mut best = t;
            let mut bd = snap_px as f64 / lanes.width() as f64 * span;
            let pts = collect_snap_targets_except(
                s,
                &snap_prefs,
                snap_cursor,
                snap_view.1 - snap_view.0,
                lanes.width(),
                snap_view,
                exclude,
            );
            for p in pts {
                for (cand, off) in [(p, 0.0), (p - len, len)] {
                    let _ = off;
                    if (cand - t).abs() < bd {
                        bd = (cand - t).abs();
                        best = cand;
                    }
                }
            }
            best
        };
        let srate = self
            .session
            .as_ref()
            .map(|s| s.rate as f64)
            .unwrap_or(48000.0);
        // snap a time to the session's sample grid (clip ends meet exactly on sample points)
        let on_grid = move |t: f64| (t * srate).round() / srate;
        let middle = ui.input(|i| i.pointer.middle_down());
        if resp.drag_started() && !middle {
            // hit-test where the button went down, not where the drag was recognised
            if let Some(pp) = ui
                .input(|i| i.pointer.press_origin())
                .or(resp.interact_pointer_pos())
            {
                let t = t_of(pp.x).max(0.0);
                let (h, track) = hit_test(self.session.as_ref().unwrap(), pp);
                let s = self.session.as_mut().unwrap();
                if let Some(tk) = track {
                    s.selected_track = tk;
                }
                self.multi_drag.drag = match (h, self.tool) {
                    // the razor only cuts (on click); dragging with it does nothing to clips
                    (_, Tool::Razor) => MDrag::None,
                    (Hit::StretchL(id), _) => match s.find_clip(id) {
                        Some((a, b)) => {
                            let c = &s.tracks[a].clips[b];
                            MDrag::StretchL {
                                id,
                                end: c.end(),
                                orig_len: c.len,
                                orig_speed: c.speed,
                            }
                        }
                        None => MDrag::None,
                    },
                    (Hit::StretchR(id), _) => match s.find_clip(id) {
                        Some((a, b)) => {
                            let c = &s.tracks[a].clips[b];
                            MDrag::StretchR {
                                id,
                                orig_len: c.len,
                                orig_speed: c.speed,
                            }
                        }
                        None => MDrag::None,
                    },
                    (Hit::LeftEdge(id), _) => MDrag::TrimL { id },
                    (Hit::RightEdge(id), _) => MDrag::TrimR { id },
                    (Hit::FadeIn(id), _) => {
                        let sh = s.clip_mut(id).map(|c| c.curves.fin_shape).unwrap_or(0.0);
                        self.multi_drag.fade_start = (pp.y, sh);
                        MDrag::FadeIn { id }
                    }
                    (Hit::FadeOut(id), _) => {
                        let sh = s.clip_mut(id).map(|c| c.curves.fout_shape).unwrap_or(0.0);
                        self.multi_drag.fade_start = (pp.y, sh);
                        MDrag::FadeOut { id }
                    }
                    (Hit::Gain(id), _) => match s.clip_mut(id) {
                        Some(c) => MDrag::ClipGain {
                            id,
                            y0: pp.y,
                            db0: c.gain_db,
                        },
                        None => MDrag::None,
                    },
                    (Hit::EnvPoint(id, idx), _) => MDrag::Env { id, idx },
                    (Hit::PanPoint(id, idx), _) => MDrag::PanEnv { id, idx },
                    (Hit::FadeLine(id, out), _) => {
                        match fade_point_at(s, &eff, id, out, t, pp, &hits) {
                            Some(idx) => {
                                s.touch();
                                self.multi_drag.changed = true;
                                self.multi_drag.key_sel.only(
                                    if out {
                                        EnvTarget::FadeOut(id)
                                    } else {
                                        EnvTarget::FadeIn(id)
                                    },
                                    idx,
                                );
                                MDrag::FadePt { id, out, idx }
                            }
                            None => MDrag::None,
                        }
                    }
                    (Hit::PanLine(id), _) => {
                        let body_y = hits.iter().find(|h| h.1 == id).map(|h| {
                            Rect::from_min_max(pos2(h.0.left(), h.0.top() + 15.0), h.0.max)
                        });
                        match (s.clip_mut(id), body_y) {
                            (Some(c), Some(body)) => {
                                let rel = (t - c.start).clamp(0.0, c.len);
                                let idx = curves::insert_key(
                                    &mut c.pan_env,
                                    &mut c.curves.pan_h,
                                    rel,
                                    pan_at(body, pp.y),
                                );
                                s.touch();
                                self.multi_drag.changed = true;
                                self.multi_drag.key_sel.only(EnvTarget::ClipPan(id), idx);
                                MDrag::PanEnv { id, idx }
                            }
                            _ => MDrag::None,
                        }
                    }
                    (Hit::EnvLine(id), _) => {
                        let body_y = hits.iter().find(|h| h.1 == id).map(|h| {
                            Rect::from_min_max(pos2(h.0.left(), h.0.top() + 15.0), h.0.max)
                        });
                        match (s.clip_mut(id), body_y) {
                            (Some(c), Some(body)) => {
                                let rel = (t - c.start).clamp(0.0, c.len);
                                let d = env_db_at(body, pp.y) - c.gain_db;
                                let idx =
                                    curves::insert_key(&mut c.env, &mut c.curves.env_h, rel, d);
                                s.touch();
                                self.multi_drag.changed = true;
                                self.multi_drag.key_sel.only(EnvTarget::ClipVol(id), idx);
                                MDrag::Env { id, idx }
                            }
                            _ => MDrag::None,
                        }
                    }
                    (Hit::Body(id), Tool::Slip) => {
                        let off = s.clip_mut(id).map(|c| c.offset).unwrap_or(0.0);
                        MDrag::Slip {
                            id,
                            grab: t,
                            orig_offset: off,
                        }
                    }
                    (Hit::Body(id), Tool::Move)
                    | (Hit::Header(id), Tool::Move | Tool::TimeSel | Tool::Slip | Tool::Direct) => {
                        let (st, tk) = s
                            .find_clip(id)
                            .map(|(tk, ci)| (s.tracks[tk].clips[ci].start, tk))
                            .unwrap_or((0.0, 0));
                        MDrag::Move {
                            id,
                            grab: t - st,
                            orig_start: st,
                            orig_track: tk,
                        }
                    }
                    _ => MDrag::TimeSel { anchor: t },
                };
                // several picked clips drag as one unit: record who moves with the clip
                // in hand, and where each of them sits now
                if let MDrag::Move { id, .. } = self.multi_drag.drag {
                    if self.multi_drag.crew.is_empty() {
                        self.multi_drag.crew = s.drag_crew(id);
                    }
                }
                // Alt+drag moves a copy of the whole unit and leaves the originals in place
                if let (
                    MDrag::Move {
                        id,
                        grab,
                        orig_start,
                        orig_track,
                    },
                    true,
                ) = (self.multi_drag.drag, ui.input(|i| i.modifiers.alt))
                {
                    let crew = self.multi_drag.crew.clone();
                    let mut copies: Vec<(u64, f64, usize)> = vec![];
                    let mut new_ids: Vec<u64> = vec![];
                    let mut grabbed: Option<u64> = None;
                    for (cid, cst, _ctrk) in crew {
                        if let Some((a, b)) = s.find_clip(cid) {
                            let nid = s.id();
                            let mut c = s.tracks[a].clips[b].clone();
                            c.id = nid;
                            s.tracks[a].clips.push(c);
                            copies.push((nid, cst, a));
                            new_ids.push(nid);
                            if cid == id {
                                grabbed = Some(nid);
                            }
                        }
                    }
                    if let Some(nid) = grabbed {
                        // the copies become the selection; the originals stay put
                        s.selected_clips = new_ids;
                        s.selected_clip = Some(nid);
                        s.touch();
                        self.multi_drag.changed = true;
                        self.multi_drag.crew = copies;
                        self.multi_drag.drag = MDrag::Move {
                            id: nid,
                            grab,
                            orig_start,
                            orig_track,
                        };
                    }
                }
                match self.multi_drag.drag {
                    MDrag::Move { id, .. } => {
                        s.selected_clip = Some(id);
                        // a multi-selection keeps its picked clips through the drag (the
                        // crew already carries the grabbed clip's group with it); a clip
                        // grabbed outside the selection picks up its group as before
                        if !s.selected_clips.contains(&id) || s.selected_clips.len() == 1 {
                            s.select_group_of(id);
                        }
                        // a clip picked last is what Delete removes, not a keyframe picked before
                        self.multi_drag.key_sel.clear();
                    }
                    MDrag::TrimL { id }
                    | MDrag::TrimR { id }
                    | MDrag::FadeIn { id }
                    | MDrag::FadeOut { id }
                    | MDrag::ClipGain { id, .. }
                    | MDrag::Slip { id, .. }
                    | MDrag::StretchL { id, .. }
                    | MDrag::StretchR { id, .. } => {
                        s.selected_clip = Some(id);
                        s.select_group_of(id);
                        // a clip picked last is what Delete removes, not a keyframe picked before
                        self.multi_drag.key_sel.clear();
                    }
                    // dragging a keyframe (or one just added on a line) selects it, not its clip
                    MDrag::Env { .. } | MDrag::PanEnv { .. } | MDrag::FadePt { .. } => {
                        s.clear_selected_clips();
                    }
                    _ => {}
                }
            }
        }
        if resp.dragged() && !middle {
            if let Some(pp) = resp.interact_pointer_pos() {
                let drag = self.multi_drag.drag;
                // Shift locks the drag to the main 8 directions: a clip, keyframe or handle
                // grabbed with Shift moves along one straight line only, so a mostly-upward
                // drag moves straight up and a mostly-rightward one straight right
                let pp = if ui.input(|i| i.modifiers.shift) && drag.is_object_drag() {
                    ui.input(|i| i.pointer.press_origin())
                        .map(|o| crate::keys::snap_dir8(o, pp))
                        .unwrap_or(pp)
                } else {
                    pp
                };
                let t = on_grid(t_of(pp.x).max(0.0));
                let track_at = lane_rects
                    .iter()
                    .position(|r| pp.y >= r.top() && pp.y < r.bottom());
                let mut snapped: Option<f64> = None;
                // a trimmed or stretched clip edge snaps like a moved clip: to the other clips'
                // starts and ends on every track (above and below too), markers, the playhead...
                let t = match drag {
                    MDrag::TrimL { id }
                    | MDrag::TrimR { id }
                    | MDrag::StretchL { id, .. }
                    | MDrag::StretchR { id, .. } => {
                        let vel = ui.input(|i| i.pointer.velocity());
                        let fast = vel.length() > self.prefs.snapping_distance() * 60.0;
                        let sn = if fast || !snapping {
                            t
                        } else {
                            on_grid(snap(self.session.as_ref().unwrap(), t, Some(id), 0.0))
                        };
                        self.multi_drag.edge_snap = ((sn - t).abs() > 1e-12).then_some(sn);
                        sn
                    }
                    _ => t,
                };
                if let MDrag::Move { id, grab, .. } = drag {
                    let len = self
                        .session
                        .as_ref()
                        .unwrap()
                        .find_clip(id)
                        .map(|(a, b)| self.session.as_ref().unwrap().tracks[a].clips[b].len)
                        .unwrap_or(0.0);
                    let raw = (t - grab).max(0.0);
                    // Sticky snapping, with the mouse's help: a drag holds the snap target it
                    // landed on until it travels past the snapping distance (so an inch of drag
                    // does not unsnap it), and a fast mouse is not pulled to a snap it cannot
                    // stop for (the drag only follows the pointer when it is still moving gently).
                    let vel = ui.input(|i| i.pointer.velocity());
                    let fast = vel.length() > self.prefs.snapping_distance() * 60.0;
                    let moved = if fast || !snapping {
                        on_grid(raw)
                    } else {
                        let sn = snap(self.session.as_ref().unwrap(), raw, Some(id), len);
                        on_grid(if (sn - raw).abs() > 1e-12 { sn } else { raw })
                    };
                    snapped = Some(moved);
                }
                // dragged below the last track: the clip gets a new track of its own
                let below_last = track_at.is_none()
                    && lane_rects
                        .last()
                        .map(|r| pp.y >= r.bottom())
                        .unwrap_or(false)
                    && pp.y < tracks_rect.bottom();
                let s = self.session.as_mut().unwrap();
                match drag {
                    MDrag::Move { id, .. } => {
                        let mut new_start = snapped.unwrap_or(0.0);
                        if let Some((ti, ci)) = s.find_clip(id) {
                            // the whole crew — every picked clip plus the grabbed clip's
                            // group — moves as one rigid unit
                            let mut crew = self.multi_drag.crew.clone();
                            if !crew.iter().any(|(c, _, _)| *c == id) {
                                // a drag without a crew (the session changed under it)
                                // still moves the clip in hand on its own
                                crew.push((id, s.tracks[ti].clips[ci].start, ti));
                            }
                            let (orig0, otrk0) = match crew.iter().find(|(c, _, _)| *c == id) {
                                Some((_, st, tr)) => (*st, *tr),
                                None => (s.tracks[ti].clips[ci].start, ti),
                            };
                            // none of the unit may go before the start of the timeline
                            let earliest = crew.iter().map(|(_, st, _)| *st).fold(orig0, f64::min);
                            new_start = new_start.max(orig0 - earliest);
                            let delta = new_start - orig0;
                            // one row for the clip in hand: the row under the pointer, a
                            // new track below the last one, or its own when the pointer
                            // left the tracks
                            let target = if below_last {
                                let stereo = s.tracks[ti].stereo;
                                Some(s.add_track(stereo))
                            } else {
                                track_at
                            };
                            let dtrack = target.map(|nt| nt as i64 - otrk0 as i64).unwrap_or(0);
                            let rows = s.tracks.len() as i64;
                            for (cid, ost, otrk) in crew {
                                let Some((ct, cc)) = s.find_clip(cid) else {
                                    continue;
                                };
                                s.tracks[ct].clips[cc].start = (ost + delta).max(0.0);
                                // the unit keeps its shape across the rows too
                                let mut row = otrk as i64 + dtrack;
                                if row < 0 {
                                    row = 0;
                                }
                                if row > rows - 1 {
                                    row = rows - 1;
                                }
                                let row = row as usize;
                                if row != ct {
                                    let c = s.tracks[ct].clips.remove(cc);
                                    s.tracks[row].clips.push(c);
                                    if cid == id {
                                        s.selected_track = row;
                                    }
                                }
                            }
                            s.touch();
                            self.multi_drag.changed = true;
                        }
                    }
                    MDrag::StretchR {
                        id,
                        orig_len,
                        orig_speed,
                    } => {
                        // stretching: the same audio over a longer or shorter time
                        if let Some(c) = s.clip_mut(id) {
                            let nl = (t - c.start).max(0.02);
                            c.len = nl;
                            c.speed = (orig_speed * orig_len / nl).clamp(0.1, 10.0);
                            c.len = orig_len * orig_speed / c.speed;
                            c.fade_out = c.fade_out.min(c.len);
                            s.touch();
                            self.multi_drag.changed = true;
                        }
                    }
                    MDrag::StretchL {
                        id,
                        end,
                        orig_len,
                        orig_speed,
                    } => {
                        if let Some(c) = s.clip_mut(id) {
                            let nl = (end - t).max(0.02);
                            c.speed = (orig_speed * orig_len / nl).clamp(0.1, 10.0);
                            c.len = orig_len * orig_speed / c.speed;
                            c.start = (end - c.len).max(0.0);
                            c.fade_in = c.fade_in.min(c.len);
                            s.touch();
                            self.multi_drag.changed = true;
                        }
                    }
                    MDrag::TrimL { id } => {
                        if let Some(c) = s.clip_mut(id) {
                            let end = c.end();
                            let min_start = (c.start - c.offset).max(0.0);
                            let ns = t.min(end - 0.01).max(min_start);
                            let d = ns - c.start;
                            c.start = ns;
                            c.offset += d * c.speed;
                            c.len -= d;
                            c.shift_env(d);
                            c.fade_in = c.fade_in.min(c.len);
                            s.touch();
                            self.multi_drag.changed = true;
                        }
                    }
                    MDrag::TrimR { id } => {
                        let file_len = s.find_clip(id).and_then(|(a, b)| {
                            let fid = s.tracks[a].clips[b].file_id;
                            self.files
                                .iter()
                                .find(|f| f.id == fid)
                                .map(|f| f.data.duration())
                        });
                        if let Some(c) = s.clip_mut(id) {
                            let max_end = c.start
                                + (file_len.unwrap_or(c.offset + c.len * c.speed) - c.offset)
                                    / c.speed.max(1e-6);
                            let ne = t.min(max_end).max(c.start + 0.01);
                            c.len = ne - c.start;
                            c.fade_out = c.fade_out.min(c.len);
                            s.touch();
                            self.multi_drag.changed = true;
                        }
                    }
                    MDrag::FadeIn { id } => {
                        let (y0, sh0) = self.multi_drag.fade_start;
                        if let Some(c) = s.clip_mut(id) {
                            // a dragged fade is the person's own length from now on (it may end
                            // before the overlap with the next clip does); up/down bends its curve
                            c.auto_in = false;
                            c.fade_in = (t - c.start).min(c.len - c.fade_out).max(0.0);
                            c.curves.fin_shape = fade_shape_drag(sh0, y0, pp.y);
                            s.touch();
                            self.multi_drag.changed = true;
                        }
                    }
                    MDrag::FadeOut { id } => {
                        let (y0, sh0) = self.multi_drag.fade_start;
                        if let Some(c) = s.clip_mut(id) {
                            c.auto_out = false;
                            c.fade_out = (c.end() - t).min(c.len - c.fade_in).max(0.0);
                            c.curves.fout_shape = fade_shape_drag(sh0, y0, pp.y);
                            s.touch();
                            self.multi_drag.changed = true;
                        }
                    }
                    MDrag::ClipGain { id, y0, db0 } => {
                        if let Some(c) = s.clip_mut(id) {
                            c.gain_db = clip_gain_drag(db0, y0, pp.y);
                            s.touch();
                            self.multi_drag.changed = true;
                        }
                    }
                    MDrag::Slip {
                        id,
                        grab,
                        orig_offset,
                    } => {
                        let file_len = s.find_clip(id).and_then(|(a, b)| {
                            let fid = s.tracks[a].clips[b].file_id;
                            self.files
                                .iter()
                                .find(|f| f.id == fid)
                                .map(|f| f.data.duration())
                        });
                        if let Some(c) = s.clip_mut(id) {
                            let maxo = (file_len.unwrap_or(c.offset + c.len) - c.len).max(0.0);
                            c.offset = (orig_offset - (t - grab)).clamp(0.0, maxo);
                            s.touch();
                            self.multi_drag.changed = true;
                        }
                    }
                    MDrag::Env { id, idx } => {
                        let body = hits.iter().find(|h| h.1 == id).map(|h| {
                            Rect::from_min_max(pos2(h.0.left(), h.0.top() + 15.0), h.0.max)
                        });
                        if let (Some(c), Some(body)) = (s.clip_mut(id), body) {
                            if idx < c.env.len() {
                                let lo = if idx > 0 { c.env[idx - 1].0 } else { 0.0 };
                                let hi = if idx + 1 < c.env.len() {
                                    c.env[idx + 1].0
                                } else {
                                    c.len
                                };
                                c.env[idx] = (
                                    (t - c.start).max(lo).min(hi),
                                    env_db_at(body, pp.y) - c.gain_db,
                                );
                                s.touch();
                                self.multi_drag.changed = true;
                            }
                        }
                    }
                    MDrag::FadePt { id, out, idx } => {
                        let (efin, efout) = eff.get(&id).map(|e| (e.0, e.1)).unwrap_or((0.0, 0.0));
                        let body = hits.iter().find(|h| h.1 == id).map(|h| {
                            Rect::from_min_max(pos2(h.0.left(), h.0.top() + 15.0), h.0.max)
                        });
                        if let (Some(c), Some(body)) = (s.clip_mut(id), body) {
                            let (pts, len, x) = if out {
                                (
                                    &mut c.curves.fout_pts,
                                    efout,
                                    (c.start + c.len - t) / efout.max(1e-9),
                                )
                            } else {
                                (&mut c.curves.fin_pts, efin, (t - c.start) / efin.max(1e-9))
                            };
                            if idx < pts.len() && len > 0.0 {
                                let lo = if idx > 0 { pts[idx - 1].0 + 1e-3 } else { 1e-3 };
                                let hi = if idx + 1 < pts.len() {
                                    pts[idx + 1].0 - 1e-3
                                } else {
                                    1.0 - 1e-3
                                };
                                let g = ((body.bottom() - pp.y) / body.height().max(1.0))
                                    .clamp(0.0, 1.0);
                                pts[idx] = (if hi > lo { x.clamp(lo, hi) } else { lo }, g);
                                s.touch();
                                self.multi_drag.changed = true;
                            }
                        }
                    }
                    MDrag::PanEnv { id, idx } => {
                        let body = hits.iter().find(|h| h.1 == id).map(|h| {
                            Rect::from_min_max(pos2(h.0.left(), h.0.top() + 15.0), h.0.max)
                        });
                        if let (Some(c), Some(body)) = (s.clip_mut(id), body) {
                            if idx < c.pan_env.len() {
                                let lo = if idx > 0 { c.pan_env[idx - 1].0 } else { 0.0 };
                                let hi = if idx + 1 < c.pan_env.len() {
                                    c.pan_env[idx + 1].0
                                } else {
                                    c.len
                                };
                                c.pan_env[idx] =
                                    ((t - c.start).max(lo).min(hi), pan_at(body, pp.y));
                                s.touch();
                                self.multi_drag.changed = true;
                            }
                        }
                    }
                    MDrag::TimeSel { anchor } => {
                        let (a, b) = if t < anchor { (t, anchor) } else { (anchor, t) };
                        selection = if b - a > 0.0 { Some((a, b)) } else { None };
                        cursor = a;
                    }
                    _ => {}
                }
                // auto-scroll horizontally at edges
                if pp.x > lanes.right() - 20.0 {
                    let d = span * 0.015;
                    view = (view.0 + d, view.1 + d);
                } else if pp.x < lanes.left() + 20.0 && view.0 > 0.0 {
                    let d = (span * 0.015).min(view.0);
                    view = (view.0 - d, view.1 - d);
                }
                ui.ctx().request_repaint();
            }
        }
        if resp.drag_stopped() {
            let label = match self.multi_drag.drag {
                MDrag::Move { .. } => Some(if self.multi_drag.crew.len() > 1 {
                    "Move Clips"
                } else {
                    "Move Clip"
                }),
                MDrag::StretchL { .. } | MDrag::StretchR { .. } => Some("Stretch Clip"),
                MDrag::TrimL { .. } | MDrag::TrimR { .. } => Some("Trim Clip"),
                MDrag::FadeIn { .. } | MDrag::FadeOut { .. } => Some("Clip Fade"),
                MDrag::FadePt { .. } => Some("Shape Fade"),
                MDrag::ClipGain { .. } => Some("Clip Volume"),
                MDrag::Slip { .. } => Some("Slip Clip"),
                MDrag::Env { .. } => Some("Clip Volume Envelope"),
                MDrag::PanEnv { .. } => Some("Clip Pan Envelope"),
                _ => None,
            };
            if let (Some(l), true) = (label, self.multi_drag.changed) {
                hist = Some(l);
            }
            self.multi_drag.changed = false;
            self.multi_drag.crew.clear();
            self.multi_drag.drag = MDrag::None;
        }
        if resp.clicked() {
            if let Some(pp) = resp.interact_pointer_pos() {
                let t = t_of(pp.x).max(0.0);
                let (h, track) = hit_test(self.session.as_ref().unwrap(), pp);
                let (shift, ctrl) =
                    ui.input(|i| (i.modifiers.shift, i.modifiers.ctrl || i.modifiers.command));
                let s = self.session.as_mut().unwrap();
                if let Some(tk) = track {
                    s.selected_track = tk;
                }
                // the razor cuts on the sample grid, anywhere on a clip (also on one cut before)
                let razor_hit = if self.tool == Tool::Razor {
                    clip_at(pp)
                } else {
                    None
                };
                // Shift and Ctrl pick a selection where the black mouse tool grabs: anywhere
                // with the Move tool, and on a clip's top bar with any tool (the one grab zone
                // every tool shares). Elsewhere the tool keeps its own click.
                let grab = self.tool == Tool::Move || matches!(h, Hit::Header(_));
                match (h, self.tool) {
                    (_, Tool::Razor) if razor_hit.is_some() || shift => {
                        let t = on_grid(t);
                        if shift {
                            let ids: Vec<u64> = s
                                .tracks
                                .iter()
                                .flat_map(|tr| tr.clips.iter())
                                .filter(|c| c.start < t && c.end() > t)
                                .map(|c| c.id)
                                .collect();
                            for i in ids {
                                s.split_clip(i, t);
                            }
                        } else if let Some(id) = razor_hit {
                            s.split_clip(id, t);
                        }
                        hist = Some("Split");
                    }
                    (Hit::StretchL(id) | Hit::StretchR(id) | Hit::Gain(id), _) => {
                        pick_clip(s, id, grab, shift, ctrl);
                        self.multi_drag.key_sel.clear();
                    }
                    (Hit::FadeLine(id, out), _) => {
                        if let Some(idx) = fade_point_at(s, &eff, id, out, t, pp, &hits) {
                            s.touch();
                            self.multi_drag.key_sel.only(
                                if out {
                                    EnvTarget::FadeOut(id)
                                } else {
                                    EnvTarget::FadeIn(id)
                                },
                                idx,
                            );
                            hist = Some("Add Fade Point");
                            s.clear_selected_clips();
                        } else {
                            pick_clip(s, id, grab, shift, ctrl);
                            self.multi_drag.key_sel.clear();
                        }
                    }
                    (Hit::None, _) => {
                        // Shift picks every empty span of the row — or drops the span
                        // clicked when it is already picked — and Ctrl toggles the span
                        // clicked; both with the black mouse tool only
                        if self.tool == Tool::Move && (shift || ctrl) {
                            if let Some(tk) = track {
                                if shift {
                                    s.select_row_gaps(tk, t);
                                } else {
                                    s.toggle_gap(tk, t);
                                }
                                self.multi_drag.key_sel.clear();
                            }
                        } else {
                            s.clear_selected_clips();
                            selection = None;
                            cursor = t;
                            self.multi_drag.key_sel.clear();
                        }
                    }
                    (Hit::EnvLine(id), _) => {
                        let body = hits.iter().find(|h| h.1 == id).map(|h| {
                            Rect::from_min_max(pos2(h.0.left(), h.0.top() + 15.0), h.0.max)
                        });
                        if let (Some(c), Some(body)) = (s.clip_mut(id), body) {
                            let rel = (t - c.start).clamp(0.0, c.len);
                            let d = env_db_at(body, pp.y) - c.gain_db;
                            let idx = curves::insert_key(&mut c.env, &mut c.curves.env_h, rel, d);
                            self.multi_drag.key_sel.only(EnvTarget::ClipVol(id), idx);
                            hist = Some("Add Envelope Point");
                        }
                        // the new keyframe is the selection, not the clip it is on
                        s.clear_selected_clips();
                    }
                    (Hit::EnvPoint(..) | Hit::PanPoint(..), _) => {
                        // the keyframe layer picks the keyframe; the clip is not selected with it
                        s.clear_selected_clips();
                    }
                    (Hit::PanLine(id), _) => {
                        let body = hits.iter().find(|h| h.1 == id).map(|h| {
                            Rect::from_min_max(pos2(h.0.left(), h.0.top() + 15.0), h.0.max)
                        });
                        if let (Some(c), Some(body)) = (s.clip_mut(id), body) {
                            let rel = (t - c.start).clamp(0.0, c.len);
                            let idx = curves::insert_key(
                                &mut c.pan_env,
                                &mut c.curves.pan_h,
                                rel,
                                pan_at(body, pp.y),
                            );
                            self.multi_drag.key_sel.only(EnvTarget::ClipPan(id), idx);
                            hist = Some("Add Pan Point");
                        }
                        s.clear_selected_clips();
                    }
                    (
                        Hit::Body(id)
                        | Hit::Header(id)
                        | Hit::LeftEdge(id)
                        | Hit::RightEdge(id)
                        | Hit::FadeIn(id)
                        | Hit::FadeOut(id),
                        _,
                    ) => {
                        pick_clip(s, id, grab, shift, ctrl);
                        // the clip clicked last is what Delete removes, not a keyframe from before
                        self.multi_drag.key_sel.clear();
                        if self.tool == Tool::TimeSel {
                            cursor = t;
                            selection = None;
                        }
                    }
                }
            }
        }
        if resp.double_clicked() {
            if let Some(pp) = resp.interact_pointer_pos() {
                let h = hit_test(self.session.as_ref().unwrap(), pp).0;
                if let Hit::PanPoint(id, idx) = h {
                    if let Some(c) = self.session.as_mut().and_then(|s| s.clip_mut(id)) {
                        curves::remove_key(&mut c.pan_env, &mut c.curves.pan_h, idx);
                    }
                    hist = Some("Remove Pan Point");
                } else if let Hit::EnvPoint(id, idx) = h {
                    if let Some(c) = self.session.as_mut().and_then(|s| s.clip_mut(id)) {
                        curves::remove_key(&mut c.env, &mut c.curves.env_h, idx);
                    }
                    hist = Some("Remove Envelope Point");
                } else if let Hit::Body(id) | Hit::Header(id) = h {
                    if let Some(s) = &mut self.session {
                        s.selected_clip = Some(id);
                    }
                    self.pending_cmd = Some("editsource");
                }
            }
        }
        self.multi_drag.geom = Some((lanes.intersect(tracks_rect), lane_rects.clone(), view));
        // drop from Files panel: the clip's start snaps to the session start, the playhead and the
        // edges of the clips already on that track when it comes within a few pixels of them
        let drop_start =
            |s: &crate::session::Session, hp: egui::Pos2, tk: usize, len: f64| -> (f64, bool) {
                let t = t_of(hp.x).max(0.0);
                if !snapping {
                    return (t, false);
                }
                let p = &self.prefs;
                let mut targets = vec![];
                if p.snap_session_start {
                    targets.push(0.0);
                }
                if p.snap_playhead {
                    targets.push(cursor);
                }
                if let Some(tr) = s.tracks.get(tk) {
                    for c in &tr.clips {
                        if p.snap_clip_starts {
                            targets.push(c.start);
                        }
                        if p.snap_clip_ends {
                            // so the dropped clip's end can land on an existing clip's start
                            targets.push(c.end());
                            targets.push(c.start - len);
                        }
                    }
                }
                let px = |a: f64| ((a - t) / span) as f32 * lanes.width();
                match targets
                    .into_iter()
                    .filter(|a| *a >= 0.0)
                    .map(|a| (a, px(a).abs()))
                    .filter(|(_, d)| *d < 12.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                {
                    Some((a, _)) => (a, true),
                    None => (t, false),
                }
            };
        if let Some(fid) = resp.dnd_hover_payload::<u64>() {
            if let Some(hp) = ui.ctx().pointer_hover_pos() {
                let tk = lane_rects
                    .iter()
                    .position(|r| hp.y >= r.top() && hp.y < r.bottom());
                if let (Some(tk), Some(f)) = (tk, self.file(*fid)) {
                    let lane = lane_rects[tk];
                    let len = f.data.duration();
                    let (t0, snapped) = drop_start(self.session.as_ref().unwrap(), hp, tk, len);
                    let x0 = x_of(t0);
                    let x1 = x0 + (len / span) as f32 * lanes.width();
                    let r = Rect::from_min_max(
                        pos2(x0, lane.top() + 2.0),
                        pos2(x1, lane.bottom() - 2.0),
                    );
                    clip_painter.rect_filled(r, 0.0, crate::theme::gray_a(255, 18));
                    clip_painter.rect_stroke(
                        r,
                        0.0,
                        Stroke::new(1.5, theme::accent()),
                        egui::StrokeKind::Inside,
                    );
                    if snapped {
                        clip_painter.line_segment(
                            [pos2(x0, tracks_rect.top()), pos2(x0, tracks_rect.bottom())],
                            Stroke::new(1.0, Color32::WHITE),
                        );
                    }
                } else if let Some(f) = self.file(*fid) {
                    // below the last track: the drop adds a new track
                    let top = lane_rects
                        .last()
                        .map(|r| r.bottom())
                        .unwrap_or(tracks_rect.top());
                    if hp.y >= top {
                        let len = f.data.duration();
                        let tk = lane_rects.len();
                        let (t0, _) = drop_start(self.session.as_ref().unwrap(), hp, tk, len);
                        let x0 = x_of(t0);
                        let x1 = x0 + (len / span) as f32 * lanes.width();
                        let r = Rect::from_min_max(pos2(x0, top + 2.0), pos2(x1, top + 94.0));
                        clip_painter.rect_filled(
                            Rect::from_min_max(
                                pos2(lanes.left(), top),
                                pos2(lanes.right(), top + 96.0),
                            ),
                            0.0,
                            crate::theme::gray_a(255, 8),
                        );
                        clip_painter.rect_filled(r, 0.0, crate::theme::gray_a(255, 18));
                        clip_painter.rect_stroke(
                            r,
                            0.0,
                            Stroke::new(1.5, theme::accent()),
                            egui::StrokeKind::Inside,
                        );
                        clip_painter.text(
                            pos2(lanes.left() + 8.0, top + 6.0),
                            egui::Align2::LEFT_TOP,
                            "New track",
                            FontId::proportional(11.0),
                            theme::text_dim(),
                        );
                    }
                }
            }
        }
        if let Some(fid) = resp.dnd_release_payload::<u64>() {
            if let Some(hp) = ui
                .ctx()
                .pointer_interact_pos()
                .or(ui.ctx().pointer_hover_pos())
            {
                let tk = lane_rects
                    .iter()
                    .position(|r| hp.y >= r.top() && hp.y < r.bottom())
                    .unwrap_or(lane_rects.len());
                let len = self.file(*fid).map(|f| f.data.duration()).unwrap_or(0.0);
                let (t, _) = drop_start(self.session.as_ref().unwrap(), hp, tk, len);
                self.place_file(*fid, tk, t);
            }
        }
        resp.context_menu(|ui| {
            let has = self
                .session
                .as_ref()
                .and_then(|s| s.selected_clip)
                .is_some();
            if ui
                .add_enabled(
                    has,
                    egui::Button::new("Split at Playhead").shortcut_text("Ctrl+K"),
                )
                .clicked()
            {
                self.pending_cmd = Some("split");
                ui.close();
            }
            if ui
                .add_enabled(has, egui::Button::new("Delete Clip").shortcut_text("Del"))
                .clicked()
            {
                self.pending_cmd = Some("delete");
                ui.close();
            }
            if ui
                .add_enabled(has, egui::Button::new("Mute / Unmute Clip"))
                .clicked()
            {
                if let Some(s) = &mut self.session {
                    if let Some(c) = s.selected_clip.and_then(|id| s.clip_mut(id)) {
                        c.muted = !c.muted;
                    }
                }
                hist = Some("Mute Clip");
                ui.close();
            }
            if ui
                .add_enabled(has, egui::Button::new("Edit Source File"))
                .clicked()
            {
                self.pending_cmd = Some("editsource");
                ui.close();
            }
            if ui
                .add_enabled(has, egui::Button::new("Edit Original"))
                .clicked()
            {
                self.pending_cmd = Some("clip.edit_original");
                ui.close();
            }
            ui.separator();
            if ui.button("Add Marker").clicked() {
                self.pending_cmd = Some("marker");
                ui.close();
            }
            if ui.button("Mixdown Session to New File").clicked() {
                self.pending_cmd = Some("mixdown");
                ui.close();
            }
        });
        // middle button drag pans the view (time and tracks)
        if ui.rect_contains_pointer(tracks_rect)
            || ui.rect_contains_pointer(ruler)
            || matches!(self.multi_drag.drag, MDrag::MidPan { .. })
        {
            let (mid, delta) = ui.input(|i| (i.pointer.middle_down(), i.pointer.delta()));
            if mid {
                if let Some(pp) = ui.ctx().pointer_latest_pos() {
                    self.multi_drag.drag = MDrag::MidPan { last: pp };
                }
                let d = -(delta.x as f64) / lanes.width().max(1.0) as f64 * span;
                let a = (view.0 + d).max(0.0);
                view = (a, a + span);
                self.multi_drag.scroll_y -= delta.y;
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if matches!(self.multi_drag.drag, MDrag::MidPan { .. }) {
                self.multi_drag.drag = MDrag::None;
            }
        }
        // wheel: scroll tracks (Ctrl zooms), resize tracks over the headers
        if ui.rect_contains_pointer(tracks_rect) {
            let (scroll, zoom_d, mods) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers));
            let hp = ui.ctx().pointer_hover_pos().unwrap_or(lanes.center());
            if mods.alt {
                self.multi_drag.scroll_y -= scroll.y;
            } else if hp.x < lanes.left() {
                // over the track headers the wheel resizes every track, like the original
                if scroll.y.abs() > 0.1 {
                    let k = (1.0025f32).powf(scroll.y);
                    let mut resized = false;
                    if let Some(s) = self.session.as_mut() {
                        for t in s.tracks.iter_mut() {
                            let h = (t.height * k).clamp(40.0, 600.0);
                            resized |= h != t.height;
                            t.height = h;
                        }
                    }
                    if resized {
                        self.session_push("Resize Tracks");
                    }
                }
            } else {
                // over the clips: the wheel scrolls the tracks up and down, Ctrl+wheel zooms
                // time around the pointer, Shift+wheel (or a sideways wheel) scrolls time
                let tc = t_of(hp.x);
                let mut factor = 1.0;
                if zoom_d != 1.0 {
                    factor = 1.0 / zoom_d as f64;
                } else if mods.command && scroll.y.abs() > 0.1 {
                    factor = (0.996f64).powf(scroll.y as f64);
                } else if scroll.y.abs() > 0.1 {
                    self.multi_drag.scroll_y -= scroll.y;
                }
                if factor != 1.0 {
                    // deep enough to see every sample (about a dozen across the view)
                    let rate = self
                        .session
                        .as_ref()
                        .map(|s| s.rate)
                        .unwrap_or(48_000)
                        .max(1) as f64;
                    let w = (span * factor).clamp(12.0 / rate, (total * 2.0).max(12.0 / rate));
                    let a = (tc - (tc - view.0) * (w / span)).max(0.0);
                    view = (a, a + w);
                }
                if scroll.x.abs() > 0.1 {
                    let d = -(scroll.x as f64) / lanes.width() as f64 * span;
                    let a = (view.0 + d).max(0.0);
                    view = (a, a + (view.1 - view.0));
                }
            }
        }

        // ---------- Master: the last row, styled like a track; it carries the session's output
        // (the Levels panel at the bottom of the window shows the same master output)
        {
            let head = Rect::from_min_max(master.min, pos2(lanes.left(), master.bottom()));
            let lane = Rect::from_min_max(pos2(lanes.left(), master.top()), master.max);
            p.rect_filled(head, 0.0, theme::track_head());
            p.rect_filled(lane, 0.0, theme::track_lane_a());
            p.rect_filled(
                Rect::from_min_max(
                    pos2(lanes.left() - 2.0, master.top()),
                    pos2(lanes.left(), master.bottom()),
                ),
                0.0,
                theme::track_divider(),
            );
            p.line_segment(
                [master.left_top(), master.right_top()],
                Stroke::new(2.0, crate::theme::gray(15)),
            );
            p.rect_filled(
                Rect::from_min_size(head.min, vec2(4.0, head.height())),
                0.0,
                Color32::from_rgb(40, 60, 110),
            );
            let meter = self.engine.lock().meter;
            let mr = Rect::from_min_max(
                pos2(head.right() - 14.0, head.top() + 6.0),
                pos2(head.right() - 4.0, head.bottom() - 6.0),
            );
            theme::vmeter(&p, mr, &meter);
            // the master header: click selects it (its effects then show in the Effects Rack)
            let mhresp = ui.interact(head, ui.scope_id().with("master_head"), Sense::click());
            let mut mui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(
                pos2(head.left() + 10.0, head.top() + 4.0),
                pos2(head.right() - 18.0, head.bottom()),
            )));
            mui.set_clip_rect(head.intersect(mui.clip_rect()));
            mui.spacing_mut().item_spacing.y = 3.0;
            let dev = self.engine.out_device_name.clone();
            let mut open_fx = false;
            let mut open_hw = false;
            let s = self.session.as_mut().unwrap();
            if s.master_selected {
                p.rect_filled(
                    Rect::from_min_max(pos2(head.left() + 4.0, head.top()), head.max),
                    0.0,
                    Color32::from_rgba_unmultiplied(80, 110, 160, 30),
                );
            }
            let mut touched = false;
            mui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                theme::paint_icon(
                    ui.painter(),
                    Rect::from_min_size(ui.cursor().min + vec2(0.0, 2.0), vec2(14.0, 14.0)),
                    Icon::Forward,
                    theme::text(),
                );
                ui.add_space(18.0);
                ui.label(egui::RichText::new("Master").strong());
                ui.add_space((ui.available_width() - 2.0 * 23.0).max(0.0));
                if letter_toggle(ui, &mut s.master.mute, "M", MUTE_ON, "Mute the Master") {
                    touched = true;
                }
                if letter_toggle(ui, &mut s.master.solo, "S", SOLO_ON, "Solo safe") {
                    touched = true;
                }
            });
            mui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                small_glyph(ui, Glyph::Volume);
                let mut v = s.master_db;
                let k = theme::knob(
                    ui,
                    &mut v,
                    -60.0,
                    15.0,
                    20.0,
                    "Master volume (drag; double-click resets)",
                );
                let hv = theme::hot_value(
                    ui,
                    &mut v,
                    -60.0,
                    15.0,
                    0.1,
                    0.0,
                    &format!("{:+.1}", s.master_db),
                    "Master volume: drag left/right, double-click for 0 dB",
                );
                if k.changed() || hv.changed() {
                    s.master_db = (v * 10.0).round() / 10.0;
                    touched = true;
                }
                let n = s.master.rack.iter().filter(|e| e.enabled).count();
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("fx").small().color(if n > 0 {
                            theme::accent()
                        } else {
                            theme::text_dim()
                        }))
                        .min_size(vec2(22.0, 18.0)),
                    )
                    .on_hover_text(format!("Master effects ({n} on): show in the Effects Rack"))
                    .clicked()
                {
                    open_fx = true;
                }
                ui.label(egui::RichText::new("\u{2190}").color(theme::text_dim()))
                    .on_hover_text("Output");
                // long device names are cut short with an ellipsis; the full name is in the tooltip
                let w = (ui.available_width() - 4.0).max(40.0);
                let shown = if s.master.output == "Default Output" {
                    dev.clone()
                } else {
                    s.master.output.clone()
                };
                egui::containers::menu::MenuButton::from_button(
                    egui::Button::new(egui::RichText::new(shown).small())
                        .truncate()
                        .min_size(vec2(w, 18.0))
                        .right_text("\u{203a}"),
                )
                .ui(ui, |ui| {
                    if ui
                        .selectable_label(true, format!("Stereo: {dev}"))
                        .clicked()
                    {
                        s.master.output = "Default Output".into();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Audio Hardware...").clicked() {
                        open_hw = true;
                        ui.close();
                    }
                })
                .0
                .on_hover_text(format!("Output device: {dev}"));
            });
            mui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                if automation_disclosure(
                    ui,
                    s.master.show_vol_env,
                    "Show or hide the Master volume automation lane",
                )
                .clicked()
                {
                    s.master.show_vol_env = !s.master.show_vol_env;
                    touched = true;
                }
                if automation_combo(ui, "master_auto", &mut s.master.automation) {
                    touched = true;
                }
            });
            if touched {
                s.touch();
            }
            if mhresp.clicked() || open_fx {
                s.master_selected = true;
            }
            if open_fx {
                self.show_panel(crate::workspace::PanelId::EffectsRack);
                self.rack_clip_tab = false;
            }
            if open_hw {
                self.dialog = Some(crate::app::Dialog::Preferences);
            }
            // master peak readout in the empty lane
            let pk = crate::audio::db(meter[0].max(meter[1]));
            let txt = if pk < -90.0 {
                "Master output: -\u{221e} dB".to_string()
            } else {
                format!("Master output: {pk:+.1} dB")
            };
            p.text(
                pos2(lane.left() + 10.0, lane.center().y),
                egui::Align2::LEFT_CENTER,
                txt,
                egui::FontId::proportional(11.0),
                theme::text_dim(),
            );
            // the Master's own chevron opens the same kind of lane for the Master fader (over the
            // Master row: its keyframes come from the keyframe layer below)
            if self.session.as_ref().unwrap().master.show_vol_env {
                let ar = Rect::from_min_max(
                    pos2(lanes.left(), master.top() + 4.0),
                    pos2(master.right(), master.bottom() - 2.0),
                );
                paint_lane_bg(&p, ar);
                let (line, read) = {
                    let m = &self.session.as_ref().unwrap().master;
                    (
                        paint_auto_env(
                            &p,
                            ar,
                            &m.vol_env,
                            &m.vol_h,
                            &x_of,
                            false,
                            true,
                            m.automation != 0,
                        ),
                        m.automation != 0,
                    )
                };
                let _ = read;
                p.text(
                    pos2(ar.left() + 6.0, ar.top() + 3.0),
                    egui::Align2::LEFT_TOP,
                    "Master Volume",
                    FontId::proportional(9.5),
                    theme::text_dim(),
                );
                key_views.push(KeyView {
                    target: EnvTarget::MasterVol,
                    area: ar,
                    clip: ar,
                    kind: VKind::Vol,
                    origin: 0.0,
                    scale: 1.0,
                    voff: 0.0,
                    tmin: 0.0,
                    tmax: f64::MAX / 4.0,
                    col: VOL_COL,
                    xl: kv_xl,
                    v0: kv_v0,
                    pps: kv_pps,
                    editable: true,
                });
                let aresp = ui.interact(
                    ar,
                    ui.scope_id().with("master_auto_lane"),
                    Sense::click_and_drag(),
                );
                let master_rate = self
                    .session
                    .as_ref()
                    .map(|s| s.rate as f64)
                    .unwrap_or(48000.0)
                    .max(1.0);
                let snap_master =
                    |x: f32| -> f64 { (t_of(x).max(0.0) * master_rate).round() / master_rate };
                let on_line = |pp: egui::Pos2| dist_to_line(&line, pp) < 6.0;
                if let Some(hp) = aresp.hover_pos() {
                    ui.ctx().set_cursor_icon(if on_line(hp) {
                        egui::CursorIcon::Crosshair
                    } else {
                        egui::CursorIcon::Default
                    });
                }
                if aresp.clicked() {
                    if let Some(pp) = aresp.interact_pointer_pos() {
                        if on_line(pp) {
                            if let Some(s) = self.session.as_mut() {
                                let i = curves::insert_key(
                                    &mut s.master.vol_env,
                                    &mut s.master.vol_h,
                                    snap_master(pp.x),
                                    auto_db_at(ar, pp.y),
                                );
                                s.master_selected = true;
                                s.touch();
                                self.multi_drag.key_sel.only(EnvTarget::MasterVol, i);
                                auto_hist = Some("Add Master Automation Keyframe");
                            }
                        } else {
                            self.multi_drag.key_sel.clear();
                        }
                    }
                }
                if aresp.drag_started() {
                    if let Some(pp) = ui
                        .input(|i| i.pointer.press_origin())
                        .or(aresp.interact_pointer_pos())
                    {
                        if on_line(pp) {
                            if let Some(s) = self.session.as_mut() {
                                let idx = curves::insert_key(
                                    &mut s.master.vol_env,
                                    &mut s.master.vol_h,
                                    snap_master(pp.x),
                                    auto_db_at(ar, pp.y),
                                );
                                s.master_selected = true;
                                s.touch();
                                self.multi_drag.key_sel.only(EnvTarget::MasterVol, idx);
                                self.multi_drag.drag = MDrag::AutoPoint {
                                    track: usize::MAX,
                                    idx,
                                };
                            }
                        }
                    }
                }
                if aresp.dragged() {
                    if let (Some(pp), MDrag::AutoPoint { track, idx }) =
                        (aresp.interact_pointer_pos(), self.multi_drag.drag)
                    {
                        if track == usize::MAX {
                            // Shift locks the point to the main 8 directions, like clips and keyframes
                            let pp = if ui.input(|i| i.modifiers.shift) {
                                ui.input(|i| i.pointer.press_origin())
                                    .map(|o| crate::keys::snap_dir8(o, pp))
                                    .unwrap_or(pp)
                            } else {
                                pp
                            };
                            if let Some(s) = self.session.as_mut() {
                                crate::workflows::move_auto_point(
                                    &mut s.master.vol_env,
                                    idx,
                                    snap_master(pp.x),
                                    auto_db_at(ar, pp.y),
                                );
                                s.touch();
                            }
                            auto_hist = Some("Edit Master Volume Automation");
                        }
                    }
                }
                if aresp.drag_stopped()
                    && matches!(self.multi_drag.drag, MDrag::AutoPoint { track, .. } if track == usize::MAX)
                {
                    self.multi_drag.drag = MDrag::None;
                }
            }
        }

        // keyframes of every envelope drawn above: hover, select, move, delete, handles
        let key_painter = p.with_clip_rect(Rect::from_min_max(
            pos2(lanes.left(), tracks_rect.top()),
            pos2(lanes.right(), master.bottom()),
        ));
        if let Some(h) = self.key_layer(ui, &key_views, &key_painter) {
            hist = Some(h);
        }

        // the take being recorded grows on every armed track as it comes in
        if let Some((at, Some(_))) = self.recording_at {
            let armed: Vec<usize> = self
                .session
                .as_ref()
                .map(|s| {
                    s.tracks
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| t.arm && t.can_record())
                        .map(|(i, _)| i)
                        .collect()
                })
                .unwrap_or_default();
            let lp = p.with_clip_rect(Rect::from_min_max(
                pos2(lanes.left(), tracks_rect.top()),
                pos2(lanes.right(), tracks_rect.bottom()),
            ));
            for ti in armed {
                if let Some(cr) = clip_rects.get(ti) {
                    self.draw_live_take(&lp, *cr, at, &x_of);
                }
            }
        }
        // playhead
        let px = x_of(cursor);
        if px >= lanes.left() && px <= lanes.right() {
            p.line_segment(
                [pos2(px, ruler.top()), pos2(px, tracks_rect.bottom())],
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
        // the snapping point: like the playhead line but brighter and in the theme's
        // negative colour, so it is obvious where a dragged edge is about to snap
        if self.snapping && resp.dragged() {
            let snap_x = match self.multi_drag.drag {
                MDrag::Move { id, grab, .. } => {
                    if let Some(pp) = resp.interact_pointer_pos() {
                        let len = self
                            .session
                            .as_ref()
                            .and_then(|s| s.find_clip(id).map(|(a, b)| s.tracks[a].clips[b].len))
                            .unwrap_or(0.0);
                        let raw = (on_grid(t_of(pp.x).max(0.0)) - grab).max(0.0);
                        let vel = ui.input(|i| i.pointer.velocity());
                        let fast = vel.length() > self.prefs.snapping_distance() * 60.0;
                        let moved = if fast || !snapping {
                            on_grid(raw)
                        } else {
                            let sn = snap(self.session.as_ref().unwrap(), raw, Some(id), len);
                            on_grid(if (sn - raw).abs() > 1e-12 { sn } else { raw })
                        };
                        Some(x_of(moved))
                    } else {
                        None
                    }
                }
                MDrag::TrimL { .. }
                | MDrag::TrimR { .. }
                | MDrag::StretchL { .. }
                | MDrag::StretchR { .. } => self.multi_drag.edge_snap.map(x_of),
                _ => None,
            };
            if let Some(x) = snap_x {
                if x >= lanes.left() && x <= lanes.right() {
                    p.line_segment(
                        [pos2(x, ruler.top()), pos2(x, tracks_rect.bottom())],
                        Stroke::new(2.0, theme::snap_line()),
                    );
                }
            }
        }
        // vertical navigator: a miniature of the tracks with the visible window; drag the window to
        // scroll, stretch its top/bottom grips (or use the wheel over it) to make the tracks taller
        // or shorter
        self.vertical_navigator(ui, &p, vnav, tracks_rect.height(), total_h);

        // write back
        let playing = self.is_playing();
        let s = self.session.as_mut().unwrap();
        s.view = view;
        s.selection = selection;
        let moved = (s.cursor - cursor).abs() > 1e-9;
        s.cursor = cursor;
        if moved && playing {
            self.set_cursor(cursor);
        }
        if let Some(h) = auto_hist.or(hist) {
            self.session_push(h);
        }
        if let Some(c) = self.pending_cmd.take() {
            let ctx = ui.ctx().clone();
            match c {
                "editsource" => self.edit_selected_clip_source(),
                "mixdown" => self.mixdown(false, false),
                other => self.run(other, &ctx),
            }
        }
    }

    pub fn mixer_view(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        let p = ui.painter_at(full);
        p.rect_filled(full, 0.0, crate::theme::gray(30));
        self.ensure_mixer_session();
        let strip_w = 104.0;
        let meters = self.engine.lock().track_meters.clone();
        let master_meter = self.engine.lock().meter;
        let n = self.session.as_ref().unwrap().tracks.len();
        let mut hist: Option<&'static str> = None;
        let mut scroll_ui = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(
            full.min,
            pos2(full.right() - strip_w - 6.0, full.bottom()),
        )));
        egui::ScrollArea::horizontal()
            .id_salt("mixer_scroll")
            .auto_shrink([false, false])
            .show(&mut scroll_ui, |ui| {
                ui.horizontal_top(|ui| {
                    for ti in 0..n {
                        let (rect, _) = ui.allocate_exact_size(
                            vec2(strip_w, (full.height() - 6.0).max(1.0)),
                            Sense::hover(),
                        );
                        let lvl = meters.get(ti).copied().unwrap_or([0.0; 2]);
                        let s = self.session.as_mut().unwrap();
                        let selected = s.selected_track == ti;
                        let tr = &mut s.tracks[ti];
                        let mut touched = false;
                        if strip(
                            ui,
                            rect,
                            &mut tr.name,
                            &mut tr.volume_db,
                            Some(&mut tr.pan),
                            Some((&mut tr.mute, &mut tr.solo, &mut tr.arm)),
                            &lvl,
                            tr.color,
                            tr.rack.len(),
                            selected,
                            &mut touched,
                            &mut hist,
                        ) {
                            s.selected_track = ti;
                            self.ws.layout.show(crate::workspace::PanelId::EffectsRack);
                        }
                        if touched {
                            if let Some(s) = self.session.as_mut() {
                                s.touch();
                            }
                        }
                    }
                });
            });
        // master strip
        let mrect = Rect::from_min_max(
            pos2(full.right() - strip_w - 4.0, full.top() + 3.0),
            pos2(full.right() - 4.0, full.bottom() - 3.0),
        );
        let s = self.session.as_mut().unwrap();
        let mut name = "Mix".to_string();
        let mut touched = false;
        strip(
            ui,
            mrect,
            &mut name,
            &mut s.master_db,
            None,
            None,
            &master_meter,
            [200, 200, 200],
            0,
            false,
            &mut touched,
            &mut hist,
        );
        if touched {
            s.touch();
        }
        if let Some(h) = hist {
            self.session_push(h);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn strip(
    ui: &mut egui::Ui,
    rect: Rect,
    name: &mut String,
    vol: &mut f32,
    pan: Option<&mut f32>,
    msr: Option<(&mut bool, &mut bool, &mut bool)>,
    meter: &[f32],
    color: [u8; 3],
    fx: usize,
    selected: bool,
    touched: &mut bool,
    hist: &mut Option<&'static str>,
) -> bool {
    let p = ui.painter().clone();
    p.rect_filled(
        rect,
        0.0,
        if selected {
            crate::theme::own_or(
                Color32::from_rgb(46, 50, 56),
                crate::theme::track_head_sel(),
            )
        } else {
            crate::theme::gray(38)
        },
    );
    p.rect_filled(
        Rect::from_min_size(rect.min, vec2(rect.width(), 4.0)),
        0.0,
        crate::theme::saturated(Color32::from_rgb(color[0], color[1], color[2])),
    );
    let mut fx_clicked = false;
    let inner = rect.shrink2(vec2(6.0, 8.0));
    let mut sui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    sui.spacing_mut().item_spacing.y = 4.0;
    if sui
        .add(
            egui::Button::new(egui::RichText::new(format!("fx  {fx}")).small())
                .min_size(vec2(inner.width(), 18.0)),
        )
        .on_hover_text("Edit effects in the Effects Rack")
        .clicked()
    {
        fx_clicked = true;
    }
    if let Some(pan) = pan {
        let mut pn = *pan * 100.0;
        let k = theme::knob(&mut sui, &mut pn, -100.0, 100.0, 30.0, "Pan");
        if k.changed() {
            *pan = pn / 100.0;
            *touched = true;
        }
        if k.drag_stopped() {
            *hist = Some("Track Pan");
        }
        let pl = if pan.abs() < 0.005 {
            "C".to_string()
        } else if *pan < 0.0 {
            format!("L{:.0}", -*pan * 100.0)
        } else {
            format!("R{:.0}", *pan * 100.0)
        };
        sui.label(egui::RichText::new(pl).small().color(theme::time_blue()));
    } else {
        sui.add_space(50.0);
    }
    if let Some((m, s, r)) = msr {
        sui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.x = 3.0;
            *touched |= letter_toggle(ui, m, "M", MUTE_ON, "Mute");
            *touched |= letter_toggle(ui, s, "S", SOLO_ON, "Solo");
            *touched |= letter_toggle(ui, r, "R", Color32::from_rgb(230, 80, 80), "Arm for Record");
        });
    } else {
        sui.add_space(22.0);
    }
    let fader_top = sui.cursor().top() + 4.0;
    let fader_bottom = inner.bottom() - 44.0;
    if fader_bottom > fader_top + 40.0 {
        let fr = Rect::from_min_max(
            pos2(inner.left() + 26.0, fader_top),
            pos2(inner.left() + 56.0, fader_bottom),
        );
        let (ch, rel) = fader(ui, fr, vol);
        if ch {
            *touched = true;
        }
        if rel {
            *hist = Some("Volume");
        }
        let mr = Rect::from_min_max(
            pos2(inner.right() - 18.0, fader_top + 8.0),
            pos2(inner.right() - 2.0, fader_bottom - 8.0),
        );
        theme::vmeter(&p, mr, meter);
    }
    p.text(
        pos2(rect.center().x, inner.bottom() - 32.0),
        egui::Align2::CENTER_CENTER,
        format!("{:+.1} dB", vol),
        FontId::monospace(11.0),
        theme::time_blue(),
    );
    let nr = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 20.0), inner.max);
    let mut nui = ui.new_child(egui::UiBuilder::new().max_rect(nr));
    if nui
        .add(
            egui::TextEdit::singleline(name)
                .desired_width(nr.width())
                .horizontal_align(egui::Align::Center),
        )
        .changed()
    {
        *touched = true;
    }
    let _ = Icon::Menu;
    fx_clicked
}
