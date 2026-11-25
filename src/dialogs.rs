//! Modal dialogs and the analysis windows.

use crate::app::*;
use crate::audio::AudioData;
use crate::dsp::{self, EffectSlot, FadeCurve, NoiseColor, Wave};
use crate::session::Session;
use crate::sysui::Ask;
use crate::theme;
use egui::{Color32, FontId, RichText, Sense, Stroke, pos2, vec2};

fn ok_cancel(ui: &mut egui::Ui, ok_label: &str, ok: &mut bool, close: &mut bool) {
    ui.separator();
    right_row(ui, |ui| {
        if crate::theme::dialog_button(ui, ok_label, true).clicked() {
            *ok = true;
        }
        if crate::theme::dialog_button(ui, "Cancel", false).clicked() {
            *close = true;
        }
    });
}

const RATES: [u32; 10] = [
    8000, 11025, 16000, 22050, 32000, 44100, 48000, 88200, 96000, 192000,
];

/// Sample rates offered by the New File / New Session dialogs (highest first).
const NEW_RATES: [u32; 13] = [
    192000, 176400, 96000, 88200, 64000, 48000, 44100, 32000, 22050, 16000, 11025, 8000, 6000,
];

/// A grid label aligned to the right, dimmed, like the classic dialogs.
/// A right-aligned label of fixed width. (A right-to-left layout without a width would take
/// all the room a window could have, so dialogs grew to the whole screen.)
fn rlabel(ui: &mut egui::Ui, t: &str) {
    ui.allocate_ui_with_layout(
        vec2(104.0, 18.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| ui.label(RichText::new(t).color(theme::text_dim())),
    );
}

/// A right-aligned row as wide as the dialog's content so far (never wider).
fn right_row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let w = ui.min_rect().width().max(300.0);
    ui.allocate_ui_with_layout(
        vec2(w, 26.0),
        egui::Layout::right_to_left(egui::Align::Center),
        add,
    );
}

/// Sample rate box: pick from the list or type any rate.
fn rate_combo(ui: &mut egui::Ui, id: &str, rate: &mut u32, w: f32) {
    egui::ComboBox::from_id_salt(id)
        .width(w)
        .selected_text(format!("{rate}"))
        .show_ui(ui, |ui| {
            for r in NEW_RATES {
                ui.selectable_value(rate, r, format!("{r}"));
            }
        });
}

/// OK and Cancel at the bottom right, as rounded outline buttons.
fn ok_cancel_right(ui: &mut egui::Ui, ok: &mut bool, close: &mut bool) {
    ui.add_space(8.0);
    right_row(ui, |ui| {
        if crate::theme::dialog_button(ui, "Cancel", false).clicked() {
            *close = true;
        }
        if crate::theme::dialog_button(ui, "OK", true).clicked() {
            *ok = true;
        }
    });
    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        *ok = true;
    }
}

/// Where new sessions go unless the person picks another folder.
pub fn default_session_folder() -> String {
    if cfg!(test) {
        return std::env::temp_dir()
            .join(format!("audiocraft-test-docs-{}", std::process::id()))
            .join("AudioCraft")
            .display()
            .to_string();
    }
    let base = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Documents")
        .join("AudioCraft")
        .display()
        .to_string()
}

/// Turn a typed session name into a safe folder and file name. Windows forbids these characters
/// in paths; replacing them keeps a typo from turning into a confusing file error.
pub fn session_name_stem(name: &str) -> Option<String> {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let stem = stem.trim().trim_matches('.').trim();
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_string())
    }
}

/// The starting values of the New Multitrack Session dialog (a pure function so it can be tested).
/// The reference dialog opens with 48 kHz / 24-bit / Stereo / six tracks.
pub fn new_session_dialog(rate: u32) -> Dialog {
    Dialog::NewSession {
        name: "Untitled Session 1".to_string(),
        rate,
        tracks: 6,
        folder: default_session_folder(),
        bits: 24,
        mix: 1,
        make_default: false,
    }
}

/// The dialog a click on the Waveform / Multitrack button should show. An empty project
/// asks for the details of the new file or session; a real (new or opened) project just
/// switches views. Kept pure so the rule can be regression-tested without a window.
pub fn dialog_for_mode(
    mode: crate::app::Mode,
    rate: u32,
    has_current_file: bool,
    has_session: bool,
    file_count: usize,
) -> Option<Dialog> {
    match mode {
        crate::app::Mode::Multitrack if !has_session => Some(new_session_dialog(rate)),
        crate::app::Mode::Waveform if !has_current_file && !has_session => Some(Dialog::NewFile {
            name: format!("Untitled {}", file_count + 1),
            rate,
            channels: 2,
            bits: 32,
            make_default: false,
        }),
        _ => None,
    }
}

/// Folder a new session is saved in, and the .sesx file inside it.
pub fn session_paths(folder: &str, name: &str) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let stem = session_name_stem(name)?;
    let base = folder.trim();
    let dir = if base.is_empty() {
        std::path::PathBuf::from(&stem)
    } else {
        std::path::Path::new(base).join(&stem)
    };
    Some((dir.clone(), dir.join(format!("{stem}.sesx"))))
}

/// Stretch and Pitch on frames a..b: `ratio` is the tempo (1/stretch), `semis` the pitch shift.
/// Mode 0 resamples (length and pitch change together); otherwise each is done on its own.
pub fn stretch_and_pitch(
    d: &AudioData,
    a: usize,
    b: usize,
    ratio: f64,
    semis: f64,
    mode: u8,
) -> AudioData {
    if mode == 0 {
        if (ratio - 1.0).abs() < 1e-6 {
            d.clone()
        } else {
            dsp::change_speed(d, a, b, ratio)
        }
    } else {
        let mut out = if (ratio - 1.0).abs() > 1e-6 {
            dsp::stretch_range(d, a, b, 1.0 / ratio)
        } else {
            d.clone()
        };
        if semis.abs() > 1e-3 {
            let nb = (a + ((b - a) as f64 / ratio).round() as usize).min(out.frames());
            out = dsp::pitch_range(&out, a, nb, semis);
        }
        out
    }
}

impl App {
    /// Play what Stretch and Pitch would make of the selection (or the next 15 s), looping.
    pub fn start_stretch_preview(&mut self, ratio: f32, semis: f32, mode: u8) {
        let Some(f) = self.cur() else { return };
        let r = f.data.rate as f64;
        let (a, b) = match f.selection {
            Some((x, y)) if y > x => ((x * r) as usize, ((y * r) as usize).min(f.data.frames())),
            _ => {
                let s = (f.cursor * r) as usize;
                (
                    s.min(f.data.frames()),
                    (s + (15.0 * r) as usize).min(f.data.frames()),
                )
            }
        };
        if b <= a {
            return;
        }
        let seg = f.data.slice(a, b);
        let n = seg.frames();
        let out = stretch_and_pitch(&seg, 0, n, ratio as f64, semis as f64, mode);
        self.play_preview(std::sync::Arc::new(out), true);
    }

    /// Play rendered audio instead of the file (dialog previews); `stop_preview` ends it.
    pub fn play_preview(&mut self, data: std::sync::Arc<AudioData>, looping: bool) {
        if self.preview_audio.is_none() {
            self.stop();
        }
        self.engine.ensure_output();
        self.reset_level_meters();
        {
            let mut s = self.engine.lock();
            s.source = crate::engine::Source::File(data.clone());
            s.pos = 0.0;
            s.end = data.duration();
            s.loop_start = 0.0;
            s.looping = looping;
            s.playing = true;
        }
        self.preview_audio = Some(data);
    }

    /// The Preview row of the generator dialogs (Tones, Noise, DTMF): plays what OK would insert
    /// (looping, at most 10 s); while it plays, a change is heard once the slider is let go.
    fn gen_preview_row(&mut self, ui: &mut egui::Ui, changed: bool, make: impl FnOnce() -> AudioData) {
        let previewing = self.preview_audio.is_some();
        let mut start = false;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if theme::icon_button(ui, theme::Icon::Play, previewing, "Preview: play what OK inserts (loops)").clicked() {
                if previewing {
                    self.stop_preview();
                } else {
                    start = true;
                }
            }
            ui.label(
                RichText::new(if previewing { "Previewing (loops)" } else { "Preview" })
                    .small()
                    .color(theme::text_dim()),
            );
        });
        if previewing && changed {
            self.preview_dirty = true;
        }
        let held = ui.input(|i| i.pointer.any_down());
        if start || (self.preview_audio.is_some() && self.preview_dirty && !held) {
            self.preview_dirty = false;
            let d = make();
            if d.frames() > 0 {
                self.play_preview(std::sync::Arc::new(d), true);
            }
        }
    }

    pub fn stop_preview(&mut self) {
        if self.preview_audio.take().is_some() {
            {
                let mut s = self.engine.lock();
                s.playing = false;
                s.looping = false;
            }
            self.reset_level_meters();
        }
    }

    pub fn dialogs(&mut self, ctx: &egui::Context) {
        let batch_msg = self.batch_poll();
        self.speech_poll();
        if self.extra.batch.is_some() || self.extra.speech_job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let Some(mut dlg) = self.dialog.take() else {
            return;
        };
        if let (Some(msg), Dialog::Batch { result, .. }) = (batch_msg, &mut dlg) {
            *result = msg;
        }
        let mut speech_action: u8 = 0;
        let mut color_convert = false;
        let mut open = true;
        let mut close = false;
        let mut ok = false;
        let mut run_now = false;
        let title = match &dlg {
            Dialog::NewWorkspace { .. } => "New Workspace".to_string(),
            Dialog::EditWorkspaces => "Edit Workspaces".to_string(),
            Dialog::RenameMarker { .. } => "Rename Marker".to_string(),
            Dialog::RenameTrack { .. } => "Rename Track".to_string(),
            Dialog::Gain { .. } => "Gain".to_string(),
            Dialog::Batch { .. } => "Batch Processor".to_string(),
            Dialog::TrackColor { .. } => "Track Color".to_string(),
            Dialog::RateMismatch { .. } => "Sample Rate Mismatch".to_string(),
            Dialog::NewFile { .. } => "New Audio File".to_string(),
            Dialog::ConfirmReplace { .. } => "File Already Exists".to_string(),
            Dialog::NewSession { .. } => "New Multitrack Session".to_string(),
            Dialog::Effect { effect, .. } => format!("Effect - {}", effect.name()),
            Dialog::Normalize { .. } => "Normalize".into(),
            Dialog::Fade { fade_in, .. } => {
                if *fade_in {
                    "Fade In".into()
                } else {
                    "Fade Out".into()
                }
            }
            Dialog::Speed { .. } => "Stretch and Pitch".into(),
            Dialog::Loudness { .. } => "Match Loudness".into(),
            Dialog::NoiseReduction { .. } => "Noise Reduction".into(),
            Dialog::Tone { .. } => "Generate Tones".into(),
            Dialog::Noise { .. } => "Generate Noise".into(),
            Dialog::InsertSilence { .. } => "Insert Silence".into(),
            Dialog::Convert { .. } => "Convert Sample Type".into(),
            Dialog::InterpretSampleRate { .. } => "Interpret Sample Rate".into(),
            Dialog::FrequencySplit { .. } => "Frequency Band Splitter".into(),
            Dialog::Speech { .. } => "Generate Speech".into(),
            Dialog::ColorSpace { .. } => "Video Color Space".into(),
            Dialog::Preferences => "Preferences".into(),
            Dialog::Envelope { gain, .. } => {
                if *gain {
                    "Gain Envelope".into()
                } else {
                    "Fade Envelope".into()
                }
            }
            Dialog::DeleteSilence { .. } => "Delete Silence".into(),
            Dialog::MarkAudio { .. } => "Mark Audio".into(),
            Dialog::PitchBender { .. } => "Pitch Bender".into(),
            Dialog::Dtmf { .. } => "Generate DTMF Tones".into(),
            Dialog::ClickPop { .. } => "Click/Pop Eliminator".into(),
            Dialog::PluginManager => "Audio Plug-In Manager".into(),
            Dialog::About => "About AudioCraft".into(),
            Dialog::Shortcuts => "Keyboard Shortcuts".into(),
            Dialog::Message(t, _) => t.clone(),
            Dialog::Export(o) => {
                if matches!(o.src, crate::export::ExportSource::File(_)) {
                    "Export Audio File".into()
                } else {
                    "Export Multitrack Mixdown".into()
                }
            }
            Dialog::Doppler(_) => "Effect - Doppler Shifter (process)".into(),
            Dialog::SoundRemover { .. } => "Effect - Sound Remover (process)".into(),
        };
        let rate = self.cur().map(|f| f.data.rate).unwrap_or(48000) as f32;
        let has_sel = self.cur().and_then(|f| f.selection).is_some();
        let scope = if has_sel {
            "the selection"
        } else {
            "the entire file"
        };
        // each dialog keeps its own size and place: one shared id made every dialog inherit
        // the size of the last big one
        let screen = ctx.content_rect();
        egui::Window::new(title.clone())
            .id(egui::Id::new(("dialog", title.as_str())))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(screen.center())
            .constrain(true)
            .min_width(320.0)
            .max_width((screen.width() * 0.9).clamp(320.0, 900.0))
            .max_height((screen.height() * 0.9).max(200.0))
            .show(ctx, |ui| match &mut dlg {
                Dialog::NewWorkspace { name } => {
                    ui.label("Name:");
                    let r = ui.text_edit_singleline(name);
                    r.request_focus();
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                    ok_cancel(ui, "OK", &mut ok, &mut close);
                }
                Dialog::EditWorkspaces => {
                    ui.label(RichText::new("Workspaces").strong());
                    let mut delete: Option<String> = None;
                    let mut go: Option<String> = None;
                    egui::Grid::new("ws_edit").num_columns(3).striped(true).show(ui, |ui| {
                        for n in self.ws.names() {
                            let custom = self.ws.custom.contains(&n);
                            let changed = self.ws.saved.iter().any(|(s, _)| *s == n);
                            ui.label(if n == self.ws.current { RichText::new(&n).color(theme::accent()) } else { RichText::new(&n) });
                            if ui.small_button("Switch").clicked() {
                                go = Some(n.clone());
                            }
                            if custom {
                                if ui.small_button("Delete").clicked() {
                                    delete = Some(n.clone());
                                }
                            } else if changed {
                                if ui.small_button("Reset").on_hover_text("Forget the saved changes").clicked() {
                                    delete = Some(n.clone());
                                }
                            } else {
                                ui.label("");
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(n) = delete {
                        self.ws.delete(&n);
                    }
                    if let Some(n) = go {
                        self.ws.switch(&n);
                    }
                    ui.add_space(6.0);
                    if crate::theme::dialog_button(ui, "Close", false).clicked() {
                        close = true;
                    }
                }
                Dialog::RenameMarker { index, name } => {
                    ui.label(format!("Marker {} at {}", *index + 1, self.fmt(self.marker_snapshot().get(*index).map(|m| m.time).unwrap_or(0.0))));
                    let r = ui.text_edit_singleline(name);
                    r.request_focus();
                    ok_cancel(ui, "OK", &mut ok, &mut close);
                }
                Dialog::RenameTrack { index, name } => {
                    ui.label(format!("Track {}", *index + 1));
                    let r = ui.text_edit_singleline(name);
                    r.request_focus();
                    ok_cancel(ui, "OK", &mut ok, &mut close);
                }
                Dialog::Gain { db } => {
                    ui.horizontal(|ui| {
                        rlabel(ui, "Gain (dB):");
                        ui.add(egui::DragValue::new(db).speed(0.1).range(-60.0..=24.0));
                    });
                    ui.label(RichText::new(scope).color(theme::text_dim()));
                    ok_cancel(ui, "Apply", &mut ok, &mut close);
                }
                Dialog::Batch { inputs, preset, out_dir, suffix, result } => {
                    let running = self.extra.batch.is_some();
                    ui.set_width(520.0);
                    ui.label(RichText::new("Files to process").strong());
                    egui::Frame::new().fill(crate::theme::gray(26)).inner_margin(4).show(ui, |ui| {
                        ui.set_min_height(120.0);
                        egui::ScrollArea::vertical().max_height(180.0).auto_shrink([false, true]).show(ui, |ui| {
                            if inputs.is_empty() {
                                ui.label(RichText::new("Drop audio files here, or use Add Files.").color(theme::text_dim()));
                            }
                            let mut remove = None;
                            egui::Grid::new("batch_files").num_columns(3).striped(true).show(ui, |ui| {
                                for (i, p) in inputs.iter().enumerate() {
                                    let name = std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.clone());
                                    ui.label(name).on_hover_text(p.as_str());
                                    let st = self.extra.batch_lines.get(i).cloned().unwrap_or_else(|| "Ready".into());
                                    let col = if st.starts_with("Error") { Color32::from_rgb(230, 110, 100) } else if st.starts_with("Done") { theme::wave() } else { theme::text_dim() };
                                    ui.label(RichText::new(st).small().color(col));
                                    if ui.add_enabled(!running, egui::Button::new("\u{2715}").small()).on_hover_text("Remove from the list").clicked() {
                                        remove = Some(i);
                                    }
                                    ui.end_row();
                                }
                            });
                            if let Some(i) = remove {
                                inputs.remove(i);
                                self.extra.batch_lines.clear();
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(!running, |ui| {
                            if ui.button("Add Files...").clicked() {
                                let exts: Vec<&str> = crate::audio::AUDIO_EXTS.to_vec();
                                if let Some(files) = rfd::FileDialog::new().add_filter("Audio files", &exts).ask_files() {
                                    for p in files {
                                        let s = p.to_string_lossy().to_string();
                                        if !inputs.contains(&s) {
                                            inputs.push(s);
                                        }
                                    }
                                }
                            }
                            if ui.button("Add Open Files").on_hover_text("Every saved file open in the Files panel").clicked() {
                                for f in &self.files {
                                    if let Some(p) = &f.path {
                                        let s = p.to_string_lossy().to_string();
                                        if crate::audio::is_audio_path(p) && !inputs.contains(&s) {
                                            inputs.push(s);
                                        }
                                    }
                                }
                            }
                            if ui.button("Clear").clicked() {
                                inputs.clear();
                                self.extra.batch_lines.clear();
                            }
                        });
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        rlabel(ui, "Preset:");
                        let selected = crate::commands::BATCH_PRESETS.get(*preset).map(|p| p.0).unwrap_or("Choose a preset");
                        egui::ComboBox::from_id_salt("batch_preset").width(250.0).selected_text(selected).show_ui(ui, |ui| {
                            for (i, (name, steps)) in crate::commands::BATCH_PRESETS.iter().enumerate() {
                                ui.selectable_value(preset, i, *name).on_hover_text(steps.join(" -> "));
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        rlabel(ui, "Output folder:");
                        ui.add(egui::TextEdit::singleline(out_dir).desired_width(220.0));
                        if ui.button("Browse...").clicked() {
                            if let Some(p) = rfd::FileDialog::new().set_directory(out_dir.as_str()).ask_folder() {
                                *out_dir = p.to_string_lossy().to_string();
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        rlabel(ui, "Name suffix:");
                        ui.add(egui::TextEdit::singleline(suffix).desired_width(120.0));
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        let label = if running { "Running..." } else { "Run Batch" };
                        if ui.add_enabled(!inputs.is_empty() && !running, egui::Button::new(RichText::new(label).strong())).clicked() {
                            run_now = true;
                        }
                        if ui.button("Open Output Folder").clicked() {
                            let _ = std::fs::create_dir_all(out_dir.as_str());
                            let _ = crate::sysui::open_external(std::path::Path::new(out_dir.as_str()));
                        }
                        if crate::theme::dialog_button(ui, "Close", false).clicked() {
                            close = true;
                        }
                    });
                    if running {
                        ui.spinner();
                    }
                    if !result.is_empty() {
                        ui.add_space(4.0);
                        ui.label(result.as_str());
                    }
                }
                Dialog::TrackColor { color, .. } => {
                    // predefined colours, a larger custom grid and a free picker for anything else
                    const PRE: [[u8; 3]; 12] = [
                        [128, 128, 128], [240, 150, 40], [226, 192, 20], [140, 210, 70], [70, 196, 100], [46, 170, 140],
                        [40, 180, 190], [80, 160, 240], [130, 130, 240], [170, 120, 230], [220, 90, 230], [230, 90, 170],
                    ];
                    let swatch = |ui: &mut egui::Ui, c: [u8; 3], cur: &mut [u8; 3]| {
                        let (r, resp) = ui.allocate_exact_size(vec2(34.0, 24.0), Sense::click());
                        ui.painter().rect_filled(r.shrink(2.0), 0.0, Color32::from_rgb(c[0], c[1], c[2]));
                        if *cur == c {
                            ui.painter().rect_stroke(r, 0.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
                        }
                        if resp.clicked() {
                            *cur = c;
                        }
                    };
                    ui.label(RichText::new("Predefined Colors").color(theme::text_dim()));
                    for row in PRE.chunks(6) {
                        ui.horizontal(|ui| {
                            for c in row {
                                swatch(ui, *c, color);
                            }
                        });
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("Custom Colors").color(theme::text_dim()));
                    for row in 0..4 {
                        ui.horizontal(|ui| {
                            for k in 0..6 {
                                let h = (row * 6 + k) as f32 / 24.0;
                                let c = egui::ecolor::Hsva::new(h, 0.62, 0.92, 1.0);
                                let c32: Color32 = c.into();
                                swatch(ui, [c32.r(), c32.g(), c32.b()], color);
                            }
                        });
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Any colour:").color(theme::text_dim()));
                        // the swatch opens the picker: its square sets saturation (across) and
                        // brightness (up), the strip under it the hue — no slider needed
                        crate::theme::colour_button(ui, "track_color", color);
                    });
                    ok_cancel(ui, "OK", &mut ok, &mut close);
                }
                Dialog::RateMismatch { file_id, choice, .. } => {
                    let (fname, frate, fbits) = self.file(*file_id).map(|f| (f.name.clone(), f.data.rate, if f.float { 32 } else { f.bits })).unwrap_or_default();
                    let (srate, sbits) = self.session.as_ref().map(|s| (s.rate, s.bits)).unwrap_or((48000, 32));
                    let bt = |b: u16| if b == 32 { "32-bit (float)".to_string() } else { format!("{b}-bit") };
                    ui.label(format!("\"{fname}\" is {frate} Hz, {}; the session is {srate} Hz, {}.", bt(fbits), bt(sbits)));
                    if frate != srate {
                        ui.label(RichText::new("Played as it is, it would sound faster or slower and at a different pitch.").color(theme::text_dim()));
                    } else {
                        ui.label(RichText::new("Only the bit depth differs; it plays at the right speed either way.").color(theme::text_dim()));
                    }
                    ui.add_space(8.0);
                    if ui.button(format!("Convert the file to the session's settings ({srate} Hz, {})", bt(sbits))).clicked() {
                        *choice = 1;
                        ok = true;
                    }
                    let empty = self.session.as_ref().map(|s| s.tracks.iter().all(|t| t.clips.is_empty())).unwrap_or(true);
                    if ui
                        .add(egui::Button::new(format!("Change the session to the file's settings ({frate} Hz, {})", bt(fbits))))
                        .on_hover_text(if empty { "The session is empty, so nothing else changes" } else { "The other clips are resampled as they play" })
                        .clicked()
                    {
                        *choice = 2;
                        ok = true;
                    }
                    let keep = if frate != srate { "Keep it as it is (plays faster / slower)" } else { "Keep it as it is" };
                    if ui.button(keep).clicked() {
                        *choice = 3;
                        ok = true;
                    }
                    ui.add_space(4.0);
                    if crate::theme::dialog_button(ui, "Cancel", false).clicked() {
                        close = true;
                    }
                }
                Dialog::NewFile { name, rate, channels, bits, make_default } => {
                    egui::Grid::new("nf").num_columns(3).spacing([8.0, 10.0]).show(ui, |ui| {
                        rlabel(ui, "File Name:");
                        let r = ui.add(egui::TextEdit::singleline(name).desired_width(210.0));
                        if ui.memory(|m| m.focused().is_none()) {
                            r.request_focus();
                        }
                        ui.label("");
                        ui.end_row();
                        rlabel(ui, "Sample Rate:");
                        rate_combo(ui, "nfr", rate, 190.0);
                        ui.label(RichText::new("Hz").color(theme::text_dim()));
                        ui.end_row();
                        rlabel(ui, "Channels:");
                        let ch_name = match *channels {
                            1 => "Mono".to_string(),
                            2 => "Stereo".to_string(),
                            6 => "5.1".to_string(),
                            n => format!("{n} channels"),
                        };
                        egui::ComboBox::from_id_salt("nfc").width(190.0).selected_text(ch_name).show_ui(ui, |ui| {
                            ui.selectable_value(channels, 1, "Mono");
                            ui.selectable_value(channels, 2, "Stereo");
                            ui.selectable_value(channels, 6, "5.1");
                            ui.separator();
                            if ui.selectable_label(!matches!(*channels, 1 | 2 | 6), "Custom...").clicked() {
                                *channels = 4;
                            }
                        });
                        if !matches!(*channels, 1 | 2 | 6) {
                            ui.add(egui::DragValue::new(channels).range(1..=32).suffix(" ch"));
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                        rlabel(ui, "Bit Depth:");
                        egui::ComboBox::from_id_salt("nfb").width(190.0).selected_text(if *bits == 32 { "32 (float)".to_string() } else { format!("{bits}") }).show_ui(ui, |ui| {
                            ui.selectable_value(bits, 8, "8");
                            ui.selectable_value(bits, 16, "16");
                            ui.selectable_value(bits, 24, "24");
                            ui.selectable_value(bits, 32, "32 (float)");
                        });
                        ui.label(RichText::new("bits").color(theme::text_dim()));
                        ui.end_row();
                    });
                    ui.checkbox(make_default, "Make this the default path");
                    ok_cancel_right(ui, &mut ok, &mut close);
                }
                Dialog::ConfirmReplace { path, .. } => {
                    ui.label(RichText::new(format!("The file {} already exists.", path.display().to_string())).color(theme::text()));
                    ui.add_space(8.0);
                    ui.label(RichText::new("Replace it with the new file?").color(theme::text_dim()));
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if ui.button("Replace").clicked() {
                            ok = true;
                        }
                        if ui.button("Don't Replace").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::NewSession { name, rate, tracks, folder, bits, mix, make_default } => {
                    egui::Grid::new("ns").num_columns(3).spacing([8.0, 10.0]).show(ui, |ui| {
                        rlabel(ui, "Session Name:");
                        ui.add(egui::TextEdit::singleline(name).desired_width(290.0));
                        ui.label("");
                        ui.end_row();
                        rlabel(ui, "Folder Location:");
                        ui.add(egui::TextEdit::singleline(folder).desired_width(290.0));
                        if ui.button("Browse...").clicked() {
                            if let Some(p) = rfd::FileDialog::new().set_directory(folder.as_str()).ask_folder() {
                                *folder = p.display().to_string();
                            }
                        }
                        ui.end_row();
                        rlabel(ui, "Template:");
                        egui::ComboBox::from_id_salt("nst").width(290.0).selected_text("None").show_ui(ui, |ui| {
                            let _ = ui.selectable_label(true, "None");
                        });
                        ui.label("");
                        ui.end_row();
                        rlabel(ui, "Number of Tracks:");
                        ui.add(egui::DragValue::new(tracks).range(1..=64).speed(0.2).suffix(" tracks"));
                        ui.label(RichText::new("you can add more later").color(theme::text_dim()));
                        ui.end_row();
                        rlabel(ui, "Sample Rate:");
                        ui.horizontal(|ui| {
                            rate_combo(ui, "nsr", rate, 140.0);
                            ui.label(RichText::new("Hz").color(theme::text_dim()));
                        });
                        ui.label("");
                        ui.end_row();
                        rlabel(ui, "Bit Depth:");
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("nsb").width(140.0).selected_text(if *bits == 32 { "32 (float)".to_string() } else { format!("{bits}") }).show_ui(ui, |ui| {
                                ui.selectable_value(bits, 32, "32 (float)");
                                ui.selectable_value(bits, 24, "24");
                                ui.selectable_value(bits, 16, "16");
                            });
                            ui.label(RichText::new("bits").color(theme::text_dim()));
                        });
                        ui.label("");
                        ui.end_row();
                        rlabel(ui, "Mix:");
                        egui::ComboBox::from_id_salt("nsm").width(140.0).selected_text(["Mono", "Stereo", "5.1"][(*mix as usize).min(2)]).show_ui(ui, |ui| {
                            ui.selectable_value(mix, 0, "Mono");
                            ui.selectable_value(mix, 1, "Stereo");
                            ui.selectable_value(mix, 2, "5.1");
                        });
                        ui.label("");
                        ui.end_row();
                    });
                    ui.checkbox(make_default, "Make this the default path");
                    ok_cancel_right(ui, &mut ok, &mut close);
                }
                Dialog::Effect { effect, preview } => {
                    ui.label(RichText::new(format!("Applies to {scope}.")).small().color(theme::text_dim()));
                    effect.ui(ui, rate);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.toggle_value(preview, "\u{25b6} Preview").on_hover_text("Loop playback with the effect applied in real time").changed() {
                            if *preview {
                                self.engine.lock().preview = Some(vec![EffectSlot::new(effect.clone())]);
                                if !self.is_playing() {
                                    self.engine.lock().looping = true;
                                    self.play();
                                }
                            } else {
                                self.engine.lock().preview = None;
                                self.stop();
                                self.engine.lock().looping = false;
                            }
                        }
                        if *preview {
                            self.engine.lock().preview = Some(vec![EffectSlot::new(effect.clone())]);
                        }
                        if crate::theme::dialog_button(ui, "Close", false).clicked() {
                            close = true;
                        }
                        if crate::theme::dialog_button(ui, "Apply", true).clicked() {
                            ok = true;
                        }
                    });
                }
                Dialog::Normalize { db } => {
                    ui.label(format!("Normalize {scope} so its peak reaches:"));
                    ui.add(crate::theme::RSlider::new(db, -30.0..=0.0).suffix(" dB"));
                    ok_cancel(ui, "Apply", &mut ok, &mut close);
                }
                Dialog::Fade { fade_in, curve } => {
                    ui.label(format!("Fade {scope}."));
                    ui.horizontal(|ui| {
                        ui.selectable_value(curve, FadeCurve::Linear, "Linear");
                        ui.selectable_value(curve, FadeCurve::Logarithmic, "Logarithmic");
                        ui.selectable_value(curve, FadeCurve::Cosine, "Cosine");
                    });
                    let (r, _) = ui.allocate_exact_size(vec2(280.0, 80.0), Sense::hover());
                    ui.painter().rect_filled(r, 0.0, theme::wave_bg());
                    let fi = *fade_in;
                    let pts: Vec<_> = (0..=60)
                        .map(|i| {
                            let x = i as f32 / 60.0;
                            let g = dsp::fade_gain(if fi { x } else { 1.0 - x }, *curve);
                            pos2(r.left() + x * r.width(), r.bottom() - g * r.height())
                        })
                        .collect();
                    ui.painter().add(egui::Shape::line(pts, Stroke::new(2.0, theme::wave())));
                    ok_cancel(ui, "Apply", &mut ok, &mut close);
                }
                Dialog::Speed { ratio, semis, mode } => {
                    // mode: 0 = stretch and pitch locked together (resample), 1 = independent
                    let cur_dur = self.cur().map(|f| match f.selection {
                        Some((a, b)) if b > a => b - a,
                        _ => f.data.duration(),
                    });
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Algorithm:").color(theme::text_dim()));
                        egui::ComboBox::from_id_salt("sp_algo").width(150.0).selected_text(if *mode == 0 { "Varispeed" } else { "High Quality" }).show_ui(ui, |ui| {
                            changed |= ui.selectable_value(mode, 1, "High Quality").changed();
                            changed |= ui.selectable_value(mode, 0, "Varispeed").changed();
                        });
                        ui.add_space(12.0);
                        ui.label(RichText::new("Precision:").color(theme::text_dim()));
                        let pid = ui.make_persistent_id("sp_prec");
                        let mut prec: usize = ui.data(|d| d.get_temp(pid)).unwrap_or(2);
                        egui::ComboBox::from_id_salt("sp_prec_c").width(90.0).selected_text(["Low", "Medium", "High"][prec.min(2)]).show_ui(ui, |ui| {
                            for (i, n) in ["Low", "Medium", "High"].iter().enumerate() {
                                ui.selectable_value(&mut prec, i, *n);
                            }
                        });
                        ui.data_mut(|d| d.insert_temp(pid, prec));
                    });
                    let mut stretch = 100.0 / ratio.max(0.01);
                    dsp::group(ui, "Duration", |ui| {
                        ui.horizontal(|ui| {
                            let cd = cur_dur.unwrap_or(0.0);
                            ui.label(RichText::new(format!("Current Duration:   {}", crate::audio::fmt_time(cd))).color(theme::text()));
                            ui.add_space(40.0);
                            ui.label(RichText::new("New Duration:").color(theme::text()));
                            ui.label(RichText::new(crate::audio::fmt_time(cd * stretch as f64 / 100.0)).color(theme::time_blue()));
                        });
                    });
                    dsp::group(ui, "Stretch and Pitch", |ui| {
                        let mut locked = *mode == 0;
                        if ui.checkbox(&mut locked, "Lock Stretch and Pitch Shift (Resample)").changed() {
                            *mode = if locked { 0 } else { 1 };
                            changed = true;
                        }
                        if dsp::scale_slider_labeled(ui, "Stretch", &mut stretch, 10.0..=1000.0, &[(200.0, "200"), (400.0, "400"), (600.0, "600"), (800.0, "800")], "%") {
                            *ratio = 100.0 / stretch.max(1.0);
                            if *mode == 0 {
                                *semis = (12.0 * ratio.log2() * 10.0).round() / 10.0;
                            }
                            changed = true;
                        }
                        let mut st = *semis;
                        if dsp::scale_slider_labeled(ui, "Pitch Shift", &mut st, -36.0..=36.0, &[(-20.0, "-20"), (0.0, "0"), (20.0, "20")], "semitones") {
                            *semis = (st * 10.0).round() / 10.0;
                            if *mode == 0 {
                                *ratio = 2f32.powf(*semis / 12.0);
                            }
                            changed = true;
                        }
                    });
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("Processes {scope}.  In: L, R | Out: L, R")).color(theme::text_dim()));
                    let previewing = self.preview_audio.is_some();
                    ui.horizontal(|ui| {
                        if theme::icon_button(ui, theme::Icon::Play, previewing, "Preview: play the result (loops)").clicked() {
                            if previewing {
                                self.stop_preview();
                            } else {
                                self.start_stretch_preview(*ratio, *semis, *mode);
                            }
                        }
                        ui.add_space(24.0);
                        ui.horizontal(|ui| {
                            if crate::theme::dialog_button(ui, "Close", false).clicked() {
                                close = true;
                            }
                            if crate::theme::dialog_button(ui, "Apply", true).clicked() {
                                ok = true;
                            }
                        });
                    });
                    // while previewing, a change is heard once the slider is let go
                    if previewing && changed && !ui.input(|i| i.pointer.any_down()) {
                        self.start_stretch_preview(*ratio, *semis, *mode);
                    } else if previewing && changed {
                        self.preview_dirty = true;
                    }
                    if previewing && self.preview_dirty && !ui.input(|i| i.pointer.any_down()) {
                        self.preview_dirty = false;
                        self.start_stretch_preview(*ratio, *semis, *mode);
                    }
                }
                Dialog::Loudness { target } => {
                    ui.label(format!("Match the loudness of {scope} to:"));
                    ui.add(crate::theme::RSlider::new(target, -40.0..=-6.0).suffix(" LUFS"));
                    ui.label(RichText::new("Approximate integrated loudness (ungated).").small().color(theme::text_dim()));
                    ok_cancel(ui, "Apply", &mut ok, &mut close);
                }
                Dialog::NoiseReduction { amount, reduce_db } => {
                    if self.noise_profile.is_none() {
                        ui.colored_label(Color32::from_rgb(235, 180, 80), "No noise print captured yet.");
                        ui.label("Select a part that contains only noise, then capture a noise print.");
                    } else {
                        ui.label(RichText::new("Noise print captured.").color(theme::wave()));
                    }
                    if ui.add_enabled(has_sel, egui::Button::new("Capture Noise Print from Selection")).clicked() {
                        if let Some(f) = self.cur() {
                            let (a, b) = f.range();
                            self.noise_profile = Some(dsp::capture_noise(&f.data, a, b));
                        }
                    }
                    ui.add(crate::theme::RSlider::new(amount, 0.0..=1.5).text("Noise Reduction"));
                    ui.add(crate::theme::RSlider::new(reduce_db, 1.0..=60.0).suffix(" dB").text("Reduce by"));
                    ui.label(RichText::new(format!("Processes {scope}. Select all (Ctrl+A) to clean the whole file.")).small().color(theme::text_dim()));
                    ui.separator();
                    ui.horizontal(|ui| {
                        if crate::theme::dialog_button(ui, "Cancel", false).clicked() {
                            close = true;
                        }
                        let can = self.noise_profile.is_some();
                        if ui.add_enabled_ui(can, |ui| crate::theme::dialog_button(ui, "Apply", true)).inner.clicked() && can {
                            ok = true;
                        }
                    });
                }
                Dialog::Tone { freq, wave, db, secs } => {
                    let before = (*freq, *wave, *db, *secs);
                    egui::Grid::new("tone").num_columns(2).show(ui, |ui| {
                        ui.label("Waveform");
                        ui.horizontal(|ui| {
                            ui.selectable_value(wave, Wave::Sine, "Sine");
                            ui.selectable_value(wave, Wave::Square, "Square");
                            ui.selectable_value(wave, Wave::Triangle, "Triangle");
                            ui.selectable_value(wave, Wave::Sawtooth, "Sawtooth");
                        });
                        ui.end_row();
                        ui.label("Frequency");
                        ui.add(crate::theme::RSlider::new(freq, 20.0..=20000.0).logarithmic(true).suffix(" Hz"));
                        ui.end_row();
                        ui.label("Volume");
                        ui.add(crate::theme::RSlider::new(db, -60.0..=0.0).suffix(" dB"));
                        ui.end_row();
                        ui.label("Duration");
                        ui.add(crate::theme::RSlider::new(secs, 0.1..=600.0).logarithmic(true).suffix(" s"));
                        ui.end_row();
                    });
                    ui.label(RichText::new(if self.cur().is_some() && self.mode == Mode::Waveform { "Inserted at the playhead of the current file." } else { "Creates a new file." }).small().color(theme::text_dim()));
                    let (f, w, d, s) = (*freq, *wave, *db, *secs);
                    let (rate, chs) = self.gen_format();
                    self.gen_preview_row(ui, before != (f, w, d, s), move || {
                        dsp::gen_tone(rate, chs, s.min(10.0), f, w, d)
                    });
                    ok_cancel(ui, "OK", &mut ok, &mut close);
                }
                Dialog::Noise { color, db, secs } => {
                    let before = (*color, *db, *secs);
                    egui::Grid::new("noise").num_columns(2).show(ui, |ui| {
                        ui.label("Color");
                        ui.horizontal(|ui| {
                            ui.selectable_value(color, NoiseColor::White, "White");
                            ui.selectable_value(color, NoiseColor::Pink, "Pink");
                            ui.selectable_value(color, NoiseColor::Brown, "Brown");
                        });
                        ui.end_row();
                        ui.label("Volume");
                        ui.add(crate::theme::RSlider::new(db, -60.0..=0.0).suffix(" dB"));
                        ui.end_row();
                        ui.label("Duration");
                        ui.add(crate::theme::RSlider::new(secs, 0.1..=600.0).logarithmic(true).suffix(" s"));
                        ui.end_row();
                    });
                    ui.label(RichText::new(if self.cur().is_some() && self.mode == Mode::Waveform { "Inserted at the playhead of the current file." } else { "Creates a new file." }).small().color(theme::text_dim()));
                    let (c, d, s) = (*color, *db, *secs);
                    let (rate, chs) = self.gen_format();
                    self.gen_preview_row(ui, before != (c, d, s), move || {
                        dsp::gen_noise(rate, chs, s.min(10.0), c, d)
                    });
                    ok_cancel(ui, "OK", &mut ok, &mut close);
                }
                Dialog::InsertSilence { secs } => {
                    ui.add(crate::theme::RSlider::new(secs, 0.01..=600.0).logarithmic(true).suffix(" s").text("Duration"));
                    ok_cancel(ui, "OK", &mut ok, &mut close);
                }
                Dialog::Convert { rate, channels, bits } => {
                    egui::Grid::new("conv").num_columns(2).show(ui, |ui| {
                        ui.label("Sample Rate");
                        egui::ComboBox::from_id_salt("cvr").selected_text(format!("{rate} Hz")).show_ui(ui, |ui| {
                            for r in RATES {
                                ui.selectable_value(rate, r, format!("{r} Hz"));
                            }
                        });
                        ui.end_row();
                        ui.label("Channels");
                        ui.horizontal(|ui| {
                            ui.selectable_value(channels, 1, "Mono");
                            ui.selectable_value(channels, 2, "Stereo");
                        });
                        ui.end_row();
                        ui.label("Bit Depth");
                        egui::ComboBox::from_id_salt("cvb").selected_text(if *bits == 32 { "32 (float)".to_string() } else { format!("{bits}") }).show_ui(ui, |ui| {
                            ui.selectable_value(bits, 8, "8");
                            ui.selectable_value(bits, 16, "16");
                            ui.selectable_value(bits, 24, "24");
                            ui.selectable_value(bits, 32, "32 (float)");
                        });
                        ui.end_row();
                    });
                    ok_cancel(ui, "OK", &mut ok, &mut close);
                }
                Dialog::InterpretSampleRate { rate } => {
                    let old = self.cur().map(|f| f.data.rate).unwrap_or(*rate);
                    ui.label("Changes the rate written in the file header; no samples are resampled.");
                    ui.horizontal(|ui| {
                        rlabel(ui, "Sample Rate:");
                        ui.add(egui::DragValue::new(rate).range(1000..=384000).speed(1.0).suffix(" Hz"));
                    });
                    ui.label(RichText::new(format!(
                        "Current: {old} Hz  •  duration × {:.3}  •  pitch × {:.3}",
                        old as f64 / (*rate).max(1) as f64,
                        *rate as f64 / old.max(1) as f64
                    )).small().color(theme::text_dim()));
                    ok_cancel(ui, "OK", &mut ok, &mut close);
                }
                Dialog::FrequencySplit { bands, cross } => {
                    ui.label(format!("Splits {scope} into separate files. Crossovers are the boundaries between bands."));
                    let mut b = (*bands).clamp(2, 4) as i32;
                    ui.add(crate::theme::RSlider::new(&mut b, 2..=4).text("Bands"));
                    *bands = b as usize;
                    let top = (rate * 0.45).max(21.0);
                    let names = ["Low / High", "Low / Mid / High", "Low / Low-Mid / High-Mid / High"];
                    for i in 0..bands.saturating_sub(1) {
                        ui.horizontal(|ui| {
                            rlabel(ui, &format!("Crossover {}:", i + 1));
                            ui.add(egui::DragValue::new(&mut cross[i]).range(20.0..=top).speed(1.0).suffix(" Hz"));
                        });
                    }
                    ui.label(RichText::new(names[bands.saturating_sub(2).min(2)]).small().color(theme::text_dim()));
                    ok_cancel(ui, "Split", &mut ok, &mut close);
                }
                Dialog::Speech { text, voice, rate, pitch, tab, tract } => {
                    ui.set_width(440.0);
                    ui.horizontal(|ui| {
                        ui.selectable_value(tab, 0, "System Voice");
                        ui.selectable_value(tab, 1, "Vocal Tract");
                    });
                    ui.separator();
                    if *tab == 0 {
                        ui.label("Type what AudioCraft should say.");
                        ui.add(egui::TextEdit::multiline(text).desired_rows(4).desired_width(420.0).hint_text("Hello, this is AudioCraft."));
                        ui.horizontal(|ui| {
                            rlabel(ui, "Voice:");
                            let shown = if voice.trim().is_empty() {
                                "System default".to_string()
                            } else if let Some(p) = crate::speech::preset(voice) {
                                p.0.to_string()
                            } else {
                                voice.clone()
                            };
                            egui::ComboBox::from_id_salt("speech_voice")
                                .selected_text(shown)
                                .width(240.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(voice, String::new(), "System default");
                                    ui.label(RichText::new("Ready voices").small().color(theme::text_dim()));
                                    for p in crate::speech::PRESETS {
                                        let id = format!("preset:{}", p.0);
                                        let label = if p.3 == 0.0 { p.0.to_string() } else { format!("{}  ({:+.0} st)", p.0, p.3) };
                                        ui.selectable_value(voice, id, label);
                                    }
                                    let installed = crate::speech::installed_voices();
                                    if !installed.is_empty() {
                                        ui.separator();
                                        ui.label(RichText::new("Installed on this computer").small().color(theme::text_dim()));
                                        for n in installed {
                                            ui.selectable_value(voice, n.clone(), n);
                                        }
                                    }
                                });
                        });
                        ui.add(crate::theme::RSlider::new(rate, -10..=10).text("Speaking rate"));
                        ui.add(crate::theme::RSlider::new(pitch, -4.5..=4.5).step_by(0.5).suffix(" st").text("Pitch"))
                            .on_hover_text("The speech engine's own pitch: the voice stays itself, only higher or lower (added to a ready voice's own pitch)");
                        ui.label(RichText::new("The Windows system voices (ready voices pick the closest installed one); on other systems espeak or flite if installed.").small().color(theme::text_dim()));
                    } else {
                        crate::tract_ui::tract_ui(ui, tract);
                    }
                    ui.separator();
                    let busy = self.extra.speech_job.is_some();
                    let previewing = self.speech_previewing();
                    if previewing {
                        // keep looking so the button turns back to Play when the speech ends
                        ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    }
                    ui.horizontal(|ui| {
                        if theme::icon_button(ui, if previewing { theme::Icon::Stop } else { theme::Icon::Play }, previewing, if previewing { "Stop the preview" } else { "Preview: hear it without adding a file" }).clicked() {
                            speech_action = if previewing { 3 } else { 1 };
                        }
                        if ui.add_enabled(!busy, egui::Button::new(RichText::new("Add as New File").strong())).on_hover_text("Adds the speech to the Files panel; this window stays open").clicked() {
                            speech_action = 2;
                        }
                        if crate::theme::dialog_button(ui, "Close", false).clicked() {
                            close = true;
                        }
                        if busy {
                            ui.spinner();
                        }
                    });
                    if !self.extra.speech_status.is_empty() {
                        ui.label(RichText::new(&self.extra.speech_status).small().color(theme::text_dim()));
                    }
                }
                Dialog::ColorSpace { path, why } => {
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    ui.label(RichText::new(&name).strong());
                    ui.label("This video is not in the standard Rec.709 colour space the preview shows:");
                    ui.label(RichText::new(why.as_str()).color(Color32::from_rgb(235, 180, 80)));
                    ui.label("Its picture will look washed out or too dark in the Video panel. The sound is not affected.");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if crate::theme::dialog_button(ui, "Convert Preview to Rec.709", true).clicked() {
                            color_convert = true;
                            close = true;
                        }
                        if crate::theme::dialog_button(ui, "Keep As Is", false).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Preferences => {
                    ui.heading("Audio Hardware");
                    self.audio_device_grid(ui);
                    ui.separator();
                    ui.heading("Playback");
                    ui.checkbox(&mut self.return_on_stop, "Return playhead to start position on stop");
                    ui.checkbox(&mut self.auto_scroll, "Auto-scroll during playback");
                    ui.separator();
                    if crate::theme::dialog_button(ui, "Close", false).clicked() {
                        close = true;
                        self.device_lists = None;
                    }
                }
                Dialog::Envelope { gain, points } => {
                    ui.label(format!("Shapes the level across {scope}. Drag points; double-click to add or remove."));
                    let yr = if *gain { (-40.0, 20.0) } else { (0.0, 1.0) };
                    crate::dsp::curve_editor(ui, points, (0.0, 1.0), yr, false);
                    ui.horizontal(|ui| {
                        if ui.button("Reset").clicked() {
                            *points = if *gain { vec![(0.0, 0.0), (1.0, 0.0)] } else { vec![(0.0, 1.0), (1.0, 1.0)] };
                        }
                        if !*gain {
                            if ui.button("Fade In").clicked() {
                                *points = vec![(0.0, 0.0), (1.0, 1.0)];
                            }
                            if ui.button("Fade Out").clicked() {
                                *points = vec![(0.0, 1.0), (1.0, 0.0)];
                            }
                            if ui.button("Swell").clicked() {
                                *points = vec![(0.0, 0.2), (0.5, 1.0), (1.0, 0.2)];
                            }
                        }
                    });
                    ok_cancel(ui, "Apply", &mut ok, &mut close);
                }
                Dialog::DeleteSilence { thr_db, min_s, keep_s } => {
                    ui.label(format!("Removes quiet passages in {scope}."));
                    ui.add(crate::theme::RSlider::new(thr_db, -90.0..=-10.0).suffix(" dB").text("Silence is below"));
                    ui.add(crate::theme::RSlider::new(min_s, 0.05..=10.0).logarithmic(true).suffix(" s").text("For at least"));
                    ui.add(crate::theme::RSlider::new(keep_s, 0.0..=2.0).suffix(" s").text("Keep of each pause"));
                    ok_cancel(ui, "Delete Silence", &mut ok, &mut close);
                }
                Dialog::MarkAudio { thr_db, min_s } => {
                    ui.label(format!("Adds a marker where sound starts after a pause, in {scope}."));
                    ui.add(crate::theme::RSlider::new(thr_db, -90.0..=-10.0).suffix(" dB").text("Silence is below"));
                    ui.add(crate::theme::RSlider::new(min_s, 0.05..=10.0).logarithmic(true).suffix(" s").text("Pause at least"));
                    ok_cancel(ui, "Mark", &mut ok, &mut close);
                }
                Dialog::PitchBender { start_st, end_st } => {
                    ui.label(format!("Bends pitch and speed together across {scope}."));
                    ui.add(crate::theme::RSlider::new(start_st, -24.0..=24.0).suffix(" st").text("Start"));
                    ui.add(crate::theme::RSlider::new(end_st, -24.0..=24.0).suffix(" st").text("End"));
                    ok_cancel(ui, "Apply", &mut ok, &mut close);
                }
                Dialog::Dtmf { digits, tone_ms, pause_ms, db } => {
                    let before = (digits.clone(), *tone_ms, *pause_ms, *db);
                    ui.horizontal(|ui| {
                        ui.label("Dial string");
                        ui.text_edit_singleline(digits);
                    });
                    ui.label(RichText::new("0-9 * # A-D; a comma is a longer pause.").small().color(theme::text_dim()));
                    ui.add(crate::theme::RSlider::new(tone_ms, 20.0..=1000.0).suffix(" ms").text("Tone length"));
                    ui.add(crate::theme::RSlider::new(pause_ms, 0.0..=1000.0).suffix(" ms").text("Pause"));
                    ui.add(crate::theme::RSlider::new(db, -60.0..=0.0).suffix(" dB").text("Volume"));
                    let now = (digits.clone(), *tone_ms, *pause_ms, *db);
                    let (rate, chs) = self.gen_format();
                    let changed = before != now;
                    self.gen_preview_row(ui, changed, move || {
                        crate::procs::dtmf(rate, chs, &now.0, now.1, now.2, now.3)
                    });
                    ok_cancel(ui, "OK", &mut ok, &mut close);
                }
                Dialog::ClickPop { sensitivity } => {
                    ui.label(format!("Finds clicks and pops in {scope} and redraws them smoothly."));
                    ui.add(crate::theme::RSlider::new(sensitivity, 0.05..=1.0).text("Sensitivity"));
                    ok_cancel(ui, "Apply", &mut ok, &mut close);
                }
                Dialog::PluginManager => {
                    if crate::plugins::hosting_disabled() {
                        ui.label(RichText::new("Plug-in scanning is turned off (AUDIOCRAFT_NO_PLUGIN_SCAN).").color(theme::text_dim()));
                    } else {
                        ui.horizontal(|ui| {
                            if ui.button("Scan for Plug-ins").clicked() {
                                self.hosted = Some(crate::plugins::scan_hosted(true));
                            }
                            ui.label(RichText::new("VST3 effects in the standard folders, AUDIOCRAFT_PLUGIN_PATH and the added folders below; CLAP from the standard folders.").small().color(theme::text_dim()));
                        });
                    }
                    // The person's own VST3 scan folders: a stack they can grow or trim, like Blender's asset folders.
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Scan folders").strong());
                        if ui.button("Add Folder...").clicked() {
                            if let Some(d) = rfd::FileDialog::new().ask_folder() {
                                let s = d.to_string_lossy().to_string();
                                if !self.prefs.vst_scan_folders.iter().any(|p| p.to_string_lossy() == s) {
                                    self.prefs.vst_scan_folders.push(std::path::PathBuf::from(&s));
                                    crate::plugins::set_scan_folds(&self.prefs.vst_scan_folders);
                                    self.hosted = Some(crate::plugins::scan_hosted(true));
                                    crate::prefs::save(&self.prefs);
                                }
                            }
                        }
                    });
                    let mut remove: Option<usize> = None;
                    egui::ScrollArea::vertical().max_height(90.0).show(ui, |ui| {
                        for (i, f) in self.prefs.vst_scan_folders.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(f.to_string_lossy());
                                if ui.small_button("x").on_hover_text("Remove this folder from the scan list").clicked() {
                                    remove = Some(i);
                                }
                            });
                        }
                    });
                    if let Some(i) = remove {
                        self.prefs.vst_scan_folders.remove(i);
                        crate::plugins::set_scan_folds(&self.prefs.vst_scan_folders);
                        self.hosted = Some(crate::plugins::scan_hosted(true));
                        crate::prefs::save(&self.prefs);
                    }
                    if self.prefs.vst_scan_folders.is_empty() {
                        ui.label(RichText::new("No extra folders: scanning uses the standard folders.").small().color(theme::text_dim()));
                    }
                    ui.label(RichText::new("VST 2 plug-ins are not supported; use their VST3 or CLAP versions.").small().color(theme::text_dim()));
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        egui::Grid::new("plugman").striped(true).num_columns(3).show(ui, |ui| {
                            ui.label(RichText::new("Name").strong());
                            ui.label(RichText::new("Vendor").strong());
                            ui.label(RichText::new("Format").strong());
                            ui.end_row();
                            for e in crate::plugins::builtin_list().iter().chain(self.hosted.clone().unwrap_or_default().iter()) {
                                ui.label(&e.name);
                                ui.label(&e.vendor);
                                ui.label(e.format);
                                ui.end_row();
                            }
                        });
                    });
                    if crate::theme::dialog_button(ui, "Close", false).clicked() {
                        close = true;
                    }
                }
                Dialog::About => {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("AudioCraft").size(26.0).color(theme::accent()).strong());
                        ui.label("Audio editor and multitrack mixer");
                        ui.label(RichText::new("Version 1.0 \u{2022} Built with Rust and egui").small().color(theme::text_dim()));
                        ui.add_space(8.0);
                        ui.label(RichText::new("Waveform editing \u{2022} Multitrack \u{2022} Mixer \u{2022} Effects \u{2022} Spectral display \u{2022} Recording").small());
                    });
                    if crate::theme::dialog_button(ui, "Close", false).clicked() {
                        close = true;
                    }
                }
                Dialog::Shortcuts => {
                    egui::Grid::new("keys").striped(true).num_columns(2).show(ui, |ui| {
                        for (k, d) in [
                            ("Space", "Play / Stop"),
                            ("Shift+Space", "Record"),
                            ("Home / End", "Playhead to start / end"),
                            ("8 / 9", "Waveform / Multitrack editor"),
                            ("Ctrl+Z / Ctrl+Shift+Z", "Undo / Redo"),
                            ("Ctrl+X / C / V", "Cut / Copy / Paste"),
                            ("Ctrl+Shift+V", "Mix Paste"),
                            ("Ctrl+T", "Crop to selection"),
                            ("Delete", "Delete selection or clip"),
                            ("Ctrl+A", "Select all"),
                            ("M", "Add marker"),
                            ("= / - / \\", "Zoom in / out / full"),
                            ("Mouse wheel", "Zoom around the pointer"),
                            ("V / R / Y / T", "Move / Razor / Slip / Time Selection tool"),
                            ("Ctrl+K", "Split clips at playhead"),
                            ("Shift+D", "Spectral frequency display"),
                            ("Shift+E", "Essential Sound panel"),
                            ("Ctrl+N / Ctrl+Shift+N", "New file / New session"),
                            ("Ctrl+O / Ctrl+S", "Open / Save"),
                        ] {
                            ui.label(RichText::new(k).monospace().color(theme::time_blue()));
                            ui.label(d);
                            ui.end_row();
                        }
                    });
                    if crate::theme::dialog_button(ui, "Close", false).clicked() {
                        close = true;
                    }
                }
                Dialog::Export(o) => {
                    if self.export_ui(ui, o, &mut close) {
                        ok = true;
                    }
                }
                Dialog::Doppler(p) => {
                    ui.label(format!("Makes {scope} sound like it moves past you: the pitch rises as it comes closer and falls as it goes away."));
                    App::doppler_ui(ui, p);
                    ok_cancel(ui, "Apply", &mut ok, &mut close);
                }
                Dialog::SoundRemover { reduce_db, strength } => {
                    if self.sound_model.is_none() {
                        ui.colored_label(Color32::from_rgb(235, 180, 80), "No sound model learned yet.");
                        ui.label("Select a part where only the sound to remove plays, then learn it.");
                    } else {
                        ui.label(RichText::new("Sound model learned.").color(theme::wave()));
                    }
                    if ui.add_enabled(has_sel, egui::Button::new("Learn Sound Model from Selection")).clicked() {
                        if let Some(f) = self.cur() {
                            let (a, b) = f.range();
                            self.sound_model = Some(dsp::capture_noise(&f.data, a, b));
                        }
                    }
                    ui.add(crate::theme::RSlider::new(strength, 0.1..=1.0).text("Strength"));
                    ui.add(crate::theme::RSlider::new(reduce_db, 3.0..=60.0).suffix(" dB").text("Reduce by"));
                    ui.label(RichText::new(format!("Removes the learned sound from {scope}. Clear the selection first to clean the whole file.")).small().color(theme::text_dim()));
                    let can = self.sound_model.is_some();
                    ui.add_enabled_ui(can, |ui| ok_cancel(ui, "Apply", &mut ok, &mut close));
                }
                Dialog::Message(_, body) => {
                    ui.label(body.as_str());
                    ui.add_space(6.0);
                    if crate::theme::dialog_button(ui, "OK", true).clicked() {
                        close = true;
                    }
                }
            });
        if run_now {
            if let Dialog::Batch {
                inputs,
                preset,
                out_dir,
                suffix,
                result,
            } = &mut dlg
            {
                let paths: Vec<std::path::PathBuf> =
                    inputs.iter().map(std::path::PathBuf::from).collect();
                let steps = crate::commands::batch_preset_steps(*preset);
                result.clear();
                // runs on a worker: the window keeps drawing and shows each file's progress
                self.batch_start(
                    paths,
                    steps,
                    std::path::PathBuf::from(out_dir.as_str()),
                    suffix.clone(),
                );
            }
        }
        if !ok && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if speech_action != 0 {
            if let Dialog::Speech {
                text,
                voice,
                rate,
                pitch,
                tab,
                tract,
            } = &dlg
            {
                match (speech_action, *tab) {
                    (3, _) => self.speech_preview_stop(),
                    (a, 0) => {
                        let (t, v, r, pt) = (text.clone(), voice.clone(), *rate, *pitch);
                        self.speech_start(&t, &v, r, pt, a);
                    }
                    (a, _) => match crate::tract::render(tract, 48000) {
                        Ok(d) => self.speech_finish(std::sync::Arc::new(d), a),
                        Err(e) => self.extra.speech_status = e,
                    },
                }
            }
        }
        if matches!(dlg, Dialog::Speech { .. }) && (!open || close) {
            self.speech_preview_stop();
        }
        if color_convert {
            if let Err(e) = self.convert_video_color() {
                self.status = e;
            }
        }
        let generator = matches!(dlg, Dialog::Tone { .. } | Dialog::Noise { .. } | Dialog::Dtmf { .. });
        if ok && generator {
            self.stop_preview();
        }
        if ok {
            self.dialog_commit(&dlg);
            close = true;
        }
        if (matches!(dlg, Dialog::Speed { .. }) || generator) && (!open || close || ok) {
            self.stop_preview();
        }
        if matches!(dlg, Dialog::Effect { .. }) && (!open || close) && !ok {
            if self.engine.lock().preview.take().is_some() {
                self.stop();
                self.engine.lock().looping = false;
            }
        }
        if open && !close {
            self.dialog = Some(dlg);
        }
    }

    pub(crate) fn dialog_commit(&mut self, dlg: &Dialog) {
        match dlg {
            Dialog::NewWorkspace { name } => self.ws.save_as(name),
            Dialog::EditWorkspaces => {}
            Dialog::RenameMarker { index, name } => {
                if self.mode == Mode::Multitrack {
                    if let Some(s) = self.session.as_mut() {
                        if let Some(m) = s.markers.get_mut(*index) {
                            m.name = name.clone();
                            s.touch();
                        }
                    }
                    self.session_push("Rename Marker");
                } else if let Some(f) = self.cur_mut() {
                    if let Some(m) = f.markers.get_mut(*index) {
                        m.name = name.clone();
                    }
                }
            }
            Dialog::RenameTrack { index, name } => {
                if let Some(s) = self.session.as_mut() {
                    if let Some(t) = s.tracks.get_mut(*index) {
                        t.name = name.clone();
                        s.touch();
                    }
                }
                self.session_push("Rename Track");
            }
            Dialog::Gain { db } => {
                let g = *db;
                self.process("Gain", move |d, a, b| {
                    dsp::apply_chain(
                        d,
                        a,
                        b,
                        &[EffectSlot::new(dsp::Effect::Amplify { gain_db: g })],
                    )
                });
            }
            // Batch runs from its own Run button; OK just leaves the results on screen.
            Dialog::Batch { .. } => {}
            Dialog::TrackColor { track, color } => {
                if let Some(s) = self.session.as_mut() {
                    if let Some(t) = s.tracks.iter_mut().find(|t| t.id == *track) {
                        t.color = *color;
                    }
                    s.touch();
                }
                self.session_push("Track Color");
            }
            Dialog::RateMismatch {
                file_id,
                track,
                at,
                choice,
            } => {
                let srate = self.session.as_ref().map(|s| s.rate).unwrap_or(48000);
                match choice {
                    1 => {
                        let sbits = self.session.as_ref().map(|s| s.bits).unwrap_or(32);
                        if let Some(f) = self.file_mut(*file_id) {
                            if f.data.rate != srate {
                                let ch = f.data.channels();
                                let d = f.data.conform(srate, ch);
                                f.push("Convert Sample Rate", d);
                            }
                            f.bits = sbits;
                            f.float = sbits == 32;
                        }
                        self.add_clip_at(*file_id, *track, *at);
                    }
                    2 => {
                        let (frate, fbits) = self
                            .file(*file_id)
                            .map(|f| (f.data.rate, if f.float { 32 } else { f.bits }))
                            .unwrap_or((srate, 32));
                        if let Some(s) = self.session.as_mut() {
                            s.rate = frate;
                            s.bits = fbits;
                            s.touch();
                        }
                        self.add_clip_at(*file_id, *track, *at);
                        self.status = format!("The session is now {frate} Hz, {fbits}-bit");
                    }
                    3 if self
                        .file(*file_id)
                        .map(|f| f.data.rate == srate)
                        .unwrap_or(true) =>
                    {
                        self.add_clip_at(*file_id, *track, *at);
                    }
                    3 => {
                        let frate =
                            self.file(*file_id).map(|f| f.data.rate).unwrap_or(srate) as f64;
                        self.add_clip_at(*file_id, *track, *at);
                        // the file's samples played at the session's rate: faster or slower
                        if let Some(s) = self.session.as_mut() {
                            if let Some(id) = s.selected_clip {
                                if let Some(c) = s.clip_mut(id) {
                                    let speed = (srate as f64 / frate.max(1.0)).clamp(0.1, 10.0);
                                    c.len = c.len / speed;
                                    c.speed = speed;
                                }
                            }
                        }
                        self.session_push("Keep Sample Rate");
                    }
                    _ => {}
                }
            }
            Dialog::NewFile {
                name,
                rate,
                channels,
                bits,
                make_default,
            } => {
                let d = AudioData::new(*rate, (*channels).clamp(1, 32), 0);
                let nm = if name.to_lowercase().ends_with(".wav") {
                    name.clone()
                } else {
                    format!("{name}.wav")
                };
                self.stop();
                // making a new file starts the Files panel: the other open files ("imports") are closed
                let used: Vec<u64> = self
                    .session
                    .as_ref()
                    .map(|s| {
                        s.tracks
                            .iter()
                            .flat_map(|t| t.clips.iter().map(|c| c.file_id))
                            .collect()
                    })
                    .unwrap_or_default();
                for id in self.files.iter().map(|f| f.id).collect::<Vec<_>>() {
                    if !used.contains(&id) {
                        self.files.retain(|f| f.id != id);
                    }
                }
                self.add_file(nm, None, d, *bits, *bits == 32, "New File");
                self.mode = Mode::Waveform;
                self.center_tab = CenterTab::Editor;
                if *make_default {
                    // "Make this the default path": pin the current default save location.
                    let dir = self
                        .default_save_dir()
                        .unwrap_or_else(|| crate::dialogs::default_session_folder().into());
                    self.prefs.default_save_mode = 0;
                    self.prefs.default_save_folder = dir.to_string_lossy().to_string();
                    crate::prefs::save(&self.prefs);
                }
            }
            Dialog::ConfirmReplace { id, path } => {
                let Some(f) = self.file(*id) else { return };
                let (bits, float, bwf, meta, data) =
                    (f.bits, f.float, f.bwf, f.meta.clone(), f.data.clone());
                self.status = format!("Saving {}...", path.display());
                match crate::audio::save_with_meta(path, &data, bits, float, bwf, &meta) {
                    Ok(()) => {
                        if let Some(f) = self.file_mut(*id) {
                            f.path = Some(path.clone());
                            f.name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            f.saved_pos = Some(f.hist_pos);
                        }
                        self.note_saved(path);
                        self.status = format!("Saved {}", path.display());
                        self.add_recent(path);
                    }
                    Err(e) => self.message("Could not save", &e),
                }
            }
            Dialog::NewSession {
                name,
                rate,
                tracks,
                folder,
                bits,
                mix,
                make_default,
            } => {
                let Some((dir, path)) = crate::dialogs::session_paths(folder, name) else {
                    self.status = "Give the session a name first".into();
                    return;
                };
                self.stop();
                let count = (*tracks).clamp(1, 64);
                let stem = crate::dialogs::session_name_stem(name)
                    .unwrap_or_else(|| "Untitled Session".into());
                let mut s = Session::new(&stem, *rate, count);
                s.bits = *bits;
                s.mix = (*mix).min(2);
                if s.mix == 0 {
                    for t in s.tracks.iter_mut() {
                        t.stereo = false;
                    }
                }
                // like the original, the session gets its own folder and is saved there at once
                match std::fs::create_dir_all(&dir) {
                    Ok(()) => {
                        s.path = Some(path.clone());
                        self.session_hist = vec![("New Session".into(), s.hist_state())];
                        self.session_hist_pos = 0;
                        self.session = Some(s);
                        self.mode = Mode::Multitrack;
                        self.center_tab = CenterTab::Editor;
                        self.save_session(false);
                        self.status = format!("Created session '{}' with {} tracks", stem, count);
                    }
                    Err(e) => {
                        // keep the session in memory, but tell the person why it was not saved
                        self.session_hist = vec![("New Session".into(), s.hist_state())];
                        self.session_hist_pos = 0;
                        self.session = Some(s);
                        self.mode = Mode::Multitrack;
                        self.center_tab = CenterTab::Editor;
                        self.status = format!(
                            "Could not create {} ({e}); save the session later with File > Save Session",
                            dir.display()
                        );
                    }
                }
                if *make_default {
                    // "Make this the default path": the folder shown in the popup becomes the default.
                    self.prefs.default_save_mode = 0;
                    self.prefs.default_save_folder = folder.clone();
                    crate::prefs::save(&self.prefs);
                }
            }
            Dialog::Effect { effect, .. } => {
                let slot = EffectSlot::new(effect.clone());
                if self.engine.lock().preview.take().is_some() {
                    self.stop();
                    self.engine.lock().looping = false;
                }
                self.process(effect.name(), move |d, a, b| {
                    dsp::apply_chain(d, a, b, &[slot])
                });
            }
            Dialog::Normalize { db } => {
                let db = *db;
                self.process("Normalize", move |d, a, b| dsp::normalize(d, a, b, db));
            }
            Dialog::Fade { fade_in, curve } => {
                let (fi, c) = (*fade_in, *curve);
                self.process(if fi { "Fade In" } else { "Fade Out" }, move |d, a, b| {
                    dsp::fade(d, a, b, fi, c)
                });
            }
            Dialog::Speed { ratio, semis, mode } => {
                let r = *ratio as f64;
                let st = *semis as f64;
                let m = *mode;
                let sel = self.cur().and_then(|f| f.selection);
                self.stop_preview();
                self.stop();
                self.transform("Stretch and Pitch", move |d, a, b| {
                    stretch_and_pitch(d, a, b, r, st, m)
                });
                if (r - 1.0).abs() > 1e-6 {
                    if let (Some((a, b)), Some(f)) = (sel, self.cur_mut()) {
                        f.selection = Some((a, a + (b - a) / r));
                    }
                }
                if let Some(f) = self.cur_mut() {
                    let dur = f.data.duration().max(0.001);
                    if f.view.1 > dur {
                        f.view = (0.0, dur);
                    }
                }
            }
            Dialog::Loudness { target } => {
                let t = *target;
                self.process("Match Loudness", move |d, a, b| {
                    dsp::match_loudness(d, a, b, t)
                });
            }
            Dialog::NoiseReduction { amount, reduce_db } => {
                if let Some(p) = self.noise_profile.clone() {
                    let (am, rd) = (*amount, *reduce_db);
                    self.process("Noise Reduction", move |d, a, b| {
                        dsp::noise_reduce(d, a, b, &p, am, rd)
                    });
                }
            }
            Dialog::Tone {
                freq,
                wave,
                db,
                secs,
            } => {
                let (rate, chs) = self.gen_format();
                let t = dsp::gen_tone(rate, chs, *secs, *freq, *wave, *db);
                self.insert_generated("Generate Tones", t);
            }
            Dialog::Noise { color, db, secs } => {
                let (rate, chs) = self.gen_format();
                let t = dsp::gen_noise(rate, chs, *secs, *color, *db);
                self.insert_generated("Generate Noise", t);
            }
            Dialog::InsertSilence { secs } => {
                let (rate, chs) = self
                    .cur()
                    .map(|f| (f.data.rate, f.data.channels()))
                    .unwrap_or((48000, 2));
                let t = AudioData::new(rate, chs, (*secs * rate as f32) as usize);
                self.insert_generated("Insert Silence", t);
            }
            Dialog::Envelope { gain, points } => {
                let (g, p) = (*gain, points.clone());
                self.process(
                    if g { "Gain Envelope" } else { "Fade Envelope" },
                    move |d, a, b| crate::procs::envelope(d, a, b, &p, g),
                );
            }
            Dialog::DeleteSilence {
                thr_db,
                min_s,
                keep_s,
            } => {
                let (t, m, k) = (*thr_db, *min_s, *keep_s);
                self.stop();
                let mut removed = 0;
                if let Some(f) = self.cur_mut() {
                    let (a, b) = f.range();
                    let (d, r) = crate::procs::delete_silence(&f.data, a, b, t, m, k);
                    removed = r;
                    if r > 0 {
                        f.selection = None;
                        f.push("Delete Silence", d);
                    }
                }
                let secs = removed as f64 / self.rate() as f64;
                self.status = format!("Delete Silence removed {secs:.2} s");
            }
            Dialog::MarkAudio { thr_db, min_s } => {
                let (t, m) = (*thr_db, *min_s);
                if let Some(f) = self.cur_mut() {
                    let (a, b) = f.range();
                    let marks = crate::procs::mark_audio(&f.data, a, b, t, m);
                    let n0 = f.markers.len();
                    for (i, tm) in marks.iter().enumerate() {
                        f.markers.push(crate::session::Marker {
                            name: format!("Audio {:02}", n0 + i + 1),
                            time: *tm,
                            duration: 0.0,
                        });
                    }
                    f.markers.sort_by(|a, b| a.time.total_cmp(&b.time));
                    self.status = format!("Mark Audio added {} markers", marks.len());
                }
            }
            Dialog::Export(o) => self.export_commit(o),
            Dialog::Doppler(p) => {
                let p = p.clone();
                self.stop();
                self.process("Doppler Shifter", move |d, a, b| {
                    crate::export::doppler(d, a, b, &p)
                });
            }
            Dialog::SoundRemover {
                reduce_db,
                strength,
            } => {
                if let Some(m) = self.sound_model.clone() {
                    let (st, rd) = (*strength, *reduce_db);
                    self.process("Sound Remover", move |d, a, b| {
                        dsp::noise_reduce(d, a, b, &m, st, rd)
                    });
                }
            }
            Dialog::PitchBender { start_st, end_st } => {
                let (s, e) = (*start_st, *end_st);
                self.stop();
                self.transform("Pitch Bender", move |d, a, b| {
                    crate::procs::pitch_bend(d, a, b, s, e)
                });
            }
            Dialog::Dtmf {
                digits,
                tone_ms,
                pause_ms,
                db,
            } => {
                let (rate, chs) = self.gen_format();
                let t = crate::procs::dtmf(rate, chs, digits, *tone_ms, *pause_ms, *db);
                self.insert_generated("Generate DTMF Tones", t);
            }
            Dialog::ClickPop { sensitivity } => {
                let s = *sensitivity;
                self.process("Click/Pop Eliminator", move |d, a, b| {
                    for c in d.ch.iter_mut() {
                        let bb = b.min(c.len());
                        if a < bb {
                            crate::fx2::declick(&mut c[a..bb], s);
                        }
                    }
                });
            }
            Dialog::Convert {
                rate,
                channels,
                bits,
            } => {
                let (r, c, b) = (*rate, *channels, *bits);
                self.stop();
                if let Some(f) = self.cur_mut() {
                    let d = f.data.conform(r, c);
                    f.bits = b;
                    f.float = b == 32;
                    f.selection = None;
                    f.cursor = 0.0;
                    f.push("Convert Sample Type", d);
                    f.view = (0.0, f.data.duration().max(0.001));
                }
            }
            Dialog::InterpretSampleRate { rate } => {
                if let Err(e) = self.interpret_sample_rate(*rate) {
                    self.message("Interpret Sample Rate", &e);
                }
            }
            Dialog::FrequencySplit { bands, cross } => {
                let n = (*bands).clamp(2, 4);
                if let Err(e) = self.split_frequency_bands(n, &cross[..n - 1]) {
                    self.message("Frequency Band Splitter", &e);
                }
            }
            _ => {}
        }
    }

    /// The sample rate and channels a generator makes: the current file's in the Waveform
    /// Editor, otherwise the session's rate (or 48 kHz) in stereo.
    pub fn gen_format(&self) -> (u32, usize) {
        match (self.mode, self.cur()) {
            (Mode::Waveform, Some(f)) => (f.data.rate, f.data.channels()),
            _ => (self.session.as_ref().map(|s| s.rate).unwrap_or(48000), 2),
        }
    }

    /// A generated sound as a new file whose history starts empty: Ctrl+Z takes the sound
    /// back (leaving the empty file) and Ctrl+Shift+Z brings it again.
    pub fn new_generated_file(&mut self, name: String, label: &str, t: AudioData) {
        self.stop();
        let empty = AudioData::new(t.rate, t.channels(), 0);
        self.add_file(name, None, empty, 32, true, "New File");
        self.switch_mode(Mode::Waveform);
        if let Some(f) = self.cur_mut() {
            f.cursor = 0.0;
            f.push(label, t);
            f.selection = None;
            f.view = (0.0, f.data.duration().max(0.001));
        }
    }

    fn insert_generated(&mut self, label: &str, t: AudioData) {
        if self.cur().is_none() || self.mode != Mode::Waveform {
            self.new_generated_file(format!("{label}.wav"), label, t);
            return;
        }
        self.stop();
        let Some(f) = self.cur_mut() else { return };
        let r = f.data.rate as f64;
        let at = (f.cursor * r) as usize;
        let len = t.duration();
        let d = f.data.inserted(at, &t);
        let st = f.cursor;
        let was_empty = f.data.frames() == 0;
        f.push(label, d);
        f.selection = Some((st, st + len));
        if was_empty || f.view.1 - f.view.0 < 0.01 {
            f.view = (0.0, f.data.duration().max(0.001));
        }
    }

    pub fn analysis_windows(&mut self, ctx: &egui::Context) {
        if self.panels.stats {
            let mut open = true;
            egui::Window::new("Amplitude Statistics")
                .open(&mut open)
                .default_width(380.0)
                .show(ctx, |ui| self.amp_stats_ui(ui));
            self.panels.stats = open;
        }
        if self.panels.freq {
            let mut open = true;
            egui::Window::new("Frequency Analysis")
                .open(&mut open)
                .default_size([520.0, 300.0])
                .show(ctx, |ui| self.freq_analysis_ui(ui));
            self.panels.freq = open;
        }
    }

    /// Amplitude Statistics of the selection (or whole file), as a window or a docked panel.
    pub fn amp_stats_ui(&mut self, ui: &mut egui::Ui) {
        {
            {
                let Some((data, (a, b), sel)) = self
                    .cur()
                    .map(|f| (f.data.clone(), f.range(), f.selection.is_some()))
                else {
                    ui.label("Open a file to analyze.");
                    return;
                };
                let key = (std::sync::Arc::as_ptr(&data) as usize, a, b);
                if self
                    .stats_cache
                    .as_ref()
                    .map(|c| c.0 != key)
                    .unwrap_or(true)
                {
                    let rows = dsp::stats(&data, a, b);
                    self.stats_cache = Some((key, rows, data.channels()));
                }
                let (_, rows, chs) = self.stats_cache.as_ref().unwrap();
                ui.label(
                    RichText::new(if sel { "Selection" } else { "Entire file" })
                        .small()
                        .color(theme::text_dim()),
                );
                egui::Grid::new("stats")
                    .striped(true)
                    .num_columns(chs + 1)
                    .show(ui, |ui| {
                        ui.label("");
                        for c in 0..*chs {
                            ui.label(
                                RichText::new(if *chs == 2 {
                                    ["Left", "Right"][c].to_string()
                                } else {
                                    format!("Channel {}", c + 1)
                                })
                                .strong(),
                            );
                        }
                        ui.end_row();
                        for (name, vals) in rows {
                            ui.label(name);
                            for v in vals {
                                ui.label(RichText::new(v).monospace());
                            }
                            ui.end_row();
                        }
                    });
            }
        }
    }

    /// Frequency Analysis around the playhead (or of the selection), as a window or a panel.
    pub fn freq_analysis_ui(&mut self, ui: &mut egui::Ui) {
        {
            let playing = self.is_playing();
            {
                let n = self.freq_size;
                let Some((data, (a, b))) = self.cur().map(|f| {
                    let r = match f.selection {
                        Some(_) if !playing => f.range(),
                        _ => {
                            let c = (f.cursor * f.data.rate as f64) as usize;
                            (c.saturating_sub(n / 2), (c + n / 2).min(f.data.frames()))
                        }
                    };
                    (f.data.clone(), r)
                }) else {
                    ui.label("Open a file to analyze.");
                    return;
                };
                let key = (std::sync::Arc::as_ptr(&data) as usize, a, b + n);
                if self.freq_cache.0 != key {
                    let spec: Vec<Vec<f32>> = data
                        .ch
                        .iter()
                        .take(2)
                        .map(|c| dsp::average_spectrum(c, a, b.max(a + n), n))
                        .collect();
                    self.freq_cache = (key, spec);
                }
                let rate = data.rate as f32;
                ui.horizontal(|ui| {
                    ui.label("FFT Size");
                    for s in [1024usize, 2048, 4096, 8192, 16384] {
                        ui.selectable_value(&mut self.freq_size, s, format!("{s}"));
                    }
                });
                let (resp, p) = ui.allocate_painter(
                    vec2(ui.available_width(), ui.available_height().max(180.0)),
                    Sense::hover(),
                );
                let r = resp.rect;
                p.rect_filled(r, 0.0, theme::wave_bg());
                let fx = |hz: f32| {
                    r.left()
                        + ((hz / 20.0).max(1.0).log10() / (rate / 2.0 / 20.0).log10()) * r.width()
                };
                let dy = |d: f32| r.top() + (-d / 120.0).clamp(0.0, 1.0) * r.height();
                for hz in [
                    50.0f32, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0,
                ] {
                    if hz < rate / 2.0 {
                        let x = fx(hz);
                        p.line_segment(
                            [pos2(x, r.top()), pos2(x, r.bottom())],
                            Stroke::new(1.0, crate::theme::gray(40)),
                        );
                        let l = if hz >= 1000.0 {
                            format!("{}k", hz / 1000.0)
                        } else {
                            format!("{hz}")
                        };
                        p.text(
                            pos2(x + 2.0, r.bottom() - 2.0),
                            egui::Align2::LEFT_BOTTOM,
                            l,
                            FontId::proportional(9.0),
                            theme::text_dim(),
                        );
                    }
                }
                for d in [-20.0f32, -40.0, -60.0, -80.0, -100.0] {
                    let y = dy(d);
                    p.line_segment(
                        [pos2(r.left(), y), pos2(r.right(), y)],
                        Stroke::new(1.0, crate::theme::gray(40)),
                    );
                    p.text(
                        pos2(r.left() + 2.0, y),
                        egui::Align2::LEFT_BOTTOM,
                        format!("{d}"),
                        FontId::proportional(9.0),
                        theme::text_dim(),
                    );
                }
                let cols = [theme::wave(), Color32::from_rgb(90, 160, 255)];
                for (ci, spec) in self.freq_cache.1.iter().enumerate() {
                    let bins = spec.len().max(1);
                    let mut pts = vec![];
                    for (k, v) in spec.iter().enumerate().skip(1) {
                        let hz = k as f32 * rate / 2.0 / bins as f32;
                        if hz < 20.0 {
                            continue;
                        }
                        pts.push(pos2(fx(hz), dy(*v)));
                    }
                    p.add(egui::Shape::line(pts, Stroke::new(1.3, cols[ci % 2])));
                }
                if let Some(hp) = resp.hover_pos() {
                    let t = ((hp.x - r.left()) / r.width()).clamp(0.0, 1.0);
                    let hz = 20.0 * (rate / 2.0 / 20.0).powf(t);
                    p.text(
                        pos2(r.right() - 6.0, r.top() + 4.0),
                        egui::Align2::RIGHT_TOP,
                        format!("{hz:.0} Hz"),
                        FontId::monospace(11.0),
                        Color32::WHITE,
                    );
                }
                if playing {
                    ui.ctx().request_repaint();
                }
            }
        }
    }
}
