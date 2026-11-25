//! Audio data container, file loading/saving, resampling and waveform peak pyramids.

use std::path::Path;

/// Planar floating point audio.
#[derive(Clone, Debug, Default)]
pub struct AudioData {
    pub rate: u32,
    pub ch: Vec<Vec<f32>>,
}

impl AudioData {
    pub fn new(rate: u32, channels: usize, frames: usize) -> Self {
        Self {
            rate,
            ch: vec![vec![0.0; frames]; channels.max(1)],
        }
    }
    pub fn frames(&self) -> usize {
        self.ch.first().map(|c| c.len()).unwrap_or(0)
    }
    /// A frame range made safe to slice: the end kept inside the audio and the start never
    /// past the end (a reversed or out-of-range request becomes empty).
    pub fn span(&self, a: usize, b: usize) -> (usize, usize) {
        let b = b.min(self.frames());
        (a.min(b), b)
    }
    pub fn channels(&self) -> usize {
        self.ch.len()
    }
    pub fn duration(&self) -> f64 {
        if self.rate == 0 {
            0.0
        } else {
            self.frames() as f64 / self.rate as f64
        }
    }
    /// The audio without frames `a..b`, the two sides joined with an equal-power crossfade of
    /// `fade` frames (the last `fade` frames before the cut fade out while the first `fade`
    /// frames after it fade in), so a delete never clicks. `fade` 0 is a plain cut.
    pub fn without_smooth(&self, a: usize, b: usize, fade: usize) -> AudioData {
        let (a, b) = self.span(a, b);
        let n = fade.min(a).min(self.frames().saturating_sub(b));
        if n == 0 {
            return self.without(a, b);
        }
        let mut out = AudioData::new(self.rate, self.channels(), 0);
        for (c, ch) in self.ch.iter().enumerate() {
            let mut v = Vec::with_capacity(ch.len() - (b - a) - n);
            v.extend_from_slice(&ch[..a - n]);
            for i in 0..n {
                let t = (i as f32 + 0.5) / n as f32;
                v.push(ch[a - n + i] * (1.0 - t).sqrt() + ch[b + i] * t.sqrt());
            }
            v.extend_from_slice(&ch[b + n..]);
            out.ch[c] = v;
        }
        out
    }

    /// Copy a range of frames (clamped).
    pub fn slice(&self, a: usize, b: usize) -> AudioData {
        let n = self.frames();
        let a = a.min(n);
        let b = b.clamp(a, n);
        AudioData {
            rate: self.rate,
            ch: self.ch.iter().map(|c| c[a..b].to_vec()).collect(),
        }
    }
    /// Remove a range and return the remainder.
    pub fn without(&self, a: usize, b: usize) -> AudioData {
        let n = self.frames();
        let a = a.min(n);
        let b = b.clamp(a, n);
        AudioData {
            rate: self.rate,
            ch: self
                .ch
                .iter()
                .map(|c| {
                    let mut v = Vec::with_capacity(n - (b - a));
                    v.extend_from_slice(&c[..a]);
                    v.extend_from_slice(&c[b..]);
                    v
                })
                .collect(),
        }
    }
    /// Insert other audio (converted to our channel layout / rate) at frame `at`.
    pub fn inserted(&self, at: usize, other: &AudioData) -> AudioData {
        let other = other.conform(self.rate, self.channels());
        let at = at.min(self.frames());
        AudioData {
            rate: self.rate,
            ch: self
                .ch
                .iter()
                .zip(other.ch.iter())
                .map(|(c, o)| {
                    let mut v = Vec::with_capacity(c.len() + o.len());
                    v.extend_from_slice(&c[..at]);
                    v.extend_from_slice(o);
                    v.extend_from_slice(&c[at..]);
                    v
                })
                .collect(),
        }
    }
    /// Mix other audio onto ours starting at `at` (extends if needed).
    pub fn mixed(&self, at: usize, other: &AudioData, other_gain: f32) -> AudioData {
        let other = other.conform(self.rate, self.channels());
        let mut out = self.clone();
        let need = at + other.frames();
        for (c, o) in out.ch.iter_mut().zip(other.ch.iter()) {
            if c.len() < need {
                c.resize(need, 0.0);
            }
            for (i, s) in o.iter().enumerate() {
                c[at + i] += s * other_gain;
            }
        }
        out
    }
    /// Return the same audio at a given rate and channel count.
    pub fn conform(&self, rate: u32, channels: usize) -> AudioData {
        let mut d = if rate != self.rate {
            resample(self, rate)
        } else {
            self.clone()
        };
        d = remap_channels(&d, channels);
        d
    }
    pub fn peak(&self) -> f32 {
        self.ch
            .iter()
            .flat_map(|c| c.iter())
            .fold(0.0f32, |m, s| m.max(s.abs()))
    }
}

pub fn remap_channels(d: &AudioData, channels: usize) -> AudioData {
    let channels = channels.max(1);
    if d.channels() == channels {
        return d.clone();
    }
    let n = d.frames();
    let mut out = AudioData::new(d.rate, channels, n);
    if channels == 1 {
        let k = d.channels() as f32;
        for i in 0..n {
            out.ch[0][i] = d.ch.iter().map(|c| c[i]).sum::<f32>() / k;
        }
    } else if d.channels() == 1 {
        for c in out.ch.iter_mut() {
            c.copy_from_slice(&d.ch[0]);
        }
    } else {
        for (ci, c) in out.ch.iter_mut().enumerate() {
            c.copy_from_slice(&d.ch[ci % d.channels()]);
        }
    }
    out
}

/// Windowed-sinc resampler (offline quality).
pub fn resample(d: &AudioData, rate: u32) -> AudioData {
    if rate == d.rate || d.frames() == 0 {
        let mut o = d.clone();
        o.rate = rate;
        return o;
    }
    let ratio = rate as f64 / d.rate as f64;
    let n_out = ((d.frames() as f64) * ratio).round() as usize;
    let taps: i64 = 16;
    let cutoff = ratio.min(1.0);
    let ch =
        d.ch.iter()
            .map(|src| {
                let n = src.len() as i64;
                let mut out = vec![0.0f32; n_out];
                for (i, o) in out.iter_mut().enumerate() {
                    let t = i as f64 / ratio;
                    let center = t.floor() as i64;
                    let mut acc = 0.0f64;
                    let mut wsum = 0.0f64;
                    for k in (center - taps + 1)..=(center + taps) {
                        if k < 0 || k >= n {
                            continue;
                        }
                        let x = t - k as f64;
                        let sinc = if x.abs() < 1e-9 {
                            1.0
                        } else {
                            let px = std::f64::consts::PI * x * cutoff;
                            px.sin() / px
                        };
                        let w = 0.5 + 0.5 * (std::f64::consts::PI * x / taps as f64).cos();
                        let wk = sinc * w;
                        acc += src[k as usize] as f64 * wk;
                        wsum += wk;
                    }
                    *o = if wsum.abs() > 1e-9 {
                        (acc / wsum) as f32
                    } else {
                        0.0
                    };
                }
                out
            })
            .collect();
    AudioData { rate, ch }
}

pub struct LoadedFile {
    pub data: AudioData,
    pub bits: u16,
    pub float: bool,
}

pub const AUDIO_EXTS: &[&str] = &[
    "wav", "mp3", "flac", "ogg", "oga", "m4a", "aac", "aif", "aiff", "mp4", "mka", "webm", "caf",
];

pub fn is_audio_path(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Refuse to hold `bytes` more of audio when the computer cannot give that much memory (with
/// some room to spare): running out would end the program instead of reporting it.
pub fn ensure_room(bytes: u64) -> Result<(), String> {
    let Some(free) = crate::sysinfo::available_memory() else {
        return Ok(());
    };
    if bytes.saturating_add(256 << 20) > free {
        let gb = |b: u64| b as f64 / 1_073_741_824.0;
        return Err(format!(
            "Not enough memory to open this file: it needs about {:.1} GB and {:.1} GB is free. AudioCraft keeps audio in memory as 32-bit samples (about 0.7 GB per hour of stereo at 48 kHz); close other files or programs, or split very long recordings.",
            gb(bytes),
            gb(free)
        ));
    }
    Ok(())
}

pub fn load(path: &Path) -> Result<LoadedFile, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "wav" {
        if let Ok(f) = load_wav(path) {
            return Ok(f);
        }
    }
    load_symphonia(path, &ext)
}

fn load_wav(path: &Path) -> Result<LoadedFile, String> {
    let mut r = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = r.spec();
    let chs = spec.channels.max(1) as usize;
    // the length is in the header: check the memory first, then fill the channels directly
    // (no interleaved copy, which needed twice the memory for a moment)
    let frames_hint = r.duration() as usize;
    ensure_room(frames_hint as u64 * chs as u64 * 4)?;
    let mut planar: Vec<Vec<f32>> = (0..chs).map(|_| Vec::with_capacity(frames_hint)).collect();
    let mut c = 0usize;
    match spec.sample_format {
        hound::SampleFormat::Float => {
            for s in r.samples::<f32>() {
                planar[c].push(s.map_err(|e| e.to_string())?);
                c = (c + 1) % chs;
            }
        }
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            for s in r.samples::<i32>() {
                planar[c].push(s.map_err(|e| e.to_string())? as f32 * scale);
                c = (c + 1) % chs;
            }
        }
    }
    // Preferences > Data: a 32-bit float file above 0 dBFS is clipped on open when asked
    if spec.sample_format == hound::SampleFormat::Float && crate::prefs::clip_float_on_open() {
        for p in planar.iter_mut() {
            for v in p.iter_mut() {
                *v = v.clamp(-1.0, 1.0);
            }
        }
    }
    // a cut-off last frame: every channel the same length
    let frames = planar.iter().map(|p| p.len()).min().unwrap_or(0);
    for p in planar.iter_mut() {
        p.truncate(frames);
    }
    let data = AudioData {
        rate: spec.sample_rate,
        ch: planar,
    };
    Ok(LoadedFile {
        data,
        bits: spec.bits_per_sample,
        float: spec.sample_format == hound::SampleFormat::Float,
    })
}

fn load_symphonia(path: &Path, ext: &str) -> Result<LoadedFile, String> {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::formats::TrackType;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if !ext.is_empty() {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| format!("Unsupported format: {e}"))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("No audio track")?;
    let params = track
        .codec_params
        .as_ref()
        .ok_or("Missing codec parameters")?;
    let aparams = params.audio().ok_or("Not an audio track")?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(aparams, &AudioDecoderOptions::default())
        .map_err(|e| format!("Unsupported codec: {e}"))?;
    let track_id = track.id;
    let mut rate = 0u32;
    let mut chs = 0usize;
    let mut planar: Vec<Vec<f32>> = Vec::new();
    let mut buf: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(_) => break,
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(ab) => {
                let spec = ab.spec();
                if chs == 0 {
                    rate = spec.rate();
                    chs = spec.channels().count().max(1);
                    planar = vec![Vec::new(); chs];
                }
                buf.resize(ab.samples_interleaved(), 0.0);
                ab.copy_to_slice_interleaved(&mut buf);
                let c_here = spec.channels().count().max(1);
                // before the channels grow again, make sure the memory is there
                let (len, cap) = planar
                    .first()
                    .map(|p| (p.len(), p.capacity()))
                    .unwrap_or((0, 0));
                if len + buf.len() / c_here > cap {
                    let next = (cap.max(1 << 16) * 2) as u64 * chs as u64 * 4;
                    ensure_room(next.saturating_sub(cap as u64 * chs as u64 * 4))?;
                }
                for fr in buf.chunks(c_here) {
                    for c in 0..chs {
                        planar[c].push(*fr.get(c).unwrap_or(&0.0));
                    }
                }
            }
            Err(Error::DecodeError(_)) | Err(Error::IoError(_)) => continue,
            Err(_) => break,
        }
    }
    if chs == 0 {
        return Err("No audio decoded".into());
    }
    Ok(LoadedFile {
        data: AudioData { rate, ch: planar },
        bits: 32,
        float: true,
    })
}

pub fn save_wav(path: &Path, d: &AudioData, bits: u16, float: bool) -> Result<(), String> {
    write_atomic(path, &wav_bytes(d, bits, float)?)
}

/// Multi-resolution min/max peaks for fast waveform drawing.
#[derive(Default)]
pub struct Peaks {
    /// levels[l][ch] = Vec<(min,max)> with block size BLOCKS[l]
    pub levels: Vec<Vec<Vec<(f32, f32)>>>,
}

pub const BLOCKS: [usize; 3] = [64, 1024, 16384];

impl Peaks {
    pub fn build(d: &AudioData) -> Peaks {
        let mut levels = Vec::new();
        // level 0 from raw samples; long files split the work over a few threads so a 20-minute
        // take is ready in a blink after an edit
        let block0 = |c: &[f32]| -> Vec<(f32, f32)> {
            c.chunks(BLOCKS[0])
                .map(|b| {
                    b.iter()
                        .fold((f32::MAX, f32::MIN), |(lo, hi), &s| (lo.min(s), hi.max(s)))
                })
                .collect()
        };
        let parallel = |c: &Vec<f32>| -> Vec<(f32, f32)> {
            const PART: usize = BLOCKS[0] * 16384;
            if c.len() < PART * 2 {
                return block0(c);
            }
            std::thread::scope(|s| {
                let parts: Vec<_> = c.chunks(PART).map(|p| s.spawn(move || block0(p))).collect();
                parts
                    .into_iter()
                    .flat_map(|h| h.join().unwrap_or_default())
                    .collect()
            })
        };
        let l0: Vec<Vec<(f32, f32)>> = d.ch.iter().map(parallel).collect();
        levels.push(l0);
        for l in 1..BLOCKS.len() {
            let f = BLOCKS[l] / BLOCKS[l - 1];
            let prev = &levels[l - 1];
            let next: Vec<Vec<(f32, f32)>> = prev
                .iter()
                .map(|c| {
                    c.chunks(f)
                        .map(|b| {
                            b.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &(a, z)| {
                                (lo.min(a), hi.max(z))
                            })
                        })
                        .collect()
                })
                .collect();
            levels.push(next);
        }
        Peaks { levels }
    }

    /// Min/max over frame range [a,b) for channel c.
    pub fn range(&self, d: &AudioData, c: usize, a: usize, b: usize) -> Option<(f32, f32)> {
        let n = d.frames();
        let a = a.min(n);
        let b = b.min(n);
        if b <= a || c >= d.channels() {
            return None;
        }
        let span = b - a;
        let mut lvl: Option<usize> = None;
        for (l, &bs) in BLOCKS.iter().enumerate() {
            if span >= bs * 2 {
                lvl = Some(l);
            }
        }
        match lvl {
            None => {
                let s = &d.ch[c][a..b];
                Some(
                    s.iter()
                        .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v))),
                )
            }
            Some(l) => {
                let bs = BLOCKS[l];
                let lv = self.levels.get(l)?.get(c)?;
                let ia = a / bs;
                let ib = ((b + bs - 1) / bs).min(lv.len());
                if ib <= ia {
                    return None;
                }
                Some(
                    lv[ia..ib]
                        .iter()
                        .fold((f32::MAX, f32::MIN), |(lo, hi), &(x, y)| {
                            (lo.min(x), hi.max(y))
                        }),
                )
            }
        }
    }
}

pub fn db(v: f32) -> f32 {
    if v <= 1e-9 { -180.0 } else { 20.0 * v.log10() }
}
pub fn undb(d: f32) -> f32 {
    10f32.powf(d / 20.0)
}

pub fn fmt_time(t: f64) -> String {
    let t = t.max(0.0);
    let total_ms = (t * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let s = (total_ms / 1000) % 60;
    let m = (total_ms / 60000) % 60;
    let h = total_ms / 3_600_000;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{ms:03}")
    } else {
        format!("{m}:{s:02}.{ms:03}")
    }
}

pub fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.split(':').collect();
    let mut total = 0.0;
    for p in parts {
        let v: f64 = p.trim().parse().ok()?;
        total = total * 60.0 + v;
    }
    Some(total)
}

// ---------------------------------------------------------------- safe saving and more formats

/// Write bytes to `path` safely: write a temporary file next to it, flush it to disk, then
/// replace the target in one step. A crash or a full disk never leaves a half-written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "audio".into());
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let res = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("could not write {}: {e}", path.display()));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not replace {}: {e}", path.display())
    })
}

/// The container a path's extension asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveFormat {
    Wav,
    Aiff,
    Flac,
    Mp3,
    Aac,
}

impl SaveFormat {
    pub fn from_path(p: &Path) -> Option<SaveFormat> {
        match p.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "wav" | "bwf" => Some(SaveFormat::Wav),
            "aif" | "aiff" => Some(SaveFormat::Aiff),
            "flac" => Some(SaveFormat::Flac),
            "mp3" => Some(SaveFormat::Mp3),
            "m4a" | "aac" | "mp4" => Some(SaveFormat::Aac),
            _ => None,
        }
    }
    pub fn lossy(self) -> bool {
        matches!(self, SaveFormat::Mp3 | SaveFormat::Aac)
    }
}

pub const SAVE_FILTERS: &[(&str, &[&str])] = &[
    ("Wave PCM / Broadcast Wave (*.wav)", &["wav"]),
    ("AIFF (*.aif, *.aiff)", &["aif", "aiff"]),
    ("FLAC (*.flac)", &["flac"]),
    ("MP3 (*.mp3, bundled FFmpeg)", &["mp3"]),
    ("AAC (*.m4a, bundled FFmpeg)", &["m4a"]),
];

/// Triangular (TPDF) dither noise in least-significant bits, from a small fixed-seed
/// generator, so a 16-bit or 24-bit file carries noise instead of distortion at low levels
/// (Preferences > Data). Off when the depth is 0.
struct Dither {
    s: u32,
    lsb: f32,
}

impl Dither {
    fn new(lsb: f32) -> Self {
        Dither {
            s: 0x9E37_79B9,
            lsb: if lsb.is_finite() { lsb.max(0.0) } else { 0.0 },
        }
    }

    fn unit(&mut self) -> f32 {
        self.s ^= self.s << 13;
        self.s ^= self.s >> 17;
        self.s ^= self.s << 5;
        self.s as f32 / u32::MAX as f32
    }

    fn noise(&mut self) -> f32 {
        if self.lsb <= 0.0 {
            0.0
        } else {
            (self.unit() - self.unit()) * self.lsb
        }
    }
}

/// A sample as an integer of `maxv` full scale, dithered when asked.
fn quantize(v: f32, maxv: f32, d: &mut Dither) -> i32 {
    (v.clamp(-1.0, 1.0) * maxv + d.noise())
        .round()
        .clamp(-maxv - 1.0, maxv) as i32
}

fn wav_bytes(d: &AudioData, bits: u16, float: bool) -> Result<Vec<u8>, String> {
    let (bits, float) = match (bits, float) {
        (32, true) => (32, true),
        (8, _) => (8, false),
        (16, _) => (16, false),
        (24, _) => (24, false),
        (32, false) => (32, false),
        _ => (32, true),
    };
    let spec = hound::WavSpec {
        channels: d.channels() as u16,
        sample_rate: d.rate,
        bits_per_sample: bits,
        sample_format: if float {
            hound::SampleFormat::Float
        } else {
            hound::SampleFormat::Int
        },
    };
    let mut cur = std::io::Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut cur, spec).map_err(|e| e.to_string())?;
        let n = d.frames();
        if float {
            for i in 0..n {
                for c in &d.ch {
                    w.write_sample(c[i]).map_err(|e| e.to_string())?;
                }
            }
        } else {
            let maxv = ((1i64 << (bits - 1)) - 1) as f32;
            let mut dz = Dither::new(crate::prefs::dither_lsb());
            for i in 0..n {
                for c in &d.ch {
                    w.write_sample(quantize(c[i], maxv, &mut dz))
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        w.finalize().map_err(|e| e.to_string())?;
    }
    Ok(cur.into_inner())
}

/// Civil date and time (UTC) for BWF metadata, without extra dependencies.
fn utc_now() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (
        format!("{y:04}-{m:02}-{d:02}"),
        format!("{:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60),
    )
}

/// Insert a Broadcast Wave `bext` chunk into WAV bytes.
fn add_bext(mut wav: Vec<u8>, description: &str, time_reference: u64) -> Vec<u8> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return wav;
    }
    let mut b = Vec::with_capacity(610);
    let fixed = |s: &str, n: usize, b: &mut Vec<u8>| {
        let mut bytes: Vec<u8> = s.bytes().filter(|c| c.is_ascii()).take(n).collect();
        bytes.resize(n, 0);
        b.extend_from_slice(&bytes);
    };
    let (date, time) = utc_now();
    fixed(description, 256, &mut b);
    fixed("AudioCraft", 32, &mut b);
    fixed("", 32, &mut b);
    fixed(&date, 10, &mut b);
    fixed(&time, 8, &mut b);
    b.extend_from_slice(&(time_reference as u32).to_le_bytes());
    b.extend_from_slice(&((time_reference >> 32) as u32).to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&[0u8; 64]);
    b.extend_from_slice(&[0u8; 10]);
    b.extend_from_slice(&[0u8; 180]);
    b.extend_from_slice(b"A=PCM,T=AudioCraft\r\n");
    if b.len() % 2 == 1 {
        b.push(0);
    }
    let mut chunk = b"bext".to_vec();
    chunk.extend_from_slice(&(b.len() as u32).to_le_bytes());
    chunk.extend_from_slice(&b);
    let tail = wav.split_off(12);
    wav.extend_from_slice(&chunk);
    wav.extend_from_slice(&tail);
    let riff = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff.to_le_bytes());
    wav
}

/// True when WAV bytes carry a Broadcast Wave `bext` chunk.
pub fn has_bext(path: &Path) -> bool {
    use std::io::Read;
    let mut buf = vec![0u8; 4096];
    let n = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut buf))
        .unwrap_or(0);
    buf[..n].windows(4).any(|w| w == b"bext")
}

fn ext80(rate: f64) -> [u8; 10] {
    // IEEE 754 80-bit extended, big-endian, for the AIFF COMM chunk.
    let mut out = [0u8; 10];
    if rate <= 0.0 {
        return out;
    }
    let exp = rate.log2().floor() as i32;
    let mant = (rate / 2f64.powi(exp) * (1u64 << 63) as f64) as u64;
    let e = (exp + 16383) as u16;
    out[0..2].copy_from_slice(&e.to_be_bytes());
    out[2..10].copy_from_slice(&mant.to_be_bytes());
    out
}

fn aiff_bytes(d: &AudioData, bits: u16) -> Vec<u8> {
    let mut dz = Dither::new(crate::prefs::dither_lsb());
    let bits = if bits == 24 {
        24u16
    } else if bits == 8 {
        8
    } else {
        16
    };
    let bps = (bits / 8) as usize;
    let n = d.frames();
    let chs = d.channels();
    let data_len = n * chs * bps;
    let mut v = Vec::with_capacity(54 + data_len);
    v.extend_from_slice(b"FORM");
    v.extend_from_slice(&((4 + 26 + 16 + data_len) as u32).to_be_bytes());
    v.extend_from_slice(b"AIFF");
    v.extend_from_slice(b"COMM");
    v.extend_from_slice(&18u32.to_be_bytes());
    v.extend_from_slice(&(chs as u16).to_be_bytes());
    v.extend_from_slice(&(n as u32).to_be_bytes());
    v.extend_from_slice(&bits.to_be_bytes());
    v.extend_from_slice(&ext80(d.rate as f64));
    v.extend_from_slice(b"SSND");
    v.extend_from_slice(&((8 + data_len) as u32).to_be_bytes());
    v.extend_from_slice(&0u32.to_be_bytes());
    v.extend_from_slice(&0u32.to_be_bytes());
    let maxv = ((1i64 << (bits - 1)) - 1) as f32;
    for i in 0..n {
        for c in &d.ch {
            let s = quantize(c[i], maxv, &mut dz);
            let be = s.to_be_bytes();
            v.extend_from_slice(&be[4 - bps..]);
        }
    }
    v
}

fn flac_bytes(d: &AudioData, bits: u16) -> Result<Vec<u8>, String> {
    let mut dz = Dither::new(crate::prefs::dither_lsb());
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    let bits = if bits == 24 { 24usize } else { 16 };
    let maxv = ((1i64 << (bits - 1)) - 1) as f32;
    let mut inter: Vec<i32> = Vec::with_capacity(d.frames() * d.channels());
    for i in 0..d.frames() {
        for c in &d.ch {
            inter.push(quantize(c[i], maxv, &mut dz));
        }
    }
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| format!("FLAC encoder setup failed: {:?}", e.1))?;
    let source =
        flacenc::source::MemSource::from_samples(&inter, d.channels(), bits, d.rate as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| format!("FLAC encoding failed: {e:?}"))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| format!("FLAC writing failed: {e:?}"))?;
    Ok(sink.as_slice().to_vec())
}

/// Find an FFmpeg executable for MP3/AAC export. The bundled decoder is used first so exporting
/// works on a machine with no FFmpeg installed; a system copy is still accepted as a fallback.
pub fn ffmpeg() -> Option<std::path::PathBuf> {
    crate::ffmpeg::find()
}

/// RIFF INFO fields: (key in the metadata map, chunk id).
pub const RIFF_INFO: [(&str, &[u8; 4]); 8] = [
    ("Title", b"INAM"),
    ("Artist", b"IART"),
    ("Album", b"IPRD"),
    ("Genre", b"IGNR"),
    ("Comment", b"ICMT"),
    ("Copyright", b"ICOP"),
    ("Date", b"ICRD"),
    ("Engineer", b"IENG"),
];

/// Append a LIST/INFO chunk with the file's metadata (skipped when there is none).
fn add_info(mut wav: Vec<u8>, meta: &std::collections::BTreeMap<String, String>) -> Vec<u8> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" {
        return wav;
    }
    let mut body = b"INFO".to_vec();
    for (key, id) in RIFF_INFO {
        if let Some(v) = meta.get(key).filter(|v| !v.trim().is_empty()) {
            let mut text: Vec<u8> = v.bytes().filter(|c| *c != 0).collect();
            text.push(0);
            if text.len() % 2 == 1 {
                text.push(0);
            }
            body.extend_from_slice(id.as_slice());
            body.extend_from_slice(&((text.len()) as u32).to_le_bytes());
            body.extend_from_slice(&text);
        }
    }
    if body.len() == 4 {
        return wav;
    }
    wav.extend_from_slice(b"LIST");
    wav.extend_from_slice(&(body.len() as u32).to_le_bytes());
    wav.extend_from_slice(&body);
    let riff = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff.to_le_bytes());
    wav
}

/// Append an iXML chunk with the production fields (skipped when there are none).
fn add_ixml(mut wav: Vec<u8>, meta: &std::collections::BTreeMap<String, String>) -> Vec<u8> {
    let fields = [
        ("Project", "PROJECT"),
        ("Scene", "SCENE"),
        ("Take", "TAKE"),
        ("Tape", "TAPE"),
        ("Note", "NOTE"),
    ];
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let inner: String = fields
        .iter()
        .filter_map(|(k, tag)| {
            meta.get(&format!("ixml.{k}"))
                .filter(|v| !v.is_empty())
                .map(|v| format!("<{tag}>{}</{tag}>", esc(v)))
        })
        .collect();
    if inner.is_empty() || wav.len() < 12 {
        return wav;
    }
    let mut x = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><BWFXML><IXML_VERSION>1.5</IXML_VERSION>{inner}</BWFXML>").into_bytes();
    if x.len() % 2 == 1 {
        x.push(0);
    }
    wav.extend_from_slice(b"iXML");
    wav.extend_from_slice(&(x.len() as u32).to_le_bytes());
    wav.extend_from_slice(&x);
    let riff = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff.to_le_bytes());
    wav
}

/// Read the RIFF INFO fields of a WAV file into a metadata map (empty when there are none).
pub fn read_wav_meta(path: &Path) -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    let Ok(b) = std::fs::read(path) else { return m };
    if b.len() < 12 || &b[0..4] != b"RIFF" {
        return m;
    }
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = i + 8;
        if body + len > b.len() {
            break;
        }
        if id == b"LIST" && len >= 4 && &b[body..body + 4] == b"INFO" {
            let mut j = body + 4;
            while j + 8 <= body + len {
                let sid = &b[j..j + 4];
                let sl = u32::from_le_bytes([b[j + 4], b[j + 5], b[j + 6], b[j + 7]]) as usize;
                let s0 = j + 8;
                if s0 + sl > body + len {
                    break;
                }
                if let Some((k, _)) = RIFF_INFO.iter().find(|(_, c)| c.as_slice() == sid) {
                    let v = String::from_utf8_lossy(&b[s0..s0 + sl])
                        .trim_end_matches('\0')
                        .to_string();
                    m.insert(k.to_string(), v);
                }
                j = s0 + sl + (sl & 1);
            }
        }
        i = body + len + (len & 1);
    }
    m
}

/// Save audio in the format the path's extension names. Writes are atomic.
pub fn save_any(
    path: &Path,
    d: &AudioData,
    bits: u16,
    float: bool,
    bwf: bool,
) -> Result<(), String> {
    save_with_meta(path, d, bits, float, bwf, &Default::default())
}

/// Like [`save_any`], writing the file's metadata too (WAV: bext, LIST/INFO and iXML; MP3/AAC:
/// title, artist, album, genre, comment, date).
pub fn save_with_meta(
    path: &Path,
    d: &AudioData,
    bits: u16,
    float: bool,
    bwf: bool,
    meta: &std::collections::BTreeMap<String, String>,
) -> Result<(), String> {
    if d.ch.iter().flat_map(|c| c.iter()).any(|v| !v.is_finite()) {
        return Err("the audio contains invalid samples (NaN); nothing was written".into());
    }
    let fmt = SaveFormat::from_path(path)
        .ok_or("unknown file type: use .wav, .aif, .flac, .mp3 or .m4a")?;
    match fmt {
        SaveFormat::Wav => {
            let mut bytes = wav_bytes(d, bits, float)?;
            if bwf {
                let desc = meta
                    .get("bext.Description")
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .unwrap_or_else(|| {
                        path.file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default()
                    });
                bytes = add_bext(bytes, &desc, 0);
            }
            bytes = add_info(bytes, meta);
            bytes = add_ixml(bytes, meta);
            write_atomic(path, &bytes)
        }
        SaveFormat::Aiff => write_atomic(path, &aiff_bytes(d, if float { 24 } else { bits })),
        SaveFormat::Flac => write_atomic(path, &flac_bytes(d, if float { 24 } else { bits })?),
        SaveFormat::Mp3 | SaveFormat::Aac => {
            let exe = ffmpeg().ok_or("MP3 and AAC export use FFmpeg, which was not found beside the app or on PATH. Reinstall AudioCraft with vendor/ffmpeg, or save as WAV, AIFF or FLAC.")?;
            let tmp =
                crate::prefs::temp_dir().join(format!("audiocraft-export-{}.wav", std::process::id()));
            std::fs::write(&tmp, wav_bytes(d, 24, false)?).map_err(|e| e.to_string())?;
            let out_tmp = path.with_extension(format!(
                "partial.{}",
                path.extension().and_then(|e| e.to_str()).unwrap_or("mp3")
            ));
            let codec: &[&str] = if fmt == SaveFormat::Mp3 {
                &["-c:a", "libmp3lame", "-b:a", "256k"]
            } else {
                &["-c:a", "aac", "-b:a", "256k"]
            };
            // ID3 / iTunes-style tags from the metadata
            let mut tags: Vec<String> = vec![];
            for (k, tag) in [
                ("Title", "title"),
                ("Artist", "artist"),
                ("Album", "album"),
                ("Genre", "genre"),
                ("Comment", "comment"),
                ("Date", "date"),
                ("Track", "track"),
            ] {
                if let Some(v) = meta.get(k).filter(|v| !v.is_empty()) {
                    tags.push("-metadata".into());
                    tags.push(format!("{tag}={v}"));
                }
            }
            let result = crate::ffmpeg::command(exe)
                .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                .arg(&tmp)
                // Hound writes mono WAVs with a front-left mask; recent FFmpeg's AAC encoder
                // rejects it. Restating the channel count gives every layout a standard mask.
                .arg("-ac")
                .arg(d.channels().to_string())
                .args(codec)
                .args(&tags)
                .arg(&out_tmp)
                .output();
            let _ = std::fs::remove_file(&tmp);
            match result {
                Ok(out) if out.status.success() => {
                    std::fs::rename(&out_tmp, path).map_err(|e| e.to_string())
                }
                Ok(out) => {
                    let _ = std::fs::remove_file(&out_tmp);
                    let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    Err(if msg.is_empty() {
                        "FFmpeg failed".into()
                    } else {
                        msg
                    })
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&out_tmp);
                    Err(format!("could not run FFmpeg: {e}"))
                }
            }
        }
    }
}
