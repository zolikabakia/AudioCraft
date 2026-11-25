//! Real-time playback and recording through the system audio devices.

use crate::audio::AudioData;
use crate::dsp::{Chain, EffectSlot};
use crate::session::{ChainCache, Mix, render_mix, sample_at};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

pub enum Source {
    None,
    File(Arc<AudioData>),
    Mix(Arc<Mix>),
}

/// How far past the dragged playhead a scrub grain plays, in seconds.
const SCRUB_GRAIN: f64 = 0.06;

pub struct Shared {
    pub source: Source,
    pub playing: bool,
    pub pos: f64,
    pub end: f64,
    pub loop_start: f64,
    pub looping: bool,
    pub rack_slots: Vec<EffectSlot>,
    rack: Chain,
    pub rack_in: f32,
    pub rack_out: f32,
    pub rack_mix: f32,
    pub rack_power: bool,
    pub rack_in_meter: [f32; 2],
    pub rack_out_meter: [f32; 2],
    pub preview: Option<Vec<EffectSlot>>,
    preview_chain: Option<(Vec<EffectSlot>, Chain)>,
    pub out_rate: u32,
    pub out_channels: usize,
    /// The device output channels the Master's Left and Right play on (Preferences > Audio
    /// Channel Mapping; 0-based).
    pub out_map: [usize; 2],
    /// Tracks sent straight to hardware outputs, one buffer per device channel (this block).
    pub direct_out: Vec<Vec<f32>>,
    /// Stop once past this time and quiet for half a second (Preferences > Playback >
    /// Stop playback at the end: the session's end, after effect tails die away).
    pub stop_after: Option<f64>,
    quiet: usize,
    /// A file-browser preview. It has its own source and position: prelisting a file must
    /// never move the editor's playhead or replace its mix. Written by the UI thread, read
    /// and advanced by the audio callback under `shared`.
    pub files_preview: Option<FilePreview>,
    pub meter: [f32; 2],
    pub track_meters: Vec<[f32; 2]>,
    pub recording: bool,
    pub rec: Vec<Vec<f32>>,
    pub rec_rate: u32,
    pub in_meter: [f32; 2],
    /// Peak of every input channel (decaying), measured whenever the input is open: armed
    /// tracks show these on their meters before and during recording.
    pub in_peaks: Vec<f32>,
    /// Channels of the open input device (0 when it is closed).
    pub in_channels: usize,
    /// Tracks whose input is heard through the output (the track's I button).
    pub monitors: Vec<Monitor>,
    /// Monitored input, every input channel of each frame in a row (`mon_stride` a frame).
    mon_ring: std::collections::VecDeque<f32>,
    mon_stride: usize,
    pub master_gain: f32,
    pub error: Option<String>,
    /// While stopped: where the playhead is being dragged to. The engine plays short stretches
    /// of audio from its read position up to just past this point (scrubbing).
    pub scrub: Option<f64>,
    scrub_fresh: bool,
    /// The last output frames (for the phase meter and goniometer).
    pub scope: std::collections::VecDeque<[f32; 2]>,
    /// Audio processing load, percent of the time each output buffer lasts.
    pub dsp_load: f32,
    cache: ChainCache,
    pub(crate) scratch: Vec<Vec<f32>>,
    tmp: Vec<Vec<f32>>,
}

impl Shared {
    pub(crate) fn new() -> Self {
        Shared {
            source: Source::None,
            playing: false,
            pos: 0.0,
            end: 0.0,
            loop_start: 0.0,
            looping: false,
            rack_slots: vec![],
            rack: Chain::default(),
            rack_in: 1.0,
            rack_out: 1.0,
            rack_mix: 1.0,
            rack_power: true,
            rack_in_meter: [0.0; 2],
            rack_out_meter: [0.0; 2],
            preview: None,
            preview_chain: None,
            out_rate: 48000,
            out_channels: 2,
            out_map: [0, 1],
            direct_out: vec![],
            stop_after: None,
            quiet: 0,
            files_preview: None,
            meter: [0.0; 2],
            track_meters: vec![],
            recording: false,
            rec: vec![],
            rec_rate: 48000,
            in_meter: [0.0; 2],
            in_peaks: vec![],
            in_channels: 0,
            monitors: vec![],
            mon_ring: std::collections::VecDeque::with_capacity(16384),
            mon_stride: 2,
            master_gain: 1.0,
            error: None,
            scrub: None,
            scrub_fresh: true,
            scope: std::collections::VecDeque::with_capacity(2048),
            dsp_load: 0.0,
            cache: ChainCache::live(),
            scratch: vec![],
            tmp: vec![],
        }
    }

    /// Set the file effects rack. UI thread only: hosted plug-ins are created here.
    /// The playing chain of a rack: the file rack (`None`), a track's (`Some(Ok(index))`, the
    /// Master at `MASTER_INDEX`) or a clip's (`Some(Err(clip id))`). UI thread.
    pub fn live_chain(&mut self, which: Option<Result<usize, u64>>) -> Option<&mut Chain> {
        match which {
            None => Some(&mut self.rack),
            Some(Ok(i)) if i == crate::session::MASTER_INDEX => Some(&mut self.cache.master.1),
            Some(Ok(i)) => self.cache.chains.get_mut(i).map(|c| &mut c.1),
            Some(Err(id)) => self.cache.clips.get_mut(&id).map(|c| &mut c.1),
        }
    }

    pub fn set_rack(
        &mut self,
        slots: &[EffectSlot],
        in_db: f32,
        out_db: f32,
        mix: f32,
        power: bool,
    ) {
        let rate = self.out_rate as f32;
        if self.rack_slots != slots {
            self.rack.update(slots, rate);
            self.rack_slots = slots.to_vec();
        }
        self.rack_in = crate::audio::undb(in_db);
        self.rack_out = crate::audio::undb(out_db);
        self.rack_mix = mix;
        self.rack_power = power;
    }

    /// Drop every effect chain (and so every plug-in instance). UI thread, at shutdown.
    pub fn release_plugins(&mut self) {
        self.playing = false;
        self.rack = Chain::default();
        self.rack_slots.clear();
        self.cache = ChainCache::live();
        self.preview = None;
        self.preview_chain = None;
        self.source = Source::None;
        self.files_preview = None;
    }

    /// Forget the built chains so the next sync rebuilds them (after the output rate changed).
    /// UI thread only.
    pub fn reset_chains(&mut self) {
        self.rack = Chain::default();
        self.rack_slots.clear();
        self.cache = ChainCache::live();
        self.preview_chain = None;
    }

    /// Silence everything the effects still hold (tails of reverbs, delays...). Called when
    /// playback stops and when the playhead is moved while stopped, so the sound that was
    /// playing can't come back out at the new spot. UI thread only.
    pub fn flush_effects(&mut self) {
        self.rack.flush();
        self.cache.flush();
        self.preview_chain = None;
    }

    /// Forget every held level reading. Call when the source or playhead changes so a quiet
    /// file can never inherit an old peak, hold line or clip light from a previous take.
    pub fn reset_meters(&mut self) {
        self.meter = [0.0; 2];
        self.track_meters.clear();
        self.rack_in_meter = [0.0; 2];
        self.rack_out_meter = [0.0; 2];
        self.in_meter = [0.0; 2];
        self.scope.clear();
    }

    /// Bring the per-track chains in line with the multitrack snapshot. UI thread only.
    pub fn sync_mix_chains(&mut self) {
        if let Source::Mix(m) = &self.source {
            let m = m.clone();
            let rate = self.out_rate as f32;
            self.cache.sync(&m, rate);
        }
    }

    /// Render the file-browser preview independently of the main transport. A preview owns
    /// its own data and position; the editor's `source`, `pos` and playhead are never touched.
    /// Returns true while audio was produced (non-looping previews stop at their end).
    fn render_files_preview(&mut self, n: usize) -> bool {
        let Some(p) = self.files_preview.as_mut() else {
            return false;
        };
        let rate = self.out_rate.max(1);
        let dt = 1.0 / rate as f64;
        let frames = if p.looping {
            n
        } else {
            (((p.end - p.pos) / dt).max(0.0) as usize).min(n)
        };
        let chans = p.data.channels().max(1);
        p.tmp.resize(chans, vec![]);
        for c in p.tmp.iter_mut() {
            c.clear();
            c.resize(frames, 0.0);
        }
        let mut t = p.pos;
        for i in 0..frames {
            if p.looping && t >= p.end {
                t = 0.0;
            }
            for c in 0..chans {
                p.tmp[c][i] = crate::session::sample_at(&p.data, c, t);
            }
            t += dt;
        }
        p.pos = t;
        for i in 0..n {
            for ch in 0..2 {
                let v = p.tmp[0].get(i).copied().unwrap_or(0.0);
                let v = if ch == 1 {
                    p.tmp[1.min(chans - 1)].get(i).copied().unwrap_or(v)
                } else {
                    v
                };
                self.scratch[ch][i] = v;
            }
        }
        if !p.looping && p.pos >= p.end - dt * 0.5 {
            self.files_preview = None;
            frames < n
        } else {
            true
        }
    }

    /// Render `n` frames of output into self.scratch (2 channels).
    pub(crate) fn render(&mut self, n: usize) {
        let rate = self.out_rate as f32;
        self.scratch.resize(2, vec![]);
        for c in self.scratch.iter_mut() {
            c.clear();
            c.resize(n, 0.0);
        }
        for c in self.direct_out.iter_mut() {
            c.clear();
            c.resize(n, 0.0);
        }
        // scrubbing: while stopped and the playhead is dragged, play from the read position
        // to a short grain past the pointer; jump when the pointer is far away or went back
        // A browser preview is independent: it has its own position and source, so the
        // editor's playhead, source and rack are left exactly as they were.
        if self.files_preview.is_some() && !self.playing {
            let mut sounded = self.render_files_preview(n);
            if self.scrub.is_some() {
                // if a scrub is still queued, it takes over once the preview ends
                sounded = false;
                self.files_preview = None;
            }
            if sounded {
                for c in 0..2 {
                    let pk = self.scratch[c].iter().fold(0.0f32, |m, s| m.max(s.abs()));
                    self.meter[c] = pk;
                }
                for c in 0..2 {
                    for s in &mut self.scratch[c] {
                        *s *= self.master_gain;
                    }
                }
                for i in 0..n.min(self.scratch[0].len()) {
                    self.scope
                        .push_back([self.scratch[0][i], self.scratch[1][i]]);
                }
                while self.scope.len() > 2048 {
                    self.scope.pop_front();
                }
                return;
            }
        }
        let mut scrub_end = None;
        if !self.playing {
            if let Some(target) = self.scrub {
                let len = match &self.source {
                    Source::File(d) => d.duration(),
                    Source::Mix(m) => m.length,
                    Source::None => 0.0,
                };
                let target = target.clamp(0.0, len.max(0.0));
                if target < self.pos - SCRUB_GRAIN - 0.005 || target > self.pos + 0.3 {
                    self.pos = if target > self.pos {
                        (target - 0.05).max(0.0)
                    } else {
                        target
                    };
                    self.scrub_fresh = true;
                }
                let lim = (target + SCRUB_GRAIN).min(len);
                if self.pos < lim {
                    scrub_end = Some(lim);
                }
            }
            if scrub_end.is_none() {
                self.meter = [self.meter[0] * 0.85, self.meter[1] * 0.85];
                for m in self.track_meters.iter_mut() {
                    m[0] *= 0.85;
                    m[1] *= 0.85;
                }
                self.mix_monitors(n);
                return;
            }
        }
        let scrubbing = scrub_end.is_some();
        let looping = self.looping && !scrubbing;
        let end = scrub_end.unwrap_or(self.end);
        let dt = 1.0 / rate as f64;
        let mut frames = n;
        let remain = ((end - self.pos) / dt).max(0.0) as usize;
        if !looping && remain < frames {
            frames = remain;
        }
        match &self.source {
            Source::None => {}
            Source::File(d) => {
                let d = d.clone();
                let chs = d.channels().max(1);
                self.tmp.resize(chs, vec![]);
                for c in self.tmp.iter_mut() {
                    c.clear();
                    c.resize(n, 0.0);
                }
                let mut t = self.pos;
                for i in 0..frames {
                    if looping && t >= self.end {
                        t = self.loop_start;
                    }
                    for c in 0..chs {
                        self.tmp[c][i] = sample_at(&d, c, t);
                    }
                    t += dt;
                }
                self.pos = t;
                // effects rack (its chain is built on the UI thread; here it only processes)
                let pk = |b: &[Vec<f32>]| -> [f32; 2] {
                    let a = b
                        .first()
                        .map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs())))
                        .unwrap_or(0.0);
                    let z = b
                        .get(1)
                        .map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs())))
                        .unwrap_or(a);
                    [a, z]
                };
                let inm = pk(&self.tmp);
                if self.rack_power
                    && (!self.rack.is_empty()
                        || (self.rack_in - 1.0).abs() > 1e-6
                        || (self.rack_out - 1.0).abs() > 1e-6)
                {
                    let mix = self.rack_mix.clamp(0.0, 1.0);
                    let dry: Option<Vec<Vec<f32>>> = if mix < 0.999 {
                        Some(self.tmp.clone())
                    } else {
                        None
                    };
                    for c in self.tmp.iter_mut() {
                        for s in c.iter_mut() {
                            *s *= self.rack_in;
                        }
                    }
                    self.rack.process(&mut self.tmp);
                    for (ci, c) in self.tmp.iter_mut().enumerate() {
                        for (i, s) in c.iter_mut().enumerate() {
                            *s *= self.rack_out;
                            if let Some(d) = &dry {
                                *s = d[ci][i] * (1.0 - mix) + *s * mix;
                            }
                        }
                    }
                }
                let outm = pk(&self.tmp);
                for k in 0..2 {
                    self.rack_in_meter[k] = inm[k].max(self.rack_in_meter[k] * 0.85);
                    self.rack_out_meter[k] = outm[k].max(self.rack_out_meter[k] * 0.85);
                }
                match &self.preview {
                    Some(p) => {
                        if self
                            .preview_chain
                            .as_ref()
                            .map(|r| &r.0 != p)
                            .unwrap_or(true)
                        {
                            self.preview_chain = Some((p.clone(), Chain::new(p, rate)));
                        }
                        if let Some((_, ch)) = self.preview_chain.as_mut() {
                            ch.process(&mut self.tmp);
                        }
                    }
                    None => self.preview_chain = None,
                }
                for i in 0..n {
                    self.scratch[0][i] = self.tmp[0][i];
                    self.scratch[1][i] = self.tmp[1.min(chs - 1)][i];
                }
            }
            Source::Mix(m) => {
                let m = m.clone();
                // Render in two parts when looping wraps inside the block.
                let mut done = 0;
                let mut out = std::mem::take(&mut self.scratch);
                let mut part = vec![];
                while done < frames {
                    let mut len = frames - done;
                    if looping {
                        let to_end = ((self.end - self.pos) / dt).max(0.0) as usize;
                        if to_end == 0 {
                            self.pos = self.loop_start;
                            continue;
                        }
                        len = len.min(to_end);
                    }
                    render_mix(
                        &m,
                        &mut self.cache,
                        rate,
                        self.pos,
                        len,
                        &mut part,
                        &mut self.tmp,
                        &mut self.track_meters,
                    );
                    for c in 0..2 {
                        out[c][done..done + len].copy_from_slice(&part[c]);
                    }
                    // tracks sent straight to hardware outputs
                    let nd = self.cache.direct.len();
                    if self.direct_out.len() < nd {
                        self.direct_out.resize(nd, vec![0.0; n]);
                    }
                    for (c, d) in self.cache.direct.iter().enumerate() {
                        if let Some(o) = self.direct_out.get_mut(c) {
                            if o.len() < n {
                                o.resize(n, 0.0);
                            }
                            o[done..done + len].copy_from_slice(&d[..len.min(d.len())]);
                        }
                    }
                    self.pos += len as f64 * dt;
                    done += len;
                }
                self.scratch = out;
            }
        }
        if scrubbing {
            // short fades at both ends of a grain keep scrubbing free of clicks
            let fade = 96.min(frames);
            for c in self.scratch.iter_mut() {
                if self.scrub_fresh {
                    for (i, s) in c.iter_mut().take(fade).enumerate() {
                        *s *= i as f32 / fade as f32;
                    }
                }
                if frames < n {
                    for i in 0..fade {
                        c[frames - 1 - i] *= i as f32 / fade as f32;
                    }
                }
            }
            self.scrub_fresh = frames < n;
        } else {
            self.scrub_fresh = true;
            if !self.looping && self.pos >= self.end - dt * 0.5 && frames < n {
                self.playing = false;
            }
        }
        for c in self.scratch.iter_mut() {
            for s in c.iter_mut() {
                *s *= self.master_gain;
            }
        }
        self.mix_monitors(n);
        for c in 0..2 {
            let pk = self.scratch[c].iter().fold(0.0f32, |m, s| m.max(s.abs()));
            self.meter[c] = pk.max(self.meter[c] * 0.85);
        }
        // past the session's end: stop when the tails have died away (half a second quiet)
        if let Some(end) = self.stop_after.filter(|_| self.playing && !self.looping) {
            if self.pos > end {
                let pk = self
                    .scratch
                    .iter()
                    .chain(self.direct_out.iter())
                    .flat_map(|c| c.iter())
                    .fold(0.0f32, |m, s| m.max(s.abs()));
                if pk < 1e-5 {
                    self.quiet += n;
                } else {
                    self.quiet = 0;
                }
                if self.quiet as f64 >= 0.5 * self.out_rate.max(1) as f64 {
                    self.playing = false;
                    self.quiet = 0;
                }
            } else {
                self.quiet = 0;
            }
        }
        for i in 0..n.min(self.scratch[0].len()) {
            self.scope
                .push_back([self.scratch[0][i], self.scratch[1][i]]);
        }
        while self.scope.len() > 2048 {
            self.scope.pop_front();
        }
    }
}

/// An independent player for the Files panel. It plays a file from the browser without
/// touching the editor's playhead, source, rack or level meters.
pub struct FilePreview {
    pub data: Arc<AudioData>,
    pub pos: f64,
    pub end: f64,
    pub looping: bool,
    tmp: Vec<Vec<f32>>,
}

impl FilePreview {
    pub fn new(data: Arc<AudioData>, looping: bool) -> Self {
        Self {
            end: data.duration(),
            data,
            pos: 0.0,
            looping,
            tmp: vec![],
        }
    }
}

/// One monitored track: which input channels it hears, at what level and pan.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    /// Input channel for the left side and for the right side (the same one for mono).
    pub chans: [usize; 2],
    pub gain: f32,
    pub pan: f32,
}

impl Shared {
    /// Add the monitored input into the output buffer (input monitoring). The ring is kept
    /// short so what you hear is no more than a few buffers behind what you play.
    fn mix_monitors(&mut self, n: usize) {
        if self.monitors.is_empty() {
            self.mon_ring.clear();
            return;
        }
        let stride = self.mon_stride.clamp(1, 64);
        while self.mon_ring.len() > (n * 3 + 256) * stride {
            for _ in 0..stride {
                self.mon_ring.pop_front();
            }
        }
        for i in 0..n {
            if self.mon_ring.len() < stride {
                break;
            }
            let mut f = [0.0f32; 64];
            for v in f.iter_mut().take(stride) {
                *v = self.mon_ring.pop_front().unwrap_or(0.0);
            }
            let (mut l, mut r) = (0.0f32, 0.0f32);
            for m in &self.monitors {
                let pick = |c: usize| f[c.min(stride - 1)];
                let (a, b) = (pick(m.chans[0]), pick(m.chans[1]));
                let pan = m.pan.clamp(-1.0, 1.0);
                l += a * m.gain * (1.0 - pan.max(0.0));
                r += b * m.gain * (1.0 + pan.min(0.0));
            }
            if let Some(s) = self.scratch.get_mut(0).and_then(|c| c.get_mut(i)) {
                *s += l;
            }
            if let Some(s) = self.scratch.get_mut(1).and_then(|c| c.get_mut(i)) {
                *s += r;
            }
        }
    }
}

pub struct Engine {
    pub shared: Arc<Mutex<Shared>>,
    out_stream: Option<cpal::Stream>,
    in_stream: Option<cpal::Stream>,
    pub out_device_name: String,
    pub in_device_name: String,
    pub out_choice: Option<String>,
    pub in_choice: Option<String>,
    pub ctx: Option<egui::Context>,
    in_failed: bool,
    /// Channels of the input device, learned once (0: not yet).
    pub in_known: usize,
    /// The background question to the input device about its channel count (see
    /// `probe_input_channels`): its answer arrives here.
    in_probe: Option<std::sync::mpsc::Receiver<usize>>,
}

fn dev_name(d: &cpal::Device) -> String {
    d.description()
        .map(|x| x.name().to_string())
        .unwrap_or_else(|_| "Unknown device".into())
}

pub fn list_devices(input: bool) -> Vec<String> {
    let host = cpal::default_host();
    let devs = if input {
        host.input_devices().map(|d| d.collect::<Vec<_>>())
    } else {
        host.output_devices().map(|d| d.collect::<Vec<_>>())
    };
    devs.map(|v| v.iter().map(dev_name).collect())
        .unwrap_or_default()
}

impl Engine {
    pub fn new() -> Self {
        Engine {
            shared: Arc::new(Mutex::new(Shared::new())),
            out_stream: None,
            in_stream: None,
            out_device_name: "(none)".into(),
            in_device_name: "(none)".into(),
            out_choice: None,
            in_choice: None,
            ctx: None,
            in_failed: false,
            in_known: 0,
            in_probe: None,
        }
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        match self.shared.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    /// Clear the engine's peak holds and clip lights (UI thread).
    pub fn reset_meters(&self) {
        self.lock().reset_meters();
    }

    pub fn ensure_output(&mut self) {
        // tests drive the whole app but never open the sound card
        if self.out_stream.is_some() || cfg!(test) {
            return;
        }
        if let Err(e) = self.start_output() {
            self.lock().error = Some(format!("Audio output unavailable: {e}"));
        }
    }

    pub fn restart_output(&mut self) {
        self.out_stream = None;
        self.ensure_output();
    }

    fn start_output(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = match &self.out_choice {
            Some(name) => host
                .output_devices()
                .map_err(|e| e.to_string())?
                .find(|d| &dev_name(d) == name)
                .or_else(|| host.default_output_device()),
            None => host.default_output_device(),
        }
        .ok_or("no output device")?;
        self.out_device_name = dev_name(&device);
        let sup = device.default_output_config().map_err(|e| e.to_string())?;
        let fmt = sup.sample_format();
        let config: cpal::StreamConfig = sup.config();
        {
            let mut s = self.lock();
            s.out_rate = config.sample_rate;
            s.out_channels = config.channels as usize;
        }
        let stream = match fmt {
            cpal::SampleFormat::F32 => self.build_out::<f32>(&device, config)?,
            cpal::SampleFormat::I16 => self.build_out::<i16>(&device, config)?,
            cpal::SampleFormat::U16 => self.build_out::<u16>(&device, config)?,
            cpal::SampleFormat::I32 => self.build_out::<i32>(&device, config)?,
            cpal::SampleFormat::F64 => self.build_out::<f64>(&device, config)?,
            other => return Err(format!("unsupported sample format {other}")),
        };
        stream.play().map_err(|e| e.to_string())?;
        self.out_stream = Some(stream);
        Ok(())
    }

    fn build_out<T>(
        &self,
        device: &cpal::Device,
        config: cpal::StreamConfig,
    ) -> Result<cpal::Stream, String>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        let shared = self.shared.clone();
        let chans = config.channels as usize;
        let ctx = self.ctx.clone();
        device
            .build_output_stream(
                config,
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / chans.max(1);
                    let mut s = match shared.lock() {
                        Ok(g) => g,
                        Err(p) => p.into_inner(),
                    };
                    // timed from here: waiting for the lock (the interface flushing effect
                    // tails after Stop, for example) is not audio work, and counting it showed
                    // loads of thousands of percent
                    let started = std::time::Instant::now();
                    let was = s.playing || s.files_preview.is_some();
                    s.render(frames);
                    // DSP load, as audio programs show it: the share of the buffer's own
                    // duration spent working it out (smoothed; peaks held briefly)
                    let budget = frames as f64 / s.out_rate.max(1) as f64;
                    if budget > 0.0 {
                        let load = (started.elapsed().as_secs_f64() / budget * 100.0) as f32;
                        s.dsp_load = if load > s.dsp_load {
                            load
                        } else {
                            s.dsp_load * 0.97 + load * 0.03
                        };
                    }
                    // the Master plays on its mapped channels (Preferences > Audio Channel
                    // Mapping), tracks sent to a hardware pair on theirs
                    let pick = |m: usize, fallback: usize| if m < chans { m } else { fallback };
                    let (ml, mr) = (pick(s.out_map[0], 0), pick(s.out_map[1], 1.min(chans - 1)));
                    for (i, fr) in data.chunks_mut(chans).enumerate() {
                        let l = s.scratch[0].get(i).copied().unwrap_or(0.0);
                        let r = s.scratch[1].get(i).copied().unwrap_or(0.0);
                        for (c, smp) in fr.iter_mut().enumerate() {
                            let mut v = s
                                .direct_out
                                .get(c)
                                .and_then(|d| d.get(i))
                                .copied()
                                .unwrap_or(0.0);
                            if chans == 1 {
                                v += 0.5 * (l + r);
                            } else {
                                if c == ml {
                                    v += l;
                                }
                                if c == mr {
                                    v += r;
                                }
                            }
                            *smp = T::from_sample(v);
                        }
                    }
                    if was {
                        if let Some(c) = &ctx {
                            c.request_repaint();
                        }
                    }
                },
                |e| eprintln!("audio output error: {e}"),
                None,
            )
            .map_err(|e| e.to_string())
    }

    /// Keep the input device open (for armed-track meters and input monitoring) or close it.
    /// It never closes while recording. Errors are kept in `error` once, not every frame.
    pub fn set_listening(&mut self, on: bool) {
        if on && self.in_stream.is_none() {
            if cfg!(test) {
                return;
            }
            if let Err(e) = self.open_input() {
                self.lock().error = Some(format!("Audio input unavailable: {e}"));
                // don't retry every frame
                self.in_failed = true;
            }
        } else if !on && self.in_stream.is_some() && !self.lock().recording {
            self.in_stream = None;
            let mut s = self.lock();
            s.in_channels = 0;
            s.in_peaks.clear();
            s.in_meter = [0.0; 2];
        }
    }

    /// How many input channels the chosen input device has (2 until it has been asked).
    pub fn input_channel_count(&self) -> usize {
        let open = self.lock().in_channels;
        if open > 0 {
            open
        } else if self.in_known > 0 {
            self.in_known
        } else {
            2
        }
    }

    /// Ask the input device how many channels it has (once; again after another device is
    /// chosen). Never in tests.
    pub fn probe_input_channels(&mut self) {
        if self.in_known > 0 || cfg!(test) {
            return;
        }
        // asking the sound system can take a moment: it runs on its own thread, so the
        // window never waits for it (the menus show 2 inputs until the answer comes)
        if let Some(rx) = &self.in_probe {
            if let Ok(n) = rx.try_recv() {
                self.in_known = n.max(1);
                self.in_probe = None;
            }
            return;
        }
        let choice = self.in_choice.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.in_probe = Some(rx);
        let _ = std::thread::Builder::new()
            .name("input-probe".into())
            .spawn(move || {
                let host = cpal::default_host();
                let device = match &choice {
                    Some(name) => host
                        .input_devices()
                        .ok()
                        .and_then(|mut it| it.find(|d| &dev_name(d) == name))
                        .or_else(|| host.default_input_device()),
                    None => host.default_input_device(),
                };
                let n = device
                    .and_then(|d| d.default_input_config().ok())
                    .map(|c| c.channels().max(1) as usize)
                    .unwrap_or(2);
                let _ = tx.send(n);
            });
    }

    pub fn input_failed(&self) -> bool {
        self.in_failed
    }

    /// Another input device was chosen: reopen on the next frame that needs the input.
    pub fn restart_input(&mut self) {
        self.in_known = 0;
        self.in_probe = None;
        if !self.lock().recording {
            self.in_stream = None;
            self.lock().in_channels = 0;
        }
        self.in_failed = false;
    }

    /// Start capturing into the take buffer (the input opens if it is not open yet).
    pub fn start_recording(&mut self) -> Result<(), String> {
        if cfg!(test) {
            return Err("no recording device in tests".into());
        }
        if self.in_stream.is_none() {
            self.in_failed = false;
            self.open_input()?;
        }
        let mut s = self.lock();
        s.rec = vec![vec![]; s.in_channels.max(1)];
        s.recording = true;
        Ok(())
    }

    fn open_input(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = match &self.in_choice {
            Some(name) => host
                .input_devices()
                .map_err(|e| e.to_string())?
                .find(|d| &dev_name(d) == name)
                .or_else(|| host.default_input_device()),
            None => host.default_input_device(),
        }
        .ok_or("No input (microphone) device found")?;
        self.in_device_name = dev_name(&device);
        let sup = device.default_input_config().map_err(|e| e.to_string())?;
        let fmt = sup.sample_format();
        let config: cpal::StreamConfig = sup.config();
        {
            let mut s = self.lock();
            s.rec_rate = config.sample_rate;
            s.in_channels = config.channels.max(1) as usize;
            s.in_peaks = vec![0.0; s.in_channels];
        }
        self.in_known = config.channels.max(1) as usize;
        let stream = match fmt {
            cpal::SampleFormat::F32 => self.build_in::<f32>(&device, config)?,
            cpal::SampleFormat::I16 => self.build_in::<i16>(&device, config)?,
            cpal::SampleFormat::U16 => self.build_in::<u16>(&device, config)?,
            cpal::SampleFormat::I32 => self.build_in::<i32>(&device, config)?,
            other => return Err(format!("unsupported input format {other}")),
        };
        stream.play().map_err(|e| e.to_string())?;
        self.in_stream = Some(stream);
        Ok(())
    }

    fn build_in<T>(
        &self,
        device: &cpal::Device,
        config: cpal::StreamConfig,
    ) -> Result<cpal::Stream, String>
    where
        T: cpal::SizedSample,
        f32: cpal::FromSample<T>,
    {
        let shared = self.shared.clone();
        let chans = config.channels.max(1) as usize;
        let ctx = self.ctx.clone();
        device
            .build_input_stream(
                config,
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    let mut s = match shared.lock() {
                        Ok(g) => g,
                        Err(p) => p.into_inner(),
                    };
                    let mut pk = [0.0f32; 2];
                    if s.in_peaks.len() != chans {
                        s.in_peaks = vec![0.0; chans];
                    }
                    let mut per = vec![0.0f32; chans];
                    let monitoring = !s.monitors.is_empty();
                    for fr in data.chunks(chans) {
                        for (c, v) in fr.iter().enumerate() {
                            let f: f32 = <f32 as cpal::FromSample<T>>::from_sample_(*v);
                            pk[c.min(1)] = pk[c.min(1)].max(f.abs());
                            per[c] = per[c].max(f.abs());
                            if s.recording {
                                if let Some(ch) = s.rec.get_mut(c) {
                                    ch.push(f);
                                }
                            }
                        }
                        if monitoring {
                            // every input channel, so a track can monitor input 3, 4...
                            s.mon_stride = chans.min(64);
                            for v in fr.iter().take(64) {
                                let f: f32 = <f32 as cpal::FromSample<T>>::from_sample_(*v);
                                s.mon_ring.push_back(f);
                            }
                        }
                    }
                    if chans == 1 {
                        pk[1] = pk[0];
                    }
                    for (c, p) in per.into_iter().enumerate() {
                        s.in_peaks[c] = p.max(s.in_peaks[c] * 0.8);
                    }
                    s.in_meter = [
                        pk[0].max(s.in_meter[0] * 0.8),
                        pk[1].max(s.in_meter[1] * 0.8),
                    ];
                    if let Some(c) = &ctx {
                        c.request_repaint();
                    }
                },
                |e| eprintln!("audio input error: {e}"),
                None,
            )
            .map_err(|e| e.to_string())
    }

    /// Stop recording and return what was captured.
    /// Stop the audio streams first (no callback may run a plug-in any more), then drop every
    /// effect chain so plug-in instances terminate on this (the UI) thread.
    pub fn shutdown(&mut self) {
        self.out_stream = None;
        self.in_stream = None;
        self.lock().release_plugins();
    }

    /// Stop capturing and return the take. The input stays open; the app closes it when no
    /// track is armed or monitored any more.
    pub fn stop_recording(&mut self) -> Option<AudioData> {
        let mut s = self.lock();
        if !s.recording {
            return None;
        }
        s.recording = false;
        let rec = std::mem::take(&mut s.rec);
        let d = AudioData {
            rate: s.rec_rate,
            ch: rec,
        };
        if d.frames() == 0 { None } else { Some(d) }
    }
}
