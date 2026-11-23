//! Preferences (Edit > Preferences): every setting the Preferences window shows, saved as JSON
//! next to the workspaces. Unknown or missing fields fall back to their defaults, so an older or
//! newer preferences file never stops the app from starting.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The categories of the Preferences window, in the order the list shows them.
pub const CATEGORIES: [&str; 17] = [
    "General",
    "Appearance",
    "Audio Channel Mapping",
    "Audio Hardware",
    "Auto Save",
    "Control Surface",
    "Data",
    "Effects",
    "Markers & Metadata",
    "Media & Disk Cache",
    "Memory",
    "Multitrack",
    "Multitrack Clips",
    "Playback and Recording",
    "Spectral Displays",
    "Time Display",
    "Video",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    // General
    pub show_tooltips: bool,
    pub zoom_factor: f32,
    pub auto_scroll_mode: u8,
    pub return_on_stop: bool,
    pub open_last_session: bool,
    pub history_limit: u32,
    // Appearance
    pub brightness: f32,
    pub ui_scale: f32,
    pub saturation: f32,
    /// 0 = AudioCraft's usual theme, 1 = the darker Audition-style theme.
    pub color_theme: u8,
    // Audio Channel Mapping
    pub map_left: u8,
    pub map_right: u8,
    // Auto Save
    pub autosave_on: bool,
    pub autosave_minutes: u32,
    pub autosave_keep: u32,
    // Control Surface
    pub control_surface: String,
    // Data
    pub dither_on: bool,
    pub dither_depth: f32,
    pub interpret_float_clip: bool,
    pub smooth_delete_ms: f32,
    pub smooth_edit_crossfade: bool,
    // Effects
    pub fx_parallel: bool,
    pub fx_default_mix: f32,
    pub plugin_scan_start: bool,
    /// VST3/CLAP editors open docked in the Plug-ins panel (off: in their own windows).
    pub dock_plugin_editors: bool,
    /// VST3 scan folders the person added (Blender-style folder stack, kept as paths).
    pub vst_scan_folders: Vec<PathBuf>,
    // Markers & Metadata
    pub marker_snap: bool,
    pub marker_name_prefix: String,
    // Media & Disk Cache
    pub save_peak_files: bool,
    pub temp_folder: String,
    pub cache_limit_gb: f32,
    // General: where new files and sessions are saved by default.
    /// 0 = a default location, 1 = where the first imported file is, 2 = where the last file was saved.
    pub default_save_mode: u8,
    /// The default save location when `default_save_mode == 0`; empty means the standard default.
    pub default_save_folder: String,
    // General: the undo history of each file.
    pub undo_steps: u32,
    // Multitrack
    pub default_track_height: f32,
    pub default_lane_height: f32,
    pub keyframe_size: f32,
    pub keyframe_hover_scale: f32,
    pub handle_size: f32,
    pub handle_line: f32,
    pub keyframe_selected_color: [u8; 3],
    pub keyframe_hit_radius: f32,
    pub smooth_automation: bool,
    pub warn_sesx_compat: bool,
    pub sesx_bake_tolerance_db: f32,
    pub pre_render_tracks: bool,
    /// Height of a track's Input and Output buttons (Preferences > Multitrack).
    pub track_io_button_height: f32,
    // Multitrack Clips
    pub auto_crossfade: bool,
    pub default_crossfade_curve: u8,
    pub clip_fade_points_smooth: bool,
    pub clip_header_height: f32,
    pub play_overlap: bool,
    // Playback and Recording
    pub preroll_s: f32,
    pub postroll_s: f32,
    pub record_punch: bool,
    pub stop_at_end: bool,
    /// Every recorded take is written here as a WAV file the moment recording stops
    /// (empty = Documents/AudioCraft/Recordings).
    pub recordings_folder: String,
    // Spectral Displays
    pub spec_window: u8,
    pub spec_resolution: u32,
    pub spec_db_range: f32,
    pub spec_log_scale: bool,
    /// Older setting, no longer used (kept so saved preferences still load).
    pub spec_proportional_move: bool,
    /// Moving a spectral selection keeps its octaves instead of its Hz range (off by default:
    /// the selection keeps the range it covers, like the original).
    pub spec_move_keep_octaves: bool,
    pub pitch_resolution: u32,
    pub pitch_db_range: f32,
    pub pitch_min_hz: f32,
    pub pitch_max_hz: f32,
    // Time Display
    pub frames_per_second: f32,
    pub time_format: u8,
    // Video
    pub video_preview: bool,
    pub video_quality: u8,
    /// Picture size of the video preview: 0 = fit the panel, 1 = 720p, 2 = 1080p, 3 = native.
    pub video_resolution: u8,
    /// Explicit path to a different FFmpeg decoder (empty = use the bundled copy, then PATH).
    pub video_decoder_exe: String,
    /// Folder containing ffmpeg.exe / ffmpeg, chosen in Preferences > Video.
    pub video_decoder_folder: String,
    /// More decoder folders, one per line, searched by the Detect button.
    pub video_decoder_repos: String,
    /// Public repositories for free FFmpeg encoder/decoder builds, one per line (informational).
    pub video_decoder_sources: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            show_tooltips: true,
            zoom_factor: 1.0,
            auto_scroll_mode: 0,
            return_on_stop: false,
            open_last_session: false,
            history_limit: 1000,
            brightness: 0.0,
            ui_scale: 1.0,
            saturation: 1.0,
            color_theme: 0,
            map_left: 1,
            map_right: 2,
            autosave_on: true,
            autosave_minutes: 5,
            autosave_keep: 10,
            control_surface: "None".into(),
            dither_on: true,
            dither_depth: 1.0,
            interpret_float_clip: false,
            smooth_delete_ms: 5.0,
            smooth_edit_crossfade: true,
            fx_parallel: true,
            fx_default_mix: 100.0,
            plugin_scan_start: true,
            dock_plugin_editors: true,
            vst_scan_folders: vec![],
            marker_snap: true,
            marker_name_prefix: "Marker".into(),
            save_peak_files: true,
            temp_folder: String::new(),
            cache_limit_gb: 10.0,
            undo_steps: 1000,
            default_save_mode: 0,
            default_save_folder: String::new(),
            default_track_height: 96.0,
            default_lane_height: 120.0,
            keyframe_size: 5.0,
            keyframe_hover_scale: 1.5,
            handle_size: 4.0,
            handle_line: 1.0,
            keyframe_selected_color: [64, 150, 255],
            keyframe_hit_radius: 6.0,
            smooth_automation: false,
            warn_sesx_compat: true,
            sesx_bake_tolerance_db: 0.25,
            pre_render_tracks: false,
            track_io_button_height: 22.0,
            auto_crossfade: true,
            default_crossfade_curve: 0,
            clip_fade_points_smooth: true,
            clip_header_height: 15.0,
            play_overlap: false,
            preroll_s: 2.0,
            postroll_s: 2.0,
            record_punch: false,
            stop_at_end: true,
            recordings_folder: String::new(),
            spec_window: 0,
            spec_resolution: 2048,
            spec_db_range: 132.0,
            spec_log_scale: true,
            spec_proportional_move: true,
            spec_move_keep_octaves: false,
            pitch_resolution: 8192,
            pitch_db_range: 90.0,
            pitch_min_hz: 30.87,
            pitch_max_hz: 1760.0,
            frames_per_second: 30.0,
            time_format: 0,
            video_preview: true,
            video_quality: 1,
            video_resolution: 0,
            video_decoder_exe: String::new(),
            video_decoder_folder: String::new(),
            video_decoder_repos: String::new(),
            video_decoder_sources: concat!(
                "https://ffmpeg.org/download.html\n",
                "https://www.gyan.dev/ffmpeg/builds/\n",
                "https://github.com/BtbN/FFmpeg-Builds/releases\n",
                "https://evermeet.cx/ffmpeg/"
            )
            .into(),
        }
    }
}

pub const SPEC_WINDOWS: [&str; 5] = [
    "Blackman-Harris",
    "Hann",
    "Hamming",
    "Blackman",
    "Rectangular",
];
pub const SPEC_RESOLUTIONS: [u32; 6] = [256, 512, 1024, 2048, 4096, 8192];
pub const PITCH_RESOLUTIONS: [u32; 4] = [2048, 4096, 8192, 16384];
pub const CROSSFADE_CURVES: [&str; 3] = ["Equal Power (balanced)", "Linear", "Logarithmic"];

impl Prefs {
    /// The folder every recording is saved to (the chosen one, or the default).
    pub fn recordings_dir(&self) -> PathBuf {
        if self.recordings_folder.trim().is_empty() {
            PathBuf::from(crate::dialogs::default_session_folder()).join("Recordings")
        } else {
            PathBuf::from(self.recordings_folder.trim())
        }
    }
    /// Keyframe square size for drawing, never too small to see or too big to aim between.
    pub fn key_size(&self) -> f32 {
        self.keyframe_size.clamp(2.0, 16.0)
    }
    pub fn handle_px(&self) -> f32 {
        self.handle_size.clamp(2.0, 14.0)
    }
    pub fn hit_radius(&self) -> f32 {
        self.keyframe_hit_radius
            .clamp(3.0, 16.0)
            .max(self.key_size() * 0.75)
    }
    pub fn selected_color(&self) -> egui::Color32 {
        let [r, g, b] = self.keyframe_selected_color;
        egui::Color32::from_rgb(r, g, b)
    }
}

/// Settings read deep inside the editors (undo history, new tracks), kept here so code without
/// access to the app's preferences follows them. Set by `set_live` at start-up and on OK.
static UNDO_LIMIT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(80);
static TRACK_HEIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(96);
static IO_BUTTON_PX: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(22);

/// Undo steps kept per file are also held to this much audio, so a long file can never fill
/// the memory with its history (the oldest steps go first; two always stay).
pub const UNDO_MEMORY_BUDGET: usize = 3 << 30;

static FPS_X100: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(3000);

pub fn set_live(p: &Prefs) {
    use std::sync::atomic::Ordering::Relaxed;
    UNDO_LIMIT.store(p.undo_steps.clamp(1, 100_000) as usize, Relaxed);
    TRACK_HEIGHT.store(p.default_track_height.clamp(40.0, 400.0) as u32, Relaxed);
    let io = if p.track_io_button_height.is_finite() {
        p.track_io_button_height.clamp(14.0, 40.0)
    } else {
        22.0
    };
    IO_BUTTON_PX.store((io * 10.0).round() as u32, Relaxed);
    let fps = if p.frames_per_second.is_finite() {
        p.frames_per_second.clamp(1.0, 120.0)
    } else {
        30.0
    };
    FPS_X100.store((fps * 100.0).round() as u32, Relaxed);
}

/// Frames per second of the SMPTE time display (Preferences > Time Display).
pub fn fps() -> f64 {
    FPS_X100.load(std::sync::atomic::Ordering::Relaxed) as f64 / 100.0
}

/// How many undo steps each file keeps (Preferences > Memory > Undo steps).
pub fn undo_limit() -> usize {
    UNDO_LIMIT.load(std::sync::atomic::Ordering::Relaxed).max(1)
}

/// Height of a new track (Preferences > Multitrack > Default track height).
pub fn track_height() -> f32 {
    TRACK_HEIGHT.load(std::sync::atomic::Ordering::Relaxed) as f32
}

/// Height of the Input and Output buttons in a fresh track's header.
pub fn io_button_height() -> f32 {
    IO_BUTTON_PX.load(std::sync::atomic::Ordering::Relaxed) as f32 / 10.0
}

impl Prefs {
    /// The name of the next marker: "<prefix> NN" (Preferences > Markers & Metadata).
    pub fn marker_name(&self, n: usize) -> String {
        let p = self.marker_name_prefix.trim();
        format!("{} {n:02}", if p.is_empty() { "Marker" } else { p })
    }
}

/// AudioCraft's settings folder (preferences, workspaces, recent files): `%APPDATA%\AudioCraft`
/// or `~/.config/AudioCraft`. Tests get a private temporary folder, so they never read or
/// change the person's own settings.
pub fn config_dir() -> Option<PathBuf> {
    if cfg!(test) {
        return Some(
            std::env::temp_dir().join(format!("audiocraft-test-config-{}", std::process::id())),
        );
    }
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("AudioCraft"))
}

pub fn path() -> Option<PathBuf> {
    Some(config_dir()?.join("preferences.json"))
}

pub fn load() -> Prefs {
    if cfg!(test) {
        return Prefs::default();
    }
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(p: &Prefs) {
    if cfg!(test) {
        return;
    }
    if let (Some(path), Ok(s)) = (path(), serde_json::to_string_pretty(p)) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = crate::audio::write_atomic(&path, s.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preference_menu_matches_the_reference_categories() {
        assert_eq!(
            CATEGORIES,
            [
                "General",
                "Appearance",
                "Audio Channel Mapping",
                "Audio Hardware",
                "Auto Save",
                "Control Surface",
                "Data",
                "Effects",
                "Markers & Metadata",
                "Media & Disk Cache",
                "Memory",
                "Multitrack",
                "Multitrack Clips",
                "Playback and Recording",
                "Spectral Displays",
                "Time Display",
                "Video",
            ]
        );
    }

    #[test]
    fn partial_or_odd_preference_files_still_load() {
        let p: Prefs = serde_json::from_str("{\"keyframe_size\": 9.0, \"nonsense\": 1}").unwrap();
        assert_eq!(p.keyframe_size, 9.0);
        assert!(p.warn_sesx_compat);
        let p: Prefs = serde_json::from_str("{\"keyframe_size\": -50.0}").unwrap();
        assert!(p.key_size() >= 2.0);
    }

    #[test]
    fn video_defaults_offer_repositories_but_do_not_pretend_a_decoder_exists() {
        let p = Prefs::default();
        assert!(p.video_decoder_exe.is_empty());
        assert!(p.video_decoder_folder.is_empty());
        assert!(p.video_decoder_repos.is_empty());
        assert!(
            p.video_decoder_sources
                .lines()
                .any(|s| s.starts_with("https://"))
        );
    }
}
