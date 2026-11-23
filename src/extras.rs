//! Recording workflow, live take drawing, input metering/monitoring, Search Help, the
//! background Batch Processor and speech jobs, decoder downloads and their commands.

use crate::app::{App, Dialog, Mode};
use crate::audio::AudioData;
use crate::commands::Cmd;
use crate::theme;
use egui::{Color32, FontId, Rect, RichText, Stroke, pos2, vec2};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

pub const EXTRA_COMMANDS: &[Cmd] = &[
    Cmd {
        id: "window.new_window",
        title: "New Window",
        shortcut: "",
        doc: "Open a new, empty window for panels; add panels to it with its Add Panel menu. Closing it docks its panels back.",
    },
    Cmd {
        id: "window.dock_all",
        title: "Dock All Windows",
        shortcut: "",
        doc: "Put the panels of every separate window back into the main window.",
    },
    Cmd {
        id: "help.search",
        title: "Search Help",
        shortcut: "",
        doc: "Search every command by name and description; pick one to run it or read its help.",
    },
    Cmd {
        id: "transport.record_ready",
        title: "Record (Ready)",
        shortcut: "Shift+Space",
        doc: "Get recording ready on the armed tracks (or the open file); recording starts when you press Play. Pressing it while playing punches in at once.",
    },
    Cmd {
        id: "video.decoders",
        title: "Video Decoders...",
        shortcut: "",
        doc: "Choose which installed video decoder is used, or download a free one (Edit > Preferences > Video).",
    },
    Cmd {
        id: "video.convert_color",
        title: "Convert Video Preview to Rec.709",
        shortcut: "",
        doc: "Re-decode the video's preview frames converted from wide-gamut / HDR colour to standard Rec.709.",
    },
];

/// Block size of the live-take overview.
const LIVE_BLOCK: usize = 32;

/// The take being recorded, as min/max blocks so it can be drawn while it grows.
#[derive(Default)]
pub struct LiveTake {
    pub frames: usize,
    pub rate: u32,
    pub peaks: Vec<Vec<(f32, f32)>>,
}

impl LiveTake {
    pub fn duration(&self) -> f64 {
        if self.rate == 0 {
            0.0
        } else {
            self.frames as f64 / self.rate as f64
        }
    }
}

/// A batch run on a worker: one status line per file.
pub struct BatchRun {
    pub status: Arc<Mutex<Vec<String>>>,
    pub done: Receiver<String>,
}

pub struct Extras {
    pub seeker: Option<crate::video::FrameSeeker>,
    /// Full-frame-rate frames while playing.
    pub play_stream: Option<crate::video::PlayStream>,
    /// Seconds the last playback decoder took to its first picture (it starts this far ahead).
    pub video_lead: f64,
    /// Files panel column widths in points (Name, Status, Duration, Sample Rate, Ch, Bits).
    pub files_cols: [f32; 6],
    /// Record was pressed: recording starts with Play.
    pub record_ready: bool,
    pub live: LiveTake,
    pub speech_job: Option<(u64, Receiver<Result<AudioData, String>>)>,
    pub speech_result: Option<(u64, Arc<AudioData>)>,
    /// What to do when the running speech job finishes: 1 preview, 2 insert.
    pub speech_then: u8,
    pub speech_status: String,
    pub batch: Option<BatchRun>,
    pub batch_lines: Vec<String>,
    pub help_query: String,
    /// The command whose help is showing (Help window).
    pub help_topic: Option<&'static str>,
    pub download: Option<(String, Receiver<Result<PathBuf, String>>)>,
    pub download_status: String,
    /// A colour-space warning already shown for this video.
    pub color_asked: Option<PathBuf>,
    /// Decoders found for Preferences > Video (refreshed on demand).
    pub decoder_list: Option<Vec<crate::ffmpeg::Installed>>,
    /// The Generate Speech preview is playing (on the separate preview player).
    pub speech_previewing: bool,
}

impl Default for Extras {
    fn default() -> Self {
        Extras {
            seeker: None,
            play_stream: None,
            video_lead: 0.0,
            files_cols: [190.0, 60.0, 78.0, 84.0, 30.0, 40.0],
            record_ready: false,
            live: LiveTake::default(),
            speech_job: None,
            speech_result: None,
            speech_then: 0,
            speech_status: String::new(),
            batch: None,
            batch_lines: vec![],
            help_query: String::new(),
            help_topic: None,
            download: None,
            download_status: String::new(),
            color_asked: None,
            decoder_list: None,
            speech_previewing: false,
        }
    }
}

/// Which input channels a track's input name means (left, right).
pub fn input_channels(name: &str) -> [usize; 2] {
    let n = name.to_ascii_lowercase();
    if n.starts_with("mono") {
        if n.contains("right") || n.ends_with(" 2") {
            [1, 1]
        } else {
            [0, 0]
        }
    } else {
        [0, 1]
    }
}

pub fn hash_of(parts: &[&str]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

impl App {
    /// Run one of `EXTRA_COMMANDS`. `Ok(false)` = not one of these.
    pub fn exec_extra(&mut self, id: &str, ctx: &egui::Context) -> Result<bool, String> {
        let _ = ctx;
        match id {
            "window.new_window" => {
                self.ws.layout.new_window(vec![]);
                self.status = "New window: add panels to it with its Add Panel menu".into();
            }
            "window.dock_all" => {
                let ids: Vec<u64> = self.ws.layout.windows.iter().map(|w| w.id).collect();
                for id in ids {
                    self.ws.layout.dock_window(id);
                }
            }
            "help.search" => {
                self.help_topic_open(None);
            }
            "transport.record_ready" => self.toggle_record(),
            "video.decoders" => self.open_prefs("Video"),
            "video.convert_color" => self.convert_video_color()?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    // ------------------------------------------------------------ recording

    /// The Record button: arm the transport (recording starts with Play), or punch in while
    /// playing, or stop a recording. Nothing records without a target.
    pub fn toggle_record(&mut self) {
        if self.is_recording() {
            self.stop();
            return;
        }
        if self.extra.record_ready {
            self.extra.record_ready = false;
            self.status = "Record cancelled".into();
            return;
        }
        if !self.record_target_ok() {
            return;
        }
        if self.is_playing() {
            // punch in where the playhead is
            self.begin_recording();
            return;
        }
        self.extra.record_ready = true;
        self.status = "Record ready: press Play (Space) to start recording".into();
    }

    /// The Record button blinks while recording is ready and waiting for Play.
    pub fn record_lit(&self, ctx: &egui::Context) -> bool {
        if !self.extra.record_ready {
            return false;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        (ctx.input(|i| i.time) * 2.0) as i64 % 2 == 0
    }

    /// Is there something to record onto? Explains what is missing when not.
    fn record_target_ok(&mut self) -> bool {
        match self.mode {
            Mode::Multitrack => match &self.session {
                None => {
                    self.message("No session", "Create a multitrack session first (File > New > Multitrack Session), pick an input on a track and arm it with R.");
                    false
                }
                Some(s) if !s.tracks.iter().any(|t| t.arm && t.can_record()) => {
                    self.status = "Choose an input on a track and arm it (R) to record".into();
                    self.message("Nothing to record on", "Pick an input for a track (the \u{2192} row of the track) and arm it with its R button, then press Record and Play.");
                    false
                }
                Some(_) => true,
            },
            Mode::Waveform => {
                if self.cur().is_none() {
                    self.status = "Create or open a file to record into".into();
                    self.dialog = Some(Dialog::NewFile {
                        name: "Untitled Recording".into(),
                        rate: 48000,
                        channels: 2,
                        bits: 32,
                        make_default: false,
                    });
                    false
                } else {
                    true
                }
            }
        }
    }

    /// Start capturing now at the playhead. Multitrack plays along; Waveform records in place.
    pub fn begin_recording(&mut self) -> bool {
        let at = self.cursor();
        let track = if self.mode == Mode::Multitrack {
            self.session
                .as_ref()
                .and_then(|s| s.tracks.iter().position(|t| t.arm && t.can_record()))
        } else {
            None
        };
        self.reset_level_meters();
        self.extra.live = LiveTake::default();
        match self.engine.start_recording() {
            Ok(()) => {
                self.extra.live.rate = self.engine.lock().rec_rate;
                self.recording_at = Some((at, track));
                self.status = format!("Recording from {}", self.engine.in_device_name);
                if self.mode == Mode::Multitrack && !self.is_playing() {
                    self.play();
                }
                true
            }
            Err(e) => {
                self.message("Recording unavailable", &e);
                false
            }
        }
    }

    /// Copy what the input captured since last frame into the live overview (cheap: only the
    /// new samples, as min/max blocks).
    pub fn tick_live_take(&mut self) {
        let s = self.engine.lock();
        if !s.recording {
            return;
        }
        let chans = s.rec.len();
        let avail = s.rec.iter().map(|c| c.len()).min().unwrap_or(0);
        let live = &mut self.extra.live;
        live.rate = s.rec_rate;
        if live.peaks.len() != chans {
            live.peaks = vec![vec![]; chans];
        }
        while live.frames + LIVE_BLOCK <= avail {
            for (c, ch) in s.rec.iter().enumerate() {
                let b = &ch[live.frames..live.frames + LIVE_BLOCK];
                let mm = b
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
                live.peaks[c].push(mm);
            }
            live.frames += LIVE_BLOCK;
        }
    }

    /// Draw the growing take: a red-tinted clip from where recording began to now, with its
    /// waveform. `lane` is the clip area; channels share it top to bottom.
    pub fn draw_live_take(
        &self,
        p: &egui::Painter,
        lane: Rect,
        at: f64,
        x_of: &dyn Fn(f64) -> f32,
    ) {
        let live = &self.extra.live;
        let dur = live.duration();
        if live.peaks.is_empty() || dur <= 0.0 {
            return;
        }
        let x0 = x_of(at).max(lane.left());
        let x1 = x_of(at + dur).min(lane.right());
        if x1 <= x0 {
            return;
        }
        let r = Rect::from_min_max(pos2(x0, lane.top() + 1.0), pos2(x1, lane.bottom() - 1.0));
        p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(120, 30, 34, 150));
        p.rect_stroke(
            r,
            0.0,
            Stroke::new(1.0, Color32::from_rgb(230, 70, 70)),
            egui::StrokeKind::Inside,
        );
        let nch = live.peaks.len().max(1);
        let h = r.height() / nch as f32;
        let span_x = (x_of(at + 1.0) - x_of(at)).max(1e-6); // points per second
        // one column per screen pixel, like a recorded clip's waveform
        let ppp = p.pixels_per_point().max(0.5);
        let colw = 1.0 / ppp;
        let ax = x_of(at);
        let col = Color32::from_rgb(255, 150, 140);
        for (c, peaks) in live.peaks.iter().enumerate() {
            let mid = r.top() + h * (c as f32 + 0.5);
            let amp = h * 0.45;
            let mut shapes = Vec::new();
            let mut x = (x0 * ppp).floor() / ppp;
            while x < x1 {
                let t0 = (x - ax) as f64 / span_x as f64;
                let t1 = t0 + colw as f64 / span_x as f64;
                let b0 = ((t0.max(0.0) * live.rate as f64) as usize / LIVE_BLOCK).min(peaks.len());
                let b1 = ((t1.max(0.0) * live.rate as f64).ceil() as usize / LIVE_BLOCK + 1)
                    .min(peaks.len());
                if b0 < b1 {
                    let (lo, hi) = peaks[b0..b1]
                        .iter()
                        .fold((f32::MAX, f32::MIN), |(a, b), &(l, h)| (a.min(l), b.max(h)));
                    let y0 = mid - hi.clamp(-1.0, 1.0) * amp;
                    let y1 = (mid - lo.clamp(-1.0, 1.0) * amp).max(y0 + colw);
                    shapes.push(egui::Shape::rect_filled(
                        Rect::from_min_max(pos2(x, y0), pos2(x + colw, y1)),
                        0.0,
                        col,
                    ));
                }
                x += colw;
            }
            p.extend(shapes);
        }
        p.text(
            r.left_top() + vec2(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            format!("\u{25cf} Recording  {:.1} s", dur),
            FontId::proportional(11.0),
            Color32::WHITE,
        );
    }

    /// Keep the input open while a track is armed, monitored or recording, and route the
    /// monitored tracks to the output. Runs once per frame; cheap when nothing changes.
    pub fn sync_input(&mut self) {
        if self.screenshot.is_some() {
            return;
        }
        let recording = self.is_recording();
        let mut monitors = Vec::new();
        let mut armed = false;
        if self.mode == Mode::Multitrack {
            if let Some(s) = &self.session {
                for t in &s.tracks {
                    if !t.can_record() {
                        continue;
                    }
                    armed |= t.arm || t.io.monitor;
                    if t.io.monitor && !t.mute {
                        monitors.push(crate::engine::Monitor {
                            chans: input_channels(&t.io.input),
                            gain: 10f32.powf(t.volume_db / 20.0),
                            pan: t.pan,
                        });
                    }
                }
            }
        }
        let need = recording || self.extra.record_ready || armed;
        {
            let mut s = self.engine.lock();
            if s.monitors != monitors {
                s.monitors = monitors;
            }
        }
        if !need || !self.engine.input_failed() {
            self.engine.set_listening(need);
        }
        if need && !recording {
            // keep the meters moving while the input is listened to
            if let Some(c) = &self.engine.ctx {
                c.request_repaint_after(std::time::Duration::from_millis(33));
            }
        }
    }

    /// The level an armed or monitored track shows: its input channels.
    pub fn track_input_level(&self, ti: usize, in_peaks: &[f32]) -> Option<[f32; 2]> {
        let t = self.session.as_ref()?.tracks.get(ti)?;
        if !(t.arm || t.io.monitor) || !t.can_record() || in_peaks.is_empty() {
            return None;
        }
        let [a, b] = input_channels(&t.io.input);
        let get = |c: usize| in_peaks.get(c).or(in_peaks.first()).copied().unwrap_or(0.0);
        Some([get(a), get(b)])
    }

    // ------------------------------------------------------------ Search Help

    /// Commands matching a query, best first (title matches before description matches).
    pub fn help_matches(q: &str) -> Vec<&'static Cmd> {
        let q = q.trim().to_lowercase();
        if q.is_empty() {
            return vec![];
        }
        let words: Vec<&str> = q.split_whitespace().collect();
        let mut v: Vec<(u8, &'static Cmd)> = crate::commands::all()
            .filter_map(|c| {
                let title = c.title.to_lowercase();
                let doc = c.doc.to_lowercase();
                if !words
                    .iter()
                    .all(|w| title.contains(w) || doc.contains(w) || c.id.contains(w))
                {
                    return None;
                }
                let rank = if title.starts_with(&q) {
                    0
                } else if title.contains(&q) {
                    1
                } else if words.iter().all(|w| title.contains(w)) {
                    2
                } else {
                    3
                };
                Some((rank, c))
            })
            .collect();
        v.sort_by_key(|(r, c)| (*r, c.title));
        v.into_iter().map(|(_, c)| c).collect()
    }

    pub fn help_topic_open(&mut self, id: Option<&'static str>) {
        self.extra.help_topic = Some(id.unwrap_or(""));
    }

    /// The results list under the Search Help box (drawn while it has text).
    pub fn help_results_popup(&mut self, ctx: &egui::Context, anchor: Rect, focused: bool) {
        let q = self.extra.help_query.clone();
        if q.trim().is_empty() {
            return;
        }
        let id = egui::Id::new("help_results");
        let hovering = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        if !focused && !hovering {
            return;
        }
        let hits = App::help_matches(&q);
        let mut run: Option<&'static str> = None;
        let mut topic: Option<&'static str> = None;
        let area = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(
                (anchor.right() - 380.0).max(0.0),
                anchor.bottom() + 2.0,
            ))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(372.0);
                    if hits.is_empty() {
                        ui.label(
                            RichText::new(format!("No help found for \u{201c}{q}\u{201d}"))
                                .color(theme::text_dim()),
                        );
                    }
                    egui::ScrollArea::vertical()
                        .max_height(360.0)
                        .show(ui, |ui| {
                            for c in hits.iter().take(40) {
                                ui.horizontal(|ui| {
                                    let b = ui.add(
                                        egui::Button::new(RichText::new(c.title).strong())
                                            .right_text(
                                                RichText::new(c.shortcut).color(theme::time_blue()),
                                            )
                                            .min_size(vec2(290.0, 20.0)),
                                    );
                                    if b.on_hover_text(c.doc).clicked() {
                                        run = Some(c.id);
                                    }
                                    if ui
                                        .small_button("?")
                                        .on_hover_text("Read the help for this command")
                                        .clicked()
                                    {
                                        topic = Some(c.id);
                                    }
                                });
                            }
                        });
                    ui.separator();
                    if ui.small_button("Open Help window").clicked() {
                        topic = Some("");
                    }
                });
            });
        ctx.data_mut(|d| d.insert_temp(id, area.response.contains_pointer()));
        if let Some(c) = run {
            self.extra.help_query.clear();
            ctx.data_mut(|d| d.insert_temp(id, false));
            if let Err(e) = self.exec(c, ctx) {
                self.message("Cannot run command", &e);
            }
        }
        if let Some(t) = topic {
            ctx.data_mut(|d| d.insert_temp(id, false));
            self.extra.help_topic = Some(t);
        }
    }

    /// The Help window: search every command and read what it does, where it is and its shortcut.
    pub fn help_window(&mut self, ctx: &egui::Context) {
        let Some(topic) = self.extra.help_topic else {
            return;
        };
        let mut open = true;
        let mut run: Option<&'static str> = None;
        let mut pick: Option<&'static str> = None;
        egui::Window::new("AudioCraft Help").id(egui::Id::new("help_window")).open(&mut open).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).default_size([640.0, 460.0]).resizable(true).collapsible(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.add(egui::TextEdit::singleline(&mut self.extra.help_query).desired_width(300.0).hint_text("e.g. fade, noise, record"));
            });
            ui.separator();
            ui.columns(2, |cols| {
                let hits = if self.extra.help_query.trim().is_empty() { crate::commands::all().collect::<Vec<_>>() } else { App::help_matches(&self.extra.help_query) };
                egui::ScrollArea::vertical().id_salt("help_list").max_height(380.0).show(&mut cols[0], |ui| {
                    for c in hits {
                        if ui.selectable_label(c.id == topic, c.title).clicked() {
                            pick = Some(c.id);
                        }
                    }
                });
                let ui = &mut cols[1];
                match crate::commands::all().find(|c| c.id == topic) {
                    Some(c) => {
                        ui.label(RichText::new(c.title).size(16.0).strong().color(theme::accent()));
                        if !c.shortcut.is_empty() {
                            ui.label(RichText::new(format!("Shortcut: {}", c.shortcut)).color(theme::time_blue()));
                        }
                        ui.label(RichText::new(format!("Command ID: {}", c.id)).small().color(theme::text_dim()));
                        ui.add_space(6.0);
                        ui.label(c.doc);
                        ui.add_space(10.0);
                        if crate::theme::dialog_button(ui, "Run it now", true).clicked() {
                            run = Some(c.id);
                        }
                    }
                    None => {
                        ui.label(RichText::new("AudioCraft Help").size(16.0).strong().color(theme::accent()));
                        ui.label("Pick a command on the left to read what it does. Every menu item, shortcut and panel button is a command; the Command Palette (Ctrl+Shift+P) runs them by name.");
                    }
                }
            });
        });
        if let Some(p) = pick {
            self.extra.help_topic = Some(p);
        }
        if !open {
            self.extra.help_topic = None;
        }
        if let Some(c) = run {
            if let Err(e) = self.exec(c, ctx) {
                self.message("Cannot run command", &e);
            }
        }
    }

    // ------------------------------------------------------------ Batch Processor

    pub fn batch_start(
        &mut self,
        inputs: Vec<PathBuf>,
        steps: Vec<String>,
        out_dir: PathBuf,
        suffix: String,
    ) {
        if self.extra.batch.is_some() {
            return;
        }
        let status = Arc::new(Mutex::new(
            inputs
                .iter()
                .map(|_| "Waiting".to_string())
                .collect::<Vec<_>>(),
        ));
        let (tx, rx) = mpsc::channel();
        let st = status.clone();
        let ctx = self.engine.ctx.clone();
        std::thread::spawn(move || {
            let msg =
                crate::commands::run_files_with(&inputs, &steps, &out_dir, &suffix, &|i, s| {
                    if let Ok(mut v) = st.lock() {
                        if let Some(x) = v.get_mut(i) {
                            *x = s;
                        }
                    }
                    if let Some(c) = &ctx {
                        c.request_repaint();
                    }
                });
            let _ = tx.send(msg.unwrap_or_else(|e| format!("Cannot run the batch: {e}")));
            if let Some(c) = &ctx {
                c.request_repaint();
            }
        });
        self.extra.batch = Some(BatchRun { status, done: rx });
    }

    /// Per-file status of the running (or last) batch, and the finished message.
    pub fn batch_poll(&mut self) -> Option<String> {
        let run = self.extra.batch.as_ref()?;
        if let Ok(v) = run.status.lock() {
            self.extra.batch_lines = v.clone();
        }
        match run.done.try_recv() {
            Ok(msg) => {
                if let Ok(v) = run.status.lock() {
                    self.extra.batch_lines = v.clone();
                }
                self.extra.batch = None;
                self.status = msg.lines().next().unwrap_or("").to_string();
                Some(msg)
            }
            Err(_) => None,
        }
    }

    // ------------------------------------------------------------ speech

    /// Start speaking `text` on a worker; `then` 1 = preview it, 2 = insert it as a file.
    pub fn speech_start(&mut self, text: &str, voice: &str, rate: i32, pitch: f32, then: u8) {
        let key = hash_of(&[text, voice, &rate.to_string(), &format!("{pitch:.2}")]);
        if let Some((k, d)) = &self.extra.speech_result {
            if *k == key {
                let d = d.clone();
                self.speech_finish(d, then);
                return;
            }
        }
        if self.extra.speech_job.is_some() {
            self.extra.speech_then = then;
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (t, v) = (text.to_string(), voice.to_string());
        let ctx = self.engine.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::speech::synthesize_pitched(&t, &v, rate, pitch));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        self.extra.speech_job = Some((key, rx));
        self.extra.speech_then = then;
        self.extra.speech_status = "Speaking...".into();
    }

    pub fn speech_poll(&mut self) {
        let Some((key, rx)) = &self.extra.speech_job else {
            return;
        };
        let key = *key;
        match rx.try_recv() {
            Ok(Ok(d)) => {
                self.extra.speech_job = None;
                let d = Arc::new(d);
                self.extra.speech_result = Some((key, d.clone()));
                let then = self.extra.speech_then;
                self.speech_finish(d, then);
            }
            Ok(Err(e)) => {
                self.extra.speech_job = None;
                self.extra.speech_status = e;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.extra.speech_job = None;
                self.extra.speech_status = "The speech engine stopped unexpectedly.".into();
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Preview (1) or insert (2) generated speech. Inserting adds a new file and keeps the
    /// dialog open so more can be made.
    /// Hear generated speech on the separate preview player (like the Files panel): the
    /// transport, the playhead and the tracks are not touched.
    pub fn speech_preview_start(&mut self, d: Arc<AudioData>) {
        self.stop_preview();
        self.stop_file_preview();
        if d.frames() == 0 {
            return;
        }
        self.engine.ensure_output();
        self.engine.lock().files_preview = Some(crate::engine::FilePreview::new(d.clone(), false));
        self.extra.speech_previewing = true;
    }

    /// Whether the speech preview is still sounding (it ends by itself).
    pub fn speech_previewing(&mut self) -> bool {
        if self.extra.speech_previewing && self.engine.lock().files_preview.is_none() {
            self.extra.speech_previewing = false;
        }
        self.extra.speech_previewing
    }

    pub fn speech_preview_stop(&mut self) {
        if self.extra.speech_previewing {
            self.extra.speech_previewing = false;
            self.engine.lock().files_preview = None;
        }
    }

    pub fn speech_finish(&mut self, d: Arc<AudioData>, then: u8) {
        self.extra.speech_status = format!("{:.1} s of speech ready", d.duration());
        match then {
            1 => self.speech_preview_start(d),
            2 => {
                self.stop_preview();
                let n = self.files.len() + 1;
                let keep = self.dialog.take();
                self.add_file(
                    format!("Speech {n:02}.wav"),
                    None,
                    (*d).clone(),
                    32,
                    true,
                    "Generate Speech",
                );
                self.switch_mode(Mode::Waveform);
                self.dialog = keep;
                self.extra.speech_status = format!("Added Speech {n:02}.wav to the Files panel");
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ decoders

    pub fn decoder_download_start(&mut self, id: &str) {
        if self.extra.download.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let id2 = id.to_string();
        let ctx = self.engine.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::ffmpeg::download(&id2));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        self.extra.download = Some((id.to_string(), rx));
        self.extra.download_status = "Downloading... (this can take a minute)".into();
    }

    /// Finished downloads become the chosen decoder of the preferences being edited.
    pub fn decoder_download_poll(&mut self) {
        let Some((_, rx)) = &self.extra.download else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(exe)) => {
                self.extra.download = None;
                self.extra.download_status = format!("Installed {} — now selected", exe.display());
                let s = exe.to_string_lossy().to_string();
                if let Some((p, _, _)) = &mut self.prefs_edit {
                    p.video_decoder_exe = s.clone();
                }
                self.prefs.video_decoder_exe = s;
                crate::ffmpeg::set_selected(Some(exe));
                crate::prefs::save(&self.prefs);
            }
            Ok(Err(e)) => {
                self.extra.download = None;
                self.extra.download_status = e;
            }
            Err(mpsc::TryRecvError::Disconnected) => self.extra.download = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Decode the preview frames again, converted to Rec.709.
    pub fn convert_video_color(&mut self) -> Result<(), String> {
        let (path, quality) = match &self.video {
            crate::video::VideoState::Ready(m) => (m.path.clone(), self.prefs.video_quality),
            _ => return Err("Import a video first.".into()),
        };
        let exe = crate::video::ensure_decoder(&self.prefs)?;
        if let crate::video::VideoState::Ready(m) = &mut self.video {
            m.frames.clear();
            m.complete = false;
            m.converted = true;
        }
        self.video_tex = None;
        let opts = crate::video::ImportOpts {
            quality,
            convert_color: true,
            frames_only: true,
        };
        self.video_queue
            .start(path, exe, opts, self.engine.ctx.clone());
        self.status = "Converting the video preview to Rec.709...".into();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_inputs_map_to_device_channels() {
        assert_eq!(input_channels("Default Stereo Input"), [0, 1]);
        assert_eq!(input_channels("Mono: Left"), [0, 0]);
        assert_eq!(input_channels("Mono: Right"), [1, 1]);
        assert_eq!(input_channels("Stereo: USB Mic"), [0, 1]);
    }

    #[test]
    fn help_search_finds_commands_by_title_and_description() {
        let hits = App::help_matches("fade");
        assert!(!hits.is_empty());
        assert!(
            hits[0].title.to_lowercase().contains("fade"),
            "{}",
            hits[0].title
        );
        assert!(App::help_matches("").is_empty());
        assert!(App::help_matches("zzzz-no-such-thing").is_empty());
        assert!(
            App::help_matches("new window")
                .iter()
                .any(|c| c.id == "window.new_window")
        );
    }

    #[test]
    fn extra_commands_are_unique() {
        let mut ids: Vec<&str> = crate::commands::all().map(|c| c.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(n, ids.len());
    }
}
