//! Application state and commands (file handling, editing, transport, sessions).

use crate::audio::{self, AudioData, Peaks};
use crate::dsp::{self, Effect, EffectSlot, FadeCurve, NoiseColor, NoiseProfile, Wave};
use crate::engine::{Engine, Source};
use crate::session::{Marker, Session, Track};
use crate::sysui::Ask;
use crate::theme;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Waveform,
    Multitrack,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Move,
    /// Direct Selection (the white arrow): keyframes with their Bezier handles.
    Direct,
    Razor,
    Slip,
    TimeSel,
    /// Rectangular spectral selection (time x frequency).
    Marquee,
    /// Free-form spectral selection.
    Lasso,
    /// Painted spectral selection.
    Brush,
    /// Paint over a sound in the spectral display to heal it away.
    SpotHeal,
}

impl Tool {
    /// The spectral tools need the Spectral Frequency Display in the Waveform Editor.
    pub fn is_spectral(self) -> bool {
        matches!(
            self,
            Tool::Marquee | Tool::Lasso | Tool::Brush | Tool::SpotHeal
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilesTab {
    Files,
    Favorites,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CenterTab {
    Editor,
    Mixer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeFormat {
    Decimal,
    Samples,
    Seconds,
    /// hh:mm:ss:ff at Preferences > Time Display > frames per second (non-drop).
    Smpte,
    /// mm:ss:ff with 75 frames a second (audio CD sectors).
    CompactDisc,
}

impl TimeFormat {
    /// Every format with its menu name, in menu order.
    pub const ALL: [(TimeFormat, &'static str); 5] = [
        (TimeFormat::Decimal, "Decimal (mm:ss.ddd)"),
        (TimeFormat::Samples, "Samples"),
        (TimeFormat::Seconds, "Seconds"),
        (TimeFormat::Smpte, "SMPTE Timecode (hh:mm:ss:ff)"),
        (TimeFormat::CompactDisc, "Compact Disc (mm:ss:ff, 75 fps)"),
    ];

    pub fn from_index(i: u8) -> TimeFormat {
        TimeFormat::ALL
            .get(i as usize)
            .map(|f| f.0)
            .unwrap_or(TimeFormat::Decimal)
    }

    /// The next format (clicking the big time display steps through them).
    pub fn next(self) -> TimeFormat {
        let i = TimeFormat::ALL
            .iter()
            .position(|f| f.0 == self)
            .unwrap_or(0);
        TimeFormat::ALL[(i + 1) % TimeFormat::ALL.len()].0
    }

    fn frames_text(t: f64, fps: f64, hours: bool) -> String {
        let fps = fps.max(1.0);
        let whole = fps.round().max(1.0) as i64;
        let total = (t.max(0.0) * fps + 1e-6).floor() as i64;
        let (f, secs) = (total % whole, total / whole);
        if hours {
            format!(
                "{:02}:{:02}:{:02}:{:02}",
                secs / 3600,
                secs / 60 % 60,
                secs % 60,
                f
            )
        } else {
            format!("{:02}:{:02}:{:02}", secs / 60, secs % 60, f)
        }
    }

    fn parse_frames(s: &str, fps: f64) -> Option<f64> {
        let p: Vec<f64> = s
            .trim()
            .split([':', ';'])
            .map(|x| x.trim().parse::<f64>().ok())
            .collect::<Option<_>>()?;
        let fps = fps.max(1.0);
        let (h, m, sec, f) = match p.as_slice() {
            [h, m, s, f] => (*h, *m, *s, *f),
            [m, s, f] => (0.0, *m, *s, *f),
            [s, f] => (0.0, 0.0, *s, *f),
            _ => return None,
        };
        let t = h * 3600.0 + m * 60.0 + sec + f / fps.round().max(1.0);
        (t.is_finite() && t >= 0.0).then_some(t)
    }

    /// `t` seconds as text in this format.
    pub fn format(self, t: f64, rate: u32) -> String {
        match self {
            TimeFormat::Decimal => audio::fmt_time(t),
            TimeFormat::Samples => format!("{}", (t * rate as f64).round() as i64),
            TimeFormat::Seconds => format!("{t:.3}"),
            TimeFormat::Smpte => TimeFormat::frames_text(t, crate::prefs::fps(), true),
            TimeFormat::CompactDisc => TimeFormat::frames_text(t, 75.0, false),
        }
    }

    /// Text typed in this format, back to seconds (other formats are understood too).
    pub fn parse(self, s: &str, rate: u32) -> Option<f64> {
        match self {
            TimeFormat::Samples => s.trim().parse::<f64>().ok().map(|v| v / rate.max(1) as f64),
            TimeFormat::Smpte => {
                TimeFormat::parse_frames(s, crate::prefs::fps()).or_else(|| audio::parse_time(s))
            }
            TimeFormat::CompactDisc => {
                TimeFormat::parse_frames(s, 75.0).or_else(|| audio::parse_time(s))
            }
            _ => audio::parse_time(s),
        }
    }
}

/// Floating analysis windows that are open.
pub struct Panels {
    pub stats: bool,
    pub freq: bool,
}

pub struct AudioFile {
    pub id: u64,
    pub name: String,
    pub path: Option<PathBuf>,
    pub data: Arc<AudioData>,
    pub bits: u16,
    /// Write Broadcast Wave (bext) metadata when saving as WAV.
    pub bwf: bool,
    pub float: bool,
    pub history: Vec<(String, Arc<AudioData>)>,
    pub hist_pos: usize,
    pub saved_pos: Option<usize>,
    pub markers: Vec<Marker>,
    pub rack: Vec<EffectSlot>,
    pub rack_in_db: f32,
    pub rack_out_db: f32,
    pub rack_mix: f32,
    pub rack_power: bool,
    /// Which channels edits and effects apply to (Edit > Enable Channels).
    pub enabled_ch: Vec<bool>,
    pub view: (f64, f64),
    pub selection: Option<(f64, f64)>,
    /// Metadata fields (Metadata panel): RIFF INFO keys, "bext.*", "ixml.*", "cart.*", "xmp.*".
    pub meta: std::collections::BTreeMap<String, String>,
    /// Spectral selection (marquee, lasso or paintbrush) over the spectral display.
    pub spec_sel: Option<crate::spectral::SpecShape>,
    pub cursor: f64,
    pub peaks: Peaks,
    pub peaks_ptr: usize,
    pub version: u64,
    /// Peak pyramids of recent history states, so Undo/Redo never rebuilds them.
    pub peaks_cache: Vec<(usize, Peaks)>,
    /// The fades made with the fade handles: the history state they were applied to and
    /// (length s, curve) of the fade in and the fade out. They stay adjustable while the
    /// fade is the latest edit.
    pub fades: Option<(usize, [(f64, f32); 2])>,
}

impl AudioFile {
    pub fn new(
        id: u64,
        name: String,
        path: Option<PathBuf>,
        data: AudioData,
        bits: u16,
        float: bool,
        label: &str,
    ) -> Self {
        Self::new_inner(id, name, path, data, bits, float, label, None)
    }

    /// Take peaks that were built on a worker thread. A long clip must never pay for a second
    /// peak pyramid on the interface thread just because it finished importing.
    pub fn new_with_peaks(
        id: u64,
        name: String,
        path: Option<PathBuf>,
        data: AudioData,
        bits: u16,
        float: bool,
        label: &str,
        peaks: Peaks,
    ) -> Self {
        Self::new_inner(id, name, path, data, bits, float, label, Some(peaks))
    }

    fn new_inner(
        id: u64,
        name: String,
        path: Option<PathBuf>,
        data: AudioData,
        bits: u16,
        float: bool,
        label: &str,
        peaks: Option<Peaks>,
    ) -> Self {
        let data = Arc::new(data);
        let ptr = Arc::as_ptr(&data) as usize;
        let dur = data.duration().max(0.001);
        let nch = data.channels();
        let ready = peaks.is_some();
        let mut f = AudioFile {
            id,
            name,
            path: path.clone(),
            data: data.clone(),
            bits,
            bwf: false,
            float,
            history: vec![(label.to_string(), data)],
            hist_pos: 0,
            saved_pos: if path.is_some() { Some(0) } else { None },
            markers: vec![],
            rack: vec![],
            rack_in_db: 0.0,
            rack_out_db: 0.0,
            rack_mix: 1.0,
            rack_power: true,
            enabled_ch: vec![true; nch],
            view: (0.0, dur),
            selection: None,
            meta: Default::default(),
            spec_sel: None,
            cursor: 0.0,
            peaks: peaks.unwrap_or_default(),
            peaks_ptr: if ready { ptr } else { 0 },
            version: 1,
            peaks_cache: vec![],
            fades: None,
        };
        if !ready {
            f.refresh_peaks();
        }
        f
    }
    pub fn dirty(&self) -> bool {
        self.saved_pos != Some(self.hist_pos)
    }
    pub fn refresh_peaks(&mut self) {
        let p = Arc::as_ptr(&self.data) as usize;
        if p == self.peaks_ptr {
            return;
        }
        // keep the outgoing pyramid for Undo/Redo (only states still in the history)
        if self.peaks_ptr != 0 && !self.peaks.levels.is_empty() {
            let old = std::mem::take(&mut self.peaks);
            self.peaks_cache.retain(|(q, _)| *q != self.peaks_ptr);
            self.peaks_cache.push((self.peaks_ptr, old));
        }
        let live: Vec<usize> = self
            .history
            .iter()
            .map(|(_, d)| Arc::as_ptr(d) as usize)
            .collect();
        self.peaks_cache.retain(|(q, _)| live.contains(q));
        while self.peaks_cache.len() > 6 {
            self.peaks_cache.remove(0);
        }
        self.peaks = match self.peaks_cache.iter().position(|(q, _)| *q == p) {
            Some(i) => self.peaks_cache.remove(i).1,
            None => Peaks::build(&self.data),
        };
        self.peaks_ptr = p;
    }

    /// The handle fades when they are still the latest edit: (base history state, fades).
    pub fn live_fades(&self) -> Option<(usize, [(f64, f32); 2])> {
        let (base, f) = self.fades?;
        let ok = self.hist_pos == base + 1
            && self
                .history
                .get(base + 1)
                .map(|h| h.0.starts_with("Fade"))
                .unwrap_or(false);
        ok.then_some((base, f))
    }

    /// Set the handle fades: re-applied from the state before them, as one undoable step.
    pub fn set_fades(&mut self, fades: [(f64, f32); 2], label: &str) {
        let base = self.live_fades().map(|(b, _)| b).unwrap_or(self.hist_pos);
        let Some(src) = self.history.get(base).map(|h| h.1.clone()) else {
            return;
        };
        let mut d = (*src).clone();
        let n = d.frames();
        let r = d.rate as f64;
        let [(lin, pin), (lout, pout)] = fades;
        let kin = ((lin.max(0.0) * r) as usize).min(n);
        let kout = ((lout.max(0.0) * r) as usize).min(n);
        if kin > 0 {
            crate::dsp::fade_pow(&mut d, 0, kin, true, pin);
        }
        if kout > 0 {
            crate::dsp::fade_pow(&mut d, n - kout, n, false, pout);
        }
        self.hist_pos = base;
        if kin == 0 && kout == 0 {
            // both handles back at the edges: the fades are gone
            self.history.truncate(base + 1);
            self.fades = None;
            self.set_data(src);
            return;
        }
        self.push(label, d);
        // push may have dropped old states to stay within the memory budget
        self.fades = Some((self.hist_pos - 1, fades));
    }
    pub fn push(&mut self, label: &str, data: AudioData) {
        let data = Arc::new(data);
        self.history.truncate(self.hist_pos + 1);
        self.history.push((label.to_string(), data.clone()));
        self.hist_pos += 1;
        // the history keeps the steps Preferences > Memory asks for, within a memory budget
        let bytes = |h: &[(String, Arc<AudioData>)]| {
            h.iter()
                .map(|(_, d)| d.ch.iter().map(|c| c.len() * 4).sum::<usize>())
                .sum::<usize>()
        };
        while self.history.len() > 2
            && (self.history.len() > crate::prefs::undo_limit() + 1
                || bytes(&self.history) > crate::prefs::UNDO_MEMORY_BUDGET)
        {
            self.history.remove(0);
            self.hist_pos -= 1;
            self.saved_pos = self.saved_pos.and_then(|s| s.checked_sub(1));
        }
        self.set_data(data);
    }
    pub fn set_data(&mut self, data: Arc<AudioData>) {
        self.data = data;
        self.version += 1;
        let dur = self.data.duration();
        self.cursor = self.cursor.min(dur);
        if let Some((a, b)) = self.selection {
            if a >= dur {
                self.selection = None;
            } else {
                self.selection = Some((a, b.min(dur)));
            }
        }
        if self.view.1 - self.view.0 > dur.max(0.001) * 1.0001 || self.view.0 > dur {
            self.view = (0.0, dur.max(0.001));
        }
        self.refresh_peaks();
    }
    pub fn goto_history(&mut self, pos: usize) {
        if pos < self.history.len() {
            self.hist_pos = pos;
            let d = self.history[pos].1.clone();
            self.set_data(d);
        }
    }
    /// Frame range of the selection or the whole file.
    pub fn range(&self) -> (usize, usize) {
        let r = self.data.rate as f64;
        match self.selection {
            Some((a, b)) if b > a => (
                (a * r).round() as usize,
                ((b * r).round() as usize).min(self.data.frames()),
            ),
            _ => (0, self.data.frames()),
        }
    }
    pub fn size_bytes(&self) -> u64 {
        let bps = if self.float {
            4
        } else {
            (self.bits as u64 / 8).max(1)
        };
        self.data.frames() as u64 * self.data.channels() as u64 * bps
    }
}

pub enum Dialog {
    /// Name a new workspace (saved from the current layout).
    NewWorkspace {
        name: String,
    },
    /// Rename order / delete the person's workspaces.
    EditWorkspaces,
    /// Rename the marker nearest the playhead (index in the marker list).
    RenameMarker {
        index: usize,
        name: String,
    },
    /// Rename a track from the Multitrack menu.
    RenameTrack {
        index: usize,
        name: String,
    },
    /// Multiply the selection's level by a dB amount.
    Gain {
        db: f32,
    },
    /// Batch Processor: many files, one preset, one output folder.
    Batch {
        inputs: Vec<String>,
        preset: usize,
        out_dir: String,
        suffix: String,
        result: String,
    },
    /// Pick a track's colour (track id, colour being edited).
    TrackColor {
        track: u64,
        color: [u8; 3],
    },
    /// A file going into the session has another sample rate or bit depth than the session.
    /// `choice`: 1 convert the file, 2 change the session, 3 keep it as it is (plays at the
    /// wrong speed).
    RateMismatch {
        file_id: u64,
        track: usize,
        at: f64,
        choice: u8,
    },
    NewFile {
        name: String,
        rate: u32,
        channels: usize,
        bits: u16,
        make_default: bool,
    },
    /// Writing a file over a path that already exists: "Replace" saves, "Don't Replace" cancels.
    ConfirmReplace {
        id: u64,
        path: PathBuf,
    },
    NewSession {
        name: String,
        rate: u32,
        tracks: usize,
        folder: String,
        bits: u16,
        mix: u8,
        make_default: bool,
    },
    Effect {
        effect: Effect,
        preview: bool,
    },
    Normalize {
        db: f32,
    },
    Fade {
        fade_in: bool,
        curve: FadeCurve,
    },
    Speed {
        ratio: f32,
        semis: f32,
        mode: u8,
    },
    Loudness {
        target: f32,
    },
    NoiseReduction {
        amount: f32,
        reduce_db: f32,
    },
    Tone {
        freq: f32,
        wave: Wave,
        db: f32,
        secs: f32,
    },
    Noise {
        color: NoiseColor,
        db: f32,
        secs: f32,
    },
    InsertSilence {
        secs: f32,
    },
    Convert {
        rate: u32,
        channels: usize,
        bits: u16,
    },
    /// Declare a different sample rate without moving any samples.
    InterpretSampleRate {
        rate: u32,
    },
    /// Split the file into low / mid / high band files. `cross[..bands-1]` are the crossovers.
    FrequencySplit {
        bands: usize,
        cross: [f32; 3],
    },
    /// Text to speech into a new file. Empty voice uses the system default. `tab` 0 is the
    /// system voice, 1 the vocal-tract synthesizer.
    Speech {
        text: String,
        voice: String,
        rate: i32,
        /// Pitch shift of the system voice in semitones.
        pitch: f32,
        tab: u8,
        tract: crate::tract::TractParams,
    },
    /// The imported video is not standard Rec.709 8-bit colour: offer to convert the preview.
    ColorSpace {
        path: PathBuf,
        why: String,
    },
    Preferences,
    /// Fade Envelope (amplitude 0..1) or Gain Envelope (dB) over the selection.
    Envelope {
        gain: bool,
        points: Vec<(f32, f32)>,
    },
    DeleteSilence {
        thr_db: f32,
        min_s: f32,
        keep_s: f32,
    },
    MarkAudio {
        thr_db: f32,
        min_s: f32,
    },
    PitchBender {
        start_st: f32,
        end_st: f32,
    },
    Dtmf {
        digits: String,
        tone_ms: f32,
        pause_ms: f32,
        db: f32,
    },
    ClickPop {
        sensitivity: f32,
    },
    PluginManager,
    About,
    Shortcuts,
    Message(String, String),
    /// Export Audio File / Export Multitrack Mixdown.
    Export(crate::export::ExportOpts),
    /// Effects > Special > Doppler Shifter (process).
    Doppler(crate::export::Doppler),
    /// Effects > Noise Reduction / Restoration > Sound Remover (process).
    SoundRemover {
        reduce_db: f32,
        strength: f32,
    },
}

pub struct Essential {
    /// Opened from a clip in the Multitrack editor: Back returns to the session.
    pub from_session: bool,
    pub kind: usize,
    pub loudness: bool,
    pub loudness_target: f32,
    pub reduce_noise: bool,
    pub noise_amount: f32,
    pub rumble: bool,
    pub dehum: bool,
    pub hum_freq: f32,
    pub dynamics: bool,
    pub dyn_amount: f32,
    pub eq: bool,
    pub eq_preset: usize,
    pub reverb: bool,
    pub reverb_amount: f32,
    pub width: bool,
    pub width_amount: f32,
}

impl Default for Essential {
    fn default() -> Self {
        Essential {
            from_session: false,
            kind: 0,
            loudness: true,
            loudness_target: -23.0,
            reduce_noise: false,
            noise_amount: 5.0,
            rumble: false,
            dehum: false,
            hum_freq: 50.0,
            dynamics: false,
            dyn_amount: 5.0,
            eq: false,
            eq_preset: 0,
            reverb: false,
            reverb_amount: 3.0,
            width: false,
            width_amount: 5.0,
        }
    }
}

pub struct App {
    pub engine: Engine,
    pub mode: Mode,
    pub tool: Tool,
    pub files: Vec<AudioFile>,
    pub current: Option<u64>,
    pub selected_file: Option<u64>,
    /// Folder of the first file opened/imported: the default-save mode "Where the first imported file is".
    pub first_import_dir: Option<PathBuf>,
    /// Folder the last file was saved into: the default-save mode "Where the last file was saved".
    pub last_saved_dir: Option<PathBuf>,
    /// The Files panel's multi-selection: a plain click selects one, Shift+click selects the
    /// range from the last selection to the clicked file, Ctrl+click toggles the file.
    pub files_sel: Vec<u64>,
    pub next_id: u64,
    pub session: Option<Session>,
    pub session_hist: Vec<(String, crate::session::HistState)>,
    /// The session revision last compared with the undo history (see `session_hist_catch_up`).
    pub session_hist_seen: u64,
    pub session_hist_pos: usize,
    pub clipboard: Option<AudioData>,
    /// Edit > Set Current Clipboard: the four clipboards not in use.
    pub clipboards: crate::menus_more::Clipboards,
    /// A copied multitrack clip (Ctrl+C / Ctrl+V in Multitrack).
    pub clip_clipboard: Option<crate::session::Clip>,
    /// A rendered preview playing instead of the file (Stretch and Pitch).
    pub preview_audio: Option<Arc<AudioData>>,
    pub preview_dirty: bool,
    pub files_tab: FilesTab,
    pub center_tab: CenterTab,
    pub panels: Panels,
    pub ws: crate::workspace::Workspaces,
    pub spectral: bool,
    /// Spectral Pitch Display in the Waveform Editor.
    pub pitch_display: bool,
    pub time_format: TimeFormat,
    pub dialog: Option<Dialog>,
    pub status: String,
    pub play_origin: f64,
    pub return_on_stop: bool,
    pub auto_scroll: bool,
    pub was_playing: bool,
    pub recording_at: Option<(f64, Option<usize>)>,
    pub noise_profile: Option<NoiseProfile>,
    /// Sound Remover: the sound model learned from a selection.
    pub sound_model: Option<NoiseProfile>,
    pub essential: Essential,
    pub media_dir: Option<PathBuf>,
    pub media_entries: Vec<(PathBuf, bool)>,
    pub media_filter: String,
    pub mix_key: (u64, u64, usize),
    pub meter_hold: [f32; 2],
    pub meter_hold_time: [f64; 2],
    /// Clip lights of the Levels meter (latched until clicked).
    pub meter_clip: [bool; 2],
    pub drag_state: crate::ui_wave::DragState,
    pub multi_drag: crate::ui_multi::MultiDrag,
    /// Edit > Preferences.
    pub prefs: crate::prefs::Prefs,
    /// The preferences being edited (OK applies them) and the category shown.
    pub prefs_edit: Option<(crate::prefs::Prefs, usize, String)>,
    /// The preferences as they were when the Preferences window opened: changes apply at once,
    /// Cancel puts these back.
    pub prefs_before: Option<crate::prefs::Prefs>,
    /// A .sesx save waiting on the compatibility check: (path, files, preview the conversion).
    pub sesx_pending: Option<crate::compat::Pending>,
    pub spec_cache: crate::ui_wave::SpecCache,
    pub screenshot: Option<PathBuf>,
    pub frame_count: u64,
    pub shot_requested: bool,
    pub shot_done: Arc<AtomicBool>,
    pub pending_cmd: Option<&'static str>,
    pub media_loaded: Option<PathBuf>,
    pub stats_cache: Option<((usize, usize, usize), Vec<(String, Vec<String>)>, usize)>,
    pub freq_cache: ((usize, usize, usize), Vec<Vec<f32>>),
    pub freq_size: usize,
    pub device_lists: Option<(Vec<String>, Vec<String>)>,
    pub show_env: bool,
    pub show_pan_env: bool,
    pub rack_clip_tab: bool,
    pub rack_window: Option<(crate::rack::RackTarget, usize)>,
    pub rack_entire: bool,
    /// Pre-render caches: track id -> the rendered stretch and the render that extends it.
    /// A cache follows the playhead (Preferences > Multitrack > Pre-render) or holds the whole
    /// track when it fits its share of the cap; outside it the track plays live.
    pub prerender: std::collections::HashMap<u64, PreCache>,
    /// When the playing mix last picked up newly rendered audio.
    pub prerender_pushed: Option<std::time::Instant>,
    /// Effects Rack presets saved by the person (name, slots).
    pub user_presets: Vec<(String, Vec<EffectSlot>)>,
    pub rack_preset_name: String,
    pub rack_selected: Option<usize>,
    /// An effect being dragged to another slot of this rack (like a track by its colour strip).
    pub rack_drag: Option<(crate::rack::RackTarget, usize)>,
    /// The last repeatable command (Edit > Repeat Last Command).
    pub last_cmd: Option<String>,
    /// What Close All / Close Unused Media / Close Session and Its Media closes once the
    /// save prompt has been answered.
    pub close_after: Option<crate::filecmds::CloseAfter>,
    pub file_rack_io: bool,
    pub hosted: Option<Vec<crate::plugins::PluginEntry>>,
    /// Open plug-in editors (docked in the Plug-ins panel, or floating).
    pub plug_dock: crate::plugdock::PlugDock,
    /// A hosted plug-in slot whose generic sliders were asked for (instead of its own editor).
    pub rack_generic: Option<(crate::rack::RackTarget, usize)>,
    pub recent: Vec<PathBuf>,
    pub palette: Option<String>,
    /// Effect presets saved by the person: (effect name, preset name, settings).
    pub effect_presets: Vec<(String, String, crate::dsp::Effect)>,
    /// The Effects Rack was the last thing clicked (Delete removes its selected effect).
    pub rack_focus: bool,
    /// The Files/Favorites browser was the last place clicked (Backspace clears its selection).
    pub files_focus: bool,
    /// Last frame's browser areas (the drop handler and clicks use them).
    pub files_drop_rects_last: Vec<egui::Rect>,
    /// What the track headers show below volume/pan: 0 inputs/outputs, 1 effects, 2 sends, 3 EQ.
    pub header_view: u8,
    /// The Multitrack Tools in the multitrack view's corner (snapping and the other modifiers)
    /// are shown.
    pub show_tools_window: bool,
    /// The snapping-target checkboxes inside that window are unfolded.
    pub snap_menu_open: bool,
    /// Global clip stretching: stretch handles on every clip's top corners.
    pub stretch_mode: bool,
    /// Snapping of clips, drops and the playhead to the targets picked in the Multitrack
    /// Tools window (clip edges, markers, ruler marks, the playhead, the session start).
    pub snapping: bool,
    /// The Track EQ window open for this track id.
    pub track_eq_window: Option<u64>,
    /// Where the Effects Rack panel was drawn last frame.
    pub rack_rect: egui::Rect,
    /// Where visible, active Files/Favorites browsers were drawn last frame. Audio dropped
    /// here is only added to the browser; audio dropped on the timeline still becomes a clip.
    pub files_drop_rects: Vec<egui::Rect>,
    /// Free disk space, memory and CPU readings for the status bar.
    pub sysmon: crate::sysinfo::Monitor,
    /// Status bar items: sample type, size, duration, free space, RAM, CPU, output device.
    pub status_items: [bool; 7],
    /// Unsaved things still to ask about on closing, and whether the app quits afterwards.
    pub close_queue: Option<(Vec<crate::dock::CloseItem>, bool)>,
    /// Files panel playback: loop, auto-play on selection, and the file playing.
    pub files_loop: bool,
    pub files_autoplay: bool,
    pub files_preview: Option<u64>,
    /// The person chose to close (after saving or not): let the window go.
    pub closing: bool,
    /// Audio decoding and peak building for ordinary files, off the interface thread.
    pub imports: crate::imports::ImportQueue,
    /// Multitrack drops waiting for their background import before a clip can be placed.
    pub pending_placements: Vec<(u64, usize, f64)>,
    /// The imported video's frames and import state (empty until a video is opened).
    pub video: crate::video::VideoState,
    pub video_queue: crate::video::VideoQueue,
    /// The one frame currently uploaded to the GPU, cached by index.
    pub video_tex: Option<(usize, egui::TextureHandle)>,
    /// Result of the Preferences > Video decoder search.
    pub video_detector_status: String,
    /// Recording workflow, help search, background jobs and other newer features.
    pub extra: crate::extras::Extras,
}

/// A session created just by opening the Mixer: ready with tracks, so the mixer shows
/// real channel strips instead of an empty panel.
pub fn mixer_session() -> Session {
    let mut s = Session::new("Untitled Session 1", 48000, 6);
    s.bits = 24;
    s
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        screenshot: Option<PathBuf>,
        demo: bool,
        start_multi: bool,
        files: Vec<PathBuf>,
    ) -> Self {
        let timing = std::env::var_os("AUDIOCRAFT_STARTUP_TIME").is_some();
        let since = || {
            crate::STARTED
                .get()
                .map(|t| t.elapsed().as_millis())
                .unwrap_or(0)
        };
        if timing {
            eprintln!("window and renderer ready after {} ms", since());
        }
        let prefs = crate::prefs::load();
        crate::theme::set_theme(prefs.color_theme);
        crate::theme::set_adjust(prefs.brightness, prefs.saturation);
        theme::setup(&cc.egui_ctx);
        if timing {
            eprintln!("theme ready after {} ms", since());
        }
        let mut engine = Engine::new();
        engine.ctx = Some(cc.egui_ctx.clone());
        let mut app = App {
            engine,
            mode: Mode::Waveform,
            tool: Tool::TimeSel,
            files: vec![],
            current: None,
            selected_file: None,
            first_import_dir: None,
            last_saved_dir: None,
            files_sel: vec![],
            next_id: 1,
            session: None,
            session_hist: vec![],
            session_hist_pos: 0,
            session_hist_seen: 0,
            clipboard: None,
            clipboards: Default::default(),
            clip_clipboard: None,
            preview_audio: None,
            preview_dirty: false,
            files_tab: FilesTab::Files,
            center_tab: CenterTab::Editor,
            panels: Panels {
                stats: false,
                freq: false,
            },
            ws: crate::workspace::Workspaces::load(),
            spectral: false,
            pitch_display: false,
            time_format: TimeFormat::Decimal,
            dialog: None,
            status: "Ready".into(),
            play_origin: 0.0,
            // stopping leaves the playhead where playback stopped (Preferences can change it)
            return_on_stop: false,
            auto_scroll: true,
            was_playing: false,
            recording_at: None,
            noise_profile: None,
            sound_model: None,
            essential: Essential::default(),
            media_dir: None,
            media_entries: vec![],
            media_filter: String::new(),
            mix_key: (0, 0, 0),
            meter_hold: [0.0; 2],
            meter_hold_time: [0.0; 2],
            meter_clip: [false; 2],
            drag_state: Default::default(),
            multi_drag: Default::default(),
            spec_cache: Default::default(),
            screenshot,
            frame_count: 0,
            shot_requested: false,
            shot_done: Arc::new(AtomicBool::new(false)),
            pending_cmd: None,
            media_loaded: None,
            stats_cache: None,
            freq_cache: ((0, 0, 0), vec![]),
            freq_size: 4096,
            device_lists: None,
            show_env: true,
            show_pan_env: true,
            prefs,
            prefs_edit: None,
            prefs_before: None,
            sesx_pending: None,
            rack_clip_tab: false,
            rack_window: None,
            rack_entire: false,
            prerender: Default::default(),
            prerender_pushed: None,
            user_presets: vec![],
            effect_presets: vec![],
            rack_focus: false,
            files_focus: false,
            files_drop_rects_last: vec![],
            header_view: 0,
            show_tools_window: true,
            snap_menu_open: false,
            stretch_mode: false,
            snapping: true,
            track_eq_window: None,
            rack_rect: egui::Rect::NOTHING,
            files_drop_rects: vec![],
            sysmon: Default::default(),
            status_items: [true, true, true, true, true, true, false],
            close_queue: None,
            files_loop: false,
            files_autoplay: false,
            files_preview: None,
            closing: false,
            imports: crate::imports::ImportQueue::default(),
            pending_placements: vec![],
            video: crate::video::VideoState::Empty,
            video_queue: crate::video::VideoQueue::default(),
            video_tex: None,
            video_detector_status: String::new(),
            extra: Default::default(),
            rack_preset_name: "(Default)".into(),
            rack_selected: None,
            rack_drag: None,
            last_cmd: None,
            close_after: None,
            file_rack_io: true,
            hosted: None,
            plug_dock: Default::default(),
            rack_generic: None,
            recent: load_recent(),
            palette: None,
        };
        crate::prefs::set_live(&app.prefs);
        crate::theme::set_theme(app.prefs.color_theme);
        crate::video::set_preview_res(app.prefs.video_resolution);
        crate::ffmpeg::set_selected(Some(PathBuf::from(&app.prefs.video_decoder_exe)));
        app.time_format = TimeFormat::from_index(app.prefs.time_format);
        for f in files {
            app.open_path(&f);
        }
        if demo {
            app.load_demo();
        }
        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|a| a == "--workspace") {
            if let Some(n) = args.get(i + 1) {
                app.ws.current = n.clone();
                app.ws.layout = crate::workspace::preset(n);
            }
        } else if app.screenshot.is_some() {
            // screenshots always show the built-in Default layout
            app.ws.current = "Default".into();
            app.ws.layout = crate::workspace::preset("Default");
        }
        if args.iter().any(|a| a == "--pitch") {
            app.pitch_display = true;
        }
        if std::env::args().any(|a| a == "--spectral") {
            // show the spectral display with a marquee selection (for screenshots)
            app.spectral = true;
            app.tool = Tool::Marquee;
            if let Some(f) = app.cur_mut() {
                f.spec_sel = Some(crate::spectral::SpecShape::Rect {
                    t0: 2.0,
                    t1: 3.2,
                    f0: 300.0,
                    f1: 2500.0,
                });
            }
        }
        // Diagnostic: open one effect editor with its default settings so the panel can be
        // rendered (and checked for crashes) without any audio file loaded.
        if let Some(i) = args.iter().position(|a| a == "--fxshot") {
            if let Some(name) = args.get(i + 1) {
                let key = name
                    .to_lowercase()
                    .replace(|c: char| !c.is_alphanumeric(), "_");
                let effect = crate::dsp::Effect::all_defaults().into_iter().find(|e| {
                    e.name()
                        .to_lowercase()
                        .replace(|c: char| !c.is_alphanumeric(), "_")
                        == key
                });
                match effect {
                    Some(effect) => {
                        app.dialog = Some(Dialog::Effect {
                            effect,
                            preview: false,
                        })
                    }
                    None => eprintln!("warning: no built-in effect named '{name}'"),
                }
            }
        }
        // Diagnostic: draw one of the creation dialogs for a screenshot check.
        if let Some(i) = args.iter().position(|a| a == "--shot-dialog") {
            match args.get(i + 1).map(String::as_str) {
                Some(page) if page.starts_with("prefs:") => {
                    app.open_prefs(page.trim_start_matches("prefs:"));
                }
                Some("file") => {
                    app.dialog = Some(Dialog::NewFile {
                        name: "Untitled 1".into(),
                        rate: 48000,
                        channels: 2,
                        bits: 32,
                        make_default: false,
                    });
                }
                Some("session") => {
                    let d = app.new_session_dialog();
                    app.dialog = Some(d);
                }
                Some("close") => {
                    if app.files.is_empty() {
                        app.load_demo();
                    }
                    let items = app
                        .files
                        .iter()
                        .map(|f| crate::dock::CloseItem::File(f.id))
                        .take(2)
                        .collect();
                    app.close_queue = Some((items, false));
                }
                Some("prefs") => {
                    let cat = args
                        .get(i + 2)
                        .cloned()
                        .unwrap_or_else(|| "Multitrack".into());
                    app.open_prefs(&cat.replace('_', " "));
                }
                Some("speech") => {
                    app.dialog = Some(Dialog::Speech {
                        pitch: 0.0,
                        text: String::new(),
                        voice: String::new(),
                        rate: 0,
                        tab: 1,
                        tract: Default::default(),
                    })
                }
                Some("batch") => {
                    let _ = app.exec("file.batch_process", &cc.egui_ctx);
                }
                Some("help") => {
                    app.extra.help_query = "fade".into();
                    app.extra.help_topic = Some("help.search");
                }
                Some("window") => {
                    app.ws.layout.undock(crate::workspace::PanelId::Markers);
                }
                _ => {}
            }
        }
        if start_multi {
            app.switch_mode(Mode::Multitrack);
            if std::env::args().any(|a| a == "--mixer") {
                app.center_tab = CenterTab::Mixer;
            }
        }
        // Diagnostic: show the automation lanes and a few points so a screenshot can verify
        // the chevron's reworked purpose without changing the normal demo or saved sessions.
        if args.iter().any(|a| a == "--autoshot") {
            app.ensure_mixer_session();
            app.switch_mode(Mode::Multitrack);
            if let Some(s) = app.session.as_mut() {
                if let Some(t) = s.tracks.first_mut() {
                    t.show_vol_env = true;
                    t.height = t.height.max(120.0);
                    t.io.automation = 1;
                    t.vol_env = vec![(0.0, 0.0), (2.0, -6.0), (4.0, -12.0), (6.0, 0.0)];
                    // one curved keyframe, shown with its handles by the Direct Selection tool
                    t.auto.vol_h = vec![
                        Default::default(),
                        crate::curves::auto_handles(&t.vol_env, 1),
                        Default::default(),
                        Default::default(),
                    ];
                    let id = t.id;
                    if let Some(c) = t.clips.first_mut() {
                        c.fade_in = c.fade_in.max(0.8);
                        c.auto_in = false;
                        c.curves.fin_pts = vec![(0.35, 0.8)];
                    }
                    app.tool = Tool::Direct;
                    app.multi_drag
                        .key_sel
                        .only(crate::keys::EnvTarget::TrackVol(id), 1);
                }

                s.selected_clip = s.tracks.first().and_then(|t| t.clips.first().map(|c| c.id));
                s.selected_clips = s.selected_clip.into_iter().collect();
                s.master.show_vol_env = true;
                s.master.vol_env = vec![(0.0, 0.0), (3.0, -3.0), (6.0, 0.0)];
            }
        }
        // Diagnostic: pick one whole row of clips and one row's empty spans — exactly what
        // Shift+click picks with the Move tool — so a screenshot can verify how the whole
        // selection is drawn, without changing the normal demo or saved sessions.
        if args.iter().any(|a| a == "--selshot") {
            app.switch_mode(Mode::Multitrack);
            app.tool = Tool::Move;
            if let Some(s) = app.session.as_mut() {
                if let Some(id) = s.tracks.get(1).and_then(|t| t.clips.first().map(|c| c.id)) {
                    s.select_row_clips(id);
                }
                // a point inside the row's first empty span (Shift+click picks from there)
                let t = s
                    .gaps_on(0)
                    .first()
                    .map(|(a, b)| a + (b - a).min(0.05) * 0.5)
                    .unwrap_or(0.0);
                s.select_row_gaps(0, t);
            }
        }
        app
    }

    pub fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn load_demo(&mut self) {
        let rate = 48000;
        let mut d = AudioData::new(rate, 2, 0);
        let notes = [
            220.0, 277.18, 329.63, 440.0, 329.63, 277.18, 246.94, 293.66, 349.23, 415.30, 493.88,
            392.00,
        ];
        for (i, f) in notes.iter().enumerate() {
            let mut t = dsp::gen_tone(
                rate,
                2,
                0.75,
                *f,
                if i % 2 == 0 {
                    Wave::Sine
                } else {
                    Wave::Triangle
                },
                -6.0,
            );
            dsp::fade(&mut t, 0, 2000, true, FadeCurve::Linear);
            let n = t.frames();
            dsp::fade(&mut t, n / 3, n, false, FadeCurve::Logarithmic);
            d = d.inserted(d.frames(), &t);
        }
        let noise = dsp::gen_noise(rate, 2, d.duration() as f32, NoiseColor::Pink, -34.0);
        d = d.mixed(0, &noise, 1.0);
        let n = d.frames();
        dsp::apply_chain(
            &mut d,
            0,
            n,
            &[EffectSlot::new(Effect::Reverb {
                room: 0.5,
                damping: 0.5,
                width: 1.0,
                mix: 0.2,
            })],
        );
        let id = self.add_file("Demo Melody.wav".into(), None, d, 32, true, "New File");
        if let Some(f) = self.file_mut(id) {
            f.selection = Some((1.5, 3.0));
            f.cursor = 1.5;
            f.markers.push(Marker {
                name: "Marker 01".into(),
                time: 0.75,
                duration: 0.0,
            });
            f.markers.push(Marker {
                name: "Marker 02".into(),
                time: 4.5,
                duration: 0.0,
            });
        }
        let mut beat = AudioData::new(rate, 1, 0);
        for _ in 0..8 {
            let mut k = dsp::gen_tone(rate, 1, 0.5, 60.0, Wave::Sine, -3.0);
            let n = k.frames();
            dsp::fade(&mut k, 0, n, false, FadeCurve::Logarithmic);
            beat = beat.inserted(beat.frames(), &k);
        }
        let bid = self.add_file("Demo Beat.wav".into(), None, beat, 32, true, "New File");
        let mut s = Session::new("Untitled Session 1", rate, 4);
        let len = self.file(id).map(|f| f.data.duration()).unwrap_or(1.0);
        let blen = self.file(bid).map(|f| f.data.duration()).unwrap_or(1.0);
        let c1 = s.id();
        s.tracks[0].clips.push(crate::session::Clip {
            id: c1,
            file_id: id,
            name: "Demo Melody".into(),
            start: 0.5,
            offset: 0.0,
            len,
            gain_db: 0.0,
            muted: false,
            env: vec![(0.0, 0.0), (2.5, -8.0), (4.5, 0.0)],
            pan_env: vec![(0.0, 0.0), (2.0, -0.6), (4.0, 0.6)],
            rack: vec![],
            speed: 1.0,
            fx: Default::default(),
            auto_in: true,
            auto_out: true,
            curves: Default::default(),
            fade_in: 0.3,
            fade_out: 0.8,
        });
        let c2 = s.id();
        s.tracks[1].clips.push(crate::session::Clip {
            id: c2,
            file_id: bid,
            name: "Demo Beat".into(),
            start: 0.0,
            offset: 0.0,
            len: blen,
            gain_db: -2.0,
            muted: false,
            env: vec![],
            pan_env: vec![],
            rack: vec![],
            speed: 1.0,
            fx: Default::default(),
            auto_in: true,
            auto_out: true,
            curves: Default::default(),
            fade_in: 0.0,
            fade_out: 0.0,
        });
        let c3 = s.id();
        s.tracks[1].clips.push(crate::session::Clip {
            id: c3,
            file_id: bid,
            name: "Demo Beat".into(),
            start: blen + 0.5,
            offset: 0.0,
            len: blen,
            gain_db: -2.0,
            muted: false,
            env: vec![],
            pan_env: vec![],
            rack: vec![],
            speed: 1.0,
            fx: Default::default(),
            auto_in: true,
            auto_out: true,
            curves: Default::default(),
            fade_in: 0.0,
            fade_out: 0.0,
        });
        // Fit the timeline to the sample clips so the waveform fills the editor, as in
        // a session that has been opened and zoomed to the project.
        s.view = (0.0, 10.0);
        s.dirty = false;
        self.session_hist = vec![("New Session".into(), s.hist_state())];
        self.session_hist_pos = 0;
        self.session = Some(s);
        self.current = Some(id);
    }

    // ------------------------------------------------------------ files

    pub fn file(&self, id: u64) -> Option<&AudioFile> {
        self.files.iter().find(|f| f.id == id)
    }
    pub fn file_mut(&mut self, id: u64) -> Option<&mut AudioFile> {
        self.files.iter_mut().find(|f| f.id == id)
    }
    pub fn cur(&self) -> Option<&AudioFile> {
        self.current.and_then(|id| self.file(id))
    }
    pub fn cur_mut(&mut self) -> Option<&mut AudioFile> {
        let id = self.current?;
        self.file_mut(id)
    }

    pub fn add_file(
        &mut self,
        name: String,
        path: Option<PathBuf>,
        data: AudioData,
        bits: u16,
        float: bool,
        label: &str,
    ) -> u64 {
        let id = self.new_id();
        let f = AudioFile::new(id, name, path, data, bits, float, label);
        self.push_file(f)
    }

    /// Add a file whose audio and peaks were already prepared on a worker thread.
    pub fn add_prepared_file(
        &mut self,
        name: String,
        path: Option<PathBuf>,
        prepared: crate::imports::PreparedImport,
        label: &str,
    ) -> u64 {
        let id = self.new_id();
        let f = AudioFile::new_with_peaks(
            id,
            name,
            path,
            prepared.data,
            prepared.bits,
            prepared.float,
            label,
            prepared.peaks,
        );
        self.push_file(f)
    }

    fn push_file(&mut self, f: AudioFile) -> u64 {
        let id = f.id;
        self.files.push(f);
        self.current = Some(id);
        self.selected_file = Some(id);
        self.files_sel = vec![id];
        id
    }

    /// Files panel selection: plain click selects one file; Shift+click selects the range
    /// from the last selected file to this one; Ctrl+click toggles this file in the set.
    pub fn files_select(&mut self, id: u64, shift: bool, ctrl: bool) {
        let Some(pos) = self.files.iter().position(|f| f.id == id) else {
            return;
        };
        let anchor = self
            .selected_file
            .and_then(|a| self.files.iter().position(|f| f.id == a));
        if shift {
            match anchor {
                Some(a) => {
                    let (lo, hi) = (a.min(pos), a.max(pos));
                    self.files_sel = self.files[lo..=hi].iter().map(|f| f.id).collect();
                }
                None => self.files_sel = vec![id],
            }
        } else if ctrl {
            if self.files_sel.contains(&id) {
                self.files_sel.retain(|f| *f != id);
            } else {
                self.files_sel.push(id);
            }
        } else {
            self.files_sel = vec![id];
        }
        self.selected_file = if self.files_sel.is_empty() {
            None
        } else {
            Some(id)
        };
    }

    pub fn open_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new();
        for (name, exts) in App::media_dialog_filters() {
            dlg = dlg.add_filter(name, exts);
        }
        if let Some(files) = dlg.ask_files() {
            for p in files {
                self.open_path(&p);
            }
        }
    }

    /// Folders become the media inside them (sub-folders too, sorted by path); files stay as
    /// they are. Other files in a folder (images, documents...) are skipped.
    pub fn expand_folders(paths: Vec<PathBuf>) -> Vec<PathBuf> {
        fn walk(dir: &std::path::Path, depth: usize, out: &mut Vec<PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else {
                return;
            };
            let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
            entries.sort();
            for p in entries {
                if out.len() >= 2000 {
                    return;
                }
                if p.is_dir() {
                    if depth < 8 {
                        walk(&p, depth + 1, out);
                    }
                } else {
                    let ext = p
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_ascii_lowercase())
                        .unwrap_or_default();
                    if audio::is_audio_path(&p)
                        || crate::video::is_video_path(&p)
                        || matches!(ext.as_str(), "sesx" | "acsession" | "audiocraft")
                    {
                        out.push(p);
                    }
                }
            }
        }
        let mut out = Vec::new();
        for p in paths {
            if p.is_dir() {
                walk(&p, 0, &mut out);
            } else {
                out.push(p);
            }
        }
        out
    }

    pub fn open_path(&mut self, p: &std::path::Path) {
        self.open_path_impl(p, true);
    }

    /// Import through the Files/Favorites browser without activating the sound in an editor.
    /// Sessions and videos keep their normal special handling; audio never changes the current
    /// editor, view or transport, including when the same sound is dropped again.
    pub fn import_path_into_browser(&mut self, p: &std::path::Path) {
        self.open_path_impl(p, false);
    }

    fn open_path_impl(&mut self, p: &std::path::Path, activate_audio: bool) {
        if p.is_dir() {
            let found = App::expand_folders(vec![p.to_path_buf()]);
            if found.is_empty() {
                self.status = format!("No audio or video files in {}", p.display());
                return;
            }
            let n = found.len();
            for f in found {
                self.open_path_impl(&f, activate_audio);
            }
            self.status = format!(
                "Importing {n} files from {}",
                p.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.display().to_string())
            );
            return;
        }
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if matches!(ext.as_str(), "acsession" | "sesx" | "xml" | "audiocraft") {
            self.load_session(p);
            return;
        }
        // A video always goes back to the Video panel, even if its audio was imported before.
        if crate::video::is_video_path(p) {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Video".into());
            self.start_video_import(p.to_path_buf(), &name);
            return;
        }
        let known = self
            .files
            .iter()
            .find(|f| f.path.as_deref() == Some(p))
            .map(|f| f.id);
        if let Some(id) = known {
            if activate_audio {
                self.current = Some(id);
                self.switch_mode(Mode::Waveform);
            } else {
                self.files_select(id, false, false);
            }
            return;
        }
        if activate_audio && self.video.path() != Some(p) {
            self.video = crate::video::VideoState::Empty;
            self.video_tex = None;
        }
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".into());
        // The file appears at once so the panel and a multitrack drop have something to point at.
        // Its audio and peaks arrive from the worker in tick_imports; nothing heavy runs here.
        let previous_current = self.current;
        let id = self.add_file(
            name.clone(),
            Some(p.to_path_buf()),
            AudioData::new(48000, 1, 0),
            32,
            true,
            "Importing...",
        );
        if !activate_audio {
            self.current = previous_current;
        }
        if self.first_import_dir.is_none() {
            if let Some(d) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                self.first_import_dir = Some(d.to_path_buf());
            }
        }
        self.status = format!("Importing {name} — decoding runs in the background");
        self.imports.start(id, p.to_path_buf(), None);
        if activate_audio && self.mode == Mode::Multitrack && self.session.is_none() {
            self.mode = Mode::Waveform;
        }
    }

    /// Finish background audio imports. Decoding and the peak pyramid were built on a worker,
    /// so this only swaps in ready data and updates the status bar.
    pub fn tick_imports(&mut self, ctx: &egui::Context) {
        if !self.imports.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        }
        for event in self.imports.drain() {
            let name = self
                .file(event.id)
                .map(|f| f.name.clone())
                .unwrap_or_default();
            match event.result {
                Ok(prepared) => {
                    if let Some(f) = self.file_mut(event.id) {
                        let data = Arc::new(prepared.data);
                        let ptr = Arc::as_ptr(&data) as usize;
                        let dur = data.duration();
                        let nch = data.channels();
                        f.data = data.clone();
                        f.peaks = prepared.peaks;
                        f.peaks_ptr = ptr;
                        f.version += 1;
                        f.bits = prepared.bits;
                        f.float = prepared.float;
                        f.enabled_ch = vec![true; nch];
                        f.view = (0.0, dur.max(0.001));
                        f.cursor = 0.0;
                        f.history = vec![("Open".to_string(), data)];
                        f.hist_pos = 0;
                        f.saved_pos = Some(0);
                        if event
                            .path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.eq_ignore_ascii_case("wav"))
                            .unwrap_or(false)
                        {
                            f.bwf = audio::has_bext(&event.path);
                            f.meta = audio::read_wav_meta(&event.path);
                        }
                    }
                    self.status = format!("Opened {name}");
                    self.add_recent(&event.path);
                    let placements: Vec<(usize, f64)> = self
                        .pending_placements
                        .iter()
                        .filter(|(id, _, _)| *id == event.id)
                        .map(|(_, track, at)| (*track, *at))
                        .collect();
                    self.pending_placements.retain(|(id, _, _)| *id != event.id);
                    for (track, at) in placements {
                        self.place_file(event.id, track, at);
                    }
                }
                Err(e) => {
                    self.files.retain(|f| f.id != event.id);
                    if self.current == Some(event.id) {
                        self.current = self.files.last().map(|f| f.id);
                    }
                    if self.selected_file == Some(event.id) {
                        self.selected_file = self.current;
                    }
                    self.files_sel.retain(|id| *id != event.id);
                    self.pending_placements.retain(|(id, _, _)| *id != event.id);
                    self.status = format!("Could not open {name}");
                    self.message(
                        "Could not open file",
                        &format!("{}\n\n{e}", event.path.display()),
                    );
                }
            }
        }
    }

    /// Pick video files and import them in the background.
    pub fn import_video_dialog(&mut self) {
        let files = rfd::FileDialog::new()
            .add_filter("Video files", crate::video::VIDEO_EXTS)
            .add_filter("All files", &["*"])
            .ask_files();
        if let Some(files) = files {
            for p in files {
                self.open_path(&p);
            }
        }
    }

    /// Start a streaming video import. The decoder is found without starting any process, the
    /// panel shows the clip as soon as its size is known, and nothing heavy runs here.
    pub fn start_video_import(&mut self, path: PathBuf, name: &str) {
        let exe = match crate::video::ensure_decoder(&self.prefs) {
            Ok(exe) => exe,
            Err(e) => {
                // a missing decoder must never look like a broken WAV
                self.video = crate::video::VideoState::Error(crate::video::VideoError {
                    path,
                    message: e.clone(),
                });
                self.video_tex = None;
                self.status = format!("Video not imported: {e}");
                self.show_panel(crate::workspace::PanelId::Video);
                return;
            }
        };
        self.video = crate::video::VideoState::Loading(path.clone());
        self.video_tex = None;
        self.status = format!("Importing video {name}...");
        let opts = crate::video::ImportOpts {
            quality: self.prefs.video_quality,
            ..Default::default()
        };
        self.video_queue
            .start(path, exe, opts, self.engine.ctx.clone());
        self.show_panel(crate::workspace::PanelId::Video);
    }

    /// Take what the video workers produced since last frame: the clip's size first, then its
    /// sound (already decoded with peaks) and thumbnails in small batches.
    pub fn tick_video_import(&mut self, ctx: &egui::Context) {
        use crate::video::{VideoMsg, VideoState};
        if self.video.is_loading() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        for event in self.video_queue.drain() {
            if self.video.path() != Some(event.path.as_path()) {
                continue;
            }
            let name = event
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Video".into());
            match event.msg {
                VideoMsg::Info(info, color) => {
                    let keep_frames = matches!(&self.video, VideoState::Ready(m) if m.converted);
                    if !keep_frames {
                        let mut m = crate::video::VideoMedia::new(event.path.clone(), info);
                        m.complete = false;
                        if let Some(why) = color.problem() {
                            if self.extra.color_asked.as_deref() != Some(event.path.as_path()) {
                                self.extra.color_asked = Some(event.path.clone());
                                self.dialog = Some(Dialog::ColorSpace {
                                    path: event.path.clone(),
                                    why,
                                });
                            }
                        }
                        m.color = color;
                        self.video = VideoState::Ready(Box::new(m));
                        self.video_tex = None;
                        self.status =
                            format!("Importing video {name}: decoding sound and preview...");
                    }
                }
                VideoMsg::Audio(prepared, has) => {
                    let label = if has { "Video" } else { "Video (no audio)" };
                    let cur = self
                        .files
                        .iter()
                        .position(|f| f.path.as_deref() == Some(event.path.as_path()));
                    if let Some(i) = cur {
                        self.files.remove(i);
                    }
                    self.add_prepared_file(name.clone(), Some(event.path.clone()), prepared, label);
                    if self.first_import_dir.is_none() {
                        if let Some(d) = event.path.parent().filter(|d| !d.as_os_str().is_empty()) {
                            self.first_import_dir = Some(d.to_path_buf());
                        }
                    }
                    self.add_recent(&event.path);
                    if self.mode == Mode::Multitrack && self.session.is_none() {
                        self.mode = Mode::Waveform;
                    }
                }
                VideoMsg::Frames(frames) => {
                    if let VideoState::Ready(m) = &mut self.video {
                        m.frames.extend(frames);
                    }
                }
                VideoMsg::Done(result) => {
                    let has_frames =
                        matches!(&self.video, VideoState::Ready(m) if !m.frames.is_empty());
                    if let VideoState::Ready(m) = &mut self.video {
                        m.complete = true;
                    }
                    match result {
                        Ok(()) => {
                            let ext = event
                                .path
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("video")
                                .to_ascii_uppercase();
                            self.status = format!("Opened video {name} ({ext})");
                        }
                        Err(e)
                            if has_frames
                                || self
                                    .files
                                    .iter()
                                    .any(|f| f.path.as_deref() == Some(event.path.as_path())) =>
                        {
                            self.status = e
                        }
                        Err(e) => {
                            self.video = VideoState::Error(crate::video::VideoError {
                                path: event.path.clone(),
                                message: e.clone(),
                            });
                            self.video_tex = None;
                            self.status = format!("Video import failed: {e}");
                        }
                    }
                }
            }
        }
    }

    /// Search the person's decoder folders and the system PATH, then remember what was found.
    /// If a video failed because no decoder was available, try importing it again.
    pub fn detect_video_decoder(&mut self) {
        let mut prefs = self
            .prefs_edit
            .as_ref()
            .map(|p| p.0.clone())
            .unwrap_or_else(|| self.prefs.clone());
        let found = match crate::video::configure_decoder(&mut prefs) {
            Ok(status) => {
                self.video_detector_status = status;
                true
            }
            Err(e) => {
                self.video_detector_status = e;
                false
            }
        };
        if let Some((p, _, _)) = &mut self.prefs_edit {
            *p = prefs.clone();
        } else {
            self.prefs = prefs.clone();
        }
        if found {
            self.retry_failed_video();
        }
    }

    /// After a decoder is found, retry a video that could not be opened before.
    fn retry_failed_video(&mut self) {
        let failed_path = match &self.video {
            crate::video::VideoState::Error(e) => Some(e.path.clone()),
            _ => None,
        };
        let Some(path) = failed_path else { return };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Video".into());
        self.start_video_import(path, &name);
    }

    pub fn message(&mut self, title: &str, body: &str) {
        self.dialog = Some(Dialog::Message(title.into(), body.into()));
    }

    /// Where new files and sessions are saved by default (Preferences > General).
    pub fn default_save_dir(&self) -> Option<PathBuf> {
        match self.prefs.default_save_mode {
            1 => self.first_import_dir.clone(),
            2 => self.last_saved_dir.clone(),
            _ => Some(if self.prefs.default_save_folder.is_empty() {
                crate::dialogs::default_session_folder().into()
            } else {
                PathBuf::from(&self.prefs.default_save_folder)
            }),
        }
    }

    /// Remember where a file was saved (the "Where the last file was saved" default).
    pub fn note_saved(&mut self, p: &std::path::Path) {
        if let Some(d) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
            self.last_saved_dir = Some(d.to_path_buf());
        }
    }

    pub fn save_file(&mut self, id: u64, save_as: bool) -> bool {
        let Some(f) = self.file(id) else { return false };
        // "Save" keeps the file's own format when it can be written losslessly; otherwise ask.
        let keep = f
            .path
            .as_ref()
            .filter(|p| {
                audio::SaveFormat::from_path(p)
                    .map(|s| !s.lossy())
                    .unwrap_or(false)
            })
            .cloned();
        let path = match (save_as, keep) {
            (false, Some(p)) => p,
            _ => {
                let stem = std::path::Path::new(&f.name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or("Untitled".into());
                let ext = f
                    .path
                    .as_ref()
                    .and_then(|p| {
                        audio::SaveFormat::from_path(p).map(|_| {
                            p.extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("wav")
                                .to_string()
                        })
                    })
                    .unwrap_or("wav".into());
                let mut dlg = rfd::FileDialog::new().set_file_name(format!("{stem}.{ext}"));
                if let Some(d) = self.default_save_dir() {
                    dlg = dlg.set_directory(d);
                }
                for (n, e) in audio::SAVE_FILTERS {
                    dlg = dlg.add_filter(*n, e);
                }
                match dlg.ask_save() {
                    Some(p) if audio::SaveFormat::from_path(&p).is_some() => p,
                    Some(p) => p.with_extension("wav"),
                    None => return false,
                }
            }
        };
        let (bits, float, bwf) = (f.bits, f.float, f.bwf);
        let meta = f.meta.clone();
        let data = f.data.clone();
        // like Audition: writing over a path that already exists asks first
        if (save_as || f.path.as_deref() != Some(path.as_path())) && path.exists() {
            self.dialog = Some(Dialog::ConfirmReplace { id, path });
            return false;
        }
        self.status = format!("Saving {}...", path.display());
        match audio::save_with_meta(&path, &data, bits, float, bwf, &meta) {
            Ok(()) => {
                let f = self.file_mut(id).unwrap();
                f.path = Some(path.clone());
                f.name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                f.saved_pos = Some(f.hist_pos);
                self.note_saved(&path);
                self.status = format!("Saved {}", path.display());
                self.add_recent(&path);
                true
            }
            Err(e) => {
                self.message("Could not save", &e);
                false
            }
        }
    }

    /// Play a file from the Files panel (from its start; loops when Loop is on). This is a
    /// separate preview player: it never sets the editor's source, position or playhead.
    pub fn start_file_preview(&mut self, id: u64) {
        let Some(data) = self.file(id).map(|f| f.data.clone()) else {
            return;
        };
        if data.frames() == 0 {
            return;
        }
        self.engine.ensure_output();
        {
            let mut s = self.engine.lock();
            s.files_preview = Some(crate::engine::FilePreview::new(data, self.files_loop));
        }
        self.files_preview = Some(id);
    }

    pub fn stop_file_preview(&mut self) {
        self.files_preview = None;
        self.engine.lock().files_preview = None;
    }

    pub fn close_file(&mut self, id: u64) {
        if self.files_preview == Some(id) {
            self.stop_file_preview();
        }

        if let Some(s) = &self.session {
            if s.tracks
                .iter()
                .any(|t| t.clips.iter().any(|c| c.file_id == id))
            {
                self.message("File in use", "This file is used by clips in the multitrack session. Remove those clips first.");
                return;
            }
        }
        self.stop();
        self.files.retain(|f| f.id != id);
        self.files_sel.retain(|f| *f != id);
        if self.current == Some(id) {
            self.current = self.files.last().map(|f| f.id);
        }
        if self.selected_file == Some(id) {
            self.selected_file = self.current;
        }
    }

    // ------------------------------------------------------------ editing

    /// Apply an in-place process to the selection (or entire file) of the current file.
    pub fn process(&mut self, label: &str, f: impl FnOnce(&mut AudioData, usize, usize)) {
        let Some(file) = self.cur_mut() else { return };
        let (a, b) = file.range();
        let mut d = (*file.data).clone();
        f(&mut d, a, b);
        // only the enabled channels change (Edit > Enable Channels)
        if d.frames() == file.data.frames() && d.channels() == file.data.channels() {
            for (ci, on) in file.enabled_ch.iter().enumerate() {
                if !*on {
                    if let (Some(dst), Some(src)) = (d.ch.get_mut(ci), file.data.ch.get(ci)) {
                        dst.copy_from_slice(src);
                    }
                }
            }
        }
        file.push(label, d);
        self.status = format!("{label} applied");
    }

    /// Replace the current file's data through a function that returns new data.
    pub fn transform(
        &mut self,
        label: &str,
        f: impl FnOnce(&AudioData, usize, usize) -> AudioData,
    ) {
        let Some(file) = self.cur_mut() else { return };
        let (a, b) = file.range();
        let d = f(&file.data, a, b);
        file.push(label, d);
        self.status = format!("{label} applied");
    }

    pub fn undo(&mut self) {
        if self.mode == Mode::Multitrack && self.session.is_some() {
            if self.session_hist_pos > 0 {
                self.session_hist_pos -= 1;
                self.restore_session_hist();
            }
            return;
        }
        if let Some(f) = self.cur_mut() {
            if f.hist_pos > 0 {
                let p = f.hist_pos - 1;
                let label = f.history[f.hist_pos].0.clone();
                f.goto_history(p);
                self.status = format!("Undo {label}");
            }
        }
    }
    pub fn redo(&mut self) {
        if self.mode == Mode::Multitrack && self.session.is_some() {
            if self.session_hist_pos + 1 < self.session_hist.len() {
                self.session_hist_pos += 1;
                self.restore_session_hist();
            }
            return;
        }
        if let Some(f) = self.cur_mut() {
            if f.hist_pos + 1 < f.history.len() {
                let p = f.hist_pos + 1;
                f.goto_history(p);
                let l = f.history[p].0.clone();
                self.status = format!("Redo {l}");
            }
        }
    }

    pub fn session_push(&mut self, label: &str) {
        if let Some(s) = &mut self.session {
            s.touch();
            self.session_hist.truncate(self.session_hist_pos + 1);
            self.session_hist.push((label.to_string(), s.hist_state()));
            self.session_hist_pos = self.session_hist.len() - 1;
            if self.session_hist.len() > 100 {
                self.session_hist.remove(0);
                self.session_hist_pos -= 1;
            }
        }
    }
    pub fn restore_session_hist(&mut self) {
        if let (Some(s), Some((_, t))) = (
            &mut self.session,
            self.session_hist.get(self.session_hist_pos),
        ) {
            s.set_hist_state(t);
            self.session_hist_seen = s.revision;
        }
    }

    /// The undo safety net: once no mouse button is held, a session that differs from its
    /// current undo step gets a step of its own. An edit that forgot to record itself (adding
    /// an effect automation lane, a master keyframe...) used to fold into the next step, so one
    /// Ctrl+Z undid two things, or the edit could not be undone at all.
    pub fn session_hist_catch_up(&mut self, pointer_down: bool) {
        let Some(s) = &self.session else { return };
        if pointer_down || s.revision == self.session_hist_seen {
            return;
        }
        self.session_hist_seen = s.revision;
        let same = self
            .session_hist
            .get(self.session_hist_pos)
            .is_some_and(|(_, h)| *h == s.hist_state());
        if !same && !self.session_hist.is_empty() {
            self.session_push("Edit");
            if let Some(s) = &self.session {
                self.session_hist_seen = s.revision;
            }
        }
    }

    pub fn copy(&mut self) {
        if self.mode == Mode::Multitrack {
            // the selected clip, with its fades, envelopes and effects
            if let Some(s) = &self.session {
                if let Some((t, c)) = s.selected_clip.and_then(|id| s.find_clip(id)) {
                    self.clip_clipboard = Some(s.tracks[t].clips[c].clone());
                    self.status = "Clip copied".into();
                }
            }
            return;
        }
        if let Some(f) = self.cur() {
            if f.selection.is_some() {
                let (a, b) = f.range();
                self.clipboard = Some(f.data.slice(a, b));
                self.status = "Copied".into();
            }
        }
    }
    pub fn cut(&mut self) {
        if self.mode == Mode::Multitrack {
            self.copy();
            self.delete_active_clip();
            return;
        }
        if self.cur().map(|f| f.selection.is_some()).unwrap_or(false) {
            self.copy();
            self.delete_selection("Cut");
        }
    }
    pub fn delete_selection(&mut self, label: &str) {
        if self.mode == Mode::Multitrack {
            self.delete_selected_clip();
            return;
        }
        let Some(f) = self.cur_mut() else { return };
        if f.selection.is_none() {
            return;
        }
        let (a, b) = f.range();
        let d = f.data.without(a, b);
        let t = a as f64 / f.data.rate as f64;
        f.selection = None;
        f.cursor = t;
        f.push(label, d);
    }
    pub fn paste(&mut self, mix: bool) {
        if self.mode == Mode::Multitrack {
            // the copied clip goes onto the selected track at the playhead
            let Some(mut c) = self.clip_clipboard.clone() else {
                return;
            };
            let Some(s) = self.session.as_mut() else {
                return;
            };
            if s.tracks.is_empty() {
                s.add_track(true);
            }
            let t = s.selected_track.min(s.tracks.len() - 1);
            c.id = s.id();
            c.start = s.cursor.max(0.0);
            let end = c.end();
            s.tracks[t].clips.push(c.clone());
            s.selected_clip = Some(c.id);
            // the playhead moves to the end of the pasted clip, so pasting again lines them up
            s.cursor = end;
            s.touch();
            self.session_push("Paste Clip");
            return;
        }
        let Some(clip) = self.clipboard.clone() else {
            return;
        };
        if self.cur().is_none() {
            self.add_file(
                "Untitled Paste.wav".into(),
                None,
                clip,
                32,
                true,
                "Paste to New",
            );
            return;
        }
        let f = self.cur_mut().unwrap();
        let r = f.data.rate as f64;
        let at = match f.selection {
            Some((a, _)) if !mix => (a * r) as usize,
            _ => (f.cursor * r) as usize,
        };
        let d = if mix {
            f.data.mixed(at, &clip, 1.0)
        } else {
            let base = match f.selection {
                Some(_) => {
                    let (a, b) = f.range();
                    f.data.without(a, b)
                }
                None => (*f.data).clone(),
            };
            base.inserted(at, &clip)
        };
        let len = clip.duration();
        let start = at as f64 / r;
        f.selection = Some((start, start + len));
        f.push(if mix { "Mix Paste" } else { "Paste" }, d);
    }
    pub fn paste_new(&mut self) {
        if let Some(c) = self.clipboard.clone() {
            let n = self.files.len() + 1;
            self.add_file(
                format!("Untitled {n}.wav"),
                None,
                c,
                32,
                true,
                "Paste to New",
            );
        }
    }
    pub fn crop(&mut self) {
        let Some(f) = self.cur_mut() else { return };
        if f.selection.is_none() {
            return;
        }
        let (a, b) = f.range();
        let d = f.data.slice(a, b);
        f.selection = None;
        f.cursor = 0.0;
        f.view = (0.0, d.duration().max(0.001));
        f.push("Crop", d);
    }
    pub fn select_all(&mut self) {
        match self.mode {
            Mode::Waveform => {
                if let Some(f) = self.cur_mut() {
                    f.selection = Some((0.0, f.data.duration()));
                }
            }
            Mode::Multitrack => {
                if let Some(s) = &mut self.session {
                    let l = s.length();
                    s.selection = Some((0.0, l));
                }
            }
        }
    }
    pub fn select_none(&mut self) {
        if let Some(f) = self.cur_mut() {
            f.selection = None;
        }
        if let Some(s) = &mut self.session {
            s.selection = None;
            s.selected_clip = None;
            s.selected_clips.clear();
            s.selected_gaps.clear();
        }
    }

    pub fn add_marker(&mut self) {
        match self.mode {
            Mode::Waveform => {
                let name = self
                    .cur()
                    .map(|f| self.prefs.marker_name(f.markers.len() + 1))
                    .unwrap_or_default();
                if let Some(f) = self.cur_mut() {
                    let (t, dur) = match f.selection {
                        Some((a, b)) if b > a => (a, b - a),
                        _ => (f.cursor, 0.0),
                    };
                    f.markers.push(Marker {
                        name,
                        time: t,
                        duration: dur,
                    });
                    f.markers.sort_by(|a, b| a.time.total_cmp(&b.time));
                }
            }
            Mode::Multitrack => {
                let name = self
                    .session
                    .as_ref()
                    .map(|s| self.prefs.marker_name(s.markers.len() + 1))
                    .unwrap_or_default();
                if let Some(s) = &mut self.session {
                    s.markers.push(Marker {
                        name,
                        time: s.cursor,
                        duration: 0.0,
                    });
                    s.markers.sort_by(|a, b| a.time.total_cmp(&b.time));
                    s.dirty = true;
                }
            }
        }
    }

    pub fn delete_selected_clip(&mut self) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let ids: Vec<u64> = if s.selected_clips.is_empty() {
            s.selected_clip.into_iter().collect()
        } else {
            s.selected_clips.clone()
        };
        let gaps = std::mem::take(&mut s.selected_gaps);
        let mut n = 0;
        if !gaps.is_empty() {
            n += crate::filecmds::ripple_delete_gaps(s, &gaps);
        }
        if !ids.is_empty() {
            n += crate::filecmds::delete_clips_plain(s, &ids);
        }
        if n > 0 {
            let label = if ids.len() > 1 {
                "Delete Clips"
            } else {
                "Delete Clip"
            };
            self.session_push(label);
        }
    }

    /// Cut's other half: the clip picked last goes — the one that was copied (the clip
    /// clipboard holds a single clip).
    pub fn delete_active_clip(&mut self) {
        let Some(s) = &mut self.session else { return };
        let Some(id) = s.selected_clip else { return };
        if let Some((t, c)) = s.find_clip(id) {
            s.tracks[t].clips.remove(c);
            s.selected_clip = None;
            self.session_push("Delete Clip");
        }
    }

    /// Shift+Delete / Shift+Backspace in the Multitrack editor: ripple delete. Every
    /// picked clip goes and the gap it leaves closes — the later clips of its track pull
    /// left — and every picked empty span closes the same way.
    pub fn ripple_delete_selection(&mut self) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let ids: Vec<u64> = if s.selected_clips.is_empty() {
            s.selected_clip.into_iter().collect()
        } else {
            s.selected_clips.clone()
        };
        let gaps = std::mem::take(&mut s.selected_gaps);
        let mut n = 0;
        if !gaps.is_empty() {
            n += crate::filecmds::ripple_delete_gaps(s, &gaps);
        }
        if !ids.is_empty() {
            n += crate::filecmds::ripple_delete_clips(s, &ids);
        }
        if n > 0 {
            let label = if ids.len() > 1 {
                "Ripple Delete Clips"
            } else {
                "Ripple Delete"
            };
            self.session_push(label);
        }
    }

    pub fn split_at_playhead(&mut self) {
        let Some(s) = &mut self.session else { return };
        let t = s.cursor;
        let ids: Vec<u64> = match s.selected_clip {
            Some(id) => vec![id],
            None => s
                .tracks
                .iter()
                .flat_map(|tr| tr.clips.iter())
                .filter(|c| c.start < t && c.end() > t)
                .map(|c| c.id)
                .collect(),
        };
        let mut any = false;
        for id in ids {
            any |= s.split_clip(id, t).is_some();
        }
        if any {
            self.session_push("Split");
        }
    }

    // ------------------------------------------------------------ zoom

    pub fn view_mut(&mut self) -> Option<(&mut (f64, f64), f64)> {
        match self.mode {
            Mode::Waveform => {
                let f = self.cur_mut()?;
                let d = f.data.duration();
                Some((&mut f.view, d))
            }
            Mode::Multitrack => {
                let s = self.session.as_mut()?;
                let d = (s.length() + 10.0).max(30.0);
                Some((&mut s.view, d))
            }
        }
    }
    pub fn zoom(&mut self, factor: f64, center: Option<f64>) {
        if let Some((v, dur)) = self.view_mut() {
            let c = center.unwrap_or((v.0 + v.1) * 0.5);
            let old = (v.1 - v.0).max(1e-9);
            let w = (old * factor).clamp(0.0005, dur.max(0.001));
            let mut a = c - (c - v.0) * (w / old);
            a = a.clamp(0.0, (dur - w).max(0.0));
            *v = (a, a + w);
        }
    }
    pub fn zoom_full(&mut self) {
        if let Some((v, dur)) = self.view_mut() {
            *v = (0.0, dur.max(0.001));
        }
    }
    pub fn zoom_selection(&mut self) {
        let sel = match self.mode {
            Mode::Waveform => self.cur().and_then(|f| f.selection),
            Mode::Multitrack => self.session.as_ref().and_then(|s| s.selection),
        };
        if let (Some((a, b)), Some((v, _))) = (sel, self.view_mut()) {
            if b > a {
                *v = (a, b);
            }
        }
    }

    // ------------------------------------------------------------ transport

    pub fn is_playing(&self) -> bool {
        self.engine.lock().playing
    }
    pub fn is_recording(&self) -> bool {
        self.engine.lock().recording
    }

    pub fn cursor(&self) -> f64 {
        match self.mode {
            Mode::Waveform => self.cur().map(|f| f.cursor).unwrap_or(0.0),
            Mode::Multitrack => self.session.as_ref().map(|s| s.cursor).unwrap_or(0.0),
        }
    }
    pub fn set_cursor(&mut self, t: f64) {
        let playing = self.is_playing();
        match self.mode {
            Mode::Waveform => {
                if let Some(f) = self.cur_mut() {
                    f.cursor = t.clamp(0.0, f.data.duration());
                }
            }
            Mode::Multitrack => {
                if let Some(s) = &mut self.session {
                    s.cursor = t.max(0.0);
                }
            }
        }
        if playing {
            let c = self.cursor();
            self.engine.lock().pos = c;
            self.play_origin = c;
        }
    }

    /// Apply a spectral edit to the current file's spectral selection (undoable). False when
    /// there is no spectral selection.
    pub fn spectral_apply(&mut self, op: crate::spectral::SpecOp, label: &str) -> bool {
        let Some(f) = self.cur_mut() else {
            return false;
        };
        let Some(shape) = f.spec_sel.clone() else {
            return false;
        };
        if shape.is_empty() {
            return false;
        }
        let mut d = (*f.data).clone();
        let chans = f.enabled_ch.clone();
        crate::spectral::apply(&mut d, &shape, &chans, op);
        f.push(label, d);
        self.status = label.to_string();
        true
    }

    /// Scrub: while stopped, dragging the playhead plays the audio under it. `None` ends it.
    pub fn scrub_to(&mut self, t: Option<f64>) {
        if self.is_playing() {
            return;
        }
        if t.is_some() {
            self.engine.ensure_output();
        }
        let mut s = self.engine.lock();
        if let (None, Some(t)) = (s.scrub, t) {
            // a new spot: nothing from before (effect tails) may sound here
            s.pos = t;
            s.flush_effects();
        }
        s.scrub = t;
    }

    /// Start from clean level readings: a new take must not inherit a previous file's peak.
    pub fn reset_level_meters(&mut self) {
        self.engine.reset_meters();
        self.meter_hold = [0.0; 2];
        self.meter_hold_time = [0.0; 2];
        self.meter_clip = [false; 2];
        crate::fx3::reset_loudness();
    }

    pub fn play(&mut self) {
        // Record was pressed first: Play starts the recording (see extras::toggle_record)
        if self.extra.record_ready && !self.is_recording() {
            self.extra.record_ready = false;
            if !self.begin_recording() || self.mode == Mode::Waveform || self.is_playing() {
                return;
            }
        }
        self.engine.lock().scrub = None;
        self.reset_level_meters();
        self.engine.ensure_output();
        let looping = self.engine.lock().looping;
        let recording = self.is_recording();
        match self.mode {
            Mode::Waveform => {
                let Some(f) = self.cur() else { return };
                let dur = f.data.duration();
                let (start, end, ls) = match f.selection {
                    Some((a, b)) if b - a > 0.0005 && f.cursor >= a - 1e-6 && f.cursor < b => {
                        (f.cursor, b, a)
                    }
                    _ => {
                        let st = if f.cursor >= dur - 0.001 {
                            0.0
                        } else {
                            f.cursor
                        };
                        (st, dur, if looping { 0.0 } else { st })
                    }
                };
                let data = f.data.clone();
                let rack = (
                    f.rack.clone(),
                    f.rack_in_db,
                    f.rack_out_db,
                    f.rack_mix,
                    f.rack_power,
                );
                let mut s = self.engine.lock();
                s.source = Source::File(data);
                s.set_rack(&rack.0, rack.1, rack.2, rack.3, rack.4);
                if (s.pos - start).abs() > 0.01 {
                    s.flush_effects();
                }
                s.pos = start;
                s.end = end;
                s.loop_start = ls;
                s.playing = true;
                drop(s);
                self.play_origin = start;
            }
            Mode::Multitrack => {
                self.refresh_mix(true);
                let Some(sess) = &self.session else { return };
                let len = sess.length().max(0.1);
                let (start, end, ls) = match sess.selection {
                    Some((a, b))
                        if b - a > 0.001
                            && sess.cursor >= a - 1e-6
                            && sess.cursor < b
                            && !recording =>
                    {
                        (sess.cursor, b, a)
                    }
                    _ => {
                        let st = if sess.cursor >= len && !recording {
                            0.0
                        } else {
                            sess.cursor
                        };
                        // like the original, Multitrack playback runs on past the last clip
                        // (effect tails ring out) until stopped; a loop covers the session
                        (
                            st,
                            if looping && !recording { len } else { 1e9 },
                            if looping { 0.0 } else { st },
                        )
                    }
                };
                let mut s = self.engine.lock();
                if (s.pos - start).abs() > 0.01 {
                    s.flush_effects();
                }
                s.pos = start;
                s.end = end;
                s.loop_start = ls;
                s.playing = true;
                drop(s);
                self.play_origin = start;
            }
        }
        self.was_playing = true;
    }

    pub fn stop(&mut self) {
        self.extra.record_ready = false;
        if self.is_recording() {
            self.finish_recording();
        }
        self.extra.live = Default::default();

        let was = {
            let mut s = self.engine.lock();
            let was = s.playing;
            s.playing = false;
            s.preview = None;
            s.scrub = None;
            // the effects' tails belong to the audio that was playing: drop them
            s.flush_effects();
            was
        };
        self.reset_level_meters();
        if was && self.return_on_stop {
            let o = self.play_origin;
            self.set_mode_cursor(o);
        }
        self.was_playing = false;
    }

    fn set_mode_cursor(&mut self, t: f64) {
        match self.mode {
            Mode::Waveform => {
                if let Some(f) = self.cur_mut() {
                    f.cursor = t.min(f.data.duration());
                }
            }
            Mode::Multitrack => {
                if let Some(s) = &mut self.session {
                    s.cursor = t;
                }
            }
        }
    }

    pub fn pause(&mut self) {
        let paused_at = {
            let mut s = self.engine.lock();
            if s.playing {
                s.playing = false;
                Some(s.pos)
            } else {
                None
            }
        };
        match paused_at {
            Some(p) => {
                self.set_mode_cursor(p);
                self.was_playing = false;
            }
            None => self.play(),
        }
    }

    pub fn toggle_play(&mut self) {
        if self.is_playing() {
            self.stop()
        } else {
            self.play()
        }
    }

    /// Write a recorded take into the recordings folder (Preferences > Playback and Recording)
    /// as 32-bit float WAV under a name that doesn't overwrite an earlier take. Returns where it
    /// went; a failure is reported but the take stays open in AudioCraft.
    pub fn save_take(&mut self, take: &AudioData, stem: &str) -> Option<PathBuf> {
        let dir = self.prefs.recordings_dir();
        let path = match unique_take_path(&dir, stem) {
            Ok(p) => p,
            Err(e) => {
                self.message(
                    "Could not save the recording",
                    &format!(
                        "The recordings folder {} can't be used: {e}
The take is still open; save it with File > Save As.",
                        dir.display()
                    ),
                );
                return None;
            }
        };
        match audio::save_any(&path, take, 32, true, false) {
            Ok(()) => {
                self.status = format!("Recording saved to {}", path.display());
                Some(path)
            }
            Err(e) => {
                self.message(
                    "Could not save the recording",
                    &format!(
                        "{e}
The take is still open; save it with File > Save As."
                    ),
                );
                None
            }
        }
    }

    fn finish_recording(&mut self) {
        let Some(rec) = self.engine.stop_recording() else {
            return;
        };
        let Some((at, track)) = self.recording_at.take() else {
            return;
        };
        match (self.mode, track) {
            (Mode::Multitrack, Some(t)) => {
                let rate = self.session.as_ref().map(|s| s.rate).unwrap_or(48000);
                let d = rec.conform(rate, rec.channels().min(2));
                let len = d.duration();
                let tname = self
                    .session
                    .as_ref()
                    .and_then(|s| s.tracks.get(t))
                    .map(|t| t.name.clone())
                    .unwrap_or("Track".into());
                let n = self.files.len() + 1;
                let wanted = format!("{tname}_Recorded {n:03}");
                // the take goes to the recordings folder right away, so it is never lost
                let saved = self.save_take(&d, &wanted);
                let name = saved
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or(format!("{wanted}.wav"));
                let cur = self.current;
                let fid = self.add_file(name.clone(), saved, d, 32, true, "Record");
                self.current = cur.or(Some(fid));
                if let Some(s) = &mut self.session {
                    // every armed track gets the take
                    let mut targets: Vec<usize> = s
                        .tracks
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| t.arm && t.can_record())
                        .map(|(i, _)| i)
                        .collect();
                    if targets.is_empty() {
                        targets.push(t);
                    }
                    for t in targets {
                        let cid = s.id();
                        if let Some(tr) = s.tracks.get_mut(t) {
                            tr.clips.push(crate::session::Clip {
                                id: cid,
                                file_id: fid,
                                name: name.trim_end_matches(".wav").into(),
                                start: at,
                                offset: 0.0,
                                len,
                                gain_db: 0.0,
                                muted: false,
                                env: vec![],
                                pan_env: vec![],
                                rack: vec![],
                                speed: 1.0,
                                fx: Default::default(),
                                auto_in: true,
                                auto_out: true,
                                curves: Default::default(),
                                fade_in: 0.0,
                                fade_out: 0.0,
                            });
                            s.selected_clip = Some(cid);
                        }
                    }
                }
                self.session_push("Record");
            }
            _ => {
                let stem = self
                    .cur()
                    .map(|f| {
                        format!(
                            "{} take",
                            std::path::Path::new(&f.name)
                                .file_stem()
                                .map(|x| x.to_string_lossy().to_string())
                                .unwrap_or_default()
                        )
                    })
                    .unwrap_or_else(|| "Untitled Recording".into());
                let take = rec.conform(rec.rate, rec.channels().min(2));
                let saved = self.save_take(&take, &stem);
                if let Some(f) = self.cur_mut() {
                    let r = f.data.rate as f64;
                    let d = f.data.inserted((at * r) as usize, &rec);
                    let len = rec.duration();
                    f.selection = Some((at, at + len));
                    f.push("Record", d);
                } else {
                    let name = saved
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or("Untitled Recording.wav".into());
                    self.add_file(name, saved, take, 32, true, "Record");
                }
            }
        }
        if !self.status.starts_with("Recording saved") {
            self.status = "Recording finished".into();
        }
    }

    pub fn refresh_mix(&mut self, force: bool) {
        if !force && self.engine.lock().files_preview.is_some() {
            // the Files panel has its own preview player: don't disturb the session mix
            return;
        }
        let Some(s) = &self.session else { return };
        let fv: u64 = self.files.iter().map(|f| f.version).sum();
        let key = (s.revision, fv, self.files.len());
        let is_mix = matches!(self.engine.lock().source, Source::Mix(_));
        if !force && key == self.mix_key && is_mix {
            return;
        }
        self.mix_key = key;
        self.update_prerenders();
        let Some(s) = &self.session else { return };
        let files = &self.files;
        let pre = &self.prerender;
        let mix = s.snapshot_with(
            &|id| files.iter().find(|f| f.id == id).map(|f| f.data.clone()),
            &|tid| pre.get(&tid).map(|p| p.range()).filter(|r| !r.chunks.is_empty()),
        );
        let mut sh = self.engine.lock();
        if sh.playing && sh.end < 1e8 && !sh.looping {
            sh.end = sh.end.max(mix.length);
        }
        sh.source = Source::Mix(Arc::new(mix));
        sh.sync_mix_chains();
    }

    /// The content hash a pre-render belongs to: the track as played (minus fader, pan, mute
    /// and solo, which act after the render) and the files it plays.
    fn prerender_key(&self, t: &crate::session::Track) -> u64 {
        let mut tr = t.clone();
        tr.volume_db = 0.0;
        tr.pan = 0.0;
        tr.mute = false;
        tr.solo = false;
        tr.name.clear();
        let fv: u64 = self.files.iter().map(|f| f.version).sum();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&serde_json::to_string(&tr).unwrap_or_default(), &mut h);
        std::hash::Hash::hash(&fv, &mut h);
        std::hash::Hasher::finish(&h)
    }

    /// Forget pre-renders that no longer match their track (an edit), or whose track has
    /// Pre-render off, so nothing ever plays stale audio. New renders start in
    /// [`App::prerender_tick`].
    pub fn update_prerenders(&mut self) {
        let Some(s) = &self.session else {
            self.prerender.clear();
            return;
        };
        let want: std::collections::HashMap<u64, u64> = s
            .tracks
            .iter()
            .filter(|t| t.prerender)
            .map(|t| (t.id, self.prerender_key(t)))
            .collect();
        self.prerender
            .retain(|id, c| want.get(id).is_some_and(|k| *k == c.key));
    }

    /// Does the track have anything that rings on (track, EQ or clip effects)?
    fn prerender_has_fx(t: &crate::session::Track) -> bool {
        t.rack.iter().any(|e| e.enabled)
            || (t.eq_on && !t.eq.is_empty())
            || t.clips.iter().any(|c| c.rack.iter().any(|e| e.enabled))
    }

    /// Start a track's render at `from` (session seconds), with a little pre-roll so its
    /// effects already ring when the kept audio begins.
    fn prerender_start(&self, t: &crate::session::Track, key: u64, from: f64) -> Option<PreCache> {
        let s = self.session.as_ref()?;
        let mut tr = t.clone();
        // the render holds clips, clip effects and track effects; the fader, pan, their
        // automation and mute act on it afterwards, live
        tr.volume_db = 0.0;
        tr.pan = 0.0;
        tr.mute = false;
        tr.solo = false;
        tr.prerender = false;
        tr.fx.pre_fader = true;
        tr.vol_env.clear();
        tr.auto.pan_env.clear();
        tr.auto.params.retain(|p| p.key != "mute");
        let has_fx = Self::prerender_has_fx(&tr);
        let content_end = tr.clips.iter().map(|c| c.end()).fold(0.0, f64::max);
        let mut one = s.clone();
        one.tracks = vec![tr];
        one.master_db = 0.0;
        one.master.mute = false;
        one.master.rack.clear();
        one.master.vol_env.clear();
        one.metronome.on = false;
        let files = &self.files;
        let mix = one.snapshot(&|id| files.iter().find(|f| f.id == id).map(|f| f.data.clone()));
        let rate = s.rate.max(1);
        let max_tail = if has_fx {
            self.prefs.prerender_max_tail_s.max(0.0) as f64
        } else {
            0.0
        };
        let from = from.max(0.0);
        let preroll_frames = if has_fx {
            (from.min(2.0) * rate as f64).round() as usize
        } else {
            0
        };
        let a = from - preroll_frames as f64 / rate as f64;
        let render = crate::session::MixRender::new(&mix, rate, a, content_end + max_tail);
        Some(PreCache {
            key,
            mix,
            render,
            rate,
            start: a + preroll_frames as f64 / rate as f64,
            skip: preroll_frames,
            chunks: vec![],
            pending: [vec![], vec![]],
            content_end,
            end: content_end + max_tail,
            tail_done: false,
            quiet: 0,
            has_fx,
            window: (from, from),
        })
    }

    /// The stretch each pre-rendered track should hold now: the whole track when it fits its
    /// share of the cache cap, otherwise a window around the playhead (ahead / behind from
    /// Preferences), never longer than the share.
    fn prerender_window(&self, c_end: f64, playhead: f64, tracks: usize) -> (f64, f64, bool) {
        let rate = self.session.as_ref().map(|s| s.rate).unwrap_or(48000).max(1) as f64;
        let share_bytes =
            (self.prefs.prerender_cap_mb.max(1.0) as f64 * 1024.0 * 1024.0) / tracks.max(1) as f64;
        let share_secs = share_bytes / (rate * 8.0);
        if c_end <= share_secs {
            return (0.0, c_end, true);
        }
        let ahead = self.prefs.prerender_ahead_s.max(0.5) as f64;
        let behind = self.prefs.prerender_behind_s.max(0.0) as f64;
        let mut a = (playhead - behind).max(0.0);
        let mut b = (playhead + ahead).min(c_end);
        if b - a > share_secs {
            // the share is smaller than ahead + behind: give each side its part
            let k = share_secs / (ahead + behind).max(1e-9);
            a = (playhead - behind * k).max(0.0);
            b = (a + share_secs).min(c_end);
        }
        (a, b.max(a), false)
    }

    /// Advance the pre-renders for about `budget`: each follows the playhead (or fills the
    /// whole track when it fits the cap), renders past the last clip until its effects fall
    /// silent, and hands new audio to the playing mix. True while any render has work left.
    pub fn prerender_tick(&mut self, budget: std::time::Duration) -> bool {
        self.update_prerenders();
        let Some(s) = self.session.as_ref() else {
            return false;
        };
        let tracks: Vec<crate::session::Track> =
            s.tracks.iter().filter(|t| t.prerender).cloned().collect();
        if tracks.is_empty() {
            return false;
        }
        let playhead = s.cursor.max(0.0);
        let n = tracks.len();
        // plan: (re)start caches and set each one's window
        for t in &tracks {
            let key = self.prerender_key(t);
            let c_end = match self.prerender.get(&t.id) {
                Some(c) => c.end,
                None => {
                    let ce = t.clips.iter().map(|c| c.end()).fold(0.0, f64::max);
                    ce + if Self::prerender_has_fx(t) {
                        self.prefs.prerender_max_tail_s.max(0.0) as f64
                    } else {
                        0.0
                    }
                }
            };
            let (wa, wb, whole) = self.prerender_window(c_end, playhead, n);
            let restart = match self.prerender.get(&t.id) {
                None => true,
                Some(c) => {
                    // the playhead left the rendered stretch (a jump back before it, or
                    // forward past what is rendered): start over where it is now
                    (!whole && (playhead < c.start - 1e-3 || playhead > c.rendered_to() + 1.0))
                        || (whole && c.start > 1e-6)
                }
            };
            if restart {
                if let Some(c) = self.prerender_start(t, key, wa) {
                    self.prerender.insert(t.id, c);
                }
            }
            if let Some(c) = self.prerender.get_mut(&t.id) {
                c.window = (wa, wb);
                if !whole {
                    c.trim_before(wa);
                }
            }
        }
        // render, a block at a time per track in turn, until the budget is used
        let t0 = std::time::Instant::now();
        let mut ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        let mut grew = false;
        loop {
            let mut any = false;
            for id in &ids {
                let Some(c) = self.prerender.get_mut(id) else {
                    continue;
                };
                if !c.wants_more() {
                    continue;
                }
                any = true;
                grew |= c.step();
                if t0.elapsed() >= budget {
                    break;
                }
            }
            if !any || t0.elapsed() >= budget {
                break;
            }
        }
        let busy = self.prerender.values().any(|c| c.wants_more());
        // hand the new audio to playback (at most ten times a second while rendering)
        let due = self
            .prerender_pushed
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_millis(100));
        if grew && (due || !busy) {
            self.prerender_pushed = Some(std::time::Instant::now());
            self.mix_key.0 = u64::MAX;
            self.refresh_mix(false);
            if !busy {
                self.status = "Track pre-rendered".into();
            }
        }
        busy
    }

    /// Run the pre-renders until they have nothing left to do at the current playhead.
    pub fn finish_prerenders(&mut self) {
        let mut guard = 0;
        while self.prerender_tick(std::time::Duration::from_secs(3600)) && guard < 1000 {
            guard += 1;
        }
        // make sure playback has the last piece
        self.mix_key.0 = u64::MAX;
        self.refresh_mix(false);
    }

    /// What the pre-render gauge shows: `None` when no track has Pre-render on.
    pub fn prerender_gauge(&self) -> Option<PrerenderGauge> {
        let s = self.session.as_ref()?;
        let ids: Vec<u64> = s
            .tracks
            .iter()
            .filter(|t| t.prerender)
            .map(|t| t.id)
            .collect();
        if ids.is_empty() {
            return None;
        }
        let caches: Vec<&PreCache> = ids.iter().filter_map(|id| self.prerender.get(id)).collect();
        // the stretch every pre-rendered track has ready, and the stretch they aim for
        let (mut ra, mut rb) = (f64::NEG_INFINITY, f64::INFINITY);
        let (mut wa, mut wb) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut done_n, mut total_n) = (0.0f64, 0.0f64);
        let mut bytes = 0usize;
        for c in &caches {
            let (a, b) = (c.start, c.rendered_to());
            ra = ra.max(a);
            rb = rb.min(b);
            let w1 = c.window.1.min(c.end);
            wa = wa.min(c.window.0);
            wb = wb.max(w1);
            let want = (w1 - c.window.0).max(0.0);
            total_n += want;
            done_n += (b.min(w1) - a.max(c.window.0)).clamp(0.0, want);
            bytes += c.frames() * 8;
        }
        if caches.len() < ids.len() {
            // a track hasn't started yet: nothing is ready for all of them
            total_n += 1.0;
            rb = ra;
        }
        if !wa.is_finite() || !wb.is_finite() {
            wa = 0.0;
            wb = 0.0;
        }
        if !ra.is_finite() {
            ra = 0.0;
        }
        if !rb.is_finite() || rb < ra {
            rb = ra;
        }
        let busy = caches.len() < ids.len() || caches.iter().any(|c| c.wants_more());
        Some(PrerenderGauge {
            rendered: (ra.max(0.0), rb.max(0.0)),
            window: (wa.max(0.0), wb.max(0.0)),
            progress: if total_n > 0.0 {
                (done_n / total_n).clamp(0.0, 1.0) as f32
            } else {
                1.0
            },
            busy,
            tracks: ids.len(),
            megabytes: bytes as f32 / (1024.0 * 1024.0),
            cap_megabytes: self.prefs.prerender_cap_mb,
        })
    }

    pub fn switch_mode(&mut self, m: Mode) {
        if m == self.mode {
            return;
        }
        self.stop();
        self.mode = m;
        if m == Mode::Waveform {
            self.center_tab = crate::app::CenterTab::Editor;
        }
    }

    /// A person asked for an editor. With an empty project, ask for the details of a new
    /// file or session first (the "New Audio File" / "New Multitrack Session" dialogs in
    /// the reference); once a real project exists the buttons just switch views.
    pub fn request_mode(&mut self, m: Mode) {
        let rate = self
            .cur()
            .map(|f| f.data.rate)
            .or_else(|| self.session.as_ref().map(|s| s.rate))
            .unwrap_or(48000);
        if let Some(d) = crate::dialogs::dialog_for_mode(
            m,
            rate,
            self.cur().is_some(),
            self.session.is_some(),
            self.files.len(),
        ) {
            self.dialog = Some(d);
            return;
        }
        self.switch_mode(m);
    }

    /// Opening the Mixer with no project makes a usable session straight away, so the
    /// mixer is never an empty shell. Unlike the mode buttons this does not ask first.
    pub fn ensure_mixer_session(&mut self) {
        if self.session.is_none() {
            let s = mixer_session();
            self.session_hist = vec![("New Session".into(), s.hist_state())];
            self.session_hist_pos = 0;
            self.session = Some(s);
            self.status = "Created a session for the Mixer".into();
        }
    }

    /// Keep the engine's view of the world in sync with the UI.
    pub fn sync_engine(&mut self) {
        let (playing, mut pos, rec) = {
            let s = self.engine.lock();
            (s.playing, s.pos, s.recording)
        };
        // With no audio device (or in a frame-by-frame harness) the engine can be "playing" while
        // its position never leaves the point playback started at. Do not let that stale position
        // pull the visible playhead backwards: if the UI cursor has moved, move the engine to it.
        if playing && !rec && (pos - self.play_origin).abs() < 1e-6 {
            let shown = self.cursor();
            if (shown - self.play_origin).abs() > 1e-3 {
                self.engine.lock().pos = shown;
                self.play_origin = shown;
                pos = shown;
            }
        }
        match self.mode {
            Mode::Waveform => {
                if let (Some(f), None) = (self.cur(), &self.preview_audio) {
                    let data = f.data.clone();
                    let rack = (
                        f.rack.clone(),
                        f.rack_in_db,
                        f.rack_out_db,
                        f.rack_mix,
                        f.rack_power,
                    );
                    let mut s = self.engine.lock();
                    let same = matches!(&s.source, Source::File(d) if Arc::ptr_eq(d, &data));
                    if !same {
                        if playing {
                            s.end = s.end.min(data.duration());
                        }
                        s.source = Source::File(data);
                        s.reset_meters();
                        self.meter_hold = [0.0; 2];
                        self.meter_hold_time = [0.0; 2];
                        self.meter_clip = [false; 2];
                        crate::fx3::reset_loudness();
                    }
                    s.set_rack(&rack.0, rack.1, rack.2, rack.3, rack.4);
                }
            }
            Mode::Multitrack => self.refresh_mix(false),
        }
        if playing || rec {
            let follow = self.auto_scroll;
            match self.mode {
                Mode::Waveform => {
                    if let Some(f) = self.cur_mut() {
                        if playing {
                            f.cursor = pos.min(f.data.duration());
                        }
                        if follow && playing {
                            let w = f.view.1 - f.view.0;
                            if f.cursor > f.view.1 || f.cursor < f.view.0 {
                                let a = f.cursor.min((f.data.duration() - w).max(0.0));
                                f.view = (a, a + w);
                            }
                        }
                    }
                }
                Mode::Multitrack => {
                    if let Some(s) = &mut self.session {
                        if playing {
                            s.cursor = pos;
                        }
                        if follow && playing {
                            let w = s.view.1 - s.view.0;
                            if s.cursor > s.view.1 || s.cursor < s.view.0 {
                                s.view = (s.cursor, s.cursor + w);
                            }
                        }
                    }
                }
            }
        } else if self.was_playing {
            // playback ended on its own
            self.was_playing = false;
            if self.return_on_stop {
                let o = self.play_origin;
                self.set_mode_cursor(o);
            }
        }
    }

    // ------------------------------------------------------------ sessions

    /// Write a `.sesx` (after the compatibility check) and mark the session saved.
    pub fn finish_sesx_save(&mut self, path: &std::path::Path, copy: &Session, files: &[String]) {
        match crate::sesx::export(copy, files, path) {
            Ok(()) => {
                self.note_saved(&path);
                if let Some(s) = self.session.as_mut() {
                    s.path = Some(path.to_path_buf());
                    if let Some(stem) = path.file_stem() {
                        s.name = stem.to_string_lossy().to_string();
                    }
                    s.mark_saved();
                }
                self.status = format!("Saved session {}", path.display());
                self.add_recent(path);
            }
            Err(e) => self.message("Could not save session", &e),
        }
    }

    pub fn save_session(&mut self, save_as: bool) {
        let Some(s) = &self.session else { return };
        let path = if save_as || s.path.is_none() {
            let mut dlg = rfd::FileDialog::new()
                .add_filter("Session XML (*.sesx)", &["sesx"])
                .add_filter("AudioCraft session (*.acsession)", &["acsession"])
                .set_file_name(format!("{}.sesx", s.name));
            if let Some(d) = self.default_save_dir() {
                dlg = dlg.set_directory(d);
            }
            match dlg.ask_save() {
                Some(p) if p.extension().is_some() => p,
                Some(p) => p.with_extension("sesx"),
                None => return,
            }
        } else {
            s.path.clone().unwrap()
        };
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let mut used: Vec<u64> = s
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.file_id))
            .collect();
        used.sort();
        used.dedup();
        let mut paths: Vec<(u64, String)> = vec![];
        // names already written by this save: two sources called the same never share a file
        let mut written: Vec<std::path::PathBuf> = vec![];
        for id in used {
            let Some(f) = self.file(id) else { continue };
            let is_wav = f
                .path
                .as_ref()
                .and_then(|p| p.extension())
                .map(|e| e.eq_ignore_ascii_case("wav"))
                .unwrap_or(false);
            let need_write = f.path.is_none() || f.dirty() || (!is_wav && f.hist_pos > 0);
            let p = if need_write {
                let stem = std::path::Path::new(&f.name)
                    .file_stem()
                    .map(|x| x.to_string_lossy().to_string())
                    .unwrap_or("Audio".into());
                // a free name: never another file of this save, nor an unrelated file already
                // in the folder (the file's own earlier copy may be replaced)
                let own = f.path.clone();
                let mut p = dir.join(format!("{stem}.wav"));
                let mut n = 2;
                while written.contains(&p) || (p.exists() && own.as_ref() != Some(&p)) {
                    p = dir.join(format!("{stem} ({n}).wav"));
                    n += 1;
                }
                written.push(p.clone());
                if let Err(e) = audio::save_any(&p, &f.data, f.bits, f.float, false) {
                    self.message("Could not save session audio", &e);
                    return;
                }
                let f = self.file_mut(id).unwrap();
                f.path = Some(p.clone());
                f.saved_pos = Some(f.hist_pos);
                p
            } else {
                f.path.clone().unwrap()
            };
            paths.push((id, p.to_string_lossy().to_string()));
        }
        let s = self.session.as_mut().unwrap();
        let mut tracks = s.tracks.clone();
        for t in tracks.iter_mut() {
            for c in t.clips.iter_mut() {
                c.file_id = paths
                    .iter()
                    .position(|(id, _)| *id == c.file_id)
                    .unwrap_or(0) as u64;
            }
        }
        let is_xml = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("sesx") || e.eq_ignore_ascii_case("xml"))
            .unwrap_or(false);
        if is_xml {
            let mut copy = s.clone();
            copy.tracks = tracks;
            let files: Vec<String> = paths.iter().map(|(_, p)| p.clone()).collect();
            // curves and shaped fades have no place in a .sesx: ask (or convert quietly)
            let issues = crate::compat::issues(&copy);
            if !issues.is_empty() {
                if self.prefs.warn_sesx_compat && self.screenshot.is_none() {
                    self.sesx_pending = Some(crate::compat::Pending {
                        path,
                        files,
                        copy,
                        issues,
                        convert: true,
                    });
                    return;
                }
                crate::compat::convert(&mut copy, self.prefs.sesx_bake_tolerance_db);
            }
            self.finish_sesx_save(&path, &copy, &files);
            return;
        }
        let file = serde_json::json!({
            "format": "AudioCraft Session 1",
            "name": s.name,
            "rate": s.rate,
            "master_db": s.master_db,
            "markers": s.markers,
            "files": paths.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
            "tracks": tracks,
            "master": s.master,
            "metronome": s.metronome,
            "groups": s.groups,
            "mix": s.mix,
            "bits": s.bits,
        });
        match audio::write_atomic(
            &path,
            serde_json::to_string_pretty(&file)
                .unwrap_or_default()
                .as_bytes(),
        ) {
            Ok(()) => {
                s.path = Some(path.clone());
                if let Some(stem) = path.file_stem() {
                    s.name = stem.to_string_lossy().to_string();
                }
                s.mark_saved();
                self.note_saved(&path);
                self.status = format!("Saved session {}", path.display());
                self.add_recent(&path);
            }
            Err(e) => self.message("Could not save session", &e),
        }
    }

    /// Open a `.sesx` session: load its audio files and map the clips onto them.
    fn load_sesx(&mut self, path: &std::path::Path) {
        let (mut s, files) = match crate::sesx::import(path) {
            Ok(v) => v,
            Err(e) => return self.message("Could not open session", &e),
        };
        let mut ids = vec![];
        let mut missing = vec![];
        for pb in &files {
            let existing = self
                .files
                .iter()
                .find(|f| f.path.as_deref() == Some(pb.as_path()))
                .map(|f| f.id);
            let id = match existing {
                Some(id) => id,
                None => match audio::load(pb) {
                    Ok(lf) => {
                        let name = pb
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        self.add_file(name, Some(pb.clone()), lf.data, lf.bits, lf.float, "Open")
                    }
                    Err(_) => {
                        missing.push(pb.display().to_string());
                        0
                    }
                },
            };
            ids.push(id);
        }
        for t in s.tracks.iter_mut() {
            for c in t.clips.iter_mut() {
                c.file_id = ids.get(c.file_id as usize).copied().unwrap_or(0);
            }
            t.clips.retain(|c| c.file_id != 0);
        }
        s.view = (0.0, (s.length() * 1.1).max(30.0));
        s.mark_saved();
        self.session_hist = vec![("Open Session".into(), s.hist_state())];
        self.session_hist_pos = 0;
        self.session = Some(s);
        self.mode = Mode::Multitrack;
        self.add_recent(path);
        self.refresh_mix(true);
        if !missing.is_empty() {
            self.message(
                "Some media is offline",
                &format!(
                    "These files could not be found or opened; their clips were left out:\n\n{}",
                    missing.join("\n")
                ),
            );
        }
    }

    pub fn load_session(&mut self, path: &std::path::Path) {
        if path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("sesx") || e.eq_ignore_ascii_case("xml"))
            .unwrap_or(false)
        {
            return self.load_sesx(path);
        }
        let txt = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => return self.message("Could not open session", &e.to_string()),
        };
        let v: serde_json::Value = match serde_json::from_str(&txt) {
            Ok(v) => v,
            Err(e) => return self.message("Could not open session", &e.to_string()),
        };
        let rate = v["rate"].as_u64().unwrap_or(48000) as u32;
        let mut s = Session::new(v["name"].as_str().unwrap_or("Session"), rate, 0);
        s.master_db = v["master_db"].as_f64().unwrap_or(0.0) as f32;
        s.markers = serde_json::from_value(v["markers"].clone()).unwrap_or_default();
        if let Ok(m) = serde_json::from_value(v["master"].clone()) {
            s.master = m;
        }
        if let Ok(m) = serde_json::from_value(v["metronome"].clone()) {
            s.metronome = m;
        }
        if let Ok(g) = serde_json::from_value(v["groups"].clone()) {
            s.groups = g;
        }
        if let Some(m) = v["mix"].as_u64() {
            s.mix = m.min(2) as u8;
        }
        if let Some(b) = v["bits"].as_u64() {
            s.bits = b as u16;
        }
        let paths: Vec<String> = serde_json::from_value(v["files"].clone()).unwrap_or_default();
        let mut ids = vec![];
        for p in &paths {
            let pb = PathBuf::from(p);
            let existing = self
                .files
                .iter()
                .find(|f| f.path.as_deref() == Some(pb.as_path()))
                .map(|f| f.id);
            let id = match existing {
                Some(id) => id,
                None => match audio::load(&pb) {
                    Ok(lf) => {
                        let name = pb
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        self.add_file(name, Some(pb.clone()), lf.data, lf.bits, lf.float, "Open")
                    }
                    Err(_) => 0,
                },
            };
            ids.push(id);
        }
        let mut tracks: Vec<Track> =
            serde_json::from_value(v["tracks"].clone()).unwrap_or_default();
        let mut maxid = 1;
        for t in tracks.iter_mut() {
            maxid = maxid.max(t.id);
            for c in t.clips.iter_mut() {
                maxid = maxid.max(c.id);
                c.file_id = ids.get(c.file_id as usize).copied().unwrap_or(0);
            }
            t.clips.retain(|c| c.file_id != 0);
        }
        s.tracks = tracks;
        s.next_id = maxid + 1;
        s.path = Some(path.to_path_buf());
        s.view = (0.0, (s.length() * 1.1).max(30.0));
        s.mark_saved();
        self.session_hist = vec![("Open Session".into(), s.hist_state())];
        self.session_hist_pos = 0;
        self.session = Some(s);
        self.mode = Mode::Multitrack;
        self.add_recent(path);
        self.refresh_mix(true);
    }

    pub fn mixdown(&mut self, selection_only: bool, to_file: bool) {
        let Some(s) = &self.session else { return };
        let files = &self.files;
        let mix = s.snapshot(&|id| files.iter().find(|f| f.id == id).map(|f| f.data.clone()));
        let (a, b) = match (selection_only, s.selection) {
            (true, Some((a, b))) if b > a => (a, b),
            _ => (0.0, s.length()),
        };
        if b <= a {
            self.message("Nothing to mix", "The session has no clips.");
            return;
        }
        let d = crate::session::mixdown(&mix, s.rate, a, b);
        let name = format!("{}_Mixdown.wav", s.name);
        if to_file {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("WAV audio", &["wav"])
                .set_file_name(&name)
                .ask_save()
            {
                match audio::save_wav(&p, &d, 24, false) {
                    Ok(()) => self.status = format!("Exported {}", p.display()),
                    Err(e) => self.message("Export failed", &e),
                }
            }
        } else {
            self.add_file(name, None, d, 32, true, "Mixdown");
            self.switch_mode(Mode::Waveform);
        }
    }

    pub fn insert_into_multitrack(&mut self, file_id: u64) {
        let Some(f) = self.file(file_id) else { return };
        let len = f.data.duration();
        let name = std::path::Path::new(&f.name)
            .file_stem()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_default();
        let stereo = f.data.channels() > 1;
        let rate = f.data.rate;
        let bits = if f.float { 32 } else { f.bits };
        let importing = len <= 0.0 && f.path.is_some();
        // No session yet: make one now, with the file's own sample rate and bit depth, before
        // any clip is placed. Switching the view alone never creates a session, so the old
        // `unwrap()` here panicked when a file was inserted from an empty Files panel.
        if self.session.is_none() {
            let mut s = Session::new("Untitled Session 1", rate, 6);
            s.bits = bits;
            self.session_hist = vec![("New Session".into(), s.hist_state())];
            self.session_hist_pos = 0;
            self.session = Some(s);
        }
        // A file that is still decoding has no duration yet. Queue the drop and let the import
        // worker's completion place it, instead of adding a zero-length clip that never plays.
        if importing {
            let track = self.session.as_ref().map(|s| s.selected_track).unwrap_or(0);
            self.pending_placements.push((file_id, track, 0.0));
            self.stop();
            self.mode = Mode::Multitrack;
            self.status = format!("Queued {name} for insertion when decoding finishes");
            return;
        }
        let Some(s) = self.session.as_mut() else {
            return;
        };
        let at = s.cursor;
        let mut ti = None;
        for i in 0..s.tracks.len() {
            let idx = (s.selected_track + i) % s.tracks.len().max(1);
            if !s.tracks[idx]
                .clips
                .iter()
                .any(|c| c.start < at + len && c.end() > at)
            {
                ti = Some(idx);
                break;
            }
        }
        let ti = match ti {
            Some(t) => t,
            None => s.add_track(stereo),
        };
        let id = s.id();
        s.tracks[ti].clips.push(crate::session::Clip {
            id,
            file_id,
            name,
            start: at,
            offset: 0.0,
            len,
            gain_db: 0.0,
            muted: false,
            env: vec![],
            pan_env: vec![],
            rack: vec![],
            speed: 1.0,
            fx: Default::default(),
            auto_in: true,
            auto_out: true,
            curves: Default::default(),
            fade_in: 0.0,
            fade_out: 0.0,
        });
        s.selected_clip = Some(id);
        s.selected_track = ti;
        if s.view.1 < at + len {
            s.view.1 = at + len + 5.0;
        }
        self.stop();
        self.mode = Mode::Multitrack;
        self.session_push("Insert Clip");
    }

    /// Put the current file into the multitrack session (a new one when asked, or when there is
    /// none) on the selected track at the session's playhead, and switch to Multitrack.
    pub fn insert_current_into_multitrack(&mut self, new_session: bool) {
        let Some(fid) = self.current else { return };
        let rate = self.cur().map(|f| f.data.rate).unwrap_or(48000);
        if new_session || self.session.is_none() {
            let s = Session::new("Untitled Session", rate, 6);
            self.session_hist = vec![("New Session".into(), s.hist_state())];
            self.session_hist_pos = 0;
            self.session = Some(s);
        }
        let (track, at) = self
            .session
            .as_ref()
            .map(|s| (s.selected_track, s.cursor))
            .unwrap_or((0, 0.0));
        self.add_clip_at(fid, track, at);
        self.switch_mode(Mode::Multitrack);
    }

    /// Make a mono file from each channel of the current file.
    pub fn extract_channels(&mut self) {
        let Some(f) = self.cur() else { return };
        if f.data.channels() < 2 {
            return;
        }
        let stem = std::path::Path::new(&f.name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Audio".into());
        let names = ["L", "R", "C", "LFE", "Ls", "Rs"];
        let parts: Vec<(String, AudioData)> = (0..f.data.channels())
            .map(|c| {
                let mut d = AudioData::new(f.data.rate, 1, 0);
                d.ch[0] = f.data.ch[c].clone();
                (
                    format!("{stem} {}.wav", names.get(c).copied().unwrap_or("Ch")),
                    d,
                )
            })
            .collect();
        let (bits, float) = (f.bits, f.float);
        for (n, d) in parts {
            self.add_file(n, None, d, bits, float, "Extract Channel");
        }
        self.status = "Channels extracted to mono files".into();
    }

    /// Put a file into the session at (track, time). A file with another sample rate first asks
    /// whether to convert it, change the session, or keep it as it is.
    pub fn place_file(&mut self, file_id: u64, track: usize, at: f64) {
        let (frate, fbits) = match self.file(file_id) {
            Some(f) => (f.data.rate, if f.float { 32 } else { f.bits }),
            None => return,
        };
        let (srate, sbits) = self
            .session
            .as_ref()
            .map(|s| (s.rate, s.bits))
            .unwrap_or((frate, fbits));
        if frate != srate || fbits != sbits {
            // ask: convert the file, change the session, or keep it as it is
            self.dialog = Some(Dialog::RateMismatch {
                file_id,
                track,
                at,
                choice: 0,
            });
            return;
        }
        self.add_clip_at(file_id, track, at);
    }

    pub fn add_clip_at(&mut self, file_id: u64, track: usize, at: f64) {
        let Some(f) = self.file(file_id) else { return };
        let len = f.data.duration();
        let stereo = f.data.channels() >= 2;
        let name = std::path::Path::new(&f.name)
            .file_stem()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_default();
        let Some(s) = self.session.as_mut() else {
            return;
        };
        while track >= s.tracks.len() {
            s.add_track(stereo);
        }
        let id = s.id();
        s.tracks[track].clips.push(crate::session::Clip {
            id,
            file_id,
            name,
            start: at.max(0.0),
            offset: 0.0,
            len,
            gain_db: 0.0,
            muted: false,
            env: vec![],
            pan_env: vec![],
            rack: vec![],
            speed: 1.0,
            fx: Default::default(),
            auto_in: true,
            auto_out: true,
            curves: Default::default(),
            fade_in: 0.0,
            fade_out: 0.0,
        });
        s.selected_clip = Some(id);
        s.selected_track = track;
        self.session_push("Insert Clip");
    }

    // ------------------------------------------------------------ time display

    pub fn rate(&self) -> u32 {
        match self.mode {
            Mode::Waveform => self.cur().map(|f| f.data.rate).unwrap_or(48000),
            Mode::Multitrack => self.session.as_ref().map(|s| s.rate).unwrap_or(48000),
        }
    }
    /// Choose the time format (index into `TimeFormat::ALL`) and keep it for next time.
    pub fn set_time_format(&mut self, i: usize) {
        self.time_format = TimeFormat::from_index(i as u8);
        self.prefs.time_format = i.min(TimeFormat::ALL.len() - 1) as u8;
        crate::prefs::save(&self.prefs);
    }

    pub fn fmt(&self, t: f64) -> String {
        self.time_format.format(t, self.rate())
    }

    pub fn view_width(&self) -> f64 {
        match self.mode {
            Mode::Waveform => self.cur().map(|f| f.view.1 - f.view.0).unwrap_or(1.0),
            Mode::Multitrack => self
                .session
                .as_ref()
                .map(|s| s.view.1 - s.view.0)
                .unwrap_or(1.0),
        }
    }

    fn points(&self) -> Vec<f64> {
        let mut v = vec![0.0];
        match self.mode {
            Mode::Waveform => {
                if let Some(f) = self.cur() {
                    v.push(f.data.duration());
                    v.extend(f.markers.iter().map(|m| m.time));
                    if let Some((a, b)) = f.selection {
                        v.push(a);
                        v.push(b);
                    }
                }
            }
            Mode::Multitrack => {
                if let Some(s) = &self.session {
                    v.push(s.length());
                    v.extend(s.markers.iter().map(|m| m.time));
                    for t in &s.tracks {
                        for c in &t.clips {
                            v.push(c.start);
                            v.push(c.end());
                        }
                    }
                }
            }
        }
        v.sort_by(|a, b| a.total_cmp(b));
        v
    }
    pub fn prev_point(&self) -> f64 {
        let c = self.cursor();
        self.points()
            .into_iter()
            .filter(|p| *p < c - 1e-4)
            .fold(0.0, f64::max)
    }
    pub fn next_point(&self) -> f64 {
        let c = self.cursor();
        self.points()
            .into_iter()
            .find(|p| *p > c + 1e-4)
            .unwrap_or(c)
    }

    pub fn ensure_session(&mut self) {
        if self.session.is_none() {
            let s = Session::new("Untitled Session 1", 48000, 0);
            self.session_hist = vec![("New Session".into(), s.hist_state())];
            self.session_hist_pos = 0;
            self.session = Some(s);
        }
    }

    pub fn undo_label(&self) -> Option<String> {
        if self.mode == Mode::Multitrack && self.session.is_some() {
            if self.session_hist_pos > 0 {
                return self
                    .session_hist
                    .get(self.session_hist_pos)
                    .map(|h| h.0.clone());
            }
            return None;
        }
        let f = self.cur()?;
        if f.hist_pos > 0 {
            Some(f.history[f.hist_pos].0.clone())
        } else {
            None
        }
    }

    /// Switch to a workspace (its saved layout).
    pub fn set_workspace(&mut self, name: &str) {
        self.ws.switch(name);
    }

    /// Show a panel in the current workspace (bring it forward, or add it).
    pub fn show_panel(&mut self, p: crate::workspace::PanelId) {
        self.ws.layout.show(p);
    }

    /// The "New Multitrack Session" request. All the fields are editable in the dialog.
    pub fn new_session_dialog(&self) -> Dialog {
        let mut d =
            crate::dialogs::new_session_dialog(self.cur().map(|f| f.data.rate).unwrap_or(48000));
        if let Dialog::NewSession { folder, .. } = &mut d {
            if let Some(dir) = self.default_save_dir() {
                *folder = dir.to_string_lossy().to_string();
            }
        }
        d
    }
}

// ------------------------------------------------------------ recent files

fn recent_path() -> Option<PathBuf> {
    Some(crate::prefs::config_dir()?.join("recent.json"))
}

pub fn load_recent() -> Vec<PathBuf> {
    recent_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok())
        .map(|v| v.into_iter().map(PathBuf::from).collect())
        .unwrap_or_default()
}

impl App {
    pub fn add_recent(&mut self, p: &std::path::Path) {
        if self.screenshot.is_some() {
            return;
        }
        self.recent.retain(|r| r != p);
        self.recent.insert(0, p.to_path_buf());
        self.recent.truncate(12);
        self.save_recent();
    }
    pub fn save_recent(&self) {
        if let Some(path) = recent_path() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let v: Vec<String> = self
                .recent
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            let _ = std::fs::write(path, serde_json::to_string_pretty(&v).unwrap_or_default());
        }
    }
}

/// A free `<dir>/<stem>.wav` (or `<stem> 2.wav`, `<stem> 3.wav`...), creating the folder.
/// Characters Windows forbids in names are replaced.
pub fn unique_take_path(dir: &std::path::Path, stem: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let clean: String = stem
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let clean = if clean.trim().is_empty() {
        "Recording".to_string()
    } else {
        clean
    };
    for i in 1..10000u32 {
        let name = if i == 1 {
            format!("{clean}.wav")
        } else {
            format!("{clean} {i}.wav")
        };
        let p = dir.join(name);
        if !p.exists() {
            return Ok(p);
        }
    }
    Err("too many takes with this name".into())
}

/// One track's pre-render: the audio rendered so far (shared pieces, see
/// [`crate::session::PreRange`]) and the render that extends it.
pub struct PreCache {
    pub key: u64,
    pub mix: crate::session::Mix,
    pub render: crate::session::MixRender,
    pub rate: u32,
    /// Session time of the first kept frame.
    pub start: f64,
    /// Pre-roll frames still to throw away (they only warm the effects up).
    pub skip: usize,
    pub chunks: Vec<Arc<[Vec<f32>; 2]>>,
    /// Frames rendered after the last full piece.
    pub pending: [Vec<f32>; 2],
    /// Where the track's last clip ends.
    pub content_end: f64,
    /// Where the render stops: the last clip's end plus the effects' tail. Until the tail is
    /// measured this is the longest tail allowed; once it falls silent it is the real end.
    pub end: f64,
    pub tail_done: bool,
    quiet: usize,
    pub has_fx: bool,
    /// The stretch this cache is asked to hold (session seconds).
    pub window: (f64, f64),
}

impl PreCache {
    pub fn frames(&self) -> usize {
        self.chunks.iter().map(|c| c[0].len()).sum::<usize>() + self.pending[0].len()
    }

    /// Session time just after the last rendered frame.
    pub fn rendered_to(&self) -> f64 {
        self.start + self.frames() as f64 / self.rate.max(1) as f64
    }

    /// The rendered stretch as playback sees it (the unfinished last piece is copied in as a
    /// shorter final piece; the cache's own pieces stay full-sized).
    pub fn range(&self) -> crate::session::PreRange {
        let mut chunks = self.chunks.clone();
        if !self.pending[0].is_empty() && self.pending[0].len() == self.pending[1].len() {
            chunks.push(Arc::new(self.pending.clone()));
        }
        crate::session::PreRange {
            start: self.start,
            rate: self.rate,
            chunks,
        }
    }

    /// More to render for the current window?
    pub fn wants_more(&self) -> bool {
        if self.tail_done && self.rendered_to() >= self.end - 1e-9 {
            return false;
        }
        self.rendered_to() < self.window.1.min(self.end) - 1e-9
    }

    /// Drop whole pieces that ended before `t` (the playhead moved on).
    pub fn trim_before(&mut self, t: f64) {
        let piece = crate::session::PRE_CHUNK as f64 / self.rate.max(1) as f64;
        while self.chunks.len() > 1 && self.start + piece <= t {
            self.chunks.remove(0);
            self.start += piece;
        }
    }

    /// Render one block. True when new audio became available to playback.
    pub fn step(&mut self) -> bool {
        let before = self.render.reached();
        let finished = self.render.step(&self.mix, 1);
        let [mut l, mut r] = self.render.drain();
        if self.skip > 0 {
            let k = self.skip.min(l.len());
            l.drain(..k);
            r.drain(..k.min(r.len()));
            self.skip -= k;
        }
        // the tail: after the last clip, stop once the effects have been silent for half a
        // second (a reverb that rings on keeps the render going)
        if before >= self.content_end - 1e-9 && !l.is_empty() {
            let peak = l
                .iter()
                .chain(r.iter())
                .fold(0.0f32, |m, v| m.max(v.abs()));
            if peak < 1e-5 || !self.has_fx {
                self.quiet += l.len();
            } else {
                self.quiet = 0;
            }
        }
        self.pending[0].extend_from_slice(&l);
        self.pending[1].extend_from_slice(&r);
        let mut grew = false;
        let piece = crate::session::PRE_CHUNK;
        while self.pending[0].len() >= piece && self.pending[1].len() >= piece {
            let l: Vec<f32> = self.pending[0].drain(..piece).collect();
            let r: Vec<f32> = self.pending[1].drain(..piece).collect();
            self.chunks.push(Arc::new([l, r]));
            grew = true;
        }
        let silent_long = self.quiet as f64 >= 0.5 * self.rate as f64
            || (!self.has_fx && self.rendered_to() >= self.content_end - 1e-9);
        if (silent_long || finished) && !self.tail_done {
            self.tail_done = true;
            self.end = self.rendered_to().max(self.content_end);
            self.render.set_end(self.end);
            grew = true;
        }
        grew
    }
}

/// The pre-render gauge's state (like a video editor's render bar).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrerenderGauge {
    /// The stretch every pre-rendered track has ready (plays from rendered audio).
    pub rendered: (f64, f64),
    /// The stretch the renders aim to hold now (the whole session, or around the playhead).
    pub window: (f64, f64),
    /// 0..=1: how much of the window is rendered.
    pub progress: f32,
    /// A render still has work to do.
    pub busy: bool,
    /// How many tracks have Pre-render on.
    pub tracks: usize,
    /// Memory the rendered audio uses, and the cap from Preferences.
    pub megabytes: f32,
    pub cap_megabytes: f32,
}
