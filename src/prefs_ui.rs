//! The Preferences window (Edit > Preferences > ...): a search field and the category list on the
//! left, the chosen category's settings on the right, Reset / Cancel / OK at the bottom. Changes
//! are made on a copy and applied by OK (Cancel drops them), except the audio devices, which
//! switch at once.

use crate::app::{App, Dialog};
use crate::prefs::{self, CATEGORIES, Prefs};
use crate::sysui::Ask;
use crate::theme;
use egui::{Color32, RichText, vec2};

/// Every setting by category, for the search field: (category, label).
const SEARCH: &[(&str, &str)] = &[
    ("General", "Show tool tips"),
    ("General", "Mouse wheel zoom factor"),
    ("General", "Auto-scroll"),
    ("General", "Return playhead to start position on stop"),
    ("Appearance", "Brightness"),
    ("Appearance", "Interface size"),
    ("Appearance", "Saturation"),
    ("Appearance", "Colour theme"),
    ("Audio Channel Mapping", "Left channel"),
    ("Audio Channel Mapping", "Right channel"),
    ("Audio Hardware", "Default Output"),
    ("Audio Hardware", "Default Input"),
    ("Auto Save", "Automatically save backup copies"),
    ("Control Surface", "Device class"),
    ("Data", "Dither"),
    ("Data", "Smooth delete and cut boundaries"),
    ("Effects", "Default mix"),
    ("Effects", "Scan for plug-ins on start-up"),
    ("Markers & Metadata", "Snap to markers"),
    ("Media & Disk Cache", "Save peak files"),
    ("Media & Disk Cache", "Temporary folder"),
    ("Memory", "History limit"),
    ("Memory", "Undo steps"),
    ("General", "Default save location"),
    ("Multitrack", "Keyframe size"),
    ("Multitrack", "Keyframe hover size"),
    ("Multitrack", "Handle size"),
    ("Multitrack", "Handle line width"),
    ("Multitrack", "Selected keyframe colour"),
    ("Multitrack", "Snapping line colour"),
    ("Control Surface", "MCP (AI assistants)"),
    ("Control Surface", "Access token"),
    ("Control Surface", "Ask me before each change"),
    ("Multitrack", "Keyframe grab distance"),
    ("Multitrack", "Default track height"),
    ("Multitrack", "Track Input/Output button height"),
    ("Multitrack", "Automation lane height"),
    ("Multitrack", "Warn when a .sesx save cannot hold curves"),
    ("Multitrack", "Conversion accuracy"),
    ("Multitrack Clips", "Automatic crossfades"),
    ("Multitrack Clips", "Default crossfade curve"),
    ("Playback and Recording", "Pre-roll"),
    ("Playback and Recording", "Post-roll"),
    ("Playback and Recording", "Recordings folder"),
    ("Spectral Displays", "Windowing function"),
    ("Spectral Displays", "Resolution"),
    ("Spectral Displays", "Decibel range"),
    ("Spectral Displays", "Logarithmic frequency scale"),
    ("Spectral Displays", "Keep selection height when moving"),
    ("Spectral Displays", "Pitch display range"),
    ("Time Display", "Frames per second"),
    ("Video", "Video preview"),
    ("Video", "Preview quality"),
    ("Video", "Video decoder"),
    ("Video", "Download a free decoder"),
    ("Video", "Decoder executable"),
    ("Video", "Decoder folder"),
    ("Video", "Decoder folders"),
    ("Video", "Detect decoder"),
    ("Video", "Free decoder repositories"),
];

/// Settings shown on a page that nothing reads yet (kept so a later version can pick them up).
const NOT_CONNECTED: &[(&str, &str)] = &[
    ("Control Surface", "Device class (a control surface needs MIDI hardware support, which this version has not)"),
    ("Effects", "parallel effects"),
];

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).strong().color(theme::strong()));
    ui.add_space(2.0);
}

fn note(ui: &mut egui::Ui, t: &str) {
    ui.label(RichText::new(t).small().color(theme::text_dim()));
}

/// A preference slider with its name at the left; double-clicking the slider or the name puts
/// the factory default back.
fn slider(
    ui: &mut egui::Ui,
    v: &mut f32,
    default: f32,
    r: std::ops::RangeInclusive<f32>,
    label: &str,
    suffix: &str,
) {
    ui.horizontal(|ui| {
        let name = ui
            .add_sized(
                vec2(190.0, 18.0),
                egui::Label::new(label).sense(egui::Sense::click()),
            )
            .on_hover_text("Double-click to reset to the default");
        ui.add(
            crate::theme::RSlider::new(v, r)
                .suffix(suffix)
                .default_value(default),
        );
        if name.double_clicked() {
            *v = default;
        }
    });
}

/// The factory settings, for the sliders' double-click reset.
fn defaults() -> &'static Prefs {
    static D: std::sync::LazyLock<Prefs> = std::sync::LazyLock::new(Prefs::default);
    &D
}

impl App {
    /// Open the Preferences window at a category.
    pub fn open_prefs(&mut self, category: &str) {
        let i = CATEGORIES
            .iter()
            .position(|c| c.eq_ignore_ascii_case(category))
            .unwrap_or(0);
        self.prefs_before = Some(self.prefs.clone());
        self.prefs_edit = Some((self.prefs.clone(), i, String::new()));
    }

    pub fn prefs_window(&mut self, ctx: &egui::Context) {
        // the older single Preferences dialog (opened by "Audio Hardware..." buttons) is now the
        // Audio Hardware page of this window
        if matches!(self.dialog, Some(Dialog::Preferences)) {
            self.dialog = None;
            self.open_prefs("Audio Hardware");
        }
        let Some((mut p, mut cat, mut query)) = self.prefs_edit.take() else {
            return;
        };
        let (mut ok, mut cancel, mut reset) = (false, false, false);
        let mut open = true;
        egui::Window::new("Preferences")
            .id(egui::Id::new("preferences_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.set_width(740.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(196.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut query)
                                .hint_text("\u{1f50d} Search Preferences")
                                .desired_width(190.0),
                        );
                        ui.add_space(4.0);
                        egui::Frame::new()
                            .fill(theme::panel_header())
                            .corner_radius(crate::theme::ROUND)
                            .inner_margin(4)
                            .show(ui, |ui| {
                                ui.set_width(186.0);
                                ui.set_min_height(420.0);
                                ui.spacing_mut().item_spacing.y = 1.0;
                                let q = query.trim().to_lowercase();
                                if q.is_empty() {
                                    for (i, c) in CATEGORIES.iter().enumerate() {
                                        if ui
                                            .add(
                                                egui::Button::selectable(i == cat, *c)
                                                    .min_size(vec2(182.0, 22.0)),
                                            )
                                            .clicked()
                                        {
                                            cat = i;
                                        }
                                    }
                                } else {
                                    let found: Vec<&(&str, &str)> = SEARCH
                                        .iter()
                                        .filter(|(c, l)| {
                                            l.to_lowercase().contains(&q)
                                                || c.to_lowercase().contains(&q)
                                        })
                                        .collect();
                                    if found.is_empty() {
                                        note(ui, "No settings found");
                                    }
                                    for (c, l) in found {
                                        if ui
                                            .add(
                                                egui::Button::selectable(false, *l)
                                                    .min_size(vec2(182.0, 20.0)),
                                            )
                                            .on_hover_text(*c)
                                            .clicked()
                                        {
                                            cat =
                                                CATEGORIES.iter().position(|x| x == c).unwrap_or(0);
                                            query.clear();
                                        }
                                        note(ui, c);
                                    }
                                }
                            });
                    });
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.set_width(520.0);
                        ui.label(
                            RichText::new(CATEGORIES[cat.min(CATEGORIES.len() - 1)])
                                .size(15.0)
                                .strong()
                                .color(theme::strong()),
                        );
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .id_salt(("prefs_page", cat))
                            .max_height(420.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                self.prefs_page(ui, &mut p, cat);
                            });
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if crate::theme::dialog_button(ui, "Reset Preferences", false)
                        .on_hover_text("Every preference back to its default (Cancel undoes it)")
                        .clicked()
                    {
                        reset = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::theme::dialog_button(ui, "OK", true).clicked() {
                            ok = true;
                        }
                        if crate::theme::dialog_button(ui, "Cancel", false).clicked() {
                            cancel = true;
                        }
                        ui.add_space(12.0);
                        if ui
                            .add_enabled(cat + 1 < CATEGORIES.len(), egui::Button::new("Next"))
                            .clicked()
                        {
                            cat += 1;
                        }
                        if ui
                            .add_enabled(cat > 0, egui::Button::new("Previous"))
                            .clicked()
                        {
                            cat -= 1;
                        }
                    });
                });
            });
        if reset {
            p = Prefs::default();
        }
        if ok {
            self.prefs_before = None;
            self.apply_prefs(p);
            return;
        }
        if cancel || !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            // Cancel: undo the changes that were already showing
            if let Some(before) = self.prefs_before.take() {
                if before != self.prefs {
                    self.apply_prefs_live(before);
                }
            }
            self.device_lists = None;
            return;
        }
        // Every change shows at once. The interface size waits until the mouse button is let go,
        // so the window doesn't rescale under a slider that is still being dragged.
        let mut live = p.clone();
        if ctx.input(|i| i.pointer.any_down()) {
            live.ui_scale = self.prefs.ui_scale;
        }
        if live != self.prefs {
            self.apply_prefs_live(live);
        }
        self.prefs_edit = Some((p, cat, query));
    }

    /// Preferences the interface itself follows (interface size, tool tips); applied when they
    /// change, so Ctrl+wheel/keyboard zoom in between is not fought every frame.
    pub fn apply_ui_prefs(&self, ctx: &egui::Context) {
        let key = egui::Id::new("ac_applied_ui_prefs");
        let want = (
            self.prefs.ui_scale.clamp(0.75, 2.0),
            self.prefs.show_tooltips,
        );
        if ctx.data(|d| d.get_temp::<(f32, bool)>(key)) == Some(want) {
            return;
        }
        ctx.data_mut(|d| d.insert_temp(key, want));
        if (ctx.zoom_factor() - want.0).abs() > 0.001 {
            ctx.set_zoom_factor(want.0);
        }
        ctx.all_styles_mut(|s| s.interaction.tooltip_delay = if want.1 { 0.5 } else { f32::MAX });
    }

    /// OK: keep the new preferences, use them and save them.
    pub fn apply_prefs(&mut self, p: Prefs) {
        self.apply_prefs_live(p);
        prefs::save(&self.prefs);
        self.device_lists = None;
        self.status = "Preferences saved".into();
    }

    /// Use preferences right away (while the Preferences window is open), without saving them.
    pub fn apply_prefs_live(&mut self, p: Prefs) {
        let theme_changed = p.color_theme != theme::theme_kind()
            || p.brightness != self.prefs.brightness
            || p.saturation != self.prefs.saturation;
        theme::set_adjust(p.brightness, p.saturation);
        theme::set_snap_override(p.snap_line_color);
        self.engine.lock().out_map = [
            p.map_left.max(1) as usize - 1,
            p.map_right.max(1) as usize - 1,
        ];
        if p.video_resolution != crate::video::preview_res() {
            crate::video::set_preview_res(p.video_resolution);
            // the playing picture and the paused frame are decoded again at the new size
            self.extra.play_stream = None;
            if let Some(s) = &mut self.extra.seeker {
                s.asked = None;
            }
        }
        self.return_on_stop = p.return_on_stop;
        prefs::set_live(&p);
        theme::set_theme(p.color_theme);
        if theme_changed {
            if let Some(ctx) = self.engine.ctx.as_ref() {
                theme::setup(ctx);
            }
            self.status = format!("Theme: {}", theme::theme_name(p.color_theme as usize));
        }
        if p.video_decoder_exe != self.prefs.video_decoder_exe {
            crate::ffmpeg::set_selected(Some(std::path::PathBuf::from(&p.video_decoder_exe)));
        }
        self.time_format = crate::app::TimeFormat::from_index(p.time_format);
        self.prefs = p;
    }

    fn prefs_page(&mut self, ui: &mut egui::Ui, p: &mut Prefs, cat: usize) {
        let name = CATEGORIES.get(cat).copied().unwrap_or("General");
        // settings on this page that are saved but that nothing in this version reads yet
        if let Some((_, list)) = NOT_CONNECTED.iter().find(|(c, _)| *c == name) {
            ui.label(
                RichText::new(format!("Not connected yet in this version: {list}."))
                    .small()
                    .color(theme::text_dim()),
            );
            ui.add_space(4.0);
        }
        match name {
            "General" => {
                ui.checkbox(&mut p.show_tooltips, "Show tool tips");
                slider(
                    ui,
                    &mut p.zoom_factor,
                    defaults().zoom_factor,
                    0.25..=4.0,
                    "Mouse wheel zoom factor",
                    "x",
                );
                ui.horizontal(|ui| {
                    ui.add_sized(
                        vec2(190.0, 18.0),
                        egui::Label::new("Auto-scroll during playback"),
                    );
                    egui::ComboBox::from_id_salt("pf_scroll")
                        .selected_text(
                            ["Page by page", "Centered", "Off"][p.auto_scroll_mode.min(2) as usize],
                        )
                        .show_ui(ui, |ui| {
                            for (i, n) in ["Page by page", "Centered", "Off"].iter().enumerate() {
                                ui.selectable_value(&mut p.auto_scroll_mode, i as u8, *n);
                            }
                        });
                });
                ui.checkbox(
                    &mut p.return_on_stop,
                    "Return playhead to start position on stop",
                );
                ui.checkbox(
                    &mut p.open_last_session,
                    "Open the last session on start-up",
                );
                // Where new files and sessions are saved by default: a default location,
                // or the folder of the first imported file / the last file saved.
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Default save location"));
                    let names = [
                        "A default location",
                        "Where the first imported file is",
                        "Where the last file was saved",
                    ];
                    egui::ComboBox::from_id_salt("pf_dsv")
                        .selected_text(names[p.default_save_mode.min(2) as usize])
                        .show_ui(ui, |ui| {
                            for (i, n) in names.iter().enumerate() {
                                ui.selectable_value(&mut p.default_save_mode, i as u8, *n);
                            }
                        });
                });
                if p.default_save_mode == 0 {
                    ui.horizontal(|ui| {
                        ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Location"));
                        ui.add(
                            egui::TextEdit::singleline(&mut p.default_save_folder)
                                .hint_text("(Documents/AudioCraft)")
                                .desired_width(220.0),
                        );
                        if ui.button("Browse...").clicked() {
                            if let Some(d) = rfd::FileDialog::new().ask_folder() {
                                p.default_save_folder = d.to_string_lossy().to_string();
                            }
                        }
                    });
                }
                note(ui, "Where new files and sessions are saved by default.");
            }
            "Appearance" => {
                slider(
                    ui,
                    &mut p.brightness,
                    defaults().brightness,
                    -1.0..=1.0,
                    "Brightness",
                    "",
                );
                slider(
                    ui,
                    &mut p.ui_scale,
                    defaults().ui_scale,
                    0.75..=2.0,
                    "Interface size",
                    "x",
                );
                slider(
                    ui,
                    &mut p.saturation,
                    defaults().saturation,
                    0.0..=1.5,
                    "Saturation",
                    "",
                );
                // PhotoCraft-style theme picker: a small preview card per theme, the dark ones
                // and the light ones in their own rows; a click switches at once
                section(ui, "Theme");
                for (title, dark) in [("Dark themes", true), ("Light themes", false)] {
                    ui.label(RichText::new(title).color(theme::text_dim()).small());
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                        for i in 0..theme::theme_count() {
                            if theme::theme_is_dark(i) != dark {
                                continue;
                            }
                            if theme::theme_card(ui, i, p.color_theme as usize == i).clicked() {
                                p.color_theme = i as u8;
                            }
                        }
                    });
                    ui.add_space(6.0);
                }
                if theme::theme_low_contrast(p.color_theme as usize) {
                    let (body, _) = theme::theme_contrast(p.color_theme as usize);
                    ui.label(
                        RichText::new(format!(
                            "\u{25cf} This theme has lower contrast text ({body:.1}:1). It stays readable; pick another theme if text looks faint."
                        ))
                        .small()
                        .color(Color32::from_rgb(232, 176, 70)),
                    );
                }
                note(
                    ui,
                    "AudioCraft is our usual look and Audition Dark a deeper charcoal; the others come from PhotoCraft and VectorCraft. Every change shows at once; Cancel puts the old look back. Ctrl+= and Ctrl+- zoom too.",
                );
            }
            "Audio Channel Mapping" => {
                let outs = self.engine.lock().out_channels.max(1);
                let ins = self.engine.input_channel_count();
                note(
                    ui,
                    &format!(
                        "Your output device has {outs} channel{}, your input device {ins}.",
                        if outs == 1 { "" } else { "s" }
                    ),
                );
                section(ui, "Master output");
                note(ui, "The outputs the Master's Left and Right play on.");
                let top = outs.clamp(2, 64) as u8;
                for (lbl, v) in [
                    ("Left channel", &mut p.map_left),
                    ("Right channel", &mut p.map_right),
                ] {
                    ui.horizontal(|ui| {
                        ui.add_sized(vec2(190.0, 18.0), egui::Label::new(lbl));
                        ui.add(egui::DragValue::new(v).range(1..=top).prefix("Output "));
                    });
                }
                section(ui, "Tracks");
                note(
                    ui,
                    "Each track records from the input in its input menu (a stereo pair or one mono input) and plays through the Master, or straight to an output pair of its own (its output menu > Hardware output: live playback only, mixdowns and exports keep every track).",
                );
            }
            "Audio Hardware" => self.prefs_audio_hardware(ui, p),
            "Auto Save" => {
                note(
                    ui,
                    "Backups go to a Backups folder beside the session (or into AudioCraft's own folder for a session never saved), as name_backup_time.acsession, whenever the session has changed. Audio never saved is left out of a backup.",
                );
                ui.checkbox(&mut p.autosave_on, "Automatically save backup copies every");
                ui.add_enabled_ui(p.autosave_on, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.add(
                            egui::DragValue::new(&mut p.autosave_minutes)
                                .range(1..=120)
                                .suffix(" minutes"),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.label("Keep at most");
                        ui.add(
                            egui::DragValue::new(&mut p.autosave_keep)
                                .range(1..=100)
                                .suffix(" backups"),
                        );
                    });
                });
            }
            "Control Surface" => {
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Device class"));
                    egui::ComboBox::from_id_salt("pf_cs")
                        .selected_text(&p.control_surface)
                        .show_ui(ui, |ui| {
                            for n in ["None", "Mackie Control", "Generic MIDI"] {
                                ui.selectable_value(&mut p.control_surface, n.to_string(), n);
                            }
                        });
                });
                note(ui, "No control surface is connected.");
                self.mcp_prefs(ui, p);
            }
            "Data" => {
                ui.checkbox(
                    &mut p.dither_on,
                    "Dither when converting to a lower bit depth",
                );
                note(ui, "Triangular dither added when WAV, AIFF or FLAC is written at 8, 16 or 24 bits.");
                slider(
                    ui,
                    &mut p.dither_depth,
                    defaults().dither_depth,
                    0.5..=2.0,
                    "Dither depth",
                    " bits",
                );
                ui.checkbox(
                    &mut p.interpret_float_clip,
                    "Clip 32-bit float values above 0 dBFS on open",
                );
                ui.checkbox(
                    &mut p.smooth_edit_crossfade,
                    "Smooth delete and cut boundaries",
                );
                slider(
                    ui,
                    &mut p.smooth_delete_ms,
                    defaults().smooth_delete_ms,
                    0.0..=50.0,
                    "Crossfade length",
                    " ms",
                );
            }
            "Effects" => {
                ui.checkbox(
                    &mut p.fx_parallel,
                    "Process effects in parallel where possible",
                );
                slider(
                    ui,
                    &mut p.fx_default_mix,
                    defaults().fx_default_mix,
                    0.0..=100.0,
                    "Default mix of new effects",
                    " %",
                );
                ui.checkbox(&mut p.plugin_scan_start, "Scan for plug-ins on start-up");
                note(
                    ui,
                    "Shows the plug-ins found by the last scan as soon as AudioCraft opens, without loading them (that kept opening and closing fast); a plug-in loads when it is first used. Rescan in the effects menus looks for new ones.",
                );
                ui.checkbox(
                    &mut p.dock_plugin_editors,
                    "Open VST3 and CLAP plug-in windows in the Plug-ins panel",
                );
            }
            "Markers & Metadata" => {
                ui.checkbox(&mut p.marker_snap, "Snap to markers");
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("New marker name"));
                    ui.text_edit_singleline(&mut p.marker_name_prefix);
                });
            }
            "Media & Disk Cache" => {
                ui.checkbox(&mut p.save_peak_files, "Save peak files");
                note(
                    ui,
                    "Waveform peaks of opened files are kept in a media cache in the temporary folder, so a file opens faster the next time; the oldest go first when the cache passes its limit. The temporary folder also holds exports in progress and speech takes.",
                );
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Temporary folder"));
                    ui.add(
                        egui::TextEdit::singleline(&mut p.temp_folder)
                            .hint_text("(system default)")
                            .desired_width(220.0),
                    );
                    if ui.button("Browse...").clicked() {
                        if let Some(d) = rfd::FileDialog::new().ask_folder() {
                            p.temp_folder = d.to_string_lossy().to_string();
                        }
                    }
                });
                slider(
                    ui,
                    &mut p.cache_limit_gb,
                    defaults().cache_limit_gb,
                    1.0..=200.0,
                    "Cache limit",
                    " GB",
                );
            }
            "Memory" => {
                section(ui, "Undo and history");
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("History limit"));
                    ui.add(
                        egui::DragValue::new(&mut p.history_limit)
                            .range(10..=100000)
                            .suffix(" steps"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Undo steps"));
                    ui.add(egui::DragValue::new(&mut p.undo_steps).range(1..=100000));
                });
            }
            "Multitrack" => {
                section(ui, "Keyframes and handles");
                note(
                    ui,
                    "Sizes of the keyframes on clip envelopes, automation lanes and fades, and of the curve handles the Direct Selection tool (A) shows.",
                );
                slider(
                    ui,
                    &mut p.keyframe_size,
                    defaults().keyframe_size,
                    2.0..=16.0,
                    "Keyframe size",
                    " px",
                );
                slider(
                    ui,
                    &mut p.keyframe_hover_scale,
                    defaults().keyframe_hover_scale,
                    1.0..=3.0,
                    "Keyframe hover size",
                    "x",
                );
                slider(
                    ui,
                    &mut p.keyframe_hit_radius,
                    defaults().keyframe_hit_radius,
                    3.0..=16.0,
                    "Keyframe grab distance",
                    " px",
                );
                slider(
                    ui,
                    &mut p.handle_size,
                    defaults().handle_size,
                    2.0..=14.0,
                    "Handle size",
                    " px",
                );
                slider(
                    ui,
                    &mut p.handle_line,
                    defaults().handle_line,
                    0.5..=4.0,
                    "Handle line width",
                    " px",
                );
                ui.horizontal(|ui| {
                    ui.add_sized(
                        vec2(190.0, 18.0),
                        egui::Label::new("Selected keyframe colour"),
                    );
                    let mut c = p.keyframe_selected_color;
                    if theme::colour_button(ui, "keyframe_selected_colour", &mut c) {
                        p.keyframe_selected_color = c;
                    }
                });
                // a live preview of the sizes
                let (r, _) = ui.allocate_exact_size(vec2(300.0, 46.0), egui::Sense::hover());
                let pnt = ui.painter();
                pnt.rect_filled(r, 0.0, theme::auto_lane());
                let y = r.center().y;
                let pts = [
                    egui::pos2(r.left() + 30.0, y + 10.0),
                    egui::pos2(r.left() + 110.0, y - 8.0),
                    egui::pos2(r.left() + 190.0, y + 6.0),
                    egui::pos2(r.left() + 270.0, y - 4.0),
                ];
                pnt.add(egui::Shape::line(
                    pts.to_vec(),
                    egui::Stroke::new(1.2, crate::ui_multi::VOL_COL),
                ));
                let sel = egui::Color32::from_rgb(
                    p.keyframe_selected_color[0],
                    p.keyframe_selected_color[1],
                    p.keyframe_selected_color[2],
                );
                let hs = p.handle_size.clamp(2.0, 14.0);
                pnt.line_segment(
                    [pts[2] - vec2(26.0, -6.0), pts[2] + vec2(26.0, -6.0)],
                    egui::Stroke::new(p.handle_line.clamp(0.5, 4.0), crate::ui_multi::VOL_COL),
                );
                for h in [pts[2] - vec2(26.0, -6.0), pts[2] + vec2(26.0, -6.0)] {
                    pnt.circle(
                        h,
                        hs * 0.5,
                        crate::theme::gray(20),
                        egui::Stroke::new(1.0, crate::ui_multi::VOL_COL),
                    );
                }
                for (i, q) in pts.iter().enumerate() {
                    let ks = p.keyframe_size.clamp(2.0, 16.0);
                    crate::keys::paint_key(
                        pnt,
                        *q,
                        crate::ui_multi::VOL_COL,
                        if i == 1 {
                            ks * p.keyframe_hover_scale.clamp(1.0, 3.0)
                        } else {
                            ks
                        },
                        i == 1,
                        i == 2,
                        i == 2,
                        sel,
                        true,
                    );
                }
                note(
                    ui,
                    "Normal \u{2022} under the pointer \u{2022} selected with handles \u{2022} normal",
                );
                section(ui, "Tracks");
                slider(
                    ui,
                    &mut p.default_track_height,
                    defaults().default_track_height,
                    40.0..=300.0,
                    "Default track height",
                    " px",
                );
                slider(
                    ui,
                    &mut p.track_io_button_height,
                    defaults().track_io_button_height,
                    14.0..=40.0,
                    "Track Input/Output button height",
                    " px",
                );
                slider(
                    ui,
                    &mut p.default_lane_height,
                    defaults().default_lane_height,
                    40.0..=400.0,
                    "Automation lane height",
                    " px",
                );
                slider(
                    ui,
                    &mut p.snapping_distance_px,
                    defaults().snapping_distance_px,
                    0.0..=100.0,
                    "Snapping distance",
                    " px",
                );
                note(
                    ui,
                    "How far apart two clips have to be before a drag with Snapping on snaps them together.",
                );
                // the snapping line's colour: the theme's contrast colour until picked
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Snapping line colour"));
                    let t = theme::theme_snap_line();
                    let mut c = p.snap_line_color.unwrap_or([t.r(), t.g(), t.b()]);
                    if theme::colour_button(ui, "snap_line_colour", &mut c) {
                        p.snap_line_color = Some(c);
                    }
                    if p.snap_line_color.is_some() {
                        if ui
                            .small_button("Theme colour")
                            .on_hover_text("Go back to the theme's contrast colour")
                            .clicked()
                        {
                            p.snap_line_color = None;
                        }
                    } else {
                        ui.label(egui::RichText::new("theme").small().color(theme::text_dim()));
                    }
                });
                section(ui, "Snapping targets");
                ui.checkbox(
                    &mut p.snap_time_display,
                    "Time display (the ruler's labelled marks)",
                );
                ui.checkbox(&mut p.snap_clip_starts, "Clip starts");
                ui.checkbox(&mut p.snap_clip_ends, "Clip ends");
                ui.checkbox(&mut p.marker_snap, "Markers");
                ui.checkbox(&mut p.snap_playhead, "Playhead");
                ui.checkbox(&mut p.snap_session_start, "Session start (0:00)");
                section(ui, "Pre-render");
                ui.checkbox(
                    &mut p.pre_render_tracks,
                    "Pre-render track effects by default",
                );
                slider(
                    ui,
                    &mut p.prerender_cap_mb,
                    defaults().prerender_cap_mb,
                    16.0..=16384.0,
                    "Pre-render cache cap",
                    " MB",
                );
                slider(
                    ui,
                    &mut p.prerender_ahead_s,
                    defaults().prerender_ahead_s,
                    1.0..=600.0,
                    "Render ahead of the playhead",
                    " s",
                );
                slider(
                    ui,
                    &mut p.prerender_behind_s,
                    defaults().prerender_behind_s,
                    0.0..=600.0,
                    "Keep behind the playhead",
                    " s",
                );
                slider(
                    ui,
                    &mut p.prerender_max_tail_s,
                    defaults().prerender_max_tail_s,
                    0.0..=120.0,
                    "Longest effect tail",
                    " s",
                );
                note(
                    ui,
                    "Pre-rendered tracks share the cache cap. When a track's whole render fits its share, the whole session is rendered; otherwise the render follows the playhead, this far ahead and behind. Each render continues past the last clip until its effects (a reverb tail) have died away, up to the longest tail.",
                );
                section(ui, "Audition compatibility (.sesx)");
                ui.checkbox(
                    &mut p.warn_sesx_compat,
                    "Warn when a .sesx save cannot hold curves or shaped fades",
                );
                slider(
                    ui,
                    &mut p.sesx_bake_tolerance_db,
                    defaults().sesx_bake_tolerance_db,
                    0.05..=2.0,
                    "Conversion accuracy",
                    " dB",
                );
                note(
                    ui,
                    "When curves are converted for Audition, the keyframes written stay within this many dB of the curve (smaller = more keyframes).",
                );
            }
            "Multitrack Clips" => {
                ui.checkbox(
                    &mut p.auto_crossfade,
                    "Create crossfades automatically where clips overlap",
                );
                ui.horizontal(|ui| {
                    ui.add_sized(
                        vec2(190.0, 18.0),
                        egui::Label::new("Default crossfade curve"),
                    );
                    egui::ComboBox::from_id_salt("pf_xf")
                        .selected_text(
                            prefs::CROSSFADE_CURVES[p.default_crossfade_curve.min(2) as usize],
                        )
                        .show_ui(ui, |ui| {
                            for (i, n) in prefs::CROSSFADE_CURVES.iter().enumerate() {
                                ui.selectable_value(&mut p.default_crossfade_curve, i as u8, *n);
                            }
                        });
                });
                ui.checkbox(
                    &mut p.clip_fade_points_smooth,
                    "Smooth curves through fade points",
                );
                ui.checkbox(&mut p.play_overlap, "Play overlapped portions of clips");
                note(
                    ui,
                    "Click a fade or crossfade line to add a point; drag points to shape it, double-click one to remove it.",
                );
            }
            "Playback and Recording" => {
                slider(
                    ui,
                    &mut p.preroll_s,
                    defaults().preroll_s,
                    0.0..=10.0,
                    "Pre-roll",
                    " s",
                );
                slider(
                    ui,
                    &mut p.postroll_s,
                    defaults().postroll_s,
                    0.0..=10.0,
                    "Post-roll",
                    " s",
                );
                ui.checkbox(
                    &mut p.record_punch,
                    "Punch in on the time selection when recording",
                );
                ui.checkbox(
                    &mut p.stop_at_end,
                    "Stop playback at the end (Multitrack: after the last clip, once effect tails have died away)",
                );
                note(
                    ui,
                    "Pre-roll and post-roll: Play with Pre-roll and Post-roll (Ctrl+Shift+Space, or from the Command Palette), and around a punch-in. Punch in: with a time selection, Record starts the pre-roll before it, keeps only the selection, and stops after the post-roll.",
                );
                section(ui, "Recordings Folder");
                ui.horizontal(|ui| {
                    let shown = p.recordings_dir().display().to_string();
                    ui.add_sized(
                        vec2(330.0, 20.0),
                        egui::Label::new(egui::RichText::new(shown).monospace()).truncate(),
                    );
                    if ui.button("Browse...").clicked() {
                        if let Some(d) = rfd::FileDialog::new()
                            .set_directory(p.recordings_dir())
                            .ask_folder()
                        {
                            p.recordings_folder = d.display().to_string();
                        }
                    }
                    if ui
                        .add_enabled(
                            !p.recordings_folder.is_empty(),
                            egui::Button::new("Default"),
                        )
                        .clicked()
                    {
                        p.recordings_folder.clear();
                    }
                });
                note(
                    ui,
                    "Every take is saved here as a WAV file as soon as recording stops, so nothing recorded is lost even if it is never saved again.",
                );
            }
            "Spectral Displays" => {
                section(ui, "Spectral Frequency Display");
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Windowing function"));
                    egui::ComboBox::from_id_salt("pf_win")
                        .selected_text(prefs::SPEC_WINDOWS[p.spec_window.min(4) as usize])
                        .show_ui(ui, |ui| {
                            for (i, n) in prefs::SPEC_WINDOWS.iter().enumerate() {
                                ui.selectable_value(&mut p.spec_window, i as u8, *n);
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Resolution"));
                    egui::ComboBox::from_id_salt("pf_res")
                        .selected_text(format!("{} bands", p.spec_resolution / 2))
                        .show_ui(ui, |ui| {
                            for r in prefs::SPEC_RESOLUTIONS {
                                ui.selectable_value(
                                    &mut p.spec_resolution,
                                    r,
                                    format!("{} bands", r / 2),
                                );
                            }
                        });
                });
                slider(
                    ui,
                    &mut p.spec_db_range,
                    defaults().spec_db_range,
                    40.0..=200.0,
                    "Decibel range",
                    " dB",
                );
                ui.checkbox(&mut p.spec_log_scale, "Logarithmic frequency scale");
                ui.checkbox(
                    &mut p.spec_move_keep_octaves,
                    "Keep octaves when moving a selection",
                );
                note(
                    ui,
                    "Off (default): a moved spectral selection keeps the frequency range it covers (60-1000 Hz is 940 Hz wide wherever it goes), so on the logarithmic scale it gets thinner as it goes up and taller as it comes down. On: it keeps its octaves instead (the Hz range grows as it goes up).",
                );
                section(ui, "Spectral Pitch Display");
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Resolution"));
                    egui::ComboBox::from_id_salt("pf_pres")
                        .selected_text(format!("{} bands", p.pitch_resolution / 2))
                        .show_ui(ui, |ui| {
                            for r in prefs::PITCH_RESOLUTIONS {
                                ui.selectable_value(
                                    &mut p.pitch_resolution,
                                    r,
                                    format!("{} bands", r / 2),
                                );
                            }
                        });
                });
                slider(
                    ui,
                    &mut p.pitch_db_range,
                    defaults().pitch_db_range,
                    30.0..=160.0,
                    "Decibel range",
                    " dB",
                );
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Pitch display range"));
                    ui.add(
                        egui::DragValue::new(&mut p.pitch_min_hz)
                            .range(20.0..=2000.0)
                            .suffix(" Hz"),
                    );
                    ui.label("to");
                    ui.add(
                        egui::DragValue::new(&mut p.pitch_max_hz)
                            .range(60.0..=8000.0)
                            .suffix(" Hz"),
                    );
                });
                if p.pitch_max_hz < p.pitch_min_hz * 2.0 {
                    p.pitch_max_hz = p.pitch_min_hz * 2.0;
                }
            }
            "Time Display" => {
                slider(
                    ui,
                    &mut p.frames_per_second,
                    defaults().frames_per_second,
                    1.0..=120.0,
                    "Frames per second (SMPTE)",
                    " fps",
                );
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Time format"));
                    let all = crate::app::TimeFormat::ALL;
                    let cur = all
                        .get(p.time_format as usize)
                        .map(|f| f.1)
                        .unwrap_or(all[0].1);
                    egui::ComboBox::from_id_salt("pf_tf")
                        .selected_text(cur)
                        .show_ui(ui, |ui| {
                            for (i, (_, n)) in all.iter().enumerate() {
                                ui.selectable_value(&mut p.time_format, i as u8, *n);
                            }
                        });
                });
                note(
                    ui,
                    "Also in View > Time Display; clicking the big time display steps through the formats.",
                );
            }
            "Video" => {
                ui.checkbox(&mut p.video_preview, "Show video preview");
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Preview quality"));
                    egui::ComboBox::from_id_salt("pf_vq")
                        .selected_text(["Low", "Medium", "High"][p.video_quality.min(2) as usize])
                        .show_ui(ui, |ui| {
                            for (i, n) in ["Low", "Medium", "High"].iter().enumerate() {
                                ui.selectable_value(&mut p.video_quality, i as u8, *n);
                            }
                        });
                });
                note(
                    ui,
                    "Quality sets how many frames are cached; Low is the lightest on long clips.",
                );
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Preview resolution"));
                    egui::ComboBox::from_id_salt("pf_vres")
                        .selected_text(crate::video::RES_NAMES[p.video_resolution.min(3) as usize])
                        .show_ui(ui, |ui| {
                            for (i, n) in crate::video::RES_NAMES.iter().enumerate() {
                                ui.selectable_value(&mut p.video_resolution, i as u8, *n);
                            }
                        });
                });
                note(
                    ui,
                    "Fit to panel decodes only as many pixels as the panel shows; Native shows every pixel of the source (heaviest on 4K).",
                );
                section(ui, "Video decoder");
                note(
                    ui,
                    "The decoder reads the picture and sound of imported videos. Pick which one AudioCraft uses; Automatic takes the first one found.",
                );
                if self.extra.decoder_list.is_none() {
                    self.extra.decoder_list = Some(crate::ffmpeg::installed());
                }
                let list = self.extra.decoder_list.clone().unwrap_or_default();
                let auto = p.video_decoder_exe.trim().is_empty();
                if ui
                    .radio(
                        auto,
                        "Automatic (bundled first, then downloaded, then the system's)",
                    )
                    .clicked()
                {
                    p.video_decoder_exe.clear();
                }
                for d in &list {
                    let on = !auto && std::path::Path::new(p.video_decoder_exe.trim()) == d.path;
                    if ui
                        .radio(on, &d.label)
                        .on_hover_text(d.path.display().to_string())
                        .clicked()
                    {
                        p.video_decoder_exe = d.path.to_string_lossy().to_string();
                    }
                }
                let custom = !auto
                    && !list
                        .iter()
                        .any(|d| std::path::Path::new(p.video_decoder_exe.trim()) == d.path);
                ui.horizontal(|ui| {
                    // picking "Other" puts the cursor in the path box
                    let other = ui.radio(custom, "Other:");
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut p.video_decoder_exe)
                            .desired_width(250.0)
                            .hint_text("path to ffmpeg"),
                    );
                    if other.clicked() {
                        field.request_focus();
                    }
                    if ui.button("Browse...").clicked() {
                        if let Some(f) = rfd::FileDialog::new()
                            .add_filter("FFmpeg", &["exe", ""])
                            .ask_file()
                        {
                            p.video_decoder_exe = f.to_string_lossy().to_string();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Test Selected").on_hover_text("Run the chosen decoder once and show its version").clicked() {
                        let exe = crate::video::ensure_decoder(p);
                        self.video_detector_status = match exe {
                            Ok(e) => format!("{} — {}", e.display(), crate::ffmpeg::version(&e)),
                            Err(e) => e,
                        };
                    }
                    if ui.button("Refresh List").clicked() {
                        self.extra.decoder_list = None;
                    }
                    if ui.button("Detect").on_hover_text("Search the folders below, the bundled copy, downloaded builds and PATH").clicked() {
                        self.video_detector_status = match crate::video::configure_decoder(p) {
                            Ok(s) => s,
                            Err(e) => e,
                        };
                    }
                });
                if !self.video_detector_status.is_empty() {
                    note(ui, &self.video_detector_status.clone());
                }
                section(ui, "Download a free decoder");
                note(
                    ui,
                    "Free FFmpeg builds from their maintainers. Each one is installed into AudioCraft's own decoders folder and selected when it finishes.",
                );
                let busy = self.extra.download.is_some();
                for d in crate::ffmpeg::catalogue() {
                    ui.horizontal(|ui| {
                        let installed = list.iter().any(|i| i.label.starts_with(d.name));
                        let label = if installed { "Reinstall" } else { "Download" };
                        if ui
                            .add_enabled(!busy, egui::Button::new(label))
                            .on_hover_text(d.url)
                            .clicked()
                        {
                            self.decoder_download_start(d.id);
                        }
                        ui.label(d.name);
                        ui.label(RichText::new(d.licence).small().color(theme::text_dim()));
                    });
                }
                if busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(&self.extra.download_status).small());
                    });
                } else if !self.extra.download_status.is_empty() {
                    note(ui, &self.extra.download_status.clone());
                    if self.extra.download_status.starts_with("Installed") {
                        self.extra.decoder_list = None;
                        p.video_decoder_exe = self.prefs.video_decoder_exe.clone();
                    }
                }
                note(
                    ui,
                    &format!(
                        "Decoders folder: {}",
                        crate::ffmpeg::install_root().display()
                    ),
                );
                section(ui, "More places to look");
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Decoder folder"));
                    ui.add(
                        egui::TextEdit::singleline(&mut p.video_decoder_folder)
                            .desired_width(230.0),
                    );
                    if ui.button("Browse...").clicked() {
                        if let Some(f) = rfd::FileDialog::new().ask_folder() {
                            p.video_decoder_folder = f.to_string_lossy().to_string();
                        }
                    }
                });
                ui.label("More decoder folders (one per line)");
                ui.add(
                    egui::TextEdit::multiline(&mut p.video_decoder_repos)
                        .desired_rows(2)
                        .desired_width(430.0)
                        .hint_text("C:\\Tools\\ffmpeg\\bin\n/usr/local/bin"),
                );
            }
            _ => {}
        }
    }

    /// Output and input device choosers (they switch at once), the device and rate in use.
    /// Shared by Preferences > Audio Hardware and the Audio Hardware dialog.
    pub fn audio_device_grid(&mut self, ui: &mut egui::Ui) {
        if self.device_lists.is_none() {
            self.device_lists = Some((
                crate::engine::list_devices(false),
                crate::engine::list_devices(true),
            ));
        }
        let (outs, ins) = self.device_lists.clone().unwrap_or_default();
        let def = "(System Default)".to_string();
        let mut out_choice = self
            .engine
            .out_choice
            .clone()
            .unwrap_or_else(|| def.clone());
        let mut in_choice = self.engine.in_choice.clone().unwrap_or_else(|| def.clone());
        egui::Grid::new("prefs_hw")
            .num_columns(2)
            .spacing(vec2(12.0, 6.0))
            .show(ui, |ui| {
                ui.label("Default Output");
                egui::ComboBox::from_id_salt("outdev")
                    .width(260.0)
                    .selected_text(&out_choice)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut out_choice, def.clone(), def.as_str());
                        for d in &outs {
                            ui.selectable_value(&mut out_choice, d.clone(), d);
                        }
                    });
                ui.end_row();
                ui.label("Default Input");
                egui::ComboBox::from_id_salt("indev")
                    .width(260.0)
                    .selected_text(&in_choice)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut in_choice, def.clone(), def.as_str());
                        for d in &ins {
                            ui.selectable_value(&mut in_choice, d.clone(), d);
                        }
                    });
                ui.end_row();
                ui.label("Current Output");
                ui.label(&self.engine.out_device_name);
                ui.end_row();
                ui.label("Output Rate");
                let r = self.engine.lock().out_rate;
                ui.label(format!("{r} Hz"));
                ui.end_row();
            });
        let oc = if out_choice == def {
            None
        } else {
            Some(out_choice)
        };
        if oc != self.engine.out_choice {
            self.engine.out_choice = oc;
            self.engine.restart_output();
        }
        let ic = if in_choice == def {
            None
        } else {
            Some(in_choice)
        };
        if ic != self.engine.in_choice {
            self.engine.in_choice = ic;
            self.engine.restart_input();
        }
    }

    /// Audio devices: these switch at once (not on OK), like the original's Audio Hardware page.
    fn prefs_audio_hardware(&mut self, ui: &mut egui::Ui, p: &mut Prefs) {
        self.audio_device_grid(ui);
        section(ui, "Playback");
        ui.checkbox(
            &mut p.return_on_stop,
            "Return playhead to start position on stop",
        );
        if ui
            .checkbox(&mut self.auto_scroll, "Auto-scroll during playback")
            .changed()
        {}
    }
}

impl App {
    /// Preferences > Control Surface > MCP: let AI assistants connect, with the guardrails.
    fn mcp_prefs(&mut self, ui: &mut egui::Ui, p: &mut Prefs) {
        section(ui, "MCP (AI assistants)");
        note(
            ui,
            "AI assistants that speak the Model Context Protocol can look at your session and, if you allow it, play, navigate and edit. Only programs on this computer that know the access token can connect.",
        );
        if ui
            .checkbox(&mut p.mcp_enabled, "Allow AI assistants to connect (MCP server)")
            .changed()
            && p.mcp_enabled
            && p.mcp_token.len() < 16
        {
            p.mcp_token = crate::mcp::new_token();
        }
        ui.add_enabled_ui(p.mcp_enabled, |ui| {
            ui.horizontal(|ui| {
                ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Port (this computer only)"));
                ui.add(egui::DragValue::new(&mut p.mcp_port).range(1024..=65535));
            });
            ui.horizontal(|ui| {
                ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Access token"));
                let shown = if p.mcp_token.len() > 8 {
                    format!("{}\u{2026}", &p.mcp_token[..8])
                } else {
                    "(none yet)".to_string()
                };
                ui.label(egui::RichText::new(shown).monospace().color(theme::text_dim()));
                if ui.small_button("Copy").on_hover_text("Copy the whole token").clicked() {
                    ui.ctx().copy_text(p.mcp_token.clone());
                }
                if ui
                    .small_button("New token")
                    .on_hover_text("Make a new token: assistants using the old one are locked out")
                    .clicked()
                {
                    p.mcp_token = crate::mcp::new_token();
                }
            });
            ui.horizontal(|ui| {
                ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Assistants may"));
                let cur = p.mcp_level.min(3) as usize;
                egui::ComboBox::from_id_salt("pf_mcp_level")
                    .selected_text(crate::mcp::LEVELS[cur].0)
                    .show_ui(ui, |ui| {
                        for (i, (name, tip)) in crate::mcp::LEVELS.iter().enumerate() {
                            ui.selectable_value(&mut p.mcp_level, i as u8, *name)
                                .on_hover_text(*tip);
                        }
                    });
            });
            note(ui, crate::mcp::LEVELS[p.mcp_level.min(3) as usize].1);
            ui.checkbox(&mut p.mcp_confirm, "Ask me before each change");
            let mut rate = p.mcp_rate_per_min as f32;
            slider(ui, &mut rate, defaults().mcp_rate_per_min as f32, 5.0..=600.0, "Most requests a minute", "");
            p.mcp_rate_per_min = rate.round().max(1.0) as u32;
            note(
                ui,
                "Never available to assistants: quitting, anything that opens a dialog or another program, plug-in scans, choosing where files are saved. Every edit can be undone with Ctrl+Z.",
            );
            ui.horizontal(|ui| {
                ui.add_sized(vec2(190.0, 18.0), egui::Label::new("Status"));
                let st = if self.mcp.status.is_empty() { "Off" } else { self.mcp.status.as_str() };
                ui.label(egui::RichText::new(st).color(theme::text_dim()));
            });
            ui.horizontal(|ui| {
                if ui
                    .button("Copy assistant setup")
                    .on_hover_text("Settings for an assistant that starts a program (e.g. a desktop chat app): it runs AudioCraft's --mcp-stdio bridge")
                    .clicked()
                {
                    ui.ctx().copy_text(self.mcp_client_config());
                }
                if ui
                    .button("Copy address")
                    .on_hover_text("For assistants that connect over HTTP: send the token as \"Authorization: Bearer <token>\"")
                    .clicked()
                {
                    ui.ctx().copy_text(format!("http://127.0.0.1:{}/mcp", p.mcp_port));
                }
            });
            if !self.mcp.log.is_empty() {
                ui.label(egui::RichText::new("Recent activity").small().color(theme::text_dim()));
                egui::ScrollArea::vertical()
                    .id_salt("mcp_log")
                    .max_height(110.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for l in self.mcp.log.iter().rev().take(40).collect::<Vec<_>>().into_iter().rev() {
                            ui.label(egui::RichText::new(l).small().monospace());
                        }
                    });
            }
        });
    }
}
