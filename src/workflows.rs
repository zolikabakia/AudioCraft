//! Small, testable building blocks for the editor's workflows: marker lists, clip and track
//! operations, clipped-peak repair, analysis reports and batch file naming.
//!
//! These functions never touch the UI or the file system (except through plain paths passed in
//! by the caller), so the rules can be tested without opening a window.

use crate::audio::AudioData;
use crate::session::{Clip, Marker, Track};

/// Insert (or replace) a volume-automation point, keeping the list sorted by time.
/// Returns the index of the point so the UI can start dragging it immediately.
pub fn insert_auto_point(env: &mut Vec<(f64, f32)>, t: f64, db: f32) -> usize {
    let t = t.max(0.0);
    if let Some(i) = env.iter().position(|(pt, _)| (*pt - t).abs() < 0.01) {
        env[i].1 = db;
        return i;
    }
    let i = env.iter().position(|(pt, _)| *pt > t).unwrap_or(env.len());
    env.insert(i, (t, db));
    i
}

/// Move an automation point without letting it cross its neighbours.
pub fn move_auto_point(env: &mut [(f64, f32)], idx: usize, t: f64, db: f32) {
    if idx >= env.len() {
        return;
    }
    let lo = if idx > 0 { env[idx - 1].0 + 0.001 } else { 0.0 };
    let hi = if idx + 1 < env.len() {
        env[idx + 1].0 - 0.001
    } else {
        f64::INFINITY
    };
    env[idx].0 = t.max(lo).min(hi);
    env[idx].1 = db;
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Markers as a small CSV file other editors can read: `Name,Start (s),Duration (s)`.
pub fn markers_to_csv(markers: &[Marker]) -> String {
    let mut out = String::from("Name,Start,Duration\n");
    for m in markers {
        out.push_str(&format!(
            "{},{:.6},{:.6}\n",
            csv_field(&m.name),
            m.time,
            m.duration
        ));
    }
    out
}

/// Read a markers CSV (the format written by [`markers_to_csv`], with or without a header).
/// Bad rows are skipped instead of failing the whole import.
pub fn markers_from_csv(s: &str) -> Vec<Marker> {
    let mut out = Vec::new();
    for line in s.lines() {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() || line.starts_with("Name,") {
            continue;
        }
        let fields = split_csv(line);
        if fields.len() < 2 {
            continue;
        }
        let Some(time) = fields[1].trim().parse::<f64>().ok() else {
            continue;
        };
        if !time.is_finite() || time < 0.0 {
            continue;
        }
        let duration = fields
            .get(2)
            .and_then(|v| v.trim().parse::<f64>().ok())
            .unwrap_or(0.0);
        out.push(Marker {
            name: fields[0].trim().trim_matches('"').to_string(),
            time,
            duration: duration.max(0.0),
        });
    }
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    out
}

fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            if quoted && chars.peek() == Some(&'"') {
                cur.push('"');
                chars.next();
            } else {
                quoted = !quoted;
            }
        } else if c == ',' && !quoted {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

/// Nearest marker to a time, with its index. Returns `None` for an empty list.
pub fn nearest_marker(markers: &[Marker], t: f64) -> Option<(usize, &Marker)> {
    markers
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (a.time - t).abs().total_cmp(&(b.time - t).abs()))
}

/// The marker after `t` (wrapping to the first one at the end).
pub fn next_marker(markers: &[Marker], t: f64) -> Option<(usize, &Marker)> {
    if markers.is_empty() {
        return None;
    }
    markers
        .iter()
        .enumerate()
        .find(|(_, m)| m.time > t + 1e-6)
        .or_else(|| markers.first().map(|m| (0, m)))
}

/// The marker before `t` (wrapping to the last one at the start).
pub fn previous_marker(markers: &[Marker], t: f64) -> Option<(usize, &Marker)> {
    if markers.is_empty() {
        return None;
    }
    markers
        .iter()
        .enumerate()
        .rev()
        .find(|(_, m)| m.time < t - 1e-6)
        .or_else(|| markers.last().map(|m| (markers.len() - 1, m)))
}

/// Rebuild flat-topped peaks from the audio around a clipped run. Only runs of samples at or
/// beyond `threshold` are replaced; silence at full scale is left alone when the neighbours are
/// also clipped, so a genuinely square wave is not smoothed away.
pub fn declip(d: &mut AudioData, a: usize, b: usize, threshold: f32) -> usize {
    let n = d.frames().min(b);
    if a >= n || threshold <= 0.0 {
        return 0;
    }
    let mut repaired = 0usize;
    // a broken sample (NaN) is neither clipped nor a usable neighbour; it must still be stepped
    // over, or the scan never moves on
    let clipped = |x: f32| x.is_finite() && x.abs() >= threshold;
    for ch in d.ch.iter_mut() {
        let mut i = a;
        while i < n {
            if !clipped(ch[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < n && clipped(ch[i]) {
                i += 1;
            }
            let end = i; // one past the last clipped sample
            if start == 0 || end >= n {
                continue;
            }
            let left = ch[start - 1];
            let right = ch[end];
            if !left.is_finite()
                || !right.is_finite()
                || left.abs() >= threshold
                || right.abs() >= threshold
            {
                continue;
            }
            for k in start..end {
                let t = (k - start + 1) as f32 / (end - start + 1) as f32;
                ch[k] = left * (1.0 - t) + right * t;
                repaired += 1;
            }
        }
    }
    repaired
}

/// Level numbers for the Export Analysis Report workflow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Report {
    pub peak_db: f32,
    pub rms_db: f32,
    pub lufs: f32,
}

fn db(v: f64) -> f32 {
    if v <= 1e-8 {
        -200.0
    } else {
        (20.0 * v.log10()) as f32
    }
}

pub fn analyze(d: &AudioData, a: usize, b: usize) -> Report {
    let end = b.min(d.frames());
    let mut sum = 0.0f64;
    let mut peak = 0.0f64;
    let mut count = 0usize;
    for ch in &d.ch {
        for s in ch.iter().take(end).skip(a.min(end)) {
            let v = *s as f64;
            sum += v * v;
            peak = peak.max(v.abs());
            count += 1;
        }
    }
    if count == 0 {
        return Report {
            peak_db: -200.0,
            rms_db: -200.0,
            lufs: -200.0,
        };
    }
    let rms = (sum / count as f64).sqrt();
    // A simple K-weighted loudness estimate: enough for a report, not a loudness master.
    let lufs = db(rms) - 0.69;
    Report {
        peak_db: db(peak),
        rms_db: db(rms),
        lufs,
    }
}

pub fn report_csv(name: &str, r: Report, seconds: f64) -> String {
    format!(
        "Name,Duration (s),Peak (dBFS),RMS (dBFS),Loudness (LUFS estimate)\n{},{:.3},{:.2},{:.2},{:.2}\n",
        csv_field(name),
        seconds,
        r.peak_db,
        r.rms_db,
        r.lufs
    )
}

/// A copy of a clip placed at `start`, with a fresh id and no selection state attached.
pub fn duplicate_clip(src: &Clip, new_id: u64, start: f64) -> Clip {
    let mut c = src.clone();
    c.id = new_id;
    c.start = start;
    c
}

/// Turn the overlap between two clips on one track into a balanced crossfade.
/// Returns the overlap in seconds, or `None` when the clips do not overlap.
pub fn crossfade_pair(left: &mut Clip, right: &mut Clip, max_sec: f64) -> Option<f64> {
    let overlap = (left.end() - right.start).min(max_sec.max(0.0));
    if overlap <= 0.001 {
        return None;
    }
    left.fade_out = overlap;
    right.fade_in = overlap;
    left.auto_out = false;
    right.auto_in = false;
    Some(overlap)
}

pub fn fade_clip(c: &mut Clip, fade_in: bool, secs: f64) -> f64 {
    let len = c.len.max(0.0).min(secs.max(0.0));
    if fade_in {
        c.fade_in = len;
        c.auto_in = false;
    } else {
        c.fade_out = len;
        c.auto_out = false;
    }
    len
}

/// A copy of a whole track, ready to append to a session. Clip ids are assigned from `next_id`.
pub fn duplicate_track(src: &Track, name: String, track_id: u64, next_id: &mut u64) -> Track {
    let mut t = src.clone();
    t.id = track_id;
    t.name = name;
    t.clips = src
        .clips
        .iter()
        .map(|c| {
            *next_id = next_id.saturating_add(1);
            duplicate_clip(c, *next_id, c.start)
        })
        .collect();
    t
}

/// Output path for one file in a batch: `stem + suffix + .wav` in the chosen folder.
pub fn batch_output(
    input: &std::path::Path,
    out_dir: &std::path::Path,
    suffix: &str,
) -> std::path::PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "audio".into());
    out_dir.join(format!("{stem}{suffix}.wav"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(name: &str, time: f64, duration: f64) -> Marker {
        Marker {
            name: name.into(),
            time,
            duration,
        }
    }

    #[test]
    fn marker_csv_roundtrip_and_navigation() {
        let ms = vec![marker("Intro, quiet", 0.0, 2.0), marker("Verse", 12.5, 0.0)];
        let csv = markers_to_csv(&ms);
        assert!(csv.contains("\"Intro, quiet\""));
        let back = markers_from_csv(&csv);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].name, "Intro, quiet");
        assert!((back[1].time - 12.5).abs() < 1e-6);
        assert_eq!(nearest_marker(&back, 11.0).map(|(i, _)| i), Some(1));
        assert_eq!(next_marker(&back, 12.5).map(|(i, _)| i), Some(0));
        assert_eq!(previous_marker(&back, 0.0).map(|(i, _)| i), Some(1));
        assert!(markers_from_csv("bad,nope\n").is_empty());
    }

    #[test]
    fn declip_rebuilds_a_flat_top() {
        let mut d = AudioData {
            rate: 8000,
            ch: vec![vec![0.2; 40]],
        };
        for i in 10..=20 {
            d.ch[0][i] = 1.0;
        }
        d.ch[0][9] = 0.1;
        d.ch[0][21] = 0.6;
        let n = declip(&mut d, 0, 40, 0.999);
        assert_eq!(n, 11);
        assert!(d.ch[0][10] > 0.1 && d.ch[0][10] < 0.2);
        assert!(d.ch[0][15] > 0.3 && d.ch[0][15] < 0.4);
        // A real square wave (both neighbours clipped) is not touched.
        let mut sq = AudioData {
            rate: 8000,
            ch: vec![vec![1.0; 40]],
        };
        assert_eq!(declip(&mut sq, 0, 40, 0.999), 0);
    }

    #[test]
    fn analysis_report_ignores_silence_and_reports_levels() {
        let d = AudioData {
            rate: 8000,
            ch: vec![vec![0.0; 8000]],
        };
        assert_eq!(analyze(&d, 0, d.frames()).peak_db, -200.0);
        let mut d = AudioData {
            rate: 8000,
            ch: vec![vec![0.5; 8000]],
        };
        d.ch[0][100] = 1.0;
        let r = analyze(&d, 0, d.frames());
        assert!(r.peak_db.abs() < 0.01);
        assert!((r.rms_db - -6.02).abs() < 0.1);
        assert!(report_csv("Take 1", r, 1.0).contains("Take 1"));
    }

    #[test]
    fn clip_and_track_workflows() {
        let mut a = Clip {
            id: 1,
            file_id: 9,
            name: "a".into(),
            start: 0.0,
            offset: 0.0,
            len: 10.0,
            gain_db: 0.0,
            muted: false,
            fade_in: 0.0,
            fade_out: 0.0,
            env: vec![],
            pan_env: vec![],
            rack: vec![],
            speed: 1.0,
            fx: Default::default(),
            auto_in: true,
            auto_out: true,
            curves: Default::default(),
        };
        let mut b = a.clone();
        b.id = 2;
        b.start = 9.0;
        assert!((crossfade_pair(&mut a, &mut b, 2.0).unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(a.fade_out, 1.0);
        assert_eq!(b.fade_in, 1.0);
        assert!(!a.auto_out && !b.auto_in);
        assert_eq!(fade_clip(&mut b, false, 100.0), 10.0);
        assert_eq!(b.fade_out, 10.0);
        let dup = duplicate_clip(&a, 7, 20.0);
        assert_eq!((dup.id, dup.start), (7, 20.0));
        let track = Track {
            id: 3,
            name: "Track 1".into(),
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            arm: false,
            stereo: true,
            clips: vec![a],
            rack: vec![],
            fx: Default::default(),
            prerender: false,
            color: [1, 2, 3],
            height: 96.0,
            io: Default::default(),
            minimized: false,
            send: Default::default(),
            eq_on: false,
            eq: vec![],
            eq_master: 0.0,
            hidden: false,
            vol_env: vec![],
            show_vol_env: false,
            auto: Default::default(),
            bus: false,
        };
        let mut next = 50;
        let copy = duplicate_track(&track, "Track 1 copy".into(), 4, &mut next);
        assert_eq!(copy.id, 4);
        assert_eq!(copy.clips[0].id, 51);
        assert_eq!(next, 51);
    }

    #[test]
    fn batch_output_names_stay_in_the_chosen_folder() {
        let out = batch_output(
            std::path::Path::new("C:\\audio\\take 1.wav"),
            std::path::Path::new("C:\\out"),
            "_fixed",
        );
        assert_eq!(out, std::path::Path::new("C:\\out\\take 1_fixed.wav"));
    }

    #[test]
    fn automation_points_stay_sorted_and_cannot_cross() {
        let mut env = vec![(0.0, 0.0), (4.0, -6.0)];
        let i = insert_auto_point(&mut env, 2.0, -3.0);
        assert_eq!(i, 1);
        assert_eq!(
            env.iter().map(|p| p.0).collect::<Vec<_>>(),
            vec![0.0, 2.0, 4.0]
        );
        // adding at the same place replaces the earlier point instead of stacking duplicates
        let j = insert_auto_point(&mut env, 2.005, -9.0);
        assert_eq!(j, 1);
        assert_eq!(env.len(), 3);
        assert!((env[1].1 + 9.0).abs() < 1e-6);
        move_auto_point(&mut env, 1, 100.0, -2.0);
        assert!(env[1].0 < env[2].0);
        move_auto_point(&mut env, 1, -5.0, -2.0);
        assert!(env[1].0 >= 0.0);
        move_auto_point(&mut env, 99, 1.0, 0.0); // out-of-range indices are ignored
        assert_eq!(env.len(), 3);
    }
}
