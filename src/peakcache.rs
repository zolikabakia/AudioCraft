//! The media cache (Preferences > Media & Disk Cache): waveform peaks of opened files are saved
//! in the temporary folder, so opening the same file again skips working them out. A cache
//! file belongs to one file as it was (path, size and modification time); the oldest cache
//! files go first when the cache passes its size limit.

use crate::audio::{AudioData, BLOCKS, Peaks};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 6] = b"ACPK01";

/// Where the peak files live.
pub fn cache_dir() -> PathBuf {
    crate::prefs::temp_dir().join("AudioCraft Media Cache")
}

/// The cache file for `path` as it is now (`None` when the file can't be read).
fn cache_file(path: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let full = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    full.to_string_lossy().hash(&mut h);
    meta.len().hash(&mut h);
    mtime.hash(&mut h);
    Some(cache_dir().join(format!("{:016x}.acpk", h.finish())))
}

/// The saved peaks of `path`, if they exist and still fit its audio.
pub fn load(path: &Path, data: &AudioData) -> Option<Peaks> {
    let bytes = std::fs::read(cache_file(path)?).ok()?;
    let mut r = Reader { b: &bytes, at: 0 };
    if r.take(MAGIC.len())? != MAGIC {
        return None;
    }
    let frames = r.u64()? as usize;
    let chans = r.u32()? as usize;
    let nlev = r.u32()? as usize;
    if frames != data.frames() || chans != data.channels() || nlev != BLOCKS.len() {
        return None;
    }
    let mut levels = Vec::with_capacity(nlev);
    for (l, block) in BLOCKS.iter().enumerate() {
        let mut chs = Vec::with_capacity(chans);
        for _ in 0..chans {
            let n = r.u64()? as usize;
            // the size a level of this file must have (guards against a damaged file)
            let want = if l == 0 { frames.div_ceil(*block) } else { n };
            if n != want || n > frames + 1 {
                return None;
            }
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push((r.f32()?, r.f32()?));
            }
            chs.push(v);
        }
        levels.push(chs);
    }
    // touched: the least recently used files go first when the cache is full
    if let Some(f) = cache_file(path) {
        let _ = std::fs::OpenOptions::new()
            .append(true)
            .open(&f)
            .and_then(|file| file.set_modified(std::time::SystemTime::now()));
    }
    Some(Peaks { levels })
}

/// Save the peaks of `path`, then keep the cache under its size limit.
pub fn store(path: &Path, data: &AudioData, peaks: &Peaks) {
    let Some(file) = cache_file(path) else {
        return;
    };
    if peaks.levels.len() != BLOCKS.len() {
        return;
    }
    let mut b = Vec::new();
    b.extend_from_slice(MAGIC);
    b.extend_from_slice(&(data.frames() as u64).to_le_bytes());
    b.extend_from_slice(&(data.channels() as u32).to_le_bytes());
    b.extend_from_slice(&(peaks.levels.len() as u32).to_le_bytes());
    for level in &peaks.levels {
        for ch in level {
            b.extend_from_slice(&(ch.len() as u64).to_le_bytes());
            for (lo, hi) in ch {
                b.extend_from_slice(&lo.to_le_bytes());
                b.extend_from_slice(&hi.to_le_bytes());
            }
        }
    }
    if std::fs::create_dir_all(cache_dir()).is_err() {
        return;
    }
    if crate::audio::write_atomic(&file, &b).is_ok() {
        prune(crate::prefs::cache_limit_bytes());
    }
}

/// Delete the least recently used cache files until the cache fits `limit` bytes.
pub fn prune(limit: u64) {
    let Ok(rd) = std::fs::read_dir(cache_dir()) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "acpk").unwrap_or(false))
        .filter_map(|p| {
            let m = std::fs::metadata(&p).ok()?;
            Some((m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len(), p))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    for (_, len, p) in files {
        if total <= limit {
            break;
        }
        if std::fs::remove_file(&p).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

/// Peaks for a freshly read file: from the cache when they are there and wanted, otherwise
/// worked out (and saved when wanted).
pub fn peaks_for(path: &Path, data: &AudioData) -> Peaks {
    if !crate::prefs::save_peak_files() {
        return Peaks::build(data);
    }
    if let Some(p) = load(path, data) {
        return p;
    }
    let p = Peaks::build(data);
    store(path, data, &p);
    p
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
}
