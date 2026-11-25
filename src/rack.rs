//! Effects Rack panel (file, Track Effects and Clip Effects) and the floating Rack Effect windows.

use crate::app::{App, Mode};
use crate::audio;
use crate::dsp::{self, Effect, EffectSlot, FilterKind};
use crate::plugins::{self, PluginEntry};
use crate::theme::{self, Icon, icon_button};
use egui::{Color32, RichText, Sense, Stroke, pos2, vec2};

/// Which rack is being edited.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RackTarget {
    File(u64),
    Track(usize),
    Clip(u64),
}

/// Slots the rack always shows (like the original's sixteen); it grows past them as effects are
/// added, always keeping one empty slot to add another, up to `MAX_SLOTS`.
pub const SLOTS: usize = 16;
/// The most effects one rack holds.
pub const MAX_SLOTS: usize = 64;

/// How many slot rows a rack holding `n` effects shows.
pub fn rows_for(n: usize) -> usize {
    (n + 1).clamp(SLOTS, MAX_SLOTS)
}

/// The effect menu is a large, separate click target: dragging starts only outside this area.
const EFFECT_ARROW_WIDTH: f32 = 32.0;
/// Height of an effect slot row (compact, like the original's rack list).
const RACK_ROW_H: f32 = 22.0;
const EFFECT_ARROW_GAP: f32 = 6.0;

/// The part of an effect row that selects or drags it. The arrow's area is deliberately excluded
/// so hovering or pressing the arrow never starts a drag or shows the drag cursor.
pub(crate) fn effect_row_drag_rect(row: egui::Rect) -> egui::Rect {
    let right = (row.right() - EFFECT_ARROW_WIDTH - EFFECT_ARROW_GAP).max(row.left());
    egui::Rect::from_min_max(row.min, egui::pos2(right, row.bottom()))
}

/// Where a dragged effect lands: the gap before the first filled row whose middle is below
/// `y` (`rows.len()` = after the last one).
pub fn drop_index(rows: &[egui::Rect], y: f32) -> usize {
    rows.iter()
        .position(|r| y < r.center().y)
        .unwrap_or(rows.len())
}

/// Move the effect at `from` into the gap `to` (0..=len). Returns its new slot, or `None` when
/// nothing moves (dropped back where it was, or out of range).
pub fn move_slot<T>(slots: &mut Vec<T>, from: usize, to: usize) -> Option<usize> {
    if from >= slots.len() || to > slots.len() {
        return None;
    }
    let dest = if to > from { to - 1 } else { to };
    if dest == from {
        return None;
    }
    let s = slots.remove(from);
    slots.insert(dest, s);
    Some(dest)
}

/// The rack still has a visible slot strip and input/output/mix section before a file or
/// track exists. It is read-only here: the same controls become live as soon as a target
/// is chosen. This mirrors the panel's shape in the supplied stills.
fn draw_empty_rack(ui: &mut egui::Ui) {
    // Drawn with exactly the live rack's sizes and shapes (row height, rounded number and power
    // boxes, the large painted arrow, the same scroll bar and bottom buttons), only greyed out,
    // so the panel never changes look when the first file or track arrives.
    ui.label(
        RichText::new("Track: Track 1")
            .strong()
            .color(theme::text_dim()),
    );
    let below = 132.0 + 40.0;
    let rows_h = (ui.available_height() - below).clamp(3.0 * RACK_ROW_H, 16.0 * RACK_ROW_H + 2.0);
    egui::ScrollArea::vertical()
        .id_salt("rack_empty")
        .max_height(rows_h)
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show(ui, |ui| {
            for i in 0..SLOTS {
                let (r, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), RACK_ROW_H), Sense::hover());
                let p = ui.painter().clone();
                p.rect_filled(
                    r,
                    0.0,
                    if i % 2 == 0 {
                        theme::own_or(crate::theme::gray(30), theme::track_lane_a())
                    } else {
                        theme::own_or(crate::theme::gray(34), theme::track_lane_b())
                    },
                );
                p.rect_filled(
                    egui::Rect::from_min_size(r.min, vec2(20.0, r.height()))
                        .shrink2(vec2(0.0, 1.0)),
                    crate::theme::ROUND as f32 - 2.0,
                    theme::own_or(crate::theme::gray(40), theme::control()),
                );
                p.text(
                    pos2(r.left() + 10.0, r.center().y),
                    egui::Align2::CENTER_CENTER,
                    format!("{}", i + 1),
                    egui::FontId::proportional(11.5),
                    theme::text_dim(),
                );
                let mut row = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(egui::Rect::from_min_max(
                            pos2(r.left() + 22.0, r.top()),
                            r.max,
                        ))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                row.add_enabled_ui(false, |row| {
                    power_switch(row, false, "Open a file or add a track to use the rack")
                });
                // the same large triangle as the live rack's effect-menu arrow, dimmed
                let ar = egui::Rect::from_min_size(
                    pos2(r.right() - 2.0 - EFFECT_ARROW_WIDTH, r.top() + 1.0),
                    vec2(EFFECT_ARROW_WIDTH, RACK_ROW_H - 2.0),
                );
                let c = ar.center();
                p.add(egui::Shape::convex_polygon(
                    vec![
                        pos2(c.x - 3.5, c.y - 6.0),
                        pos2(c.x + 5.5, c.y),
                        pos2(c.x - 3.5, c.y + 6.0),
                    ],
                    theme::text_dim(),
                    egui::Stroke::NONE,
                ));
            }
        });
    ui.add_space(4.0);
    ui.add_enabled_ui(false, |ui| {
        for name in ["Input:", "Output:"] {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [52.0, 18.0],
                    egui::Label::new(RichText::new(name).color(theme::text_dim())),
                );
                let mut v = 0.0f32;
                theme::knob(ui, &mut v, -24.0, 24.0, 20.0, "Gain");
                ui.add_sized(
                    [34.0, 18.0],
                    egui::Label::new(RichText::new("+0").color(theme::time_blue())),
                );
                let (r, _) = ui.allocate_exact_size(
                    vec2((ui.available_width() - 6.0).max(20.0), 14.0),
                    Sense::hover(),
                );
                mini_meter(ui.painter(), r, [0.0, 0.0]);
            });
        }
        ui.horizontal(|ui| {
            ui.add_space(92.0);
            let (r, _) = ui.allocate_exact_size(
                vec2((ui.available_width() - 6.0).max(20.0), 12.0),
                Sense::hover(),
            );
            db_scale(ui.painter(), r);
        });
        ui.horizontal(|ui| {
            ui.add_sized(
                [52.0, 18.0],
                egui::Label::new(RichText::new("Mix:").color(theme::text_dim())),
            );
            ui.label(RichText::new("Dry").color(theme::text_dim()));
            let mut pct = 100.0f32;
            ui.spacing_mut().slider_width = (ui.available_width() - 90.0).max(40.0);
            ui.add(crate::theme::RSlider::new(&mut pct, 0.0..=100.0).show_value(false));
            ui.label(RichText::new("Wet").color(theme::text_dim()));
            ui.label(RichText::new("100 %").color(theme::time_blue()));
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            power_switch(ui, true, "Open a file or add a track to use the rack");
            tool_toggle(
                ui,
                Icon::List,
                true,
                "Show or hide the Input, Output and Mix controls",
            );
            tool_toggle(ui, Icon::PostFader, true, "Pre- or post-fader");
            tool_toggle(ui, Icon::Lightning, false, "Pre-render the track's effects");
        });
    })
    .response
    .on_disabled_hover_text("Open a file or add a track to use the rack.");
}

const PRESETS: &[&str] = &[
    "(Default)",
    "Vocal Clean-Up",
    "Podcast Voice",
    "Telephone",
    "Warm Room",
    "Wide Echo",
    "Loudness Maximizer",
];

fn preset(name: &str) -> Vec<EffectSlot> {
    let s = EffectSlot::new;
    match name {
        "Vocal Clean-Up" => vec![
            s(Effect::Filter {
                kind: FilterKind::HighPass,
                freq: 90.0,
                q: 0.707,
                gain_db: 0.0,
            }),
            s(Effect::NoiseGate {
                threshold: -50.0,
                attack_ms: 2.0,
                release_ms: 150.0,
                floor_db: -30.0,
            }),
            s(Effect::Compressor {
                threshold: -20.0,
                ratio: 3.0,
                attack_ms: 8.0,
                release_ms: 120.0,
                makeup: 3.0,
            }),
        ],
        "Podcast Voice" => vec![
            s(Effect::ParametricEq {
                bands: crate::panels::eq_preset(0).unwrap_or_default(),
                master_db: 0.0,
            }),
            s(Effect::Compressor {
                threshold: -22.0,
                ratio: 3.0,
                attack_ms: 5.0,
                release_ms: 120.0,
                makeup: 4.0,
            }),
            s(Effect::HardLimiter {
                ceiling_db: -1.0,
                input_boost: 0.0,
                release_ms: 50.0,
            }),
        ],
        "Telephone" => vec![s(Effect::ParametricEq {
            bands: crate::panels::eq_preset(3).unwrap_or_default(),
            master_db: 0.0,
        })],
        "Warm Room" => vec![
            s(Effect::ParametricEq {
                bands: crate::panels::eq_preset(1).unwrap_or_default(),
                master_db: 0.0,
            }),
            s(Effect::Reverb {
                room: 0.45,
                damping: 0.6,
                width: 1.0,
                mix: 0.2,
            }),
        ],
        "Wide Echo" => vec![
            s(Effect::Delay {
                time_ms: 380.0,
                feedback: 0.4,
                mix: 0.35,
            }),
            s(Effect::StereoExpander { width: 1.6 }),
        ],
        "Loudness Maximizer" => vec![
            s(Effect::Compressor {
                threshold: -18.0,
                ratio: 2.5,
                attack_ms: 10.0,
                release_ms: 150.0,
                makeup: 4.0,
            }),
            s(Effect::HardLimiter {
                ceiling_db: -0.3,
                input_boost: 6.0,
                release_ms: 60.0,
            }),
        ],
        _ => vec![],
    }
}

/// Small horizontal level meter (input/output of the rack).
fn mini_meter(p: &egui::Painter, r: egui::Rect, lv: [f32; 2]) {
    p.rect_filled(r, 0.0, crate::theme::gray(18));
    let h = r.height() / 2.0;
    for (i, l) in lv.iter().enumerate() {
        let d = audio::db(*l).max(-60.0);
        let w = ((d + 60.0) / 60.0).clamp(0.0, 1.0) * r.width();
        let rr = egui::Rect::from_min_size(
            pos2(r.left(), r.top() + i as f32 * h + 0.5),
            vec2(w, h - 1.0),
        );
        p.rect_filled(rr, 0.0, theme::meter_color(d));
    }
    p.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0, crate::theme::gray(60)),
        egui::StrokeKind::Inside,
    );
}

impl App {
    /// The rack the panel is showing.
    pub fn rack_target(&self) -> Option<RackTarget> {
        match self.mode {
            Mode::Waveform => self.current.map(RackTarget::File),
            Mode::Multitrack => {
                let s = self.session.as_ref()?;
                if self.rack_clip_tab {
                    s.selected_clip.map(RackTarget::Clip)
                } else if s.master_selected {
                    Some(RackTarget::Track(crate::session::MASTER_INDEX))
                } else if s.tracks.is_empty() {
                    None
                } else {
                    Some(RackTarget::Track(s.selected_track.min(s.tracks.len() - 1)))
                }
            }
        }
    }

    /// (label, slots, sample rate) of a rack.
    pub fn rack_get(&self, t: RackTarget) -> Option<(String, Vec<EffectSlot>, u32)> {
        match t {
            RackTarget::File(id) => self
                .file(id)
                .map(|f| (format!("File: {}", f.name), f.rack.clone(), f.data.rate)),
            RackTarget::Track(i) => {
                let s = self.session.as_ref()?;
                if i == crate::session::MASTER_INDEX {
                    return Some(("Track: Master".to_string(), s.master.rack.clone(), s.rate));
                }
                s.tracks
                    .get(i)
                    .map(|tr| (format!("Track: {}", tr.name), tr.rack.clone(), s.rate))
            }
            RackTarget::Clip(id) => {
                let s = self.session.as_ref()?;
                let (ti, ci) = s.find_clip(id)?;
                let c = &s.tracks[ti].clips[ci];
                Some((format!("Clip: {}", c.name), c.rack.clone(), s.rate))
            }
        }
    }

    /// Store a rack's slots. `history` records an undo step (structure changes, not every drag).
    pub fn rack_set(&mut self, t: RackTarget, slots: Vec<EffectSlot>, history: Option<&str>) {
        match t {
            RackTarget::File(id) => {
                if let Some(f) = self.file_mut(id) {
                    f.rack = slots;
                }
            }
            RackTarget::Track(i) => {
                if let Some(s) = self.session.as_mut() {
                    if i == crate::session::MASTER_INDEX {
                        s.master.rack = slots;
                        s.touch();
                    } else if let Some(tr) = s.tracks.get_mut(i) {
                        tr.rack = slots;
                        s.touch();
                    }
                }
                if let Some(h) = history {
                    self.session_push(h);
                }
            }
            RackTarget::Clip(id) => {
                if let Some(s) = self.session.as_mut() {
                    if let Some(c) = s.clip_mut(id) {
                        c.rack = slots;
                    }
                    s.touch();
                }
                if let Some(h) = history {
                    self.session_push(h);
                }
            }
        }
        // make the change audible right away (chains are rebuilt on this, the UI, thread)
        if self.mode == Mode::Multitrack {
            self.refresh_mix(true);
        }
    }

    /// Put an effect into the first free slot (or the given one) and open its window.
    pub fn rack_insert(&mut self, t: RackTarget, slot: Option<usize>, e: Effect) {
        let Some((_, mut slots, _)) = self.rack_get(t) else {
            return;
        };
        let name = e.name().to_string();
        // Preferences > Effects > Default mix of new effects
        let mut new_slot = EffectSlot::new(e);
        new_slot.mix = crate::prefs::fx_default_mix();
        let idx = match slot {
            Some(i) if i < slots.len() => {
                slots[i] = new_slot;
                i
            }
            _ => {
                if slots.len() >= MAX_SLOTS {
                    self.message(
                        "Effects Rack is full",
                        &format!("All {MAX_SLOTS} slots are in use. Remove an effect first."),
                    );
                    return;
                }
                slots.push(new_slot);
                slots.len() - 1
            }
        };
        self.rack_set(t, slots, Some("Add Effect"));
        self.rack_window = Some((t, idx));
        self.status = format!("{name} added to the Effects Rack");
    }

    fn effect_menu(&mut self, ui: &mut egui::Ui, t: RackTarget, slot: Option<usize>) {
        // the rack's effect list, category by category, in the original's order
        const RACK_MENU: &[(&str, &[&str])] = &[
            (
                "Amplitude and Compression",
                &[
                    "Amplify",
                    "Channel Mixer",
                    "DeEsser",
                    "Dynamics",
                    "Dynamics Processing",
                    "Hard Limiter",
                    "Multiband Compressor",
                    "Single-band Compressor",
                    "Speech Volume Leveler",
                    "Tube-modeled Compressor",
                ],
            ),
            ("Delay and Echo", &["Analog Delay", "Delay", "Echo"]),
            (
                "Filter and EQ",
                &[
                    "FFT Filter",
                    "Graphic Equalizer (10 Bands)",
                    "Graphic Equalizer (20 Bands)",
                    "Graphic Equalizer (30 Bands)",
                    "Notch Filter",
                    "Parametric Equalizer",
                    "Scientific Filter",
                ],
            ),
            (
                "Modulation",
                &["Chorus", "Chorus/Flanger", "Flanger", "Phaser"],
            ),
            (
                "Noise Reduction / Restoration",
                &[
                    "DeNoise",
                    "Adaptive Noise Reduction",
                    "Automatic Click Remover",
                    "Automatic Phase Correction",
                    "DeHummer",
                    "DeReverb",
                ],
            ),
            (
                "Reverb",
                &[
                    "Convolution Reverb",
                    "Full Reverb",
                    "Reverb",
                    "Studio Reverb",
                    "Surround Reverb",
                ],
            ),
            (
                "Special",
                &[
                    "Distortion",
                    "Guitar Suite",
                    "Mastering",
                    "Loudness Meter",
                    "Loudness Radar Meter",
                    "Vocal Enhancer",
                ],
            ),
            (
                "Stereo Imagery",
                &[
                    "Center Channel Extractor",
                    "Graphic Phase Shifter",
                    "Stereo Expander",
                ],
            ),
            (
                "Time and Pitch",
                &["Automatic Pitch Correction", "Pitch Shifter"],
            ),
        ];
        for (cat, names) in RACK_MENU {
            ui.menu_button(*cat, |ui| {
                for n in names.iter() {
                    match crate::menus::effect_named(n) {
                        Some(e) => {
                            if ui.button(format!("{n}...")).clicked() {
                                self.rack_insert(t, slot, e);
                                ui.close();
                            }
                        }
                        None => {
                            ui.add_enabled(false, egui::Button::new(format!("{n}...")));
                        }
                    }
                }
            });
        }
        ui.separator();
        ui.menu_button("VST", |ui| {
            ui.label(
                RichText::new("VST 2 plug-ins aren't hosted; use VST3 or CLAP.")
                    .color(theme::text_dim()),
            );
        });
        ui.menu_button("SoundCraft", |ui| {
            let list = plugins::builtin_list();
            let mut cats: Vec<&'static str> = list.iter().map(|e| e.category).collect();
            cats.dedup();
            cats.sort();
            cats.dedup();
            for cat in cats {
                ui.menu_button(cat, |ui| {
                    for e in list.iter().filter(|e| e.category == cat) {
                        if ui.button(&e.name).clicked() {
                            self.rack_insert(t, slot, plugins::effect_for(e));
                            ui.close();
                        }
                    }
                });
            }
        });
        for fmt in ["VST3", "CLAP"] {
            ui.menu_button(fmt, |ui| {
                if plugins::hosting_disabled() {
                    ui.label(
                        RichText::new("Plug-in scanning is turned off.").color(theme::text_dim()),
                    );
                    return;
                }
                match &self.hosted {
                    None => {
                        if ui.button("Scan for Plug-ins").clicked() {
                            self.hosted = Some(plugins::scan_hosted(false));
                        }
                    }
                    Some(list) => {
                        let list: Vec<PluginEntry> =
                            list.iter().filter(|e| e.format == fmt).cloned().collect();
                        if list.is_empty() {
                            ui.label(
                                RichText::new(format!("No {fmt} effects found."))
                                    .color(theme::text_dim()),
                            );
                        }
                        for e in list {
                            let label = if e.vendor.is_empty() {
                                e.name.clone()
                            } else {
                                format!("{}  ({})", e.name, e.vendor)
                            };
                            if ui.button(label).clicked() {
                                let eff = plugins::effect_for(&e);
                                self.rack_insert(t, slot, eff);
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button("Rescan").clicked() {
                            self.hosted = Some(plugins::scan_hosted(true));
                        }
                    }
                }
            });
        }
    }

    /// Apply the file's rack (with its input/output gain and mix) to the selection or entire file.
    pub fn apply_rack(&mut self, id: u64) {
        let entire = self.rack_entire;
        let Some(f) = self.file(id) else { return };
        let slots = f.rack.clone();
        let (gi, go, mix) = (
            audio::undb(f.rack_in_db),
            audio::undb(f.rack_out_db),
            f.rack_mix.clamp(0.0, 1.0),
        );
        let (a, b) = if entire {
            (0, f.data.frames())
        } else {
            f.range()
        };
        let enabled = f.enabled_ch.clone();
        let mut d = (*f.data).clone();
        let dry = d.slice(a, b);
        for c in d.ch.iter_mut() {
            for s in c[a..b].iter_mut() {
                *s *= gi;
            }
        }
        dsp::apply_chain(&mut d, a, b, &slots);
        for (ci, c) in d.ch.iter_mut().enumerate() {
            let on = enabled.get(ci).copied().unwrap_or(true);
            for (k, s) in c[a..b].iter_mut().enumerate() {
                let wet = *s * go;
                let dv = dry.ch[ci][k];
                *s = if on { dv * (1.0 - mix) + wet * mix } else { dv };
            }
        }
        if let Some(f) = self.file_mut(id) {
            f.push("Effects Rack", d);
            f.rack.clear();
        }
        self.rack_window = None;
        self.status = "Effects Rack applied".into();
    }

    /// Floating "Rack Effect" window for one slot.
    pub fn rack_windows(&mut self, ctx: &egui::Context) {
        // a VST3/CLAP effect opens its own editor (in the Plug-ins panel), not the sliders;
        // the sliders stay for plug-ins without an editor or when they are asked for
        if let Some((t, i)) = self.rack_window {
            if self.rack_generic != Some((t, i)) && self.slot_is_hosted(t, i) {
                self.rack_window = None;
                if let Err(e) = self.plug_open(t, i) {
                    self.status =
                        format!("Plug-in window unavailable ({e}); showing its parameters");
                    self.rack_generic = Some((t, i));
                    self.rack_window = Some((t, i));
                }
            }
        }
        let Some((t, i)) = self.rack_window else {
            self.rack_generic = None;
            return;
        };
        let Some((label, mut slots, rate)) = self.rack_get(t) else {
            self.rack_window = None;
            return;
        };
        if i >= slots.len() {
            self.rack_window = None;
            return;
        }
        let orig = slots[i].clone();
        let mut open = true;
        let mut want_editor = false;
        let title = format!("Rack Effect - {}", slots[i].effect.name());
        egui::Window::new(title)
            .id(egui::Id::new(("rack_effect", format!("{t:?}"), i)))
            .open(&mut open)
            .resizable(true)
            .default_width(560.0)
            .show(ctx, |ui| {
                let s = &mut slots[i];
                // presets of this effect: its defaults, the person's saved ones, or (Custom)
                let ename = s.effect.name().to_string();
                let default = Effect::all_defaults()
                    .into_iter()
                    .find(|e| e.name() == ename);
                let saved: Vec<(String, Effect)> = self
                    .effect_presets
                    .iter()
                    .filter(|(e, _, _)| *e == ename)
                    .map(|(_, n, fx)| (n.clone(), fx.clone()))
                    .collect();
                let current = if default.as_ref() == Some(&s.effect) {
                    "(Default)".to_string()
                } else {
                    saved
                        .iter()
                        .find(|(_, fx)| *fx == s.effect)
                        .map(|(n, _)| n.clone())
                        .unwrap_or_else(|| "(Custom)".into())
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Presets:").color(theme::text_dim()));
                    egui::ComboBox::from_id_salt(("fx_presets", i))
                        .width(260.0)
                        .selected_text(&current)
                        .show_ui(ui, |ui| {
                            if let Some(d) = &default {
                                if ui
                                    .selectable_label(current == "(Default)", "(Default)")
                                    .clicked()
                                {
                                    s.effect = d.clone();
                                }
                            }
                            for (n, fx) in &saved {
                                if ui.selectable_label(current == *n, n).clicked() {
                                    s.effect = fx.clone();
                                }
                            }
                        });
                    if icon_button(ui, Icon::Insert, false, "Save these settings as a preset")
                        .clicked()
                    {
                        let n = format!("{} {}", ename, saved.len() + 1);
                        self.effect_presets
                            .push((ename.clone(), n, s.effect.clone()));
                    }
                    let can_delete = saved.iter().any(|(n, _)| *n == current);
                    if ui
                        .add_enabled_ui(can_delete, |ui| {
                            icon_button(ui, Icon::Trash, false, "Delete this saved preset")
                        })
                        .inner
                        .clicked()
                    {
                        self.effect_presets
                            .retain(|(e, n, _)| !(*e == ename && *n == current));
                    }
                });
                ui.separator();
                if let Effect::Plugin { id, .. } = &s.effect {
                    if plugins::is_hosted(id) {
                        ui.horizontal(|ui| {
                            let running = self.plug_dock.is_open(t, i);
                            if ui
                                .add_enabled(!running, egui::Button::new("Open Plug-in Window"))
                                .clicked()
                            {
                                want_editor = true;
                            }
                            ui.label(RichText::new(id.as_str()).small().color(theme::text_dim()));
                        });
                        ui.separator();
                    }
                }
                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .show(ui, |ui| {
                        s.effect.ui(ui, rate as f32);
                    });
                // the dry/wet mix on its own row, then the footer like the original:
                // channels, power at the left, where the effect sits at the right
                dsp::slider(ui, "Mix (Dry/Wet)", &mut s.mix, 0.0..=1.0, "", false);
                ui.separator();
                ui.label(RichText::new("In: L, R | Out: L, R").color(theme::text_dim()));
                ui.horizontal(|ui| {
                    if power_switch(ui, s.enabled, "Effect on/off").clicked() {
                        s.enabled = !s.enabled;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("Slot {}", i + 1)).color(theme::text_dim()));
                        ui.separator();
                        ui.label(RichText::new(&label).color(theme::text_dim()));
                    });
                });
            });
        if slots[i] != orig {
            self.rack_set(t, slots.clone(), None);
        }
        if want_editor {
            if let Err(e) = self.plug_open(t, i) {
                self.message("Cannot open the plug-in window", &e);
            }
        }
        if !open {
            self.rack_window = None;
            self.rack_generic = None;
        }
    }
}

// the rack's switches are the shared ones, so they look and react like every other toggle
use theme::{icon_toggle as tool_toggle, power_toggle as power_switch};

/// dB scale under the Input/Output meters.
fn db_scale(p: &egui::Painter, r: egui::Rect) {
    for d in [-54, -48, -42, -36, -30, -24, -18, -12, -6, 0] {
        let x = r.left() + ((d as f32 + 60.0) / 60.0) * r.width();
        p.text(
            pos2(x, r.center().y),
            egui::Align2::CENTER_CENTER,
            format!("{d}"),
            egui::FontId::proportional(9.0),
            theme::text_dim(),
        );
    }
    p.text(
        pos2(r.left() + 6.0, r.center().y),
        egui::Align2::CENTER_CENTER,
        "dB",
        egui::FontId::proportional(9.0),
        theme::text_dim(),
    );
}

impl App {
    /// Rack controls (power, input/output, mix, pre/post fader, controls shown) of a rack.
    pub fn rack_opts(&self, t: RackTarget) -> crate::dsp::RackOpts {
        match t {
            RackTarget::File(id) => self
                .file(id)
                .map(|f| crate::dsp::RackOpts {
                    power: f.rack_power,
                    in_db: f.rack_in_db,
                    out_db: f.rack_out_db,
                    mix: f.rack_mix,
                    pre_fader: true,
                    show_io: self.file_rack_io,
                })
                .unwrap_or_default(),
            RackTarget::Track(i) => self
                .session
                .as_ref()
                .and_then(|s| {
                    if i == crate::session::MASTER_INDEX {
                        Some(s.master.fx.clone())
                    } else {
                        s.tracks.get(i).map(|t| t.fx.clone())
                    }
                })
                .unwrap_or_default(),
            RackTarget::Clip(id) => self
                .session
                .as_ref()
                .and_then(|s| {
                    s.find_clip(id)
                        .map(|(ti, ci)| s.tracks[ti].clips[ci].fx.clone())
                })
                .unwrap_or_default(),
        }
    }

    pub fn set_rack_opts(&mut self, t: RackTarget, o: crate::dsp::RackOpts) {
        match t {
            RackTarget::File(id) => {
                self.file_rack_io = o.show_io;
                if let Some(f) = self.file_mut(id) {
                    f.rack_power = o.power;
                    f.rack_in_db = o.in_db;
                    f.rack_out_db = o.out_db;
                    f.rack_mix = o.mix;
                }
            }
            RackTarget::Track(i) => {
                if let Some(s) = self.session.as_mut() {
                    if i == crate::session::MASTER_INDEX {
                        s.master.fx = o;
                    } else if let Some(tr) = s.tracks.get_mut(i) {
                        tr.fx = o;
                    }
                    s.touch();
                }
                self.refresh_mix(true);
            }
            RackTarget::Clip(id) => {
                if let Some(s) = self.session.as_mut() {
                    if let Some(c) = s.clip_mut(id) {
                        c.fx = o;
                    }
                    s.touch();
                }
                self.refresh_mix(true);
            }
        }
    }

    /// The Effects Rack panel. Clip Effects and Track Effects look and work the same; one acts
    /// on the highlighted clip, the other on the whole track.
    pub fn effects_rack(&mut self, ui: &mut egui::Ui) {
        let multi = self.mode == Mode::Multitrack;
        if multi {
            ui.horizontal(|ui| {
                let w = ui.available_width();
                ui.add_space((w - 190.0).max(0.0) / 2.0);
                ui.spacing_mut().item_spacing.x = 0.0;
                for (clip, label) in [(true, "Clip Effects"), (false, "Track Effects")] {
                    let on = self.rack_clip_tab == clip;
                    let b = egui::Button::new(RichText::new(label).color(if on {
                        theme::time_blue()
                    } else {
                        theme::text()
                    }))
                    .fill(if on {
                        theme::own_or(crate::theme::gray(52), theme::pressed())
                    } else {
                        theme::own_or(crate::theme::gray(38), theme::control())
                    })
                    // a segmented switch: rounded on its outer ends only
                    .corner_radius({
                        let r = crate::theme::ROUND;
                        if clip {
                            egui::CornerRadius {
                                nw: r,
                                sw: r,
                                ne: 0,
                                se: 0,
                            }
                        } else {
                            egui::CornerRadius {
                                nw: 0,
                                sw: 0,
                                ne: r,
                                se: r,
                            }
                        }
                    });
                    if ui.add(b).clicked() {
                        self.rack_clip_tab = clip;
                    }
                }
            });
        }
        let target = self.rack_target();
        let info = target.and_then(|t| self.rack_get(t));
        // the preset name always describes what the rack holds now (so undo shows the right one)
        if let Some((_, slots, _)) = &info {
            let strip = |v: &[EffectSlot]| v.iter().map(|s| s.effect.clone()).collect::<Vec<_>>();
            let now = strip(slots);
            self.rack_preset_name = if now.is_empty() {
                "(Default)".into()
            } else if let Some(p) = PRESETS.iter().find(|p| strip(&preset(p)) == now) {
                p.to_string()
            } else if let Some((n, _)) = self.user_presets.iter().find(|(_, s)| strip(s) == now) {
                n.clone()
            } else {
                "(Custom)".into()
            };
        }
        // presets: built-ins plus the person's own
        ui.horizontal(|ui| {
            ui.label(RichText::new("Presets:").color(theme::text_dim()));
            let mut pick: Option<Vec<EffectSlot>> = None;
            let mut delete: Option<usize> = None;
            let w = (ui.available_width() - 70.0).max(80.0);
            let current = self.rack_preset_name.clone();
            ui.add_enabled_ui(info.is_some(), |ui| {
                egui::ComboBox::from_id_salt("rack_presets2")
                    .width(w)
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for p in PRESETS {
                            if ui.selectable_label(false, *p).clicked() {
                                pick = Some(preset(p));
                                self.rack_preset_name = p.to_string();
                            }
                        }
                        if !self.user_presets.is_empty() {
                            ui.separator();
                        }
                        for (n, s) in self.user_presets.iter() {
                            if ui.selectable_label(false, n).clicked() {
                                pick = Some(s.clone());
                                self.rack_preset_name = n.clone();
                            }
                        }
                    });
                if theme::icon_button(ui, Icon::Insert, false, "Save the rack as a preset")
                    .clicked()
                {
                    if let Some((_, slots, _)) = &info {
                        let n = format!("My Rack {}", self.user_presets.len() + 1);
                        self.user_presets.push((n.clone(), slots.clone()));
                        self.rack_preset_name = n;
                    }
                }
                if theme::icon_button(ui, Icon::Trash, false, "Delete the selected saved preset")
                    .clicked()
                {
                    delete = self
                        .user_presets
                        .iter()
                        .position(|(n, _)| *n == self.rack_preset_name);
                }
            });
            if let Some(i) = delete {
                self.user_presets.remove(i);
                self.rack_preset_name = "(Default)".into();
            }
            if let (Some(p), Some(t)) = (pick, target) {
                // a preset replaces the whole rack; any open effect window belonged to the old one
                self.rack_window = None;
                self.rack_selected = None;
                self.rack_set(t, p, Some("Rack Preset"));
            }
        });
        let Some(t) = target else {
            if multi && self.rack_clip_tab {
                ui.label(
                    RichText::new("Select a clip to edit its Clip Effects.")
                        .color(theme::text_dim()),
                );
            } else {
                draw_empty_rack(ui);
            }
            return;
        };
        let Some((label, mut slots, _rate)) = info else {
            return;
        };
        ui.label(RichText::new(label).strong());
        let orig = slots.clone();
        let mut opts = self.rack_opts(t);
        let orig_opts = opts.clone();
        let mut open_win: Option<usize> = None;
        let mut remove: Option<usize> = None;
        // slots
        // the slot list gives way so the input/output/mix block and the bottom buttons always fit
        let below = if opts.show_io { 132.0 } else { 0.0 } + 40.0;
        let rows_h =
            (ui.available_height() - below).clamp(3.0 * RACK_ROW_H, 16.0 * RACK_ROW_H + 2.0);
        // the scroll bar is always there, so the rows never shift sideways when it would come and go
        let mut row_rects: Vec<egui::Rect> = Vec::new();
        let mut reordered = false;
        // an effect being dragged in this rack (dropped or cancelled ones are forgotten)
        if self
            .rack_drag
            .map(|(dt, i)| dt != t || i >= slots.len())
            .unwrap_or(false)
        {
            self.rack_drag = None;
        }
        let dragging_before = self.rack_drag.map(|(_, i)| i);
        let slots_out = egui::ScrollArea::vertical()
            .id_salt("rack_slots2")
            .max_height(rows_h)
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
            for i in 0..rows_for(slots.len()) {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), RACK_ROW_H), Sense::hover());
                let has = i < slots.len();
                // Only the body of the row can drag. The larger arrow beside it is exclusively
                // a menu button, so it cannot accidentally pick up or reorder the effect.
                let row_resp = ui.interact(
                    effect_row_drag_rect(r),
                    ui.scope_id().with(("rack_effect_row", i)),
                    if has { Sense::click_and_drag() } else { Sense::click() },
                );
                row_rects.push(r);
                let selected = self.rack_selected == Some(i);
                let p = ui.painter();
                p.rect_filled(r, 0.0, if selected { theme::own_or(crate::theme::gray(60), theme::pressed()) } else if i % 2 == 0 { theme::own_or(crate::theme::gray(30), theme::track_lane_a()) } else { theme::own_or(crate::theme::gray(34), theme::track_lane_b()) });
                p.rect_filled(egui::Rect::from_min_size(r.min, vec2(20.0, r.height())).shrink2(vec2(0.0, 1.0)), crate::theme::ROUND as f32 - 2.0, if selected { theme::own_or(crate::theme::gray(70), theme::selected()) } else { theme::own_or(crate::theme::gray(40), theme::control()) });
                let is_dragged = dragging_before == Some(i);
                p.text(pos2(r.left() + 10.0, r.center().y), egui::Align2::CENTER_CENTER, format!("{}", i + 1), egui::FontId::proportional(11.5), theme::text());
                let mut row = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(pos2(r.left() + 22.0, r.top()), r.max)).layout(egui::Layout::left_to_right(egui::Align::Center)));
                row.spacing_mut().item_spacing.x = 5.0;
                if let Some(s) = slots.get_mut(i) {
                    let tip = if s.enabled { "Turn this effect off (press and drag across others to switch them the same way)" } else { "Turn this effect on (press and drag across others to switch them the same way)" };
                    if theme::power_drag(&mut row, "rack_power", &mut s.enabled, tip) {
                        self.rack_selected = Some(i);
                        self.rack_focus = true;
                    }
                    // the name is painted, not a text widget: never selectable or highlighted
                    // the dragged effect stays faint in its old place until it is dropped
                    let col = if is_dragged { crate::theme::gray(90) } else if s.enabled && opts.power { theme::text() } else { theme::text_dim() };
                    let name_rect = egui::Rect::from_min_max(pos2(r.left() + 52.0, r.top()), pos2((r.right() - EFFECT_ARROW_WIDTH - 8.0).max(r.left() + 52.0), r.bottom()));
                    let galley = ui.painter().layout_no_wrap(s.effect.name().to_string(), egui::FontId::proportional(12.5), col);
                    ui.painter().with_clip_rect(name_rect).galley(pos2(name_rect.left(), r.center().y - galley.size().y / 2.0), galley, col);
                } else {
                    power_switch(&mut row, false, "Empty slot: choose an effect with the arrow");
                }
                // click selects the slot, double-click opens the effect (anywhere on the row)
                if row_resp.clicked() {
                    self.rack_selected = Some(i);
                    // Delete now removes this effect rather than a clip or audio
                    self.rack_focus = true;
                }
                if row_resp.double_clicked() && has {
                    open_win = Some(i);
                }
                if has && row_resp.drag_started_by(egui::PointerButton::Primary) {
                    self.rack_drag = Some((t, i));
                    self.rack_selected = Some(i);
                    self.rack_focus = true;
                }
                if has {
                    // an open hand over a filled slot says it can be dragged (like a track's colour strip)
                    if row_resp.hovered() && self.rack_drag.is_none() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    }
                    let _ = row_resp.on_hover_text("Drag to move the effect to another slot, double-click to open its settings");
                }
                row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |row| {
                    row.add_space(2.0);
                    // the triangle is painted, not a font glyph, so it sits exactly in the middle
                    let arrow = egui::Button::new("")
                        .min_size(vec2(EFFECT_ARROW_WIDTH, RACK_ROW_H - 2.0))
                        .fill(Color32::TRANSPARENT)
                        .corner_radius(crate::theme::ROUND);
                    let menu = egui::containers::menu::MenuButton::from_button(arrow).ui(row, |ui| {
                        if ui.add_enabled(has, egui::Button::new("Edit Effect")).clicked() {
                            open_win = Some(i);
                            ui.close();
                        }
                        if ui.add_enabled(has, egui::Button::new("Remove Effect")).clicked() {
                            remove = Some(i);
                            ui.close();
                        }
                        ui.separator();
                        self.effect_menu(ui, t, if has { Some(i) } else { None });
                    });
                    if menu.0.clicked() {
                        self.rack_selected = Some(i);
                        self.rack_focus = true;
                    }
                    let c = menu.0.rect.center();
                    let col = if menu.0.hovered() { theme::strong() } else { theme::text() };
                    row.painter().add(egui::Shape::convex_polygon(vec![pos2(c.x - 3.5, c.y - 6.0), pos2(c.x + 5.5, c.y), pos2(c.x - 3.5, c.y + 6.0)], col, egui::Stroke::NONE));
                    // This is a menu, not a drag handle. In particular, do not inherit the
                    // filled effect row's open-hand cursor.
                    menu.0.on_hover_cursor(egui::CursorIcon::Default);
                });
            }
        });
        // drag: like moving a track by its colour strip, the effect's name follows the pointer,
        // a line shows where it will land, and it moves there when the button is let go
        // (Esc cancels)
        if let Some((_, from)) = self
            .rack_drag
            .filter(|(dt, i)| *dt == t && *i < slots.len())
        {
            let n = slots.len();
            let pointer = ui.ctx().pointer_latest_pos();
            let to = pointer
                .map(|pp| drop_index(&row_rects[..n.min(row_rects.len())], pp.y))
                .unwrap_or(from);
            let released = ui.input(|i| i.pointer.any_released() || !i.pointer.primary_down());
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            let view = slots_out.inner_rect;
            if let (Some(pp), false) = (pointer, released || cancel) {
                let y = if to < n {
                    row_rects[to].top()
                } else {
                    row_rects[n - 1].bottom()
                };
                let y = y.clamp(view.top() + 1.0, view.bottom() - 1.0);
                let lp = ui.painter().with_clip_rect(view.expand(2.0));
                lp.line_segment(
                    [pos2(view.left(), y), pos2(view.right() - 12.0, y)],
                    egui::Stroke::new(2.0, theme::accent()),
                );
                lp.circle_filled(pos2(view.left() + 3.0, y), 3.5, theme::accent());
                // the effect's name rides with the pointer
                let ghost = ui.ctx().layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("rack_drag_ghost"),
                ));
                let name = slots[from].effect.name().to_string();
                let g = ghost.layout_no_wrap(name, egui::FontId::proportional(12.5), theme::text());
                let gr =
                    egui::Rect::from_min_size(pp + vec2(14.0, -10.0), g.size() + vec2(16.0, 6.0));
                ghost.rect_filled(
                    gr,
                    crate::theme::ROUND as f32,
                    crate::theme::gray_a(60, 230),
                );
                ghost.rect_stroke(
                    gr,
                    crate::theme::ROUND as f32,
                    egui::Stroke::new(1.0, theme::accent()),
                    egui::StrokeKind::Inside,
                );
                ghost.galley(gr.min + vec2(8.0, 3.0), g, theme::text());
                ui.ctx().request_repaint();
            }
            if released || cancel {
                self.rack_drag = None;
                if released && !cancel {
                    if let Some(dest) = move_slot(&mut slots, from, to) {
                        self.rack_selected = Some(dest);
                        reordered = true;
                    }
                }
            }
        }
        if let Some(i) = remove {
            if i < slots.len() {
                slots.remove(i);
            }
            self.rack_window = None;
        }
        // Input / Output / Mix
        if opts.show_io {
            let (inm, outm) = match t {
                RackTarget::File(_) => {
                    let e = self.engine.lock();
                    (e.rack_in_meter, e.rack_out_meter)
                }
                RackTarget::Track(i) => {
                    let e = self.engine.lock();
                    let m = if i == crate::session::MASTER_INDEX {
                        e.meter
                    } else {
                        e.track_meters.get(i).copied().unwrap_or([0.0; 2])
                    };
                    ([0.0; 2], m)
                }
                RackTarget::Clip(_) => ([0.0; 2], [0.0; 2]),
            };
            ui.add_space(4.0);
            for (label, which, lv) in [("Input:", 0, inm), ("Output:", 1, outm)] {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [52.0, 18.0],
                        egui::Label::new(RichText::new(label).color(theme::text_dim())),
                    );
                    let v = if which == 0 {
                        &mut opts.in_db
                    } else {
                        &mut opts.out_db
                    };
                    theme::knob(ui, v, -24.0, 24.0, 20.0, "Gain (drag; double-click resets)");
                    ui.add_sized(
                        [34.0, 18.0],
                        egui::Label::new(
                            RichText::new(format!("{:+.0}", v)).color(theme::time_blue()),
                        ),
                    );
                    let (r, _) = ui.allocate_exact_size(
                        vec2((ui.available_width() - 6.0).max(20.0), 14.0),
                        Sense::hover(),
                    );
                    mini_meter(ui.painter(), r, lv);
                });
            }
            ui.horizontal(|ui| {
                ui.add_space(92.0);
                let (r, _) = ui.allocate_exact_size(
                    vec2((ui.available_width() - 6.0).max(20.0), 12.0),
                    Sense::hover(),
                );
                db_scale(ui.painter(), r);
            });
            ui.horizontal(|ui| {
                ui.add_sized(
                    [52.0, 18.0],
                    egui::Label::new(RichText::new("Mix:").color(theme::text_dim())),
                );
                ui.label(RichText::new("Dry").color(theme::text_dim()));
                let mut pct = opts.mix * 100.0;
                ui.spacing_mut().slider_width = (ui.available_width() - 90.0).max(40.0);
                if ui
                    .add(crate::theme::RSlider::new(&mut pct, 0.0..=100.0).show_value(false))
                    .changed()
                {
                    opts.mix = pct / 100.0;
                }
                ui.label(RichText::new("Wet").color(theme::text_dim()));
                ui.label(RichText::new(format!("{:.0} %", pct)).color(theme::time_blue()));
            });
        }
        // bottom: all effects on/off, show controls, pre/post fader, pre-render, Apply
        ui.separator();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            if power_switch(ui, opts.power, "Turn all effects in this rack on or off").clicked() {
                opts.power = !opts.power;
            }
            if tool_toggle(ui, Icon::List, opts.show_io, "Show or hide the Input, Output and Mix controls").clicked() {
                opts.show_io = !opts.show_io;
            }
            if let RackTarget::Track(ti) = t {
                let (icon, tip) = if opts.pre_fader { (Icon::PreFader, "Effects are pre-fader (before volume and pan). Click for post-fader.") } else { (Icon::PostFader, "Effects are post-fader (after volume and pan). Click for pre-fader.") };
                if tool_toggle(ui, icon, !opts.pre_fader, tip).clicked() {
                    opts.pre_fader = !opts.pre_fader;
                }
                let pre = self.session.as_ref().and_then(|s| s.tracks.get(ti)).map(|tr| tr.prerender).unwrap_or(false);
                if tool_toggle(ui, Icon::Lightning, pre, "Pre-render the track's effects (plays a rendered copy, re-rendered when the track changes)").clicked() {
                    if let Some(s) = self.session.as_mut() {
                        if let Some(tr) = s.tracks.get_mut(ti) {
                            tr.prerender = !pre;
                        }
                        s.touch();
                    }
                    self.refresh_mix(true);
                }
            }
            if let RackTarget::File(id) = t {
                ui.add_space(6.0);
                if ui.add_enabled(!slots.is_empty(), egui::Button::new("Apply")).on_hover_text("Process the audio with the rack, then clear it").clicked() {
                    self.apply_rack(id);
                    slots.clear();
                }
                ui.label(RichText::new("Process:").color(theme::text_dim()));
                egui::ComboBox::from_id_salt("rack_scope2").selected_text(if self.rack_entire { "Entire File" } else { "Selection Only" }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.rack_entire, false, "Selection Only");
                    ui.selectable_value(&mut self.rack_entire, true, "Entire File");
                });
            }
        });
        if opts != orig_opts {
            self.set_rack_opts(t, opts);
        }
        if let Some(i) = open_win {
            self.rack_window = Some((t, i));
        }
        let stored_now = self.rack_get(t).map(|r| r.1);
        if stored_now.as_ref() == Some(&orig) && slots != orig {
            let hist = if reordered {
                Some("Move Effect")
            } else if slots.len() != orig.len() {
                Some("Remove Effect")
            } else {
                Some("Effect On/Off")
            };
            self.rack_set(t, slots, hist);
        }
    }
}
