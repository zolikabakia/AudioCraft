//! Workspace layout: menu bar, toolbar, panels, transport, status bar, keyboard.

use crate::app::*;
use crate::audio;
use crate::theme::{self, Icon, icon_button};
use egui::{Color32, Frame, Margin, RichText};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

/// What the status bar can show (View > Status Bar, or right-click the status bar).
pub const STATUS_ITEMS: [&str; 7] = [
    "Sample Type",
    "Uncompressed Audio Size",
    "Duration",
    "Free Space",
    "Memory (RAM) Use",
    "CPU Use",
    "Output Device",
];

pub fn mi(ui: &mut egui::Ui, label: &str, shortcut: &str) -> bool {
    let b = if shortcut.is_empty() {
        egui::Button::new(label)
    } else {
        egui::Button::new(label).shortcut_text(shortcut)
    };
    let r = ui.add(b).clicked();
    if r {
        ui.close();
    }
    r
}

/// How far (points) the pointer may stray from the menu bar and its open menus before it
/// counts as having left them: a diagonal move toward a submenu never closes anything.
const MENU_SLACK: f32 = 40.0;
/// How long the pointer must stay away before a menu opened from the menu bar closes (the
/// classic desktop hover delay is about 0.4 s; a little longer, so it is never in the way).
const MENU_AWAY_SECS: f64 = 0.6;

/// Close the menu bar's open menus once the pointer has been away from them for a moment
/// (never while it is over the bar, a menu or a submenu, or just beside them).
fn menu_autoclose(ctx: &egui::Context, bar: egui::Rect) {
    let key = egui::Id::new("ac_menu_away_since");
    if !egui::Popup::is_any_open(ctx) {
        ctx.data_mut(|d| d.remove::<f64>(key));
        return;
    }
    // the open menus and submenus (foreground areas); only menus hanging from the bar count,
    // so drop-down lists and right-click menus elsewhere behave as before
    let menus: Vec<egui::Rect> = ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| l.order == egui::Order::Foreground)
            .filter_map(|l| m.area_rect(l.id))
            .collect()
    });
    let from_bar = menus
        .iter()
        .any(|r| (r.top() - bar.bottom()).abs() < 8.0 && r.left() < bar.right());
    if !from_bar {
        ctx.data_mut(|d| d.remove::<f64>(key));
        return;
    }
    let now = ctx.input(|i| i.time);
    let near = ctx
        .pointer_latest_pos()
        .filter(|_| ctx.input(|i| i.pointer.has_pointer()))
        .is_some_and(|p| {
            bar.expand(4.0).contains(p) || menus.iter().any(|r| r.expand(MENU_SLACK).contains(p))
        });
    if near {
        ctx.data_mut(|d| d.remove::<f64>(key));
        return;
    }
    let since: f64 = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(key, || now));
    if now - since >= MENU_AWAY_SECS {
        egui::Popup::close_all(ctx);
        ctx.data_mut(|d| d.remove::<f64>(key));
    } else {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

fn sep(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 22.0), egui::Sense::hover());
    ui.painter().line_segment(
        [
            egui::pos2(r.center().x, r.top() + 3.0),
            egui::pos2(r.center().x, r.bottom() - 3.0),
        ],
        egui::Stroke::new(1.0, crate::theme::gray(64)),
    );
}

fn seg_button(ui: &mut egui::Ui, icon: Icon, label: &str, on: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(98.0, 24.0), egui::Sense::click());
    ui.painter().rect_filled(
        r,
        theme::ROUND as f32,
        theme::control_fill(on, theme::selected(), &resp, true),
    );
    let ir = egui::Rect::from_min_size(r.min + egui::vec2(6.0, 4.0), egui::vec2(16.0, 16.0));
    let col = if on || resp.hovered() {
        theme::strong()
    } else {
        theme::text()
    };
    theme::paint_icon(ui.painter(), ir, icon, col);
    ui.painter().text(
        egui::pos2(r.left() + 27.0, r.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.5),
        col,
    );
    resp
}

pub fn sub_panel<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    Frame::new()
        .fill(theme::panel())
        .corner_radius(theme::ROUND)
        .outer_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            add(ui)
        })
        .inner
}

impl eframe::App for App {
    /// Orderly plug-in teardown while the process is fully alive: streams stop, instances are
    /// dropped (each terminates its component/controller), then the VST3 factories are released
    /// and each module's ExitDll runs on this, the main, thread. Libraries are never unloaded.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.plug_close_all();
        self.engine.shutdown();
        crate::plugins::shutdown_hosting();
        if crate::plugins::hosted_loaded() {
            crate::plugins::finish_without_teardown();
        }
    }

    /// Screenshot diagnostic: `AUDIOCRAFT_SHOT_CLICKS="frame:x,y;frame:x,y:h"` clicks (or with
    /// `:h` only moves the pointer to) those points at those frames, so a shot can show an open
    /// menu or a hovered item.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.screenshot.is_none() {
            return;
        }
        let Ok(spec) = std::env::var("AUDIOCRAFT_SHOT_CLICKS") else {
            return;
        };
        let now = self.frame_count + 1;
        for item in spec.split(';') {
            let parts: Vec<&str> = item.trim().split(':').collect();
            let (Some(f), Some(xy)) = (
                parts.first().and_then(|f| f.parse::<u64>().ok()),
                parts.get(1),
            ) else {
                continue;
            };
            let mut it = xy.split(',').filter_map(|v| v.trim().parse::<f32>().ok());
            let (Some(x), Some(y)) = (it.next(), it.next()) else {
                continue;
            };
            let pos = egui::pos2(x, y);
            let hover = parts.get(2) == Some(&"h");
            let button = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            if now == f {
                raw.events.push(egui::Event::PointerMoved(pos));
                if !hover {
                    raw.events.push(button(true));
                }
            } else if now == f + 1 && !hover {
                raw.events.push(button(false));
            } else if now > f && hover {
                raw.events.push(egui::Event::PointerMoved(pos));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame_count += 1;
        self.plug_note_window(frame);
        if self.screenshot.is_none() {
            theme::sync_title_bar(self.plug_dock.main);
        }
        self.apply_ui_prefs(&ctx);
        if self.frame_count == 1 && std::env::var_os("AUDIOCRAFT_STARTUP_TIME").is_some() {
            if let Some(t) = crate::STARTED.get() {
                eprintln!("first frame after {} ms", t.elapsed().as_millis());
            }
        }
        // open the audio device once the window is up, so it never holds up the first frame
        if self.frame_count == 2 && self.screenshot.is_none() {
            let before = self.engine.lock().out_rate;
            self.engine.ensure_output();
            if self.engine.lock().out_rate != before {
                self.engine.lock().reset_chains();
                self.mix_key = (u64::MAX, 0, 0);
            }
            if std::env::var_os("AUDIOCRAFT_STARTUP_TIME").is_some() {
                if let Some(t) = crate::STARTED.get() {
                    eprintln!("audio ready after {} ms", t.elapsed().as_millis());
                }
            }
        }
        // AUDIOCRAFT_IDLE_TEST=seconds: print frames drawn per second, then close (diagnostics)
        if let Some(secs) = std::env::var("AUDIOCRAFT_IDLE_TEST")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
        {
            let now = ctx.input(|i| i.time);
            let id = egui::Id::new("idle_test");
            let (mut last, mut frames): (f64, u32) =
                ctx.data(|d| d.get_temp(id)).unwrap_or((now, 0));
            frames += 1;
            if now - last >= 1.0 {
                let cpu = self.sysmon.tick(None).cpu.unwrap_or(-1.0);
                eprintln!("{:.0}s: {frames} frames, cpu {cpu:.1}%", now);
                last = now;
                frames = 0;
            }
            ctx.data_mut(|d| d.insert_temp(id, (last, frames)));
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            if now > secs {
                // a diagnostic run closes without asking about unsaved changes
                self.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        self.screenshot_tick(&ctx);
        self.tick_imports(&ctx);
        self.tick_video_import(&ctx);
        self.tick_live_take();
        self.sync_input();
        self.decoder_download_poll();
        // a non-looping Files preview that reached its end clears itself in the engine;
        // reflect that state in the button. The preview never uses the main transport.
        if self.files_preview.is_some() && self.engine.lock().files_preview.is_none() {
            self.files_preview = None;
        }
        self.sync_engine();
        self.handle_keys(&ctx);
        self.handle_dropped(&ctx);
        // The drop handler used last frame's hit areas. Rebuild them below from visible,
        // active browser tabs so a hidden or removed Files panel cannot capture a drop.
        self.files_drop_rects_last = std::mem::take(&mut self.files_drop_rects);
        let t = ctx.input(|i| i.time);
        self.update_meter_hold(t);
        // a press anywhere but the Effects Rack hands Delete back to the editor
        if ctx.input(|i| i.pointer.primary_pressed()) {
            if let Some(p) = ctx.input(|i| i.pointer.interact_pos()) {
                let on_panels = ctx
                    .layer_id_at(p)
                    .map(|l| l.order == egui::Order::Background)
                    .unwrap_or(true);
                if self.rack_focus && on_panels && !self.rack_rect.contains(p) {
                    self.rack_focus = false;
                }
                // Backspace clears the Files selection only while the Files panel was the last
                // place clicked
                self.files_focus =
                    on_panels && self.files_drop_rects_last.iter().any(|r| r.contains(p));
            }
        }
        if self.is_playing() || self.is_recording() || self.engine.lock().files_preview.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        let err = self.engine.lock().error.take();
        if let Some(e) = err {
            self.status = e;
        }
        ui.painter().rect_filled(ui.max_rect(), 0.0, theme::bg());

        let bar = egui::Panel::top("menubar")
            .frame(
                Frame::new()
                    .fill(theme::panel_header())
                    .inner_margin(Margin::symmetric(4, 1)),
            )
            .show(ui, |ui| self.menu_bar(ui));
        menu_autoclose(&ctx, bar.response.rect);
        egui::Panel::top("toolbar")
            .exact_size(30.0)
            .frame(
                Frame::new()
                    .fill(theme::bg())
                    .inner_margin(Margin::symmetric(6, 2)),
            )
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .exact_size(22.0)
            .frame(
                Frame::new()
                    .fill(theme::own_or(crate::theme::gray(24), theme::bg()))
                    .inner_margin(Margin::symmetric(8, 2)),
            )
            .show(ui, |ui| self.status_bar(ui));
        // the workspace's panels around the editor
        self.dock(ui);
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::bg()).inner_margin(Margin::same(1)))
            .show(ui, |ui| sub_panel(ui, |ui| self.center(ui)));

        self.handle_panel_request(&ctx);
        let down = ctx.input(|i| i.pointer.any_down());
        self.session_hist_catch_up(down);
        self.close_guard(&ctx);
        self.dialogs(&ctx);
        self.compat_dialog(&ctx);
        self.prefs_window(&ctx);
        self.analysis_windows(&ctx);
        self.command_palette(&ctx);
        self.rack_windows(&ctx);
        self.track_eq_windows(&ctx);
        self.help_window(&ctx);
        // panels in windows of their own (New Window, Undock Panel, dual monitor)
        self.float_windows(&ctx);
        // last: docked plug-in editors follow the Plug-ins panel and step aside for menus
        self.plug_service(&ctx);
    }
}

impl App {
    // ------------------------------------------------------------ keyboard

    fn handle_keys(&mut self, ctx: &egui::Context) {
        // the Preferences window takes the keys while it is open (Escape cancels it)
        if ctx.egui_wants_keyboard_input() || self.prefs_edit.is_some() {
            return;
        }
        // editing a clip's file from Essential Sound: Esc goes back to the session
        if self.essential.from_session
            && self.mode == Mode::Waveform
            && self.dialog.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.essential_back();
            return;
        }
        use egui::{Key, KeyboardShortcut as KS, Modifiers as M};
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            // these take Esc themselves (Cancel, close, cancel the drag); an open menu or list
            // closes on its own
            if self.dialog.is_some()
                || self.palette.is_some()
                || self.close_queue.is_some()
                || self.rack_drag.is_some()
                || egui::Popup::is_any_open(ctx)
            {
                return;
            }
            if self.close_top_window() {
                ctx.input_mut(|i| i.consume_key(M::NONE, Key::Escape));
                return;
            }
        }
        let mut cmd: Vec<&'static str> = vec![];
        ctx.input_mut(|i| {
            let mut sc = |m: M, k: Key, c: &'static str| {
                if i.consume_shortcut(&KS::new(m, k)) {
                    cmd.push(c);
                }
            };
            // combinations first: a plain-key shortcut would also catch them
            sc(M::COMMAND, Key::R, "cmd:edit.repeat_last");
            sc(M::ALT | M::SHIFT, Key::C, "cmd:edit.copy_to_new");
            sc(M::ALT, Key::K, "cmd:edit.keyboard_shortcuts");
            sc(M::SHIFT, Key::Home, "cmd:edit.select_to_start");
            sc(M::SHIFT, Key::End, "cmd:edit.select_to_end");
            sc(M::ALT, Key::OpenBracket, "cmd:clip.trim_start");
            sc(M::ALT, Key::CloseBracket, "cmd:clip.trim_end");
            sc(M::ALT, Key::Equals, "cmd:view.zoom_in_amp");
            sc(M::ALT, Key::Minus, "cmd:view.zoom_out_amp");
            sc(M::NONE, Key::F1, "cmd:help.manual");
            sc(M::SHIFT, Key::T, "cmd:edit.convert");
            sc(M::COMMAND | M::SHIFT, Key::E, "cmd:file.export_file");
            sc(M::COMMAND | M::SHIFT, Key::P, "palette");
            sc(M::COMMAND | M::SHIFT, Key::K, "prefs");
            sc(M::COMMAND | M::SHIFT, Key::Z, "redo");
            sc(M::COMMAND, Key::Z, "undo");
            sc(M::COMMAND, Key::Y, "redo");
            sc(M::COMMAND | M::SHIFT, Key::V, "mixpaste");
            sc(M::COMMAND | M::ALT, Key::V, "pastenew");
            sc(M::COMMAND, Key::X, "cut");
            sc(M::COMMAND, Key::C, "copy");
            sc(M::COMMAND, Key::V, "paste");
            sc(M::COMMAND | M::SHIFT, Key::A, "none");
            sc(M::COMMAND, Key::A, "all");
            sc(M::COMMAND, Key::T, "crop");
            sc(M::COMMAND, Key::U, "autoheal");
            sc(M::ALT, Key::Num0, "effectsrack");
            sc(M::ALT, Key::A, "autolane");
            sc(M::COMMAND, Key::K, "split");
            sc(M::COMMAND | M::SHIFT, Key::D, "duptrack");
            sc(M::COMMAND, Key::D, "dupclip");
            sc(M::COMMAND, Key::G, "gain");
            sc(M::NONE, Key::PageDown, "nextmarker");
            sc(M::NONE, Key::PageUp, "prevmarker");
            sc(M::COMMAND | M::SHIFT, Key::I, "cmd:file.import_video");
            sc(M::COMMAND | M::SHIFT, Key::N, "newsession");
            sc(M::COMMAND, Key::N, "new");
            sc(M::COMMAND, Key::O, "open");
            sc(M::COMMAND | M::SHIFT, Key::S, "saveas");
            sc(M::COMMAND, Key::S, "save");
            sc(M::COMMAND, Key::W, "close");
            sc(M::COMMAND, Key::Q, "quit");
            sc(M::SHIFT, Key::Space, "record");
            sc(M::SHIFT, Key::D, "spectral");
            sc(M::SHIFT, Key::E, "essential");
            sc(M::NONE, Key::Space, "play");
            sc(M::NONE, Key::Delete, "delete");
            sc(M::NONE, Key::Backspace, "backspace");
            sc(M::NONE, Key::Home, "home");
            sc(M::NONE, Key::End, "end");
            sc(M::NONE, Key::M, "marker");
            sc(M::NONE, Key::Equals, "zoomin");
            sc(M::NONE, Key::Plus, "zoomin");
            sc(M::NONE, Key::Minus, "zoomout");
            sc(M::NONE, Key::Backslash, "zoomfull");
            sc(M::NONE, Key::Num8, "wave");
            sc(M::NONE, Key::Num9, "multi");
            sc(M::NONE, Key::V, "tmove");
            sc(M::NONE, Key::A, "tdirect");
            sc(M::NONE, Key::R, "trazor");
            sc(M::NONE, Key::Y, "tslip");
            sc(M::NONE, Key::T, "tsel");
            sc(M::NONE, Key::E, "tmarquee");
            sc(M::NONE, Key::D, "tlasso");
            sc(M::NONE, Key::P, "tbrush");
            sc(M::NONE, Key::B, "theal");
            sc(M::NONE, Key::Escape, "none");
            sc(M::NONE, Key::ArrowLeft, "left");
            sc(M::NONE, Key::ArrowRight, "right");
        });
        for c in cmd {
            self.run(c, ctx);
        }
    }

    pub fn run(&mut self, c: &str, ctx: &egui::Context) {
        if let Some(id) = c.strip_prefix("cmd:") {
            if let Err(e) = self.exec(id, ctx) {
                self.status = e;
            }
            return;
        }
        if self.mode == Mode::Multitrack && matches!(c, "delete" | "backspace" | "cut" | "copy") {
            // settle clip-or-keyframe first, so the edit hits only what was picked last
            self.keys_take_selection();
        }
        match c {
            "palette" => self.palette = Some(String::new()),
            "prefs" => self.open_prefs("General"),
            "undo" => self.undo(),
            "redo" => self.redo(),
            "cut" => self.cut(),
            "copy" => self.copy(),
            "paste" => self.paste(false),
            "mixpaste" => self.paste(true),
            "pastenew" => self.paste_new(),
            "all" => self.select_all(),
            "none" => {
                self.multi_drag.key_sel.clear();
                if let Some(f) = self.cur_mut() {
                    f.spec_sel = None;
                }
                self.select_none()
            }
            "crop" => self.crop(),
            "autoheal" => {
                if !(self.mode == Mode::Waveform
                    && self.spectral_apply(crate::spectral::SpecOp::Heal, "Auto Heal (spectral)"))
                {
                    self.auto_heal()
                }
            }
            "effectsrack" => {
                self.show_panel(crate::workspace::PanelId::EffectsRack);
            }
            "split" => self.split_at_playhead(),
            "backspace" if self.files_focus && !self.files_sel.is_empty() => {
                // Backspace in the Files panel only clears its selection (it never closes files)
                self.files_sel.clear();
                self.selected_file = None;
                self.status = "File selection cleared".into();
            }
            "delete" | "backspace" if self.rack_focus && self.rack_selected.is_some() => {
                // the effect picked last in the Effects Rack goes, not the clip or audio
                if let (Some(t), Some(i)) = (self.rack_target(), self.rack_selected) {
                    if let Some((_, mut slots, _)) = self.rack_get(t) {
                        if i < slots.len() {
                            slots.remove(i);
                            self.rack_window = None;
                            self.rack_set(t, slots, Some("Remove Effect"));
                        }
                    }
                }
                self.rack_selected = None;
            }
            "delete" | "backspace"
                if self.mode == Mode::Multitrack && !self.multi_drag.key_sel.is_empty() =>
            {
                // selected keyframes go first (not the clip they are on)
                self.delete_selected_keys();
            }
            "delete" | "backspace" => {
                // with a spectral selection, Delete removes just those frequencies
                if !(self.mode == Mode::Waveform
                    && self
                        .spectral_apply(crate::spectral::SpecOp::Gain(-200.0), "Delete (spectral)"))
                {
                    self.delete_selection("Delete")
                }
            }
            "new" => {
                let n = self.files.len() + 1;
                self.dialog = Some(Dialog::NewFile {
                    name: format!("Untitled {n}"),
                    rate: 48000,
                    channels: 2,
                    bits: 32,
                    make_default: false,
                })
            }
            "newsession" => self.dialog = Some(self.new_session_dialog()),
            "open" => self.open_dialog(),
            "quit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            "save" | "saveas" => {
                let as_ = c == "saveas";
                match self.mode {
                    Mode::Multitrack => self.save_session(as_),
                    Mode::Waveform => {
                        if let Some(id) = self.current {
                            self.save_file(id, as_);
                        }
                    }
                }
            }
            "close" => {
                if let Some(id) = self.current {
                    self.request_close_file(id)
                }
            }
            "record" => self.toggle_record(),
            "spectral" => self.spectral = !self.spectral,
            "essential" => self.ws.layout.toggle(crate::workspace::PanelId::Essential),
            "play" => self.toggle_play(),
            "home" => self.set_cursor(0.0),
            "end" => {
                let e = match self.mode {
                    Mode::Waveform => self.cur().map(|f| f.data.duration()).unwrap_or(0.0),
                    Mode::Multitrack => self.session.as_ref().map(|s| s.length()).unwrap_or(0.0),
                };
                self.set_cursor(e)
            }
            "left" | "right" => {
                let step = self.view_width() / 50.0;
                let t = self.cursor() + if c == "left" { -step } else { step };
                self.set_cursor(t.max(0.0));
            }
            "marker" => self.add_marker(),
            "autolane" => {
                if let Err(e) = self.exec("view.volume_automation", ctx) {
                    self.status = e;
                }
            }
            "nextmarker" | "prevmarker" | "gain" | "dupclip" | "duptrack" => {
                let id = match c {
                    "nextmarker" => "marker.next",
                    "prevmarker" => "marker.previous",
                    "gain" => "process.gain",
                    "dupclip" => "clip.duplicate",
                    _ => "track.duplicate",
                };
                if let Err(e) = self.exec(id, ctx) {
                    self.status = e;
                }
            }
            "zoomin" => self.zoom(0.5, None),
            "zoomout" => self.zoom(2.0, None),
            "zoomfull" => self.zoom_full(),
            "wave" => self.request_mode(Mode::Waveform),
            "multi" => self.request_mode(Mode::Multitrack),
            "tmove" => self.tool = Tool::Move,
            "tdirect" => self.tool = Tool::Direct,
            "trazor" => self.tool = Tool::Razor,
            "tslip" => self.tool = Tool::Slip,
            "tsel" => self.tool = Tool::TimeSel,
            "tmarquee" | "tlasso" | "tbrush" | "theal" => {
                // the spectral tools work on the spectral display of the Waveform Editor
                if self.mode == Mode::Waveform {
                    self.tool = match c {
                        "tmarquee" => Tool::Marquee,
                        "tlasso" => Tool::Lasso,
                        "tbrush" => Tool::Brush,
                        _ => Tool::SpotHeal,
                    };
                    self.spectral = true;
                }
            }
            _ => {}
        }
    }

    /// Act on a choice made in a panel's ≡ menu.
    fn handle_panel_request(&mut self, ctx: &egui::Context) {
        let Some(req) = theme::take_panel_request(ctx) else {
            return;
        };
        if let Some(key) = req.strip_prefix("panel.close:") {
            if let Some(p) = crate::workspace::PanelId::from_title(key) {
                self.ws.layout.remove(p);
                self.status = format!("{key} panel closed (the Window menu shows it again)");
            }
        } else if let Some(key) = req.strip_prefix("panel.undock:") {
            if let Some(p) = crate::workspace::PanelId::from_title(key) {
                self.ws.layout.undock(p);
                self.status = format!(
                    "{key} moved to a window of its own (close the window to dock it back)"
                );
            }
        } else if let Some(key) = req.strip_prefix("panel.undock_group:") {
            if let Some(p) = crate::workspace::PanelId::from_title(key) {
                self.ws.layout.undock_group(p);
                self.status = format!("{key}'s group moved to a window of its own");
            }
        } else if let Some(key) = req.strip_prefix("panel.close_others:") {
            if let Some(p) = crate::workspace::PanelId::from_title(key) {
                let n = self.ws.layout.close_others_in_group(p);
                self.status =
                    format!("{n} other panel(s) closed (the Window menu shows them again)");
            }
        } else if let Some(key) = req.strip_prefix("panel.max:") {
            // maximise the group (the editor alone for the editor), or put everything back
            if let Some(prev) = self.ws.maximized.take() {
                self.ws.layout = prev;
            } else {
                self.ws.maximized = Some(self.ws.layout.clone());
                let mut l = crate::workspace::Layout::default();
                if let Some(p) = crate::workspace::PanelId::from_title(key) {
                    l.left = vec![crate::workspace::Group {
                        tabs: vec![p],
                        active: 0,
                        weight: 1.0,
                    }];
                    l.left_w = 900.0;
                }
                self.ws.layout = l;
            }
        } else {
            match req.as_str() {
                "panel.reset" => self.ws.reset(),
                "panel.levels_reset" => {
                    self.meter_clip = [false; 2];
                    self.meter_hold = [0.0; 2];
                }
                "panel.time_decimal" => self.time_format = TimeFormat::Decimal,
                "panel.time_samples" => self.time_format = TimeFormat::Samples,
                "panel.time_seconds" => self.time_format = TimeFormat::Seconds,
                id => {
                    if let Err(e) = self.exec(id, ctx) {
                        self.status = e;
                    }
                }
            }
        }
    }

    /// Esc: close the topmost non-critical window (an effect's settings, a track EQ, an
    /// analysis window, Help). True when one was closed.
    pub fn close_top_window(&mut self) -> bool {
        if self.rack_window.take().is_some() {
            return true;
        }
        if self.track_eq_window.take().is_some() {
            return true;
        }
        if self.panels.freq {
            self.panels.freq = false;
            return true;
        }
        if self.panels.stats {
            self.panels.stats = false;
            return true;
        }
        if self.extra.help_topic.take().is_some() {
            return true;
        }
        false
    }

    fn handle_dropped(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        // a dropped folder brings in the media inside it
        let dropped = App::expand_folders(dropped);
        if dropped.is_empty() {
            self.status = "The dropped folder has no audio or video files.".into();
            return;
        }
        // files dropped while the Batch Processor is open go onto its list
        if let Some(Dialog::Batch { inputs, .. }) = &mut self.dialog {
            if self.extra.batch.is_none() {
                for p in dropped {
                    let s = p.to_string_lossy().to_string();
                    if audio::is_audio_path(&p) && !inputs.contains(&s) {
                        inputs.push(s);
                    }
                }
            }
            return;
        }

        let drop_pos = if cfg!(test) {
            ctx.input(|i| i.pointer.latest_pos())
        } else {
            self.drop_pointer_pos(ctx)
        };
        // Dropping onto the Files/Favorites browser only adds the sound to the browser. It must
        // never place a clip at the playhead just because a multitrack session happens to be open.
        let browser_drop = drop_pos
            .map(|p| self.files_drop_rects.iter().any(|r| r.contains(p)))
            .unwrap_or(false);
        // in Multitrack, audio dropped from outside goes straight onto a track (the one under
        // the pointer, the next ones for further files); sessions still open as sessions
        let multi = !browser_drop && self.mode == Mode::Multitrack && self.session.is_some();
        let (mut track, at) = self.multitrack_drop_target(drop_pos);
        for p in dropped {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();
            let is_session = matches!(ext.as_str(), "sesx" | "acsession" | "xml" | "audiocraft");
            if browser_drop {
                self.import_path_into_browser(&p);
                continue;
            }
            // A video always goes to the Video panel, never onto a multitrack audio lane.
            if !multi || is_session || crate::video::is_video_path(&p) {
                self.open_path(&p);
                continue;
            }
            let before = self.files.len();
            let known = self
                .files
                .iter()
                .find(|f| f.path.as_deref() == Some(p.as_path()))
                .map(|f| f.id);
            let id = match known {
                Some(id) => Some(id),
                None => {
                    self.open_path(&p);
                    if self.files.len() > before {
                        self.files.last().map(|f| f.id)
                    } else {
                        None
                    }
                }
            };
            // opening must not leave Multitrack
            self.mode = Mode::Multitrack;
            if let Some(id) = id {
                if self.imports.is_pending(id) {
                    // The file is still decoding: place the clip when its real length is known.
                    self.pending_placements.push((id, track, at));
                } else {
                    self.place_file(id, track, at);
                }
                track += 1;
            }
        }
    }

    // ------------------------------------------------------------ menus

    /// A menu entry that runs a registered command (its shortcut comes from the registry);
    /// greyed out when `enabled` is false. Errors go to the status bar.
    /// The marker commands (Edit > Marker and View > Markers show the same list).
    fn markers_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        for (label, cmd) in [
            ("Add Marker", "marker.add"),
            ("Go to Next Marker", "marker.next"),
            ("Go to Previous Marker", "marker.previous"),
            ("Rename Marker...", "marker.rename"),
            ("Delete Marker", "marker.delete"),
            ("Export Markers...", "marker.export"),
            ("Import Markers...", "marker.import"),
        ] {
            self.menu_cmd(ui, true, label, cmd, ctx);
        }
    }

    /// A command's menu item with a check mark in the gutter when `on`.
    fn menu_cmd_check(
        &mut self,
        ui: &mut egui::Ui,
        enabled: bool,
        on: bool,
        label: &str,
        id: &str,
        ctx: &egui::Context,
    ) {
        let sc = crate::commands::find(id).map(|c| c.shortcut).unwrap_or("");
        let doc = crate::commands::find(id).map(|c| c.doc).unwrap_or("");
        let r = theme::check_item(ui, enabled, on, label, sc);
        let r = if doc.is_empty() {
            r
        } else {
            r.on_hover_text(doc)
        };
        if r.clicked() {
            ui.close();
            if let Err(e) = self.exec(id, ctx) {
                self.status = e;
            }
        }
    }

    pub(crate) fn menu_cmd(
        &mut self,
        ui: &mut egui::Ui,
        enabled: bool,
        label: &str,
        id: &str,
        ctx: &egui::Context,
    ) {
        let sc = crate::commands::find(id).map(|c| c.shortcut).unwrap_or("");
        let doc = crate::commands::find(id).map(|c| c.doc).unwrap_or("");
        let r = theme::menu_button(ui, enabled, label, sc, doc);
        if r.clicked() {
            ui.close();
            if let Err(e) = self.exec(id, ctx) {
                self.status = e;
            }
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // what is open decides which entries are available (greyed out otherwise, like the original)
        let wave_file = self.mode == Mode::Waveform && self.cur().is_some();
        let has_file = self.current.is_some();
        let has_session = self.session.is_some();
        let multi_on = self.mode == Mode::Multitrack && has_session;
        let has_wave_sel = self
            .cur()
            .map(|f| f.selection.map(|(a, b)| b > a).unwrap_or(false))
            .unwrap_or(false);
        let multi_sel = self
            .session
            .as_ref()
            .and_then(|s| s.selection)
            .map(|(a, b)| b - a > 0.001)
            .unwrap_or(false);
        let clip_sel = multi_on
            && self
                .session
                .as_ref()
                .map(|s| s.selected_clip.is_some() || !s.selected_clips.is_empty())
                .unwrap_or(false);
        let can_edit = if self.mode == Mode::Waveform {
            wave_file
        } else {
            clip_sel || multi_sel
        };
        let has_path = match self.mode {
            Mode::Multitrack => self
                .session
                .as_ref()
                .map(|s| s.path.is_some())
                .unwrap_or(false),
            Mode::Waveform => self.cur().map(|f| f.path.is_some()).unwrap_or(false),
        };
        let any_dirty = self.files.iter().any(|f| f.dirty())
            || self.session.as_ref().map(|s| s.dirty).unwrap_or(false);
        let can_revert = wave_file
            && self
                .cur()
                .map(|f| f.path.is_some() && f.dirty())
                .unwrap_or(false);
        let stereo_file = wave_file && self.cur().map(|f| f.data.channels() > 1).unwrap_or(false);
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                ui.menu_button("New", |ui| {
                    if mi(ui, "Multitrack Session...", "Ctrl+Shift+N") {
                        self.run("newsession", &ctx);
                    }
                    if mi(ui, "Audio File...", "Ctrl+N") {
                        self.run("new", &ctx);
                    }
                });
                if mi(ui, "Open...", "Ctrl+O") {
                    self.open_dialog();
                }
                if mi(ui, "Import Video File...", "Ctrl+Shift+I") {
                    self.run("cmd:file.import_video", &ctx);
                }
                self.menu_cmd(ui, true, "Import Folder...", "file.import_folder", &ctx);
                ui.menu_button("Open Append", |ui| {
                    self.menu_cmd(ui, true, "To New...", "file.open_append_new", &ctx);
                    self.menu_cmd(
                        ui,
                        has_file,
                        "To Current...",
                        "file.open_append_current",
                        &ctx,
                    );
                });
                ui.menu_button("Open Recent", |ui| {
                    if self.recent.is_empty() {
                        ui.label(RichText::new("No recent files").color(theme::text_dim()));
                    }
                    let mut pick: Option<PathBuf> = None;
                    for p in &self.recent {
                        let name = p
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if ui
                            .button(name)
                            .on_hover_text(p.display().to_string())
                            .clicked()
                        {
                            pick = Some(p.clone());
                            ui.close();
                        }
                    }
                    if !self.recent.is_empty() {
                        ui.separator();
                        if ui.button("Clear Recent").clicked() {
                            self.recent.clear();
                            self.save_recent();
                            ui.close();
                        }
                    }
                    if let Some(p) = pick {
                        if p.exists() {
                            let is_session = p
                                .extension()
                                .map(|e| e == "acsession" || e == "sesx")
                                .unwrap_or(false);
                            if !is_session {
                                self.switch_mode(Mode::Waveform);
                            }
                            self.open_path(&p);
                        } else {
                            self.recent.retain(|r| r != &p);
                            self.save_recent();
                            self.message(
                                "File not found",
                                &format!("{} no longer exists.", p.display()),
                            );
                        }
                    }
                });
                self.menu_cmd(ui, has_path, "Reveal in Explorer", "file.reveal", &ctx);
                ui.separator();
                if ui
                    .add_enabled(has_file, egui::Button::new("Close").shortcut_text("Ctrl+W"))
                    .clicked()
                {
                    self.run("close", &ctx);
                    ui.close();
                }
                self.menu_cmd(
                    ui,
                    has_file || has_session,
                    "Close All",
                    "file.close_all",
                    &ctx,
                );
                self.menu_cmd(
                    ui,
                    has_session,
                    "Close Unused Media",
                    "file.close_unused",
                    &ctx,
                );
                self.menu_cmd(
                    ui,
                    has_session,
                    "Close Session and Its Media",
                    "file.close_session_media",
                    &ctx,
                );
                ui.separator();
                if ui
                    .add_enabled(
                        has_file || has_session,
                        egui::Button::new("Save").shortcut_text("Ctrl+S"),
                    )
                    .clicked()
                {
                    self.run("save", &ctx);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        has_file || has_session,
                        egui::Button::new("Save As...").shortcut_text("Ctrl+Shift+S"),
                    )
                    .clicked()
                {
                    self.run("saveas", &ctx);
                    ui.close();
                }
                self.menu_cmd(
                    ui,
                    has_wave_sel,
                    "Save Selection As...",
                    "file.save_selection_as",
                    &ctx,
                );
                self.menu_cmd(ui, any_dirty, "Save All", "file.save_all", &ctx);
                self.menu_cmd(ui, can_revert, "Revert to Saved", "file.revert", &ctx);
                ui.separator();
                ui.menu_button("Import", |ui| {
                    if mi(ui, "File...", "Ctrl+I") {
                        self.open_dialog();
                    }
                    self.menu_cmd(
                        ui,
                        true,
                        "Files into Multitrack...",
                        "multitrack.insert_files",
                        &ctx,
                    );
                    self.menu_cmd(ui, true, "Markers from CSV...", "marker.import", &ctx);
                    ui.separator();
                    self.menu_cmd(
                        ui,
                        true,
                        "Application Settings...",
                        "file.import_settings",
                        &ctx,
                    );
                });
                ui.menu_button("Export", |ui| {
                    self.menu_cmd(ui, has_file, "File...", "file.export_file", &ctx);
                    ui.add_enabled_ui(has_session, |ui| {
                        ui.menu_button("Multitrack Mixdown", |ui| {
                            self.menu_cmd(
                                ui,
                                true,
                                "Entire Session...",
                                "file.export_mixdown",
                                &ctx,
                            );
                            self.menu_cmd(
                                ui,
                                multi_sel,
                                "Time Selection...",
                                "file.export_mixdown_selection",
                                &ctx,
                            );
                        });
                    });
                    if ui
                        .add_enabled(has_session, egui::Button::new("Session..."))
                        .clicked()
                    {
                        self.save_session(true);
                        ui.close();
                    }
                    self.menu_cmd(ui, true, "Markers to CSV...", "marker.export", &ctx);
                    self.menu_cmd(
                        ui,
                        has_file,
                        "Analysis Report...",
                        "file.export_report",
                        &ctx,
                    );
                    ui.separator();
                    self.menu_cmd(
                        ui,
                        true,
                        "Application Settings...",
                        "file.export_settings",
                        &ctx,
                    );
                });
                ui.separator();
                self.menu_cmd(ui, true, "Batch Processor...", "file.batch_process", &ctx);
                ui.separator();
                if mi(ui, "Exit", "Ctrl+Q") {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                let undo_label = self.undo_label();
                let ul = format!("Undo {}", undo_label.clone().unwrap_or_default());
                if ui
                    .add_enabled(
                        undo_label.is_some(),
                        egui::Button::new(ul.trim_end()).shortcut_text("Ctrl+Z"),
                    )
                    .clicked()
                {
                    self.undo();
                    ui.close();
                }
                if mi(ui, "Redo", "Ctrl+Shift+Z") {
                    self.redo();
                }
                let last = self
                    .last_cmd
                    .as_deref()
                    .and_then(crate::commands::find)
                    .map(|c| format!("Repeat {}", c.title))
                    .unwrap_or_else(|| "Repeat Last Command".into());
                self.menu_cmd(ui, self.last_cmd.is_some(), &last, "edit.repeat_last", &ctx);
                ui.separator();
                ui.menu_button("Tools", |ui| {
                    for (label, id, sc, t) in [
                        ("Move", "tool.move", "V", Tool::Move),
                        ("Razor", "tool.razor", "R", Tool::Razor),
                        ("Slip", "tool.slip", "Y", Tool::Slip),
                        ("Time Selection", "tool.time", "T", Tool::TimeSel),
                        (
                            "Direct Selection",
                            "tool.direct_selection",
                            "A",
                            Tool::Direct,
                        ),
                        ("Marquee Selection", "tool.marquee", "E", Tool::Marquee),
                        ("Lasso Selection", "tool.lasso", "D", Tool::Lasso),
                        ("Paintbrush Selection", "tool.brush", "P", Tool::Brush),
                        ("Spot Healing Brush", "tool.spot_heal", "B", Tool::SpotHeal),
                    ] {
                        let on = self.tool == t;
                        if theme::radio_item(ui, true, on, label, sc).clicked() {
                            if let Err(e) = self.exec(id, &ctx) {
                                self.status = e;
                            }
                            ui.close();
                        }
                    }
                });
                ui.separator();
                ui.add_enabled_ui(wave_file, |ui| {
                    ui.menu_button("Enable Channels", |ui| {
                        let en = self.cur().map(|f| f.enabled_ch.clone()).unwrap_or_default();
                        for (i, (label, id)) in
                            [("Left", "edit.enable_left"), ("Right", "edit.enable_right")]
                                .into_iter()
                                .enumerate()
                        {
                            let on = en.get(i).copied().unwrap_or(false);
                            let exists = i < en.len();
                            if theme::check_item(ui, exists, on, label, "").clicked() {
                                if let Err(e) = self.exec(id, &ctx) {
                                    self.status = e;
                                }
                            }
                        }
                    });
                });
                self.clipboard_menu(ui, &ctx);
                ui.separator();
                if ui
                    .add_enabled(can_edit, egui::Button::new("Cut").shortcut_text("Ctrl+X"))
                    .clicked()
                {
                    self.cut();
                    ui.close();
                }
                if ui
                    .add_enabled(can_edit, egui::Button::new("Copy").shortcut_text("Ctrl+C"))
                    .clicked()
                {
                    self.copy();
                    ui.close();
                }
                self.menu_cmd(
                    ui,
                    wave_file && has_wave_sel,
                    "Copy to New",
                    "edit.copy_to_new",
                    &ctx,
                );
                if mi(ui, "Paste", "Ctrl+V") {
                    self.paste(false);
                }
                if mi(ui, "Paste to New", "Ctrl+Alt+V") {
                    self.paste_new();
                }
                if ui
                    .add_enabled(
                        wave_file,
                        egui::Button::new("Mix Paste...").shortcut_text("Ctrl+Shift+V"),
                    )
                    .clicked()
                {
                    self.paste(true);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        can_edit,
                        egui::Button::new("Delete").shortcut_text("Delete"),
                    )
                    .clicked()
                {
                    self.run("delete", &ctx);
                    ui.close();
                }
                ui.add_enabled_ui(multi_on, |ui| {
                    ui.menu_button("Ripple Delete", |ui| {
                        self.menu_cmd(
                            ui,
                            clip_sel,
                            "Selected Clips",
                            "edit.ripple_delete_clips",
                            &ctx,
                        );
                        self.menu_cmd(
                            ui,
                            multi_sel,
                            "Time Selection in All Tracks",
                            "edit.ripple_delete_time",
                            &ctx,
                        );
                    });
                });
                ui.separator();
                if ui
                    .add_enabled(
                        wave_file && has_wave_sel,
                        egui::Button::new("Crop").shortcut_text("Ctrl+T"),
                    )
                    .clicked()
                {
                    self.crop();
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Select", |ui| {
                    if mi(ui, "Select All", "Ctrl+A") {
                        self.select_all();
                    }
                    if mi(ui, "Deselect All", "Ctrl+Shift+A") {
                        self.run("none", &ctx);
                    }
                    self.menu_cmd(ui, true, "Select Current View", "edit.select_view", &ctx);
                    self.menu_cmd(
                        ui,
                        true,
                        "Select Playhead to Start",
                        "edit.select_to_start",
                        &ctx,
                    );
                    self.menu_cmd(
                        ui,
                        true,
                        "Select Playhead to End",
                        "edit.select_to_end",
                        &ctx,
                    );
                    self.menu_cmd(
                        ui,
                        multi_on,
                        "Select All Clips in Selected Track",
                        "edit.select_track_clips",
                        &ctx,
                    );
                    if wave_file {
                        ui.separator();
                        self.menu_cmd(
                            ui,
                            true,
                            "Select All Frequencies",
                            "spectral.select_all",
                            &ctx,
                        );
                        self.menu_cmd(
                            ui,
                            true,
                            "Select Inverse (Spectral)",
                            "spectral.invert",
                            &ctx,
                        );
                    }
                });
                ui.menu_button("Insert", |ui| {
                    if ui
                        .add_enabled(wave_file, egui::Button::new("Silence..."))
                        .clicked()
                    {
                        self.dialog = Some(Dialog::InsertSilence { secs: 1.0 });
                        ui.close();
                    }
                    if mi(ui, "Marker", "M") {
                        self.add_marker();
                    }
                    self.menu_cmd(
                        ui,
                        true,
                        "Files into Multitrack...",
                        "multitrack.insert_files",
                        &ctx,
                    );
                    if ui
                        .add_enabled(wave_file, egui::Button::new("Current File into Multitrack"))
                        .clicked()
                    {
                        self.insert_current_into_multitrack(false);
                        ui.close();
                    }
                });
                ui.menu_button("Marker", |ui| self.markers_menu(ui, &ctx));
                ui.add_enabled_ui(wave_file && has_wave_sel, |ui| {
                    ui.menu_button("Zero Crossings", |ui| {
                        self.menu_cmd(
                            ui,
                            true,
                            "Adjust Selection Inward",
                            "edit.zero_cross_in",
                            &ctx,
                        );
                        self.menu_cmd(
                            ui,
                            true,
                            "Adjust Selection Outward",
                            "edit.zero_cross_out",
                            &ctx,
                        );
                    });
                });
                ui.menu_button("Snapping", |ui| {
                    if theme::check_item(ui, true, self.snapping, "Snapping", "S").clicked() {
                        self.snapping = !self.snapping;
                        ui.close();
                    }
                });
                ui.separator();
                self.menu_cmd(ui, true, "Batch Process", "file.batch_process", &ctx);
                self.menu_cmd(
                    ui,
                    stereo_file,
                    "Extract Channels to Mono Files",
                    "edit.extract_mono",
                    &ctx,
                );
                self.menu_cmd(
                    ui,
                    wave_file,
                    "Interpret Sample Rate...",
                    "edit.interpret_sample_rate",
                    &ctx,
                );
                self.menu_cmd(
                    ui,
                    wave_file,
                    "Frequency Band Splitter...",
                    "edit.frequency_band_splitter",
                    &ctx,
                );
                ui.separator();
                if ui
                    .add_enabled(
                        wave_file,
                        egui::Button::new("Convert Sample Type...").shortcut_text("Shift+T"),
                    )
                    .clicked()
                {
                    if let Err(e) = self.exec("edit.convert", &ctx) {
                        self.status = e;
                    }
                    ui.close();
                }
                self.menu_cmd(ui, wave_file, "Gain...", "process.gain", &ctx);
                self.menu_cmd(
                    ui,
                    wave_file,
                    "Repair Clipped Peaks",
                    "process.declip",
                    &ctx,
                );
                ui.separator();
                self.menu_cmd(ui, clip_sel, "Edit Source File", "clip.edit_source", &ctx);
                ui.separator();
                self.menu_cmd(
                    ui,
                    true,
                    "Detect Video Decoder...",
                    "video.detect_decoder",
                    &ctx,
                );
                self.menu_cmd(
                    ui,
                    true,
                    "Keyboard Shortcuts...",
                    "edit.keyboard_shortcuts",
                    &ctx,
                );
                ui.menu_button("Preferences", |ui| {
                    for (i, c) in crate::prefs::CATEGORIES.iter().enumerate() {
                        if mi(
                            ui,
                            &format!("{c}..."),
                            if i == 0 { "Ctrl+Shift+K" } else { "" },
                        ) {
                            self.open_prefs(c);
                        }
                        if i == 0 {
                            ui.separator();
                        }
                    }
                });
            });
            ui.menu_button("Multitrack", |ui| {
                ui.menu_button("Track", |ui| {
                    self.menu_cmd(ui, true, "Add Mono Audio Track", "track.add_mono", &ctx);
                    self.menu_cmd(ui, true, "Add Stereo Audio Track", "track.add_stereo", &ctx);
                    ui.separator();
                    self.menu_cmd(
                        ui,
                        multi_on,
                        "Duplicate Selected Track",
                        "track.duplicate",
                        &ctx,
                    );
                    self.menu_cmd(ui, multi_on, "Delete Selected Track", "track.delete", &ctx);
                    self.menu_cmd(
                        ui,
                        multi_on,
                        "Rename Selected Track...",
                        "track.rename",
                        &ctx,
                    );
                    self.menu_cmd(ui, multi_on, "Move Track Up", "track.move_up", &ctx);
                    self.menu_cmd(ui, multi_on, "Move Track Down", "track.move_down", &ctx);
                    ui.separator();
                    self.menu_cmd(
                        ui,
                        multi_on,
                        "Mute / Unmute All Tracks",
                        "track.mute_all",
                        &ctx,
                    );
                    self.menu_cmd(ui, multi_on, "Clear All Solos", "track.solo_clear", &ctx);
                    ui.separator();
                    self.menu_cmd(
                        ui,
                        true,
                        "Show/Hide Volume Automation Lane",
                        "view.volume_automation",
                        &ctx,
                    );
                    self.menu_cmd(
                        ui,
                        multi_on,
                        "Add Volume Automation Point",
                        "track.automation_point",
                        &ctx,
                    );
                    self.menu_cmd(
                        ui,
                        multi_on,
                        "Clear Volume Automation",
                        "track.automation_clear",
                        &ctx,
                    );
                });
                if ui
                    .add_enabled(
                        self.selected_file.or(self.current).is_some(),
                        egui::Button::new("Insert File"),
                    )
                    .clicked()
                {
                    if let Some(id) = self.selected_file.or(self.current) {
                        self.insert_into_multitrack(id);
                    }
                    ui.close();
                }
                self.menu_cmd(ui, true, "Insert Files...", "multitrack.insert_files", &ctx);
                ui.separator();
                ui.add_enabled_ui(has_session, |ui| {
                    ui.menu_button("Mixdown Session to New File", |ui| {
                        if mi(ui, "Entire Session", "") {
                            self.mixdown(false, false);
                        }
                        if ui
                            .add_enabled(multi_sel, egui::Button::new("Time Selection"))
                            .clicked()
                        {
                            self.mixdown(true, false);
                            ui.close();
                        }
                    });
                    ui.menu_button("Bounce to New Track", |ui| {
                        self.menu_cmd(ui, true, "Selected Track", "multitrack.bounce_track", &ctx);
                        self.menu_cmd(
                            ui,
                            multi_sel,
                            "Time Selection",
                            "multitrack.bounce_selection",
                            &ctx,
                        );
                    });
                });
                ui.separator();
                self.metronome_menu(ui, &ctx);
                self.track_visibility_menu(ui, &ctx);
                theme::check_toggle(ui, &mut self.show_env, "Show Clip Volume Envelopes", "");
                theme::check_toggle(ui, &mut self.show_pan_env, "Show Clip Pan Envelopes", "");
                theme::check_toggle(ui, &mut self.snapping, "Snapping", "");
                ui.separator();
                self.menu_cmd(
                    ui,
                    has_session,
                    "Convert Curves for Audition",
                    "session.convert_for_audition",
                    &ctx,
                );
            });
            ui.menu_button("Clip", |ui| {
                self.menu_cmd(ui, clip_sel, "Edit Source File", "clip.edit_source", &ctx);
                self.menu_cmd(ui, clip_sel, "Edit Original", "clip.edit_original", &ctx);
                ui.separator();
                if ui
                    .add_enabled(
                        has_session,
                        egui::Button::new("Split").shortcut_text("Ctrl+K"),
                    )
                    .clicked()
                {
                    self.split_at_playhead();
                    ui.close();
                }
                self.menu_cmd(
                    ui,
                    has_session,
                    "Split at Markers",
                    "clip.split_markers",
                    &ctx,
                );
                if ui
                    .add_enabled(
                        clip_sel,
                        egui::Button::new("Delete").shortcut_text("Delete"),
                    )
                    .clicked()
                {
                    self.delete_selected_clip();
                    ui.close();
                }
                ui.add_enabled_ui(multi_on, |ui| {
                    ui.menu_button("Ripple Delete", |ui| {
                        self.menu_cmd(
                            ui,
                            clip_sel,
                            "Selected Clips",
                            "edit.ripple_delete_clips",
                            &ctx,
                        );
                        self.menu_cmd(
                            ui,
                            multi_sel,
                            "Time Selection in All Tracks",
                            "edit.ripple_delete_time",
                            &ctx,
                        );
                    });
                });
                ui.separator();
                self.menu_cmd(
                    ui,
                    clip_sel && multi_sel,
                    "Trim to Time Selection",
                    "clip.trim_selection",
                    &ctx,
                );
                self.menu_cmd(
                    ui,
                    clip_sel,
                    "Trim Start to Playhead",
                    "clip.trim_start",
                    &ctx,
                );
                self.menu_cmd(ui, clip_sel, "Trim End to Playhead", "clip.trim_end", &ctx);
                ui.separator();
                let muted = self
                    .session
                    .as_ref()
                    .and_then(|s| {
                        s.selected_clip
                            .and_then(|id| s.find_clip(id))
                            .map(|(t, c)| s.tracks[t].clips[c].muted)
                    })
                    .unwrap_or(false);
                self.menu_cmd_check(ui, clip_sel, muted, "Mute", "clip.mute", &ctx);
                self.menu_cmd(ui, clip_sel, "Duplicate", "clip.duplicate", &ctx);
                self.clip_gain_menu(ui, clip_sel, &ctx);
                self.menus_item(ui, clip_sel, "Bring to Front", "clip.bring_front", &ctx);
                self.clip_group_menu(ui, &ctx);
                self.menu_cmd(
                    ui,
                    self.session.is_some(),
                    "Match Clip Loudness",
                    "clip.match_loudness",
                    &ctx,
                );
                ui.separator();
                ui.add_enabled_ui(clip_sel, |ui| {
                    ui.menu_button("Fade In", |ui| {
                        self.menu_cmd(ui, true, "Fade In", "clip.fade_in", &ctx);
                        self.menus_item(ui, true, "Remove Fade In", "clip.fade_in_remove", &ctx);
                    });
                    ui.menu_button("Fade Out", |ui| {
                        self.menu_cmd(ui, true, "Fade Out", "clip.fade_out", &ctx);
                        self.menus_item(ui, true, "Remove Fade Out", "clip.fade_out_remove", &ctx);
                    });
                });
                self.menu_cmd(
                    ui,
                    clip_sel,
                    "Crossfade with Next Clip",
                    "clip.crossfade",
                    &ctx,
                );
                ui.separator();
                let stretch = self.stretch_mode;
                self.menu_cmd_check(
                    ui,
                    true,
                    stretch,
                    "Global Clip Stretching",
                    "view.toggle_clip_stretch",
                    &ctx,
                );
                self.menu_cmd(
                    ui,
                    clip_sel,
                    "Clip Effects in Effects Rack",
                    "window.clip_effects",
                    &ctx,
                );
            });
            ui.menu_button("Effects", |ui| self.effects_menu(ui));
            ui.menu_button("Favorites", |ui| self.favorites_menu(ui));
            ui.menu_button("View", |ui| {
                if theme::radio_item(
                    ui,
                    true,
                    self.mode == Mode::Waveform,
                    "Waveform Editor",
                    "8",
                )
                .clicked()
                {
                    self.request_mode(Mode::Waveform);
                    ui.close();
                }
                if theme::radio_item(
                    ui,
                    true,
                    self.mode == Mode::Multitrack,
                    "Multitrack Editor",
                    "9",
                )
                .clicked()
                {
                    self.request_mode(Mode::Multitrack);
                    ui.close();
                }
                ui.separator();
                theme::check_toggle(
                    ui,
                    &mut self.spectral,
                    "Show Spectral Frequency Display",
                    "Shift+D",
                );
                self.menu_cmd(
                    ui,
                    true,
                    "Show/Hide Spectral Pitch Display",
                    "view.pitch_display",
                    &ctx,
                );
                theme::check_toggle(ui, &mut self.auto_scroll, "Auto-scroll During Playback", "");
                self.menu_cmd(ui, true, "Next Colour Theme", "view.theme_toggle", &ctx);
                theme::check_toggle(ui, &mut self.show_env, "Show Clip Volume Envelopes", "");
                theme::check_toggle(ui, &mut self.show_pan_env, "Show Clip Pan Envelopes", "");
                self.menu_cmd(
                    ui,
                    true,
                    "Show/Hide Volume Automation Lane",
                    "view.volume_automation",
                    &ctx,
                );
                self.menu_cmd(
                    ui,
                    true,
                    "Add Volume Automation Point",
                    "track.automation_point",
                    &ctx,
                );
                self.menu_cmd(
                    ui,
                    true,
                    "Clear Volume Automation",
                    "track.automation_clear",
                    &ctx,
                );
                theme::check_toggle(
                    ui,
                    &mut self.return_on_stop,
                    "Return Playhead to Start on Stop",
                    "",
                );
                ui.separator();
                if mi(ui, "Zoom In", "=") {
                    self.zoom(0.5, None);
                }
                if mi(ui, "Zoom Out", "-") {
                    self.zoom(2.0, None);
                }
                if mi(ui, "Zoom Out Full", "\\") {
                    self.zoom_full();
                }
                if mi(ui, "Zoom to Selection", "") {
                    self.zoom_selection();
                }
                self.menu_cmd(ui, true, "Zoom In Amplitude", "view.zoom_in_amp", &ctx);
                self.menu_cmd(ui, true, "Zoom Out Amplitude", "view.zoom_out_amp", &ctx);
                self.menu_cmd(
                    ui,
                    true,
                    "Reset Amplitude Zoom",
                    "view.zoom_reset_amp",
                    &ctx,
                );
                ui.separator();
                ui.menu_button("Markers", |ui| self.markers_menu(ui, &ctx));
                ui.separator();
                ui.menu_button("Time Display", |ui| {
                    for (i, (f, name)) in TimeFormat::ALL.iter().enumerate() {
                        if theme::radio_item(ui, true, self.time_format == *f, name, "").clicked() {
                            self.set_time_format(i);
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Status Bar", |ui| {
                    for (i, n) in STATUS_ITEMS.iter().enumerate() {
                        theme::check_toggle(ui, &mut self.status_items[i], n, "");
                    }
                });
            });
            ui.menu_button("Window", |ui| {
                self.window_menu(ui);
            });
            ui.menu_button("Help", |ui| {
                self.menu_cmd(ui, true, "AudioCraft Help...", "help.manual", &ctx);
                ui.separator();
                if mi(ui, "Command Palette...", "Ctrl+Shift+P") {
                    self.palette = Some(String::new());
                }
                if mi(ui, "Keyboard Shortcuts", "") {
                    self.dialog = Some(Dialog::Shortcuts);
                }
                ui.separator();
                self.menu_cmd(
                    ui,
                    true,
                    "Open Settings Folder",
                    "help.settings_folder",
                    &ctx,
                );
                ui.separator();
                if mi(ui, "About AudioCraft", "") {
                    self.dialog = Some(Dialog::About);
                }
            });
        });
    }

    pub fn edit_selected_clip_source(&mut self) {
        let fid = self.session.as_ref().and_then(|s| {
            let (t, c) = s.selected_clip.and_then(|id| s.find_clip(id))?;
            Some(s.tracks[t].clips[c].file_id)
        });
        if let Some(fid) = fid {
            self.switch_mode(Mode::Waveform);
            self.current = Some(fid);
            self.essential.from_session = true;
        }
    }

    /// Leave the clip's file without applying anything and return to the multitrack session.
    pub fn essential_back(&mut self) {
        self.essential.from_session = false;
        if self.session.is_some() {
            self.switch_mode(Mode::Multitrack);
            self.status = "Back to the session; nothing was applied".into();
        }
    }

    fn favorites_menu(&mut self, ui: &mut egui::Ui) {
        let wave = self.mode == Mode::Waveform && self.cur().is_some();
        ui.add_enabled_ui(wave, |ui| {
            for name in crate::panels::FAVORITES {
                if ui.button(*name).clicked() {
                    self.apply_favorite(name);
                    ui.close();
                }
            }
        });
    }

    // ------------------------------------------------------------ toolbar

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let w = self.mode == Mode::Waveform;
            if seg_button(ui, Icon::Waveform, "Waveform", w)
                .on_hover_text("Waveform Editor (8)")
                .clicked()
            {
                self.request_mode(Mode::Waveform);
            }
            if seg_button(ui, Icon::Multitrack, "Multitrack", !w)
                .on_hover_text("Multitrack Editor (9)")
                .clicked()
            {
                self.request_mode(Mode::Multitrack);
            }
            ui.add_space(8.0);
            sep(ui);
            // unavailable buttons are greyed out (the spectral display only exists in the
            // Waveform Editor; Move/Razor/Slip only in Multitrack; the spectral tools need the
            // spectral display)
            let r = ui
                .add_enabled_ui(w, |ui| {
                    icon_button(
                        ui,
                        Icon::Spectral,
                        self.spectral && w,
                        "Show Spectral Frequency Display (Shift+D)",
                    )
                })
                .inner;
            if r.clicked() {
                self.spectral = !self.spectral;
            }
            let r = ui
                .add_enabled_ui(w, |ui| {
                    icon_button(
                        ui,
                        Icon::Pitch,
                        self.pitch_display && w,
                        "Show Spectral Pitch Display",
                    )
                })
                .inner;
            if r.clicked() {
                self.pitch_display = !self.pitch_display;
            }
            sep(ui);
            // a spectral tool chosen while it cannot be used falls back to Time Selection
            let spec_ok = w && self.spectral;
            if (self.tool.is_spectral() && !spec_ok)
                || (!w && self.tool.is_spectral())
                || (w
                    && matches!(
                        self.tool,
                        Tool::Move | Tool::Direct | Tool::Razor | Tool::Slip
                    ))
            {
                self.tool = Tool::TimeSel;
            }
            for (t, ic, tip) in [
                (Tool::Move, Icon::Move, "Move Tool (V)"),
                (
                    Tool::Direct,
                    Icon::Direct,
                    "Direct Selection Tool (A): keyframes and their curve handles",
                ),
                (Tool::Razor, Icon::Razor, "Razor Tool (R)"),
                (Tool::Slip, Icon::Slip, "Slip Tool (Y)"),
                (Tool::TimeSel, Icon::TimeSel, "Time Selection Tool (T)"),
                (Tool::Marquee, Icon::Marquee, "Marquee Selection Tool (E)"),
                (Tool::Lasso, Icon::Lasso, "Lasso Selection Tool (D)"),
                (Tool::Brush, Icon::Brush, "Paintbrush Selection Tool (P)"),
                (Tool::SpotHeal, Icon::Heal, "Spot Healing Brush Tool (B)"),
            ] {
                let enabled = match t {
                    Tool::TimeSel => true,
                    Tool::Move | Tool::Direct | Tool::Razor | Tool::Slip => !w,
                    _ => spec_ok,
                };
                let r = ui
                    .add_enabled_ui(enabled, |ui| icon_button(ui, ic, self.tool == t, tip))
                    .inner;
                if r.clicked() {
                    self.tool = t;
                }
            }
            if self.tool == Tool::Brush || self.tool == Tool::SpotHeal {
                ui.add_space(6.0);
                ui.label(RichText::new("Size").small().color(theme::text_dim()));
                ui.add(
                    egui::DragValue::new(&mut self.drag_state.brush_px)
                        .range(2.0..=200.0)
                        .speed(0.5)
                        .suffix(" px"),
                )
                .on_hover_text("Brush size");
                if self.tool == Tool::Brush {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Opacity").small().color(theme::text_dim()));
                    ui.add(
                        egui::DragValue::new(&mut self.drag_state.brush_opacity)
                            .range(1.0..=100.0)
                            .speed(0.5)
                            .suffix(" %"),
                    )
                    .on_hover_text("How strongly painted areas are selected");
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                self.workspace_bar_rtl(ui);
                ui.add_space(12.0);
                ui.label(RichText::new("AudioCraft").color(theme::accent()).strong());
            });
        });
    }

    // ------------------------------------------------------------ screenshot

    fn screenshot_tick(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        ctx.request_repaint();
        // diagnostic: AUDIOCRAFT_SHOT_PLUGIN=name opens that installed VST3/CLAP effect in the
        // Plug-ins panel, and the shot also saves `<name>-native.png` with the docked editor
        if let Ok(want) = std::env::var("AUDIOCRAFT_SHOT_PLUGIN") {
            if self.frame_count == 10 {
                self.shot_open_plugin(&want);
            }
            // play through the app's own engine (rendered here, no sound device) and report
            // the rack's levels; at frame 70 a live edit turns the first parameter down
            if (40..110).contains(&self.frame_count) {
                if self.frame_count == 40 {
                    let mut e = self.engine.lock();
                    e.playing = true;
                    e.pos = 0.0;
                    e.end = 1e9;
                    e.looping = true;
                }
                if self.frame_count == 70 {
                    if let Some(id) = self.current {
                        let t = crate::rack::RackTarget::File(id);
                        if let Some((_, mut slots, _)) = self.rack_get(t) {
                            if let Some(crate::dsp::EffectSlot {
                                effect: crate::dsp::Effect::Plugin { params, .. },
                                ..
                            }) = slots.get_mut(0)
                            {
                                for p in params.iter_mut() {
                                    p.1 = 0.0;
                                }
                            }
                            self.rack_set(t, slots, None);
                        }
                    }
                }
                let mut e = self.engine.lock();
                e.render(800);
                if self.frame_count == 69 || self.frame_count == 109 {
                    eprintln!(
                        "frame {}: rack in {:?} out {:?}",
                        self.frame_count, e.rack_in_meter, e.rack_out_meter
                    );
                }
                if self.frame_count == 109 {
                    e.playing = false;
                }
            }
            if self.frame_count == 100 || self.frame_count == 106 {
                let r = self.plug_toggle_pop();
                let open = self
                    .plug_dock
                    .wins
                    .first()
                    .map(|w| (w.ed.is_open(), w.embed.is_some()));
                eprintln!(
                    "frame {}: pop/dock {:?} -> {} (open, docked) {:?}",
                    self.frame_count, r, self.status, open
                );
            }
            if self.frame_count == 118 {
                for w in &self.plug_dock.wins {
                    let d = w
                        .embed
                        .as_ref()
                        .map(|e| e.describe())
                        .unwrap_or_else(|| "floating".into());
                    eprintln!(
                        "plug-in {}: open {} live {} focus-in-plugin {} {d}",
                        w.name,
                        w.ed.is_open(),
                        w.ed.is_live(),
                        crate::plugdock::native::focus_in_plugin()
                    );
                    if let Some((cw, ch, px)) = w
                        .embed
                        .as_ref()
                        .and_then(|e| crate::plugdock::native::capture(e.window()))
                    {
                        if let Some(img) = image::RgbaImage::from_raw(cw, ch, px) {
                            let _ = img.save(path.with_file_name("plugin-editor.png"));
                        }
                    }
                }
                let np = path.with_file_name(format!(
                    "{}-native.png",
                    path.file_stem().and_then(|s| s.to_str()).unwrap_or("shot")
                ));
                match crate::plugdock::native::capture(self.plug_dock.main) {
                    Some((w, h, px)) => {
                        if let Some(img) = image::RgbaImage::from_raw(w, h, px) {
                            let _ = img.save(&np);
                        }
                    }
                    None => eprintln!("native capture unavailable"),
                }
            }
        }
        if self.frame_count >= 120 && !self.shot_requested {
            self.shot_requested = true;
            let done = self.shot_done.clone();
            ctx.request_screenshot(move |img| {
                let [w, h] = img.size;
                let mut bytes = Vec::with_capacity(w * h * 4);
                for p in &img.pixels {
                    bytes.extend_from_slice(&[p.r(), p.g(), p.b(), 255]);
                }
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                    if let Err(e) = buf.save(&path) {
                        eprintln!("screenshot save failed: {e}");
                    }
                }
                done.store(true, Ordering::SeqCst);
            });
        }
        if self.shot_done.load(Ordering::SeqCst) || self.frame_count > 3000 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Screenshot diagnostic: put the first installed effect whose name contains `want` into
    /// the current file's rack (its editor opens in the Plug-ins panel, as a click would).
    fn shot_open_plugin(&mut self, want: &str) {
        let Some(id) = self.current else {
            eprintln!("no file for the plug-in shot (add --demo)");
            return;
        };
        let list = crate::plugins::scan_hosted(false);
        let want = want.to_lowercase();
        let Some(e) = list.iter().find(|e| e.name.to_lowercase().contains(&want)) else {
            eprintln!(
                "no installed effect matches '{want}': {:?}",
                list.iter().map(|e| &e.name).collect::<Vec<_>>()
            );
            return;
        };
        self.rack_insert(
            crate::rack::RackTarget::File(id),
            None,
            crate::plugins::effect_for(e),
        );
    }

    fn update_meter_hold(&mut self, t: f64) {
        let m = self.engine.lock().meter;
        for c in 0..2 {
            // Only a level above 0 dBFS lights the clip indicator: a file touching full scale
            // (e.g. a 16-bit -32768 sample) is not clipping, and a quiet file must never
            // inherit a red light from earlier playback.
            if crate::theme::is_over(m[c]) {
                self.meter_clip[c] = true;
            }
            if m[c] >= self.meter_hold[c] || t - self.meter_hold_time[c] > 1.5 {
                self.meter_hold[c] = m[c];
                self.meter_hold_time[c] = t;
            }
        }
    }

    // ------------------------------------------------------------ layout

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        // right-click picks what the status bar shows
        let bar = ui.interact(
            ui.max_rect(),
            ui.scope_id().with("status_menu"),
            egui::Sense::click(),
        );
        bar.context_menu(|ui| {
            ui.label(RichText::new("Show in the status bar").color(theme::text_dim()));
            for (i, n) in STATUS_ITEMS.iter().enumerate() {
                theme::check_toggle(ui, &mut self.status_items[i], n, "");
            }
        });
        ui.horizontal_centered(|ui| {
            let (playing, rec) = {
                let s = self.engine.lock();
                (s.playing, s.recording)
            };
            let st = if rec { "Recording" } else if playing { "Playing" } else { "Stopped" };
            let col = if rec { Color32::from_rgb(235, 90, 90) } else if playing { Color32::from_rgb(110, 210, 140) } else { theme::text() };
            ui.label(RichText::new(st).color(col).strong());
            ui.add_space(12.0);
            ui.label(RichText::new(&self.status).color(theme::text_dim()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let info = match self.mode {
                    Mode::Waveform => self.cur().map(|f| {
                        let chs = match f.data.channels() {
                            1 => "Mono".to_string(),
                            2 => "Stereo".to_string(),
                            n => format!("{n} ch"),
                        };
                        let bits = if f.float { "32-bit (float)".to_string() } else { format!("{}-bit", f.bits) };
                        (format!("{} Hz \u{2022} {} \u{2022} {}", f.data.rate, bits, chs), f.size_bytes() as u64, f.data.duration())
                    }),
                    Mode::Multitrack => self.session.as_ref().map(|s| {
                        // the audio the session uses, as it sits in memory
                        let used: std::collections::HashSet<u64> = s.tracks.iter().flat_map(|t| t.clips.iter().map(|c| c.file_id)).collect();
                        let bytes: u64 = self.files.iter().filter(|f| used.contains(&f.id)).map(|f| f.size_bytes() as u64).sum();
                        (format!("{} Hz \u{2022} {} \u{2022} 32-bit Mixing", s.rate, if s.bits == 32 { "32-bit (float)".to_string() } else { format!("{}-bit", s.bits) }), bytes, s.length())
                    }),
                };
                let path = match self.mode {
                    Mode::Waveform => self.cur().and_then(|f| f.path.clone()),
                    Mode::Multitrack => self.session.as_ref().and_then(|s| s.path.clone()),
                };
                let sys = self.sysmon.tick(path.as_deref());
                let on = self.status_items;
                let sep = |ui: &mut egui::Ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 16.0), egui::Sense::hover());
                    ui.painter().line_segment([egui::pos2(r.center().x, r.top()), egui::pos2(r.center().x, r.bottom())], egui::Stroke::new(1.0, crate::theme::gray(55)));
                };
                let item = |ui: &mut egui::Ui, t: String| {
                    ui.label(RichText::new(t).color(theme::text()));
                };
                let fmt_bytes = |b: u64| {
                    let gb = b as f64 / 1_073_741_824.0;
                    if gb >= 1.0 { format!("{gb:.2} GB") } else { format!("{:.2} MB", b as f64 / 1_048_576.0) }
                };
                if on[6] {
                    ui.label(RichText::new(format!("Output: {}", self.engine.out_device_name)).color(theme::text_dim()));
                    sep(ui);
                }
                if on[5] {
                    // the audio engine's load, like a DAW's CPU meter: how much of each audio
                    // buffer's time goes into working it out
                    let load = self.engine.lock().dsp_load;
                    let col = if load > 80.0 { Color32::from_rgb(235, 90, 90) } else if load > 50.0 { Color32::from_rgb(235, 200, 80) } else { theme::text() };
                    let r = ui.label(RichText::new(format!("CPU {load:.0}%")).color(col));
                    let whole = sys.cpu.map(|c| format!("{c:.1}%")).unwrap_or_else(|| "-".into());
                    r.on_hover_text(format!("Audio engine load: {load:.1}% of each buffer's time\nWhole program: {whole} of the processor"));
                    sep(ui);
                    // keep the reading fresh while nothing else redraws
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(1000));
                }
                if on[4] {
                    if let Some(m) = sys.mem {
                        item(ui, format!("RAM {}", fmt_bytes(m)));
                        sep(ui);
                    }
                }
                if on[3] {
                    if let Some(f) = sys.disk_free {
                        item(ui, format!("{} free", fmt_bytes(f)));
                        sep(ui);
                    }
                }
                if let Some((fmt, size, dur)) = info {
                    if on[2] {
                        item(ui, audio::fmt_time(dur));
                        sep(ui);
                    }
                    if on[1] && size > 0 {
                        item(ui, fmt_bytes(size));
                        sep(ui);
                    }
                    if on[0] {
                        item(ui, fmt);
                    }
                }
            });
        });
    }

    fn center(&mut self, ui: &mut egui::Ui) {
        let title = match self.mode {
            Mode::Waveform => self
                .cur()
                .map(|f| format!("Editor: {}{}", f.name, if f.dirty() { " *" } else { "" }))
                .unwrap_or_else(|| "Editor".into()),
            Mode::Multitrack => {
                let s = self.session.as_ref();
                format!(
                    "Editor: {}{}",
                    s.map(|s| s.name.clone()).unwrap_or_default(),
                    if s.map(|s| s.dirty).unwrap_or(false) {
                        " *"
                    } else {
                        ""
                    }
                )
            }
        };
        let mut tab = self.center_tab;
        let tabs: Vec<(CenterTab, &str)> = vec![
            (CenterTab::Editor, title.as_str()),
            (CenterTab::Mixer, "Mixer"),
        ];
        theme::tab_header(ui, &tabs, &mut tab);
        if tab != self.center_tab {
            if tab == CenterTab::Mixer {
                self.ensure_mixer_session();
                self.switch_mode(Mode::Multitrack);
            }
            self.center_tab = tab;
        }
        // a workspace with its own Transport panel (Classic) has no transport under the editor
        let transport_h = if self
            .ws
            .layout
            .contains(crate::workspace::PanelId::Transport)
        {
            0.0
        } else {
            46.0
        };
        let avail = ui.available_rect_before_wrap();
        let body = egui::Rect::from_min_max(
            avail.min,
            egui::pos2(avail.right(), avail.bottom() - transport_h),
        );
        let trans = egui::Rect::from_min_max(
            egui::pos2(avail.left(), avail.bottom() - transport_h),
            avail.max,
        );
        let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
        body_ui.set_clip_rect(body);
        match (self.mode, self.center_tab) {
            (Mode::Multitrack, CenterTab::Mixer) => self.mixer_view(&mut body_ui),
            (Mode::Multitrack, _) => self.multitrack_view(&mut body_ui),
            (Mode::Waveform, _) => self.waveform_view(&mut body_ui),
        }
        if transport_h > 0.0 {
            let mut tui = ui.new_child(egui::UiBuilder::new().max_rect(trans));
            self.transport(&mut tui);
        }
        ui.allocate_rect(avail, egui::Sense::hover());
    }

    fn transport(&mut self, ui: &mut egui::Ui) {
        let r = ui.max_rect();
        ui.painter().rect_filled(
            r,
            0.0,
            theme::own_or(crate::theme::gray(31), theme::panel()),
        );
        ui.painter().line_segment(
            [r.left_top(), r.right_top()],
            egui::Stroke::new(1.0, crate::theme::gray(20)),
        );
        let (playing, rec, looping) = {
            let s = self.engine.lock();
            (s.playing, s.recording, s.looping)
        };
        ui.horizontal_centered(|ui| {
            ui.add_space(10.0);
            let t = self.cursor();
            let txt = self.fmt(t);
            let resp = ui.add(
                egui::Label::new(
                    RichText::new(txt)
                        .font(egui::FontId::proportional(19.0))
                        .color(theme::time_blue()),
                )
                .sense(egui::Sense::click()),
            );
            if resp
                .on_hover_text("Click to change the time format (View > Time Display)")
                .clicked()
            {
                let next = self.time_format.next();
                let i = TimeFormat::ALL
                    .iter()
                    .position(|f| f.0 == next)
                    .unwrap_or(0);
                self.set_time_format(i);
            }
            ui.add_space(24.0);
            let sz = egui::vec2(30.0, 28.0);
            if theme::icon_button_sized(ui, Icon::Stop, false, "Stop", sz).clicked() {
                self.stop();
            }
            if theme::icon_button_sized(ui, Icon::Play, playing, "Play (Space)", sz).clicked() {
                if playing { self.stop() } else { self.play() }
            }
            if theme::icon_button_sized(ui, Icon::Pause, false, "Pause", sz).clicked() {
                self.pause();
            }
            if theme::icon_button_sized(ui, Icon::ToStart, false, "Move Playhead to Previous", sz)
                .clicked()
            {
                let t = self.prev_point();
                self.set_cursor(t);
            }
            let rw = theme::icon_button_sized(ui, Icon::Rewind, false, "Rewind (hold)", sz);
            if rw.is_pointer_button_down_on() {
                let step = self.view_width() / 90.0;
                let t = (self.cursor() - step).max(0.0);
                self.set_cursor(t);
                ui.ctx().request_repaint();
            }
            let ff = theme::icon_button_sized(ui, Icon::Forward, false, "Fast Forward (hold)", sz);
            if ff.is_pointer_button_down_on() {
                let step = self.view_width() / 90.0;
                let t = self.cursor() + step;
                self.set_cursor(t);
                ui.ctx().request_repaint();
            }
            if theme::icon_button_sized(ui, Icon::ToEnd, false, "Move Playhead to Next", sz)
                .clicked()
            {
                let t = self.next_point();
                self.set_cursor(t);
            }
            // in Multitrack, recording needs a track armed (R); otherwise the button is greyed
            let can_rec = rec
                || self.mode == Mode::Waveform
                || self
                    .session
                    .as_ref()
                    .map(|s| s.tracks.iter().any(|t| t.arm && t.can_record()))
                    .unwrap_or(false);
            let tip = if self.extra.record_ready {
                "Record is ready: press Play to start (click to cancel)"
            } else if can_rec {
                "Record (Shift+Space): get ready, then Play starts recording"
            } else {
                "Record: choose a track input and arm the track (R) first"
            };
            let lit = rec || self.record_lit(ui.ctx());
            if ui
                .add_enabled_ui(can_rec, |ui| {
                    theme::icon_button_sized(ui, Icon::Record, lit, tip, sz)
                })
                .inner
                .clicked()
            {
                self.toggle_record();
            }
            if theme::icon_button_sized(ui, Icon::Loop, looping, "Loop Playback", sz).clicked() {
                let mut s = self.engine.lock();
                s.looping = !s.looping;
            }
            // the zoom buttons only where there is room for all six beside the transport
            if ui.available_width() < 6.0 * 30.0 + 12.0 {
                return;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                let sz = egui::vec2(26.0, 26.0);
                if theme::icon_button_sized(ui, Icon::ZoomSelIn, false, "Zoom to Selection", sz)
                    .clicked()
                {
                    self.zoom_selection();
                }
                if theme::icon_button_sized(ui, Icon::ZoomFull, false, "Zoom Out Full (\\)", sz)
                    .clicked()
                {
                    self.zoom_full();
                }
                if theme::icon_button_sized(ui, Icon::ZoomOut, false, "Zoom Out (Time) (-)", sz)
                    .clicked()
                {
                    self.zoom(2.0, None);
                }
                if theme::icon_button_sized(ui, Icon::ZoomIn, false, "Zoom In (Time) (=)", sz)
                    .clicked()
                {
                    self.zoom(0.5, None);
                }
                if theme::icon_button_sized(ui, Icon::ZoomOutAmp, false, "Zoom Out (Amplitude)", sz)
                    .clicked()
                {
                    self.drag_state.amp_zoom = (self.drag_state.amp_zoom / 1.5).max(1.0);
                }
                if theme::icon_button_sized(ui, Icon::ZoomInAmp, false, "Zoom In (Amplitude)", sz)
                    .clicked()
                {
                    self.drag_state.amp_zoom = (self.drag_state.amp_zoom * 1.5).min(64.0);
                }
            });
        });
    }
}
