//! Side and bottom panels: Files, History, Media Browser, Effects Rack, Markers,
//! Properties, Essential Sound, Levels and Selection/View.

use crate::app::*;
use crate::audio;
use crate::dsp::{self, Effect, EffectSlot, FadeCurve, FilterKind};
use crate::theme::{self, Icon, icon_button};
use egui::{Color32, FontId, Rect, RichText, Sense, pos2, vec2};
use std::path::PathBuf;

pub const FAVORITES: &[&str] = &[
    "Fade In",
    "Fade Out",
    "Normalize to -0.1 dB",
    "Normalize to -3 dB",
    "Remove DC Offset",
    "Reduce Rumble",
    "Telephone Voice",
    "Podcast Voice",
    "Light Reverb",
    "Invert",
    "Reverse",
    "Silence",
];

/// A command from the registry, drawn with the same plain menu item style as the menu bar.
/// The Files panel's right-click menu used to use a dialog button here, which looked foreign.
fn file_menu_cmd(ui: &mut egui::Ui, id: &str, label: &str) -> egui::Response {
    let cmd = crate::commands::find(id);
    let shortcut = cmd.map(|c| c.shortcut).unwrap_or("");
    let doc = cmd.map(|c| c.doc).unwrap_or("");
    crate::theme::menu_button(ui, true, label, shortcut, doc)
}

pub fn time_drag(
    ui: &mut egui::Ui,
    v: &mut f64,
    fmt: TimeFormat,
    rate: u32,
    enabled: bool,
) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::DragValue::new(v)
            .speed(0.01)
            .range(0.0..=1.0e7)
            .custom_formatter(move |x, _| fmt.format(x, rate))
            .custom_parser(move |s| fmt.parse(s, rate)),
    )
}

pub fn eq_preset(i: usize) -> Option<Vec<dsp::EqBand>> {
    use dsp::EqBand as B;
    let b = |kind, freq, gain_db, q| B {
        kind,
        freq,
        gain_db,
        q,
        on: true,
    };
    Some(match i {
        0 => vec![
            b(FilterKind::HighPass, 80.0, 0.0, 0.707),
            b(FilterKind::Peak, 250.0, -2.0, 1.0),
            b(FilterKind::Peak, 3500.0, 3.0, 0.9),
            b(FilterKind::HighShelf, 10000.0, 2.0, 0.707),
        ],
        1 => vec![
            b(FilterKind::LowShelf, 180.0, 3.0, 0.707),
            b(FilterKind::HighShelf, 7000.0, -2.5, 0.707),
        ],
        2 => vec![
            b(FilterKind::Peak, 300.0, -2.0, 1.0),
            b(FilterKind::HighShelf, 6000.0, 4.0, 0.707),
        ],
        3 => vec![
            b(FilterKind::HighPass, 400.0, 0.0, 0.9),
            b(FilterKind::LowPass, 3200.0, 0.0, 0.9),
            b(FilterKind::Peak, 1500.0, 4.0, 1.0),
        ],
        4 => vec![
            b(FilterKind::HighPass, 120.0, 0.0, 0.8),
            b(FilterKind::Peak, 2500.0, 5.0, 0.8),
            b(FilterKind::LowPass, 9000.0, 0.0, 0.7),
        ],
        _ => return None,
    })
}
pub const EQ_PRESETS: [&str; 5] = ["Podcast Voice", "Warm", "Bright", "Telephone", "Radio"];

impl App {
    // ------------------------------------------------------------ Files

    pub fn files_panel(&mut self, ui: &mut egui::Ui) {
        const FILE_ROW_H: f32 = 17.0;
        let mut tab = self.files_tab;
        theme::tab_header(
            ui,
            &[
                (FilesTab::Files, "Files"),
                (FilesTab::Favorites, "Favorites"),
            ],
            &mut tab,
        );
        self.files_tab = tab;
        if self.files_tab == FilesTab::Favorites {
            egui::Frame::new().inner_margin(6).show(ui, |ui| {
                let wave = self.mode == Mode::Waveform && self.cur().is_some();
                if !wave {
                    ui.label(
                        RichText::new("Open a file in the Waveform editor to apply favorites.")
                            .color(theme::text_dim())
                            .small(),
                    );
                }
                egui::ScrollArea::vertical()
                    .id_salt("favs")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_enabled_ui(wave, |ui| {
                            for f in FAVORITES {
                                if ui
                                    .add(
                                        egui::Button::new(*f)
                                            .min_size(vec2(ui.available_width(), 20.0)),
                                    )
                                    .clicked()
                                {
                                    self.apply_favorite(f);
                                }
                            }
                        });
                    });
            });
            return;
        }
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.spacing_mut().item_spacing.x = 1.0;
            if icon_button(ui, Icon::Folder, false, "Open File (Ctrl+O)").clicked() {
                self.open_dialog();
            }
            if icon_button(ui, Icon::NewFile, false, "New Audio File (Ctrl+N)").clicked() {
                self.pending_cmd = Some("new");
            }
            if icon_button(ui, Icon::Insert, false, "Insert into Multitrack").clicked() {
                if let Some(id) = self.selected_file {
                    self.insert_into_multitrack(id);
                }
            }
            if icon_button(ui, Icon::Trash, false, "Close Selected File").clicked() {
                if let Some(id) = self.selected_file {
                    self.request_close_file(id);
                }
            }
        });
        // columns: drag a divider in the header to resize, double-click it to fit the names;
        // wider than the panel, the list scrolls sideways
        const NAMES: [&str; 6] = ["Name", "Status", "Duration", "Sample Rate", "Ch", "Bits"];
        let widths = self.extra.files_cols;
        let total: f32 = widths.iter().sum::<f32>() + 8.0;
        let col_x = |i: usize| -> f32 { 6.0 + widths[..i].iter().sum::<f32>() };
        let mut open_id = None;
        let mut select_id = None;
        let mut select_mods: Option<(u64, bool, bool)> = None;
        let mut insert_id = None;
        let mut close_id = None;
        let mut new_widths = widths;
        let mut fit_name = false;
        // The list shares this panel with the column header (18px) and the Play/Loop/AutoPlay
        // row (22px buttons + 2px gap). Reserving only 30px used to push that bottom row past
        // the panel's clip rect, so it was drawn underneath this window and the panels below.
        let list_h = (ui.available_height() - 44.0).max(20.0);
        egui::ScrollArea::horizontal().id_salt("files_h").max_height(list_h + 18.0).auto_shrink([false, true]).show(ui, |ui| {
        let row_w = ui.available_width().max(total);
        let (hr, _) = ui.allocate_exact_size(vec2(row_w, 18.0), Sense::hover());
        ui.painter().rect_filled(hr, 0.0, theme::own_or(crate::theme::gray(30), theme::panel_header()));
        for (i, n) in NAMES.iter().enumerate() {
            let x = hr.left() + col_x(i);
            let w = widths[i];
            ui.painter().with_clip_rect(Rect::from_min_max(pos2(x, hr.top()), pos2(x + w - 4.0, hr.bottom()))).text(pos2(x, hr.center().y), egui::Align2::LEFT_CENTER, *n, FontId::proportional(10.5), theme::text_dim());
            // the divider after this column
            let dx = x + w - 3.0;
            let dr = Rect::from_min_max(pos2(dx - 3.0, hr.top()), pos2(dx + 3.0, hr.bottom()));
            let d = ui.interact(dr, ui.scope_id().with(("files_col", i)), Sense::click_and_drag());
            let hot = d.hovered() || d.dragged();
            ui.painter().line_segment([pos2(dx, hr.top() + 3.0), pos2(dx, hr.bottom() - 3.0)], egui::Stroke::new(1.0, if hot { theme::accent() } else { crate::theme::gray(58) }));
            if hot {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
            }
            if d.dragged() {
                new_widths[i] = (new_widths[i] + d.drag_delta().x).clamp(24.0, 1200.0);
            }
            if d.double_clicked() && i == 0 {
                fit_name = true;
            }
            let _ = d.on_hover_text("Drag to resize the column; double-click the Name divider to fit the names");
        }
        egui::ScrollArea::vertical().id_salt("files_list").auto_shrink([false, false]).max_height(list_h).show(ui, |ui| {
            ui.set_min_width(row_w);
            if let Some(s) = &self.session {
                let (r, resp) = ui.allocate_exact_size(vec2(row_w, FILE_ROW_H), Sense::click());
                let on = self.mode == Mode::Multitrack;
                if on {
                    ui.painter().rect_filled(r, 0.0, theme::own_or(crate::theme::gray(44), theme::hover()));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, theme::own_or(crate::theme::gray(45), theme::hover()));
                }
                let name_col = if on { theme::time_blue() } else { theme::text() };
                theme::paint_icon(ui.painter(), Rect::from_min_size(r.min + vec2(4.0, (FILE_ROW_H - 12.0) / 2.0), vec2(12.0, 12.0)), Icon::Multitrack, theme::text());
                ui.painter().text(pos2(r.left() + 22.0, r.center().y), egui::Align2::LEFT_CENTER, format!("{}{}", s.name, if s.dirty { " *" } else { "" }), FontId::proportional(12.0), name_col);
                ui.painter().text(pos2(r.left() + col_x(3), r.center().y), egui::Align2::LEFT_CENTER, format!("{} Hz", s.rate), FontId::proportional(11.0), theme::text_dim());
                let bits = if s.bits == 32 { "32f".to_string() } else { format!("{}", s.bits) };
                ui.painter().text(pos2(r.left() + col_x(5), r.center().y), egui::Align2::LEFT_CENTER, bits, FontId::proportional(11.0), theme::text_dim());
                if resp.clicked() {
                    self.pending_cmd = Some("multi");
                }
            }
            for f in &self.files {
                let (r, resp) = ui.allocate_exact_size(vec2(row_w, FILE_ROW_H), Sense::click_and_drag());
                let is_cur = self.current == Some(f.id) && self.mode == Mode::Waveform;
                let is_sel = self.files_sel.contains(&f.id);
                if is_cur {
                    // the open file: blue name on a quiet row, as in the original's Files panel
                    ui.painter().rect_filled(r, 0.0, theme::own_or(crate::theme::gray(44), theme::hover()));
                } else if is_sel {
                    ui.painter().rect_filled(r, 0.0, theme::own_or(crate::theme::gray(52), theme::pressed()));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, theme::own_or(crate::theme::gray(42), theme::hover()));
                }
                theme::paint_icon(ui.painter(), Rect::from_min_size(r.min + vec2(4.0, (FILE_ROW_H - 12.0) / 2.0), vec2(12.0, 12.0)), Icon::Waveform, theme::wave());
                let vals = [
                    // unsaved changes show as " *" after the name; Status is for file states
                    if f.dirty() { format!("{} *", f.name) } else { f.name.clone() },
                    String::new(),
                    audio::fmt_time(f.data.duration()),
                    format!("{} Hz", f.data.rate),
                    format!("{}", f.data.channels()),
                    if f.float { "32f".into() } else { format!("{}", f.bits) },
                ];
                for (i, v) in vals.iter().enumerate() {
                    let x = r.left() + col_x(i);
                    let tx = if i == 0 { x + 16.0 } else { x };
                    let maxw = widths[i] - if i == 0 { 20.0 } else { 4.0 };
                    let clip = Rect::from_min_max(pos2(tx, r.top()), pos2(tx + maxw, r.bottom()));
                    let shown = if i == 0 { middle_ellipsis(ui.painter(), v, FontId::proportional(12.0), maxw) } else { v.clone() };
                    ui.painter().with_clip_rect(clip).text(
                        pos2(tx, r.center().y),
                        egui::Align2::LEFT_CENTER,
                        shown,
                        FontId::proportional(if i == 0 { 12.0 } else { 11.0 }),
                        if is_cur { theme::time_blue() } else if i == 0 { theme::text() } else { theme::text_dim() },
                    );
                }
                let resp = resp.on_hover_text(f.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| f.name.clone()));
                if resp.clicked() {
                    let mods = ui.input(|i| (i.modifiers.shift, i.modifiers.ctrl || i.modifiers.command));
                    select_mods = Some((f.id, mods.0, mods.1));
                    select_id = Some(f.id);
                }
                if resp.double_clicked() {
                    open_id = Some(f.id);
                }
                if resp.drag_started() {
                    egui::DragAndDrop::set_payload(ui.ctx(), f.id);
                }
                if resp.dragged() {
                    if let Some(pp) = ui.ctx().pointer_hover_pos() {
                        let lp = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("dragfile")));
                        let tr = Rect::from_min_size(pp + vec2(12.0, 4.0), vec2(190.0, 20.0));
                        lp.rect_filled(tr, 0.0, Color32::from_rgba_unmultiplied(40, 110, 190, 220));
                        lp.text(tr.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, &f.name, FontId::proportional(11.5), theme::strong());
                    }
                }
                resp.context_menu(|ui| {
                    if file_menu_cmd(ui, "file.open_selected", "Open in Waveform Editor").clicked() {
                        open_id = Some(f.id);
                        ui.close();
                    }
                    if file_menu_cmd(ui, "file.insert_selected", "Insert into Multitrack").clicked() {
                        insert_id = Some(f.id);
                        ui.close();
                    }
                    ui.separator();
                    if file_menu_cmd(ui, "file.close_selected", "Close Selected File").clicked() {
                        close_id = Some(f.id);
                        ui.close();
                    }
                });
            }
            if self.files.is_empty() && self.session.is_none() {
                ui.add_space(8.0);
                ui.label(RichText::new("  No files open. Drop audio files onto the window,\n  or use File \u{203a} Open.").color(theme::text_dim()).small());
            }
        });
        });
        if fit_name {
            let font = FontId::proportional(12.0);
            let widest = self
                .files
                .iter()
                .map(|f| {
                    ui.painter()
                        .layout_no_wrap(format!("{} *", f.name), font.clone(), Color32::WHITE)
                        .size()
                        .x
                })
                .fold(60.0f32, f32::max);
            new_widths[0] = (widest + 28.0).min(1200.0);
        }
        self.extra.files_cols = new_widths;

        // under the list: play the selected file, loop it, and auto-play files as they are picked
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.add_space(((ui.available_width() - 3.0 * 28.0) / 2.0).max(0.0));
            let playing = self.files_preview.is_some();
            let sel = self.selected_file.or(self.current);
            let tip = if playing {
                "Stop"
            } else {
                "Play the selected file"
            };
            if ui
                .add_enabled_ui(sel.is_some() || playing, |ui| {
                    icon_button(
                        ui,
                        if playing { Icon::Stop } else { Icon::Play },
                        playing,
                        tip,
                    )
                })
                .inner
                .clicked()
            {
                if playing {
                    self.stop_file_preview();
                } else if let Some(id) = sel {
                    self.start_file_preview(id);
                }
            }
            if icon_button(ui, Icon::Loop, self.files_loop, "Loop playback").clicked() {
                self.files_loop = !self.files_loop;
                if let Some(p) = self.engine.lock().files_preview.as_mut() {
                    p.looping = self.files_loop;
                }
            }
            if icon_button(
                ui,
                Icon::AutoPlay,
                self.files_autoplay,
                "Auto-play: a file plays when you select it",
            )
            .clicked()
            {
                self.files_autoplay = !self.files_autoplay;
            }
        });
        if let Some((id, s, c)) = select_mods {
            self.files_select(id, s, c);
        }
        if let Some(id) = select_id {
            if self.files_autoplay {
                self.start_file_preview(id);
            }
        }
        if let Some(id) = open_id {
            self.switch_mode(Mode::Waveform);
            self.current = Some(id);
            self.selected_file = Some(id);
        }
        if let Some(id) = insert_id {
            self.insert_into_multitrack(id);
        }
        if let Some(id) = close_id {
            self.request_close_file(id);
        }
        if let Some(c) = self.pending_cmd.take() {
            let ctx = ui.ctx().clone();
            self.run(c, &ctx);
        }
    }

    // ------------------------------------------------------------ History

    pub fn history_panel(&mut self, ui: &mut egui::Ui) {
        theme::title_header(ui, "History");
        egui::Frame::new().inner_margin(4).show(ui, |ui| {
            let mut goto: Option<usize> = None;
            let multi = self.mode == Mode::Multitrack && self.session.is_some();
            let (entries, pos): (Vec<String>, usize) = if multi {
                (
                    self.session_hist.iter().map(|h| h.0.clone()).collect(),
                    self.session_hist_pos,
                )
            } else {
                match self.cur() {
                    Some(f) => (f.history.iter().map(|h| h.0.clone()).collect(), f.hist_pos),
                    None => (vec![], 0),
                }
            };
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(pos > 0, egui::Button::new("Undo").small())
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(pos + 1 < entries.len(), egui::Button::new("Redo").small())
                    .clicked()
                {
                    self.redo();
                }
                ui.label(
                    RichText::new(format!("{} states", entries.len()))
                        .small()
                        .color(theme::text_dim()),
                );
            });
            // The reference workspace keeps a New Session button under Undo/Redo.
            if ui.add(egui::Button::new("New Session").small()).clicked() {
                let _ = self.exec("file.new_session", ui.ctx());
            }
            egui::ScrollArea::vertical()
                .id_salt("hist")
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, e) in entries.iter().enumerate() {
                        let col = if i > pos {
                            theme::text_dim().gamma_multiply(0.7)
                        } else {
                            theme::text()
                        };
                        let (r, resp) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 17.0), Sense::click());
                        if i == pos {
                            ui.painter().rect_filled(
                                r,
                                0.0,
                                theme::own_or(crate::theme::gray(58), theme::pressed()),
                            );
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                r,
                                0.0,
                                theme::own_or(crate::theme::gray(44), theme::hover()),
                            );
                        }
                        ui.painter().with_clip_rect(r).text(
                            pos2(r.left() + 8.0, r.center().y),
                            egui::Align2::LEFT_CENTER,
                            e,
                            FontId::proportional(12.0),
                            col,
                        );
                        if resp.clicked() {
                            goto = Some(i);
                        }
                    }
                });
            if let Some(i) = goto {
                if multi {
                    self.session_hist_pos = i;
                    self.restore_session_hist();
                } else if let Some(f) = self.cur_mut() {
                    f.goto_history(i);
                }
            }
        });
    }

    // ------------------------------------------------------------ Media Browser

    pub fn media_browser(&mut self, ui: &mut egui::Ui) {
        if self.media_dir.is_none() {
            self.media_dir = std::env::var_os("USERPROFILE")
                .or(std::env::var_os("HOME"))
                .map(PathBuf::from)
                .or_else(|| std::env::current_dir().ok());
        }
        let dir = self.media_dir.clone().unwrap_or_default();
        if self.media_loaded.as_ref() != Some(&dir) {
            let mut v: Vec<(PathBuf, bool)> = std::fs::read_dir(&dir)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .map(|e| {
                            let p = e.path();
                            let is_dir = p.is_dir();
                            (p, is_dir)
                        })
                        .filter(|(p, d)| {
                            *d || audio::is_audio_path(p)
                                || p.extension().map(|e| e == "acsession").unwrap_or(false)
                        })
                        .filter(|(p, _)| {
                            !p.file_name()
                                .map(|n| n.to_string_lossy().starts_with('.'))
                                .unwrap_or(false)
                        })
                        .collect()
                })
                .unwrap_or_default();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.file_name().cmp(&b.0.file_name())));
            self.media_entries = v;
            self.media_loaded = Some(dir.clone());
        }
        let mut go: Option<PathBuf> = None;
        let mut open: Option<PathBuf> = None;
        ui.horizontal(|ui| {
            if icon_button(ui, Icon::Up, false, "Up one folder").clicked() {
                if let Some(p) = dir.parent() {
                    go = Some(p.to_path_buf());
                }
            }
            #[cfg(windows)]
            {
                let drive = dir
                    .components()
                    .next()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("drives")
                    .selected_text(drive)
                    .width(54.0)
                    .show_ui(ui, |ui| {
                        for l in b'A'..=b'Z' {
                            let d = format!("{}:\\", l as char);
                            if std::path::Path::new(&d).exists() && ui.button(&d).clicked() {
                                go = Some(PathBuf::from(d));
                            }
                        }
                    });
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.media_filter)
                    .hint_text("Filter")
                    .desired_width(ui.available_width()),
            );
        });
        ui.label(
            RichText::new(dir.display().to_string())
                .small()
                .color(theme::text_dim()),
        );
        let filter = self.media_filter.to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("media")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (p, is_dir) in &self.media_entries {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                        continue;
                    }
                    let (r, resp) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 19.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, 0.0, crate::theme::gray(45));
                    }
                    let (ic, col) = if *is_dir {
                        (Icon::Folder, Color32::from_rgb(200, 170, 90))
                    } else {
                        (Icon::Waveform, theme::wave())
                    };
                    theme::paint_icon(
                        ui.painter(),
                        Rect::from_min_size(r.min + vec2(2.0, 3.0), vec2(13.0, 13.0)),
                        ic,
                        col,
                    );
                    ui.painter().text(
                        pos2(r.left() + 20.0, r.center().y),
                        egui::Align2::LEFT_CENTER,
                        &name,
                        FontId::proportional(12.0),
                        theme::text(),
                    );
                    if resp.double_clicked() {
                        if *is_dir {
                            go = Some(p.clone());
                        } else {
                            open = Some(p.clone());
                        }
                    }
                }
                if self.media_entries.is_empty() {
                    ui.label(
                        RichText::new("No folders or audio files here.")
                            .small()
                            .color(theme::text_dim()),
                    );
                }
            });
        if let Some(g) = go {
            self.media_dir = Some(g);
        }
        if let Some(o) = open {
            self.switch_mode(Mode::Waveform);
            self.open_path(&o);
        }
    }

    // ------------------------------------------------------------ Effects Rack

    /// Previous simple rack (kept for reference; the panel now uses `rack.rs`).
    #[allow(dead_code)]
    pub fn effects_rack_simple(&mut self, ui: &mut egui::Ui) {
        let multi = self.mode == Mode::Multitrack;
        let (target, mut slots, rate) = if multi {
            match &self.session {
                Some(s) if !s.tracks.is_empty() => {
                    let t = &s.tracks[s.selected_track.min(s.tracks.len() - 1)];
                    (format!("Track: {}", t.name), t.rack.clone(), s.rate)
                }
                _ => {
                    ui.label(RichText::new("No tracks in the session.").color(theme::text_dim()));
                    return;
                }
            }
        } else {
            match self.cur() {
                Some(f) => (format!("File: {}", f.name), f.rack.clone(), f.data.rate),
                None => {
                    ui.label(
                        RichText::new("Open a file to build an effects rack.")
                            .color(theme::text_dim()),
                    );
                    return;
                }
            }
        };
        let orig = slots.clone();
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(RichText::new(target).strong()).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("Add Effect \u{25be}", |ui| {
                    let mut cats: Vec<&'static str> = vec![];
                    for e in Effect::all_defaults() {
                        if !cats.contains(&e.category()) {
                            cats.push(e.category());
                        }
                    }
                    for c in cats {
                        ui.menu_button(c, |ui| {
                            for e in Effect::all_defaults()
                                .into_iter()
                                .filter(|e| e.category() == c)
                            {
                                if ui.button(e.name()).clicked() {
                                    slots.push(EffectSlot::new(e));
                                    ui.close();
                                }
                            }
                        });
                    }
                });
            });
        });
        ui.separator();
        let mut remove: Option<usize> = None;
        let mut swap: Option<(usize, usize)> = None;
        let avail = (ui.available_height() - 36.0).max(40.0);
        egui::ScrollArea::vertical().id_salt("rack").auto_shrink([false, false]).max_height(avail).show(ui, |ui| {
            if slots.is_empty() {
                ui.label(RichText::new("The rack is empty. Use \u{201c}Add Effect\u{201d} to insert effects.\nThey are heard in real time during playback.").small().color(theme::text_dim()));
            }
            let n = slots.len();
            for (i, s) in slots.iter_mut().enumerate() {
                let id = ui.make_persistent_id(("slot", i, s.effect.name()));
                egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
                    .show_header(ui, |ui| {
                        ui.label(RichText::new(format!("{}", i + 1)).color(theme::text_dim()).monospace());
                        if icon_button(ui, Icon::Power, s.enabled, if s.enabled { "Bypass" } else { "Enable" }).clicked() {
                            s.enabled = !s.enabled;
                        }
                        ui.label(RichText::new(s.effect.name()).color(if s.enabled { theme::text() } else { theme::text_dim() }));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("\u{2715}").on_hover_text("Remove").clicked() {
                                remove = Some(i);
                            }
                            if i + 1 < n && ui.small_button("\u{25bc}").on_hover_text("Move down").clicked() {
                                swap = Some((i, i + 1));
                            }
                            if i > 0 && ui.small_button("\u{25b2}").on_hover_text("Move up").clicked() {
                                swap = Some((i, i - 1));
                            }
                        });
                    })
                    .body(|ui| {
                        s.effect.ui(ui, rate as f32);
                        dsp::slider(ui, "Mix (Dry/Wet)", &mut s.mix, 0.0..=1.0, "", false);
                    });
            }
        });
        if let Some(i) = remove {
            slots.remove(i);
        }
        if let Some((a, b)) = swap {
            slots.swap(a, b);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if !multi {
                let can = !slots.is_empty();
                if ui
                    .add_enabled(can, egui::Button::new("Apply"))
                    .on_hover_text(
                        "Process the selection (or whole file) with the rack, then clear it",
                    )
                    .clicked()
                {
                    let rack = slots.clone();
                    self.process("Effects Rack", |d, a, b| dsp::apply_chain(d, a, b, &rack));
                    slots.clear();
                }
            }
            if ui
                .add_enabled(!slots.is_empty(), egui::Button::new("Remove All"))
                .clicked()
            {
                slots.clear();
            }
            ui.label(
                RichText::new(if multi {
                    "Live on this track"
                } else {
                    "Live preview while playing"
                })
                .small()
                .color(theme::text_dim()),
            );
        });
        if slots != orig {
            if multi {
                if let Some(s) = &mut self.session {
                    let i = s.selected_track.min(s.tracks.len() - 1);
                    s.tracks[i].rack = slots;
                    s.touch();
                }
            } else if let Some(f) = self.cur_mut() {
                f.rack = slots;
            }
        }
    }

    // ------------------------------------------------------------ Markers

    pub fn markers_panel(&mut self, ui: &mut egui::Ui) {
        let multi = self.mode == Mode::Multitrack;
        let fmt = self.time_format;
        let rate = self.rate();
        let mut goto: Option<(f64, f64)> = None;
        let mut remove: Option<usize> = None;
        let mut add = false;
        let cur = self.current;
        let markers: Option<&mut Vec<crate::session::Marker>> = if multi {
            self.session.as_mut().map(|s| &mut s.markers)
        } else {
            cur.and_then(|id| self.files.iter_mut().find(|f| f.id == id))
                .map(|f| &mut f.markers)
        };
        let Some(markers) = markers else {
            ui.label(RichText::new("Nothing open.").color(theme::text_dim()));
            return;
        };
        ui.horizontal(|ui| {
            if icon_button(ui, Icon::Marker, false, "Add Marker (M)").clicked() {
                add = true;
            }
            ui.label(
                RichText::new(format!("{} markers", markers.len()))
                    .small()
                    .color(theme::text_dim()),
            );
        });
        egui::ScrollArea::vertical()
            .id_salt("markers")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("markers_grid")
                    .striped(true)
                    .num_columns(4)
                    .spacing([6.0, 3.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Name").small().color(theme::text_dim()));
                        ui.label(RichText::new("Start").small().color(theme::text_dim()));
                        ui.label(RichText::new("Duration").small().color(theme::text_dim()));
                        ui.label("");
                        ui.end_row();
                        for (i, m) in markers.iter_mut().enumerate() {
                            ui.add(egui::TextEdit::singleline(&mut m.name).desired_width(100.0));
                            time_drag(ui, &mut m.time, fmt, rate, true);
                            time_drag(ui, &mut m.duration, fmt, rate, true);
                            ui.horizontal(|ui| {
                                if ui.small_button("Go").clicked() {
                                    goto = Some((m.time, m.duration));
                                }
                                if ui.small_button("\u{2715}").clicked() {
                                    remove = Some(i);
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
        if let Some(i) = remove {
            markers.remove(i);
        }
        if add {
            self.add_marker();
        }
        if let Some((t, d)) = goto {
            self.set_cursor(t);
            if d > 0.0 {
                if multi {
                    if let Some(s) = &mut self.session {
                        s.selection = Some((t, t + d));
                    }
                } else if let Some(f) = self.cur_mut() {
                    f.selection = Some((t, t + d));
                }
            }
        }
    }

    // ------------------------------------------------------------ Properties

    pub fn properties_panel(&mut self, ui: &mut egui::Ui) {
        let fmt = self.time_format;
        let rate = self.rate();
        let multi = self.mode == Mode::Multitrack;
        egui::ScrollArea::vertical()
            .id_salt("props")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if multi {
                    let Some(s) = &mut self.session else { return };
                    let mut changed = false;
                    egui::CollapsingHeader::new("Session")
                        .default_open(true)
                        .show(ui, |ui| {
                            egui::Grid::new("sess_props").num_columns(2).show(ui, |ui| {
                                ui.label("Name");
                                ui.text_edit_singleline(&mut s.name);
                                ui.end_row();
                                ui.label("Sample Rate");
                                ui.label(format!("{} Hz", s.rate));
                                ui.end_row();
                                ui.label("Bit Depth");
                                ui.label(if s.bits == 32 {
                                    "32 (float)".to_string()
                                } else {
                                    format!("{}-bit", s.bits)
                                });
                                ui.end_row();
                                ui.label("Mix");
                                ui.label(["Mono", "Stereo", "5.1"][(s.mix as usize).min(2)]);
                                ui.end_row();
                                ui.label("Tracks");
                                ui.label(format!("{}", s.tracks.len()));
                                ui.end_row();
                                ui.label("Duration");
                                ui.label(audio::fmt_time(s.length()));
                                ui.end_row();
                            });
                        });
                    if let Some(id) = s.selected_clip {
                        if let Some(c) = s.clip_mut(id) {
                            egui::CollapsingHeader::new("Clip")
                                .default_open(true)
                                .show(ui, |ui| {
                                    egui::Grid::new("clip_props").num_columns(2).show(ui, |ui| {
                                        ui.label("Name");
                                        changed |= ui.text_edit_singleline(&mut c.name).changed();
                                        ui.end_row();
                                        ui.label("Start");
                                        changed |=
                                            time_drag(ui, &mut c.start, fmt, rate, true).changed();
                                        ui.end_row();
                                        ui.label("Source Offset");
                                        changed |=
                                            time_drag(ui, &mut c.offset, fmt, rate, true).changed();
                                        ui.end_row();
                                        ui.label("Duration");
                                        changed |=
                                            time_drag(ui, &mut c.len, fmt, rate, true).changed();
                                        ui.end_row();
                                        ui.label("Clip Gain");
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut c.gain_db)
                                                    .range(-60.0..=24.0)
                                                    .speed(0.1)
                                                    .suffix(" dB"),
                                            )
                                            .changed();
                                        ui.end_row();
                                        let l = c.len;
                                        ui.label("Fade In");
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut c.fade_in)
                                                    .range(0.0..=l)
                                                    .speed(0.01)
                                                    .suffix(" s"),
                                            )
                                            .changed();
                                        ui.end_row();
                                        ui.label("Fade Out");
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut c.fade_out)
                                                    .range(0.0..=l)
                                                    .speed(0.01)
                                                    .suffix(" s"),
                                            )
                                            .changed();
                                        ui.end_row();
                                        ui.label("Mute");
                                        changed |= ui.checkbox(&mut c.muted, "").changed();
                                        ui.end_row();
                                    });
                                });
                        }
                    }
                    if !s.tracks.is_empty() {
                        let i = s.selected_track.min(s.tracks.len() - 1);
                        let t = &mut s.tracks[i];
                        egui::CollapsingHeader::new("Track")
                            .default_open(true)
                            .show(ui, |ui| {
                                egui::Grid::new("track_props")
                                    .num_columns(2)
                                    .show(ui, |ui| {
                                        ui.label("Name");
                                        changed |= ui.text_edit_singleline(&mut t.name).changed();
                                        ui.end_row();
                                        ui.label("Volume");
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut t.volume_db)
                                                    .range(-60.0..=12.0)
                                                    .speed(0.1)
                                                    .suffix(" dB"),
                                            )
                                            .changed();
                                        ui.end_row();
                                        ui.label("Pan");
                                        changed |= ui
                                            .add(crate::theme::RSlider::new(&mut t.pan, -1.0..=1.0))
                                            .changed();
                                        ui.end_row();
                                        ui.label("Channels");
                                        ui.horizontal(|ui| {
                                            changed |= ui
                                                .selectable_value(&mut t.stereo, false, "Mono")
                                                .changed();
                                            changed |= ui
                                                .selectable_value(&mut t.stereo, true, "Stereo")
                                                .changed();
                                        });
                                        ui.end_row();
                                        ui.label("Color");
                                        let mut c = crate::theme::saturated(Color32::from_rgb(
                                            t.color[0], t.color[1], t.color[2],
                                        ));
                                        if egui::color_picker::color_edit_button_srgba(
                                            ui,
                                            &mut c,
                                            egui::color_picker::Alpha::Opaque,
                                        )
                                        .changed()
                                        {
                                            t.color = [c.r(), c.g(), c.b()];
                                            changed = true;
                                        }
                                        ui.end_row();
                                        ui.label("Height");
                                        ui.add(crate::theme::RSlider::new(
                                            &mut t.height,
                                            50.0..=220.0,
                                        ));
                                        ui.end_row();
                                    });
                            });
                    }
                    if changed {
                        s.touch();
                    }
                } else {
                    let Some(f) = self.cur_mut() else {
                        ui.label(RichText::new("No file open.").color(theme::text_dim()));
                        return;
                    };
                    egui::Grid::new("file_props")
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Name");
                            ui.label(&f.name);
                            ui.end_row();
                            ui.label("Path");
                            ui.label(
                                f.path
                                    .as_ref()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or("(not saved)".into()),
                            );
                            ui.end_row();
                            ui.label("Duration");
                            ui.label(audio::fmt_time(f.data.duration()));
                            ui.end_row();
                            ui.label("Sample Rate");
                            ui.label(format!("{} Hz", f.data.rate));
                            ui.end_row();
                            ui.label("Channels");
                            ui.label(format!("{}", f.data.channels()));
                            ui.end_row();
                            ui.label("Samples");
                            ui.label(format!("{}", f.data.frames()));
                            ui.end_row();
                            ui.label("Save Format");
                            let mut fmtsel: i32 = if f.float { 0 } else { f.bits as i32 };
                            let txt = match fmtsel {
                                0 => "32-bit float",
                                16 => "16-bit",
                                24 => "24-bit",
                                32 => "32-bit integer",
                                8 => "8-bit",
                                _ => "?",
                            };
                            egui::ComboBox::from_id_salt("bitsel")
                                .selected_text(txt)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut fmtsel, 0, "32-bit float");
                                    ui.selectable_value(&mut fmtsel, 32, "32-bit integer");
                                    ui.selectable_value(&mut fmtsel, 24, "24-bit");
                                    ui.selectable_value(&mut fmtsel, 16, "16-bit");
                                    ui.selectable_value(&mut fmtsel, 8, "8-bit");
                                });
                            if fmtsel == 0 {
                                f.float = true;
                                f.bits = 32;
                            } else {
                                f.float = false;
                                f.bits = fmtsel as u16;
                            }
                            ui.end_row();
                            ui.label("Size (approx.)");
                            ui.label(format!("{:.2} MB", f.size_bytes() as f64 / 1_048_576.0));
                            ui.end_row();
                            ui.label("Peak");
                            ui.label(format!("{:.2} dBFS", audio::db(f.data.peak())));
                            ui.end_row();
                            ui.label("Markers");
                            ui.label(format!("{}", f.markers.len()));
                            ui.end_row();
                        });
                }
            });
    }

    // ------------------------------------------------------------ Essential Sound

    pub fn essential_panel(&mut self, ui: &mut egui::Ui) {
        theme::title_header(ui, "Essential Sound");
        egui::Frame::new().inner_margin(8).show(ui, |ui| {
            let target = self.cur().map(|f| f.name.clone());
            let wave = self.mode == Mode::Waveform;
            if !wave {
                self.essential.from_session = false;
            }
            let back = wave && self.essential.from_session && self.session.is_some();
            if back {
                ui.horizontal(|ui| {
                    if theme::dialog_button(ui, "\u{2190} Back to Session", false)
                        .on_hover_text("Return to the multitrack session without applying anything (Esc)")
                        .clicked()
                    {
                        self.essential_back();
                    }
                });
                ui.add_space(4.0);
                if self.mode != Mode::Waveform {
                    return;
                }
            }
            match (&target, wave) {
                (Some(n), true) => {
                    ui.add(egui::Label::new(RichText::new(format!("Target: {n}")).color(theme::text())).truncate());
                    let sel = self.cur().and_then(|f| f.selection).is_some();
                    ui.label(RichText::new(if sel { "Applies to the selection" } else { "Applies to the whole file" }).small().color(theme::text_dim()));
                }
                _ => {
                    ui.label(RichText::new("Open a file in the Waveform editor, then choose what kind of audio it is to get one-click repair, loudness and polish.").color(theme::text_dim()));
                    let has_clip = self.mode == Mode::Multitrack && self.session.as_ref().and_then(|s| s.selected_clip).is_some();
                    if has_clip && ui.button("Edit selected clip's file").clicked() {
                        self.edit_selected_clip_source();
                    }
                    return;
                }
            }
            ui.add_space(6.0);
            let kinds = ["Dialogue", "Music", "SFX", "Ambience"];
            ui.columns(2, |cols| {
                for (i, k) in kinds.iter().enumerate() {
                    let c = &mut cols[i % 2];
                    let on = self.essential.kind == i;
                    let b = egui::Button::new(RichText::new(*k).size(13.0).color(if on { Color32::WHITE } else { theme::text() }))
                        .fill(if on { crate::theme::selected() } else { crate::theme::control() })
                        .min_size(vec2(c.available_width(), 34.0));
                    if c.add(b).clicked() {
                        self.essential.kind = i;
                    }
                }
            });
            ui.add_space(6.0);
            let es = &mut self.essential;
            let h = (ui.available_height() - 40.0).max(60.0);
            egui::ScrollArea::vertical().id_salt("ess").auto_shrink([false, false]).max_height(h).show(ui, |ui| {
                egui::CollapsingHeader::new("Loudness").default_open(true).show(ui, |ui| {
                    ui.checkbox(&mut es.loudness, "Auto-Match");
                    ui.add_enabled(es.loudness, crate::theme::RSlider::new(&mut es.loudness_target, -36.0..=-10.0).suffix(" LUFS").text("Target"));
                });
                if es.kind == 0 {
                    egui::CollapsingHeader::new("Repair").default_open(true).show(ui, |ui| {
                        ui.checkbox(&mut es.reduce_noise, "Reduce Noise");
                        ui.add_enabled(es.reduce_noise, crate::theme::RSlider::new(&mut es.noise_amount, 0.0..=10.0).text("Amount"));
                        ui.checkbox(&mut es.rumble, "Reduce Rumble");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut es.dehum, "DeHum");
                            ui.add_enabled_ui(es.dehum, |ui| {
                                ui.selectable_value(&mut es.hum_freq, 50.0, "50 Hz");
                                ui.selectable_value(&mut es.hum_freq, 60.0, "60 Hz");
                            });
                        });
                    });
                    egui::CollapsingHeader::new("Clarity").default_open(true).show(ui, |ui| {
                        ui.checkbox(&mut es.dynamics, "Dynamics");
                        ui.add_enabled(es.dynamics, crate::theme::RSlider::new(&mut es.dyn_amount, 0.0..=10.0).text("Amount"));
                        ui.checkbox(&mut es.eq, "EQ");
                        ui.add_enabled_ui(es.eq, |ui| {
                            egui::ComboBox::from_id_salt("eqp").selected_text(EQ_PRESETS[es.eq_preset]).show_ui(ui, |ui| {
                                for (i, n) in EQ_PRESETS.iter().enumerate() {
                                    ui.selectable_value(&mut es.eq_preset, i, *n);
                                }
                            });
                        });
                    });
                }
                if es.kind == 1 || es.kind == 3 {
                    egui::CollapsingHeader::new("Stereo Width").default_open(true).show(ui, |ui| {
                        ui.checkbox(&mut es.width, "Enable");
                        ui.add_enabled(es.width, crate::theme::RSlider::new(&mut es.width_amount, 0.0..=10.0).text("Width"));
                    });
                }
                egui::CollapsingHeader::new("Creative").default_open(true).show(ui, |ui| {
                    ui.checkbox(&mut es.reverb, "Reverb");
                    ui.add_enabled(es.reverb, crate::theme::RSlider::new(&mut es.reverb_amount, 0.0..=10.0).text("Amount"));
                });
            });
            ui.horizontal(|ui| {
                if theme::dialog_button(ui, "Clear", false)
                    .on_hover_text("Put every setting here back to its default (nothing is applied)")
                    .clicked()
                {
                    let k = self.essential.kind;
                    let from = self.essential.from_session;
                    self.essential = Essential::default();
                    self.essential.kind = k;
                    self.essential.from_session = from;
                }
                if back
                    && theme::dialog_button(ui, "Cancel", false)
                        .on_hover_text("Back to the session without applying (Esc)")
                        .clicked()
                {
                    self.essential_back();
                }
                if theme::dialog_button(ui, "Apply", true)
                    .on_hover_text("Process the file with these settings (one Undo takes it back)")
                    .clicked()
                {
                    self.apply_essential();
                }
            });
        });
    }

    fn apply_essential(&mut self) {
        let es = &self.essential;
        let mut chain: Vec<EffectSlot> = vec![];
        if es.kind == 0 && es.rumble {
            chain.push(EffectSlot::new(Effect::Filter {
                kind: FilterKind::HighPass,
                freq: 80.0,
                q: 0.707,
                gain_db: 0.0,
            }));
        }
        if es.kind == 0 && es.dehum {
            chain.push(EffectSlot::new(Effect::DeHum {
                freq: es.hum_freq,
                harmonics: 5,
                q: 30.0,
            }));
        }
        let nr = if es.kind == 0 && es.reduce_noise {
            Some(es.noise_amount / 10.0)
        } else {
            None
        };
        let mut post: Vec<EffectSlot> = vec![];
        if es.kind == 0 && es.eq {
            if let Some(b) = eq_preset(es.eq_preset) {
                post.push(EffectSlot::new(Effect::ParametricEq {
                    bands: b,
                    master_db: 0.0,
                }));
            }
        }
        if es.kind == 0 && es.dynamics {
            let a = es.dyn_amount / 10.0;
            post.push(EffectSlot::new(Effect::Compressor {
                threshold: -12.0 - 20.0 * a,
                ratio: 1.5 + 4.5 * a,
                attack_ms: 5.0,
                release_ms: 120.0,
                makeup: 6.0 * a,
            }));
        }
        if (es.kind == 1 || es.kind == 3) && es.width {
            post.push(EffectSlot::new(Effect::StereoExpander {
                width: 1.0 + es.width_amount / 10.0 * 1.5,
            }));
        }
        if es.reverb {
            let a = es.reverb_amount / 10.0;
            post.push(EffectSlot::new(Effect::Reverb {
                room: 0.4 + 0.5 * a,
                damping: 0.5,
                width: 1.0,
                mix: 0.08 + 0.4 * a,
            }));
        }
        let loud = if es.loudness {
            Some(es.loudness_target)
        } else {
            None
        };
        self.process("Essential Sound", move |d, a, b| {
            dsp::apply_chain(d, a, b, &chain);
            if let Some(amount) = nr {
                // estimate a noise print from the quietest 300 ms of the range
                let win = (d.rate as usize * 3 / 10).max(1);
                let mut best = (f64::MAX, a);
                let mut pos = a;
                while pos + win <= b {
                    let e: f64 =
                        d.ch.iter()
                            .map(|c| {
                                c[pos..pos + win]
                                    .iter()
                                    .map(|v| (*v as f64).powi(2))
                                    .sum::<f64>()
                            })
                            .sum();
                    if e < best.0 {
                        best = (e, pos);
                    }
                    pos += win / 2;
                }
                let prof = dsp::capture_noise(d, best.1, (best.1 + win).min(b));
                dsp::noise_reduce(d, a, b, &prof, 0.3 + amount * 0.9, 6.0 + amount * 24.0);
            }
            dsp::apply_chain(d, a, b, &post);
            if let Some(t) = loud {
                dsp::match_loudness(d, a, b, t);
            }
        });
    }

    pub fn apply_favorite(&mut self, name: &str) {
        let one = |e: Effect| vec![EffectSlot::new(e)];
        match name {
            "Fade In" => self.process("Fade In", |d, a, b| {
                dsp::fade(d, a, b, true, FadeCurve::Cosine)
            }),
            "Fade Out" => self.process("Fade Out", |d, a, b| {
                dsp::fade(d, a, b, false, FadeCurve::Cosine)
            }),
            "Normalize to -0.1 dB" => {
                self.process("Normalize", |d, a, b| dsp::normalize(d, a, b, -0.1))
            }
            "Normalize to -3 dB" => {
                self.process("Normalize", |d, a, b| dsp::normalize(d, a, b, -3.0))
            }
            "Remove DC Offset" => self.process("DC Offset", dsp::remove_dc),
            "Reduce Rumble" => {
                let c = one(Effect::Filter {
                    kind: FilterKind::HighPass,
                    freq: 80.0,
                    q: 0.707,
                    gain_db: 0.0,
                });
                self.process("Reduce Rumble", move |d, a, b| {
                    dsp::apply_chain(d, a, b, &c)
                })
            }
            "Telephone Voice" => {
                let c = one(Effect::ParametricEq {
                    bands: eq_preset(3).unwrap(),
                    master_db: 0.0,
                });
                self.process("Telephone Voice", move |d, a, b| {
                    dsp::apply_chain(d, a, b, &c)
                })
            }
            "Podcast Voice" => {
                let c = vec![
                    EffectSlot::new(Effect::ParametricEq {
                        bands: eq_preset(0).unwrap(),
                        master_db: 0.0,
                    }),
                    EffectSlot::new(Effect::Compressor {
                        threshold: -22.0,
                        ratio: 3.0,
                        attack_ms: 5.0,
                        release_ms: 120.0,
                        makeup: 4.0,
                    }),
                    EffectSlot::new(Effect::HardLimiter {
                        ceiling_db: -1.0,
                        input_boost: 0.0,
                        release_ms: 50.0,
                    }),
                ];
                self.process("Podcast Voice", move |d, a, b| {
                    dsp::apply_chain(d, a, b, &c)
                })
            }
            "Light Reverb" => {
                let c = one(Effect::Reverb {
                    room: 0.45,
                    damping: 0.5,
                    width: 1.0,
                    mix: 0.18,
                });
                self.process("Light Reverb", move |d, a, b| dsp::apply_chain(d, a, b, &c))
            }
            "Invert" => self.process("Invert", dsp::invert),
            "Reverse" => self.process("Reverse", dsp::reverse),
            "Silence" => self.process("Silence", dsp::silence),
            _ => {}
        }
    }

    // ------------------------------------------------------------ Levels & Selection/View

    pub fn levels_panel(&mut self, ui: &mut egui::Ui) {
        theme::title_header(ui, "Levels");
        let r = ui.available_rect_before_wrap().shrink2(vec2(6.0, 4.0));
        // the input shows while recording, and while an armed track listens with transport stopped
        let (rec, inm, m) = {
            let s = self.engine.lock();
            (
                s.recording || (s.in_channels > 0 && !s.playing),
                s.in_meter,
                s.meter,
            )
        };
        let levels = if rec { inm } else { m };
        let holds = if rec { inm } else { self.meter_hold };
        // a tall, narrow Levels panel stands the meter up (dB scale on the right)
        if r.height() > r.width() * 1.6 && r.height() > 60.0 {
            let bars = Rect::from_min_max(
                pos2(r.left() + 2.0, r.top() + 14.0),
                pos2(r.right() - 26.0, r.bottom() - 2.0),
            );
            let p = ui.painter();
            let leds = Rect::from_min_max(
                pos2(bars.left(), r.top()),
                pos2(bars.right(), r.top() + 10.0),
            );
            for (i, on) in self.meter_clip.iter().enumerate() {
                let w = leds.width() / 2.0;
                let lr = Rect::from_min_size(
                    pos2(leds.left() + i as f32 * w + 1.0, leds.top()),
                    vec2(w - 2.0, leds.height()),
                );
                p.rect_filled(
                    lr,
                    0.0,
                    if *on {
                        Color32::from_rgb(235, 40, 30)
                    } else {
                        Color32::from_rgb(50, 22, 22)
                    },
                );
            }
            theme::vmeter(p, bars, &levels);
            for d in [
                0, -3, -6, -9, -12, -15, -18, -21, -24, -27, -30, -33, -36, -39, -42, -45, -48,
                -51, -54, -57,
            ] {
                let y = bars.bottom() - ((d as f32 + 60.0) / 60.0) * bars.height();
                if y > bars.top() - 1.0 {
                    p.line_segment(
                        [pos2(bars.right() + 1.0, y), pos2(bars.right() + 4.0, y)],
                        egui::Stroke::new(1.0, theme::text_dim()),
                    );
                    p.text(
                        pos2(r.right() - 2.0, y),
                        egui::Align2::RIGHT_CENTER,
                        format!("{d}"),
                        FontId::proportional(9.0),
                        theme::text_dim(),
                    );
                }
            }
            if ui
                .interact(leds, ui.scope_id().with("cliplights_v"), Sense::click())
                .on_hover_text("Clip indicators: click to reset")
                .clicked()
            {
                self.meter_clip = [false; 2];
            }
            ui.allocate_rect(r, Sense::hover());
            return;
        }
        if r.height() > 10.0 {
            let leds = theme::hmeter(ui.painter(), r, &levels, &holds, &self.meter_clip);
            if rec {
                ui.painter().text(
                    r.right_bottom() + vec2(-4.0, -2.0),
                    egui::Align2::RIGHT_BOTTOM,
                    "INPUT",
                    FontId::proportional(9.0),
                    Color32::from_rgb(235, 90, 90),
                );
            }
            if ui
                .interact(leds, ui.scope_id().with("cliplights"), Sense::click())
                .on_hover_text("Clip indicators: click to reset")
                .clicked()
            {
                self.meter_clip = [false; 2];
            }
        }
        ui.allocate_rect(r, Sense::hover());
    }

    pub fn selection_view(&mut self, ui: &mut egui::Ui) {
        theme::title_header(ui, "Selection/View");
        // the panel's fixed height holds three rows and the format dropdown: compact controls
        ui.spacing_mut().interact_size.y = 16.0;
        let fmt = self.time_format;
        let rate = self.rate();
        let (mut sel, mut view) = match self.mode {
            Mode::Waveform => match self.cur() {
                Some(f) => (f.selection.unwrap_or((f.cursor, f.cursor)), f.view),
                None => ((0.0, 0.0), (0.0, 0.0)),
            },
            Mode::Multitrack => match &self.session {
                Some(s) => (s.selection.unwrap_or((s.cursor, s.cursor)), s.view),
                None => ((0.0, 0.0), (0.0, 0.0)),
            },
        };
        let enabled = self.cur().is_some() || self.mode == Mode::Multitrack;
        let (osel, oview) = (sel, view);
        let mut sel_dur = sel.1 - sel.0;
        let mut view_dur = view.1 - view.0;
        // the time-format selector for every time readout, at the top of the panel above the
        // Selection/View rows, so shrinking the panel never hides it (View > Time Display has
        // the same list)
        let idx = TimeFormat::ALL
            .iter()
            .position(|(f, _)| *f == fmt)
            .unwrap_or(0);
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 10,
                right: 6,
                top: 3,
                bottom: 1,
            })
            .show(ui, |ui| {
                egui::ComboBox::from_id_salt("sv_time_format")
                    .width((ui.available_width() - 2.0).max(120.0))
                    .selected_text(TimeFormat::ALL[idx].1)
                    .show_ui(ui, |ui| {
                        for (i, (f, name)) in TimeFormat::ALL.iter().enumerate() {
                            if ui
                                .selectable_label(self.time_format == *f, *name)
                                .clicked()
                            {
                                self.set_time_format(i);
                            }
                        }
                    });
            });
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(10, 2))
            .show(ui, |ui| {
                egui::Grid::new("selview")
                    .num_columns(4)
                    .spacing([14.0, 1.0])
                    .show(ui, |ui| {
                        ui.label("");
                        ui.label(RichText::new("Start").small().color(theme::text_dim()));
                        ui.label(RichText::new("End").small().color(theme::text_dim()));
                        ui.label(RichText::new("Duration").small().color(theme::text_dim()));
                        ui.end_row();
                        ui.label("Selection");
                        time_drag(ui, &mut sel.0, fmt, rate, enabled);
                        time_drag(ui, &mut sel.1, fmt, rate, enabled);
                        if time_drag(ui, &mut sel_dur, fmt, rate, enabled).changed() {
                            sel.1 = sel.0 + sel_dur;
                        }
                        ui.end_row();
                        ui.label("View");
                        time_drag(ui, &mut view.0, fmt, rate, enabled);
                        time_drag(ui, &mut view.1, fmt, rate, enabled);
                        if time_drag(ui, &mut view_dur, fmt, rate, enabled).changed() {
                            view.1 = view.0 + view_dur.max(0.001);
                        }
                        ui.end_row();
                    });
            });
        if sel != osel || view != oview {
            let sel = (sel.0.min(sel.1), sel.0.max(sel.1));
            let view = if view.1 > view.0 + 1e-4 { view } else { oview };
            match self.mode {
                Mode::Waveform => {
                    if let Some(f) = self.cur_mut() {
                        let d = f.data.duration();
                        f.selection = if sel.1 > sel.0 {
                            Some((sel.0.min(d), sel.1.min(d)))
                        } else {
                            None
                        };
                        f.cursor = sel.0.min(d);
                        f.view = (view.0.clamp(0.0, d), view.1.clamp(0.0, d.max(0.001)));
                    }
                }
                Mode::Multitrack => {
                    if let Some(s) = &mut self.session {
                        s.selection = if sel.1 > sel.0 { Some(sel) } else { None };
                        s.cursor = sel.0;
                        s.view = view;
                    }
                }
            }
        }
    }
}

/// Shortens a name in the middle ("How_To_...roy_Angels.wav *") so its start, its end and
/// the unsaved mark stay visible, as the Files panel does in the original.
pub fn middle_ellipsis(p: &egui::Painter, text: &str, font: FontId, max_w: f32) -> String {
    middle_ellipsis_by(text, max_w, |s| {
        p.layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE)
            .size()
            .x
    })
}

/// The shortening itself, with any text-width measure (kept separate so it can be tested).
pub fn middle_ellipsis_by(text: &str, max_w: f32, width: impl Fn(&str) -> f32) -> String {
    if max_w <= 0.0 || width(text) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len().saturating_sub(1);
    while keep > 2 {
        let head = (keep + 1) / 2;
        let tail = keep - head;
        let s: String = chars[..head]
            .iter()
            .copied()
            .chain("...".chars())
            .chain(chars[chars.len() - tail..].iter().copied())
            .collect();
        if width(&s) <= max_w {
            return s;
        }
        keep -= 1;
    }
    "...".to_string()
}
