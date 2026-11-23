//! Video import and preview.
//!
//! AudioCraft stays a native Rust app: the frames are decoded by a free FFmpeg executable
//! (the bundled one, or the one chosen in Edit > Preferences > Video). Import streams:
//! the clip's size and length arrive in a fraction of a second, its sound is decoded straight
//! from FFmpeg's output pipe on one worker, and preview thumbnails arrive in batches on another.
//! No temporary files are written and no console window ever opens. When the playhead stops,
//! a third worker decodes the exact frame under it at a higher resolution.

use crate::audio::{self, AudioData, Peaks};
use crate::imports::PreparedImport;
use crate::prefs::Prefs;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// Common video containers accepted by the import dialog and drag/drop.
pub const VIDEO_EXTS: &[&str] = &[
    "mp4", "mov", "m4v", "mkv", "avi", "webm", "mpeg", "mpg", "wmv", "flv", "ts", "mts", "3gp",
    "ogv",
];

pub fn is_video_path(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoInfo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

/// The picture's pixel format and colour description, as FFmpeg reports it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColorInfo {
    /// e.g. `yuv420p` or `yuv420p10le`.
    pub pix_fmt: String,
    /// e.g. `tv, bt709` or `tv, bt2020nc/bt2020/smpte2084`.
    pub space: String,
    /// The codec name, e.g. `h264`, `hevc` or `av1` (picks the graphics-card decoder).
    pub codec: String,
}

impl ColorInfo {
    /// Why this picture is not the standard Rec.709 / sRGB 8-bit the preview expects, if it isn't.
    pub fn problem(&self) -> Option<String> {
        let s = self.space.to_ascii_lowercase();
        let hdr = s.contains("smpte2084") || s.contains("arib-std-b67");
        let wide = s.contains("bt2020");
        let deep = ["10le", "10be", "12le", "12be", "16le", "p010", "p016"]
            .iter()
            .any(|k| self.pix_fmt.contains(k));
        let mut why = Vec::new();
        if hdr {
            why.push("HDR (PQ/HLG) transfer");
        }
        if wide {
            why.push("BT.2020 wide-gamut colour");
        }
        if deep {
            why.push("more than 8 bits per channel");
        }
        if why.is_empty() {
            None
        } else {
            Some(
                format!("{} ({} {})", why.join(", "), self.pix_fmt, self.space)
                    .trim()
                    .to_string(),
            )
        }
    }
}

#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub time: f64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct VideoMedia {
    pub path: PathBuf,
    pub name: String,
    pub info: VideoInfo,
    pub frames: Vec<VideoFrame>,
    /// False while thumbnails are still arriving.
    pub complete: bool,
    pub color: ColorInfo,
    /// Preview frames are converted to Rec.709.
    pub converted: bool,
}

impl VideoMedia {
    pub fn new(path: PathBuf, info: VideoInfo) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Video".into());
        Self {
            path,
            name,
            info,
            frames: vec![],
            complete: true,
            color: ColorInfo::default(),
            converted: false,
        }
    }

    /// The frame whose timestamp is closest to `t`, with its index so the UI can cache textures.
    pub fn frame_at(&self, t: f64) -> Option<(usize, &VideoFrame)> {
        if self.frames.is_empty() {
            return None;
        }
        let t = t.max(0.0);
        let lo = self.frames.partition_point(|f| f.time < t);
        let after = lo.min(self.frames.len() - 1);
        let before = lo.saturating_sub(1);
        let pick = if (self.frames[after].time - t).abs() < (t - self.frames[before].time).abs() {
            after
        } else {
            before
        };
        Some((pick, &self.frames[pick]))
    }
}

/// A failed import still remembers which video was picked, so the panel can show its name
/// instead of an empty black rectangle.
#[derive(Clone, Debug)]
pub struct VideoError {
    pub path: PathBuf,
    pub message: String,
}

/// Everything the Video panel needs, in one place.
pub enum VideoState {
    Empty,
    Loading(PathBuf),
    Ready(Box<VideoMedia>),
    Error(VideoError),
}

impl VideoState {
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading(_)) || matches!(self, Self::Ready(m) if !m.complete)
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Loading(p) => Some(p.as_path()),
            Self::Ready(m) => Some(m.path.as_path()),
            Self::Error(e) => Some(e.path.as_path()),
            _ => None,
        }
    }
}

/// One step of a streaming import.
pub enum VideoMsg {
    /// Length, size and colour: the panel can show the clip at once.
    Info(VideoInfo, ColorInfo),
    /// The sound (or a silent timebase) with its peaks; `true` when the clip had sound.
    Audio(PreparedImport, bool),
    /// More thumbnails, in time order.
    Frames(Vec<VideoFrame>),
    /// The import finished (an error only matters if nothing arrived).
    Done(Result<(), String>),
}

pub struct VideoEvent {
    pub path: PathBuf,
    pub msg: VideoMsg,
}

/// Options for one import.
#[derive(Clone, Debug, Default)]
pub struct ImportOpts {
    pub quality: u8,
    /// Convert wide-gamut / HDR frames to Rec.709 for the preview.
    pub convert_color: bool,
    /// Only re-decode the thumbnails (the sound is already in the Files panel).
    pub frames_only: bool,
}

/// One worker per import, cancellable. Stale events are ignored by the interface thread.
pub struct VideoQueue {
    tx: Sender<VideoEvent>,
    rx: Receiver<VideoEvent>,
    jobs: HashMap<PathBuf, Arc<AtomicBool>>,
}

impl Default for VideoQueue {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            jobs: HashMap::new(),
        }
    }
}

impl VideoQueue {
    /// Start importing; an import of the same file already running is replaced.
    pub fn start(
        &mut self,
        path: PathBuf,
        exe: PathBuf,
        opts: ImportOpts,
        ctx: Option<egui::Context>,
    ) {
        if let Some(old) = self.jobs.remove(&path) {
            old.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.jobs.insert(path.clone(), cancel.clone());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let p2 = path.clone();
            let send = move |msg: VideoMsg| {
                let _ = tx.send(VideoEvent {
                    path: p2.clone(),
                    msg,
                });
                if let Some(c) = &ctx {
                    c.request_repaint();
                }
            };
            import_stream(&path, &exe, &opts, &cancel, &send);
        });
    }

    pub fn drain(&mut self) -> Vec<VideoEvent> {
        let mut out = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            if matches!(event.msg, VideoMsg::Done(_)) {
                self.jobs.remove(&event.path);
            }
            out.push(event);
        }
        out
    }
}

/// Folders the person listed in Preferences, then standard install places.
pub fn decoder_folders(prefs: &Prefs) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut push = |p: PathBuf| {
        if !p.as_os_str().is_empty() && !out.contains(&p) {
            out.push(p);
        }
    };
    push(PathBuf::from(prefs.video_decoder_folder.trim()));
    for line in prefs.video_decoder_repos.lines() {
        push(PathBuf::from(line.trim()));
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(root) = std::env::var_os(var) {
            push(PathBuf::from(root).join("ffmpeg"));
        }
    }
    for s in [
        "/usr/bin",
        "/usr/local/bin",
        "/opt/ffmpeg/bin",
        "/opt/homebrew/bin",
    ] {
        push(PathBuf::from(s));
    }
    out
}

/// Search the person's choice, their folders, the bundled copy, downloaded builds and PATH.
pub fn detect_decoder(prefs: &Prefs) -> Result<PathBuf, String> {
    for f in decoder_folders(prefs) {
        if let Some(exe) = crate::ffmpeg::exe_below(&f, 2) {
            return Ok(exe);
        }
    }
    if let Some(i) = crate::ffmpeg::installed().into_iter().next() {
        return Ok(i.path);
    }
    Err("No video decoder was found. Keep the bundled FFmpeg in vendor/ffmpeg beside AudioCraft, or download or choose a decoder in Edit > Preferences > Video. The video was not imported.".into())
}

/// The decoder an import will use: the one chosen in Preferences first.
pub fn ensure_decoder(prefs: &Prefs) -> Result<PathBuf, String> {
    let configured = PathBuf::from(prefs.video_decoder_exe.trim());
    if configured.is_file() {
        return Ok(configured);
    }
    detect_decoder(prefs)
}

/// Run the detector and remember the result in the preferences being edited.
pub fn configure_decoder(prefs: &mut Prefs) -> Result<String, String> {
    let exe = detect_decoder(prefs)?;
    let version = crate::ffmpeg::version(&exe);
    prefs.video_decoder_exe = exe.to_string_lossy().to_string();
    Ok(format!("Using {} — {}", exe.display(), version))
}

/// Parse the `Duration: HH:MM:SS.ss` line FFmpeg prints on stderr.
pub fn parse_duration(stderr: &str) -> Option<f64> {
    for line in stderr.lines() {
        let Some(rest) = line.trim().strip_prefix("Duration:") else {
            continue;
        };
        let stamp = rest.split(',').next().unwrap_or("").trim();
        let parts: Vec<&str> = stamp.split(':').collect();
        if parts.len() != 3 {
            continue;
        }
        let h: f64 = parts[0].trim().parse().ok()?;
        let m: f64 = parts[1].trim().parse().ok()?;
        let s: f64 = parts[2].trim().parse().ok()?;
        return Some(h * 3600.0 + m * 60.0 + s);
    }
    None
}

/// Split a stream line at commas that are not inside parentheses.
fn top_level_parts(line: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut depth = 0i32;
    for c in line.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth <= 0 => {
                parts.push(String::new());
                continue;
            }
            _ => {}
        }
        if let Some(last) = parts.last_mut() {
            last.push(c);
        }
    }
    parts.into_iter().map(|p| p.trim().to_string()).collect()
}

fn stream_line<'a>(stderr: &'a str, kind: &str) -> Option<&'a str> {
    stderr
        .lines()
        .find(|l| l.contains("Stream #") && l.contains(kind))
        .or_else(|| stderr.lines().find(|l| l.contains(kind)))
}

/// Parse resolution and frame rate from a `Video: ...` stream line.
pub fn parse_video_line(stderr: &str) -> Option<(u32, u32, f64)> {
    let line = stream_line(stderr, "Video:")?;
    let mut wh = None;
    let mut fps = None;
    let words: Vec<&str> = line
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect();
    for (i, w) in words.iter().enumerate() {
        if wh.is_none() {
            let bits: Vec<&str> = w.split('x').collect();
            if bits.len() == 2 {
                if let (Ok(a), Ok(b)) = (bits[0].parse::<u32>(), bits[1].parse::<u32>()) {
                    if a > 0 && b > 0 {
                        wh = Some((a, b));
                    }
                }
            }
        }
        if fps.is_none() && *w == "fps" && i > 0 {
            fps = words[i - 1].parse::<f64>().ok();
        }
    }
    wh.map(|(w, h)| (w, h, fps.unwrap_or(30.0).max(0.1)))
}

/// Pixel format and colour description from the `Video:` line.
pub fn parse_color(stderr: &str) -> ColorInfo {
    let Some(line) = stream_line(stderr, "Video:") else {
        return ColorInfo::default();
    };
    let after = line.split_once("Video:").map(|(_, r)| r).unwrap_or(line);
    let parts = top_level_parts(after);
    let Some(px) = parts.get(1) else {
        return ColorInfo::default();
    };
    let (fmt, space) = match px.split_once('(') {
        Some((f, rest)) => (
            f.trim().to_string(),
            rest.trim_end_matches(')').trim().to_string(),
        ),
        None => (px.trim().to_string(), String::new()),
    };
    let codec = parts
        .first()
        .and_then(|c| c.split_whitespace().next())
        .unwrap_or("")
        .to_ascii_lowercase();
    ColorInfo {
        pix_fmt: fmt,
        space,
        codec,
    }
}

/// Sample rate and channel count of the first audio stream, if the file has sound.
pub fn parse_audio_line(stderr: &str) -> Option<(u32, usize)> {
    let line = stream_line(stderr, "Audio:")?;
    let words: Vec<&str> = line
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect();
    let rate = words
        .iter()
        .position(|w| *w == "Hz")
        .and_then(|i| words.get(i.wrapping_sub(1)))
        .and_then(|w| w.parse::<u32>().ok())
        .unwrap_or(48000);
    let ch = if words.contains(&"mono") {
        1
    } else if words.iter().any(|w| w.starts_with("5.1")) {
        6
    } else if words.iter().any(|w| w.starts_with("7.1")) {
        8
    } else {
        2
    };
    Some((rate.max(1000), ch))
}

pub fn parse_probe(stderr: &str) -> Option<VideoInfo> {
    let duration = parse_duration(stderr)?;
    let (width, height, fps) = parse_video_line(stderr).unwrap_or((0, 0, 30.0));
    Some(VideoInfo {
        duration,
        width,
        height,
        fps,
    })
}

fn probe(exe: &Path, path: &Path) -> Result<(VideoInfo, ColorInfo, Option<(u32, usize)>), String> {
    let out = crate::ffmpeg::command(exe)
        .arg("-hide_banner")
        .arg("-i")
        .arg(path)
        .stdout(Stdio::null())
        .output()
        .map_err(|e| format!("Could not start the decoder: {e}"))?;
    let text = String::from_utf8_lossy(&out.stderr);
    let info = parse_probe(&text)
        .ok_or("The file has no readable video stream, or the decoder could not open it.")?;
    Ok((info, parse_color(&text), parse_audio_line(&text)))
}

/// How many thumbnails to cache and how wide they are. A bounded cache keeps long clips cheap.
fn budget(quality: u8) -> (usize, u32) {
    match quality.min(2) {
        0 => (240, 192),
        1 => (600, 256),
        _ => (1200, 384),
    }
}

/// The Preview resolution choices (Preferences > Video).
pub const RES_NAMES: [&str; 4] = ["Fit to panel", "720p", "1080p", "Native (full resolution)"];
static PREVIEW_RES: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn set_preview_res(v: u8) {
    PREVIEW_RES.store(v.min(3), Ordering::Relaxed);
}

pub fn preview_res() -> u8 {
    PREVIEW_RES.load(Ordering::Relaxed)
}

/// The decode width for the chosen Preview resolution; `fit` is the panel-sized width.
/// Never wider than the source.
pub fn res_width(info: &VideoInfo, fit: u32) -> u32 {
    let src = info.width.max(16);
    match preview_res() {
        0 => fit,
        1 => 1280,
        2 => 1920,
        _ => src,
    }
    .min(src)
}

/// Output size for a target width, keeping the aspect ratio and even dimensions.
pub fn scaled_size(info: &VideoInfo, width: u32) -> (u32, u32) {
    let w = (width.max(16) / 2) * 2;
    let h = if info.width > 0 && info.height > 0 {
        (w as f64 * info.height as f64 / info.width as f64 / 2.0).round() as u32 * 2
    } else {
        w * 9 / 16
    };
    (w, h.max(2))
}

fn rate_string(rate: f64) -> String {
    if (rate - rate.round()).abs() < 0.0001 {
        format!("{}", rate.round() as u32)
    } else {
        format!("{rate:.4}")
    }
}

/// The Rec.709 conversion chain for wide-gamut / HDR preview frames.
fn color_filter(convert: bool) -> &'static str {
    if convert {
        "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,"
    } else {
        ""
    }
}

/// Read raw RGBA frames of `w`×`h` from a running decoder, calling `each` per frame.
fn read_frames(
    child: &mut std::process::Child,
    w: u32,
    h: u32,
    cancel: &AtomicBool,
    mut each: impl FnMut(Vec<u8>) -> bool,
) -> usize {
    let Some(mut out) = child.stdout.take() else {
        return 0;
    };
    let size = w as usize * h as usize * 4;
    let mut n = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            break;
        }
        let mut buf = vec![0u8; size];
        if out.read_exact(&mut buf).is_err() {
            break;
        }
        n += 1;
        if !each(buf) {
            let _ = child.kill();
            break;
        }
    }
    let _ = child.wait();
    n
}

/// Stream evenly spaced thumbnails. Long clips decode key frames only, which is many times
/// faster than decoding every picture and plenty for a scrubbing preview.
fn stream_frames(
    exe: &Path,
    path: &Path,
    info: &VideoInfo,
    opts: &ImportOpts,
    cancel: &AtomicBool,
    send: &dyn Fn(VideoMsg),
) -> Result<usize, String> {
    let (limit, width) = budget(opts.quality);
    let (w, h) = scaled_size(info, width);
    let rate =
        (limit as f64 / info.duration.max(0.5)).clamp(0.05f64, 8.0f64.min(info.fps.max(0.1f64)));
    let run = |convert: bool| -> Result<usize, String> {
        let mut cmd = crate::ffmpeg::command(exe);
        cmd.args(["-hide_banner", "-loglevel", "error", "-threads", "0"]);
        if info.duration > 90.0 {
            cmd.args(["-skip_frame", "nokey"]);
        }
        cmd.arg("-i").arg(path);
        let filter = format!(
            "{}fps={},scale={w}:{h}:flags=fast_bilinear,format=rgba",
            color_filter(convert),
            rate_string(rate)
        );
        cmd.args([
            "-an", "-sn", "-dn", "-vf", &filter, "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1",
        ]);
        cmd.stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Could not start the decoder: {e}"))?;
        let mut batch: Vec<VideoFrame> = Vec::new();
        let mut last_send = std::time::Instant::now();
        let mut i = 0usize;
        let n = read_frames(&mut child, w, h, cancel, |rgba| {
            batch.push(VideoFrame {
                time: i as f64 / rate,
                width: w,
                height: h,
                rgba,
            });
            i += 1;
            // the first picture goes out at once; later ones in small batches
            if i == 1 || batch.len() >= 12 || last_send.elapsed().as_millis() > 150 {
                send(VideoMsg::Frames(std::mem::take(&mut batch)));
                last_send = std::time::Instant::now();
            }
            i < limit
        });
        if !batch.is_empty() {
            send(VideoMsg::Frames(batch));
        }
        Ok(n)
    };
    let n = run(opts.convert_color)?;
    if n == 0 && opts.convert_color && !cancel.load(Ordering::Relaxed) {
        // this decoder build lacks the tone-mapping filters: show the frames unconverted
        return run(false);
    }
    Ok(n)
}

/// Decode the sound straight from the decoder's output pipe as 32-bit float. Long clips are
/// split into a few parts decoded at the same time (audio decoders use one core each).
fn stream_audio(
    exe: &Path,
    path: &Path,
    rate: u32,
    channels: usize,
    duration: f64,
    cancel: &AtomicBool,
) -> Option<AudioData> {
    let parts = if duration > 120.0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            .clamp(1, 6)
    } else {
        1
    };
    if parts == 1 {
        return stream_audio_part(exe, path, rate, channels, 0.0, None, cancel)
            .map(|ch| AudioData { rate, ch });
    }
    let seg = duration / parts as f64;
    let pieces: Vec<Option<Vec<Vec<f32>>>> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..parts)
            .map(|i| {
                let len = if i + 1 == parts { None } else { Some(seg) };
                s.spawn(move || {
                    stream_audio_part(exe, path, rate, channels, seg * i as f64, len, cancel)
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().ok().flatten()).collect()
    });
    let mut ch: Vec<Vec<f32>> = (0..channels)
        .map(|_| Vec::with_capacity((duration * rate as f64) as usize + rate as usize))
        .collect();
    for (i, p) in pieces.into_iter().enumerate() {
        let p = p?;
        // every part but the last is cut to its exact length so the joins stay sample-accurate
        let want =
            ((seg * (i + 1) as f64 * rate as f64).round() as usize).saturating_sub(ch[0].len());
        for (c, v) in p.into_iter().enumerate().take(channels) {
            let n = if i + 1 == parts {
                v.len()
            } else {
                v.len().min(want)
            };
            ch[c].extend_from_slice(&v[..n]);
            if i + 1 < parts && n < want {
                let l = ch[c].len();
                ch[c].resize(l + want - n, 0.0);
            }
        }
    }
    let frames = ch.iter().map(|v| v.len()).min().unwrap_or(0);
    if frames == 0 {
        return None;
    }
    for v in &mut ch {
        v.truncate(frames);
    }
    Some(AudioData { rate, ch })
}

fn stream_audio_part(
    exe: &Path,
    path: &Path,
    rate: u32,
    channels: usize,
    start: f64,
    len: Option<f64>,
    cancel: &AtomicBool,
) -> Option<Vec<Vec<f32>>> {
    let channels = channels.max(1);
    let mut cmd = crate::ffmpeg::command(exe);
    cmd.args(["-hide_banner", "-loglevel", "error"]);
    if start > 0.0 {
        cmd.args(["-ss", &format!("{start:.6}")]);
    }
    if let Some(l) = len {
        cmd.args(["-t", &format!("{:.6}", l + 0.05)]);
    }
    let mut child = cmd
        .arg("-i")
        .arg(path)
        .args([
            "-vn",
            "-sn",
            "-dn",
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-ac",
            &channels.to_string(),
            "-ar",
            &rate.to_string(),
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let mut bytes: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        match out.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
        }
    }
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    let frame = 4 * channels;
    let frames = bytes.len() / frame;
    if !ok && frames == 0 {
        return None;
    }
    let mut ch: Vec<Vec<f32>> = (0..channels).map(|_| Vec::with_capacity(frames)).collect();
    for f in bytes.chunks_exact(frame) {
        for (c, v) in ch.iter_mut().enumerate() {
            let o = c * 4;
            v.push(f32::from_le_bytes([f[o], f[o + 1], f[o + 2], f[o + 3]]));
        }
    }
    Some(ch)
}

/// Pad audio to the length of the picture and build its peak pyramid here, on the worker.
pub fn prepare_timeline_audio(
    data: AudioData,
    bits: u16,
    float: bool,
    duration: f64,
) -> PreparedImport {
    let mut data = data;
    let target = (duration.max(0.0) * data.rate as f64).round() as usize;
    for c in &mut data.ch {
        if c.len() < target {
            c.resize(target, 0.0);
        }
    }
    let peaks = Peaks::build(&data);
    PreparedImport {
        data,
        bits,
        float,
        peaks,
    }
}

/// The whole import as a stream of messages: Info first, then Audio and Frames as they are
/// ready (the two decode in parallel), then Done.
pub fn import_stream(
    path: &Path,
    exe: &Path,
    opts: &ImportOpts,
    cancel: &AtomicBool,
    send: &(dyn Fn(VideoMsg) + Sync),
) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Video".into());
    let (info, color, audio_fmt) = match probe(exe, path) {
        Ok(x) => x,
        Err(e) => {
            send(VideoMsg::Done(Err(format!("{name}: {e}"))));
            return;
        }
    };
    send(VideoMsg::Info(info.clone(), color));
    let frames = std::thread::scope(|scope| {
        if !opts.frames_only {
            scope.spawn(|| {
                let decoded = audio_fmt
                    .and_then(|(rate, ch)| stream_audio(exe, path, rate, ch, info.duration, cancel))
                    .map(|d| (d, 32u16, true));
                // the app's own readers handle the odd file the decoder pipe could not
                let decoded =
                    decoded.or_else(|| audio::load(path).ok().map(|l| (l.data, l.bits, l.float)));
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let has = decoded.is_some();
                let prepared = match decoded {
                    Some((d, bits, float)) => prepare_timeline_audio(d, bits, float, info.duration),
                    None => {
                        prepare_timeline_audio(silent_track(info.duration), 32, true, info.duration)
                    }
                };
                send(VideoMsg::Audio(prepared, has));
            });
        }
        stream_frames(exe, path, &info, opts, cancel, send)
    });
    send(VideoMsg::Done(match frames {
        Ok(0) => Err(format!(
            "{name}: no picture could be decoded (the sound was still imported)"
        )),
        Ok(_) => Ok(()),
        Err(e) => Err(format!("{name}: {e}")),
    }));
}

/// Decode a whole import synchronously (tests).
#[cfg(test)]
pub fn import_file(
    path: &Path,
    prefs: &Prefs,
) -> Result<(VideoMedia, Option<(PreparedImport, bool)>), String> {
    let exe = ensure_decoder(prefs)?;
    let got: Mutex<(
        Option<VideoMedia>,
        Option<(PreparedImport, bool)>,
        Option<Result<(), String>>,
    )> = Mutex::new((None, None, None));
    let send = |msg: VideoMsg| {
        let Ok(mut g) = got.lock() else { return };
        match msg {
            VideoMsg::Info(info, color) => {
                let mut m = VideoMedia::new(path.to_path_buf(), info);
                m.color = color;
                g.0 = Some(m);
            }
            VideoMsg::Audio(a, has) => g.1 = Some((a, has)),
            VideoMsg::Frames(f) => {
                if let Some(m) = &mut g.0 {
                    m.frames.extend(f);
                }
            }
            VideoMsg::Done(r) => g.2 = Some(r),
        }
    };
    let opts = ImportOpts {
        quality: prefs.video_quality,
        ..Default::default()
    };
    import_stream(path, &exe, &opts, &AtomicBool::new(false), &send);
    let (media, audio, done) = got.into_inner().map_err(|_| "import failed".to_string())?;
    match (media, done) {
        (Some(m), Some(Ok(()))) => Ok((m, audio)),
        (_, Some(Err(e))) => Err(e),
        _ => Err("import failed".into()),
    }
}

/// A silent, low-rate track used only when a video has no audio. It gives the transport a
/// timebase without allocating an hour of 48 kHz samples.
pub fn silent_track(duration: f64) -> AudioData {
    let frames = (duration.max(0.0) * 1000.0).round() as usize;
    AudioData::new(1000, 1, frames)
}

/// Decodes the exact frame under a stopped playhead at a sharper size, newest request first.
pub struct FrameSeeker {
    tx: Sender<(PathBuf, PathBuf, f64, VideoInfo, bool)>,
    latest: Arc<Mutex<Option<(PathBuf, VideoFrame)>>>,
    pub asked: Option<(PathBuf, f64)>,
}

impl FrameSeeker {
    pub fn new(ctx: Option<egui::Context>) -> Self {
        let (tx, rx) = mpsc::channel::<(PathBuf, PathBuf, f64, VideoInfo, bool)>();
        let latest: Arc<Mutex<Option<(PathBuf, VideoFrame)>>> = Arc::new(Mutex::new(None));
        let out = latest.clone();
        std::thread::spawn(move || {
            while let Ok(mut job) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    job = newer;
                }
                let (exe, path, t, info, convert) = job;
                if let Some(f) = decode_one(&exe, &path, t, &info, convert) {
                    if let Ok(mut l) = out.lock() {
                        *l = Some((path, f));
                    }
                    if let Some(c) = &ctx {
                        c.request_repaint();
                    }
                }
            }
        });
        Self {
            tx,
            latest,
            asked: None,
        }
    }

    pub fn request(&mut self, exe: PathBuf, path: &Path, t: f64, info: &VideoInfo, convert: bool) {
        let t = (t * info.fps.max(1.0)).floor() / info.fps.max(1.0);
        if self
            .asked
            .as_ref()
            .map(|(p, at)| p == path && (at - t).abs() < 1e-6)
            .unwrap_or(false)
        {
            return;
        }
        self.asked = Some((path.to_path_buf(), t));
        let _ = self
            .tx
            .send((exe, path.to_path_buf(), t, info.clone(), convert));
    }

    /// Whether `t` needs a new exact frame (it has not been asked for yet).
    pub fn wants(&self, path: &Path, t: f64, fps: f64) -> bool {
        let t = (t * fps.max(1.0)).floor() / fps.max(1.0);
        !self
            .asked
            .as_ref()
            .map(|(p, at)| p == path && (at - t).abs() < 1e-6)
            .unwrap_or(false)
    }

    /// The time of the sharp frame ready for this clip near `t`, without copying it.
    pub fn exact_time(&self, path: &Path, t: f64, fps: f64) -> Option<f64> {
        let l = self.latest.lock().ok()?;
        let (p, f) = l.as_ref()?;
        (p == path && (f.time - t).abs() <= 1.0 / fps.max(1.0) + 1e-6).then_some(f.time)
    }

    /// The sharp frame if it is for this clip and within one frame of `t`.
    pub fn frame_for(&self, path: &Path, t: f64, fps: f64) -> Option<VideoFrame> {
        let l = self.latest.lock().ok()?;
        let (p, f) = l.as_ref()?;
        (p == path && (f.time - t).abs() <= 1.0 / fps.max(1.0) + 1e-6).then(|| f.clone())
    }
}

/// Playback frames: while the transport runs, one decoder streams every picture from the
/// playhead on, at the clip's own frame rate (up to 60) and a sharp size, a few frames ahead.
/// The interface takes the frame due at the playhead; a jump restarts the stream there.
pub struct PlayStream {
    path: PathBuf,
    start: f64,
    rx: Receiver<VideoFrame>,
    cancel: Arc<AtomicBool>,
    pending: Option<VideoFrame>,
    /// The frame showing now.
    pub current: Option<VideoFrame>,
    /// Counts frames taken, so the UI knows when to upload a new texture.
    pub serial: usize,
    pub fps: f64,
    pub width: u32,
    born: std::time::Instant,
    /// How far ahead of the playhead the stream was started.
    pub lead: f64,
    /// Seconds from starting the decoder to its first picture (seek + warm-up).
    pub startup: Option<f64>,
    /// When pictures began to flow.
    flowing_since: Option<std::time::Instant>,
}

/// Set once the graphics-card decoder failed on this computer, so later streams skip it.
static HW_DECODE_FAILED: AtomicBool = AtomicBool::new(false);

/// Decoder arguments that hand decoding to the graphics card (Windows, Direct3D 11). AV1 needs
/// FFmpeg's own decoder named, since the default software one ignores the hardware.
fn hw_args(codec: &str) -> Vec<&'static str> {
    if !cfg!(windows) || HW_DECODE_FAILED.load(Ordering::Relaxed) {
        return vec![];
    }
    let mut a = vec!["-hwaccel", "d3d11va"];
    match codec {
        "av1" => a.extend(["-c:v", "av1"]),
        "h264" | "hevc" | "vp9" | "mpeg2video" | "vc1" => {}
        _ => return vec![],
    }
    a
}

impl Drop for PlayStream {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl PlayStream {
    #[cfg(test)]
    pub fn start(
        exe: PathBuf,
        path: &Path,
        t: f64,
        info: &VideoInfo,
        convert: bool,
        ctx: Option<egui::Context>,
    ) -> PlayStream {
        Self::start_sized(exe, path, t, info, convert, 1280, info.fps, ctx)
    }

    /// Start with a frame width (panel-sized) and frame rate (lowered when the computer can't keep up).
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn start_sized(
        exe: PathBuf,
        path: &Path,
        t: f64,
        info: &VideoInfo,
        convert: bool,
        width: u32,
        fps: f64,
        ctx: Option<egui::Context>,
    ) -> PlayStream {
        Self::launch(exe, path, t, info, convert, width, fps, "", ctx)
    }

    /// Start decoding at `t` (already including any lead for the decoder's warm-up), on the
    /// graphics card when the codec allows it, falling back to the software decoder.
    #[allow(clippy::too_many_arguments)]
    pub fn launch(
        exe: PathBuf,
        path: &Path,
        t: f64,
        info: &VideoInfo,
        convert: bool,
        width: u32,
        fps: f64,
        codec: &str,
        ctx: Option<egui::Context>,
    ) -> PlayStream {
        let (tx, rx) = mpsc::sync_channel::<VideoFrame>(6);
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        let p2 = path.to_path_buf();
        let fps = fps.min(info.fps).clamp(1.0, 60.0);
        let (w, h) = scaled_size(info, width.clamp(160, 8192).min(info.width.max(16)));
        let start = t.max(0.0);
        let hw = hw_args(codec);
        std::thread::spawn(move || {
            let filter = format!(
                "{}fps={},scale={w}:{h}:flags=bilinear,format=rgba",
                color_filter(convert),
                rate_string(fps)
            );
            let mut i = 0usize;
            let mut run = |hw: &[&str]| -> usize {
                let mut cmd = crate::ffmpeg::command(&exe);
                cmd.args(["-hide_banner", "-loglevel", "error"]);
                if hw.is_empty() {
                    cmd.args(["-threads", "0"]);
                } else {
                    cmd.args(hw);
                }
                cmd.args(["-ss", &format!("{start:.3}"), "-i"]).arg(&p2);
                cmd.args([
                    "-an", "-sn", "-dn", "-vf", &filter, "-f", "rawvideo", "-pix_fmt", "rgba",
                    "pipe:1",
                ]);
                let Ok(mut child) = cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
                    return 0;
                };
                read_frames(&mut child, w, h, &c2, |rgba| {
                    let f = VideoFrame {
                        time: start + i as f64 / fps,
                        width: w,
                        height: h,
                        rgba,
                    };
                    i += 1;
                    // blocks while the interface is a few frames behind: the decoder never runs away
                    let ok = tx.send(f).is_ok();
                    if let Some(c) = &ctx {
                        c.request_repaint();
                    }
                    ok && !c2.load(Ordering::Relaxed)
                })
            };
            if !hw.is_empty() {
                if run(&hw) > 0 || c2.load(Ordering::Relaxed) {
                    return;
                }
                // the graphics card could not decode this: software from now on
                HW_DECODE_FAILED.store(true, Ordering::Relaxed);
            }
            run(&[]);
        });
        PlayStream {
            path: path.to_path_buf(),
            start,
            rx,
            cancel,
            pending: None,
            current: None,
            serial: 0,
            fps,
            width: w,
            born: std::time::Instant::now(),
            lead: 0.0,
            startup: None,
            flowing_since: None,
        }
    }

    /// Seconds pictures have been flowing (0 while the decoder still warms up).
    pub fn flowing_for(&self) -> f64 {
        self.flowing_since
            .map(|s| s.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }

    pub fn path_is(&self, p: &Path) -> bool {
        self.path == p
    }

    /// How far the showing frame is behind the playhead (decoding slower than real time).
    pub fn behind(&self, t: f64) -> f64 {
        match &self.current {
            Some(c) => t - c.time,
            None => t - self.start,
        }
    }

    /// Does this stream serve `t` for this clip, or must it restart (a seek or another clip)?
    pub fn serves(&self, path: &Path, t: f64) -> bool {
        // a stream started ahead (to cover the decoder's warm-up) serves the time before it too
        if self.path != path || t + 0.05 + self.lead < self.start {
            return false;
        }
        match (&self.current, &self.pending) {
            (Some(c), _) => t >= c.time - 0.25 && t < c.time + 4.0,
            (None, Some(p)) => t < p.time + 4.0,
            (None, None) => t < self.start + 4.0,
        }
    }

    /// Advance to the frame due at `t`; true when the showing frame changed.
    pub fn advance(&mut self, t: f64) -> bool {
        let mut changed = false;
        loop {
            if self.pending.is_none() {
                match self.rx.try_recv() {
                    Ok(f) => {
                        if self.startup.is_none() {
                            self.startup = Some(self.born.elapsed().as_secs_f64());
                            self.flowing_since = Some(std::time::Instant::now());
                        }
                        self.pending = Some(f);
                    }
                    Err(_) => break,
                }
            }
            match &self.pending {
                Some(f) if f.time <= t + 0.008 => {
                    self.current = self.pending.take();
                    self.serial += 1;
                    changed = true;
                }
                _ => break,
            }
        }
        changed
    }
}

fn decode_one(
    exe: &Path,
    path: &Path,
    t: f64,
    info: &VideoInfo,
    convert: bool,
) -> Option<VideoFrame> {
    let (w, h) = scaled_size(info, res_width(info, 1280));
    let filter = format!("{}scale={w}:{h},format=rgba", color_filter(convert));
    let mut child = crate::ffmpeg::command(exe)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            &format!("{:.3}", t.max(0.0)),
            "-i",
        ])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-an",
            "-sn",
            "-vf",
            &filter,
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut frame = None;
    read_frames(&mut child, w, h, &AtomicBool::new(false), |rgba| {
        frame = Some(VideoFrame {
            time: t,
            width: w,
            height: h,
            rgba,
        });
        false
    });
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_extensions_are_recognised() {
        assert!(is_video_path(Path::new("clip.MP4")));
        assert!(is_video_path(Path::new("a/b/movie.webm")));
        assert!(!is_video_path(Path::new("song.wav")));
    }

    const PROBE: &str = "Input #0, mov,mp4 from 'clip.mp4':\n  Duration: 00:03:52.92, start: 0.000000, bitrate: 900 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1280x720 [SAR 1:1 DAR 16:9], 25 fps, 25 tbr\n  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, stereo, fltp, 128 kb/s\n";

    #[test]
    fn probe_text_is_parsed() {
        let info = parse_probe(PROBE).expect("probe");
        assert!((info.duration - 232.92).abs() < 0.01);
        assert_eq!((info.width, info.height), (1280, 720));
        assert!((info.fps - 25.0).abs() < 0.01);
        assert_eq!(parse_audio_line(PROBE), Some((44100, 2)));
        let c = parse_color(PROBE);
        assert_eq!(c.pix_fmt, "yuv420p");
        assert_eq!(c.codec, "h264");
        assert!(c.space.contains("bt709"));
        assert!(c.problem().is_none(), "Rec.709 8-bit needs no warning");
    }

    #[test]
    fn hdr_and_deep_colour_are_flagged() {
        let hdr = "  Duration: 00:00:10.00, start: 0\n  Stream #0:0: Video: hevc (Main 10), yuv420p10le(tv, bt2020nc/bt2020/smpte2084), 3840x2160, 60 fps\n";
        let c = parse_color(hdr);
        assert_eq!(c.pix_fmt, "yuv420p10le");
        let why = c.problem().expect("HDR must be flagged");
        assert!(
            why.contains("HDR") && why.contains("BT.2020") && why.contains("8 bits"),
            "{why}"
        );
        assert!(parse_audio_line(hdr).is_none());
    }

    #[test]
    fn scaled_frames_keep_the_aspect_and_even_sizes() {
        let info = VideoInfo {
            duration: 1.0,
            width: 1920,
            height: 1080,
            fps: 30.0,
        };
        assert_eq!(scaled_size(&info, 256), (256, 144));
        let odd = VideoInfo {
            width: 0,
            height: 0,
            ..info
        };
        assert_eq!(scaled_size(&odd, 255), (254, 142));
    }

    fn media(n: usize) -> VideoMedia {
        let mut m = VideoMedia::new(
            PathBuf::from("clip.mp4"),
            VideoInfo {
                duration: 2.0,
                width: 4,
                height: 4,
                fps: 10.0,
            },
        );
        m.frames = (0..n)
            .map(|i| VideoFrame {
                time: i as f64,
                width: 4,
                height: 4,
                rgba: vec![0; 64],
            })
            .collect();
        m
    }

    #[test]
    fn nearest_frame_is_bounded() {
        let m = media(4);
        assert_eq!(m.frame_at(-5.0).map(|(i, _)| i), Some(0));
        assert_eq!(m.frame_at(1.4).map(|(i, _)| i), Some(1));
        assert_eq!(m.frame_at(1.6).map(|(i, _)| i), Some(2));
        assert_eq!(m.frame_at(999.0).map(|(i, _)| i), Some(3));
        assert!(media(0).frame_at(0.0).is_none());
    }

    #[test]
    fn decoder_is_found_in_a_configured_folder() {
        let dir =
            std::env::temp_dir().join(format!("audiocraft-decoder-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let exe = dir.join(if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        });
        let _ = std::fs::write(&exe, b"test");
        let mut prefs = Prefs::default();
        prefs.video_decoder_folder = dir.to_string_lossy().to_string();
        assert_eq!(detect_decoder(&prefs).expect("configured decoder"), exe);
        assert_eq!(ensure_decoder(&prefs).expect("ensured"), exe);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_decoder_rejects_missing_configured_executable() {
        let mut prefs = Prefs::default();
        prefs.video_decoder_exe = std::env::temp_dir()
            .join("missing-ffmpeg-audiocraft.exe")
            .to_string_lossy()
            .to_string();
        prefs.video_decoder_folder.clear();
        prefs.video_decoder_repos.clear();
        let old_path = std::env::var_os("PATH");
        let old_bundled = std::env::var_os(crate::ffmpeg::DISABLE_ENV);
        let old_local = std::env::var_os("LOCALAPPDATA");
        unsafe {
            std::env::set_var("PATH", "");
            std::env::set_var(crate::ffmpeg::DISABLE_ENV, "1");
            std::env::set_var("LOCALAPPDATA", std::env::temp_dir().join("ac-empty-local"));
        }
        let result = ensure_decoder(&prefs);
        unsafe {
            match old_path {
                Some(v) => std::env::set_var("PATH", v),
                None => std::env::remove_var("PATH"),
            }
            match old_bundled {
                Some(v) => std::env::set_var(crate::ffmpeg::DISABLE_ENV, v),
                None => std::env::remove_var(crate::ffmpeg::DISABLE_ENV),
            }
            match old_local {
                Some(v) => std::env::set_var("LOCALAPPDATA", v),
                None => std::env::remove_var("LOCALAPPDATA"),
            }
        }
        let err = result.expect_err("a missing decoder must be refused");
        assert!(err.to_ascii_lowercase().contains("decoder"), "{err}");
    }

    #[test]
    fn extra_decoder_folders_are_searched() {
        let root = std::env::temp_dir().join(format!(
            "audiocraft-decoder-repos-test-{}",
            std::process::id()
        ));
        let nested = root.join("ffmpeg-nightly").join("bin");
        let _ = std::fs::create_dir_all(&nested);
        let exe = nested.join(if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        });
        let _ = std::fs::write(&exe, b"test");
        let mut prefs = Prefs::default();
        prefs.video_decoder_repos = root.to_string_lossy().to_string();
        assert!(decoder_folders(&prefs).contains(&root));
        assert_eq!(detect_decoder(&prefs).expect("repo decoder"), exe);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn timeline_audio_is_padded_and_prepared_before_it_reaches_the_ui() {
        let prepared = prepare_timeline_audio(AudioData::new(1000, 1, 100), 16, false, 0.25);
        assert_eq!(prepared.data.frames(), 250);
        assert!(!prepared.peaks.levels.is_empty());
    }

    #[test]
    fn a_failed_import_keeps_the_video_path_for_the_panel() {
        let state = VideoState::Error(VideoError {
            path: PathBuf::from("holiday.mp4"),
            message: "no decoder".into(),
        });
        assert_eq!(state.path(), Some(Path::new("holiday.mp4")));
    }

    #[test]
    fn bundled_decoder_imports_a_synthetic_video_end_to_end() {
        if !cfg!(windows) {
            return; // the Linux build uses a system decoder instead
        }
        let Some(exe) = crate::ffmpeg::bundled() else {
            return;
        };
        let dir =
            std::env::temp_dir().join(format!("audiocraft-ffmpeg-e2e-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let clip = dir.join("clip.mp4");
        let made = crate::ffmpeg::command(&exe)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x120:rate=10:duration=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:v",
                "mpeg4",
                "-q:v",
                "5",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
            ])
            .arg(&clip)
            .output()
            .expect("run bundled ffmpeg");
        if !made.status.success() {
            let _ = std::fs::remove_dir_all(&dir);
            panic!(
                "could not make a test clip: {}",
                String::from_utf8_lossy(&made.stderr)
            );
        }
        let mut prefs = Prefs::default();
        prefs.video_decoder_exe = exe.to_string_lossy().to_string();
        let started = std::time::Instant::now();
        let result = import_file(&clip, &prefs);
        let seek = decode_one(
            &exe,
            &clip,
            0.5,
            &VideoInfo {
                duration: 1.0,
                width: 160,
                height: 120,
                fps: 10.0,
            },
            false,
        );
        let _ = std::fs::remove_dir_all(&dir);
        let (media, audio) = result.expect("the bundled decoder must import a normal MP4");
        assert!(started.elapsed().as_secs() < 20);
        assert!((media.info.duration - 1.0).abs() < 0.2, "{:?}", media.info);
        assert_eq!((media.info.width, media.info.height), (160, 120));
        assert!(!media.frames.is_empty(), "a real frame should be decoded");
        assert!(
            media
                .frames
                .iter()
                .all(|f| f.rgba.len() == (f.width * f.height * 4) as usize)
        );
        let (audio, has) = audio.expect("audio message");
        assert!(has, "the sine track should be decoded");
        assert!(audio.data.frames() > 8000);
        assert!(
            seek.is_some(),
            "the exact-frame seeker must decode a picture"
        );
    }

    /// Timing check for a long clip: `AUDIOCRAFT_VIDEO_TIMING=clip.mp4 cargo test ... -- --ignored`.
    #[test]
    #[ignore]
    fn time_a_long_import() {
        let Some(p) = std::env::var_os("AUDIOCRAFT_VIDEO_TIMING") else {
            return;
        };
        let exe = crate::ffmpeg::bundled().expect("bundled decoder");
        let t0 = std::time::Instant::now();
        let first_frames = std::sync::Mutex::new(None);
        let send = |m: VideoMsg| {
            let ms = t0.elapsed().as_millis();
            match m {
                VideoMsg::Info(i, c) => eprintln!("{ms} ms: info {:?} {:?}", i, c),
                VideoMsg::Audio(a, has) => {
                    eprintln!("{ms} ms: audio {} frames (has {has})", a.data.frames())
                }
                VideoMsg::Frames(f) => {
                    let mut g = first_frames.lock().unwrap();
                    if g.is_none() {
                        *g = Some(ms);
                        eprintln!("{ms} ms: first {} frames", f.len());
                    }
                }
                VideoMsg::Done(r) => eprintln!("{ms} ms: done {:?}", r),
            }
        };
        import_stream(
            Path::new(&p),
            &exe,
            &ImportOpts {
                quality: 1,
                ..Default::default()
            },
            &AtomicBool::new(false),
            &send,
        );
        let info = VideoInfo {
            duration: 600.0,
            width: 1280,
            height: 720,
            fps: 25.0,
        };
        let t1 = std::time::Instant::now();
        let f = decode_one(&exe, Path::new(&p), 300.0, &info, false);
        eprintln!(
            "exact frame at 5:00 in {} ms ({})",
            t1.elapsed().as_millis(),
            f.is_some()
        );
        // playback throughput: take frames as fast as they come for two seconds
        let info = VideoInfo {
            duration: 1200.0,
            width: 1920,
            height: 1080,
            fps: 30.0,
        };
        let mut ps = PlayStream::start_sized(
            crate::ffmpeg::bundled().unwrap(),
            Path::new(&p),
            300.0,
            &info,
            false,
            1280,
            30.0,
            None,
        );
        let t2 = std::time::Instant::now();
        let mut n = 0;
        while t2.elapsed().as_secs_f64() < 2.0 {
            if ps.advance(1e9) {
                n += ps.serial - n;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        eprintln!(
            "playback stream: {:.1} frames per second decoded at {} wide",
            ps.serial as f64 / 2.0,
            ps.width
        );
    }

    #[test]
    fn playback_stream_delivers_every_frame_at_full_size() {
        if !cfg!(windows) {
            return;
        }
        let Some(exe) = crate::ffmpeg::bundled() else {
            return;
        };
        let dir =
            std::env::temp_dir().join(format!("audiocraft-playstream-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let clip = dir.join("clip.mp4");
        let made = crate::ffmpeg::command(&exe)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=25:duration=2",
                "-c:v",
                "mpeg4",
                "-q:v",
                "5",
            ])
            .arg(&clip)
            .output()
            .expect("run ffmpeg");
        assert!(made.status.success());
        let info = VideoInfo {
            duration: 2.0,
            width: 640,
            height: 360,
            fps: 25.0,
        };
        let mut ps = PlayStream::start(exe, &clip, 0.5, &info, false, None);
        assert!(
            ps.serves(&clip, 0.5)
                && !ps.serves(&clip, 0.0)
                && !ps.serves(Path::new("other.mp4"), 0.5)
        );
        let mut seen = 0;
        for step in 0..200 {
            let t = 0.5 + step as f64 * 0.005;
            if ps.advance(t) {
                seen += 1;
            }
            if t > 1.0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let cur = ps.current.clone().expect("a frame is showing");
        drop(ps);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            (cur.width, cur.height),
            (640, 360),
            "playback keeps the picture sharp"
        );
        assert!(
            seen >= 8,
            "about every frame of half a second at 25 fps: {seen}"
        );
        assert!(cur.time <= 1.01 && cur.time >= 0.9, "{}", cur.time);
    }

    #[test]
    fn frame_budget_shrinks_for_long_clips() {
        assert_eq!(budget(0).0, 240);
        assert_eq!(budget(9).0, 1200);
        let long = VideoInfo {
            duration: 3600.0,
            width: 1920,
            height: 1080,
            fps: 30.0,
        };
        let rate = (budget(1).0 as f64 / long.duration).clamp(0.05, 8.0);
        assert!((rate * long.duration) <= 600.0 + 1.0);
    }
}
