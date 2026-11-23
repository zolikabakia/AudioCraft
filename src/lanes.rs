//! The open automation lane under a track: its header (Show Envelopes, Select, the keyframe
//! tools) and which envelope it edits. Besides Volume and Pan a lane can show and automate Mute,
//! the effects rack's Input, Output, Mix and Power, the Track EQ's settings and every setting of
//! every effect in the track's rack (each effect has its own submenu, like the original).

use crate::curves::{self, Handles, Interp};
use crate::dsp::Effect;
use crate::keys::{EnvTarget, VKind};
use crate::session::{AutoParam, Track};
use crate::theme;
use egui::{Color32, Rect, Sense, Stroke, pos2, vec2};

/// Envelope colours of the extra parameters (volume is yellow, pan blue).
pub fn param_color(i: usize) -> Color32 {
    const C: [Color32; 6] = [
        Color32::from_rgb(120, 220, 130),
        Color32::from_rgb(230, 130, 230),
        Color32::from_rgb(110, 215, 230),
        Color32::from_rgb(245, 150, 80),
        Color32::from_rgb(200, 200, 255),
        Color32::from_rgb(255, 120, 120),
    ];
    C[i % C.len()]
}

pub fn kind_of(p: &AutoParam) -> VKind {
    VKind::Lin {
        min: p.min,
        max: p.max.max(p.min + 1e-3),
        binary: p.binary(),
    }
}

/// The value a parameter has without automation (the fader, switch or effect setting).
pub fn static_value(tr: &Track, key: &str) -> Option<f32> {
    match key {
        "mute" => Some(if tr.mute { 1.0 } else { 0.0 }),
        "rack.in" => Some(tr.fx.in_db),
        "rack.out" => Some(tr.fx.out_db),
        "rack.mix" => Some(tr.fx.mix),
        "rack.power" => Some(if tr.fx.power { 1.0 } else { 0.0 }),
        k => {
            if let Some(path) = k.strip_prefix("eq.") {
                let e = Effect::ParametricEq {
                    bands: tr.eq.clone(),
                    master_db: tr.eq_master,
                };
                return e
                    .auto_params()
                    .into_iter()
                    .find(|p| p.0 == path)
                    .map(|p| p.2);
            }
            let rest = k.strip_prefix("fx.")?;
            let (slot, path) = rest.split_once('.')?;
            let s = tr.rack.get(slot.parse::<usize>().ok()?)?;
            s.effect
                .auto_params()
                .into_iter()
                .find(|p| p.0 == path)
                .map(|p| p.2)
        }
    }
}

/// The parameter's value at time `t` (its automation, or its fixed value).
pub fn param_now(tr: &Track, p: &AutoParam, t: f64) -> f32 {
    if !p.env.is_empty() {
        return curves::eval(&p.env, &p.hs, t, p.min, false);
    }
    static_value(tr, &p.key).unwrap_or(p.min)
}

/// The envelope the lane edits (Select): its keyframe target and value scale.
pub fn edited(tr: &Track) -> (EnvTarget, VKind) {
    match tr.auto.param {
        1 => (EnvTarget::TrackPan(tr.id), VKind::Pan),
        2 => match tr.auto.params.iter().position(|p| p.key == tr.auto.sel) {
            Some(i) => (
                EnvTarget::TrackParam(tr.id, i as u32),
                kind_of(&tr.auto.params[i]),
            ),
            None => (EnvTarget::TrackVol(tr.id), VKind::Vol),
        },
        _ => (EnvTarget::TrackVol(tr.id), VKind::Vol),
    }
}

fn edited_env(tr: &Track) -> (&Vec<(f64, f32)>, &Vec<Handles>) {
    match edited(tr).0 {
        EnvTarget::TrackPan(_) => (&tr.auto.pan_env, &tr.auto.pan_h),
        EnvTarget::TrackParam(_, i) => {
            let p = &tr.auto.params[i as usize];
            (&p.env, &p.hs)
        }
        _ => (&tr.vol_env, &tr.auto.vol_h),
    }
}

fn edited_env_mut(tr: &mut Track) -> (&mut Vec<(f64, f32)>, &mut Vec<Handles>) {
    match edited(tr).0 {
        EnvTarget::TrackPan(_) => (&mut tr.auto.pan_env, &mut tr.auto.pan_h),
        EnvTarget::TrackParam(_, i) => {
            let p = &mut tr.auto.params[i as usize];
            (&mut p.env, &mut p.hs)
        }
        _ => (&mut tr.vol_env, &mut tr.auto.vol_h),
    }
}

/// The edited envelope's value at `t` when there are no keyframes yet (the current setting).
pub fn edited_now(tr: &Track, t: f64) -> f32 {
    match edited(tr).0 {
        EnvTarget::TrackPan(_) => {
            if tr.auto.pan_env.is_empty() {
                tr.pan
            } else {
                curves::eval(&tr.auto.pan_env, &tr.auto.pan_h, t, 0.0, false)
            }
        }
        EnvTarget::TrackParam(_, i) => param_now(tr, &tr.auto.params[i as usize], t),
        _ => curves::eval(&tr.vol_env, &tr.auto.vol_h, t, 0.0, false),
    }
}

/// Add a keyframe to the edited envelope (on/off parameters hold their value). Returns its index.
pub fn add_key(tr: &mut Track, t: f64, v: f32) -> usize {
    let binary = matches!(edited(tr).1, VKind::Lin { binary: true, .. });
    let (e, h) = edited_env_mut(tr);
    let i = curves::insert_key(e, h, t, v);
    if binary {
        curves::set_interp(e, h, i, Interp::Hold);
    }
    i
}

/// One Show Envelopes entry for an extra parameter: tick to show (and edit) it.
fn param_item(
    ui: &mut egui::Ui,
    tr: &mut Track,
    key: &str,
    label: &str,
    fx_name: &str,
    min: f32,
    max: f32,
) -> bool {
    let cur = tr.auto.params.iter().find(|p| p.key == key);
    let shown = cur.map(|p| p.shown).unwrap_or(false);
    let automated = cur.map(|p| !p.env.is_empty()).unwrap_or(false);
    let mut v = shown;
    let text = if automated {
        format!("{label}  \u{25c6}")
    } else {
        label.to_string()
    };
    let r = ui.checkbox(&mut v, text);
    let r = if automated {
        r.on_hover_text("Has automation keyframes")
    } else {
        r
    };
    if r.changed() {
        let i = tr.auto.param_mut(key, label, fx_name, min, max);
        tr.auto.params[i].shown = v;
        if v {
            tr.auto.param = 2;
            tr.auto.sel = key.to_string();
        } else if tr.auto.param == 2 && tr.auto.sel == key {
            tr.auto.param = 0;
            tr.auto.show_vol = true;
        }
        return true;
    }
    false
}

/// The Show Envelopes menu: Volume, Mute, Pan, Track EQ, the rack's Input / Output / Mix /
/// Power, then one submenu per effect in the track's rack with its settings.
pub fn show_envelopes_menu(ui: &mut egui::Ui, tr: &mut Track) -> bool {
    let mut touched = false;
    let toggle_main =
        |ui: &mut egui::Ui, on: &mut bool, label: &str| ui.checkbox(on, label).changed();
    if toggle_main(ui, &mut tr.auto.show_vol, "Volume") {
        if !tr.auto.show_vol && tr.auto.param == 0 {
            tr.auto.param = if tr.auto.show_pan { 1 } else { 0 };
        } else if tr.auto.show_vol {
            tr.auto.param = 0;
        }
        touched = true;
    }
    touched |= param_item(ui, tr, "mute", "Mute", "", 0.0, 1.0);
    if toggle_main(ui, &mut tr.auto.show_pan, "Pan") {
        if tr.auto.show_pan {
            tr.auto.param = 1;
        } else if tr.auto.param == 1 {
            tr.auto.param = 0;
        }
        touched = true;
    }
    let eq = Effect::ParametricEq {
        bands: tr.eq.clone(),
        master_db: tr.eq_master,
    };
    ui.menu_button("Track EQ", |ui| {
        if !tr.eq_on {
            ui.label(
                egui::RichText::new("The Track EQ is off: its automation plays once it is on.")
                    .small()
                    .color(theme::text_dim()),
            );
        }
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .show(ui, |ui| {
                for (path, label, _, lo, hi) in eq.auto_params() {
                    touched |= param_item(
                        ui,
                        tr,
                        &format!("eq.{path}"),
                        &format!("Track EQ: {label}"),
                        "Parametric Equalizer",
                        lo,
                        hi,
                    );
                }
            });
    });
    ui.separator();
    touched |= param_item(ui, tr, "rack.in", "Rack Input", "", -48.0, 24.0);
    touched |= param_item(ui, tr, "rack.out", "Rack Output", "", -48.0, 24.0);
    touched |= param_item(ui, tr, "rack.mix", "Rack Mix", "", 0.0, 1.0);
    touched |= param_item(ui, tr, "rack.power", "Rack Power", "", 0.0, 1.0);
    let effects: Vec<(usize, String, Vec<(String, String, f32, f32, f32)>, bool)> = tr
        .rack
        .iter()
        .enumerate()
        .map(|(i, s)| {
            (
                i,
                s.effect.name().to_string(),
                s.effect.auto_params(),
                s.effect.glides_when_automated(),
            )
        })
        .collect();
    if !effects.is_empty() {
        ui.separator();
    }
    for (slot, name, params, glides) in effects {
        ui.menu_button(&name, |ui| {
            if !glides && !params.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "\u{26a0} This effect restarts on every automated change: its sound (tails, smoothing) resets while the value moves. Fine for steps, not for slow sweeps.",
                    )
                    .small()
                    .color(egui::Color32::from_rgb(232, 176, 70)),
                );
                ui.separator();
            }
            if params.is_empty() {
                ui.label(
                    egui::RichText::new("No settings to automate")
                        .small()
                        .color(theme::text_dim()),
                );
            }
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    for (path, label, _, lo, hi) in params {
                        touched |= param_item(
                            ui,
                            tr,
                            &format!("fx.{slot}.{path}"),
                            &format!("{name}: {label}"),
                            &name,
                            lo,
                            hi,
                        );
                    }
                });
        });
    }
    touched
}

/// Select: which shown envelope the lane edits.
pub fn select_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    tr: &mut Track,
) -> bool {
    let mut touched = false;
    let cur = match tr.auto.param {
        1 => "Pan".to_string(),
        2 => tr
            .auto
            .params
            .iter()
            .find(|p| p.key == tr.auto.sel)
            .map(|p| p.label.clone())
            .unwrap_or_else(|| "Volume".into()),
        _ => "Volume".to_string(),
    };
    egui::ComboBox::from_id_salt(id)
        .width(110.0)
        .selected_text(egui::RichText::new(cur).small())
        .truncate()
        .show_ui(ui, |ui| {
            if ui.selectable_label(tr.auto.param == 0, "Volume").clicked() {
                tr.auto.param = 0;
                tr.auto.show_vol = true;
                touched = true;
            }
            if ui.selectable_label(tr.auto.param == 1, "Pan").clicked() {
                tr.auto.param = 1;
                tr.auto.show_pan = true;
                touched = true;
            }
            let shown: Vec<(String, String)> = tr
                .auto
                .params
                .iter()
                .filter(|p| p.shown)
                .map(|p| (p.key.clone(), p.label.clone()))
                .collect();
            if !shown.is_empty() {
                ui.separator();
            }
            for (k, l) in shown {
                if ui
                    .selectable_label(tr.auto.param == 2 && tr.auto.sel == k, &l)
                    .clicked()
                {
                    tr.auto.param = 2;
                    tr.auto.sel = k;
                    touched = true;
                }
            }
        });
    touched
}

/// The small tools at the bottom of an open automation lane's header.
#[derive(Clone, Copy, PartialEq)]
pub enum LaneGlyph {
    Lock,
    Eraser,
    Prev,
    Add,
    Next,
}

/// A lane tool button; greyed out (and not clickable) when it can't do anything.
pub fn lane_tool(
    ui: &mut egui::Ui,
    g: LaneGlyph,
    on: bool,
    enabled: bool,
    tip: &str,
) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(
        vec2(16.0, 16.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let col = if !enabled {
        crate::theme::gray(70)
    } else if on {
        theme::accent()
    } else if resp.hovered() {
        Color32::WHITE
    } else {
        theme::text_dim()
    };
    let p = ui.painter();
    let c = r.center();
    let st = Stroke::new(1.3, col);
    match g {
        LaneGlyph::Lock => {
            let body = Rect::from_center_size(c + vec2(0.0, 3.0), vec2(10.0, 7.0));
            p.rect_stroke(body, 0.0, st, egui::StrokeKind::Middle);
            let pts: Vec<egui::Pos2> = (0..=12)
                .map(|k| {
                    let a = std::f32::consts::PI * k as f32 / 12.0;
                    pos2(
                        c.x - 3.0 * a.cos() + if on { 0.0 } else { 2.0 },
                        c.y - 1.0 + if on { 0.0 } else { -2.0 } - 4.0 * a.sin(),
                    )
                })
                .collect();
            p.add(egui::Shape::line(pts, st));
            p.circle_filled(body.center(), 1.0, col);
        }
        LaneGlyph::Eraser => {
            let pts = vec![
                pos2(c.x - 6.0, c.y + 2.0),
                pos2(c.x - 1.0, c.y - 4.0),
                pos2(c.x + 6.0, c.y + 1.0),
                pos2(c.x + 2.0, c.y + 6.0),
                pos2(c.x - 3.0, c.y + 6.0),
            ];
            p.add(egui::Shape::closed_line(pts, st));
            p.line_segment([pos2(c.x - 3.5, c.y - 1.0), pos2(c.x + 3.5, c.y + 4.0)], st);
        }
        LaneGlyph::Prev => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(c.x + 3.0, c.y - 5.0),
                    pos2(c.x + 3.0, c.y + 5.0),
                    pos2(c.x - 4.0, c.y),
                ],
                col,
                Stroke::NONE,
            ));
        }
        LaneGlyph::Next => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(c.x - 3.0, c.y - 5.0),
                    pos2(c.x + 4.0, c.y),
                    pos2(c.x - 3.0, c.y + 5.0),
                ],
                col,
                Stroke::NONE,
            ));
        }
        LaneGlyph::Add => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(c.x, c.y - 4.5),
                    pos2(c.x + 4.5, c.y),
                    pos2(c.x, c.y + 4.5),
                    pos2(c.x - 4.5, c.y),
                ],
                col,
                Stroke::NONE,
            ));
        }
    }
    resp.on_hover_text(tip)
}

/// The lane tools' states this frame: which keyframe Prev/Next jump to and whether the others
/// can act. Tools that can't act are greyed out (unpressable): Clear without keyframes,
/// Previous/Next without a keyframe that way, Add with a keyframe at the playhead or while the
/// lane is protected.
#[derive(Clone, Copy, Debug, Default)]
pub struct LaneToolStates {
    /// A keyframe sits at the playhead.
    pub on_key: bool,
    /// The edited envelope has keyframes.
    pub has_keys: bool,
    /// A keyframe before the playhead; Prev can jump there.
    pub prev: Option<f64>,
    /// A keyframe after the playhead; Next can jump there.
    pub next: Option<f64>,
    /// The lane is protected: the editing tools are greyed out.
    pub locked: bool,
}

impl LaneToolStates {
    /// Add can act: nothing at the playhead and the lane is not protected.
    pub fn add_enabled(&self) -> bool {
        !self.locked && !self.on_key
    }
    /// Clear can act: there is something to clear and the lane is not protected.
    pub fn clear_enabled(&self) -> bool {
        self.has_keys && !self.locked
    }
}

/// The lane tools' states, as a pure read of the track, so tests can check the greyed ones.
pub fn lane_tool_states(tr: &Track, cursor: &f64) -> LaneToolStates {
    let times: Vec<f64> = edited_env(tr).0.iter().map(|k| k.0).collect();
    LaneToolStates {
        on_key: times.iter().any(|t| (*t - *cursor).abs() < 1e-6),
        has_keys: !times.is_empty(),
        prev: times
            .iter()
            .copied()
            .filter(|t| *t < *cursor - 1e-6)
            .fold(None, |a: Option<f64>, t| Some(a.map_or(t, |a| a.max(t)))),
        next: times
            .iter()
            .copied()
            .filter(|t| *t > *cursor + 1e-6)
            .fold(None, |a: Option<f64>, t| Some(a.map_or(t, |a| a.min(t)))),
        locked: tr.auto.lock,
    }
}

/// The lane's tools: protect, clear, previous / add / next keyframe. Tools that can't act are
/// greyed out: Clear without keyframes, Previous/Next without a keyframe that way, editing tools
/// while the lane is protected. Returns (changed, History label).
pub fn lane_tools(
    ui: &mut egui::Ui,
    tr: &mut Track,
    cursor: &mut f64,
) -> (bool, Option<&'static str>) {
    let mut touched = false;
    let mut hist = None;
    let st = lane_tool_states(tr, cursor);
    let tip_lock = if st.locked {
        "Keyframes are protected (click to allow editing)"
    } else {
        "Protect this lane's keyframes from edits"
    };
    if lane_tool(ui, LaneGlyph::Lock, tr.auto.lock, true, tip_lock).clicked() {
        tr.auto.lock = !st.locked;
        touched = true;
    }
    if lane_tool(
        ui,
        LaneGlyph::Eraser,
        false,
        st.clear_enabled(),
        "Clear all keyframes of the selected parameter",
    )
    .clicked()
    {
        let (e, h) = edited_env_mut(tr);
        e.clear();
        h.clear();
        touched = true;
        hist = Some("Clear Automation Keyframes");
    }
    ui.add_space(4.0);
    if lane_tool(
        ui,
        LaneGlyph::Prev,
        false,
        st.prev.is_some(),
        "Previous keyframe",
    )
    .clicked()
    {
        if let Some(t) = st.prev {
            *cursor = t;
        }
    }
    let tip_add = if st.on_key {
        "There is a keyframe at the playhead"
    } else {
        "Add a keyframe at the playhead"
    };
    if lane_tool(ui, LaneGlyph::Add, st.on_key, st.add_enabled(), tip_add).clicked() {
        let v = edited_now(tr, *cursor);
        add_key(tr, *cursor, v);
        touched = true;
        hist = Some("Add Automation Keyframe");
    }
    if lane_tool(
        ui,
        LaneGlyph::Next,
        false,
        st.next.is_some(),
        "Next keyframe",
    )
    .clicked()
    {
        if let Some(t) = st.next {
            *cursor = t;
        }
    }
    (touched, hist)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;

    #[test]
    fn extra_parameters_are_edited_and_hold_when_binary() {
        let mut s = Session::new("t", 48000, 1);
        let tr = &mut s.tracks[0];
        let i = tr.auto.param_mut("mute", "Mute", "", 0.0, 1.0);
        tr.auto.params[i].shown = true;
        tr.auto.param = 2;
        tr.auto.sel = "mute".into();
        assert!(matches!(edited(tr).0, EnvTarget::TrackParam(_, 0)));
        add_key(tr, 1.0, 1.0);
        add_key(tr, 2.0, 0.0);
        assert!(
            tr.auto.params[0].hs[0].hold,
            "mute keyframes hold their value"
        );
        assert_eq!(edited_now(tr, 1.5), 1.0);
        // a missing selection falls back to Volume
        tr.auto.sel = "nope".into();
        assert!(matches!(edited(tr).0, EnvTarget::TrackVol(_)));
    }

    #[test]
    fn lane_tools_are_greyed_when_keyframes_are_present_and_absent() {
        let mut s = Session::new("t", 48000, 1);
        let tr = &mut s.tracks[0];
        // No keyframes anywhere: the greyed tools cannot act.
        let st = lane_tool_states(tr, &0.0);
        assert!(!st.on_key && !st.has_keys && st.prev.is_none() && st.next.is_none());
        assert!(st.add_enabled() && !st.clear_enabled());
        // Keyframes at 0 and 2: at the playhead Add is greyed ("one is present");
        // away from them the neighbours and Clear are enabled.
        let (e, h) = edited_env_mut(tr);
        curves::insert_key(e, h, 0.0, 0.0);
        curves::insert_key(e, h, 2.0, 1.0);
        let st = lane_tool_states(tr, &0.0);
        assert!(st.on_key && st.has_keys && st.clear_enabled() && !st.add_enabled());
        let st = lane_tool_states(tr, &1.0);
        assert!(!st.on_key && st.prev.is_some() && st.next.is_some() && st.add_enabled());
        let st = lane_tool_states(tr, &2.5);
        assert!(st.prev.is_some() && st.next.is_none());
        // A protected lane: every editing tool is greyed.
        tr.auto.lock = true;
        let st = lane_tool_states(tr, &1.0);
        assert!(!st.add_enabled() && !st.clear_enabled());
    }

    #[test]
    fn shown_effect_params_are_edited_and_automatable() {
        use crate::dsp::{Effect, EffectSlot};
        let mut s = Session::new("t", 48000, 1);
        let tr = &mut s.tracks[0];
        tr.rack
            .push(EffectSlot::new(Effect::Amplify { gain_db: 6.0 }));
        // The effect's setting is ticked in Show Envelopes: it becomes the edited envelope.
        let i = tr
            .auto
            .param_mut("fx.0.gain_db", "Amplify: Gain (dB)", "Amplify", -40.0, 24.0);
        tr.auto.params[i].shown = true;
        tr.auto.param = 2;
        tr.auto.sel = "fx.0.gain_db".into();
        assert!(matches!(edited(tr).0, EnvTarget::TrackParam(id, k) if id == tr.id && k == 0));
        // The static setting when nothing is automated.
        assert!((param_now(tr, &tr.auto.params[0], 0.0) - 6.0).abs() < 1e-4);
        // With keyframes the automated value is used (the playback carries them; see tests_auto).
        tr.auto.params[0].env = vec![(0.0, 0.0), (1.0, -20.0)];
        tr.auto.params[0].hs = vec![Handles::default(), Handles::default()];
        assert!((param_now(tr, &tr.auto.params[0], 1.0) + 20.0).abs() < 1e-3);
    }
}
