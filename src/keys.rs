//! Keyframes in the Multitrack Editor, one way for every envelope: clip volume and pan, the
//! track automation lanes, the Master lane and the points that shape clip fades.
//!
//! Each keyframe is its own small widget on top of the timeline: it lights up under the pointer,
//! a click selects it (Shift/Ctrl adds to the selection), a drag moves it (with the other
//! selected keyframes of the same envelope; Shift locks the move to the main 8 directions),
//! a double-click or Delete removes it.
//!
//! With the Direct Selection tool (the white arrow, A) keyframes also show their Bezier handles:
//! drag a handle to bend the curve (the opposite handle turns with it), Alt+drag a handle to
//! move it alone (a corner), Alt+drag a keyframe to pull out fresh symmetric handles, Alt+click a
//! keyframe to give it handles or take them away.

use crate::app::{App, Tool};
use crate::curves::{self, Handles, Side};
use crate::session::Session;
use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};

/// Lock a drag to the main 8 directions (multiples of 45°), the way drawing apps do: the
/// pointer's travel from where the grab started (`origin`) is projected onto the nearest of
/// the eight, so a mostly-upward drag moves straight up, a mostly-rightward one straight
/// right, and a diagonal one exactly along the diagonal. Returns `p` unchanged when the
/// pointer has barely moved or the numbers went odd.
pub fn snap_dir8(origin: Pos2, p: Pos2) -> Pos2 {
    let d = p - origin;
    let len = d.length();
    if !len.is_finite() || len < 1e-3 {
        return p;
    }
    let step = std::f32::consts::FRAC_PI_2 * 0.5; // 45°
    let k = (d.y.atan2(d.x) / step).round();
    let (s, c) = (k * step).sin_cos();
    if !c.is_finite() || !s.is_finite() {
        return p;
    }
    // how far the drag reaches along the chosen direction
    let along = d.x * c + d.y * s;
    if !along.is_finite() {
        return p;
    }
    origin + vec2(c, s) * along
}

/// Which envelope a keyframe belongs to (clips and tracks by id, so it survives reordering).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnvTarget {
    ClipVol(u64),
    ClipPan(u64),
    FadeIn(u64),
    FadeOut(u64),
    TrackVol(u64),
    TrackPan(u64),
    MasterVol,
    /// Another automated track parameter: (track id, index into the track's `auto.params`).
    TrackParam(u64, u32),
}

impl EnvTarget {
    pub fn label(self) -> &'static str {
        match self {
            EnvTarget::ClipVol(_) => "Clip Volume",
            EnvTarget::ClipPan(_) => "Clip Pan",
            EnvTarget::FadeIn(_) => "Fade In",
            EnvTarget::FadeOut(_) => "Fade Out",
            EnvTarget::TrackVol(_) => "Track Volume",
            EnvTarget::TrackPan(_) => "Track Pan",
            EnvTarget::MasterVol => "Master Volume",
            EnvTarget::TrackParam(..) => "Automation",
        }
    }
    fn is_fade(self) -> bool {
        matches!(self, EnvTarget::FadeIn(_) | EnvTarget::FadeOut(_))
    }
}

/// How a value maps to a height on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VKind {
    /// dB on the envelope taper (0 dB at 65 %, +6 dB at the top).
    Vol,
    /// Pan: left (-1) up, right (+1) down.
    Pan,
    /// Fade gain 0..1, bottom to top.
    Fade,
    /// A parameter's own range, bottom to top (on/off parameters snap to 0 or 1).
    Lin { min: f32, max: f32, binary: bool },
}

/// What was chosen in a keyframe's right-click menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyMenu {
    Delete,
    SelectAll,
    Deselect,
    Interp(curves::Interp),
    /// Spline Curves for the whole envelope: on gives every keyframe smooth handles, off makes
    /// them all straight again.
    Spline(bool),
}

/// The keyframe right-click menu. `sel` keyframes are selected out of `n`; `cur` is the
/// clicked keyframe's interpolation. Items that can't do anything are greyed out.
pub fn key_menu(
    ui: &mut egui::Ui,
    cur: curves::Interp,
    sel: usize,
    n: usize,
    spline: bool,
    act: &mut Option<KeyMenu>,
) {
    use curves::Interp;
    let mut item = |ui: &mut egui::Ui, on: bool, label: &str, sc: &str, a: KeyMenu| {
        if ui
            .add_enabled(on, egui::Button::new(label).shortcut_text(sc))
            .clicked()
        {
            *act = Some(a);
            ui.close();
        }
    };
    item(
        ui,
        sel > 0,
        if sel > 1 {
            "Delete Selected Keyframes"
        } else {
            "Delete Keyframe"
        },
        "Del",
        KeyMenu::Delete,
    );
    item(
        ui,
        n > 0 && sel < n,
        "Select All Keyframes",
        "",
        KeyMenu::SelectAll,
    );
    item(ui, sel > 0, "Deselect Keyframes", "Esc", KeyMenu::Deselect);
    ui.separator();
    ui.label(
        egui::RichText::new("Interpolation")
            .small()
            .color(crate::theme::text_dim()),
    );
    for (m, label) in [
        (Interp::Linear, "Linear"),
        (Interp::Hold, "Hold"),
        (Interp::Bezier, "Bezier (smooth)"),
        (Interp::BezierBroken, "Bezier (independent handles)"),
    ] {
        if ui
            .add_enabled(sel > 0, egui::Button::selectable(cur == m, label))
            .clicked()
        {
            *act = Some(KeyMenu::Interp(m));
            ui.close();
        }
    }
    ui.separator();
    let mut sp = spline;
    if ui
        .add_enabled(n > 1, egui::Checkbox::new(&mut sp, "Spline Curves"))
        .on_hover_text("Smooth curves through every keyframe of this envelope")
        .changed()
    {
        *act = Some(KeyMenu::Spline(sp));
        ui.close();
    }
}

/// One envelope as drawn this frame: where it is and how its (time, value) map to the screen.
#[derive(Clone, Copy, Debug)]
pub struct KeyView {
    pub target: EnvTarget,
    /// The area values map into.
    pub area: Rect,
    /// Where keyframes may be seen and grabbed.
    pub clip: Rect,
    pub kind: VKind,
    /// Session time of key time 0, and session seconds per key time unit (negative: runs right
    /// to left, as fade-out points do from the clip's end).
    pub origin: f64,
    pub scale: f64,
    /// Added to values for display (a clip's own gain on its volume line).
    pub voff: f32,
    /// Key times stay within this range.
    pub tmin: f64,
    pub tmax: f64,
    pub col: Color32,
    /// Timeline mapping: screen x of session time `v0`, pixels per second.
    pub xl: f32,
    pub v0: f64,
    pub pps: f64,
    /// Keyframes can be edited (an unlocked lane, the parameter chosen in Select).
    pub editable: bool,
}

pub fn y_of(kind: VKind, area: Rect, v: f32) -> f32 {
    match kind {
        VKind::Vol => crate::ui_multi::env_y(area, v),
        VKind::Pan => crate::ui_multi::pan_y(area, v),
        VKind::Fade => area.bottom() - v.clamp(0.0, 1.0) * area.height(),
        VKind::Lin { min, max, .. } => {
            let n = ((v - min) / (max - min).max(1e-6)).clamp(0.0, 1.0);
            area.bottom() - 3.0 - n * (area.height() - 6.0).max(1.0)
        }
    }
}

pub fn v_at(kind: VKind, area: Rect, y: f32) -> f32 {
    match kind {
        VKind::Vol => crate::ui_multi::env_db_at(area, y),
        VKind::Pan => crate::ui_multi::pan_at(area, y),
        VKind::Fade => ((area.bottom() - y) / area.height().max(1.0)).clamp(0.0, 1.0),
        VKind::Lin { min, max, binary } => {
            let n = ((area.bottom() - 3.0 - y) / (area.height() - 6.0).max(1.0)).clamp(0.0, 1.0);
            let v = min + n * (max - min);
            if binary {
                if n >= 0.5 { max } else { min }
            } else {
                v
            }
        }
    }
}

/// A keyframe's value in words.
/// How far a value moved, in its own unit (the drag readout's change).
pub fn delta_text(kind: VKind, d: f32) -> String {
    match kind {
        VKind::Vol => format!("{d:+.1} dB"),
        VKind::Pan => {
            let p = (d * 100.0).round();
            if p.abs() < 0.5 {
                "0".into()
            } else if p < 0 as f32 {
                format!("{:.0} to L", -p)
            } else {
                format!("{p:.0} to R")
            }
        }
        VKind::Fade => format!("{:+.0} %", d * 100.0),
        VKind::Lin { binary: true, .. } => String::new(),
        VKind::Lin { min, max, .. } => {
            if (max - min) > 50.0 {
                format!("{d:+.0}")
            } else if (max - min) > 5.0 {
                format!("{d:+.1}")
            } else {
                format!("{d:+.2}")
            }
        }
    }
}

pub fn value_text(kind: VKind, v: f32) -> String {
    match kind {
        VKind::Vol => format!("{v:+.1} dB"),
        VKind::Pan => {
            let pv = (v * 100.0).round();
            if pv.abs() < 0.5 {
                "Center".into()
            } else if pv < 0.0 {
                format!("L{:.0}", -pv)
            } else {
                format!("R{pv:.0}")
            }
        }
        VKind::Fade => format!("{:.0} %", v * 100.0),
        VKind::Lin { binary: true, .. } => {
            if v >= 0.5 {
                "On".into()
            } else {
                "Off".into()
            }
        }
        VKind::Lin { min, max, .. } => {
            if (max - min) > 50.0 {
                format!("{v:.0}")
            } else if (max - min) > 5.0 {
                format!("{v:.1}")
            } else {
                format!("{v:.2}")
            }
        }
    }
}

impl KeyView {
    pub fn x(&self, t: f64) -> f32 {
        self.xl + ((self.origin + t * self.scale - self.v0) * self.pps) as f32
    }
    pub fn pos(&self, t: f64, v: f32) -> Pos2 {
        pos2(self.x(t), y_of(self.kind, self.area, v + self.voff))
    }
    /// Key time and value at a screen point (unclamped time).
    pub fn inv(&self, p: Pos2) -> (f64, f32) {
        let st = (p.x - self.xl) as f64 / self.pps.max(1e-12) + self.v0;
        let t = (st - self.origin)
            / if self.scale.abs() < 1e-12 {
                1.0
            } else {
                self.scale
            };
        (t, v_at(self.kind, self.area, p.y) - self.voff)
    }
    /// Raw value (no rounding) at a screen y, for handle offsets.
    fn v_raw(&self, y: f32) -> f32 {
        match self.kind {
            VKind::Vol => {
                let n = ((self.area.bottom() - 3.0 - y) / (self.area.height() - 6.0).max(1.0))
                    .clamp(-0.5, 1.5);
                crate::sesx::env_key_db(n.max(0.0) as f64) - self.voff
            }
            _ => v_at(self.kind, self.area, y) - self.voff,
        }
    }
}

/// The envelope (keyframes and their handles) a target names.
pub fn env_of(s: &Session, t: EnvTarget) -> Option<(&Vec<(f64, f32)>, &Vec<Handles>)> {
    match t {
        EnvTarget::ClipVol(id) => s
            .find_clip(id)
            .map(|(a, b)| &s.tracks[a].clips[b])
            .map(|c| (&c.env, &c.curves.env_h)),
        EnvTarget::ClipPan(id) => s
            .find_clip(id)
            .map(|(a, b)| &s.tracks[a].clips[b])
            .map(|c| (&c.pan_env, &c.curves.pan_h)),
        EnvTarget::FadeIn(id) => s
            .find_clip(id)
            .map(|(a, b)| &s.tracks[a].clips[b])
            .map(|c| (&c.curves.fin_pts, &c.curves.fin_h)),
        EnvTarget::FadeOut(id) => s
            .find_clip(id)
            .map(|(a, b)| &s.tracks[a].clips[b])
            .map(|c| (&c.curves.fout_pts, &c.curves.fout_h)),
        EnvTarget::TrackVol(id) => s
            .tracks
            .iter()
            .find(|t| t.id == id)
            .map(|t| (&t.vol_env, &t.auto.vol_h)),
        EnvTarget::TrackPan(id) => s
            .tracks
            .iter()
            .find(|t| t.id == id)
            .map(|t| (&t.auto.pan_env, &t.auto.pan_h)),
        EnvTarget::MasterVol => Some((&s.master.vol_env, &s.master.vol_h)),
        EnvTarget::TrackParam(id, i) => s
            .tracks
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.auto.params.get(i as usize))
            .map(|p| (&p.env, &p.hs)),
    }
}

pub fn env_mut(s: &mut Session, t: EnvTarget) -> Option<(&mut Vec<(f64, f32)>, &mut Vec<Handles>)> {
    match t {
        EnvTarget::ClipVol(id) => s.clip_mut(id).map(|c| (&mut c.env, &mut c.curves.env_h)),
        EnvTarget::ClipPan(id) => s
            .clip_mut(id)
            .map(|c| (&mut c.pan_env, &mut c.curves.pan_h)),
        EnvTarget::FadeIn(id) => s
            .clip_mut(id)
            .map(|c| (&mut c.curves.fin_pts, &mut c.curves.fin_h)),
        EnvTarget::FadeOut(id) => s
            .clip_mut(id)
            .map(|c| (&mut c.curves.fout_pts, &mut c.curves.fout_h)),
        EnvTarget::TrackVol(id) => s
            .tracks
            .iter_mut()
            .find(|t| t.id == id)
            .map(|t| (&mut t.vol_env, &mut t.auto.vol_h)),
        EnvTarget::TrackPan(id) => s
            .tracks
            .iter_mut()
            .find(|t| t.id == id)
            .map(|t| (&mut t.auto.pan_env, &mut t.auto.pan_h)),
        EnvTarget::MasterVol => Some((&mut s.master.vol_env, &mut s.master.vol_h)),
        EnvTarget::TrackParam(id, i) => s
            .tracks
            .iter_mut()
            .find(|t| t.id == id)
            .and_then(|t| t.auto.params.get_mut(i as usize))
            .map(|p| (&mut p.env, &mut p.hs)),
    }
}

/// Selected keyframes: all on one envelope.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeySel {
    pub target: Option<EnvTarget>,
    pub idx: Vec<usize>,
}

impl KeySel {
    pub fn has(&self, t: EnvTarget, i: usize) -> bool {
        self.target == Some(t) && self.idx.contains(&i)
    }
    pub fn is_empty(&self) -> bool {
        self.target.is_none() || self.idx.is_empty()
    }
    pub fn clear(&mut self) {
        self.target = None;
        self.idx.clear();
    }
    pub fn only(&mut self, t: EnvTarget, i: usize) {
        self.target = Some(t);
        self.idx = vec![i];
    }
    pub fn toggle(&mut self, t: EnvTarget, i: usize) {
        if self.target != Some(t) {
            self.only(t, i);
        } else if let Some(p) = self.idx.iter().position(|j| *j == i) {
            self.idx.remove(p);
        } else {
            self.idx.push(i);
        }
    }
}

/// A drag in progress on a keyframe or a handle.
#[derive(Clone, Debug)]
pub enum KeyDrag {
    /// Moving keyframes: the grabbed one, the pointer's offset from it, and where each selected
    /// keyframe started (index, time, value).
    Keys {
        target: EnvTarget,
        grab: usize,
        start: Vec<(usize, f64, f32)>,
        origin: Pos2,
    },
    /// Alt+drag on a keyframe: pull out symmetric handles.
    Pull { target: EnvTarget, idx: usize },
    /// Dragging one handle; `alone` breaks the pair.
    Handle {
        target: EnvTarget,
        idx: usize,
        side: Side,
        alone: bool,
    },
}

/// Draw one keyframe: a small square (a diamond when it has handles), brighter and larger under
/// the pointer, filled with the selection colour when selected.
pub fn paint_key(
    p: &egui::Painter,
    c: Pos2,
    col: Color32,
    size: f32,
    hovered: bool,
    selected: bool,
    curved: bool,
    sel_col: Color32,
    editable: bool,
) {
    let s = size;
    let fill = if selected {
        sel_col
    } else if editable {
        col
    } else {
        col.gamma_multiply(0.5)
    };
    let edge = if hovered {
        Stroke::new(1.5, Color32::WHITE)
    } else if selected {
        Stroke::new(1.0, Color32::WHITE)
    } else {
        Stroke::new(1.0, Color32::from_black_alpha(200))
    };
    if curved {
        let h = s * 0.75;
        let pts = vec![
            pos2(c.x, c.y - h),
            pos2(c.x + h, c.y),
            pos2(c.x, c.y + h),
            pos2(c.x - h, c.y),
        ];
        p.add(egui::Shape::convex_polygon(pts, fill, edge));
    } else {
        let r = Rect::from_center_size(c, vec2(s, s));
        p.rect_filled(r, 0.0, fill);
        p.rect_stroke(r, 0.0, edge, egui::StrokeKind::Outside);
    }
}

impl App {
    /// Keyframes of the visible envelopes: hover, select, drag, delete and (Direct Selection
    /// tool) Bezier handles. Returns the History label of an edit made this frame.
    pub fn key_layer(
        &mut self,
        ui: &mut egui::Ui,
        views: &[KeyView],
        painter: &egui::Painter,
    ) -> Option<&'static str> {
        let direct = self.tool == Tool::Direct;
        let size = self.prefs.key_size();
        let hover_size = size * self.prefs.keyframe_hover_scale.clamp(1.0, 3.0);
        let hit = self.prefs.hit_radius();
        let hsize = self.prefs.handle_px();
        let hline = self.prefs.handle_line.clamp(0.5, 4.0);
        let sel_col = self.prefs.selected_color();
        let alt = ui.input(|i| i.modifiers.alt);
        let add_mod = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        let mut hist: Option<&'static str> = None;
        // what to draw at the end: (pos, col, hovered, selected, curved, editable, clip)
        let mut draw: Vec<(Pos2, Color32, bool, bool, bool, bool, Rect)> = vec![];
        let mut handle_draw: Vec<(Pos2, Pos2, Color32, bool, Rect)> = vec![];
        let mut any_key_hovered = false;
        self.multi_drag.key_spots.clear();
        self.multi_drag.handle_spots.clear();
        for v in views {
            let Some((env, hs)) = self
                .session
                .as_ref()
                .and_then(|s| env_of(s, v.target))
                .map(|(e, h)| (e.clone(), h.clone()))
            else {
                continue;
            };
            let n = env.len();
            for (i, &(t, val)) in env.iter().enumerate() {
                let c = v.pos(t, val);
                if !v.clip.expand(1.0).contains(c) {
                    continue;
                }
                let h = curves::h_at(&hs, n, i);
                self.multi_drag.key_spots.push((v.target, i, c));
                let selected = self.multi_drag.key_sel.has(v.target, i);
                // handles first, so a handle over its own keyframe can still be grabbed
                if direct && h.on && (selected || self.multi_drag.key_sel.target == Some(v.target))
                {
                    for side in [Side::In, Side::Out] {
                        let (dt, dv) = match side {
                            Side::In => (h.in_dt, h.in_dv),
                            Side::Out => (h.out_dt, h.out_dv),
                        };
                        let hp = v.pos(t + dt, val + dv);
                        self.multi_drag
                            .handle_spots
                            .push((v.target, i, side == Side::In, hp));
                        let hr = Rect::from_center_size(hp, vec2(hit * 1.6, hit * 1.6));
                        let id = ui.scope_id().with(("keyh", v.target, i, side == Side::In));
                        let r = if v.editable {
                            ui.interact(hr, id, Sense::click_and_drag())
                        } else {
                            ui.interact(hr, id, Sense::hover())
                        };
                        let hov = r.hovered() || r.dragged();
                        if hov {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                            any_key_hovered = true;
                        }
                        handle_draw.push((c, hp, v.col, hov, v.clip));
                        if !v.editable {
                            continue;
                        }
                        if r.drag_started() {
                            self.multi_drag.key_drag = Some(KeyDrag::Handle {
                                target: v.target,
                                idx: i,
                                side,
                                alone: alt || h.broken,
                            });
                            self.multi_drag.key_sel.only(v.target, i);
                        }
                        if r.dragged() {
                            if let (
                                Some(pp),
                                Some(KeyDrag::Handle {
                                    target,
                                    idx,
                                    side,
                                    alone,
                                }),
                            ) = (r.interact_pointer_pos(), self.multi_drag.key_drag.clone())
                            {
                                if target == v.target && idx == i {
                                    // Ctrl held: the handle follows at a fifth of the speed
                                    let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
                                    let real_pp = pp;
                                    let pp = self.fine_pos(pp, ctrl);
                                    // Shift keeps the handle on one of the main 8 directions
                                    // through its own keyframe (45° steps, like a drawing app)
                                    let pp = if ui.input(|i| i.modifiers.shift) {
                                        snap_dir8(c, pp)
                                    } else {
                                        pp
                                    };
                                    self.move_handle(v, idx, side, alone, pp);
                                    self.handle_readout(v, idx, side == Side::In, real_pp);
                                }
                            }
                        }
                        if r.drag_stopped() {
                            if matches!(self.multi_drag.key_drag, Some(KeyDrag::Handle { target, idx, .. }) if target == v.target && idx == i)
                            {
                                hist = Some("Edit Keyframe Handle");
                            }
                            self.multi_drag.key_drag = None;
                        }
                    }
                }
                let kr = Rect::from_center_size(c, vec2(hit * 2.0, hit * 2.0));
                let id = ui.scope_id().with(("key", v.target, i));
                let r = if v.editable {
                    ui.interact(kr, id, Sense::click_and_drag())
                } else {
                    ui.interact(kr, id, Sense::hover())
                };
                let hov = r.hovered() || r.dragged();
                if hov {
                    any_key_hovered = true;
                    ui.ctx().set_cursor_icon(if r.dragged() {
                        egui::CursorIcon::Grabbing
                    } else {
                        egui::CursorIcon::PointingHand
                    });
                    let unit = value_text(v.kind, val + v.voff);
                    let tip = if direct {
                        format!(
                            "{} keyframe: {unit}\nAlt+click: handles on/off \u{2022} Alt+drag: pull new handles \u{2022} right-click: menu",
                            v.target.label()
                        )
                    } else {
                        format!(
                            "{} keyframe: {unit}\nClick to select, drag to move (Shift locks it to the 8 main directions), double-click or Delete to remove, right-click for interpolation",
                            v.target.label()
                        )
                    };
                    r.clone().on_hover_text(tip);
                }
                draw.push((c, v.col, hov, selected, h.on || h.hold, v.editable, v.clip));
                if !v.editable {
                    continue;
                }
                // right-click: the keyframe menu (delete, select, interpolation, spline curves)
                if r.secondary_clicked() && !self.multi_drag.key_sel.has(v.target, i) {
                    self.multi_drag.key_sel.only(v.target, i);
                    self.select_owner(v.target);
                }
                let sel_n = if self.multi_drag.key_sel.target == Some(v.target) {
                    self.multi_drag.key_sel.idx.len()
                } else {
                    0
                };
                let spline = n > 1 && (0..n).all(|k| curves::h_at(&hs, n, k).on);
                let mut act: Option<KeyMenu> = None;
                r.context_menu(|ui| key_menu(ui, h.interp(), sel_n, n, spline, &mut act));
                if let Some(a) = act {
                    if let Some(l) = self.apply_key_menu(v.target, a) {
                        hist = Some(l);
                    }
                }
                if r.double_clicked() {
                    if let Some(s) = self.session.as_mut() {
                        if let Some((e, h)) = env_mut(s, v.target) {
                            curves::remove_key(e, h, i);
                        }
                        s.touch();
                    }
                    self.multi_drag.key_sel.clear();
                    self.multi_drag.key_drag = None;
                    hist = Some("Remove Keyframe");
                    continue;
                }
                if r.clicked() {
                    if direct && alt {
                        // Alt+click: a corner gets handles, a curve point loses them
                        if let Some(s) = self.session.as_mut() {
                            if let Some((e, h)) = env_mut(s, v.target) {
                                curves::fit(h, e.len());
                                if let Some(k) = h.get_mut(i) {
                                    *k = if k.on {
                                        Handles::default()
                                    } else {
                                        curves::auto_handles(e, i)
                                    };
                                }
                            }
                            s.touch();
                        }
                        self.multi_drag.key_sel.only(v.target, i);
                        hist = Some("Toggle Keyframe Handles");
                    } else if add_mod {
                        self.multi_drag.key_sel.toggle(v.target, i);
                    } else {
                        self.multi_drag.key_sel.only(v.target, i);
                    }
                    self.select_owner(v.target);
                }
                if r.drag_started() {
                    let origin = ui.input(|inp| inp.pointer.press_origin()).unwrap_or(c);
                    if direct && alt {
                        self.multi_drag.key_drag = Some(KeyDrag::Pull {
                            target: v.target,
                            idx: i,
                        });
                        self.multi_drag.key_sel.only(v.target, i);
                    } else {
                        if !self.multi_drag.key_sel.has(v.target, i) {
                            if add_mod {
                                self.multi_drag.key_sel.toggle(v.target, i);
                            } else {
                                self.multi_drag.key_sel.only(v.target, i);
                            }
                        }
                        let start: Vec<(usize, f64, f32)> = self
                            .multi_drag
                            .key_sel
                            .idx
                            .iter()
                            .filter_map(|j| env.get(*j).map(|(tt, vv)| (*j, *tt, *vv)))
                            .collect();
                        self.multi_drag.key_drag = Some(KeyDrag::Keys {
                            target: v.target,
                            grab: i,
                            start,
                            origin,
                        });
                    }
                    self.select_owner(v.target);
                }
                if r.dragged() {
                    if let Some(pp0) = r.interact_pointer_pos() {
                        // Ctrl held: the keyframes follow at a fifth of the mouse's speed
                        let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
                        let real_pp = pp0;
                        let pp0 = self.fine_pos(pp0, ctrl);
                        match self.multi_drag.key_drag.clone() {
                            Some(KeyDrag::Keys {
                                target,
                                grab,
                                start,
                                origin,
                            }) if target == v.target && grab == i => {
                                // Shift locks the move to the main 8 directions from where the
                                // grab started, so a mostly-upward drag moves the keyframe
                                // straight up and a mostly-rightward one straight right
                                let pp = if ui.input(|i| i.modifiers.shift) {
                                    snap_dir8(origin, pp0)
                                } else {
                                    pp0
                                };
                                self.move_keys(v, &start, grab, origin, pp);
                                // the floating readout: where the grabbed keyframe is now and
                                // how far it moved (value and time)
                                let rate = self.rate();
                                let fmt = self.time_format;
                                let now = self
                                    .session
                                    .as_ref()
                                    .and_then(|s| env_of(s, v.target))
                                    .and_then(|(e, _)| e.get(grab).copied());
                                let was = start.iter().find(|x| x.0 == grab).map(|x| (x.1, x.2));
                                if let (Some((t1, v1)), Some((t0, v0))) = (now, was) {
                                    let d = delta_text(v.kind, v1 - v0);
                                    let value = value_text(v.kind, v1 + v.voff);
                                    let head = if d.is_empty() {
                                        value
                                    } else {
                                        format!("{value}   ({d})")
                                    };
                                    let at = v.origin + t1 * v.scale;
                                    let dt = (t1 - t0) * v.scale;
                                    self.multi_drag.readout = Some((
                                        real_pp,
                                        format!("{head}\n{}   ({dt:+.3} s)", fmt.format(at.max(0.0), rate)),
                                    ));
                                }
                            }
                            Some(KeyDrag::Pull { target, idx })
                                if target == v.target && idx == i =>
                            {
                                // Shift keeps the pulled handle on one of the main 8 directions
                                let pp = if ui.input(|i| i.modifiers.shift) {
                                    snap_dir8(c, pp0)
                                } else {
                                    pp0
                                };
                                self.pull_handles(v, idx, pp);
                                self.handle_readout(v, idx, false, real_pp);
                            }
                            _ => {}
                        }
                    }
                    ui.ctx().request_repaint();
                }
                if r.drag_stopped() {
                    match self.multi_drag.key_drag {
                        Some(KeyDrag::Keys { target, grab, .. })
                            if target == v.target && grab == i =>
                        {
                            hist = Some("Move Keyframe")
                        }
                        Some(KeyDrag::Pull { target, idx }) if target == v.target && idx == i => {
                            hist = Some("Edit Keyframe Handles")
                        }
                        _ => {}
                    }
                    self.multi_drag.key_drag = None;
                }
            }
        }
        // the handles' lines under their keyframes, then the keyframes
        for (a, b, col, hov, clip) in handle_draw {
            let hp = painter.with_clip_rect(clip.intersect(painter.clip_rect()));
            hp.line_segment([a, b], Stroke::new(hline, col.gamma_multiply(0.85)));
            let fill = if hov {
                Color32::WHITE
            } else {
                crate::theme::gray(20)
            };
            hp.circle(
                b,
                hsize * 0.5 * if hov { 1.4 } else { 1.0 },
                fill,
                Stroke::new(1.0, col),
            );
        }
        for (c, col, hov, sel, curved, editable, clip) in draw {
            let kp = painter.with_clip_rect(clip.expand(hover_size).intersect(painter.clip_rect()));
            paint_key(
                &kp,
                c,
                col,
                if hov { hover_size } else { size },
                hov,
                sel,
                curved,
                sel_col,
                editable,
            );
        }
        self.multi_drag.key_hovered = any_key_hovered;
        // keyframes whose envelope went away (a clip deleted, a lane closed) are not selected
        if let Some(t) = self.multi_drag.key_sel.target {
            let n = self
                .session
                .as_ref()
                .and_then(|s| env_of(s, t))
                .map(|(e, _)| e.len());
            match n {
                None => self.multi_drag.key_sel.clear(),
                Some(n) => self.multi_drag.key_sel.idx.retain(|i| *i < n),
            }
        }
        hist
    }

    /// A keyframe click makes the track (or Master) it belongs to the current one. Clips are
    /// deselected: a keyframe and a clip are never selected together, so Delete and the other
    /// edits always act on the one thing that looks selected.
    fn select_owner(&mut self, t: EnvTarget) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        match t {
            EnvTarget::ClipVol(id)
            | EnvTarget::ClipPan(id)
            | EnvTarget::FadeIn(id)
            | EnvTarget::FadeOut(id) => {
                if let Some((ti, _)) = s.find_clip(id) {
                    s.selected_track = ti;
                    s.clear_selected_clips();
                    s.master_selected = false;
                }
            }
            EnvTarget::TrackVol(id) | EnvTarget::TrackPan(id) | EnvTarget::TrackParam(id, _) => {
                if let Some(ti) = s.tracks.iter().position(|t| t.id == id) {
                    s.selected_track = ti;
                    s.master_selected = false;
                }
            }
            EnvTarget::MasterVol => s.master_selected = true,
        }
    }

    /// Move the selected keyframes by the pointer's travel, each kept between its neighbours.
    fn move_keys(
        &mut self,
        v: &KeyView,
        start: &[(usize, f64, f32)],
        grab: usize,
        origin: Pos2,
        pp: Pos2,
    ) {
        let Some(&(_, gt, gv)) = start.iter().find(|s| s.0 == grab) else {
            return;
        };
        let g0 = v.pos(gt, gv);
        // the grabbed keyframe follows the pointer (keeping where it was grabbed)
        let target_pos = g0 + (pp - origin);
        let (nt, nv) = v.inv(target_pos);
        let dt = nt - gt;
        let dv = nv - gv;
        let rate = self
            .session
            .as_ref()
            .map(|s| s.rate as f64)
            .unwrap_or(48000.0)
            .max(1.0);
        let fade = v.target.is_fade();
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let Some((env, _)) = env_mut(s, v.target) else {
            return;
        };
        let moving: Vec<usize> = start.iter().map(|s| s.0).collect();
        for &(j, t0, v0) in start {
            if j >= env.len() {
                continue;
            }
            // neighbours that are not moving bound the move
            let lo = (0..j)
                .rev()
                .find(|k| !moving.contains(k))
                .map(|k| env[k].0 + 1e-4)
                .unwrap_or(v.tmin);
            let hi = (j + 1..env.len())
                .find(|k| !moving.contains(k))
                .map(|k| env[k].0 - 1e-4)
                .unwrap_or(v.tmax);
            let mut t = t0 + dt;
            if !fade {
                t = (t * rate).round() / rate;
            }
            let t = if hi > lo { t.clamp(lo, hi) } else { lo };
            let val = if moving.len() == 1 { nv } else { v0 + dv };
            let val = match v.kind {
                VKind::Vol => ((val * 10.0).round() / 10.0).clamp(-96.0, 6.1),
                VKind::Pan => val.clamp(-1.0, 1.0),
                VKind::Fade => val.clamp(0.0, 1.0),
                VKind::Lin { min, max, binary } => {
                    if binary {
                        if val >= (min + max) * 0.5 { max } else { min }
                    } else {
                        val.clamp(min, max.max(min))
                    }
                }
            };
            env[j] = (t, val);
        }
        // keep the order (the clamps above already do, this guards odd input)
        env.sort_by(|a, b| a.0.total_cmp(&b.0));
        s.touch();
    }

    /// Alt+drag on a keyframe: fresh smooth handles, the out handle at the pointer and the in
    /// handle mirrored through the keyframe.
    /// The floating readout while a curve handle is dragged: the keyframe's value and how much
    /// the handle rises over how long.
    fn handle_readout(&mut self, v: &KeyView, idx: usize, inside: bool, pp: Pos2) {
        let Some(s) = self.session.as_ref() else {
            return;
        };
        let Some((e, hs)) = env_of(s, v.target) else {
            return;
        };
        let Some(&(_, val)) = e.get(idx) else {
            return;
        };
        let h = curves::h_at(hs, e.len(), idx);
        let (dt, dv) = if inside {
            (h.in_dt, h.in_dv)
        } else {
            (h.out_dt, h.out_dv)
        };
        let rise = delta_text(v.kind, dv);
        self.multi_drag.readout = Some((
            pp,
            format!(
                "{}\nhandle {}   over {:.3} s",
                value_text(v.kind, val + v.voff),
                if rise.is_empty() { "-".into() } else { rise },
                (dt * v.scale).abs()
            ),
        ));
    }

    fn pull_handles(&mut self, v: &KeyView, idx: usize, pp: Pos2) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let Some((env, hs)) = env_mut(s, v.target) else {
            return;
        };
        if idx >= env.len() {
            return;
        }
        curves::fit(hs, env.len());
        let (t, val) = env[idx];
        let c = v.pos(t, val);
        let mut d = pp - c;
        // pulling right of the keyframe drags the out handle (time runs right on screen,
        // except for fade-out points, which run from the clip's end leftwards)
        let flip = v.scale < 0.0;
        if (d.x < 0.0) != flip {
            d = -d;
        }
        let out_p = c + d;
        let in_p = c - d;
        let (ot, _) = v.inv(out_p);
        let (it, _) = v.inv(in_p);
        let ov = v.v_raw(out_p.y);
        let iv = v.v_raw(in_p.y);
        hs[idx] = Handles {
            on: true,
            broken: false,
            out_dt: (ot - t).max(0.0),
            out_dv: ov - val,
            in_dt: (it - t).min(0.0),
            in_dv: iv - val,
            hold: false,
        };
        s.touch();
    }

    /// Drag one handle. Unless broken, the opposite handle turns to stay in line (keeping its
    /// own length on screen).
    fn move_handle(&mut self, v: &KeyView, idx: usize, side: Side, alone: bool, pp: Pos2) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let Some((env, hs)) = env_mut(s, v.target) else {
            return;
        };
        if idx >= env.len() {
            return;
        }
        curves::fit(hs, env.len());
        let (t, val) = env[idx];
        let c = v.pos(t, val);
        let h = hs[idx];
        let (ht, _) = v.inv(pp);
        let hv = v.v_raw(pp.y);
        let mut nh = h;
        nh.on = true;
        let (dt, dv) = (ht - t, hv - val);
        match side {
            Side::Out => {
                nh.out_dt = dt.max(0.0);
                nh.out_dv = dv;
            }
            Side::In => {
                nh.in_dt = dt.min(0.0);
                nh.in_dv = dv;
            }
        }
        if alone {
            nh.broken = true;
        } else {
            // the other handle: same line, its own length
            let other = match side {
                Side::Out => v.pos(t + h.in_dt, val + h.in_dv),
                Side::In => v.pos(t + h.out_dt, val + h.out_dv),
            };
            let len = (other - c).length().max(6.0);
            let dir = (pp - c).normalized();
            if dir.is_finite() && dir.length() > 0.5 {
                let op = c - dir * len;
                let (ot, _) = v.inv(op);
                let ov = v.v_raw(op.y);
                match side {
                    Side::Out => {
                        nh.in_dt = (ot - t).min(0.0);
                        nh.in_dv = ov - val;
                    }
                    Side::In => {
                        nh.out_dt = (ot - t).max(0.0);
                        nh.out_dv = ov - val;
                    }
                }
            }
        }
        hs[idx] = nh;
        s.touch();
    }

    /// Do what was chosen in a keyframe's right-click menu. Returns the History label.
    pub fn apply_key_menu(&mut self, target: EnvTarget, a: KeyMenu) -> Option<&'static str> {
        match a {
            KeyMenu::Delete => {
                self.delete_selected_keys();
                None
            }
            KeyMenu::Deselect => {
                self.multi_drag.key_sel.clear();
                None
            }
            KeyMenu::SelectAll => {
                let n = self
                    .session
                    .as_ref()
                    .and_then(|s| env_of(s, target))
                    .map(|(e, _)| e.len())
                    .unwrap_or(0);
                self.multi_drag.key_sel.target = Some(target);
                self.multi_drag.key_sel.idx = (0..n).collect();
                None
            }
            KeyMenu::Interp(m) => {
                let idx = if self.multi_drag.key_sel.target == Some(target) {
                    self.multi_drag.key_sel.idx.clone()
                } else {
                    vec![]
                };
                let s = self.session.as_mut()?;
                let (e, h) = env_mut(s, target)?;
                for i in idx {
                    curves::set_interp(e, h, i, m);
                }
                s.touch();
                Some("Keyframe Interpolation")
            }
            KeyMenu::Spline(on) => {
                let s = self.session.as_mut()?;
                let (e, h) = env_mut(s, target)?;
                for i in 0..e.len() {
                    curves::set_interp(
                        e,
                        h,
                        i,
                        if on {
                            curves::Interp::Bezier
                        } else {
                            curves::Interp::Linear
                        },
                    );
                }
                s.touch();
                Some(if on {
                    "Spline Curves"
                } else {
                    "Straight Lines"
                })
            }
        }
    }

    /// Delete the selected keyframes (Delete / Backspace in the Multitrack Editor).
    pub fn delete_selected_keys(&mut self) -> bool {
        let sel = std::mem::take(&mut self.multi_drag.key_sel);
        let Some(t) = sel.target else { return false };
        if sel.idx.is_empty() {
            return false;
        }
        let Some(s) = self.session.as_mut() else {
            return false;
        };
        let Some((e, h)) = env_mut(s, t) else {
            return false;
        };
        let mut idx = sel.idx.clone();
        idx.sort_unstable();
        idx.dedup();
        for i in idx.into_iter().rev() {
            curves::remove_key(e, h, i);
        }
        s.touch();
        self.session_push("Remove Keyframes");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_map_points_back_and_forth() {
        let area = Rect::from_min_max(pos2(0.0, 100.0), pos2(400.0, 200.0));
        let v = KeyView {
            target: EnvTarget::MasterVol,
            area,
            clip: area,
            kind: VKind::Pan,
            origin: 2.0,
            scale: 1.0,
            voff: 0.0,
            tmin: 0.0,
            tmax: 10.0,
            col: Color32::WHITE,
            xl: 0.0,
            v0: 0.0,
            pps: 40.0,
            editable: true,
        };
        let p = v.pos(1.5, -0.5);
        assert!((p.x - 140.0).abs() < 1e-3);
        let (t, val) = v.inv(p);
        assert!((t - 1.5).abs() < 1e-6 && (val + 0.5).abs() < 1e-3);
        // fade-out points run leftwards from the clip end
        let f = KeyView {
            kind: VKind::Fade,
            origin: 5.0,
            scale: -2.0,
            ..v
        };
        let q = f.pos(0.25, 0.5);
        assert!((q.x - 180.0).abs() < 1e-3);
        let (t, g) = f.inv(q);
        assert!((t - 0.25).abs() < 1e-6 && (g - 0.5).abs() < 1e-3);
    }

    #[test]
    fn selection_toggles_and_switches_envelopes() {
        let mut s = KeySel::default();
        s.only(EnvTarget::MasterVol, 2);
        s.toggle(EnvTarget::MasterVol, 3);
        assert!(s.has(EnvTarget::MasterVol, 2) && s.has(EnvTarget::MasterVol, 3));
        s.toggle(EnvTarget::MasterVol, 2);
        assert!(!s.has(EnvTarget::MasterVol, 2));
        s.toggle(EnvTarget::ClipVol(1), 0);
        assert_eq!(s.target, Some(EnvTarget::ClipVol(1)));
        assert_eq!(s.idx, vec![0]);
    }
}
