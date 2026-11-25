//! Media links that survive moving a project folder.
//!
//! A session remembers each media file *relative to the session file* (`../stm/rain.wav`), so
//! what it really depends on is the **library folder**: the deepest folder that holds the
//! session file and every media file it uses. Move or copy that whole folder anywhere (another
//! drive, another parent) and the session still opens, because only the layout inside the
//! library is expected to stay the same. The absolute path saved alongside is only a fallback.
//!
//! [`find`] is the Relink Media search: look for missing files by name in one folder or in a
//! folder and everything inside it, with a few detector options.

use std::path::{Component, Path, PathBuf};

/// Resolve `.` and `..` without touching the disk.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                let last_is_normal = matches!(out.components().next_back(), Some(Component::Normal(_)));
                if last_is_normal {
                    out.pop();
                } else if !out.has_root() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn same_part(a: &Component, b: &Component) -> bool {
    let (a, b) = (a.as_os_str().to_string_lossy(), b.as_os_str().to_string_lossy());
    if cfg!(windows) {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

/// `to` written relative to the folder `from`, '/'-separated with `..` steps
/// (`C:/smt/stm/ggg` → `C:/smt/stm/rain.wav` gives `../rain.wav`). `None` when the two share
/// no root (another drive), so nothing relative can reach it.
pub fn relative(from: &Path, to: &Path) -> Option<String> {
    let (nf, nt) = (normalize(from), normalize(to));
    let f: Vec<Component> = nf.components().collect();
    let t: Vec<Component> = nt.components().collect();
    let mut i = 0;
    while i < f.len() && i < t.len() && same_part(&f[i], &t[i]) {
        i += 1;
    }
    // nothing shared, or only a drive letter without its root: no relative way there
    let shares_root = i > 0
        && f[..i]
            .iter()
            .any(|c| matches!(c, Component::RootDir | Component::Normal(_)));
    if !shares_root && (from.has_root() || to.has_root()) {
        return None;
    }
    let mut parts: Vec<String> = vec!["..".into(); f.len() - i];
    parts.extend(t[i..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    if parts.is_empty() {
        return Some(".".into());
    }
    Some(parts.join("/"))
}

/// The library folder: the deepest folder holding the session's folder and every media file.
pub fn library_root(session_dir: &Path, media: &[PathBuf]) -> PathBuf {
    let base = normalize(session_dir);
    let mut root: Vec<Component> = base.components().collect();
    for m in media {
        let dir = m.parent().map(normalize).unwrap_or_default();
        let parts: Vec<Component> = dir.components().collect();
        let mut i = 0;
        while i < root.len() && i < parts.len() && same_part(&root[i], &parts[i]) {
            i += 1;
        }
        root.truncate(i);
    }
    root.iter().map(|c| c.as_os_str()).collect()
}

/// Where a session's media file is expected, and whether it is there: first relative to the
/// session (it moves with the library), then the absolute path it was saved with, then a file
/// of the same name beside the session. When none exists the relative place is the expected one.
pub fn resolve(session_dir: &Path, rel: Option<&str>, abs: Option<&Path>) -> (PathBuf, bool) {
    let rel_p = rel
        .filter(|r| !r.trim().is_empty())
        .map(|r| normalize(&session_dir.join(r.replace('\\', "/"))));
    let by_name = abs
        .and_then(|a| a.file_name())
        .or_else(|| rel.and_then(|r| Path::new(r).file_name()))
        .map(|n| session_dir.join(n));
    for p in [rel_p.clone(), abs.map(Path::to_path_buf), by_name]
        .into_iter()
        .flatten()
    {
        if p.is_file() {
            return (p, true);
        }
    }
    let expected = rel_p
        .or_else(|| abs.map(Path::to_path_buf))
        .unwrap_or_default();
    (expected, false)
}

/// The Relink Media detector settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detect {
    /// Search every folder inside the chosen one too (otherwise that folder only).
    pub recursive: bool,
    /// A file with the same name but another audio extension also counts (rain.wav ↔ rain.flac).
    pub any_extension: bool,
    /// Upper and lower case do not matter (Rain.WAV ↔ rain.wav).
    pub ignore_case: bool,
}

impl Default for Detect {
    fn default() -> Self {
        Detect {
            recursive: true,
            any_extension: false,
            ignore_case: true,
        }
    }
}

/// Audio extensions the detector accepts when "any extension" is on.
pub const AUDIO_EXTS: &[&str] = &[
    "wav", "wave", "bwf", "aif", "aiff", "aifc", "flac", "mp3", "ogg", "oga", "opus", "m4a",
    "aac", "wma", "caf", "w64", "rf64", "mp4", "mov",
];

fn key(name: &str, d: Detect) -> String {
    let n = if d.any_extension {
        Path::new(name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| name.to_string())
    } else {
        name.to_string()
    };
    if d.ignore_case {
        n.to_lowercase()
    } else {
        n
    }
}

/// Look for each of `names` (file names) under `root`: one list of candidate paths per name,
/// nearest folders first. Stops after `limit` directory entries so a huge drive never hangs it.
pub fn find(root: &Path, names: &[String], d: Detect, limit: usize) -> Vec<Vec<PathBuf>> {
    let wanted: Vec<String> = names.iter().map(|n| key(n, d)).collect();
    let mut out: Vec<Vec<PathBuf>> = vec![vec![]; names.len()];
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut seen = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut subdirs = vec![];
        for e in rd.flatten() {
            seen += 1;
            if seen > limit {
                return out;
            }
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() {
                if d.recursive && !ft.is_symlink() {
                    subdirs.push(p);
                }
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if d.any_extension {
                let ext = Path::new(&name)
                    .extension()
                    .map(|x| x.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if !AUDIO_EXTS.contains(&ext.as_str()) {
                    continue;
                }
            }
            let k = key(&name, d);
            for (i, w) in wanted.iter().enumerate() {
                if *w == k {
                    out[i].push(p.clone());
                }
            }
        }
        subdirs.sort();
        for s in subdirs {
            queue.push_back((s, depth + 1));
        }
    }
    out
}

/// Extensions AudioCraft saves or opens as sessions: a name never keeps one of these (or an
/// audio extension) in front of its real extension ("xyz.sesx.acsession" is "xyz.acsession").
pub const KNOWN_EXTS: &[&str] = &["sesx", "acsession", "audiocraft", "xml"];

fn is_known_ext(e: &str) -> bool {
    let e = e.to_ascii_lowercase();
    KNOWN_EXTS.contains(&e.as_str()) || AUDIO_EXTS.contains(&e.as_str())
}

/// A name without any trailing known extensions ("xyz.sesx.sesx" -> "xyz"; "my.song" stays).
pub fn bare_name(name: &str) -> String {
    let mut n = name.trim().to_string();
    loop {
        let p = Path::new(&n);
        match (p.file_stem(), p.extension()) {
            (Some(stem), Some(ext)) if is_known_ext(&ext.to_string_lossy()) && !stem.is_empty() => {
                n = stem.to_string_lossy().to_string();
            }
            _ => return n,
        }
    }
}

/// The path a Save / Save As really writes: the chosen extension stays (or `default_ext` when
/// there is none or it is not one of `allowed`), and every known extension doubled in front of
/// it is dropped, so saving "xyz.sesx" as an AudioCraft session gives "xyz.acsession", never
/// "xyz.sesx.acsession", and saving again never grows "xyz.sesx.sesx".
pub fn clean_save_path(p: &Path, allowed: &[&str], default_ext: &str) -> PathBuf {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = Path::new(&name)
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .filter(|e| allowed.iter().any(|a| a.eq_ignore_ascii_case(e)))
        .unwrap_or_else(|| default_ext.to_string());
    let mut stem = bare_name(&name);
    if stem.is_empty() {
        stem = "Untitled".into();
    }
    p.with_file_name(format!("{stem}.{}", ext.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_names_never_stack_extensions() {
        let s = ["sesx", "acsession"];
        let c = |p: &str| clean_save_path(Path::new(p), &s, "sesx");
        assert_eq!(c("/a/xyz.sesx.sesx"), PathBuf::from("/a/xyz.sesx"));
        assert_eq!(c("/a/xyz.sesx.acsession"), PathBuf::from("/a/xyz.acsession"));
        assert_eq!(c("/a/xyz.acsession.sesx.sesx"), PathBuf::from("/a/xyz.sesx"));
        assert_eq!(c("/a/xyz"), PathBuf::from("/a/xyz.sesx"));
        assert_eq!(c("/a/my.song.sesx"), PathBuf::from("/a/my.song.sesx"));
        assert_eq!(c("/a/xyz.SESX"), PathBuf::from("/a/xyz.sesx"));
        let a = ["wav", "mp3", "flac"];
        assert_eq!(
            clean_save_path(Path::new("/a/rain.wav.mp3"), &a, "wav"),
            PathBuf::from("/a/rain.mp3")
        );
        assert_eq!(bare_name("Hangokos.sesx"), "Hangokos");
        assert_eq!(bare_name(".sesx"), ".sesx");
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ac-lib-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn relative_paths_climb_out_of_the_session_folder() {
        let s = Path::new("/smt/stm/ggg");
        assert_eq!(relative(s, Path::new("/smt/stm/rain.wav")).as_deref(), Some("../rain.wav"));
        assert_eq!(
            relative(s, Path::new("/smt/stm/ggg/a/b.wav")).as_deref(),
            Some("a/b.wav")
        );
        assert_eq!(
            relative(s, Path::new("/smt/other/c.wav")).as_deref(),
            Some("../../other/c.wav")
        );
        assert_eq!(normalize(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
    }

    #[cfg(windows)]
    #[test]
    fn another_drive_has_no_relative_path() {
        assert_eq!(relative(Path::new("C:\\a\\b"), Path::new("D:\\a\\x.wav")), None);
        assert_eq!(
            relative(Path::new("C:\\A\\b"), Path::new("c:\\a\\x.wav")).as_deref(),
            Some("../x.wav")
        );
    }

    #[test]
    fn the_library_folder_holds_the_session_and_all_media() {
        let r = library_root(
            Path::new("/smt/stm/ggg"),
            &[PathBuf::from("/smt/stm/rain.wav"), PathBuf::from("/smt/stm/ggg/x/y.wav")],
        );
        assert_eq!(r, PathBuf::from("/smt/stm"));
    }

    /// The person's example: sound in `stm`, session in `stm/ggg`; the whole tree moves to
    /// another parent and the session still finds the sound through its relative link.
    #[test]
    fn a_moved_library_still_resolves() {
        let base = tmp("moved");
        let old = base.join("smt/stm/ggg");
        let new_root = base.join("gugugaga/pleple/stm/ggg");
        std::fs::create_dir_all(&new_root).unwrap();
        std::fs::write(base.join("gugugaga/pleple/stm/rain.wav"), b"x").unwrap();
        let rel = relative(&old, &base.join("smt/stm/rain.wav")).unwrap();
        assert_eq!(rel, "../rain.wav");
        let (p, found) = resolve(&new_root, Some(&rel), Some(&base.join("smt/stm/rain.wav")));
        assert!(found, "found at {}", p.display());
        assert_eq!(p, normalize(&base.join("gugugaga/pleple/stm/rain.wav")));
        // nothing there: the relative place is what it expects
        let (p, found) = resolve(&new_root, Some("../gone.wav"), None);
        assert!(!found);
        assert_eq!(p, normalize(&base.join("gugugaga/pleple/stm/gone.wav")));
    }

    #[test]
    fn the_detector_searches_one_folder_or_all_inside_it() {
        let base = tmp("find");
        std::fs::create_dir_all(base.join("a/b")).unwrap();
        std::fs::write(base.join("Rain.WAV"), b"x").unwrap();
        std::fs::write(base.join("a/b/rain.flac"), b"x").unwrap();
        std::fs::write(base.join("a/notes.txt"), b"x").unwrap();
        let names = vec!["rain.wav".to_string()];
        let only = find(
            &base,
            &names,
            Detect {
                recursive: false,
                any_extension: false,
                ignore_case: true,
            },
            10_000,
        );
        assert_eq!(only[0].len(), 1, "{:?}", only);
        let strict = find(
            &base,
            &names,
            Detect {
                recursive: true,
                any_extension: false,
                ignore_case: false,
            },
            10_000,
        );
        assert!(strict[0].is_empty(), "case matters: {:?}", strict);
        let all = find(
            &base,
            &names,
            Detect {
                recursive: true,
                any_extension: true,
                ignore_case: true,
            },
            10_000,
        );
        assert_eq!(all[0].len(), 2, "both, nearest first: {:?}", all);
        assert!(all[0][0].ends_with("Rain.WAV"));
    }
}
