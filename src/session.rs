//! Multitrack session model and the mixing renderer shared by playback and export.

use crate::audio::{AudioData, undb};
use crate::curves::{self, Handles};
use crate::dsp::{Chain, EffectSlot, RackOpts};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Marker {
    pub name: String,
    pub time: f64,
    #[serde(default)]
    pub duration: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: u64,
    pub file_id: u64,
    pub name: String,
    pub start: f64,
    pub offset: f64,
    pub len: f64,
    pub gain_db: f32,
    pub muted: bool,
    pub fade_in: f64,
    pub fade_out: f64,
    /// Volume envelope: (time relative to clip start in seconds, gain in dB), sorted by time.
    #[serde(default)]
    pub env: Vec<(f64, f32)>,
    /// Pan envelope: (relative time, pan -1 = left .. +1 = right), sorted by time.
    #[serde(default)]
    pub pan_env: Vec<(f64, f32)>,
    /// Clip effects (Effects Rack > Clip Effects), applied before the track effects.
    #[serde(default)]
    pub rack: Vec<EffectSlot>,
    /// Source seconds played per timeline second (1 = normal; stretched clips from .sesx differ).
    #[serde(default = "one")]
    pub speed: f64,
    /// Clip Effects rack controls (power, input/output, mix).
    #[serde(default)]
    pub fx: RackOpts,
    /// While true, an overlap with the neighbouring clip sets this fade to a balanced crossfade
    /// spanning the overlap. Dragging the fade handle turns it off for that side.
    #[serde(default = "yes")]
    pub auto_in: bool,
    #[serde(default = "yes")]
    pub auto_out: bool,
    /// Keyframe handles and fade-curve points (AudioCraft additions; see `curves`).
    #[serde(default)]
    pub curves: ClipCurves,
}

/// A clip's curve extras: Bezier handles for its volume / pan keyframes (parallel to `env` /
/// `pan_env`) and points that shape its fade-in / fade-out (fraction from the silent end, gain).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipCurves {
    pub env_h: Vec<Handles>,
    pub pan_h: Vec<Handles>,
    pub fin_pts: Vec<(f64, f32)>,
    pub fin_h: Vec<Handles>,
    pub fout_pts: Vec<(f64, f32)>,
    pub fout_h: Vec<Handles>,
    /// The fade-in / fade-out curve's shape, set by dragging its handle up or down:
    /// 0 the plain curve, up to +1 bulging up (fast, logarithmic-like), down to -1 sagging
    /// (slow, exponential-like).
    pub fin_shape: f32,
    pub fout_shape: f32,
}

impl ClipCurves {}

/// A track's automation lane (the chevron left of the Read selector opens it under the track):
/// pan automation and the lane's own settings. Volume automation keeps its older `vol_env`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackAuto {
    pub vol_h: Vec<Handles>,
    pub pan_env: Vec<(f64, f32)>,
    pub pan_h: Vec<Handles>,
    /// Height of the open lane, below the track's clips.
    pub lane_h: f32,
    /// Which envelopes the lane shows (Show Envelopes), and which one it edits (Select).
    pub show_vol: bool,
    pub show_pan: bool,
    /// 0 = Volume, 1 = Pan.
    pub param: u8,
    /// Protect the lane's keyframes from edits.
    pub lock: bool,
    /// Every other automated parameter (Show Envelopes): Mute, the rack's Input/Output/Mix/Power,
    /// Track EQ settings and each effect's settings.
    pub params: Vec<AutoParam>,
    /// The edited envelope when `param` is 2: an `AutoParam` key.
    pub sel: String,
}

impl Default for TrackAuto {
    fn default() -> Self {
        TrackAuto {
            vol_h: vec![],
            pan_env: vec![],
            pan_h: vec![],
            lane_h: 120.0,
            show_vol: true,
            show_pan: false,
            param: 0,
            lock: false,
            params: vec![],
            sel: String::new(),
        }
    }
}

/// One automatable track parameter besides volume and pan. Keys: "mute", "rack.in",
/// "rack.out", "rack.mix", "rack.power", "eq.<path>" (Track EQ) and "fx.<slot>.<path>" (an
/// effect in the track's rack; `fx_name` must still match the effect in that slot).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoParam {
    pub key: String,
    pub label: String,
    pub fx_name: String,
    pub env: Vec<(f64, f32)>,
    pub hs: Vec<Handles>,
    pub shown: bool,
    pub min: f32,
    pub max: f32,
}

impl AutoParam {
    /// On/off parameters (their keyframes hold, and 0.5 is the switch point).
    pub fn binary(&self) -> bool {
        self.key == "mute" || self.key == "rack.power"
    }
}

impl TrackAuto {
    /// The parameter with this key, made (with its range and label) when it isn't there yet.
    pub fn param_mut(
        &mut self,
        key: &str,
        label: &str,
        fx_name: &str,
        min: f32,
        max: f32,
    ) -> usize {
        if let Some(i) = self.params.iter().position(|p| p.key == key) {
            return i;
        }
        let (lo, hi) = if max > min {
            (min, max)
        } else {
            (min, min + 1.0)
        };
        self.params.push(AutoParam {
            key: key.into(),
            label: label.into(),
            fx_name: fx_name.into(),
            min: lo,
            max: hi,
            ..Default::default()
        });
        self.params.len() - 1
    }
}

/// Automation of a track's other parameters, as played (straight-line keyframes).
#[derive(Clone, Debug, Default)]
pub struct MixAuto {
    pub mute: Vec<(f64, f32)>,
    pub rack_in: Vec<(f64, f32)>,
    pub rack_out: Vec<(f64, f32)>,
    pub rack_mix: Vec<(f64, f32)>,
    pub rack_power: Vec<(f64, f32)>,
    /// (rack slot, the effect's name, parameter path, envelope)
    pub fx: Vec<(usize, String, String, Vec<(f64, f32)>)>,
}

impl MixAuto {
    fn rack_opts(&self, base: &RackOpts, t: f64) -> Option<RackOpts> {
        if self.rack_in.is_empty()
            && self.rack_out.is_empty()
            && self.rack_mix.is_empty()
            && self.rack_power.is_empty()
        {
            return None;
        }
        let mut o = base.clone();
        if !self.rack_in.is_empty() {
            o.in_db = env_db(&self.rack_in, t).clamp(-96.0, 48.0);
        }
        if !self.rack_out.is_empty() {
            o.out_db = env_db(&self.rack_out, t).clamp(-96.0, 48.0);
        }
        if !self.rack_mix.is_empty() {
            o.mix = env_db(&self.rack_mix, t).clamp(0.0, 1.0);
        }
        if !self.rack_power.is_empty() {
            o.power = env_db(&self.rack_power, t) >= 0.5;
        }
        Some(o)
    }
}

fn yes() -> bool {
    true
}

fn one() -> f64 {
    1.0
}

/// Envelope value in dB at relative time `rel` (0 dB when empty; held at the ends).
pub fn env_db(env: &[(f64, f32)], rel: f64) -> f32 {
    match env.len() {
        0 => 0.0,
        1 => env[0].1,
        _ => {
            if rel <= env[0].0 {
                return env[0].1;
            }
            for w in env.windows(2) {
                let (t0, d0) = w[0];
                let (t1, d1) = w[1];
                if rel <= t1 {
                    let f = if t1 - t0 > 1e-9 {
                        ((rel - t0) / (t1 - t0)) as f32
                    } else {
                        1.0
                    };
                    return d0 + (d1 - d0) * f;
                }
            }
            env[env.len() - 1].1
        }
    }
}

/// Fader gain from a track's automation lane at session time `t`. Off means no automation:
/// the plain fader value is used, so a written lane never changes playback until it is read.
#[inline]
pub fn automation_gain(env: &[(f64, f32)], t: f64, mode: u8) -> f32 {
    if mode == 0 { 1.0 } else { undb(env_db(env, t)) }
}

/// Move envelope points after the clip's left edge moved by `d` seconds, keeping the value at the new edge.
fn shift_points(v: &mut Vec<(f64, f32)>, d: f64) {
    if v.is_empty() || d.abs() < 1e-12 {
        return;
    }
    let edge = env_db(v, d);
    let mut out: Vec<(f64, f32)> = v
        .iter()
        .map(|(t, g)| (t - d, *g))
        .filter(|(t, _)| *t > 0.0)
        .collect();
    // a point at the new edge is only needed where the line is sloping there; a flat stretch
    // (or nothing left) needs no extra handle
    match out.first() {
        Some((_, g)) if (g - edge).abs() < 0.01 => {}
        // every point was before the edge: the value is held, one point keeps it
        None => out.push((0.0, edge)),
        _ => out.insert(0, (0.0, edge)),
    }
    *v = out;
}

/// `shift_points` for an envelope with handles: a curved envelope is first turned into
/// straight-line keyframes that follow the curve, then cut at the new edge.
fn shift_keys(v: &mut Vec<(f64, f32)>, hs: &mut Vec<Handles>, d: f64) {
    if v.is_empty() || d.abs() < 1e-12 {
        return;
    }
    if curves::has_curves(v.len(), hs) {
        *v = curves::bake(v, hs, false, 0.02);
    }
    shift_points(v, d);
    hs.clear();
    hs.resize(v.len(), Handles::default());
}

impl Clip {
    pub fn end(&self) -> f64 {
        self.start + self.len
    }
    /// Shift the envelope after the clip's left edge moved by `d` seconds (positive = later).
    pub fn shift_env(&mut self, d: f64) {
        shift_keys(&mut self.env, &mut self.curves.env_h, d);
        shift_keys(&mut self.pan_env, &mut self.curves.pan_h, d);
    }
    /// The volume envelope with its curves as straight-line keyframes (what plays, and what a
    /// .sesx file can hold).
    pub fn baked_env(&self, tol: f32) -> Vec<(f64, f32)> {
        curves::bake(&self.env, &self.curves.env_h, false, tol)
    }
    pub fn baked_pan(&self, tol: f32) -> Vec<(f64, f32)> {
        curves::bake(&self.pan_env, &self.curves.pan_h, false, tol)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: u64,
    pub name: String,
    pub volume_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub arm: bool,
    pub stereo: bool,
    pub clips: Vec<Clip>,
    pub rack: Vec<EffectSlot>,
    /// Track Effects rack controls (power, input/output, mix, pre/post fader).
    #[serde(default)]
    pub fx: RackOpts,
    /// Pre-render the track's effects (play a rendered copy instead of running them live).
    #[serde(default)]
    pub prerender: bool,
    pub color: [u8; 3],
    #[serde(default = "default_height")]
    pub height: f32,
    /// Input, output, phase, monitoring and automation mode.
    #[serde(default)]
    pub io: TrackIo,
    /// Folded down to a single row (name and M S R I).
    #[serde(default)]
    pub minimized: bool,
    /// Send slot (shown with the Sends header view).
    #[serde(default)]
    pub send: TrackSend,
    /// Track EQ (a parametric EQ after the track effects), switched by its power button.
    #[serde(default)]
    pub eq_on: bool,
    #[serde(default)]
    pub eq: Vec<crate::dsp::EqBand>,
    #[serde(default)]
    pub eq_master: f32,
    /// Hidden from the editor (Tracks panel eye); it still plays.
    #[serde(default)]
    pub hidden: bool,
    /// Track volume automation: (session time in seconds, dB), sorted by time.
    #[serde(default)]
    pub vol_env: Vec<(f64, f32)>,
    /// The chevron beside the automation mode reveals this lane in the timeline.
    #[serde(default)]
    pub show_vol_env: bool,
    /// The automation lane's pan envelope, handles and settings.
    #[serde(default)]
    pub auto: TrackAuto,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackSend {
    pub on: bool,
    /// Bus the send feeds ("None" until a bus exists).
    pub target: String,
    pub level_db: f32,
    pub pre_fader: bool,
}

impl Default for TrackSend {
    fn default() -> Self {
        TrackSend {
            on: false,
            target: "None".into(),
            level_db: -96.0,
            pre_fader: false,
        }
    }
}

/// The Master track: its own effects, mute, output and automation mode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterTrack {
    pub rack: Vec<EffectSlot>,
    pub fx: RackOpts,
    pub mute: bool,
    pub solo: bool,
    pub automation: u8,
    pub output: String,
    #[serde(default)]
    pub vol_env: Vec<(f64, f32)>,
    #[serde(default)]
    pub show_vol_env: bool,
    #[serde(default)]
    pub vol_h: Vec<Handles>,
}

impl Default for MasterTrack {
    fn default() -> Self {
        MasterTrack {
            rack: vec![],
            fx: RackOpts::default(),
            mute: false,
            solo: false,
            automation: 1,
            output: "Default Output".into(),
            vol_env: vec![],
            show_vol_env: false,
            vol_h: vec![],
        }
    }
}

/// The metronome row: a click on every beat (accented on the first of the bar).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Metronome {
    pub on: bool,
    pub bpm: f32,
    pub beats: u32,
    pub volume_db: f32,
    pub pan: f32,
    pub mute: bool,
}

impl Default for Metronome {
    fn default() -> Self {
        Metronome {
            on: false,
            bpm: 120.0,
            beats: 4,
            volume_db: -6.0,
            pan: 0.0,
            mute: false,
        }
    }
}

/// Track index used by the Effects Rack for the Master track.
pub const MASTER_INDEX: usize = usize::MAX;

/// Height of a minimised track.
pub const MIN_TRACK_H: f32 = 30.0;

impl Track {
    /// The height the track takes on screen.
    pub fn shown_height(&self) -> f32 {
        if self.hidden {
            0.0
        } else if self.minimized {
            MIN_TRACK_H
        } else {
            self.height + self.lane_height()
        }
    }
    /// Height of the open automation lane under the clips (0 when closed).
    pub fn lane_height(&self) -> f32 {
        if self.show_vol_env && !self.minimized {
            self.auto.lane_h.clamp(40.0, 600.0)
        } else {
            0.0
        }
    }
    /// Recording needs an input.
    pub fn can_record(&self) -> bool {
        !self.io.input.is_empty() && self.io.input != "None"
    }
}

fn mix_stereo() -> u8 {
    1
}

fn bits32() -> u16 {
    32
}

fn default_height() -> f32 {
    96.0
}

/// Automation modes, in menu order.
pub const AUTOMATION_MODES: [&str; 5] = ["Off", "Read", "Latch", "Touch", "Write"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackIo {
    /// Recording input ("None", "Default Stereo Input", "Mono: 1"...).
    pub input: String,
    /// Where the track goes (only the Master bus exists).
    pub output: String,
    /// Invert the polarity of the track's output.
    pub invert: bool,
    /// Monitor the input while armed.
    pub monitor: bool,
    /// Index into AUTOMATION_MODES.
    pub automation: u8,
}

impl Default for TrackIo {
    fn default() -> Self {
        // like the original, a new track has no input until one is chosen
        TrackIo {
            input: "None".into(),
            output: "Master".into(),
            invert: false,
            monitor: false,
            automation: 1,
        }
    }
}

pub const TRACK_COLORS: [[u8; 3]; 8] = [
    [46, 160, 120],
    [60, 130, 200],
    [190, 120, 60],
    [150, 90, 190],
    [180, 70, 100],
    [90, 160, 60],
    [200, 170, 60],
    [70, 170, 180],
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    #[serde(skip)]
    pub path: Option<std::path::PathBuf>,
    pub rate: u32,
    /// Bit depth of the session (32 = float).
    #[serde(default = "bits32")]
    pub bits: u16,
    /// Master mix: 0 mono, 1 stereo, 2 5.1 (played as stereo).
    #[serde(default = "mix_stereo")]
    pub mix: u8,
    pub tracks: Vec<Track>,
    pub master_db: f32,
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub master: MasterTrack,
    #[serde(default)]
    pub metronome: Metronome,
    /// Clip groups (Clip > Group): clip id -> group id. Grouped clips select, move and delete
    /// together.
    #[serde(default)]
    pub groups: std::collections::BTreeMap<u64, u64>,
    /// The Master track is the one selected (its effects show in the Effects Rack).
    #[serde(skip)]
    pub master_selected: bool,
    #[serde(skip)]
    pub cursor: f64,
    #[serde(skip)]
    pub view: (f64, f64),
    #[serde(skip)]
    pub selection: Option<(f64, f64)>,
    #[serde(skip)]
    pub selected_track: usize,
    #[serde(skip)]
    pub selected_clip: Option<u64>,
    /// Clips selected in the Multitrack area: a plain click selects one, Shift+click selects
    /// the range between the last selected clip and the clicked clip, Ctrl+click toggles it.
    #[serde(skip)]
    pub selected_clips: Vec<u64>,
    #[serde(skip)]
    pub dirty: bool,
    /// Fingerprint of the content when it was last saved or opened: a change that is undone,
    /// or a control that only "touches" the session, doesn't leave it asking to be saved.
    #[serde(skip)]
    pub saved_fp: Option<u64>,
    #[serde(skip)]
    pub revision: u64,
    #[serde(skip)]
    pub next_id: u64,
}

/// What one undo step of a session holds: the tracks (clips, envelopes, effects, automation
/// lanes), the master track (its effects and automation) and the clip groups.
#[derive(Clone, Debug, PartialEq)]
pub struct HistState {
    pub tracks: Vec<Track>,
    pub master: MasterTrack,
    pub master_db: f32,
    pub groups: std::collections::BTreeMap<u64, u64>,
}

impl Session {
    /// This session's content as an undo step.
    pub fn hist_state(&self) -> HistState {
        HistState {
            tracks: self.tracks.clone(),
            master: self.master.clone(),
            master_db: self.master_db,
            groups: self.groups.clone(),
        }
    }

    /// Put an undo step's content back.
    pub fn set_hist_state(&mut self, h: &HistState) {
        self.tracks = h.tracks.clone();
        self.master = h.master.clone();
        self.master_db = h.master_db;
        self.groups = h.groups.clone();
        self.touch();
    }

    pub fn new(name: &str, rate: u32, n_tracks: usize) -> Self {
        let mut s = Session {
            name: name.to_string(),
            path: None,
            rate,
            bits: 32,
            mix: 1,
            tracks: vec![],
            master_db: 0.0,
            markers: vec![],
            master: MasterTrack::default(),
            metronome: Metronome::default(),
            groups: Default::default(),
            master_selected: false,
            cursor: 0.0,
            view: (0.0, 60.0),
            selection: None,
            selected_track: 0,
            selected_clip: None,
            selected_clips: vec![],
            dirty: false,
            saved_fp: None,
            revision: 1,
            next_id: 1,
        };
        for _ in 0..n_tracks {
            s.add_track(true);
        }
        s.dirty = false;
        s
    }

    /// The clips in `id`'s group (just `id` when it has none), existing clips only.
    pub fn group_of(&self, id: u64) -> Vec<u64> {
        match self.groups.get(&id) {
            Some(g) => self
                .groups
                .iter()
                .filter(|(c, gg)| *gg == g && self.find_clip(**c).is_some())
                .map(|(c, _)| *c)
                .collect(),
            None => vec![id],
        }
    }

    /// Select `id` with the rest of its group.
    pub fn select_group_of(&mut self, id: u64) {
        let g = self.group_of(id);
        if g.len() > 1 {
            self.selected_clips = g;
            self.selected_clip = Some(id);
        }
    }

    /// Move the other clips of `id`'s group by `delta` seconds (none before 0).
    pub fn shift_group(&mut self, id: u64, delta: f64) {
        if delta == 0.0 {
            return;
        }
        let others: Vec<u64> = self.group_of(id).into_iter().filter(|c| *c != id).collect();
        for c in others {
            if let Some(cl) = self.clip_mut(c) {
                cl.start = (cl.start + delta).max(0.0);
            }
        }
    }

    pub fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// A fingerprint of everything a save writes (selection, view and other UI state left out).
    pub fn content_fp(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(self).unwrap_or_default().hash(&mut h);
        h.finish()
    }

    /// The session was just saved or opened: what it holds now is what's on disk.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
        self.saved_fp = Some(self.content_fp());
    }

    /// Whether closing should ask to save: something changed and the content really differs
    /// from what was saved (or it was never saved).
    pub fn unsaved(&self) -> bool {
        self.dirty
            && self
                .saved_fp
                .map(|f| f != self.content_fp())
                .unwrap_or(true)
    }

    pub fn touch(&mut self) {
        self.revision += 1;
        self.dirty = true;
    }

    /// Multitrack selection of clips: a plain click selects one; Shift+click selects the range
    /// between the last selected clip and the clicked one; Ctrl+click toggles the clicked
    /// clip in the set. The clicked clip is the active one (`selected_clip`).
    pub fn select_clips(&mut self, id: u64, shift: bool, ctrl: bool) {
        let flat: Vec<u64> = self
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect();
        let Some(pos) = flat.iter().position(|c| *c == id) else {
            return;
        };
        if shift {
            let anchor = self
                .selected_clip
                .and_then(|a| flat.iter().position(|c| *c == a));
            match anchor {
                Some(a) => {
                    let (lo, hi) = (a.min(pos), a.max(pos));
                    self.selected_clips = flat[lo..=hi].to_vec();
                }
                None => self.selected_clips = vec![id],
            }
        } else if ctrl {
            if self.selected_clips.contains(&id) {
                self.selected_clips.retain(|c| *c != id);
            } else {
                self.selected_clips.push(id);
            }
        } else {
            self.selected_clips = vec![id];
        }
        self.selected_clip = if self.selected_clips.is_empty() {
            None
        } else {
            Some(id)
        };
    }

    pub fn clear_selected_clips(&mut self) {
        self.selected_clips.clear();
        self.selected_clip = None;
    }

    pub fn add_track(&mut self, stereo: bool) -> usize {
        let n = self.tracks.len();
        let id = self.id();
        self.tracks.push(Track {
            id,
            name: format!("Track {}", n + 1),
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            arm: false,
            stereo,
            clips: vec![],
            rack: vec![],
            fx: RackOpts::default(),
            prerender: false,
            color: TRACK_COLORS[n % TRACK_COLORS.len()],
            height: crate::prefs::track_height(),
            io: TrackIo::default(),
            minimized: false,
            send: TrackSend::default(),
            eq_on: false,
            eq: crate::equi::standard_bands(),
            eq_master: 0.0,
            hidden: false,
            vol_env: vec![],
            show_vol_env: false,
            auto: Default::default(),
        });
        self.touch();
        n
    }

    pub fn length(&self) -> f64 {
        self.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.end())
            .fold(0.0, f64::max)
    }

    pub fn find_clip(&self, id: u64) -> Option<(usize, usize)> {
        for (ti, t) in self.tracks.iter().enumerate() {
            for (ci, c) in t.clips.iter().enumerate() {
                if c.id == id {
                    return Some((ti, ci));
                }
            }
        }
        None
    }

    pub fn clip_mut(&mut self, id: u64) -> Option<&mut Clip> {
        let (t, c) = self.find_clip(id)?;
        Some(&mut self.tracks[t].clips[c])
    }

    /// Split a clip at time t. Returns the id of the new right-hand clip.
    pub fn split_clip(&mut self, id: u64, t: f64) -> Option<u64> {
        let (ti, ci) = self.find_clip(id)?;
        let c = self.tracks[ti].clips[ci].clone();
        if t <= c.start + 0.001 || t >= c.end() - 0.001 {
            return None;
        }
        let nid = self.id();
        let left_len = t - c.start;
        let mut right = c.clone();
        right.id = nid;
        right.start = t;
        right.offset = c.offset + left_len * c.speed;
        right.len = c.len - left_len;
        right.fade_in = 0.0;
        right.curves.fin_pts.clear();
        right.curves.fin_h.clear();
        right.shift_env(left_len);
        let left = &mut self.tracks[ti].clips[ci];
        left.len = left_len;
        left.fade_out = 0.0;
        left.curves.fout_pts.clear();
        left.curves.fout_h.clear();
        if curves::has_curves(left.env.len(), &left.curves.env_h) {
            left.env = curves::bake(&left.env, &left.curves.env_h, false, 0.02);
            left.curves.env_h = vec![Handles::default(); left.env.len()];
        }
        if curves::has_curves(left.pan_env.len(), &left.curves.pan_h) {
            left.pan_env = curves::bake(&left.pan_env, &left.curves.pan_h, false, 0.002);
            left.curves.pan_h = vec![Handles::default(); left.pan_env.len()];
        }
        self.tracks[ti].clips.insert(ci + 1, right);
        self.touch();
        Some(nid)
    }

    /// Build a playback snapshot using a lookup from file id to audio data.
    pub fn snapshot(&self, lookup: &dyn Fn(u64) -> Option<Arc<AudioData>>) -> Mix {
        self.snapshot_with(lookup, &|_| None)
    }

    /// Like [`Session::snapshot`]; `pre(track id)` returns a pre-rendered copy of a track (its
    /// clips, clip effects and track effects, before the fader) to play instead.
    pub fn snapshot_with(
        &self,
        lookup: &dyn Fn(u64) -> Option<Arc<AudioData>>,
        pre: &dyn Fn(u64) -> Option<Arc<AudioData>>,
    ) -> Mix {
        let any_solo = self.tracks.iter().any(|t| t.solo);
        let tracks = self
            .tracks
            .iter()
            .map(|t| {
                let vol = undb(t.volume_db);
                let pan = t.pan.clamp(-1.0, 1.0);
                let rendered = if t.prerender { pre(t.id) } else { None };
                let (clips, rack, fx) = match rendered {
                    Some(data) => {
                        let len = data.duration();
                        let one = MixClip {
                            data,
                            start: 0.0,
                            offset: 0.0,
                            len,
                            gain: 1.0,
                            fade_in: 0.0,
                            fade_out: 0.0,
                            xin: false,
                            xout: false,
                            env: vec![],
                            id: u64::MAX - t.id,
                            pan_env: vec![],
                            rack: vec![],
                            fx: RackOpts::default(),
                            speed: 1.0,
                            fin_curve: vec![],
                            fout_curve: vec![],
                        };
                        (
                            vec![one],
                            vec![],
                            RackOpts {
                                pre_fader: t.fx.pre_fader,
                                ..RackOpts::default()
                            },
                        )
                    }
                    None => {
                        let fades = effective_fades(t);
                        let clips = t
                            .clips
                            .iter()
                            .zip(fades)
                            .filter(|(c, _)| !c.muted)
                            .filter_map(|(c, (fin, fout, xin, xout))| {
                                Some(MixClip {
                                    data: lookup(c.file_id)?,
                                    start: c.start,
                                    offset: c.offset,
                                    len: c.len,
                                    gain: undb(c.gain_db),
                                    fade_in: fin,
                                    fade_out: fout,
                                    xin,
                                    xout,
                                    env: c.baked_env(0.01),
                                    id: c.id,
                                    pan_env: c.baked_pan(0.001),
                                    // shaping points win; otherwise the handle's up/down shape
                                    fin_curve: if c.curves.fin_pts.is_empty() {
                                        shape_table(xin, c.curves.fin_shape)
                                    } else {
                                        fade_table(&c.curves.fin_pts, &c.curves.fin_h)
                                    },
                                    fout_curve: if c.curves.fout_pts.is_empty() {
                                        shape_table(xout, c.curves.fout_shape)
                                    } else {
                                        fade_table(&c.curves.fout_pts, &c.curves.fout_h)
                                    },
                                    speed: if c.speed.is_finite() && c.speed > 0.0 {
                                        c.speed
                                    } else {
                                        1.0
                                    },
                                    rack: c.rack.clone(),
                                    fx: c.fx.clone(),
                                })
                            })
                            .collect();
                        (clips, t.rack.clone(), t.fx.clone())
                    }
                };
                let polarity = if t.io.invert { -1.0 } else { 1.0 };
                let rack_len_before_eq = rack.len();
                // the Track EQ runs after the track's effects
                let mut rack = rack;
                if t.eq_on && !t.eq.is_empty() {
                    rack.push(EffectSlot::new(crate::dsp::Effect::ParametricEq {
                        bands: t.eq.clone(),
                        master_db: t.eq_master,
                    }));
                }
                MixTrack {
                    clips,
                    gain_l: polarity * vol * (1.0 - pan).min(1.0),
                    gain_r: polarity * vol * (1.0 + pan).min(1.0),
                    audible: !t.mute && (!any_solo || t.solo),
                    rack,
                    fx,
                    mono: !t.stereo,
                    vol_env: curves::bake(&t.vol_env, &t.auto.vol_h, false, 0.01),
                    automation: t.io.automation,
                    pan_env: curves::bake(&t.auto.pan_env, &t.auto.pan_h, false, 0.001),
                    vol,
                    polarity,
                    extra: track_mix_auto(t, rack_len_before_eq),
                }
            })
            .collect();
        let m = &self.metronome;
        let metronome = if m.on && !m.mute {
            let g = undb(m.volume_db);
            let p = m.pan.clamp(-1.0, 1.0);
            Some(Click {
                bpm: m.bpm.clamp(20.0, 400.0),
                beats: m.beats.clamp(1, 16),
                gain_l: g * (1.0 - p).min(1.0),
                gain_r: g * (1.0 + p).min(1.0),
            })
        } else {
            None
        };
        Mix {
            tracks,
            master: if self.master.mute {
                0.0
            } else {
                undb(self.master_db)
            },
            master_env: curves::bake(&self.master.vol_env, &self.master.vol_h, false, 0.01),
            master_automation: self.master.automation,
            length: self.length(),
            master_rack: self.master.rack.clone(),
            master_fx: self.master.fx.clone(),
            metronome,
            mono_out: self.mix == 0,
        }
    }
}

/// The parameter automation a track plays (nothing while its automation mode is Off). `eq_slot`
/// is where the Track EQ sits in the played rack (after the track's own effects).
fn track_mix_auto(t: &Track, eq_slot: usize) -> MixAuto {
    let mut m = MixAuto::default();
    if t.io.automation == 0 {
        return m;
    }
    for p in &t.auto.params {
        if p.env.is_empty() {
            continue;
        }
        let tol = ((p.max - p.min).abs() * 0.002).max(1e-4);
        // on/off lanes (Mute, rack power) hold their keyframes: curve handles mean nothing there,
        // so a stray handle must not move the switch point
        let env = if p.binary() {
            p.env.clone()
        } else {
            curves::bake(&p.env, &p.hs, false, tol)
        };
        match p.key.as_str() {
            "mute" => m.mute = env,
            "rack.in" => m.rack_in = env,
            "rack.out" => m.rack_out = env,
            "rack.mix" => m.rack_mix = env,
            "rack.power" => m.rack_power = env,
            k => {
                if let Some(path) = k.strip_prefix("eq.") {
                    if t.eq_on {
                        m.fx.push((
                            eq_slot,
                            "Parametric Equalizer".into(),
                            path.to_string(),
                            env,
                        ));
                    }
                } else if let Some(rest) = k.strip_prefix("fx.") {
                    if let Some((slot, path)) = rest.split_once('.') {
                        if let Ok(slot) = slot.parse::<usize>() {
                            m.fx.push((slot, p.fx_name.clone(), path.to_string(), env));
                        }
                    }
                }
            }
        }
    }
    m
}

/// The fades actually used for each clip of a track: `(fade_in, fade_out, in_is_crossfade,
/// out_is_crossfade)`. Where two clips partly overlap, a side still on automatic gets a balanced
/// (equal-power) crossfade spanning the whole overlap, from the later clip's start to the earlier
/// clip's end. Dragging a fade handle switches that side to the dragged length.
pub fn effective_fades(t: &Track) -> Vec<(f64, f64, bool, bool)> {
    let mut v: Vec<(f64, f64, bool, bool)> = t
        .clips
        .iter()
        .map(|c| (c.fade_in, c.fade_out, false, false))
        .collect();
    for (i, a) in t.clips.iter().enumerate() {
        for (j, b) in t.clips.iter().enumerate() {
            if i == j || a.muted || b.muted {
                continue;
            }
            // a starts first and ends inside b: a partial overlap
            if a.start < b.start && a.end() > b.start + 1e-9 && a.end() < b.end() {
                let ov = a.end() - b.start;
                if a.auto_out {
                    v[i].1 = ov.min(a.len);
                }
                v[i].3 = true;
                if b.auto_in {
                    v[j].0 = ov.min(b.len);
                }
                v[j].2 = true;
            }
        }
    }
    v
}

/// A fade shaped by points, as a dense straight-line table (fraction, gain) for playback; empty
/// when the fade has no points (the plain curve is used).
pub fn fade_table(pts: &[(f64, f32)], hs: &[Handles]) -> Vec<(f64, f32)> {
    if pts.is_empty() {
        return vec![];
    }
    let (full, fh) = curves::fade_full(pts, hs);
    curves::bake(&full, &fh, true, 0.0005)
}

/// Fade gain at `x` through a fade: its point table when it has one, else the plain curve.
#[inline]
pub fn fade_at(table: &[(f64, f32)], x: f64, crossfade: bool) -> f32 {
    if table.is_empty() {
        fade_curve(x, crossfade)
    } else {
        env_db(table, x.clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }
}

/// A fade's gain with its shape (see `ClipCurves::fin_shape`): the plain curve bent up or down.
pub fn shaped_fade(x: f64, crossfade: bool, shape: f32) -> f32 {
    let g = fade_curve(x, crossfade);
    let s = if shape.is_finite() {
        shape.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    if s.abs() < 1e-3 {
        return g;
    }
    let k = if s > 0.0 {
        1.0 / (1.0 + 3.0 * s)
    } else {
        1.0 + 3.0 * -s
    };
    g.max(0.0).powf(k)
}

/// A shaped fade as a table for playback (empty when it is the plain curve).
pub fn shape_table(crossfade: bool, shape: f32) -> Vec<(f64, f32)> {
    if !shape.is_finite() || shape.abs() < 1e-3 {
        return vec![];
    }
    (0..=128)
        .map(|k| {
            let x = k as f64 / 128.0;
            (x, shaped_fade(x, crossfade, shape))
        })
        .collect()
}

/// Fade gain at position `x` (0..1 through the fade): balanced for crossfades, linear otherwise.
#[inline]
pub fn fade_curve(x: f64, crossfade: bool) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if crossfade {
        (x * std::f64::consts::FRAC_PI_2).sin() as f32
    } else {
        x as f32
    }
}

pub struct MixClip {
    pub data: Arc<AudioData>,
    pub start: f64,
    pub offset: f64,
    pub len: f64,
    pub gain: f32,
    pub fade_in: f64,
    pub fade_out: f64,
    /// The fades are crossfades (balanced curves).
    pub xin: bool,
    pub xout: bool,
    pub env: Vec<(f64, f32)>,
    pub id: u64,
    pub pan_env: Vec<(f64, f32)>,
    pub rack: Vec<EffectSlot>,
    pub fx: RackOpts,
    pub speed: f64,
    /// Fades shaped by points (empty: the plain curve).
    pub fin_curve: Vec<(f64, f32)>,
    pub fout_curve: Vec<(f64, f32)>,
}

pub struct MixTrack {
    pub clips: Vec<MixClip>,
    pub gain_l: f32,
    pub gain_r: f32,
    pub audible: bool,
    pub rack: Vec<EffectSlot>,
    pub fx: RackOpts,
    pub mono: bool,
    /// Fader automation in session seconds; 0 dB when empty.
    pub vol_env: Vec<(f64, f32)>,
    /// Automation mode index; Off ignores the envelope.
    pub automation: u8,
    /// Pan automation (-1 left .. 1 right); empty = the fixed pan in `gain_l` / `gain_r`.
    pub pan_env: Vec<(f64, f32)>,
    pub vol: f32,
    pub polarity: f32,
    /// Mute, rack and effect-setting automation.
    pub extra: MixAuto,
}

impl MixTrack {
    /// The track's left/right gain at session time `t` (pan automation when it is read).
    #[inline]
    fn gains(&self, t: f64) -> (f32, f32) {
        if self.pan_env.is_empty() || self.automation == 0 {
            return (self.gain_l, self.gain_r);
        }
        let p = env_db(&self.pan_env, t).clamp(-1.0, 1.0);
        (
            self.polarity * self.vol * (1.0 - p).min(1.0),
            self.polarity * self.vol * (1.0 + p).min(1.0),
        )
    }
}

pub struct Mix {
    pub tracks: Vec<MixTrack>,
    pub master: f32,
    /// Master fader automation and mode.
    pub master_env: Vec<(f64, f32)>,
    pub master_automation: u8,
    pub length: f64,
    pub master_rack: Vec<EffectSlot>,
    pub master_fx: RackOpts,
    pub metronome: Option<Click>,
    /// A mono session: both outputs carry the same (summed) signal.
    pub mono_out: bool,
}

/// Metronome settings as heard.
#[derive(Clone, Copy, Debug)]
pub struct Click {
    pub bpm: f32,
    pub beats: u32,
    pub gain_l: f32,
    pub gain_r: f32,
}

impl Click {
    /// The click's sample at time `t` (seconds): a short decaying tone on every beat, higher
    /// and louder on the first beat of the bar.
    pub fn at(&self, t: f64) -> f32 {
        if t < 0.0 {
            return 0.0;
        }
        let beat_len = 60.0 / self.bpm.max(1.0) as f64;
        let beat = (t / beat_len).floor();
        let since = t - beat * beat_len;
        if since > 0.05 {
            return 0.0;
        }
        let accent = (beat as i64).rem_euclid(self.beats.max(1) as i64) == 0;
        let f = if accent { 1600.0 } else { 1000.0 };
        let a = if accent { 0.9 } else { 0.6 };
        (a * (-since * 90.0).exp() * (2.0 * std::f64::consts::PI * f * since).sin()) as f32
    }
}

#[inline]
pub fn sample_at(d: &AudioData, ch: usize, t: f64) -> f32 {
    let c = &d.ch[ch.min(d.channels() - 1)];
    let x = t * d.rate as f64;
    if x < 0.0 {
        return 0.0;
    }
    let i = x as usize;
    if i + 1 >= c.len() {
        return if i < c.len() { c[i] } else { 0.0 };
    }
    let f = (x - i as f64) as f32;
    c[i] * (1.0 - f) + c[i + 1] * f
}

/// Per-track chain cache kept by the renderer between blocks.
#[derive(Default)]
pub struct ChainCache {
    pub chains: Vec<(Vec<EffectSlot>, Chain)>,
    /// The Master track's effects.
    pub master: (Vec<EffectSlot>, Chain),
    /// Clip effect chains by clip id.
    pub clips: std::collections::HashMap<u64, (Vec<EffectSlot>, Chain)>,
    pub rate: f32,
    cbuf: Vec<Vec<f32>>,
    dry: Vec<Vec<f32>>,
}

impl ChainCache {
    /// Clear every track, clip and master effect's held sound (see `Chain::flush`).
    pub fn flush(&mut self) {
        for (_, c) in self.chains.iter_mut() {
            c.flush();
        }
        self.master.1.flush();
        for (_, (_, c)) in self.clips.iter_mut() {
            c.flush();
        }
    }
    pub fn sync(&mut self, mix: &Mix, rate: f32) {
        if (self.rate - rate).abs() > 0.5 {
            self.chains.clear();
            self.master = (vec![], Chain::default());
            self.rate = rate;
        }
        if self.master.0 != mix.master_rack {
            self.master.1.update(&mix.master_rack, rate);
            self.master.0 = mix.master_rack.clone();
        }
        self.chains.truncate(mix.tracks.len());
        for (i, t) in mix.tracks.iter().enumerate() {
            if i >= self.chains.len() {
                self.chains
                    .push((t.rack.clone(), Chain::new(&t.rack, rate)));
            } else if self.chains[i].0 != t.rack {
                self.chains[i].1.update(&t.rack, rate);
                self.chains[i].0 = t.rack.clone();
            }
        }
        // clip chains: create/update the ones in use, drop the rest
        let mut live = std::collections::HashSet::new();
        for t in &mix.tracks {
            for c in t.clips.iter().filter(|c| c.rack.iter().any(|s| s.enabled)) {
                live.insert(c.id);
                match self.clips.get_mut(&c.id) {
                    Some(e) if e.0 == c.rack => {}
                    Some(e) => {
                        e.1.update(&c.rack, rate);
                        e.0 = c.rack.clone();
                    }
                    None => {
                        self.clips
                            .insert(c.id, (c.rack.clone(), Chain::new(&c.rack, rate)));
                    }
                }
            }
        }
        self.clips.retain(|id, _| live.contains(id));
    }
}

/// Render `n` stereo frames starting at time `t0` at `rate` into `out` (2 channels).
pub fn render_mix(
    mix: &Mix,
    cache: &mut ChainCache,
    rate: f32,
    t0: f64,
    n: usize,
    out: &mut Vec<Vec<f32>>,
    tmp: &mut Vec<Vec<f32>>,
    track_peaks: &mut Vec<[f32; 2]>,
) {
    out.resize(2, vec![]);
    for c in out.iter_mut() {
        c.clear();
        c.resize(n, 0.0);
    }
    tmp.resize(2, vec![]);
    track_peaks.resize(mix.tracks.len(), [0.0; 2]);
    let dt = 1.0 / rate as f64;
    let t1 = t0 + n as f64 * dt;
    for (ti, t) in mix.tracks.iter().enumerate() {
        for c in tmp.iter_mut() {
            c.clear();
            c.resize(n, 0.0);
        }
        let mut any = false;
        for c in &t.clips {
            if c.start >= t1 || c.start + c.len <= t0 {
                continue;
            }
            any = true;
            let chs = c.data.channels();
            // render the clip into its own buffer when it has effects or a pan envelope
            let own =
                cache.clips.contains_key(&c.id) || !c.pan_env.is_empty() || !c.fx.is_neutral(true);
            if own {
                cache.cbuf.resize(2, vec![]);
                for b in cache.cbuf.iter_mut() {
                    b.clear();
                    b.resize(n, 0.0);
                }
            }
            for i in 0..n {
                let tt = t0 + i as f64 * dt;
                let rel = tt - c.start;
                if rel < 0.0 || rel >= c.len {
                    continue;
                }
                let mut g = c.gain;
                if c.fade_in > 0.0 && rel < c.fade_in {
                    g *= fade_at(&c.fin_curve, rel / c.fade_in, c.xin);
                }
                let to_end = c.len - rel;
                if c.fade_out > 0.0 && to_end < c.fade_out {
                    g *= fade_at(&c.fout_curve, to_end / c.fade_out, c.xout);
                }
                if !c.env.is_empty() {
                    g *= undb(env_db(&c.env, rel));
                }
                let st = c.offset + rel * c.speed;
                let (mut l, mut r) = if chs == 1 || t.mono {
                    let v = if chs == 1 {
                        sample_at(&c.data, 0, st)
                    } else {
                        (0..chs).map(|ch| sample_at(&c.data, ch, st)).sum::<f32>() / chs as f32
                    } * g;
                    (v, v)
                } else {
                    (sample_at(&c.data, 0, st) * g, sample_at(&c.data, 1, st) * g)
                };
                if !c.pan_env.is_empty() {
                    let p = env_db(&c.pan_env, rel).clamp(-1.0, 1.0);
                    l *= (1.0 - p).min(1.0);
                    r *= (1.0 + p).min(1.0);
                }
                if own {
                    cache.cbuf[0][i] = l;
                    cache.cbuf[1][i] = r;
                } else {
                    tmp[0][i] += l;
                    tmp[1][i] += r;
                }
            }
            if own {
                let mut buf = std::mem::take(&mut cache.cbuf);
                match cache.clips.get_mut(&c.id) {
                    Some((_, chain)) => {
                        crate::dsp::run_rack(chain, &c.fx, &mut buf, &mut cache.dry)
                    }
                    None => {
                        crate::dsp::run_rack(&mut Chain::default(), &c.fx, &mut buf, &mut cache.dry)
                    }
                }
                for k in 0..2 {
                    for i in 0..n {
                        tmp[k][i] += buf[k][i];
                    }
                }
                cache.cbuf = buf;
            }
        }
        // post-fader racks see the track's volume, pan and automation first
        let pre = t.fx.pre_fader;
        if !pre {
            for i in 0..n {
                let tt = t0 + i as f64 * dt;
                let ag = automation_gain(&t.vol_env, tt, t.automation);
                let (gl, gr) = t.gains(tt);
                tmp[0][i] *= gl * ag;
                tmp[1][i] *= gr * ag;
            }
        }
        // automated effect settings: the rack's chain follows them, block by block
        if !t.extra.fx.is_empty() {
            let mut rack = t.rack.clone();
            for (slot, name, path, env) in &t.extra.fx {
                if let Some(s) = rack.get_mut(*slot) {
                    if s.effect.name() == name.as_str() {
                        s.effect.set_auto_param(path, env_db(env, t0));
                    }
                }
            }
            if let Some(entry) = cache.chains.get_mut(ti) {
                if entry.0 != rack {
                    entry.1.update(&rack, rate);
                    entry.0 = rack;
                }
            }
        }
        let fx_now = t.extra.rack_opts(&t.fx, t0);
        let fxo = fx_now.as_ref().unwrap_or(&t.fx);
        // run the track rack even without clips so tails ring out
        if let Some((_, chain)) = cache.chains.get_mut(ti) {
            if !fxo.is_neutral(chain.is_empty()) {
                crate::dsp::run_rack(chain, fxo, tmp, &mut cache.dry);
                any = true;
            }
        }
        if !any || !t.audible {
            track_peaks[ti] = [0.0; 2];
            continue;
        }
        let mut pk = [0.0f32; 2];
        for i in 0..n {
            let tt = t0 + i as f64 * dt;
            let (gl, gr) = if pre { t.gains(tt) } else { (1.0, 1.0) };
            let ag = if pre {
                automation_gain(&t.vol_env, tt, t.automation)
            } else {
                1.0
            };
            // automated Mute
            let ag = if !t.extra.mute.is_empty() && env_db(&t.extra.mute, tt) >= 0.5 {
                0.0
            } else {
                ag
            };
            let l = tmp[0][i] * gl * ag;
            let r = tmp[1][i] * gr * ag;

            pk[0] = pk[0].max(l.abs());
            pk[1] = pk[1].max(r.abs());
            out[0][i] += l;
            out[1][i] += r;
        }
        track_peaks[ti] = pk;
    }
    if let Some(click) = &mix.metronome {
        for i in 0..n {
            let v = click.at(t0 + i as f64 * dt);
            out[0][i] += v * click.gain_l;
            out[1][i] += v * click.gain_r;
        }
    }
    // the Master track's effects, then its fader
    if !mix.master_fx.is_neutral(cache.master.1.is_empty()) {
        let mut chain = std::mem::take(&mut cache.master.1);
        crate::dsp::run_rack(&mut chain, &mix.master_fx, out, &mut cache.dry);
        cache.master.1 = chain;
    }
    if mix.mono_out && out.len() >= 2 {
        for i in 0..n {
            let m = 0.5 * (out[0][i] + out[1][i]);
            out[0][i] = m;
            out[1][i] = m;
        }
    }
    for c in out.iter_mut() {
        for (i, s) in c.iter_mut().enumerate() {
            // `i` is the frame, not the channel: master automation must follow every frame
            // of the block, or a quiet passage can be rendered at a loud gain (and vice versa).
            let ag = automation_gain(&mix.master_env, t0 + i as f64 * dt, mix.master_automation);
            *s *= mix.master * ag;
        }
    }
}

/// Offline mixdown of [a,b) seconds.
/// Match Clip Loudness: give every clip on the selected track the gain that brings its own
/// audio (the part of its source it plays) to the clips' average RMS level. Silent clips and
/// clips whose source is missing keep their gain; changes stay within ±24 dB. Returns how many
/// clips were matched (0 when there was nothing to match).
pub fn match_clip_loudness(
    s: &mut Session,
    source: impl Fn(u64) -> Option<std::sync::Arc<AudioData>>,
) -> usize {
    let i = s.selected_track.min(s.tracks.len().saturating_sub(1));
    let Some(t) = s.tracks.get_mut(i) else {
        return 0;
    };
    let levels: Vec<Option<f32>> = t
        .clips
        .iter()
        .map(|c| {
            let d = source(c.file_id)?;
            let rate = d.rate.max(1) as f64;
            let a = (c.offset.max(0.0) * rate) as usize;
            let b = ((c.offset.max(0.0) + c.len.max(0.0) * c.speed.max(0.0)) * rate) as usize;
            let (a, b) = d.span(a, b);
            let n = ((b - a) * d.channels()) as f64;
            if n < 1.0 {
                return None;
            }
            let sum: f64 =
                d.ch.iter()
                    .flat_map(|ch| ch[a..b].iter())
                    .filter(|v| v.is_finite())
                    .map(|v| (*v as f64).powi(2))
                    .sum();
            let rms = (sum / n).sqrt();
            // what is heard: the source level plus the clip's own gain
            (rms > 1e-6).then(|| (20.0 * rms.log10()) as f32 + c.gain_db)
        })
        .collect();
    let heard: Vec<f32> = levels.iter().flatten().copied().collect();
    if heard.len() < 2 {
        return 0;
    }
    let target = heard.iter().sum::<f32>() / heard.len() as f32;
    let mut n = 0;
    for (c, l) in t.clips.iter_mut().zip(levels) {
        if let Some(l) = l {
            c.gain_db = (c.gain_db + (target - l)).clamp(-24.0, 24.0);
            n += 1;
        }
    }
    n
}

pub fn mixdown(mix: &Mix, rate: u32, a: f64, b: f64) -> AudioData {
    let mut cache = ChainCache::default();
    cache.sync(mix, rate as f32);
    let total = ((b - a).max(0.0) * rate as f64) as usize;
    let mut res = AudioData::new(rate, 2, 0);
    let mut out = vec![];
    let mut tmp = vec![];
    let mut peaks = vec![];
    let block = 4096;
    let mut done = 0usize;
    while done < total {
        let n = block.min(total - done);
        let t0 = a + done as f64 / rate as f64;
        render_mix(
            mix,
            &mut cache,
            rate as f32,
            t0,
            n,
            &mut out,
            &mut tmp,
            &mut peaks,
        );
        res.ch[0].extend_from_slice(&out[0]);
        res.ch[1].extend_from_slice(&out[1]);
        done += n;
    }
    res
}

#[cfg(test)]
mod binary_lane_tests {
    use super::*;

    /// Mute automation holds its keyframes: a curve handle left on a Mute keyframe must not
    /// bend the lane (the switch point stays where the keyframes put it).
    #[test]
    fn mute_lane_keeps_its_keyframes_even_with_handles() {
        let mut t = Session::new("t", 48000, 1).tracks.remove(0);
        t.io.automation = 1;
        let env = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)];
        let hs = vec![crate::curves::Handles::smooth(0.4, 0.6); 3];
        t.auto.params.push(AutoParam {
            key: "mute".into(),
            label: "Mute".into(),
            fx_name: String::new(),
            env: env.clone(),
            hs: hs.clone(),
            shown: true,
            min: 0.0,
            max: 1.0,
        });
        let m = track_mix_auto(&t, 0);
        assert_eq!(m.mute, env);
        // a level lane with the same handles is curved (baked into more points)
        t.auto.params[0].key = "rack.in".into();
        let m = track_mix_auto(&t, 0);
        assert!(m.rack_in.len() > env.len());
    }
}
