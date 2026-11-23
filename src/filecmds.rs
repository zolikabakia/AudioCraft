//! More of the original's menu commands: file handling (Open Append, Close All / Unused Media /
//! Session and its Media with the save prompt, Save Selection As, Save All, Revert, Reveal in
//! Explorer), Edit (Repeat Last Command, Copy to New, Ripple Delete, Zero Crossings, Select,
//! Enable Channels), Multitrack (Insert Files, Bounce to New Track) and Clip (Trim, Mute).
//! Every one is a registered command (see `MORE_COMMANDS`), so menus, shortcuts, the command
//! palette and the command line all reach it.

use crate::app::{App, Mode};
use crate::audio::{self, AudioData};
use crate::commands::Cmd;
use crate::curves::Handles;
use crate::dock::CloseItem;
use crate::dsp;
use crate::sysui::Ask;
use std::path::{Path, PathBuf};

macro_rules! more {
    ($(($id:expr, $title:expr, $sc:expr, $doc:expr)),* $(,)?) => {
        &[$(Cmd { id: $id, title: $title, shortcut: $sc, doc: $doc }),*]
    };
}

pub const MORE_COMMANDS: &[Cmd] = more![
    (
        "file.open_append_new",
        "Open Append: To New...",
        "",
        "Open several audio files one after another as one new file (converted to the first file's sample rate and channels)."
    ),
    (
        "file.open_append_current",
        "Open Append: To Current...",
        "",
        "Add audio files to the end of the current file (one Undo takes it back)."
    ),
    (
        "file.open_selected",
        "Open in Waveform Editor",
        "",
        "Open the file selected in the Files list in the Waveform editor."
    ),
    (
        "file.insert_selected",
        "Insert into Multitrack",
        "",
        "Put the file selected in the Files list on a track at the playhead, creating a session when one is not open."
    ),
    (
        "file.close_selected",
        "Close Selected File",
        "",
        "Close the file selected in the Files list, asking first if it has unsaved changes."
    ),
    (
        "file.close_all",
        "Close All",
        "",
        "Close every file and the session; anything unsaved is asked about first (Yes, Yes To All, No, No To All, Cancel)."
    ),
    (
        "file.close_unused",
        "Close Unused Media",
        "",
        "Close the files that no clip of the session uses."
    ),
    (
        "file.close_session_media",
        "Close Session and Its Media",
        "",
        "Close the session and the files its clips use; unsaved changes are asked about first."
    ),
    (
        "file.save_selection_as",
        "Save Selection As...",
        "",
        "Save only the selected range of the current file as a new WAV file."
    ),
    (
        "file.save_all",
        "Save All",
        "",
        "Save every file and the session that have unsaved changes."
    ),
    (
        "file.revert",
        "Revert to Saved",
        "",
        "Throw away the changes to the current file and load it again from disk (one Undo takes it back)."
    ),
    (
        "file.reveal",
        "Reveal in Explorer",
        "",
        "Show the current file (or the session) in the system file browser."
    ),
    (
        "edit.repeat_last",
        "Repeat Last Command",
        "Ctrl+R",
        "Run the last command again (a process, effect, favorite or generator)."
    ),
    (
        "edit.copy_to_new",
        "Copy to New",
        "Alt+Shift+C",
        "Copy the selection into a new file."
    ),
    (
        "edit.select_view",
        "Select Current View",
        "",
        "Select the time range that is showing."
    ),
    (
        "edit.select_to_start",
        "Select Playhead to Start",
        "Shift+Home",
        "Select from the start to the playhead."
    ),
    (
        "edit.select_to_end",
        "Select Playhead to End",
        "Shift+End",
        "Select from the playhead to the end."
    ),
    (
        "edit.select_track_clips",
        "Select All Clips in Selected Track",
        "",
        "Select every clip of the selected track."
    ),
    (
        "edit.ripple_delete_clips",
        "Ripple Delete: Selected Clips",
        "",
        "Delete the selected clips and pull the later clips of their tracks left to close the gap."
    ),
    (
        "edit.ripple_delete_time",
        "Ripple Delete: Time Selection in All Tracks",
        "",
        "Cut the time selection out of every track and pull everything after it left."
    ),
    (
        "edit.zero_cross_in",
        "Zero Crossings: Adjust Selection Inward",
        "",
        "Move both ends of the selection inward to the nearest zero crossing (clean cuts without clicks)."
    ),
    (
        "edit.zero_cross_out",
        "Zero Crossings: Adjust Selection Outward",
        "",
        "Move both ends of the selection outward to the nearest zero crossing."
    ),
    (
        "edit.enable_left",
        "Enable Channels: Left",
        "",
        "Turn editing of the left channel on or off (edits and effects leave a disabled channel alone)."
    ),
    (
        "edit.enable_right",
        "Enable Channels: Right",
        "",
        "Turn editing of the right channel on or off."
    ),
    (
        "edit.extract_mono",
        "Extract Channels to Mono Files",
        "",
        "Make a mono file from each channel of the current file."
    ),
    (
        "edit.interpret_sample_rate",
        "Interpret Sample Rate...",
        "",
        "Change the file's declared rate without resampling: the same samples now play faster or slower, changing pitch and duration."
    ),
    (
        "edit.frequency_band_splitter",
        "Frequency Band Splitter...",
        "",
        "Split the file (or selection) into low, mid and high band files at adjustable crossovers."
    ),
    (
        "edit.keyboard_shortcuts",
        "Keyboard Shortcuts...",
        "Alt+K",
        "List the keyboard shortcuts."
    ),
    (
        "multitrack.insert_files",
        "Insert Files...",
        "",
        "Open audio files and put them on the selected track and the ones under it, at the playhead."
    ),
    (
        "multitrack.bounce_track",
        "Bounce to New Track: Selected Track",
        "",
        "Render the selected track (clips, effects, volume) into a new file on a new track."
    ),
    (
        "multitrack.bounce_selection",
        "Bounce to New Track: Time Selection",
        "",
        "Render the time selection of the whole session into a new file on a new track."
    ),
    (
        "clip.trim_selection",
        "Trim to Time Selection",
        "",
        "Cut the selected clip down to the time selection."
    ),
    (
        "clip.trim_start",
        "Trim Start to Playhead",
        "Alt+[",
        "Move the selected clip's start to the playhead."
    ),
    (
        "clip.trim_end",
        "Trim End to Playhead",
        "Alt+]",
        "Move the selected clip's end to the playhead."
    ),
    (
        "clip.mute",
        "Mute Clip",
        "",
        "Mute or unmute the selected clip."
    ),
    (
        "clip.edit_source",
        "Edit Source File",
        "",
        "Open the selected clip's file in the Waveform Editor."
    ),
    (
        "clip.edit_original",
        "Edit Original",
        "",
        "Open the selected clip's original file in the default application for its file type."
    ),
    (
        "view.zoom_in_amp",
        "Zoom In Amplitude",
        "Alt+=",
        "Make the waveform taller."
    ),
    (
        "view.zoom_out_amp",
        "Zoom Out Amplitude",
        "Alt+-",
        "Make the waveform shorter."
    ),
    (
        "view.zoom_reset_amp",
        "Reset Amplitude Zoom",
        "",
        "Show the waveform at its normal height."
    ),
    (
        "file.export_file",
        "Export Audio File...",
        "Ctrl+Shift+E",
        "Export the current file: name, folder, format (WAV, AIFF, FLAC, MP3, AAC), sample rate, channels, bit depth and metadata."
    ),
    (
        "file.export_mixdown_selection",
        "Export Multitrack Mixdown: Time Selection...",
        "",
        "Export the session's time selection as one audio file, with format and sample type choices."
    ),
    (
        "file.export_settings",
        "Export Application Settings...",
        "",
        "Save the preferences, workspaces, favorites and other settings into one file (to move them to another computer)."
    ),
    (
        "file.import_settings",
        "Import Application Settings...",
        "",
        "Load settings saved with Export Application Settings."
    ),
    (
        "process.doppler",
        "Doppler Shifter (process)...",
        "",
        "Make the selection sound like it passes by in a straight line or circles around you."
    ),
    (
        "process.sound_remover",
        "Sound Remover (process)...",
        "",
        "Learn a sound from a selection, then remove it from the file."
    ),
    (
        "help.manual",
        "AudioCraft Help",
        "F1",
        "A short guide to the editors, the panels and where to find each command."
    ),
];

/// Commands that `Repeat Last Command` runs again.
pub fn repeatable(id: &str) -> bool {
    [
        "process.",
        "effect.",
        "favorite.",
        "generate.",
        "spectral.attenuate",
        "spectral.silence",
        "spectral.heal",
    ]
    .iter()
    .any(|p| id.starts_with(p))
}

/// What happens after the close prompt has asked about every unsaved item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CloseAfter {
    All,
    SessionAndMedia,
    Unused,
}

/// Find the nearest sample at or after (`dir` > 0) / before (`dir` < 0) frame `i` where the
/// sum of the channels changes sign. Searches at most one second; without a crossing, `i`.
pub fn zero_crossing(d: &AudioData, i: usize, dir: i64) -> usize {
    let n = d.frames();
    if n < 2 || d.channels() == 0 {
        return i.min(n);
    }
    let v = |k: usize| -> f32 { d.ch.iter().map(|c| c.get(k).copied().unwrap_or(0.0)).sum() };
    let limit = d.rate as i64;
    let mut k = i.min(n - 1) as i64;
    for _ in 0..limit {
        let next = k + dir.signum();
        if next < 0 || next >= n as i64 {
            break;
        }
        let (a, b) = (v(k as usize), v(next as usize));
        if a == 0.0 {
            return k as usize;
        }
        if (a < 0.0) != (b < 0.0) || b == 0.0 {
            // the sample nearer to zero of the pair
            return if a.abs() <= b.abs() {
                k as usize
            } else {
                next as usize
            };
        }
        k = next;
    }
    i.min(n)
}

/// `src` converted to `rate` and `channels` (mono is copied to every channel; more channels
/// than wanted are mixed down).
pub fn conform(src: &AudioData, rate: u32, channels: usize) -> AudioData {
    let d = if src.rate != rate {
        audio::resample(src, rate)
    } else {
        src.clone()
    };
    let channels = channels.max(1);
    if d.channels() == channels || d.channels() == 0 {
        return d;
    }
    let n = d.frames();
    let mut out = AudioData::new(rate, channels, n);
    if d.channels() == 1 {
        for c in out.ch.iter_mut() {
            c.copy_from_slice(&d.ch[0][..n]);
        }
    } else if channels == 1 {
        let k = 1.0 / d.channels() as f32;
        for i in 0..n {
            out.ch[0][i] = d.ch.iter().map(|c| c[i]).sum::<f32>() * k;
        }
    } else {
        for (ci, c) in out.ch.iter_mut().enumerate() {
            c.copy_from_slice(&d.ch[ci.min(d.channels() - 1)][..n]);
        }
    }
    out
}

/// `a` followed by `b` (b conformed to a's rate and channels).
pub fn append(a: &AudioData, b: &AudioData) -> AudioData {
    let b = conform(b, a.rate, a.channels().max(1));
    let mut out = a.clone();
    if out.channels() == 0 {
        return b;
    }
    for (ci, c) in out.ch.iter_mut().enumerate() {
        c.extend_from_slice(&b.ch[ci.min(b.channels() - 1)]);
    }
    out
}

/// Move a track's automation with the audio: points inside `a..b` go, points after it move left
/// by the same amount. Handle offsets travel with their keyframe (they are relative to it), so
/// only points that disappear need their parallel handle entries removed.
fn shift_keyframes(env: &mut Vec<(f64, f32)>, hs: &mut Vec<Handles>, a: f64, b: f64) -> usize {
    let gap = b - a;
    if !(gap > 0.0) {
        return 0;
    }
    let eps = 1e-6;
    let keep: Vec<bool> = env
        .iter()
        .map(|(t, _)| !(*t > a + eps && *t < b - eps))
        .collect();
    let removed = keep.iter().filter(|k| !**k).count();
    if removed > 0 {
        let mut i = 0;
        env.retain(|_| {
            let k = keep.get(i).copied().unwrap_or(true);
            i += 1;
            k
        });
        i = 0;
        hs.retain(|_| {
            let k = keep.get(i).copied().unwrap_or(true);
            i += 1;
            k
        });
    }
    for (t, _) in env.iter_mut() {
        if *t >= b - eps {
            *t = (*t - gap).max(0.0);
        }
    }
    crate::curves::fit(hs, env.len());
    removed
}

/// Every automation lane of a track (volume, pan and the Show Envelopes parameters).
fn shift_track_automation(t: &mut crate::session::Track, a: f64, b: f64) {
    shift_keyframes(&mut t.vol_env, &mut t.auto.vol_h, a, b);
    shift_keyframes(&mut t.auto.pan_env, &mut t.auto.pan_h, a, b);
    for p in t.auto.params.iter_mut() {
        shift_keyframes(&mut p.env, &mut p.hs, a, b);
    }
}

/// Cut `a..b` seconds out of every track and pull what follows left (Ripple Delete > Time
/// Selection in All Tracks). Markers and automation inside go; later ones move with the audio.
pub fn ripple_delete_time(s: &mut crate::session::Session, a: f64, b: f64) -> usize {
    if !(b > a) {
        return 0;
    }
    let gap = b - a;
    let ids: Vec<u64> = s
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.id))
        .collect();
    for id in ids {
        if let Some(r) = s.split_clip(id, a) {
            s.split_clip(r, b);
        } else {
            s.split_clip(id, b);
        }
    }
    let eps = 1e-6;
    let mut removed = 0;
    for t in s.tracks.iter_mut() {
        let before = t.clips.len();
        t.clips
            .retain(|c| !(c.start >= a - eps && c.end() <= b + eps));
        removed += before - t.clips.len();
        for c in t.clips.iter_mut() {
            if c.start >= b - eps {
                c.start = (c.start - gap).max(0.0);
            }
        }
        shift_track_automation(t, a, b);
    }
    shift_keyframes(&mut s.master.vol_env, &mut s.master.vol_h, a, b);
    s.markers
        .retain(|m| !(m.time > a + eps && m.time < b - eps));
    for m in s.markers.iter_mut() {
        if m.time >= b - eps {
            m.time -= gap;
        }
    }
    s.selected_clips
        .retain(|id| s.tracks.iter().any(|t| t.clips.iter().any(|c| c.id == *id)));
    if s.selected_clip
        .map(|id| s.find_clip(id).is_none())
        .unwrap_or(false)
    {
        s.selected_clip = None;
    }
    s.touch();
    removed
}

/// Delete these clips and close each gap on its track. Returns how many went.
pub fn ripple_delete_clips(s: &mut crate::session::Session, ids: &[u64]) -> usize {
    let mut spots: Vec<(usize, f64, f64, u64)> = ids
        .iter()
        .filter_map(|id| {
            s.find_clip(*id).map(|(ti, ci)| {
                (
                    ti,
                    s.tracks[ti].clips[ci].start,
                    s.tracks[ti].clips[ci].len,
                    *id,
                )
            })
        })
        .collect();
    // latest first, so the earlier ones' positions stay right
    spots.sort_by(|x, y| y.1.total_cmp(&x.1));
    let mut n = 0;
    for (ti, start, len, id) in spots {
        let t = &mut s.tracks[ti];
        let end = start + len;
        let before = t.clips.len();
        t.clips.retain(|c| c.id != id);
        if t.clips.len() == before {
            continue;
        }
        n += 1;
        for c in t.clips.iter_mut() {
            if c.start >= end - 1e-6 {
                c.start = (c.start - len).max(0.0);
            }
        }
        shift_track_automation(t, start, end);
    }
    if n > 0 {
        s.selected_clips.clear();
        s.selected_clip = None;
        s.touch();
    }
    n
}

/// Trim a clip to `a..b` seconds on the timeline (start, end or both). False when the clip
/// would be left with nothing.
pub fn trim_clip(c: &mut crate::session::Clip, a: f64, b: f64) -> bool {
    let ns = a.max(c.start);
    let ne = b.min(c.end());
    if ne - ns < 0.001 {
        return false;
    }
    let d = ns - c.start;
    if d > 0.0 {
        c.offset += d * c.speed;
        c.shift_env(d);
        c.start = ns;
    }
    c.len = ne - ns;
    c.fade_in = c.fade_in.min(c.len);
    c.fade_out = c.fade_out.min(c.len - c.fade_in);
    true
}

pub const HELP_TEXT: &str = "AudioCraft has two editors: the Waveform Editor (8) changes one audio file; the Multitrack \
Editor (9) arranges clips on tracks and mixes them.\n\n\
Every menu command is also in the Command Palette (Ctrl+Shift+P): type part of its name.\n\n\
Waveform: drag to select, then use Effects, Edit or Favorites. Shift+D shows the spectral display; \
E, D, P and B are its selection and healing tools.\n\n\
Multitrack: drag files from the Files panel onto a track. Drag a clip to move it, its edges to trim it, \
its corner squares to fade it. Drag a track by its coloured strip to move it up or down. In the Effects Rack, \
drag an effect by its name to another slot: a blue line shows where it will land.\n\n\
Edit > Keyboard Shortcuts (Alt+K) lists every shortcut; Edit > Preferences (Ctrl+Shift+K) has the settings.";

/// The same samples with a different declared rate: no resampling, so pitch and duration change.
pub fn with_interpreted_rate(d: &AudioData, rate: u32) -> AudioData {
    let mut out = d.clone();
    if rate > 0 {
        out.rate = rate;
    }
    out
}

/// Find the on-disk source of a clip, for Clip > Edit Original.
pub fn clip_source_path(
    s: &crate::session::Session,
    files: &[crate::app::AudioFile],
    clip_id: u64,
) -> Option<PathBuf> {
    let (t, c) = s.find_clip(clip_id)?;
    let fid = s.tracks.get(t)?.clips.get(c)?.file_id;
    files.iter().find(|f| f.id == fid)?.path.clone()
}

pub use crate::sysui::open_external;

impl App {
    /// Declare another sample rate for the current file without touching its samples.
    pub fn interpret_sample_rate(&mut self, rate: u32) -> Result<(), String> {
        if rate == 0 {
            return Err("A sample rate must be greater than zero.".into());
        }
        let old = self
            .cur()
            .map(|f| f.data.rate)
            .ok_or("Open a file in the Waveform editor first.")?;
        if old == rate {
            return Ok(());
        }
        self.stop();
        let (duration, new_duration) = {
            let Some(f) = self.cur_mut() else {
                return Ok(());
            };
            let scale = old.max(1) as f64 / rate as f64;
            let before = f.data.duration();
            f.selection = f.selection.map(|(a, b)| (a * scale, b * scale));
            f.cursor *= scale;
            f.view = (f.view.0 * scale, f.view.1 * scale);
            for m in f.markers.iter_mut() {
                m.time *= scale;
                m.duration *= scale;
            }
            let interpreted = with_interpreted_rate(&f.data, rate);
            f.push("Interpret Sample Rate", interpreted);
            (before, f.data.duration())
        };
        self.status =
            format!("Sample rate interpreted as {rate} Hz ({duration:.2} s → {new_duration:.2} s)");
        Ok(())
    }

    /// Split the selection (or the whole file) into separate band-limited files.
    pub fn split_frequency_bands(
        &mut self,
        bands: usize,
        crossovers: &[f32],
    ) -> Result<(), String> {
        let n = bands.clamp(2, 4);
        let (src, bits, float, name) = {
            let f = self
                .cur()
                .ok_or("Open a file in the Waveform editor first.")?;
            let (a, b) = f.range();
            (f.data.slice(a, b), f.bits, f.float, f.name.clone())
        };
        if src.frames() == 0 {
            return Err("There is no audio to split.".into());
        }
        let defaults = [250.0, 2000.0, 8000.0];
        let mut xs: Vec<f32> = crossovers.iter().copied().take(n - 1).collect();
        while xs.len() < n - 1 {
            let i = xs.len();
            xs.push(defaults.get(i).copied().unwrap_or(1000.0));
        }
        let parts = dsp::split_bands(&src, &xs);
        let stem = Path::new(&name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Audio".into());
        let labels: [&str; 4] = if n == 2 {
            ["Low", "High", "", ""]
        } else if n == 3 {
            ["Low", "Mid", "High", ""]
        } else {
            ["Low", "Low-Mid", "High-Mid", "High"]
        };
        let count = parts.len();
        for (i, data) in parts.into_iter().enumerate() {
            self.add_file(
                format!(
                    "{stem} {} Band.wav",
                    labels.get(i).copied().unwrap_or("Band")
                ),
                None,
                data,
                bits,
                float,
                "Frequency Band Split",
            );
        }
        self.status = format!("Split {name} into {count} frequency bands");
        Ok(())
    }

    /// File types shown by Open and Import. Sessions and AudioCraft projects come first: the
    /// person asked for those to be the default, with ordinary audio still one click away.
    pub fn media_dialog_filters() -> [(&'static str, &'static [&'static str]); 3] {
        [
            (
                "Session and AudioCraft files (*.sesx, *.acsession, *.audiocraft)",
                &["sesx", "acsession", "audiocraft"],
            ),
            ("Audio files", audio::AUDIO_EXTS),
            ("All files", &["*"]),
        ]
    }

    fn pick_audio_files(&self) -> Option<Vec<PathBuf>> {
        let mut dlg = rfd::FileDialog::new();
        for (name, exts) in Self::media_dialog_filters() {
            dlg = dlg.add_filter(name, exts);
        }
        dlg.ask_files()
    }

    /// Ask about anything unsaved among `items`, then close what `after` says.
    pub fn close_with_prompt(&mut self, after: CloseAfter) {
        let (session, files) = self.close_targets(after);
        let mut items: Vec<CloseItem> = files
            .iter()
            .filter(|id| {
                self.file(**id)
                    .map(|f| f.dirty() && f.data.frames() > 0)
                    .unwrap_or(false)
            })
            .map(|id| CloseItem::File(*id))
            .collect();
        if session
            && self
                .session
                .as_ref()
                .map(|s| s.dirty && s.tracks.iter().any(|t| !t.clips.is_empty()))
                .unwrap_or(false)
        {
            items.insert(0, CloseItem::Session);
        }
        if items.is_empty() || self.screenshot.is_some() {
            self.finish_close_after(after);
        } else {
            self.close_after = Some(after);
            self.close_queue = Some((items, false));
        }
    }

    /// (close the session?, which files) for a close command.
    fn close_targets(&self, after: CloseAfter) -> (bool, Vec<u64>) {
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
        match after {
            CloseAfter::All => (true, self.files.iter().map(|f| f.id).collect()),
            CloseAfter::SessionAndMedia => (
                true,
                self.files
                    .iter()
                    .map(|f| f.id)
                    .filter(|id| used.contains(id))
                    .collect(),
            ),
            CloseAfter::Unused => (
                false,
                self.files
                    .iter()
                    .map(|f| f.id)
                    .filter(|id| !used.contains(id))
                    .collect(),
            ),
        }
    }

    /// The close prompt is done (every answer given): close what was asked for.
    pub fn finish_close_after(&mut self, after: CloseAfter) {
        let (session, files) = self.close_targets(after);
        self.close_after = None;
        self.stop();
        if session && self.session.is_some() {
            self.session = None;
            self.session_hist.clear();
            self.session_hist_pos = 0;
            self.multi_drag.key_sel.clear();
            self.rack_drag = None;
        }
        let n = files.len();
        for id in files {
            self.close_file(id);
        }
        if self.session.is_none() && self.mode == Mode::Multitrack {
            self.mode = Mode::Waveform;
        }
        self.status = match after {
            CloseAfter::All => "Closed everything".into(),
            CloseAfter::SessionAndMedia => format!("Closed the session and {n} files"),
            CloseAfter::Unused => format!("Closed {n} unused files"),
        };
    }

    pub(crate) fn selected_clip_ids(&self) -> Vec<u64> {
        let Some(s) = &self.session else {
            return vec![];
        };
        if !s.selected_clips.is_empty() {
            s.selected_clips.clone()
        } else {
            s.selected_clip.into_iter().collect()
        }
    }

    /// Run one of `MORE_COMMANDS`. `Ok(false)` = not one of these.
    pub fn exec_more(&mut self, id: &str, ctx: &egui::Context) -> Result<bool, String> {
        match id {
            // ---- File --------------------------------------------------------------------
            "file.open_append_new" | "file.open_append_current" => {
                let to_current = id == "file.open_append_current";
                if to_current && self.cur().is_none() {
                    return Err("Open a file first: the audio is added to the end of it.".into());
                }
                let Some(paths) = self.pick_audio_files() else {
                    return Ok(true);
                };
                let mut parts = Vec::new();
                for p in &paths {
                    match audio::load(p) {
                        Ok(lf) => parts.push(lf),
                        Err(e) => self.status = format!("Skipped {}: {e}", p.display()),
                    }
                }
                if parts.is_empty() {
                    return Err("None of those files could be opened.".into());
                }
                if to_current {
                    let n = parts.len();
                    self.transform("Open Append", move |d, _, _| {
                        parts.iter().fold(d.clone(), |acc, p| append(&acc, &p.data))
                    });
                    self.switch_mode(Mode::Waveform);
                    self.status = format!("Appended {n} files");
                } else {
                    let (bits, float) = (parts[0].bits, parts[0].float);
                    let mut all = parts[0].data.clone();
                    for p in &parts[1..] {
                        all = append(&all, &p.data);
                    }
                    let stem = paths[0]
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "Appended".into());
                    self.add_file(
                        format!("{stem} (appended).wav"),
                        None,
                        all,
                        bits,
                        float,
                        "Open Append",
                    );
                    self.switch_mode(Mode::Waveform);
                }
            }
            "file.open_selected" => {
                let Some(id) = self.selected_file.or(self.current) else {
                    return Err("Select a file in the Files list first.".into());
                };
                let name = self
                    .file(id)
                    .map(|f| f.name.clone())
                    .unwrap_or_else(|| "file".into());
                self.current = Some(id);
                self.switch_mode(Mode::Waveform);
                self.status = format!("Opened {name} in the Waveform editor");
            }
            "file.insert_selected" => {
                let Some(id) = self.selected_file.or(self.current) else {
                    return Err("Select a file in the Files list first.".into());
                };
                self.insert_into_multitrack(id);
            }
            "file.close_selected" => {
                let Some(id) = self.selected_file.or(self.current) else {
                    return Err("Select a file in the Files list first.".into());
                };
                self.request_close_file(id);
            }
            "file.close_all" => self.close_with_prompt(CloseAfter::All),
            "file.close_unused" => {
                if self.session.is_none() {
                    return Err("There is no multitrack session, so no file is in use.".into());
                }
                self.close_with_prompt(CloseAfter::Unused);
            }
            "file.close_session_media" => {
                if self.session.is_none() {
                    return Err("No multitrack session is open.".into());
                }
                self.close_with_prompt(CloseAfter::SessionAndMedia);
            }
            "file.save_selection_as" => {
                let f = self
                    .cur()
                    .ok_or("Open a file in the Waveform editor first.")?;
                if f.selection.is_none() {
                    return Err("Select part of the file first.".into());
                }
                let (a, b) = f.range();
                let mut d = AudioData::new(f.data.rate, f.data.channels(), 0);
                for (ci, c) in d.ch.iter_mut().enumerate() {
                    *c = f.data.ch[ci][a.min(b)..b].to_vec();
                }
                let stem = std::path::Path::new(&f.name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Audio".into());
                let (bits, float) = (f.bits, f.float);
                let mut dlg = rfd::FileDialog::new()
                    .add_filter("WAV audio", &["wav"])
                    .set_file_name(format!("{stem} selection.wav"));
                if let Some(dir) = self.default_save_dir() {
                    dlg = dlg.set_directory(dir);
                }
                let Some(p) = dlg.ask_save() else {
                    return Ok(true);
                };
                audio::save_wav(&p, &d, if float { 32 } else { bits }, float)
                    .map_err(|e| format!("Could not save: {e}"))?;
                self.note_saved(&p);
                self.status = format!("Saved the selection as {}", p.display());
            }
            "file.save_all" => {
                let ids: Vec<u64> = self
                    .files
                    .iter()
                    .filter(|f| f.dirty())
                    .map(|f| f.id)
                    .collect();
                let n = ids.len();
                for id in ids {
                    self.save_file(id, false);
                }
                if self.session.as_ref().map(|s| s.unsaved()).unwrap_or(false) {
                    self.save_session(false);
                }
                self.status = format!("Saved {n} files");
            }
            "file.revert" => {
                let f = self
                    .cur()
                    .ok_or("Open a file in the Waveform editor first.")?;
                let p = f
                    .path
                    .clone()
                    .ok_or("This file has never been saved, so there is nothing to go back to.")?;
                if !f.dirty() {
                    return Err("The file has no unsaved changes.".into());
                }
                let lf =
                    audio::load(&p).map_err(|e| format!("Could not read {}: {e}", p.display()))?;
                if let Some(f) = self.cur_mut() {
                    f.push("Revert", lf.data);
                    f.saved_pos = Some(f.hist_pos);
                    f.selection = None;
                }
                self.status = "Reverted to the saved file".into();
            }
            "file.reveal" => {
                let p = match self.mode {
                    Mode::Multitrack => self.session.as_ref().and_then(|s| s.path.clone()),
                    Mode::Waveform => self.cur().and_then(|f| f.path.clone()),
                }
                .ok_or("Save it first: it doesn't have a place on disk yet.")?;
                reveal(&p).map_err(|e| format!("Could not open the file browser: {e}"))?;
            }

            // ---- Edit --------------------------------------------------------------------
            "edit.repeat_last" => {
                let last = self
                    .last_cmd
                    .clone()
                    .ok_or("Nothing to repeat yet: run a process or effect first.")?;
                self.exec(&last, ctx)?;
            }
            "edit.copy_to_new" => {
                if self.mode != Mode::Waveform || self.cur().is_none() {
                    return Err("Copy to New works on a file in the Waveform editor.".into());
                }
                self.copy();
                self.paste_new();
            }
            "edit.select_view" | "edit.select_to_start" | "edit.select_to_end" => {
                let cur = self.cursor();
                match self.mode {
                    Mode::Waveform => {
                        let f = self.cur_mut().ok_or("Open a file first.")?;
                        let dur = f.data.duration();
                        f.selection = Some(match id {
                            "edit.select_view" => f.view,
                            "edit.select_to_start" => (0.0, cur),
                            _ => (cur, dur),
                        });
                    }
                    Mode::Multitrack => {
                        let s = self.session.as_mut().ok_or("No session is open.")?;
                        let len = s.length();
                        s.selection = Some(match id {
                            "edit.select_view" => s.view,
                            "edit.select_to_start" => (0.0, cur),
                            _ => (cur, len.max(cur)),
                        });
                    }
                }
            }
            "edit.select_track_clips" => {
                let s = self.session.as_mut().ok_or("No session is open.")?;
                let t = s
                    .tracks
                    .get(s.selected_track)
                    .ok_or("Select a track first.")?;
                let ids: Vec<u64> = t.clips.iter().map(|c| c.id).collect();
                s.selected_clip = ids.first().copied();
                s.selected_clips = ids;
            }
            "edit.ripple_delete_clips" => {
                let ids = self.selected_clip_ids();
                if ids.is_empty() {
                    return Err("Select the clips to delete first.".into());
                }
                let n = self
                    .session
                    .as_mut()
                    .map(|s| ripple_delete_clips(s, &ids))
                    .unwrap_or(0);
                if n > 0 {
                    self.session_push("Ripple Delete Clips");
                }
            }
            "edit.ripple_delete_time" => {
                let sel = self
                    .session
                    .as_ref()
                    .and_then(|s| s.selection)
                    .filter(|(a, b)| b - a > 0.001);
                let Some((a, b)) = sel else {
                    return Err("Make a time selection in the Multitrack editor first.".into());
                };
                if let Some(s) = self.session.as_mut() {
                    ripple_delete_time(s, a, b);
                    s.selection = None;
                    s.cursor = a;
                }
                self.session_push("Ripple Delete Time Selection");
            }
            "edit.zero_cross_in" | "edit.zero_cross_out" => {
                let inward = id == "edit.zero_cross_in";
                let f = self
                    .cur_mut()
                    .ok_or("Open a file in the Waveform editor first.")?;
                let (sa, sb) = f.selection.ok_or("Make a selection first.")?;
                let r = f.data.rate as f64;
                let (ia, ib) = ((sa * r).round() as usize, (sb * r).round() as usize);
                let na = zero_crossing(&f.data, ia, if inward { 1 } else { -1 });
                let nb = zero_crossing(&f.data, ib, if inward { -1 } else { 1 });
                if nb > na {
                    f.selection = Some((na as f64 / r, nb as f64 / r));
                }
            }
            "edit.enable_left" | "edit.enable_right" => {
                let ci = if id == "edit.enable_left" { 0 } else { 1 };
                let f = self
                    .cur_mut()
                    .ok_or("Open a file in the Waveform editor first.")?;
                if ci >= f.enabled_ch.len() {
                    return Err("This file has only one channel.".into());
                }
                f.enabled_ch[ci] = !f.enabled_ch[ci];
                // at least one channel stays editable
                if !f.enabled_ch.iter().any(|x| *x) {
                    f.enabled_ch[1 - ci] = true;
                }
            }
            "edit.extract_mono" => {
                if self.cur().map(|f| f.data.channels() < 2).unwrap_or(true) {
                    return Err("Open a file with two or more channels first.".into());
                }
                self.extract_channels();
            }
            "edit.interpret_sample_rate" => {
                let rate = self.cur().map(|f| f.data.rate).unwrap_or(48000);
                self.dialog = Some(crate::app::Dialog::InterpretSampleRate { rate });
            }
            "edit.frequency_band_splitter" => {
                if self.cur().is_none() {
                    return Err("Open a file in the Waveform editor first.".into());
                }
                self.dialog = Some(crate::app::Dialog::FrequencySplit {
                    bands: 3,
                    cross: [250.0, 2000.0, 8000.0],
                });
            }
            "edit.keyboard_shortcuts" => self.dialog = Some(crate::app::Dialog::Shortcuts),

            // ---- Multitrack ----------------------------------------------------------------
            "multitrack.insert_files" => {
                let Some(paths) = self.pick_audio_files() else {
                    return Ok(true);
                };
                self.ensure_session();
                let (mut track, at) = self
                    .session
                    .as_ref()
                    .map(|s| (s.selected_track, s.cursor))
                    .unwrap_or((0, 0.0));
                for p in paths {
                    let known = self
                        .files
                        .iter()
                        .find(|f| f.path.as_deref() == Some(p.as_path()))
                        .map(|f| f.id);
                    let fid = match known {
                        Some(f) => Some(f),
                        None => {
                            let before = self.files.len();
                            self.open_path(&p);
                            if self.files.len() > before {
                                self.files.last().map(|f| f.id)
                            } else {
                                None
                            }
                        }
                    };
                    self.mode = Mode::Multitrack;
                    if let Some(fid) = fid {
                        self.place_file(fid, track, at);
                        track += 1;
                    }
                }
            }
            "multitrack.bounce_track" | "multitrack.bounce_selection" => {
                let whole_track = id == "multitrack.bounce_track";
                let s = self.session.as_ref().ok_or("No session is open.")?;
                let mut copy = s.clone();
                let (a, b, name) = if whole_track {
                    let ti = s.selected_track;
                    let t = s.tracks.get(ti).ok_or("Select a track first.")?;
                    if t.clips.is_empty() {
                        return Err("The selected track has no clips.".into());
                    }
                    for (i, tr) in copy.tracks.iter_mut().enumerate() {
                        tr.solo = false;
                        tr.mute = i != ti;
                    }
                    let end = t.clips.iter().map(|c| c.end()).fold(0.0, f64::max);
                    (0.0, end, format!("{} Bounce.wav", t.name))
                } else {
                    let (a, b) = s
                        .selection
                        .filter(|(a, b)| b - a > 0.001)
                        .ok_or("Make a time selection first.")?;
                    (a, b, format!("{} Bounce.wav", s.name))
                };
                let files = &self.files;
                let mix = copy
                    .snapshot(&|fid| files.iter().find(|f| f.id == fid).map(|f| f.data.clone()));
                let d = crate::session::mixdown(&mix, copy.rate, a, b);
                let stereo = d.channels() > 1;
                let fid = self.add_file(name, None, d, 32, true, "Bounce");
                let ti = self
                    .session
                    .as_mut()
                    .map(|s| s.add_track(stereo))
                    .unwrap_or(0);
                self.add_clip_at(fid, ti, a);
                self.mode = Mode::Multitrack;
                self.status = "Bounced to a new track".into();
            }

            // ---- Clip --------------------------------------------------------------------
            "clip.trim_selection" | "clip.trim_start" | "clip.trim_end" => {
                let ids = self.selected_clip_ids();
                if ids.is_empty() {
                    return Err("Select a clip first.".into());
                }
                let s = self.session.as_mut().ok_or("No session is open.")?;
                let (a, b) = match id {
                    "clip.trim_selection" => s
                        .selection
                        .filter(|(a, b)| b > a)
                        .ok_or("Make a time selection first.")?,
                    "clip.trim_start" => (s.cursor, f64::INFINITY),
                    _ => (f64::NEG_INFINITY, s.cursor),
                };
                let mut n = 0;
                for cid in ids {
                    if let Some(c) = s.clip_mut(cid) {
                        if trim_clip(c, a, b) {
                            n += 1;
                        }
                    }
                }
                if n == 0 {
                    return Err("The playhead or selection must be inside the clip.".into());
                }
                s.touch();
                self.session_push("Trim Clip");
            }
            "clip.mute" => {
                let ids = self.selected_clip_ids();
                if ids.is_empty() {
                    return Err("Select a clip first.".into());
                }
                if let Some(s) = self.session.as_mut() {
                    for cid in ids {
                        if let Some(c) = s.clip_mut(cid) {
                            c.muted = !c.muted;
                        }
                    }
                    s.touch();
                }
                self.session_push("Mute Clip");
            }
            "clip.edit_source" => self.edit_selected_clip_source(),
            "clip.edit_original" => {
                let cid = self
                    .selected_clip_ids()
                    .into_iter()
                    .next()
                    .ok_or("Select a clip first.")?;
                let path = self
                    .session
                    .as_ref()
                    .and_then(|s| clip_source_path(s, &self.files, cid))
                    .ok_or("That clip has no source file on disk yet. Save the file first.")?;
                open_external(&path)
                    .map_err(|e| format!("Could not open the original file: {e}"))?;
                self.status = format!("Opened {} in its default application", path.display());
            }

            // ---- View / Help ---------------------------------------------------------------
            "view.zoom_in_amp" => {
                self.drag_state.amp_zoom = (self.drag_state.amp_zoom * 1.5).min(64.0)
            }
            "view.zoom_out_amp" => {
                self.drag_state.amp_zoom = (self.drag_state.amp_zoom / 1.5).max(0.25)
            }
            "view.zoom_reset_amp" => self.drag_state.amp_zoom = 1.0,
            "help.manual" => self.message("AudioCraft Help", HELP_TEXT),
            "file.export_file" => {
                let id = self.current.ok_or("Open a file first.")?;
                self.open_export(crate::export::ExportSource::File(id))?;
            }
            "file.export_mixdown_selection" => {
                self.open_export(crate::export::ExportSource::Mixdown { selection: true })?
            }
            "file.export_settings" => self.settings_transfer(true)?,
            "file.import_settings" => self.settings_transfer(false)?,
            "process.doppler" => {
                self.dialog = Some(crate::app::Dialog::Doppler(Default::default()))
            }
            "process.sound_remover" => {
                self.dialog = Some(crate::app::Dialog::SoundRemover {
                    reduce_db: 30.0,
                    strength: 0.8,
                })
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

pub use crate::sysui::reveal;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Clip, Session};

    fn clip(id: u64, start: f64, len: f64) -> Clip {
        Clip {
            id,
            file_id: 1,
            name: format!("c{id}"),
            start,
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
        }
    }

    #[test]
    fn open_and_import_offer_sessions_before_audio() {
        let filters = App::media_dialog_filters();
        let first = filters[0].0;
        assert!(first.contains("Session"), "{first}");
        assert!(filters[0].1.contains(&"sesx"));
        assert!(filters[0].1.contains(&"acsession"));
        assert!(filters[0].1.contains(&"audiocraft"));
        assert_eq!(filters[1].1, audio::AUDIO_EXTS);
    }

    #[test]
    fn every_extra_command_is_documented_and_unique() {
        for c in MORE_COMMANDS {
            assert!(!c.title.is_empty() && !c.doc.is_empty(), "{}", c.id);
            assert_eq!(
                crate::commands::all().filter(|d| d.id == c.id).count(),
                1,
                "{} twice",
                c.id
            );
        }
    }

    #[test]
    fn zero_crossings_move_the_selection_onto_a_sign_change() {
        let mut d = AudioData::new(1000, 1, 1000);
        // a 10 Hz sine: zero at every 50 samples
        for i in 0..1000 {
            d.ch[0][i] = (i as f32 * std::f32::consts::TAU / 100.0).sin();
        }
        let a = zero_crossing(&d, 20, 1);
        assert!((a as i64 - 50).abs() <= 1, "{a}");
        let b = zero_crossing(&d, 20, -1);
        assert!(b <= 1, "{b}");
        // never out of range, even at the ends or on silence
        assert!(zero_crossing(&d, 5000, 1) <= 1000);
        let silent = AudioData::new(1000, 2, 10);
        assert_eq!(zero_crossing(&silent, 3, 1), 3);
        assert_eq!(zero_crossing(&AudioData::new(1000, 1, 0), 0, 1), 0);
    }

    #[test]
    fn append_conforms_rate_and_channels() {
        let mut a = AudioData::new(48000, 2, 100);
        a.ch[0].fill(0.5);
        let mut b = AudioData::new(48000, 1, 50);
        b.ch[0].fill(0.25);
        let out = append(&a, &b);
        assert_eq!(out.channels(), 2);
        assert_eq!(out.frames(), 150);
        assert_eq!(out.ch[1][120], 0.25, "mono goes to both channels");
        let c = AudioData::new(24000, 2, 100);
        let out = append(&a, &c);
        assert_eq!(out.rate, 48000);
        assert!((out.frames() as i64 - 300).abs() <= 2, "{}", out.frames());
    }

    #[test]
    fn ripple_delete_time_closes_the_gap_in_every_track() {
        let mut s = Session::new("t", 48000, 2);
        s.tracks[0].clips = vec![clip(101, 0.0, 10.0)];
        s.tracks[1].clips = vec![clip(102, 6.0, 2.0), clip(103, 12.0, 3.0)];
        s.next_id = 200;
        ripple_delete_time(&mut s, 4.0, 7.0);
        let t0: Vec<(f64, f64)> = s.tracks[0].clips.iter().map(|c| (c.start, c.len)).collect();
        assert_eq!(t0.len(), 2);
        assert!((t0[0].0 - 0.0).abs() < 1e-9 && (t0[0].1 - 4.0).abs() < 1e-9);
        assert!(
            (t0[1].0 - 4.0).abs() < 1e-9 && (t0[1].1 - 3.0).abs() < 1e-9,
            "{t0:?}"
        );
        // the right part keeps playing the audio from 7 s on
        assert!((s.tracks[0].clips[1].offset - 7.0).abs() < 1e-9);
        let mut t1: Vec<(f64, f64)> = s.tracks[1].clips.iter().map(|c| (c.start, c.len)).collect();
        t1.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(t1.len(), 2, "{t1:?}");
        assert!(
            (t1[0].0 - 4.0).abs() < 1e-9 && (t1[0].1 - 1.0).abs() < 1e-9,
            "{t1:?}"
        );
        assert!((t1[1].0 - 9.0).abs() < 1e-9, "{t1:?}");
        // nothing selected: nothing happens
        assert_eq!(ripple_delete_time(&mut s, 5.0, 5.0), 0);
    }

    #[test]
    fn ripple_delete_clips_pulls_later_clips_left() {
        let mut s = Session::new("t", 48000, 1);
        s.tracks[0].clips = vec![
            clip(1, 0.0, 2.0),
            clip(2, 2.0, 3.0),
            clip(3, 5.0, 1.0),
            clip(4, 8.0, 1.0),
        ];
        assert_eq!(ripple_delete_clips(&mut s, &[2, 99]), 1);
        let starts: Vec<f64> = s.tracks[0].clips.iter().map(|c| c.start).collect();
        assert_eq!(starts, vec![0.0, 2.0, 5.0]);
    }

    #[test]
    fn ripple_delete_carries_automation_points_with_the_audio() {
        let mut s = Session::new("t", 48000, 2);
        s.tracks[0].clips = vec![clip(101, 0.0, 10.0)];
        s.tracks[0].vol_env = vec![(1.0, 0.0), (5.0, -6.0), (8.0, 3.0)];
        s.tracks[0].auto.vol_h = vec![Handles::smooth(0.2, 0.1); 3];
        s.tracks[0].auto.pan_env = vec![(6.0, 0.5)];
        s.tracks[0].auto.pan_h = vec![Handles::smooth(0.1, 0.2)];
        let pi = s.tracks[0].auto.param_mut("rack.mix", "Mix", "", 0.0, 1.0);
        s.tracks[0].auto.params[pi].env = vec![(7.0, 0.5)];
        s.master.vol_env = vec![(9.0, 0.0)];
        s.master.vol_h = vec![Handles::smooth(0.1, 0.0)];
        ripple_delete_time(&mut s, 4.0, 6.0);
        let vol: Vec<f64> = s.tracks[0].vol_env.iter().map(|(t, _)| *t).collect();
        assert_eq!(
            vol,
            vec![1.0, 6.0],
            "points inside the cut go, later ones move left"
        );
        assert_eq!(
            s.tracks[0].auto.vol_h.len(),
            2,
            "handles stay beside their keyframes"
        );
        assert!((s.tracks[0].auto.vol_h[1].out_dt - 0.2).abs() < 1e-9);
        assert_eq!(s.tracks[0].auto.pan_env[0].0, 4.0);
        assert_eq!(s.tracks[0].auto.params[pi].env[0].0, 5.0);
        assert_eq!(s.master.vol_env[0].0, 7.0);
    }

    #[test]
    fn ripple_delete_clips_moves_the_lane_on_that_track_only() {
        let mut s = Session::new("t", 48000, 2);
        s.tracks[0].clips = vec![clip(1, 0.0, 2.0), clip(2, 2.0, 3.0)];
        s.tracks[0].vol_env = vec![(1.0, 0.0), (3.0, -2.0), (6.0, 1.0)];
        s.tracks[1].vol_env = vec![(6.0, 0.0)];
        assert_eq!(ripple_delete_clips(&mut s, &[2]), 1);
        let vol: Vec<f64> = s.tracks[0].vol_env.iter().map(|(t, _)| *t).collect();
        assert_eq!(
            vol,
            vec![1.0, 3.0],
            "a point in the removed clip goes, later ones shift"
        );
        assert_eq!(
            s.tracks[1].vol_env[0].0, 6.0,
            "other tracks keep their automation"
        );
    }

    #[test]
    fn trim_keeps_the_audio_in_place() {
        let mut c = clip(1, 10.0, 5.0);
        c.fade_in = 1.0;
        c.env = vec![(0.0, 0.0), (4.0, -6.0)];
        c.curves.env_h = vec![Default::default(); 2];
        assert!(trim_clip(&mut c, 12.0, f64::INFINITY));
        assert_eq!((c.start, c.len, c.offset), (12.0, 3.0, 2.0));
        assert!(
            (c.env.last().unwrap().0 - 2.0).abs() < 1e-9,
            "the envelope moves with the audio"
        );
        assert!(trim_clip(&mut c, f64::NEG_INFINITY, 14.0));
        assert_eq!(c.len, 2.0);
        // a trim that would leave nothing is refused
        assert!(!trim_clip(&mut c, 20.0, 30.0));
        assert_eq!(c.len, 2.0);
    }

    #[test]
    fn next_clip_is_found_by_time_not_list_order() {
        let clips = vec![clip(1, 10.0, 2.0), clip(2, 0.0, 2.0), clip(3, 5.0, 2.0)];
        assert_eq!(crate::commands::next_clip_index(&clips, 1), Some(2));
        assert_eq!(crate::commands::next_clip_index(&clips, 2), Some(0));
        assert_eq!(crate::commands::next_clip_index(&clips, 0), None);
        assert_eq!(crate::commands::next_clip_index(&clips, 9), None);
    }

    #[test]
    fn repeatable_commands() {
        assert!(
            repeatable("process.normalize")
                && repeatable("effect.delay")
                && repeatable("favorite.fade_in")
        );
        assert!(!repeatable("edit.repeat_last") && !repeatable("file.save"));
    }

    #[test]
    fn interpret_rate_keeps_the_samples_and_changes_the_duration() {
        let mut d = AudioData::new(48000, 2, 96000);
        d.ch[0][123] = 0.5;
        let interpreted = with_interpreted_rate(&d, 96000);
        assert_eq!(interpreted.rate, 96000);
        assert_eq!(interpreted.frames(), 96000);
        assert_eq!(interpreted.ch[0][123], 0.5, "no resampling happened");
        assert!((interpreted.duration() - 1.0).abs() < 1e-9);
        assert_eq!(
            with_interpreted_rate(&d, 0).rate,
            48000,
            "an invalid rate is ignored"
        );
    }

    #[test]
    fn edit_original_finds_the_clip_source_file() {
        let data = AudioData::new(48000, 1, 48000);
        let path = PathBuf::from("C:\\audio\\voice.wav");
        let f = crate::app::AudioFile::new(
            7,
            "voice.wav".into(),
            Some(path.clone()),
            data,
            16,
            false,
            "Open",
        );
        let mut s = Session::new("t", 48000, 1);
        s.tracks[0].clips.push(clip(42, 0.0, 1.0));
        s.tracks[0].clips[0].file_id = 7;
        assert_eq!(clip_source_path(&s, &[f], 42), Some(path));
        assert_eq!(clip_source_path(&s, &[], 42), None);
        assert_eq!(clip_source_path(&s, &[], 99), None);
    }
}
