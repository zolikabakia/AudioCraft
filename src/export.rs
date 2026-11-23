//! Export: the Export Audio File / Export Multitrack Mixdown dialog (name, location, format,
//! sample type, metadata), Application Settings export/import, and two more effects of the
//! original's Effects menu: Doppler Shifter (process) and Sound Remover (process).

use crate::app::{App, Dialog};
use crate::audio::{self, AudioData};
use crate::sysui::Ask;
use crate::theme;
use egui::RichText;
use std::path::{Path, PathBuf};

/// What an export writes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExportSource {
    File(u64),
    /// The session mixdown: the whole session, or only the time selection.
    Mixdown {
        selection: bool,
    },
}

/// (menu name, extension) of each export format.
pub const FORMATS: &[(&str, &str)] = &[
    ("Wave PCM (*.wav)", "wav"),
    ("AIFF (*.aif)", "aif"),
    ("FLAC (*.flac)", "flac"),
    ("MP3 Audio (*.mp3)", "mp3"),
    ("AAC (*.m4a)", "m4a"),
];
pub const RATES: &[u32] = &[
    8000, 11025, 16000, 22050, 32000, 44100, 48000, 88200, 96000, 176400, 192000,
];
/// (label, bits, float)
pub const SAMPLE_TYPES: &[(&str, u16, bool)] = &[
    ("8-bit", 8, false),
    ("16-bit", 16, false),
    ("24-bit", 24, false),
    ("32-bit (integer)", 32, false),
    ("32-bit (float)", 32, true),
];

/// The export dialog's fields.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportOpts {
    pub src: ExportSource,
    pub name: String,
    pub folder: String,
    pub format: usize,
    pub rate: u32,
    pub channels: usize,
    pub sample_type: usize,
    pub meta: bool,
    /// Lossy formats: kbit/s.
    pub kbps: u32,
}

impl ExportOpts {
    pub fn ext(&self) -> &'static str {
        FORMATS.get(self.format).map(|f| f.1).unwrap_or("wav")
    }
    pub fn path(&self) -> PathBuf {
        let stem = Path::new(self.name.trim())
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        Path::new(&self.folder).join(format!(
            "{}.{}",
            if stem.is_empty() { "Untitled" } else { &stem },
            self.ext()
        ))
    }
    pub fn bits_float(&self) -> (u16, bool) {
        let (_, b, f) = SAMPLE_TYPES
            .get(self.sample_type)
            .copied()
            .unwrap_or(("", 24, false));
        // AIFF and FLAC store integers only; MP3/AAC are encoded from 24-bit
        match self.ext() {
            "wav" => (b, f),
            "mp3" | "m4a" => (24, false),
            _ => (if f { 24 } else { b.max(16).min(24) }, false),
        }
    }
    /// Uncompressed size estimate in bytes for `secs` seconds (lossy: from the bit rate).
    pub fn estimate(&self, secs: f64) -> u64 {
        match self.ext() {
            "mp3" | "m4a" => (secs * self.kbps as f64 * 1000.0 / 8.0) as u64,
            "flac" => {
                (secs * self.rate as f64 * self.channels as f64 * self.bits_float().0 as f64 / 8.0
                    * 0.6) as u64
            }
            _ => {
                (secs * self.rate as f64 * self.channels as f64 * self.bits_float().0 as f64 / 8.0)
                    as u64
                    + 44
            }
        }
    }
}

pub fn human_size(b: u64) -> String {
    if b >= 1 << 30 {
        format!("{:.2} GB", b as f64 / (1u64 << 30) as f64)
    } else if b >= 1 << 20 {
        format!("{:.2} MB", b as f64 / (1u64 << 20) as f64)
    } else {
        format!("{:.0} KB", (b as f64 / 1024.0).max(1.0))
    }
}

// ------------------------------------------------------------------ Doppler Shifter

/// Doppler Shifter settings: a sound passing by in a straight line or going round in a circle.
#[derive(Clone, Debug, PartialEq)]
pub struct Doppler {
    pub circular: bool,
    /// Straight line: how far away the source starts (m) and how far the line passes from you (m).
    pub start_m: f32,
    pub perp_m: f32,
    /// Circle: radius (m) and how far its centre is in front of you (m).
    pub radius_m: f32,
    pub center_m: f32,
    /// Speed (m/s).
    pub speed: f32,
    pub volume: bool,
    pub direction: bool,
}

impl Default for Doppler {
    fn default() -> Self {
        Doppler {
            circular: false,
            start_m: 60.0,
            perp_m: 4.0,
            radius_m: 20.0,
            center_m: 25.0,
            speed: 25.0,
            volume: true,
            direction: true,
        }
    }
}

const SOUND_SPEED: f64 = 343.0;

impl Doppler {
    /// The source's position (x right, y in front of you) after `t` seconds.
    fn pos(&self, t: f64) -> (f64, f64) {
        let v = self.speed.max(0.1) as f64;
        if self.circular {
            let r = self.radius_m.max(0.5) as f64;
            let a = v / r * t;
            (r * a.sin(), self.center_m as f64 - r * a.cos())
        } else {
            (-(self.start_m as f64) + v * t, self.perp_m.max(0.1) as f64)
        }
    }
}

/// Apply the Doppler Shifter to frames `a..b`: the pitch rises as the sound comes closer and
/// falls as it goes away (the travel time of sound changes), optionally louder when close and
/// panned to where it is.
pub fn doppler(d: &mut AudioData, a: usize, b: usize, p: &Doppler) {
    let n = d.frames();
    let b = b.min(n);
    if a >= b || d.channels() == 0 {
        return;
    }
    let rate = d.rate as f64;
    let len = b - a;
    let dist = |i: usize| -> (f64, f64) {
        let (x, y) = p.pos(i as f64 / rate);
        ((x * x + y * y).sqrt().max(0.1), x)
    };
    // the shortest distance sets the reference: no delay and full level there
    let step = (len / 2000).max(1);
    let rmin = (0..len)
        .step_by(step)
        .map(|i| dist(i).0)
        .fold(f64::MAX, f64::min)
        .max(0.1);
    let src: Vec<Vec<f32>> = d.ch.iter().map(|c| c[a..b].to_vec()).collect();
    let mono: Vec<f32> = (0..len)
        .map(|i| src.iter().map(|c| c[i]).sum::<f32>() / src.len() as f32)
        .collect();
    let read = |s: &[f32], t: f64| -> f32 {
        if t < 0.0 {
            return 0.0;
        }
        let i = t.floor() as usize;
        let f = (t - i as f64) as f32;
        let x0 = s.get(i).copied().unwrap_or(0.0);
        let x1 = s.get(i + 1).copied().unwrap_or(0.0);
        x0 + (x1 - x0) * f
    };
    let nch = d.channels();
    for i in 0..len {
        let (r, x) = dist(i);
        let delay = (r - rmin) / SOUND_SPEED * rate;
        let t = i as f64 - delay;
        let g = if p.volume { (rmin / r) as f32 } else { 1.0 };
        if p.direction && nch >= 2 {
            // equal-power pan by the direction the sound comes from
            let pan = (x / r).clamp(-1.0, 1.0) as f32;
            let ang = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
            let v = read(&mono, t) * g;
            d.ch[0][a + i] = v * ang.cos() * std::f32::consts::SQRT_2;
            d.ch[1][a + i] = v * ang.sin() * std::f32::consts::SQRT_2;
            for c in 2..nch {
                d.ch[c][a + i] = read(&src[c], t) * g;
            }
        } else {
            for c in 0..nch {
                d.ch[c][a + i] = read(&src[c], t) * g;
            }
        }
    }
}

// ------------------------------------------------------------------ Application Settings

/// Every settings file of AudioCraft's settings folder, in one JSON file.
pub fn export_settings(dir: &Path, out: &Path) -> Result<usize, String> {
    let mut files = serde_json::Map::new();
    let rd =
        std::fs::read_dir(dir).map_err(|e| format!("Could not read the settings folder: {e}"))?;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().map(|x| x == "json").unwrap_or(false) {
            if let Ok(text) = std::fs::read_to_string(&p) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    files.insert(
                        p.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        v,
                    );
                }
            }
        }
    }
    let n = files.len();
    let doc = serde_json::json!({ "audiocraft_settings": 1, "files": files });
    let s = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    audio::write_atomic(out, s.as_bytes())?;
    Ok(n)
}

/// Write the files of an exported settings file back into the settings folder.
pub fn import_settings(dir: &Path, input: &Path) -> Result<usize, String> {
    let text = std::fs::read_to_string(input)
        .map_err(|e| format!("Could not read {}: {e}", input.display()))?;
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| "That is not an AudioCraft settings file.".to_string())?;
    let files = v
        .get("files")
        .and_then(|f| f.as_object())
        .filter(|_| v.get("audiocraft_settings").is_some())
        .ok_or("That is not an AudioCraft settings file.")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut n = 0;
    for (name, content) in files {
        // only plain file names inside the settings folder
        let safe = Path::new(name)
            .file_name()
            .map(|f| f.to_string_lossy() == name.as_str())
            .unwrap_or(false)
            && name.ends_with(".json");
        if !safe {
            continue;
        }
        let s = serde_json::to_string_pretty(content).map_err(|e| e.to_string())?;
        audio::write_atomic(&dir.join(name), s.as_bytes())?;
        n += 1;
    }
    Ok(n)
}

impl App {
    /// Open the export dialog for a file or the session mixdown.
    pub fn open_export(&mut self, src: ExportSource) -> Result<(), String> {
        let (name, rate, channels, float, bits) = match src {
            ExportSource::File(id) => {
                let f = self.file(id).ok_or("Open a file first.")?;
                (
                    Path::new(&f.name)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    f.data.rate,
                    f.data.channels(),
                    f.float,
                    f.bits,
                )
            }
            ExportSource::Mixdown { selection } => {
                let s = self
                    .session
                    .as_ref()
                    .ok_or("No multitrack session is open.")?;
                if selection && s.selection.map(|(a, b)| b - a <= 0.001).unwrap_or(true) {
                    return Err("Make a time selection first.".into());
                }
                (format!("{}_mixdown", s.name), s.rate, 2, false, 24)
            }
        };
        let folder = self
            .default_save_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let sample_type = SAMPLE_TYPES
            .iter()
            .position(|(_, b, f)| *b == bits && *f == float)
            .unwrap_or(2);
        self.dialog = Some(Dialog::Export(ExportOpts {
            src,
            name,
            folder,
            format: 0,
            rate,
            channels,
            sample_type,
            meta: true,
            kbps: 256,
        }));
        Ok(())
    }

    fn export_seconds(&self, src: ExportSource) -> f64 {
        match src {
            ExportSource::File(id) => self.file(id).map(|f| f.data.duration()).unwrap_or(0.0),
            ExportSource::Mixdown { selection } => self
                .session
                .as_ref()
                .map(|s| match (selection, s.selection) {
                    (true, Some((a, b))) => b - a,
                    _ => s.length(),
                })
                .unwrap_or(0.0),
        }
    }

    /// The export dialog's controls. Returns true when Export was pressed.
    pub fn export_ui(&mut self, ui: &mut egui::Ui, o: &mut ExportOpts, close: &mut bool) -> bool {
        let mut go = false;
        egui::Grid::new("export_grid")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label("File Name:");
                ui.add(egui::TextEdit::singleline(&mut o.name).desired_width(320.0));
                ui.end_row();
                ui.label("Location:");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut o.folder).desired_width(240.0));
                    if ui.button("Browse...").clicked() {
                        if let Some(d) =
                            rfd::FileDialog::new().set_directory(&o.folder).ask_folder()
                        {
                            o.folder = d.display().to_string();
                        }
                    }
                });
                ui.end_row();
                ui.label("Format:");
                egui::ComboBox::from_id_salt("exp_fmt")
                    .width(320.0)
                    .selected_text(FORMATS[o.format.min(FORMATS.len() - 1)].0)
                    .show_ui(ui, |ui| {
                        for (i, (n, _)) in FORMATS.iter().enumerate() {
                            ui.selectable_value(&mut o.format, i, *n);
                        }
                    });
                ui.end_row();
                ui.label("Sample Type:");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("exp_rate")
                        .selected_text(format!("{} Hz", o.rate))
                        .show_ui(ui, |ui| {
                            for r in RATES {
                                ui.selectable_value(&mut o.rate, *r, format!("{r} Hz"));
                            }
                        });
                    egui::ComboBox::from_id_salt("exp_ch")
                        .selected_text(match o.channels {
                            1 => "Mono",
                            2 => "Stereo",
                            _ => "5.1",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut o.channels, 1, "Mono");
                            ui.selectable_value(&mut o.channels, 2, "Stereo");
                        });
                });
                ui.end_row();
                ui.label("Format Settings:");
                match o.ext() {
                    "mp3" | "m4a" => {
                        egui::ComboBox::from_id_salt("exp_kbps")
                            .selected_text(format!("{} kbps (constant)", o.kbps))
                            .show_ui(ui, |ui| {
                                for k in [96, 128, 160, 192, 224, 256, 320] {
                                    ui.selectable_value(&mut o.kbps, k, format!("{k} kbps"));
                                }
                            });
                    }
                    ext => {
                        let allowed: Vec<usize> = (0..SAMPLE_TYPES.len())
                            .filter(|i| {
                                ext == "wav"
                                    || (SAMPLE_TYPES[*i].1 >= 16 && SAMPLE_TYPES[*i].1 <= 24)
                            })
                            .collect();
                        if !allowed.contains(&o.sample_type) {
                            o.sample_type = 2;
                        }
                        egui::ComboBox::from_id_salt("exp_bits")
                            .selected_text(SAMPLE_TYPES[o.sample_type].0)
                            .show_ui(ui, |ui| {
                                for i in allowed {
                                    ui.selectable_value(&mut o.sample_type, i, SAMPLE_TYPES[i].0);
                                }
                            });
                    }
                }
                ui.end_row();
            });
        ui.checkbox(&mut o.meta, "Include markers and other metadata");
        if matches!(o.ext(), "mp3" | "m4a") {
            ui.label(
                RichText::new("MP3 and AAC are written with the FFmpeg bundled in AudioCraft.")
                    .small()
                    .color(theme::text_dim()),
            );
        }
        let secs = self.export_seconds(o.src);
        ui.label(
            RichText::new(format!(
                "Estimated File Size: {}",
                human_size(o.estimate(secs))
            ))
            .color(theme::text_dim()),
        );
        let path = o.path();
        if path.exists() {
            ui.colored_label(
                egui::Color32::from_rgb(235, 180, 80),
                format!("{} exists and will be replaced.", path.display()),
            );
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if crate::theme::dialog_button(ui, "OK", true).clicked() {
                go = true;
            }
            if crate::theme::dialog_button(ui, "Cancel", false).clicked() {
                *close = true;
            }
        });
        go
    }

    /// Write the export. Errors are shown; the dialog closes either way.
    pub fn export_commit(&mut self, o: &ExportOpts) {
        let (data, meta) = match o.src {
            ExportSource::File(id) => match self.file(id) {
                Some(f) => (
                    f.data.as_ref().clone(),
                    if o.meta {
                        f.meta.clone()
                    } else {
                        Default::default()
                    },
                ),
                None => return,
            },
            ExportSource::Mixdown { selection } => {
                let Some(s) = &self.session else { return };
                let files = &self.files;
                let mix =
                    s.snapshot(&|id| files.iter().find(|f| f.id == id).map(|f| f.data.clone()));
                let (a, b) = match (selection, s.selection) {
                    (true, Some((a, b))) if b > a => (a, b),
                    _ => (0.0, s.length()),
                };
                if b <= a {
                    self.message("Nothing to export", "The session has no clips.");
                    return;
                }
                (
                    crate::session::mixdown(&mix, s.rate, a, b),
                    Default::default(),
                )
            }
        };
        let data = data.conform(o.rate, o.channels);
        let (bits, float) = o.bits_float();
        let path = o.path();
        if o.folder.trim().is_empty() || !Path::new(&o.folder).is_dir() {
            self.message("Export", "Choose a folder that exists (Browse...).");
            return;
        }
        self.status = format!("Exporting {}...", path.display());
        match audio::save_with_meta(&path, &data, bits, float, false, &meta) {
            Ok(()) => {
                self.note_saved(&path);
                self.add_recent(&path);
                self.status = format!("Exported {}", path.display());
            }
            Err(e) => self.message("Export failed", &e),
        }
    }

    /// Export or import the Application Settings (preferences, workspaces, favorites, recent files).
    pub fn settings_transfer(&mut self, export: bool) -> Result<(), String> {
        let dir = crate::prefs::path()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .ok_or("The settings folder is unknown on this system.")?;
        if export {
            crate::prefs::save(&self.prefs);
            let Some(out) = rfd::FileDialog::new()
                .add_filter("AudioCraft settings", &["json"])
                .set_file_name("AudioCraft Settings.json")
                .ask_save()
            else {
                return Ok(());
            };
            let n = export_settings(&dir, &out)?;
            self.status = format!("Exported {n} settings files to {}", out.display());
        } else {
            let Some(inp) = rfd::FileDialog::new()
                .add_filter("AudioCraft settings", &["json"])
                .ask_file()
            else {
                return Ok(());
            };
            let n = import_settings(&dir, &inp)?;
            self.prefs = crate::prefs::load();
            self.message("Import Application Settings", &format!("Imported {n} settings files. Preferences apply now; workspaces and favorites apply the next time AudioCraft starts."));
        }
        Ok(())
    }

    /// Doppler Shifter / Sound Remover dialog contents (shared with the dialog window).
    pub fn doppler_ui(ui: &mut egui::Ui, p: &mut Doppler) {
        ui.horizontal(|ui| {
            ui.label("Path Type:");
            ui.selectable_value(&mut p.circular, false, "Straight Line");
            ui.selectable_value(&mut p.circular, true, "Circular");
        });
        if p.circular {
            ui.add(
                crate::theme::RSlider::new(&mut p.radius_m, 1.0..=200.0)
                    .suffix(" m")
                    .text("Radius"),
            );
            ui.add(
                crate::theme::RSlider::new(&mut p.center_m, 0.0..=200.0)
                    .suffix(" m")
                    .text("Centre in front of you"),
            );
        } else {
            ui.add(
                crate::theme::RSlider::new(&mut p.start_m, 0.0..=500.0)
                    .suffix(" m")
                    .text("Starting distance away"),
            );
            ui.add(
                crate::theme::RSlider::new(&mut p.perp_m, 0.5..=100.0)
                    .suffix(" m")
                    .text("Passes in front of you at"),
            );
        }
        ui.add(
            crate::theme::RSlider::new(&mut p.speed, 1.0..=150.0)
                .suffix(" m/s")
                .text("Velocity"),
        );
        ui.checkbox(&mut p.volume, "Adjust volume based on distance");
        ui.checkbox(&mut p.direction, "Adjust volume based on direction (pan)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doppler_raises_pitch_coming_and_lowers_it_going() {
        let rate = 8000;
        let mut d = AudioData::new(rate, 2, rate as usize * 4);
        for i in 0..d.frames() {
            let v = (i as f32 * std::f32::consts::TAU * 500.0 / rate as f32).sin() * 0.5;
            d.ch[0][i] = v;
            d.ch[1][i] = v;
        }
        let p = Doppler {
            start_m: 50.0,
            speed: 25.0,
            perp_m: 2.0,
            volume: false,
            direction: false,
            ..Default::default()
        };
        let n = d.frames();
        doppler(&mut d, 0, n, &p);
        let zc = |s: &[f32]| {
            s.windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count()
        };
        // first second: approaching (higher than 500 Hz); last second: going away (lower)
        let first = zc(&d.ch[0][800..rate as usize]);
        let last = zc(&d.ch[0][n - rate as usize..n - 200]);
        assert!(first > last + 20, "{first} vs {last}");
        assert!(d.ch.iter().flatten().all(|v| v.is_finite()));
        // empty or out-of-range spans do nothing and never panic
        doppler(&mut d, 10, 5, &p);
        doppler(&mut d, n + 5, n + 50, &p);
    }

    #[test]
    fn settings_round_trip_and_reject_foreign_files() {
        let base = std::env::temp_dir().join(format!("audiocraft_settings_{}", std::process::id()));
        let src = base.join("src");
        let dst = base.join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("preferences.json"), r#"{"zoom_factor": 2.0}"#).unwrap();
        std::fs::write(src.join("notes.txt"), "not settings").unwrap();
        let out = base.join("bundle.json");
        assert_eq!(export_settings(&src, &out).unwrap(), 1);
        assert_eq!(import_settings(&dst, &out).unwrap(), 1);
        assert!(
            std::fs::read_to_string(dst.join("preferences.json"))
                .unwrap()
                .contains("zoom_factor")
        );
        // a file that isn't a settings bundle, or one naming paths outside the folder
        std::fs::write(base.join("bad.json"), r#"{"x": 1}"#).unwrap();
        assert!(import_settings(&dst, &base.join("bad.json")).is_err());
        std::fs::write(
            base.join("evil.json"),
            r#"{"audiocraft_settings":1,"files":{"../x.json":{}}}"#,
        )
        .unwrap();
        assert_eq!(import_settings(&dst, &base.join("evil.json")).unwrap(), 0);
    }

    #[test]
    fn export_paths_and_sample_types() {
        let mut o = ExportOpts {
            src: ExportSource::File(1),
            name: "song.wav".into(),
            folder: "out".into(),
            format: 2,
            rate: 44100,
            channels: 2,
            sample_type: 4,
            meta: true,
            kbps: 256,
        };
        assert_eq!(o.path(), Path::new("out").join("song.flac"));
        assert_eq!(o.bits_float(), (24, false), "FLAC has no float samples");
        o.format = 0;
        assert_eq!(o.bits_float(), (32, true));
        assert!(o.estimate(1.0) > 350_000);
        o.format = 3;
        assert_eq!(o.estimate(8.0), 256_000);
    }
}
