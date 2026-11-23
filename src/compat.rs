//! Audition compatibility of `.sesx` saves.
//!
//! AudioCraft keeps a few things a `.sesx` file has no place for: Bezier handles on keyframes and
//! fades shaped by points. Before such a session is saved as `.sesx`, a window lists them and
//! offers to convert them: curves become ordinary straight-line keyframes that follow them, and
//! a shaped fade becomes part of the clip's volume envelope (so the clip sounds the same in
//! Audition). "Preview Conversion" applies the conversion to the open session as an undoable step
//! so it can be checked first. The window can be turned off (Preferences > Multitrack).

use crate::app::App;
use crate::curves;
use crate::session::{Session, effective_fades, fade_curve};
use egui::{Color32, RichText};
use std::path::PathBuf;

/// A `.sesx` save waiting for the person's choice.
pub struct Pending {
    pub path: PathBuf,
    pub files: Vec<String>,
    /// The session as it will be written (file ids already mapped to `files`).
    pub copy: Session,
    pub issues: Vec<String>,
    /// true: convert; false: save as is (curves and shaped fades are left out).
    pub convert: bool,
}

/// What in `s` a `.sesx` file cannot hold, in words.
pub fn issues(s: &Session) -> Vec<String> {
    let mut out = vec![];
    for t in &s.tracks {
        for c in &t.clips {
            let k = &c.curves;
            let n = k.env_h.iter().filter(|h| h.on || h.hold).count();
            if n > 0 && curves::has_curves(c.env.len(), &k.env_h) {
                out.push(format!(
                    "{} \u{203a} {}: {n} curved or held volume keyframe{}",
                    t.name,
                    c.name,
                    if n == 1 { "" } else { "s" }
                ));
            }
            let n = k.pan_h.iter().filter(|h| h.on || h.hold).count();
            if n > 0 && curves::has_curves(c.pan_env.len(), &k.pan_h) {
                out.push(format!(
                    "{} \u{203a} {}: {n} curved or held pan keyframe{}",
                    t.name,
                    c.name,
                    if n == 1 { "" } else { "s" }
                ));
            }
            if !k.fin_pts.is_empty() {
                out.push(format!(
                    "{} \u{203a} {}: fade-in shaped by {} point{}",
                    t.name,
                    c.name,
                    k.fin_pts.len(),
                    if k.fin_pts.len() == 1 { "" } else { "s" }
                ));
            }
            if !k.fout_pts.is_empty() {
                out.push(format!(
                    "{} \u{203a} {}: fade-out shaped by {} point{}",
                    t.name,
                    c.name,
                    k.fout_pts.len(),
                    if k.fout_pts.len() == 1 { "" } else { "s" }
                ));
            }
        }
        if curves::has_curves(t.vol_env.len(), &t.auto.vol_h) {
            out.push(format!("{}: curved volume automation", t.name));
        }
        if curves::has_curves(t.auto.pan_env.len(), &t.auto.pan_h) {
            out.push(format!("{}: curved pan automation", t.name));
        }
        let extra: Vec<&str> = t
            .auto
            .params
            .iter()
            .filter(|p| !p.env.is_empty())
            .map(|p| p.label.as_str())
            .collect();
        if !extra.is_empty() {
            out.push(format!(
                "{}: automation of {} (kept in AudioCraft sessions only)",
                t.name,
                extra.join(", ")
            ));
        }
    }
    if curves::has_curves(s.master.vol_env.len(), &s.master.vol_h) {
        out.push("Master: curved volume automation".into());
    }
    out
}

fn db_of(g: f32) -> f32 {
    if g <= 1e-5 {
        -96.0
    } else {
        (20.0 * g.log10()).max(-96.0)
    }
}

/// Everything turned into what a `.sesx` holds: curves into straight-line keyframes within
/// `tol_db`, shaped fades into the clip's volume envelope. Returns the number of keyframes the
/// conversion wrote.
pub fn convert(s: &mut Session, tol_db: f32) -> usize {
    let tol = tol_db.clamp(0.01, 3.0);
    let mut written = 0;
    for t in s.tracks.iter_mut() {
        let eff = effective_fades(t);
        for (c, (efin, efout, xin, xout)) in t.clips.iter_mut().zip(eff) {
            c.env = curves::bake(&c.env, &c.curves.env_h, false, tol);
            c.pan_env = curves::bake(&c.pan_env, &c.curves.pan_h, false, tol / 100.0);
            let shaped_in = !c.curves.fin_pts.is_empty() && efin > 0.0;
            let shaped_out = !c.curves.fout_pts.is_empty() && efout > 0.0;
            if shaped_in || shaped_out {
                // sample the clip's whole gain (envelope x shaped fades) densely where it bends,
                // then keep just enough keyframes to follow it
                let env = c.env.clone();
                let len = c.len.max(1e-6);
                let mut dense: Vec<(f64, f32)> = vec![];
                let push_range = |a: f64, b: f64, n: usize, dense: &mut Vec<(f64, f32)>| {
                    for k in 0..=n {
                        dense.push((a + (b - a) * k as f64 / n as f64, 0.0));
                    }
                };
                if shaped_in {
                    push_range(0.0, efin, 96, &mut dense);
                }
                for (et, _) in &env {
                    dense.push((*et, 0.0));
                }
                if shaped_out {
                    push_range(len - efout, len, 96, &mut dense);
                }
                dense.push((0.0, 0.0));
                dense.push((len, 0.0));
                dense.sort_by(|a, b| a.0.total_cmp(&b.0));
                dense.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-7);
                let fin = (c.curves.fin_pts.clone(), c.curves.fin_h.clone());
                let fout = (c.curves.fout_pts.clone(), c.curves.fout_h.clone());
                for (tt, v) in dense.iter_mut() {
                    let mut g = crate::session::env_db(&env, *tt);
                    if shaped_in && *tt < efin {
                        g += db_of(curves::fade_gain(&fin.0, &fin.1, *tt / efin, &|u| {
                            fade_curve(u, xin)
                        }));
                    }
                    if shaped_out && len - *tt < efout {
                        g += db_of(curves::fade_gain(
                            &fout.0,
                            &fout.1,
                            (len - *tt) / efout,
                            &|u| fade_curve(u, xout),
                        ));
                    }
                    *v = g.max(-96.0);
                }
                c.env = curves::bake(&dense, &[], false, 0.0).into_iter().collect();
                c.env = simplify_db(&c.env, tol);
                if shaped_in {
                    c.fade_in = 0.0;
                    c.auto_in = false;
                }
                if shaped_out {
                    c.fade_out = 0.0;
                    c.auto_out = false;
                }
            }
            written += c.env.len() + c.pan_env.len();
            c.curves = Default::default();
            c.curves.env_h = vec![Default::default(); c.env.len()];
            c.curves.pan_h = vec![Default::default(); c.pan_env.len()];
        }
        t.vol_env = curves::bake(&t.vol_env, &t.auto.vol_h, false, tol);
        t.auto.vol_h = vec![Default::default(); t.vol_env.len()];
        t.auto.pan_env = curves::bake(&t.auto.pan_env, &t.auto.pan_h, false, tol / 100.0);
        t.auto.pan_h = vec![Default::default(); t.auto.pan_env.len()];
        for p in t.auto.params.iter_mut() {
            let ptol = ((p.max - p.min).abs() * 0.002).max(1e-4);
            p.env = curves::bake(&p.env, &p.hs, false, ptol);
            p.hs = vec![Default::default(); p.env.len()];
        }
    }
    s.master.vol_env = curves::bake(&s.master.vol_env, &s.master.vol_h, false, tol);
    s.master.vol_h = vec![Default::default(); s.master.vol_env.len()];
    written
}

/// Straight-line simplification of dense dB keyframes (silence counts as -96 dB).
fn simplify_db(p: &[(f64, f32)], tol: f32) -> Vec<(f64, f32)> {
    // the same Ramer-Douglas-Peucker simplification the curve baker uses
    crate::curves::simplify(p, tol)
}

impl App {
    /// The compatibility window for a waiting `.sesx` save.
    pub fn compat_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut pend) = self.sesx_pending.take() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let mut preview = false;
        let mut save_native = false;
        let mut warn = self.prefs.warn_sesx_compat;
        egui::Window::new("Audition Compatibility")
            .id(egui::Id::new("sesx_compat"))
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .min_width(460.0)
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("\u{26a0}").size(26.0).color(Color32::from_rgb(250, 200, 40)));
                    ui.vertical(|ui| {
                        ui.label(RichText::new("This session uses AudioCraft features a .sesx file cannot store:").strong());
                        ui.label(RichText::new(pend.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).small().color(crate::theme::text_dim()));
                    });
                });
                ui.add_space(4.0);
                egui::Frame::new().fill(crate::theme::gray(29)).inner_margin(6).corner_radius(crate::theme::ROUND).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(140.0).auto_shrink([false, true]).show(ui, |ui| {
                        for i in &pend.issues {
                            ui.label(format!("\u{2022} {i}"));
                        }
                    });
                });
                ui.add_space(6.0);
                ui.radio_value(&mut pend.convert, true, "Convert for Audition (recommended)");
                ui.label(RichText::new("    Curves become ordinary keyframes that follow them; shaped fades go into the clip's\n    volume envelope. Audition plays it the same; the open session keeps its curves.").small().color(crate::theme::text_dim()));
                ui.radio_value(&mut pend.convert, false, "Save without them");
                ui.label(RichText::new("    Audition shows straight lines between the keyframes and plain fades.").small().color(crate::theme::text_dim()));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if crate::theme::dialog_button(ui, "Preview Conversion", false).on_hover_text("Apply the conversion to the open session (one Undo takes it back) and look before saving").clicked() {
                        preview = true;
                    }
                    if crate::theme::dialog_button(ui, "Save as AudioCraft Session...", false).on_hover_text("Keep everything: .acsession holds curves and shaped fades").clicked() {
                        save_native = true;
                    }
                });
                ui.add_space(4.0);
                ui.checkbox(&mut warn, "Show this message when saving a .sesx file");
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::theme::dialog_button(ui, "Save", true).clicked() {
                            save = true;
                        }
                        if crate::theme::dialog_button(ui, "Cancel", false).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        if warn != self.prefs.warn_sesx_compat {
            self.prefs.warn_sesx_compat = warn;
            crate::prefs::save(&self.prefs);
        }
        if preview {
            if let Some(s) = self.session.as_mut() {
                let n = convert(s, self.prefs.sesx_bake_tolerance_db);
                s.touch();
                self.status = format!(
                    "Converted for Audition ({n} keyframes). Check it, then save again; Undo takes it back."
                );
            }
            self.session_push("Convert for Audition");
            return;
        }
        if save_native {
            let p = pend.path.with_extension("acsession");
            if let Some(s) = self.session.as_mut() {
                s.path = Some(p);
            }
            self.save_session(false);
            return;
        }
        if cancel {
            return;
        }
        if save {
            if pend.convert {
                convert(&mut pend.copy, self.prefs.sesx_bake_tolerance_db);
            }
            self.finish_sesx_save(&pend.path, &pend.copy, &pend.files);
            return;
        }
        self.sesx_pending = Some(pend);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::Handles;
    use crate::session::Clip;

    fn clip() -> Clip {
        Clip {
            id: 1,
            file_id: 0,
            name: "c".into(),
            start: 0.0,
            offset: 0.0,
            len: 4.0,
            gain_db: 0.0,
            muted: false,
            fade_in: 1.0,
            fade_out: 0.0,
            env: vec![(0.0, 0.0), (4.0, -12.0)],
            pan_env: vec![],
            rack: vec![],
            speed: 1.0,
            fx: Default::default(),
            auto_in: true,
            auto_out: true,
            curves: Default::default(),
        }
    }

    #[test]
    fn plain_sessions_have_nothing_to_convert() {
        let mut s = Session::new("s", 48000, 1);
        s.tracks[0].clips.push(clip());
        assert!(issues(&s).is_empty());
    }

    #[test]
    fn conversion_keeps_the_sound_and_leaves_no_curves() {
        let mut s = Session::new("s", 48000, 1);
        let mut c = clip();
        c.curves.env_h = vec![Handles::smooth(1.0, 0.0), Handles::smooth(1.0, 0.0)];
        c.curves.fin_pts = vec![(0.5, 0.9)];
        s.tracks[0].clips.push(c.clone());
        assert_eq!(issues(&s).len(), 2);
        // the gain before conversion, at a few places
        let before: Vec<f32> = [0.25, 0.5, 0.9, 2.0, 3.5]
            .iter()
            .map(|t| {
                let f = if *t < 1.0 {
                    db_of(curves::fade_gain(&c.curves.fin_pts, &[], *t, &|u| {
                        fade_curve(u, false)
                    }))
                } else {
                    0.0
                };
                curves::eval(&c.env, &c.curves.env_h, *t, 0.0, false) + f
            })
            .collect();
        convert(&mut s, 0.1);
        assert!(issues(&s).is_empty());
        let cc = &s.tracks[0].clips[0];
        assert_eq!(cc.fade_in, 0.0);
        for (t, b) in [0.25, 0.5, 0.9, 2.0, 3.5].iter().zip(before) {
            let a = crate::session::env_db(&cc.env, *t);
            assert!((a - b).abs() < 0.3, "{t}: {a} vs {b}");
        }
    }
}
