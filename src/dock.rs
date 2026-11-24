//! Draws the current workspace: panel groups (tabbed stacks) in columns and rows around the
//! editor, and the panels only workspaces show (Video, Tracks, Metadata, Phase Analysis, Phase
//! Meter, Match Loudness, Diagnostics, Transport, Time, Zoom).

use crate::app::{App, Mode};
use crate::layout::sub_panel;
use crate::theme::{self, Icon};
use crate::workspace::{Group, Layout, PanelId};
use egui::{Color32, FontId, Frame, Margin, Rect, RichText, Sense, Stroke, pos2, vec2};

/// The smallest bottom-row height that still shows all of Selection/View (its tab, the time
/// format dropdown and the Start/End/Duration rows): the row's resize stopper.
pub const SELECTION_VIEW_MIN_H: f32 = 116.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Region {
    Left,
    Left2,
    Right,
    Right2,
    Top,
    Bottom,
    Bottom2,
    /// A separate window (index into `Layout::windows`).
    Float(usize),
}

impl Region {
    fn key(self) -> usize {
        match self {
            Region::Left => 0,
            Region::Left2 => 1,
            Region::Right => 2,
            Region::Right2 => 3,
            Region::Top => 4,
            Region::Bottom => 5,
            Region::Bottom2 => 6,
            Region::Float(i) => 100 + i,
        }
    }
}

static NO_GROUPS: Vec<Group> = Vec::new();

fn groups(l: &Layout, r: Region) -> &Vec<Group> {
    match r {
        Region::Float(i) => l.windows.get(i).map(|w| &w.groups).unwrap_or(&NO_GROUPS),
        Region::Left => &l.left,
        Region::Left2 => &l.left2,
        Region::Right => &l.right,
        Region::Right2 => &l.right2,
        Region::Top => &l.top,
        Region::Bottom => &l.bottom,
        Region::Bottom2 => &l.bottom2,
    }
}

fn groups_mut(l: &mut Layout, r: Region) -> &mut Vec<Group> {
    match r {
        Region::Float(i) => {
            if i >= l.windows.len() {
                l.windows.resize_with(i + 1, Default::default);
            }
            &mut l.windows[i].groups
        }
        Region::Left => &mut l.left,
        Region::Left2 => &mut l.left2,
        Region::Right => &mut l.right,
        Region::Right2 => &mut l.right2,
        Region::Top => &mut l.top,
        Region::Bottom => &mut l.bottom,
        Region::Bottom2 => &mut l.bottom2,
    }
}

/// Split `len` between groups: fixed sizes (negative weights) first, the rest by weight.
fn split(len: f32, gs: &[Group]) -> Vec<f32> {
    let fixed: f32 = gs
        .iter()
        .filter(|g| g.weight < 0.0)
        .map(|g| -g.weight)
        .sum();
    let wsum: f32 = gs
        .iter()
        .filter(|g| g.weight > 0.0)
        .map(|g| g.weight)
        .sum::<f32>()
        .max(1e-6);
    let rest = (len - fixed).max(0.0);
    gs.iter()
        .map(|g| {
            if g.weight < 0.0 {
                (-g.weight).min(len)
            } else {
                rest * g.weight / wsum
            }
        })
        .collect()
}

/// While docked panels draw, their own headers are left out (the group draws the tabs).
pub fn set_skip_headers(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("ac_skip_headers"), on));
}

pub fn skip_headers(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new("ac_skip_headers")))
        .unwrap_or(false)
}

impl App {
    /// Lay out the workspace's panels; the editor takes what is left (drawn by the caller).
    pub fn dock(&mut self, ui: &mut egui::Ui) {
        let lay = self.ws.layout.clone();
        let key = self.ws.current.clone();
        let fill = Frame::new().fill(theme::bg()).inner_margin(Margin::same(1));
        if !lay.bottom2.is_empty() {
            egui::Panel::bottom(egui::Id::new(("ws_b2", &key)))
                .resizable(true)
                .default_size(lay.bottom2_h)
                .size_range(60.0..=400.0)
                .frame(fill)
                .show(ui, |ui| self.dock_row(ui, Region::Bottom2));
        }
        if !lay.left.is_empty() {
            egui::Panel::left(egui::Id::new(("ws_l", &key)))
                .resizable(true)
                .default_size(lay.left_w)
                .size_range(160.0..=900.0)
                .frame(fill)
                .show(ui, |ui| self.dock_column(ui, Region::Left));
        }
        if !lay.left2.is_empty() {
            egui::Panel::left(egui::Id::new(("ws_l2", &key)))
                .resizable(true)
                .default_size(lay.left2_w)
                .size_range(90.0..=600.0)
                .frame(fill)
                .show(ui, |ui| self.dock_column(ui, Region::Left2));
        }
        // a plug-in editor that just opened widens its column once to fit (the person can
        // still drag the border afterwards); the editor in the middle keeps 600 points
        let fit = self.plug_dock.fit_px.take().map(|px| {
            // the panel's frames, padding and scroll bar around the editor
            let w = px as f32 / ui.ctx().pixels_per_point().max(0.1) + 50.0;
            let room = ui.available_width()
                - 600.0
                - if lay.right.is_empty() || lay.right2.is_empty() {
                    0.0
                } else {
                    lay.right_w.min(lay.right2_w)
                };
            w.min(room).max(300.0)
        });
        let fit_in = |gs: &Vec<crate::workspace::Group>| {
            fit.filter(|_| gs.iter().any(|g| g.tabs.contains(&PanelId::PlugIns)))
        };
        if !lay.right2.is_empty() {
            let mut p = egui::Panel::right(egui::Id::new(("ws_r2", &key)))
                .resizable(true)
                .default_size(lay.right2_w)
                .size_range(60.0..=1600.0)
                .frame(fill);
            if let Some(w) = fit_in(&lay.right2) {
                p = p.exact_size(w);
            }
            p.show(ui, |ui| self.dock_column(ui, Region::Right2));
        }
        if !lay.right.is_empty() {
            let mut p = egui::Panel::right(egui::Id::new(("ws_r", &key)))
                .resizable(true)
                .default_size(lay.right_w)
                .size_range(160.0..=1600.0)
                .frame(fill);
            if let Some(w) = fit_in(&lay.right) {
                p = p.exact_size(w);
            }
            p.show(ui, |ui| self.dock_column(ui, Region::Right));
        }
        if !lay.bottom.is_empty() {
            // a stopper: with Selection/View in the bottom row, the row can't be dragged
            // smaller than that panel's format dropdown and its two rows of times
            let min_b = if lay
                .bottom
                .iter()
                .any(|g| g.tabs.contains(&crate::workspace::PanelId::SelectionView))
            {
                SELECTION_VIEW_MIN_H
            } else {
                60.0
            };
            egui::Panel::bottom(egui::Id::new(("ws_b", &key)))
                .resizable(true)
                .default_size(lay.bottom_h.max(min_b))
                .size_range(min_b..=700.0)
                .frame(fill)
                .show(ui, |ui| self.dock_row(ui, Region::Bottom));
        }
        if !lay.top.is_empty() {
            egui::Panel::top(egui::Id::new(("ws_t", &key)))
                .resizable(true)
                .default_size(lay.top_h)
                .size_range(80.0..=800.0)
                .frame(fill)
                .show(ui, |ui| self.dock_row(ui, Region::Top));
        }
    }

    /// Panels in windows of their own: Window > New Window, a panel's Undock Panel, and the
    /// dual-monitor workspace's second window. Closing a window docks its panels back.
    pub fn float_windows(&mut self, ctx: &egui::Context) {
        let wins: Vec<(u64, [f32; 2], String)> = self
            .ws
            .layout
            .windows
            .iter()
            .map(|w| {
                let names: Vec<&str> = w
                    .groups
                    .iter()
                    .flat_map(|g| g.tabs.iter().map(|t| t.title()))
                    .collect();
                (
                    w.id,
                    w.size,
                    if names.is_empty() {
                        "AudioCraft - Panels".to_string()
                    } else {
                        format!("AudioCraft - {}", names.join(", "))
                    },
                )
            })
            .collect();
        let mut close: Vec<u64> = Vec::new();
        for (i, (id, size, title)) in wins.into_iter().enumerate() {
            let vid = egui::ViewportId::from_hash_of(("ac_float", id));
            let mut builder = egui::ViewportBuilder::default()
                .with_title(title)
                .with_inner_size(size)
                .with_min_inner_size([260.0, 180.0]);
            if self.screenshot.is_some() {
                builder = builder.with_active(false);
            }
            let closed = ctx.show_viewport_immediate(vid, builder, |ui, _class| {
                ui.painter().rect_filled(ui.max_rect(), 0.0, theme::bg());
                egui::Panel::top(egui::Id::new(("float_bar", id))).exact_size(26.0).frame(Frame::new().fill(theme::panel_header()).inner_margin(Margin::symmetric(6, 2))).show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.menu_button("Add Panel", |ui| {
                            for p in PanelId::ALL {
                                if p == PanelId::PlugIns || p == PanelId::Mixer && self.ws.layout.contains(p) {
                                    continue;
                                }
                                let here = self.ws.layout.windows.iter().find(|w| w.id == id).map(|w| w.groups.iter().any(|g| g.tabs.contains(&p))).unwrap_or(false);
                                if ui.add_enabled(!here, egui::Button::new(p.title())).clicked() {
                                    self.ws.layout.add_to_window(id, p);
                                    ui.close();
                                }
                            }
                        });
                        if ui.button("Dock Back").on_hover_text("Put these panels back in the main window and close this window").clicked() {
                            close.push(id);
                        }
                    });
                });
                egui::CentralPanel::default().frame(Frame::new().fill(theme::bg()).inner_margin(Margin::same(1))).show(ui, |ui| {
                    if groups(&self.ws.layout, Region::Float(i)).is_empty() {
                        ui.centered_and_justified(|ui| ui.label(RichText::new("An empty window: add panels with Add Panel above.").color(theme::text_dim())));
                    } else {
                        self.dock_column(ui, Region::Float(i));
                    }
                });
                if let Some(r) = ui.ctx().input(|i| i.viewport().inner_rect) {
                    if let Some(w) = self.ws.layout.windows.iter_mut().find(|w| w.id == id) {
                        w.size = [r.width().max(260.0), r.height().max(180.0)];
                    }
                }
                ui.ctx().input(|i| i.viewport().close_requested())
            });
            if closed {
                close.push(id);
            }
        }
        for id in close {
            self.ws.layout.dock_window(id);
        }
    }

    fn dock_column(&mut self, ui: &mut egui::Ui, region: Region) {
        let r = ui.max_rect();
        let gs = groups(&self.ws.layout, region).clone();
        let sizes = split(r.height(), &gs);
        let mut y = r.top();
        for (gi, h) in sizes.into_iter().enumerate() {
            let gr = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), h));
            y += h;
            self.dock_group(ui, gr, region, gi);
        }
        ui.allocate_rect(r, Sense::hover());
    }

    fn dock_row(&mut self, ui: &mut egui::Ui, region: Region) {
        let r = ui.max_rect();
        let gs = groups(&self.ws.layout, region).clone();
        let sizes = split(r.width(), &gs);
        let mut x = r.left();
        for (gi, w) in sizes.into_iter().enumerate() {
            let gr = Rect::from_min_size(pos2(x, r.top()), vec2(w, r.height()));
            x += w;
            self.dock_group(ui, gr, region, gi);
        }
        ui.allocate_rect(r, Sense::hover());
    }

    fn dock_group(&mut self, ui: &mut egui::Ui, rect: Rect, region: Region, gi: usize) {
        let Some(grp) = groups(&self.ws.layout, region).get(gi).cloned() else {
            return;
        };
        if grp.tabs.is_empty() || rect.height() < 4.0 || rect.width() < 4.0 {
            return;
        }
        let mut gui = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt((
            "dock",
            region.key(),
            gi,
        )));
        gui.set_clip_rect(rect);
        let active = grp.tabs.get(grp.active).copied().unwrap_or(grp.tabs[0]);
        if active == PanelId::EffectsRack {
            self.rack_rect = rect;
        }
        // Used by the drop handler on the next frame: dropping a file on a visible browser
        // imports it there, without placing a clip at the playhead. Keep every visible browser
        // separately; the list is cleared before the workspace is drawn each frame.
        if matches!(active, PanelId::Files | PanelId::Favorites) {
            self.files_drop_rects.push(rect);
        }
        sub_panel(&mut gui, |ui| {
            let tabs: Vec<(PanelId, &str)> = grp.tabs.iter().map(|p| (*p, p.title())).collect();
            let mut a = active;
            theme::tab_header(ui, &tabs, &mut a);
            if a != active {
                if let Some(g) = groups_mut(&mut self.ws.layout, region).get_mut(gi) {
                    g.active = g.tabs.iter().position(|t| *t == a).unwrap_or(0);
                }
            }
            set_skip_headers(ui.ctx(), true);
            self.panel_content(ui, a);
            set_skip_headers(ui.ctx(), false);
        });
    }

    /// The contents of one panel (without its header).
    pub fn panel_content(&mut self, ui: &mut egui::Ui, p: PanelId) {
        let pad = |ui: &mut egui::Ui, f: &mut dyn FnMut(&mut egui::Ui)| {
            Frame::new()
                .inner_margin(Margin::same(6))
                .show(ui, |ui| f(ui));
        };
        match p {
            PanelId::Files => {
                self.files_tab = crate::app::FilesTab::Files;
                self.files_panel(ui);
            }
            PanelId::Favorites => {
                self.files_tab = crate::app::FilesTab::Favorites;
                self.files_panel(ui);
            }
            PanelId::MediaBrowser => pad(ui, &mut |ui| self.media_browser(ui)),
            PanelId::EffectsRack => pad(ui, &mut |ui| self.effects_rack(ui)),
            PanelId::Markers => pad(ui, &mut |ui| self.markers_panel(ui)),
            PanelId::Properties => pad(ui, &mut |ui| self.properties_panel(ui)),
            PanelId::History => self.history_panel(ui),
            PanelId::Levels => self.levels_panel(ui),
            PanelId::SelectionView => self.selection_view(ui),
            PanelId::Essential => self.essential_panel(ui),
            PanelId::Mixer => {
                self.ensure_mixer_session();
                let r = ui.available_rect_before_wrap();
                let mut mui = ui.new_child(egui::UiBuilder::new().max_rect(r));
                mui.set_clip_rect(r);
                self.mixer_view(&mut mui);
                ui.allocate_rect(r, Sense::hover());
            }
            PanelId::FreqAnalysis => pad(ui, &mut |ui| self.freq_analysis_ui(ui)),
            PanelId::AmpStats => pad(ui, &mut |ui| self.amp_stats_ui(ui)),
            PanelId::Video => self.video_panel(ui),
            PanelId::Tracks => pad(ui, &mut |ui| self.tracks_panel(ui)),
            PanelId::Metadata => pad(ui, &mut |ui| self.metadata_panel(ui)),
            PanelId::PhaseAnalysis => pad(ui, &mut |ui| self.phase_analysis_panel(ui)),
            PanelId::PhaseMeter => pad(ui, &mut |ui| self.phase_meter_panel(ui)),
            PanelId::MatchLoudness => pad(ui, &mut |ui| self.match_loudness_panel(ui)),
            PanelId::Diagnostics => pad(ui, &mut |ui| self.diagnostics_panel(ui)),
            PanelId::Transport => self.transport_panel(ui),
            PanelId::Time => self.time_panel(ui),
            PanelId::Zoom => self.zoom_panel(ui),
            PanelId::PlugIns => pad(ui, &mut |ui| self.plugins_panel(ui)),
        }
    }

    fn video_panel(&mut self, ui: &mut egui::Ui) {
        let r = ui.available_rect_before_wrap().shrink(4.0);
        ui.painter().rect_filled(r, 0.0, crate::theme::gray(14));
        use crate::video::VideoState;
        let (playing, engine_pos) = {
            let e = self.engine.lock();
            (e.playing, e.pos)
        };
        let t = if playing {
            engine_pos
        } else {
            self.cur()
                .filter(|f| f.path.as_deref() == self.video.path())
                .map(|f| f.cursor)
                .unwrap_or(0.0)
        };
        // stopped: ask for the exact frame under the playhead at a sharper size
        if !playing && self.prefs.video_preview {
            if let VideoState::Ready(m) = &self.video {
                if m.info.width > 0 {
                    let ctx = ui.ctx().clone();
                    let seeker = self
                        .extra
                        .seeker
                        .get_or_insert_with(|| crate::video::FrameSeeker::new(Some(ctx)));
                    if seeker.wants(&m.path, t, m.info.fps) {
                        if let Ok(exe) = crate::video::ensure_decoder(&self.prefs) {
                            seeker.request(exe, &m.path, t, &m.info, m.converted);
                        }
                    }
                }
            }
        }
        // playing: a stream decodes every frame from the playhead on at full frame rate
        let mut play_key: Option<usize> = None;
        if playing && self.prefs.video_preview {
            if let VideoState::Ready(m) = &self.video {
                if m.info.width > 0 {
                    // frames as wide as the panel shows them (in steps, so resizing doesn't restart)
                    let px =
                        ((r.width() * ui.ctx().pixels_per_point()) / 160.0).ceil() as u32 * 160;
                    let ok = self
                        .extra
                        .play_stream
                        .as_ref()
                        .map(|s| s.serves(&m.path, t))
                        .unwrap_or(false);
                    // A computer that can't keep up gets fewer, smaller frames instead of a stall.
                    // Only judged once pictures flow: a big clip's seek and warm-up (about a second
                    // for 4K AV1) is not lag, and restarting for it would never let a picture through.
                    let lagging = self
                        .extra
                        .play_stream
                        .as_ref()
                        .filter(|s| ok && s.flowing_for() > 2.0 && s.behind(t) > 0.6)
                        .map(|s| (s.fps, s.width));
                    if !ok || lagging.is_some() {
                        if let Ok(exe) = crate::video::ensure_decoder(&self.prefs) {
                            let (fps, width) = match lagging {
                                Some((f, w)) => ((f / 2.0).max(12.0), (w * 3 / 4).max(480)),
                                None => self
                                    .extra
                                    .play_stream
                                    .as_ref()
                                    .filter(|s| s.path_is(&m.path))
                                    .map(|s| (s.fps, s.width))
                                    .unwrap_or((
                                        m.info.fps,
                                        crate::video::res_width(&m.info, px.min(1920)),
                                    )),
                            };
                            // start as far ahead as the last decoder needed to warm up
                            let lead = self.extra.video_lead.clamp(0.0, 2.0);
                            let mut ps = crate::video::PlayStream::launch(
                                exe,
                                &m.path,
                                t + lead,
                                &m.info,
                                m.converted,
                                width,
                                fps,
                                &m.color.codec,
                                Some(ui.ctx().clone()),
                            );
                            ps.lead = lead;
                            self.extra.play_stream = Some(ps);
                        }
                    }
                    if let Some(ps) = &mut self.extra.play_stream {
                        ps.advance(t);
                        if let Some(s) = ps.startup {
                            self.extra.video_lead = s;
                        }
                        if ps.current.is_some() {
                            play_key = Some(usize::MAX / 2 + ps.serial);
                        }
                    }
                }
            }
        } else {
            self.extra.play_stream = None;
        }
        let exact = match (&self.video, &self.extra.seeker, playing) {
            (VideoState::Ready(m), Some(sk), false) => sk
                .exact_time(&m.path, t, m.info.fps)
                .map(|ft| (ft, m.path.clone(), m.info.fps)),
            _ => None,
        };
        let picked = match &self.video {
            VideoState::Empty => None,
            VideoState::Loading(p) => {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                ui.painter().text(
                    r.center() - vec2(0.0, 12.0),
                    egui::Align2::CENTER_CENTER,
                    "Importing video…",
                    FontId::proportional(14.0),
                    theme::accent(),
                );
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    name,
                    FontId::proportional(12.0),
                    theme::text_dim(),
                );
                let phase = (ui.ctx().input(|i| i.time) * 1.5) % 1.0;
                let bar = egui::Rect::from_min_size(
                    egui::pos2(r.center().x - 80.0, r.center().y + 18.0),
                    vec2(160.0, 3.0),
                );
                ui.painter().rect_filled(bar, 0.0, crate::theme::gray(60));
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(bar.min, vec2(bar.width() * phase as f32, 3.0)),
                    0.0,
                    theme::accent(),
                );
                None
            }
            VideoState::Error(e) => {
                // A visible slate, so an imported video is never mistaken for a silent WAV even
                // when the bundled decoder is missing from this installation.
                let slate = r.shrink(16.0);
                ui.painter()
                    .rect_filled(slate, 4.0, Color32::from_rgb(26, 36, 44));
                let step = slate.width() / 8.0;
                for i in 0..8 {
                    let x = slate.min.x + i as f32 * step;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(x, slate.min.y),
                            vec2(5.0, slate.height()),
                        ),
                        0.0,
                        Color32::from_rgb(44, 58, 68),
                    );
                }
                let name = e
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "video".into());
                let short: String = name.chars().take(38).collect();
                ui.painter().text(
                    r.center() - vec2(0.0, 30.0),
                    egui::Align2::CENTER_CENTER,
                    "VIDEO",
                    FontId::proportional(20.0),
                    theme::accent(),
                );
                ui.painter().text(
                    r.center() - vec2(0.0, 4.0),
                    egui::Align2::CENTER_CENTER,
                    short,
                    FontId::proportional(12.0),
                    crate::theme::gray(235),
                );
                let first: String = e
                    .message
                    .lines()
                    .next()
                    .unwrap_or(&e.message)
                    .chars()
                    .take(72)
                    .collect();
                ui.painter().text(
                    r.center() + vec2(0.0, 18.0),
                    egui::Align2::CENTER_CENTER,
                    first,
                    FontId::proportional(10.0),
                    Color32::from_rgb(210, 130, 120),
                );
                ui.painter().text(r.center() + vec2(0.0, 34.0), egui::Align2::CENTER_CENTER, "Keep vendor/ffmpeg beside the app, or set the decoder folder in Preferences > Video.", FontId::proportional(10.0), theme::text_dim());
                None
            }
            VideoState::Ready(m) => {
                if !self.prefs.video_preview {
                    ui.painter().text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        "Video preview is off",
                        FontId::proportional(13.0),
                        theme::text_dim(),
                    );
                    None
                } else {
                    let info = format!(
                        "{}×{} · {:.1} fps · {:.1}s{}",
                        m.info.width,
                        m.info.height,
                        m.info.fps,
                        m.info.duration,
                        if m.complete {
                            String::new()
                        } else {
                            format!(" · loading preview ({} frames)", m.frames.len())
                        }
                    );
                    if m.frames.is_empty() && exact.is_none() && play_key.is_none() {
                        ui.painter().text(
                            r.center() - vec2(0.0, 8.0),
                            egui::Align2::CENTER_CENTER,
                            &m.name,
                            FontId::proportional(13.0),
                            theme::text(),
                        );
                        ui.painter().text(
                            r.center() + vec2(0.0, 10.0),
                            egui::Align2::CENTER_CENTER,
                            "Decoding the preview…",
                            FontId::proportional(11.0),
                            theme::text_dim(),
                        );
                        None
                    } else {
                        // the exact frame (stopped) wins over the nearest cached thumbnail
                        let idx = match (play_key, &exact) {
                            (Some(k), _) => k,
                            (None, Some((ft, _, _))) => usize::MAX - (ft * 1000.0) as usize,
                            (None, None) => m.frame_at(t).map(|(i, _)| i).unwrap_or(0),
                        };
                        Some((idx, 0, 0, m.name.clone(), info))
                    }
                }
            }
        };
        if let Some((idx, _, _, name, info)) = picked {
            if self.video_tex.as_ref().map(|t| t.0) != Some(idx) {
                let frame = match (&exact, &self.extra.seeker) {
                    _ if play_key.is_some() => self
                        .extra
                        .play_stream
                        .as_ref()
                        .and_then(|s| s.current.clone()),
                    (Some((_, path, fps)), Some(sk)) => sk.frame_for(path, t, *fps),
                    _ => match &self.video {
                        VideoState::Ready(m) => m.frame_at(t).map(|(_, f)| f.clone()),
                        _ => None,
                    },
                };
                if let Some(f) = frame {
                    if f.rgba.len() == f.width as usize * f.height as usize * 4 {
                        // opaque pixels: no alpha conversion needed; the texture is updated in place
                        let img = egui::ColorImage::from_rgba_premultiplied(
                            [f.width.max(1) as usize, f.height.max(1) as usize],
                            &f.rgba,
                        );
                        match &mut self.video_tex {
                            Some((k, tex)) if tex.size() == img.size => {
                                tex.set(img, egui::TextureOptions::LINEAR);
                                *k = idx;
                            }
                            _ => {
                                self.video_tex = Some((
                                    idx,
                                    ui.ctx().load_texture(
                                        "video_frame",
                                        img,
                                        egui::TextureOptions::LINEAR,
                                    ),
                                ))
                            }
                        }
                    }
                }
            }
            if let Some((_, tex)) = &self.video_tex {
                // the picture above, one caption line below it: name (shortened to fit) and time on
                // the left, the clip's format on the right, never drawn over each other
                let caption_h = 18.0;
                let pic = egui::Rect::from_min_max(
                    r.min,
                    egui::pos2(r.max.x, (r.max.y - caption_h).max(r.min.y + 1.0)),
                );
                let [tw, th] = tex.size();
                let scale = (pic.width() / tw.max(1) as f32).min(pic.height() / th.max(1) as f32);
                let sz = vec2(tw as f32 * scale, th as f32 * scale);
                let img_rect = egui::Rect::from_center_size(pic.center(), sz);
                ui.painter().image(
                    tex.id(),
                    img_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                let p = ui.painter().with_clip_rect(r);
                let y = r.max.y - caption_h / 2.0;
                let time = format!("{:.2}s", t.max(0.0));
                let tg = p.layout_no_wrap(time, FontId::monospace(11.0), crate::theme::gray(230));
                let ig = p.layout_no_wrap(info, FontId::proportional(10.0), theme::text_dim());
                let avail = r.width() - 12.0;
                // the format line gives way first when the panel is narrow
                let show_info = tg.size().x + ig.size().x + 80.0 <= avail;
                let name_w =
                    avail - tg.size().x - 10.0 - if show_info { ig.size().x + 14.0 } else { 0.0 };
                let mut job = egui::text::LayoutJob::single_section(
                    name,
                    egui::TextFormat::simple(FontId::proportional(11.0), crate::theme::gray(230)),
                );
                job.wrap = egui::text::TextWrapping {
                    max_width: name_w.max(0.0),
                    max_rows: 1,
                    break_anywhere: true,
                    overflow_character: Some('\u{2026}'),
                };
                let ng = p.layout_job(job);
                let mut x = r.min.x + 6.0;
                if name_w > 24.0 {
                    p.galley(
                        egui::pos2(x, y - ng.size().y / 2.0),
                        ng.clone(),
                        Color32::WHITE,
                    );
                    x += ng.size().x + 10.0;
                }
                p.galley(egui::pos2(x, y - tg.size().y / 2.0), tg, Color32::WHITE);
                if show_info {
                    p.galley(
                        egui::pos2(r.max.x - 6.0 - ig.size().x, y - ig.size().y / 2.0),
                        ig,
                        Color32::WHITE,
                    );
                }
            }
        } else if matches!(&self.video, VideoState::Empty) {
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "No video",
                FontId::proportional(13.0),
                theme::text_dim(),
            );
        }
        ui.allocate_rect(r, Sense::hover());
    }

    /// Every track with an eye to show or hide it in the editor; click a name to select it.
    /// The Tracks panel: which tracks the Editor shows. Hiding a track only takes it out of
    /// view (it still plays and mixes); the eye at the left of each row shows or hides it.
    fn tracks_panel(&mut self, ui: &mut egui::Ui) {
        if self.session.is_none() {
            ui.label(
                RichText::new("Open or create a multitrack session.").color(theme::text_dim()),
            );
            return;
        }
        let ctx = ui.ctx().clone();
        ui.label(
            RichText::new("Choose which tracks the Editor shows. Click an eye to hide or show a track; hidden tracks still play.")
                .small()
                .color(theme::text_dim()),
        );
        ui.add_space(2.0);
        let mut run: Option<&'static str> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (label, id, tip) in [
                ("Show All", "tracks.show_all", "Show every track"),
                (
                    "Hide All",
                    "tracks.hide_all",
                    "Hide every track (they still play)",
                ),
                (
                    "Only Selected",
                    "tracks.only_selected",
                    "Show just the selected track",
                ),
                (
                    "Hide Empty",
                    "tracks.hide_empty",
                    "Hide tracks without clips",
                ),
            ] {
                if ui.small_button(label).on_hover_text(tip).clicked() {
                    run = Some(id);
                }
            }
        });
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let shown = s.tracks.iter().filter(|t| !t.hidden).count();
        ui.label(
            RichText::new(format!("{shown} of {} tracks shown", s.tracks.len()))
                .small()
                .color(theme::text_dim()),
        );
        ui.separator();
        let mut touched = false;
        egui::ScrollArea::vertical()
            .id_salt("tracks_panel")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for ti in 0..s.tracks.len() {
                    let sel = s.selected_track == ti && !s.master_selected;
                    let (row, resp) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                    let t = &mut s.tracks[ti];
                    let p = ui.painter();
                    if sel {
                        p.rect_filled(row, 0.0, theme::track_head_sel());
                    } else if resp.hovered() {
                        p.rect_filled(row, 0.0, theme::hover());
                    }
                    // the eye always sits in the same place, whatever its state
                    let eye = egui::Rect::from_min_size(row.min + vec2(4.0, 2.0), vec2(18.0, 18.0));
                    let er = ui.interact(eye, ui.scope_id().with(("track_eye", ti)), Sense::click_and_drag());
                    if er.hovered() {
                        p.rect_filled(eye, 3.0, theme::control());
                    }
                    crate::lucide::draw(
                        p,
                        eye.shrink(1.5),
                        if t.hidden {
                            &crate::lucide::EYE_OFF
                        } else {
                            &crate::lucide::EYE
                        },
                        if t.hidden {
                            theme::text_dim()
                        } else {
                            theme::text()
                        },
                    );
                    let er = er.on_hover_text(if t.hidden {
                        "Hidden: click to show this track in the Editor (press and drag to show or hide several)"
                    } else {
                        "Shown: click to hide this track; it still plays (press and drag to hide or show several)"
                    });
                    if theme::drag_toggle(ui, "track_eye", &er, &mut t.hidden) {
                        touched = true;
                    }
                    let c = crate::theme::saturated(Color32::from_rgb(t.color[0], t.color[1], t.color[2]));
                    let sw = egui::Rect::from_min_size(row.min + vec2(28.0, 4.0), vec2(4.0, 14.0));
                    p.rect_filled(sw, 1.0, if t.hidden { c.gamma_multiply(0.4) } else { c });
                    let name_col = if t.hidden {
                        theme::text_dim()
                    } else if sel {
                        theme::strong()
                    } else {
                        theme::text()
                    };
                    p.with_clip_rect(row).text(
                        pos2(row.left() + 38.0, row.center().y),
                        egui::Align2::LEFT_CENTER,
                        &t.name,
                        egui::FontId::proportional(12.0),
                        name_col,
                    );
                    let n = t.clips.len();
                    let mut info = format!("{n} clip{}", if n == 1 { "" } else { "s" });
                    if t.mute {
                        info.push_str("  M");
                    }
                    if t.solo {
                        info.push_str("  S");
                    }
                    if t.hidden {
                        info = format!("hidden \u{2022} {info}");
                    }
                    p.text(
                        pos2(row.right() - 6.0, row.center().y),
                        egui::Align2::RIGHT_CENTER,
                        info,
                        egui::FontId::proportional(10.5),
                        theme::text_dim(),
                    );
                    if resp.clicked() {
                        s.selected_track = ti;
                        s.master_selected = false;
                    }
                }
            });
        if touched {
            s.touch();
        }
        if let Some(id) = run {
            if let Err(e) = self.exec(id, &ctx) {
                self.status = e;
            }
        }
    }

    /// Metadata of the current file: one tab per schema, each with its own fields. RIFF INFO,
    /// Broadcast Wave and iXML are written into WAV files on save; ID3 tags go into MP3/AAC
    /// exports; the rest stays with the file while it is open.
    fn metadata_panel(&mut self, ui: &mut egui::Ui) {
        let id = ui.make_persistent_id("meta_tab");
        let mut tab: usize = ui.data(|d| d.get_temp(id)).unwrap_or(0);
        ui.horizontal(|ui| {
            for (i, n) in ["BWF", "ID3", "IXML", "RIFF", "CART", "XMP"]
                .iter()
                .enumerate()
            {
                if ui.selectable_label(tab == i, *n).clicked() {
                    tab = i;
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(id, tab));
        let fid = ui.make_persistent_id("meta_filter");
        let mut filter: String = ui.data(|d| d.get_temp(fid)).unwrap_or_default();
        ui.add(
            egui::TextEdit::singleline(&mut filter)
                .hint_text("\u{2315} Filter fields")
                .desired_width(f32::INFINITY),
        );
        ui.data_mut(|d| d.insert_temp(fid, filter.clone()));
        ui.separator();
        let Some(f) = self.cur_mut() else {
            ui.label(
                RichText::new("Open a file in the Waveform Editor to see and edit its metadata.")
                    .color(theme::text_dim()),
            );
            return;
        };
        let info = [
            ("Name", f.name.clone()),
            (
                "Path",
                f.path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(not saved)".into()),
            ),
            ("Sample Rate", format!("{} Hz", f.data.rate)),
            ("Channels", format!("{}", f.data.channels())),
            (
                "Bit Depth",
                if f.float {
                    "32 (float)".to_string()
                } else {
                    format!("{}", f.bits)
                },
            ),
            ("Duration", crate::audio::fmt_time(f.data.duration())),
        ];
        // (section, [(label, key in the metadata map)])
        let sections: Vec<(&str, Vec<(&str, String)>)> = match tab {
            0 => vec![(
                "Broadcast Audio Extension (bext)",
                vec![
                    ("Description", "bext.Description".into()),
                    ("Originator", "bext.Originator".into()),
                    ("Originator Reference", "bext.OriginatorReference".into()),
                    ("Coding History", "bext.CodingHistory".into()),
                ],
            )],
            1 => vec![(
                "ID3",
                vec![
                    ("Title", "Title".into()),
                    ("Artist", "Artist".into()),
                    ("Album", "Album".into()),
                    ("Year", "Date".into()),
                    ("Track", "Track".into()),
                    ("Genre", "Genre".into()),
                    ("Comment", "Comment".into()),
                ],
            )],
            2 => vec![(
                "iXML",
                vec![
                    ("Project", "ixml.Project".into()),
                    ("Scene", "ixml.Scene".into()),
                    ("Take", "ixml.Take".into()),
                    ("Tape", "ixml.Tape".into()),
                    ("Note", "ixml.Note".into()),
                ],
            )],
            3 => vec![(
                "RIFF Info",
                vec![
                    ("Title (INAM)", "Title".into()),
                    ("Artist (IART)", "Artist".into()),
                    ("Product (IPRD)", "Album".into()),
                    ("Genre (IGNR)", "Genre".into()),
                    ("Comment (ICMT)", "Comment".into()),
                    ("Copyright (ICOP)", "Copyright".into()),
                    ("Creation Date (ICRD)", "Date".into()),
                    ("Engineer (IENG)", "Engineer".into()),
                ],
            )],
            4 => vec![(
                "AES CART",
                vec![
                    ("Title", "cart.Title".into()),
                    ("Artist", "cart.Artist".into()),
                    ("Cut ID", "cart.CutID".into()),
                    ("Client ID", "cart.ClientID".into()),
                    ("Category", "cart.Category".into()),
                    ("Classification", "cart.Classification".into()),
                    ("Out Cue", "cart.OutCue".into()),
                    ("Start Date", "cart.StartDate".into()),
                    ("End Date", "cart.EndDate".into()),
                ],
            )],
            _ => vec![
                (
                    "Dublin Core",
                    vec![
                        ("Title", "Title".into()),
                        ("Creator", "Artist".into()),
                        ("Description", "xmp.Description".into()),
                        ("Rights", "Copyright".into()),
                        ("Subject", "xmp.Subject".into()),
                    ],
                ),
                (
                    "Basic",
                    vec![
                        ("Label", "xmp.Label".into()),
                        ("Rating", "xmp.Rating".into()),
                        ("Creator Tool", "xmp.CreatorTool".into()),
                    ],
                ),
                (
                    "Rights Management",
                    vec![
                        ("Copyright Status", "xmp.CopyrightStatus".into()),
                        ("Usage Terms", "xmp.UsageTerms".into()),
                        ("Web Statement", "xmp.WebStatement".into()),
                    ],
                ),
                (
                    "Dynamic Media",
                    vec![
                        ("Album", "Album".into()),
                        ("Artist", "Artist".into()),
                        ("Genre", "Genre".into()),
                        ("Engineer", "Engineer".into()),
                        ("Log Comment", "Comment".into()),
                    ],
                ),
            ],
        };
        let flt = filter.to_lowercase();
        let mut changed = false;
        egui::ScrollArea::vertical()
            .id_salt("meta")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::CollapsingHeader::new(RichText::new("File Properties").strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::Grid::new("meta_fp").num_columns(2).show(ui, |ui| {
                            for (k, v) in &info {
                                if flt.is_empty() || k.to_lowercase().contains(&flt) {
                                    ui.label(RichText::new(*k).color(theme::text_dim()));
                                    ui.label(v);
                                    ui.end_row();
                                }
                            }
                        });
                    });
                if tab == 0 {
                    changed |= ui
                        .checkbox(
                            &mut f.bwf,
                            "Write the Broadcast Wave (bext) chunk when saving as WAV",
                        )
                        .changed();
                }
                for (title, fields) in &sections {
                    egui::CollapsingHeader::new(RichText::new(*title).strong())
                        .default_open(true)
                        .show(ui, |ui| {
                            egui::Grid::new(("meta_grid", *title))
                                .num_columns(2)
                                .spacing([10.0, 4.0])
                                .show(ui, |ui| {
                                    for (label, key) in fields {
                                        if !flt.is_empty() && !label.to_lowercase().contains(&flt) {
                                            continue;
                                        }
                                        ui.label(RichText::new(*label).color(theme::text_dim()));
                                        let mut v = f.meta.get(key).cloned().unwrap_or_default();
                                        if ui
                                            .add(
                                                egui::TextEdit::singleline(&mut v)
                                                    .desired_width(220.0),
                                            )
                                            .changed()
                                        {
                                            if v.is_empty() {
                                                f.meta.remove(key);
                                            } else {
                                                f.meta.insert(key.clone(), v);
                                            }
                                            changed = true;
                                        }
                                        ui.end_row();
                                    }
                                });
                        });
                }
                let where_ = match tab {
                    0 | 2 | 3 => "Written into the file when you save it as WAV.",
                    1 => "Written into MP3 and AAC (M4A) exports.",
                    _ => "Kept with the file while it is open.",
                };
                ui.label(RichText::new(where_).small().color(theme::text_dim()));
            });
        if changed {
            // saving writes the new metadata: the file counts as changed
            f.saved_pos = None;
        }
    }

    /// Goniometer of the output: left/right as a dot cloud (mono up, out of phase down).
    fn phase_analysis_panel(&mut self, ui: &mut egui::Ui) {
        let scope: Vec<[f32; 2]> = self.engine.lock().scope.iter().copied().collect();
        let r = ui.available_rect_before_wrap();
        let side = r.width().min(r.height() - 4.0).max(40.0);
        let sq = Rect::from_center_size(r.center(), vec2(side, side));
        let p = ui.painter_at(r);
        p.rect_filled(sq, 0.0, Color32::BLACK);
        for k in 1..=10 {
            p.circle_stroke(
                sq.center(),
                side * 0.5 * k as f32 / 10.0,
                Stroke::new(1.0, Color32::from_rgb(20, 90, 30)),
            );
        }
        p.line_segment(
            [
                pos2(sq.center().x, sq.top()),
                pos2(sq.center().x, sq.bottom()),
            ],
            Stroke::new(1.0, Color32::from_rgb(160, 30, 30)),
        );
        p.line_segment(
            [
                pos2(sq.left(), sq.center().y),
                pos2(sq.right(), sq.center().y),
            ],
            Stroke::new(1.0, Color32::from_rgb(160, 30, 30)),
        );
        p.text(
            pos2(sq.center().x, sq.top() + 3.0),
            egui::Align2::CENTER_TOP,
            "Mono",
            FontId::proportional(10.0),
            Color32::from_rgb(200, 40, 40),
        );
        p.text(
            pos2(sq.left() + 3.0, sq.center().y),
            egui::Align2::LEFT_BOTTOM,
            "Left",
            FontId::proportional(10.0),
            Color32::from_rgb(200, 40, 40),
        );
        p.text(
            pos2(sq.right() - 3.0, sq.center().y),
            egui::Align2::RIGHT_BOTTOM,
            "Right",
            FontId::proportional(10.0),
            Color32::from_rgb(200, 40, 40),
        );
        p.text(
            pos2(sq.center().x, sq.bottom() - 3.0),
            egui::Align2::CENTER_BOTTOM,
            "Inverse",
            FontId::proportional(10.0),
            Color32::from_rgb(200, 40, 40),
        );
        for [l, rr] in scope.iter().step_by(2) {
            // mid up, side across
            let m = (l + rr) * 0.5;
            let sd = (rr - l) * 0.5;
            let q = pos2(
                sq.center().x + sd.clamp(-1.0, 1.0) * side * 0.5,
                sq.center().y - m.clamp(-1.0, 1.0) * side * 0.5,
            );
            p.rect_filled(
                Rect::from_center_size(q, vec2(1.5, 1.5)),
                0.0,
                Color32::from_rgb(230, 220, 120),
            );
        }
        ui.allocate_rect(r, Sense::hover());
        if self.is_playing() {
            ui.ctx().request_repaint();
        }
    }

    /// Phase correlation of the output: -1 (out of phase) .. +1 (mono).
    fn phase_meter_panel(&mut self, ui: &mut egui::Ui) {
        let scope: Vec<[f32; 2]> = self.engine.lock().scope.iter().copied().collect();
        let (mut lr, mut ll, mut rr) = (0.0f64, 0.0f64, 0.0f64);
        for [l, r] in &scope {
            lr += (*l as f64) * (*r as f64);
            ll += (*l as f64) * (*l as f64);
            rr += (*r as f64) * (*r as f64);
        }
        let corr = if ll * rr > 1e-12 {
            (lr / (ll * rr).sqrt()) as f32
        } else {
            0.0
        };
        let r = ui.available_rect_before_wrap();
        let bar = Rect::from_min_max(
            pos2(r.left() + 6.0, r.top() + 6.0),
            pos2(r.right() - 6.0, (r.bottom() - 18.0).max(r.top() + 12.0)),
        );
        let p = ui.painter_at(r);
        let n = 40;
        for i in 0..n {
            let t = i as f32 / n as f32;
            let c = if t < 0.5 {
                Color32::from_rgb(200, (60.0 + t * 2.0 * 150.0) as u8, 30)
            } else {
                Color32::from_rgb((200.0 - (t - 0.5) * 2.0 * 160.0) as u8, 200, 40)
            };
            let x0 = bar.left() + t * bar.width();
            p.rect_filled(
                Rect::from_min_max(
                    pos2(x0, bar.top()),
                    pos2(x0 + bar.width() / n as f32, bar.bottom()),
                ),
                0.0,
                c,
            );
        }
        let x = bar.left() + (corr.clamp(-1.0, 1.0) + 1.0) * 0.5 * bar.width();
        p.line_segment(
            [pos2(x, bar.top() - 2.0), pos2(x, bar.bottom() + 2.0)],
            Stroke::new(2.0, Color32::WHITE),
        );
        for k in -5..=5 {
            let v = k as f32 / 5.0;
            let xx = bar.left() + (v + 1.0) * 0.5 * bar.width();
            p.text(
                pos2(xx, bar.bottom() + 2.0),
                egui::Align2::CENTER_TOP,
                format!("{v:.1}"),
                FontId::proportional(9.0),
                theme::text_dim(),
            );
        }
        ui.allocate_rect(r, Sense::hover());
        if self.is_playing() {
            ui.ctx().request_repaint();
        }
    }

    /// Match Loudness: measure the open files and bring them to one target.
    fn match_loudness_panel(&mut self, ui: &mut egui::Ui) {
        let tid = ui.make_persistent_id("ml_target");
        let mut target: f32 = ui.data(|d| d.get_temp(tid)).unwrap_or(-23.0);
        let mut run = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Match Loudness Settings").color(theme::text_dim()));
            ui.add(
                egui::DragValue::new(&mut target)
                    .range(-40.0..=-6.0)
                    .speed(0.1)
                    .suffix(" LUFS"),
            )
            .on_hover_text("Target loudness");
            ui.add_space(10.0);
            run = ui
                .add_enabled(!self.files.is_empty(), egui::Button::new("Run"))
                .on_hover_text("Bring every file to the target loudness")
                .clicked();
        });
        ui.data_mut(|d| d.insert_temp(tid, target));
        ui.separator();
        if self.files.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Open audio files to measure them here").color(theme::text_dim()),
                )
            });
            return;
        }
        let rows: Vec<(u64, String, f32, f32, f32)> = self
            .files
            .iter()
            .map(|f| {
                let (lufs, rms, peak) = measure(&f.data);
                (f.id, f.name.clone(), lufs, rms, peak)
            })
            .collect();
        egui::ScrollArea::vertical()
            .id_salt("ml_rows")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("ml_grid")
                    .striped(true)
                    .num_columns(5)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for h in ["Name", "ITU Loudness", "Total RMS", "Peak", "Status"] {
                            ui.label(RichText::new(h).color(theme::text_dim()));
                        }
                        ui.end_row();
                        for (_, n, l, r, pk) in &rows {
                            ui.label(n);
                            ui.label(
                                RichText::new(format!("{l:.1} LUFS")).color(theme::time_blue()),
                            );
                            ui.label(format!("{r:.1} dB"));
                            ui.label(format!("{pk:.1} dB"));
                            let ok = (l - target).abs() < 0.5;
                            ui.label(
                                RichText::new(if ok { "Matched" } else { "" })
                                    .color(Color32::from_rgb(110, 210, 140)),
                            );
                            ui.end_row();
                        }
                    });
            });
        if run {
            for (id, ..) in rows {
                if let Some(f) = self.file_mut(id) {
                    let mut d = (*f.data).clone();
                    let n = d.frames();
                    crate::dsp::match_loudness(&mut d, 0, n, target);
                    f.push("Match Loudness", d);
                }
            }
            self.status = format!("Matched {} files to {target:.1} LUFS", self.files.len());
        }
    }

    /// Diagnostics: scan the file for clicks, clipping or silence and repair what is found.
    fn diagnostics_panel(&mut self, ui: &mut egui::Ui) {
        let kid = ui.make_persistent_id("diag_kind");
        let rid = ui.make_persistent_id("diag_found");
        let mut kind: usize = ui.data(|d| d.get_temp(kid)).unwrap_or(0);
        let mut found: Vec<(usize, usize, usize)> =
            ui.data(|d| d.get_temp(rid)).unwrap_or_default();
        let names = ["DeClicker", "DeClipper", "Delete Silence", "Mark Audio"];
        let mut scan = false;
        let mut repair_all = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Effect:").color(theme::text_dim()));
            egui::ComboBox::from_id_salt("diag_combo")
                .selected_text(names[kind.min(3)])
                .show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        if ui.selectable_label(kind == i, *n).clicked() {
                            kind = i;
                            found.clear();
                        }
                    }
                });
        });
        let has = self.cur().is_some();
        ui.horizontal(|ui| {
            scan = ui.add_enabled(has, egui::Button::new("Scan")).clicked();
            repair_all = ui
                .add_enabled(
                    !found.is_empty(),
                    egui::Button::new(if kind == 3 {
                        "Mark All"
                    } else if kind == 2 {
                        "Delete All"
                    } else {
                        "Repair All"
                    }),
                )
                .clicked();
            if ui
                .add_enabled(!found.is_empty(), egui::Button::new("Clear"))
                .clicked()
            {
                found.clear();
            }
        });
        if scan {
            found = self
                .cur()
                .map(|f| diagnose(&f.data, kind))
                .unwrap_or_default();
        }
        ui.separator();
        let rate = self.cur().map(|f| f.data.rate).unwrap_or(48000).max(1) as f64;
        egui::ScrollArea::vertical()
            .id_salt("diag_rows")
            .max_height((ui.available_height() - 24.0).max(40.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("diag_grid")
                    .striped(true)
                    .num_columns(3)
                    .show(ui, |ui| {
                        for h in ["Start", "Duration", "Channel"] {
                            ui.label(RichText::new(h).color(theme::text_dim()));
                        }
                        ui.end_row();
                        for (a, b, c) in found.iter().take(500) {
                            if ui.link(crate::audio::fmt_time(*a as f64 / rate)).clicked() {
                                if let Some(f) = self.cur_mut() {
                                    f.selection = Some((*a as f64 / rate, *b as f64 / rate));
                                    f.cursor = *a as f64 / rate;
                                }
                            }
                            ui.label(format!("{:.3} s", (b - a) as f64 / rate));
                            ui.label(if *c == usize::MAX {
                                "All".to_string()
                            } else {
                                format!("{}", c + 1)
                            });
                            ui.end_row();
                        }
                    });
            });
        ui.label(
            RichText::new(format!("{} problems detected.", found.len())).color(theme::text_dim()),
        );
        if repair_all && !found.is_empty() {
            match kind {
                3 => {
                    if let Some(f) = self.cur_mut() {
                        for (i, (a, b, _)) in found.iter().enumerate() {
                            f.markers.push(crate::session::Marker {
                                name: format!("Audio {:02}", i + 1),
                                time: *a as f64 / rate,
                                duration: (*b - *a) as f64 / rate,
                            });
                        }
                    }
                }
                2 => {
                    if let Some(f) = self.cur_mut() {
                        let mut d = (*f.data).clone();
                        for (a, b, _) in found.iter().rev() {
                            d = d.without(*a, *b);
                        }
                        f.push("Delete Silence", d);
                    }
                }
                _ => {
                    if let Some(f) = self.cur_mut() {
                        let mut d = (*f.data).clone();
                        for (a, b, _) in &found {
                            let pad = 8;
                            crate::procs::auto_heal(&mut d, a.saturating_sub(pad), b + pad);
                        }
                        f.push(if kind == 0 { "DeClicker" } else { "DeClipper" }, d);
                    }
                }
            }
            found.clear();
        }
        ui.data_mut(|d| {
            d.insert_temp(kid, kind);
            d.insert_temp(rid, found);
        });
    }

    fn transport_panel(&mut self, ui: &mut egui::Ui) {
        let (playing, rec, looping) = {
            let s = self.engine.lock();
            (s.playing, s.recording, s.looping)
        };
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let sz = vec2(30.0, 26.0);
            if theme::icon_button_sized(ui, Icon::Stop, false, "Stop", sz).clicked() {
                self.stop();
            }
            if theme::icon_button_sized(ui, Icon::Play, playing, "Play (Space)", sz).clicked() {
                self.toggle_play();
            }
            if theme::icon_button_sized(ui, Icon::Pause, false, "Pause", sz).clicked() {
                self.pause();
            }
            if theme::icon_button_sized(ui, Icon::ToStart, false, "Move Playhead to Start", sz)
                .clicked()
            {
                self.set_cursor(0.0);
            }
            if theme::icon_button_sized(ui, Icon::ToEnd, false, "Move Playhead to End", sz)
                .clicked()
            {
                let ctx = ui.ctx().clone();
                self.run("end", &ctx);
            }
            let lit = rec || self.record_lit(ui.ctx());
            if theme::icon_button_sized(
                ui,
                Icon::Record,
                lit,
                "Record: get ready, then Play starts recording",
                sz,
            )
            .clicked()
            {
                self.toggle_record();
            }
            if theme::icon_button_sized(ui, Icon::Loop, looping, "Loop Playback", sz).clicked() {
                let mut s = self.engine.lock();
                s.looping = !s.looping;
            }
        });
    }

    fn time_panel(&mut self, ui: &mut egui::Ui) {
        let r = ui.available_rect_before_wrap();
        let txt = self.fmt(self.cursor());
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            txt,
            FontId::proportional((r.height() * 0.55).clamp(14.0, 64.0)),
            crate::theme::gray(150),
        );
        ui.allocate_rect(r, Sense::hover());
        if self.is_playing() {
            ui.ctx().request_repaint();
        }
    }

    fn zoom_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(6.0);
            for (ic, tip, cmd) in [
                (Icon::ZoomIn, "Zoom In", "zoomin"),
                (Icon::ZoomOut, "Zoom Out", "zoomout"),
                (Icon::ZoomFull, "Zoom Out Full", "zoomfull"),
                (Icon::ZoomSelIn, "Zoom to Selection", "zoomsel"),
                (Icon::ZoomInAmp, "Zoom In Amplitude", "ampin"),
                (Icon::ZoomOutAmp, "Zoom Out Amplitude", "ampout"),
            ] {
                if theme::icon_button_sized(ui, ic, false, tip, vec2(30.0, 26.0)).clicked() {
                    match cmd {
                        "zoomsel" => self.zoom_selection(),
                        "ampin" => {
                            self.drag_state.amp_zoom = (self.drag_state.amp_zoom * 1.5).min(64.0)
                        }
                        "ampout" => {
                            self.drag_state.amp_zoom = (self.drag_state.amp_zoom / 1.5).max(0.25)
                        }
                        c => {
                            let ctx = ui.ctx().clone();
                            self.run(c, &ctx);
                        }
                    }
                }
            }
        });
    }

    /// The workspace tabs at the right of the toolbar, with the workspace menu and the overflow.
    pub fn workspace_bar(&mut self, ui: &mut egui::Ui) {
        let names = self.ws.names();
        let current = self.ws.current.clone();
        let mut shown: Vec<String> = names.iter().take(3).cloned().collect();
        if !shown.contains(&current) {
            shown.push(current.clone());
        }
        let mut pick: Option<String> = None;
        let mut action: Option<&'static str> = None;
        for n in &shown {
            let on = *n == current;
            let resp = ui.add(
                egui::Button::new(RichText::new(n).color(if on {
                    theme::accent()
                } else {
                    theme::text_dim()
                }))
                .frame(false),
            );
            if resp.clicked() && !on {
                pick = Some(n.clone());
            }
            if on {
                let _ = egui::containers::menu::MenuButton::from_button(
                    egui::Button::new(RichText::new("\u{2261}").color(theme::accent()))
                        .frame(false),
                )
                .ui(ui, |ui| {
                    if ui.button("Reset to Saved Layout").clicked() {
                        action = Some("reset");
                        ui.close();
                    }
                    if ui.button("Save Changes to this Workspace").clicked() {
                        action = Some("save");
                        ui.close();
                    }
                    if ui.button("Save as New Workspace...").clicked() {
                        action = Some("new");
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Edit Workspaces...").clicked() {
                        action = Some("edit");
                        ui.close();
                    }
                });
            }
            ui.add_space(14.0);
        }
        let _ = egui::containers::menu::MenuButton::from_button(
            egui::Button::new(RichText::new("\u{00bb}").color(theme::text())).frame(false),
        )
        .ui(ui, |ui| {
            for n in names.iter().skip(3) {
                if ui.selectable_label(*n == current, n).clicked() {
                    pick = Some(n.clone());
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Edit Workspaces...").clicked() {
                action = Some("edit");
                ui.close();
            }
        });
        if let Some(n) = pick {
            self.set_workspace(&n);
        }
        match action {
            Some("reset") => self.ws.reset(),
            Some("save") => {
                self.ws.save_current();
                self.status = format!("Saved the \"{}\" workspace", self.ws.current);
            }
            Some("new") => {
                self.dialog = Some(crate::app::Dialog::NewWorkspace {
                    name: format!("{} copy", self.ws.current),
                })
            }
            Some("edit") => self.dialog = Some(crate::app::Dialog::EditWorkspaces),
            _ => {}
        }
    }

    /// Right end of the toolbar (laid out right to left): Search Help, then the workspace tabs.
    pub fn workspace_bar_rtl(&mut self, ui: &mut egui::Ui) {
        let mut q = std::mem::take(&mut self.extra.help_query);
        // the magnifier is drawn by hand, centred on the box's middle (a text glyph sits high)
        let r = ui.add(
            egui::TextEdit::singleline(&mut q)
                .hint_text("Search Help")
                .desired_width(200.0)
                .vertical_align(egui::Align::Center)
                .margin(egui::Margin {
                    left: 22,
                    right: 4,
                    top: 2,
                    bottom: 2,
                }),
        );
        {
            let c = pos2(r.rect.left() + 11.0, r.rect.center().y - 1.0);
            let col = if r.has_focus() {
                theme::text()
            } else {
                theme::text_dim()
            };
            let p = ui.painter();
            p.circle_stroke(c, 4.0, Stroke::new(1.3, col));
            p.line_segment(
                [c + vec2(2.9, 2.9), c + vec2(6.0, 6.0)],
                Stroke::new(1.6, col),
            );
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) && r.has_focus() {
            q.clear();
        }
        self.extra.help_query = q;
        // results drop down under the box as you type; Enter opens the first one's help
        if enter {
            let first = App::help_matches(&self.extra.help_query)
                .first()
                .map(|c| c.id);
            self.help_topic_open(first);
        } else {
            let ctx = ui.ctx().clone();
            self.help_results_popup(&ctx, r.rect, r.has_focus());
        }
        ui.add_space(6.0);
        // the tabs sit right against the search box, like the original: their width is
        // measured from what was drawn last frame (estimated on the very first one)
        let wid = ui.make_persistent_id("ws_tabs_width");
        let measured: Option<f32> = ui.data(|d| d.get_temp(wid));
        let w = match measured {
            Some(m) => m,
            None => {
                let mut shown: Vec<String> = self.ws.names().into_iter().take(3).collect();
                if !shown.contains(&self.ws.current) {
                    shown.push(self.ws.current.clone());
                }
                let font = egui::TextStyle::Button.resolve(ui.style());
                shown
                    .iter()
                    .map(|n| {
                        ui.painter()
                            .layout_no_wrap(n.clone(), font.clone(), Color32::WHITE)
                            .size()
                            .x
                            + 14.0
                            + 2.0 * ui.spacing().button_padding.x
                    })
                    .sum::<f32>()
                    + 28.0
            }
        }
        .min(ui.available_width());
        let used = ui.allocate_ui_with_layout(
            vec2(w, ui.available_height()),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                self.workspace_bar(ui);
                ui.min_rect().width()
            },
        );
        let real = used.inner.ceil();
        if measured.map(|m| (m - real).abs() > 0.5).unwrap_or(true) {
            ui.data_mut(|d| d.insert_temp(wid, real));
            ui.ctx().request_repaint();
        }
    }

    /// Window menu: workspaces and every panel (ticked when it is in the workspace).
    pub fn window_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Workspace", |ui| {
            for n in self.ws.names() {
                if theme::radio_item(ui, true, self.ws.current == n, &n, "").clicked() {
                    self.set_workspace(&n);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Reset to Saved Layout").clicked() {
                self.ws.reset();
                ui.close();
            }
            if ui.button("Save Changes to this Workspace").clicked() {
                self.ws.save_current();
                ui.close();
            }
            if ui.button("Save as New Workspace...").clicked() {
                self.dialog = Some(crate::app::Dialog::NewWorkspace {
                    name: format!("{} copy", self.ws.current),
                });
                ui.close();
            }
            if ui.button("Edit Workspaces...").clicked() {
                self.dialog = Some(crate::app::Dialog::EditWorkspaces);
                ui.close();
            }
        });
        if ui
            .button("New Window")
            .on_hover_text("A new window to put panels in")
            .clicked()
        {
            let _ = self.exec("window.new_window", ui.ctx());
            ui.close();
        }
        if ui
            .add_enabled(
                !self.ws.layout.windows.is_empty(),
                egui::Button::new("Dock All Windows"),
            )
            .clicked()
        {
            let _ = self.exec("window.dock_all", ui.ctx());
            ui.close();
        }
        ui.separator();
        let mut sorted = PanelId::ALL.to_vec();
        sorted.sort_by_key(|p| p.title());
        for p in sorted {
            let on = self.ws.layout.contains(p);
            if theme::check_item(ui, true, on, p.title(), "").clicked() {
                self.ws.layout.toggle(p);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Editor: Mixer").clicked() {
            self.ensure_mixer_session();
            self.switch_mode(Mode::Multitrack);
            self.center_tab = crate::app::CenterTab::Mixer;
            ui.close();
        }
        theme::check_toggle(
            ui,
            &mut self.panels.stats,
            "Amplitude Statistics (floating)",
            "",
        );
        theme::check_toggle(
            ui,
            &mut self.panels.freq,
            "Frequency Analysis (floating)",
            "",
        );
    }
}

impl App {
    /// What still has unsaved changes, as close-prompt items.
    fn unsaved_items(&self) -> Vec<CloseItem> {
        let mut v: Vec<CloseItem> = self
            .files
            .iter()
            .filter(|f| f.dirty() && f.data.frames() > 0)
            .map(|f| CloseItem::File(f.id))
            .collect();
        if let Some(s) = &self.session {
            if s.unsaved() && s.tracks.iter().any(|t| !t.clips.is_empty()) {
                v.push(CloseItem::Session);
            }
        }
        v
    }

    fn item_unsaved(&self, it: CloseItem) -> bool {
        match it {
            CloseItem::File(id) => self
                .file(id)
                .map(|f| f.dirty() && f.data.frames() > 0)
                .unwrap_or(false),
            CloseItem::Session => self
                .session
                .as_ref()
                .map(|s| s.unsaved() && s.tracks.iter().any(|t| !t.clips.is_empty()))
                .unwrap_or(false),
        }
    }

    /// File > Close: a file with unsaved changes is asked about first; otherwise it closes.
    pub fn request_close_file(&mut self, id: u64) {
        if self.item_unsaved(CloseItem::File(id)) && self.screenshot.is_none() {
            self.close_queue = Some((vec![CloseItem::File(id)], false));
        } else {
            self.close_file(id);
        }
    }

    /// Closing the window (or a file) with unsaved changes asks about each one in turn, like the
    /// original: Yes saves it (Alt+click saves a copy under a new name), No drops the changes,
    /// the To All answers do the same for every one left, and Cancel stops. Enter is Yes, Esc is
    /// Cancel.
    pub fn close_guard(&mut self, ctx: &egui::Context) {
        if self.closing || (self.screenshot.is_some() && self.close_queue.is_none()) {
            return;
        }
        if self.screenshot.is_none() && ctx.input(|i| i.viewport().close_requested()) {
            let items = self.unsaved_items();
            if !items.is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if self.close_queue.is_none() {
                    self.close_queue = Some((items, true));
                }
            }
        }
        let Some((mut queue, quit)) = self.close_queue.take() else {
            return;
        };
        // items saved or closed elsewhere meanwhile need no question
        queue.retain(|it| self.item_unsaved(*it));
        let Some(&cur) = queue.first() else {
            self.finish_close_queue(ctx, quit);
            return;
        };
        let name = match cur {
            CloseItem::File(id) => self.file(id).map(|f| f.name.clone()).unwrap_or_default(),
            CloseItem::Session => self
                .session
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_default(),
        };
        let more = queue.len() > 1;
        // 0 yes, 1 yes to all, 2 save as new, 3 no, 4 no to all, 5 cancel
        let mut choice: Option<u8> = None;
        let (enter, esc) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if enter {
            choice = Some(0);
        }
        if esc {
            choice = Some(5);
        }
        // dim the window behind; the dim layer never takes the pointer
        egui::Area::new(egui::Id::new("close_dim"))
            .order(egui::Order::Middle)
            .interactable(false)
            .fixed_pos(pos2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.painter()
                    .rect_filled(ctx.content_rect(), 0.0, Color32::from_black_alpha(110));
            });
        // the same window style, buttons and hover as every other dialog
        egui::Window::new("Save Changes")
            .id(egui::Id::new("close_prompt"))
            .resizable(false)
            .collapsible(false)
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .constrain(true)
            .show(ctx, |ui| {
                ui.set_width(520.0);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let (wr, _) = ui.allocate_exact_size(vec2(40.0, 32.0), Sense::hover());
                    let tri = vec![
                        pos2(wr.center().x, wr.top()),
                        pos2(wr.left() + 3.0, wr.bottom()),
                        pos2(wr.right() - 3.0, wr.bottom()),
                    ];
                    ui.painter().add(egui::Shape::convex_polygon(
                        tri,
                        Color32::from_rgb(246, 199, 30),
                        Stroke::new(1.5, Color32::from_rgb(211, 159, 10)),
                    ));
                    ui.painter().text(
                        wr.center() + vec2(0.0, 6.0),
                        egui::Align2::CENTER_CENTER,
                        "!",
                        FontId::proportional(19.0),
                        Color32::from_rgb(49, 42, 25),
                    );
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "Save changes to \u{2018}{name}\u{2019} before closing?"
                                ))
                                .size(14.0)
                                .color(theme::strong()),
                            )
                            .wrap(),
                        );
                        let rest = queue.len().saturating_sub(1);
                        if rest > 0 {
                            ui.label(
                                RichText::new(format!(
                                    "{rest} more unsaved item(s) after this one."
                                ))
                                .small()
                                .color(theme::text_dim()),
                            );
                        }
                    });
                });
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    // what throws the changes away sits on the left, apart from Cancel and Save
                    if crate::theme::dialog_button(ui, "Don't Save", false)
                        .on_hover_text("Close without saving")
                        .clicked()
                    {
                        choice = Some(3);
                    }
                    if more && crate::theme::dialog_button(ui, "Don't Save Any", false).clicked() {
                        choice = Some(4);
                    }
                    ui.add_space(24.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::theme::dialog_button(ui, "Save", true)
                            .on_hover_text("Save changes; Alt+click to save as a new file")
                            .clicked()
                        {
                            choice = Some(if ui.input(|i| i.modifiers.alt) { 2 } else { 0 });
                        }
                        if more && crate::theme::dialog_button(ui, "Save All", false).clicked() {
                            choice = Some(1);
                        }
                        if crate::theme::dialog_button(ui, "Cancel", false).clicked() {
                            choice = Some(5);
                        }
                    });
                });
            });
        // keep hover feedback moving while the question is up
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        let save = |app: &mut App, it: CloseItem, as_new: bool| match it {
            CloseItem::File(id) => {
                app.save_file(id, as_new);
            }
            CloseItem::Session => app.save_session(as_new),
        };
        match choice {
            Some(0) | Some(2) => {
                save(self, cur, choice == Some(2));
                // still unsaved (the save was cancelled or failed): the question stays
                if !self.item_unsaved(cur) {
                    queue.remove(0);
                    if !quit {
                        self.discard_item(cur);
                    }
                }
            }
            Some(1) => {
                for it in queue.clone() {
                    save(self, it, false);
                }
                queue.retain(|it| self.item_unsaved(*it));
                if !queue.is_empty() {
                    self.status = "Some changes were not saved".into();
                }
            }
            Some(3) => {
                queue.remove(0);
                if !quit {
                    self.discard_item(cur);
                }
            }
            Some(4) => {
                let rest: Vec<CloseItem> = queue.drain(..).collect();
                if !quit {
                    for it in rest {
                        self.discard_item(it);
                    }
                }
            }
            Some(5) => {
                // Cancel: nothing more closes
                self.close_after = None;
                return;
            }
            _ => {}
        }
        if queue.is_empty() {
            self.finish_close_queue(ctx, quit);
        } else {
            self.close_queue = Some((queue, quit));
        }
    }

    /// Closing a single file: after its question is answered, it closes.
    fn discard_item(&mut self, it: CloseItem) {
        // Close All and the like close everything together once every question is answered
        if self.close_after.is_some() {
            return;
        }
        if let CloseItem::File(id) = it {
            self.close_file(id);
        }
    }

    fn finish_close_queue(&mut self, ctx: &egui::Context, quit: bool) {
        if let (false, Some(after)) = (quit, self.close_after) {
            self.finish_close_after(after);
        }
        if quit {
            self.closing = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Something the close prompt asks about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CloseItem {
    File(u64),
    Session,
}

/// (approximate loudness LUFS, RMS dB, peak dB) of the whole file.
pub fn measure(d: &crate::audio::AudioData) -> (f32, f32, f32) {
    let n = d.frames();
    if n == 0 || d.channels() == 0 {
        return (-96.0, -96.0, -96.0);
    }
    let mut ms = 0.0f64;
    let mut pk = 0.0f32;
    for c in &d.ch {
        for v in c {
            ms += (*v as f64) * (*v as f64);
            pk = pk.max(v.abs());
        }
    }
    ms /= (n * d.channels()) as f64;
    let rms = if ms > 1e-12 {
        10.0 * ms.log10() as f32
    } else {
        -96.0
    };
    (rms - 0.691, rms, crate::audio::db(pk))
}

/// Problems a diagnostics scan finds: (start frame, end frame, channel or MAX for all).
pub fn diagnose(d: &crate::audio::AudioData, kind: usize) -> Vec<(usize, usize, usize)> {
    let mut out = vec![];
    match kind {
        // clicks: a sample far from the line its neighbours make, compared with the local level
        0 => {
            for (ci, c) in d.ch.iter().enumerate() {
                let n = c.len();
                let win = 256usize;
                let mut i = 2;
                while i + 2 < n {
                    let pred = 2.0 * c[i - 1] - c[i - 2];
                    let err = (c[i] - pred).abs();
                    let a = i.saturating_sub(win);
                    let b = (i + win).min(n);
                    if err > 0.2 {
                        let lvl =
                            (c[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32).sqrt();
                        if err > 8.0 * lvl.max(0.005) {
                            out.push((i.saturating_sub(4), (i + 4).min(n), ci));
                            i += 64;
                            continue;
                        }
                    }
                    i += 1;
                }
            }
        }
        // clipping: runs of 3+ samples at full scale
        1 => {
            for (ci, c) in d.ch.iter().enumerate() {
                let mut i = 0;
                while i < c.len() {
                    if c[i].abs() >= 0.999 {
                        let s = i;
                        while i < c.len() && c[i].abs() >= 0.999 {
                            i += 1;
                        }
                        if i - s >= 3 {
                            out.push((s, i, ci));
                        }
                    }
                    i += 1;
                }
            }
        }
        2 => {
            out = crate::procs::quiet_spans(d, 0, d.frames(), -50.0, 0.5)
                .into_iter()
                .map(|(a, b)| (a, b, usize::MAX))
                .collect();
        }
        _ => {
            // audio between the quiet stretches
            let q = crate::procs::quiet_spans(d, 0, d.frames(), -50.0, 0.5);
            let mut prev = 0;
            for (a, b) in q {
                if a > prev {
                    out.push((prev, a, usize::MAX));
                }
                prev = b;
            }
            if prev < d.frames() {
                out.push((prev, d.frames(), usize::MAX));
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::{self, Wave};

    #[test]
    fn diagnostics_find_clicks_and_clipping() {
        let mut d = dsp::gen_tone(48000, 1, 0.5, 200.0, Wave::Sine, -20.0);
        d.ch[0][10000] = 0.9;
        let clicks = diagnose(&d, 0);
        assert!(
            clicks.iter().any(|(a, b, _)| *a <= 10000 && *b > 10000),
            "{clicks:?}"
        );
        for i in 20000..20010 {
            d.ch[0][i] = 1.0;
        }
        assert!(!diagnose(&d, 1).is_empty());
        let (l, r, p) = measure(&d);
        assert!(l < r && p > -0.1);
    }
}
