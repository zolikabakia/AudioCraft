//! More of the original's menu structure: the five clipboards (Edit > Set Current Clipboard),
//! the Metronome submenu (Multitrack), and Clip > Clip Gain / Bring to Front / fade removal.
//! Each is a registered command so it also works from the Command Palette and shortcuts.

use crate::app::App;
use crate::audio::AudioData;
use crate::commands::Cmd;

pub const MENU_COMMANDS: &[Cmd] = &[
    Cmd {
        id: "edit.clipboard_1",
        title: "Set Current Clipboard 1",
        shortcut: "",
        doc: "Use clipboard 1 for Cut, Copy and Paste (each of the five clipboards keeps its own audio).",
    },
    Cmd {
        id: "edit.clipboard_2",
        title: "Set Current Clipboard 2",
        shortcut: "",
        doc: "Use clipboard 2 for Cut, Copy and Paste.",
    },
    Cmd {
        id: "edit.clipboard_3",
        title: "Set Current Clipboard 3",
        shortcut: "",
        doc: "Use clipboard 3 for Cut, Copy and Paste.",
    },
    Cmd {
        id: "edit.clipboard_4",
        title: "Set Current Clipboard 4",
        shortcut: "",
        doc: "Use clipboard 4 for Cut, Copy and Paste.",
    },
    Cmd {
        id: "edit.clipboard_5",
        title: "Set Current Clipboard 5",
        shortcut: "",
        doc: "Use clipboard 5 for Cut, Copy and Paste.",
    },
    Cmd {
        id: "metronome.beats_2",
        title: "Metronome: 2 Beats per Bar",
        shortcut: "",
        doc: "Accent every second click.",
    },
    Cmd {
        id: "metronome.beats_3",
        title: "Metronome: 3 Beats per Bar",
        shortcut: "",
        doc: "Accent every third click (3/4).",
    },
    Cmd {
        id: "metronome.beats_4",
        title: "Metronome: 4 Beats per Bar",
        shortcut: "",
        doc: "Accent every fourth click (4/4).",
    },
    Cmd {
        id: "metronome.beats_6",
        title: "Metronome: 6 Beats per Bar",
        shortcut: "",
        doc: "Accent every sixth click (6/8).",
    },
    Cmd {
        id: "metronome.tempo_up",
        title: "Metronome: Faster Tempo",
        shortcut: "",
        doc: "Raise the session tempo by 5 BPM (up to 300).",
    },
    Cmd {
        id: "metronome.tempo_down",
        title: "Metronome: Slower Tempo",
        shortcut: "",
        doc: "Lower the session tempo by 5 BPM (down to 20).",
    },
    Cmd {
        id: "clip.gain_up",
        title: "Clip Gain +1 dB",
        shortcut: "",
        doc: "Raise the selected clips' gain by 1 dB.",
    },
    Cmd {
        id: "clip.gain_down",
        title: "Clip Gain -1 dB",
        shortcut: "",
        doc: "Lower the selected clips' gain by 1 dB.",
    },
    Cmd {
        id: "clip.gain_reset",
        title: "Reset Clip Gain",
        shortcut: "",
        doc: "Set the selected clips' gain back to 0 dB.",
    },
    Cmd {
        id: "clip.bring_front",
        title: "Bring Clip to Front",
        shortcut: "",
        doc: "Draw and play the selected clip on top of clips it overlaps on its track.",
    },
    Cmd {
        id: "clip.group",
        title: "Group Clips",
        shortcut: "",
        doc: "Group the selected clips: they then select, move and delete together.",
    },
    Cmd {
        id: "clip.ungroup",
        title: "Ungroup Clips",
        shortcut: "",
        doc: "Take the selected clips out of their group.",
    },
    Cmd {
        id: "tracks.show_all",
        title: "Show All Tracks",
        shortcut: "",
        doc: "Show every track in the Editor (Tracks panel).",
    },
    Cmd {
        id: "tracks.hide_all",
        title: "Hide All Tracks",
        shortcut: "",
        doc: "Hide every track from the Editor; hidden tracks still play.",
    },
    Cmd {
        id: "tracks.only_selected",
        title: "Show Only Selected Track",
        shortcut: "",
        doc: "Hide every track except the selected one.",
    },
    Cmd {
        id: "tracks.hide_empty",
        title: "Hide Empty Tracks",
        shortcut: "",
        doc: "Hide tracks that hold no clips.",
    },
    Cmd {
        id: "clip.fade_in_remove",
        title: "Remove Fade In",
        shortcut: "",
        doc: "Take the fade-in off the selected clips.",
    },
    Cmd {
        id: "clip.fade_out_remove",
        title: "Remove Fade Out",
        shortcut: "",
        doc: "Take the fade-out off the selected clips.",
    },
];

/// The five clipboards: the current one lives in `App::clipboard`, the others wait here.
#[derive(Default)]
pub struct Clipboards {
    pub current: usize,
    pub others: [Option<AudioData>; 5],
}

impl App {
    /// Run one of `MENU_COMMANDS`. `Ok(false)` = not one of these.
    pub fn exec_menus(&mut self, id: &str) -> Result<bool, String> {
        if let Some(n) = id.strip_prefix("edit.clipboard_") {
            let n: usize = n.parse().map_err(|_| "No such clipboard.".to_string())?;
            self.set_clipboard(n.saturating_sub(1));
            return Ok(true);
        }
        match id {
            "metronome.beats_2" | "metronome.beats_3" | "metronome.beats_4"
            | "metronome.beats_6" => {
                let beats: u32 = id
                    .rsplit('_')
                    .next()
                    .and_then(|b| b.parse().ok())
                    .unwrap_or(4);
                self.ensure_session();
                if let Some(s) = self.session.as_mut() {
                    s.metronome.beats = beats;
                    s.touch();
                }
                self.status = format!("Metronome: {beats} beats per bar");
            }
            "metronome.tempo_up" | "metronome.tempo_down" => {
                let step = if id.ends_with("up") { 5.0 } else { -5.0 };
                self.ensure_session();
                if let Some(s) = self.session.as_mut() {
                    s.metronome.bpm = (s.metronome.bpm + step).clamp(20.0, 300.0);
                    s.touch();
                    self.status = format!("Tempo {:.0} BPM", s.metronome.bpm);
                }
            }
            "clip.gain_up" | "clip.gain_down" | "clip.gain_reset" => {
                let ids = self.selected_clip_ids();
                if ids.is_empty() {
                    return Err("Select a clip first.".into());
                }
                if let Some(s) = self.session.as_mut() {
                    for cid in ids {
                        if let Some(c) = s.clip_mut(cid) {
                            c.gain_db = match id {
                                "clip.gain_up" => (c.gain_db + 1.0).min(48.0),
                                "clip.gain_down" => (c.gain_db - 1.0).max(-96.0),
                                _ => 0.0,
                            };
                        }
                    }
                    s.touch();
                }
                self.session_push("Clip Gain");
            }
            "clip.group" | "clip.ungroup" => {
                let ids = self.selected_clip_ids();
                let s = self
                    .session
                    .as_mut()
                    .ok_or("Open a multitrack session first.")?;
                if id == "clip.group" {
                    if ids.len() < 2 {
                        return Err("Select two or more clips (Ctrl+click) to group them.".into());
                    }
                    let g = s.id();
                    for c in &ids {
                        s.groups.insert(*c, g);
                    }
                    self.status = format!("{} clips grouped", ids.len());
                } else {
                    let mut all = vec![];
                    for c in &ids {
                        all.extend(s.group_of(*c));
                    }
                    for c in all {
                        s.groups.remove(&c);
                    }
                    self.status = "Clips ungrouped".into();
                }
                s.touch();
                self.session_push(if id == "clip.group" {
                    "Group Clips"
                } else {
                    "Ungroup Clips"
                });
            }
            "tracks.show_all"
            | "tracks.hide_all"
            | "tracks.only_selected"
            | "tracks.hide_empty" => {
                let s = self
                    .session
                    .as_mut()
                    .ok_or("Open a multitrack session first.")?;
                let sel = s.selected_track;
                for (i, t) in s.tracks.iter_mut().enumerate() {
                    t.hidden = match id {
                        "tracks.show_all" => false,
                        "tracks.hide_all" => true,
                        "tracks.only_selected" => i != sel,
                        _ => t.hidden || t.clips.is_empty(),
                    };
                }
                s.touch();
                let shown = s.tracks.iter().filter(|t| !t.hidden).count();
                self.status = format!("{shown} of {} tracks shown", s.tracks.len());
            }
            "clip.bring_front" => {
                let ids = self.selected_clip_ids();
                if ids.is_empty() {
                    return Err("Select a clip first.".into());
                }
                if let Some(s) = self.session.as_mut() {
                    for cid in ids {
                        if let Some((t, i)) = s.find_clip(cid) {
                            let c = s.tracks[t].clips.remove(i);
                            s.tracks[t].clips.push(c);
                        }
                    }
                    s.touch();
                }
                self.session_push("Bring to Front");
            }
            "clip.fade_in_remove" | "clip.fade_out_remove" => {
                let ids = self.selected_clip_ids();
                if ids.is_empty() {
                    return Err("Select a clip first.".into());
                }
                let fin = id == "clip.fade_in_remove";
                if let Some(s) = self.session.as_mut() {
                    for cid in ids {
                        if let Some(c) = s.clip_mut(cid) {
                            if fin {
                                c.fade_in = 0.0;
                                c.auto_in = false;
                            } else {
                                c.fade_out = 0.0;
                                c.auto_out = false;
                            }
                        }
                    }
                    s.touch();
                }
                self.session_push(if fin {
                    "Remove Fade In"
                } else {
                    "Remove Fade Out"
                });
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Switch to clipboard `n` (0-based): what was copied stays in the old one.
    pub fn set_clipboard(&mut self, n: usize) {
        let n = n.min(4);
        let cur = self.clipboards.current.min(4);
        if n == cur {
            return;
        }
        self.clipboards.others[cur] = self.clipboard.take();
        self.clipboard = self.clipboards.others[n].take();
        self.clipboards.current = n;
        self.status = format!(
            "Clipboard {} is current{}",
            n + 1,
            if self.clipboard.is_some() {
                ""
            } else {
                " (empty)"
            }
        );
    }

    fn menus_run(&mut self, ui: &mut egui::Ui, id: &str, ctx: &egui::Context) {
        if let Err(e) = self.exec(id, ctx) {
            self.status = e;
        }
        ui.close();
    }

    /// Edit > Set Current Clipboard.
    pub fn clipboard_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.menu_button("Set Current Clipboard", |ui| {
            for n in 0..5 {
                let filled = if n == self.clipboards.current {
                    self.clipboard.is_some()
                } else {
                    self.clipboards.others[n].is_some()
                };
                let label = format!(
                    "Clipboard {}{}",
                    n + 1,
                    if filled { "" } else { " (empty)" }
                );
                if crate::theme::radio_item(ui, true, n == self.clipboards.current, &label, "")
                    .clicked()
                {
                    let id = format!("edit.clipboard_{}", n + 1);
                    self.menus_run(ui, &id, ctx);
                }
            }
        });
    }

    /// Multitrack > Metronome.
    pub fn metronome_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let m = self
            .session
            .as_ref()
            .map(|s| s.metronome.clone())
            .unwrap_or_default();
        ui.menu_button("Metronome", |ui| {
            if crate::theme::check_item(ui, true, m.on, "Enable Metronome", "").clicked() {
                self.menus_run(ui, "view.toggle_metronome", ctx);
            }
            ui.separator();
            ui.menu_button("Beats per Bar", |ui| {
                for b in [2u32, 3, 4, 6] {
                    if crate::theme::radio_item(ui, true, m.beats == b, &format!("{b}"), "")
                        .clicked()
                    {
                        self.menus_run(ui, &format!("metronome.beats_{b}"), ctx);
                    }
                }
            });
            ui.menu_button(format!("Tempo ({:.0} BPM)", m.bpm), |ui| {
                if ui.button("Faster (+5 BPM)").clicked() {
                    self.menus_run(ui, "metronome.tempo_up", ctx);
                }
                if ui.button("Slower (-5 BPM)").clicked() {
                    self.menus_run(ui, "metronome.tempo_down", ctx);
                }
            });
        });
    }

    /// Multitrack > Track Visibility (the Tracks panel's buttons).
    pub fn track_visibility_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let on = self.session.is_some();
        ui.add_enabled_ui(on, |ui| {
            ui.menu_button("Track Visibility", |ui| {
                self.menus_item(ui, true, "Show All Tracks", "tracks.show_all", ctx);
                self.menus_item(ui, true, "Hide All Tracks", "tracks.hide_all", ctx);
                self.menus_item(
                    ui,
                    true,
                    "Show Only Selected Track",
                    "tracks.only_selected",
                    ctx,
                );
                self.menus_item(ui, true, "Hide Empty Tracks", "tracks.hide_empty", ctx);
                ui.separator();
                if ui.button("Tracks Panel...").clicked() {
                    self.show_panel(crate::workspace::PanelId::Tracks);
                    ui.close();
                }
            });
        });
    }

    /// Clip > Group.
    pub fn clip_group_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (n, grouped) = {
            let ids = self.selected_clip_ids();
            let g = self
                .session
                .as_ref()
                .map(|s| ids.iter().any(|c| s.groups.contains_key(c)))
                .unwrap_or(false);
            (ids.len(), g)
        };
        ui.menu_button("Group", |ui| {
            self.menus_item(ui, n >= 2, "Group Clips", "clip.group", ctx);
            self.menus_item(ui, grouped, "Ungroup Clips", "clip.ungroup", ctx);
        });
    }

    /// Clip > Clip Gain.
    pub fn clip_gain_menu(&mut self, ui: &mut egui::Ui, enabled: bool, ctx: &egui::Context) {
        ui.add_enabled_ui(enabled, |ui| {
            ui.menu_button("Clip Gain", |ui| {
                for (label, id) in [
                    ("Increase by 1 dB", "clip.gain_up"),
                    ("Decrease by 1 dB", "clip.gain_down"),
                    ("Reset to 0 dB", "clip.gain_reset"),
                ] {
                    if ui.button(label).clicked() {
                        self.menus_run(ui, id, ctx);
                    }
                }
            });
        });
    }

    /// One plain menu item that runs a command.
    pub fn menus_item(
        &mut self,
        ui: &mut egui::Ui,
        enabled: bool,
        label: &str,
        id: &str,
        ctx: &egui::Context,
    ) {
        // the same item as every other menu entry (shortcut and help text included)
        self.menu_cmd(ui, enabled, label, id, ctx);
    }
}
