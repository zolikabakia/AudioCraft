//! Finding, choosing and installing the free FFmpeg decoder AudioCraft uses for video.
//!
//! The bundled executable lives in `vendor/ffmpeg/bin` (both in the source folder and beside
//! the built program). It is a separate LGPL-licensed program that AudioCraft starts as a
//! subprocess. Edit > Preferences > Video lists every decoder found, lets the person pick which
//! one is used, and can download other free builds into AudioCraft's own decoders folder.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// Diagnostic switch used by tests: ignore the bundled copy and search only PATH/folders.
pub const DISABLE_ENV: &str = "AUDIOCRAFT_NO_BUNDLED_FFMPEG";

/// The decoder the person picked in Preferences (empty = automatic).
static SELECTED: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Remember the decoder chosen in Preferences so every caller (import, export) uses it.
pub fn set_selected(path: Option<PathBuf>) {
    if let Ok(mut s) = SELECTED.lock() {
        *s = path.filter(|p| p.is_file());
    }
}

pub fn selected() -> Option<PathBuf> {
    SELECTED
        .lock()
        .ok()
        .and_then(|s| s.clone())
        .filter(|p| p.is_file())
}

/// A command for a helper program that never opens a console window (AudioCraft is a windowed
/// app, so without this every FFmpeg call flashed a terminal) and never waits on stdin.
pub fn command(exe: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

fn relative_path() -> PathBuf {
    Path::new("vendor")
        .join("ffmpeg")
        .join("bin")
        .join(exe_name())
}

/// The copy of FFmpeg shipped with this build, if it is present.
///
/// A normal build can find it next to the executable (including after the app folder is moved)
/// or in the source folder. `AUDIOCRAFT_NO_BUNDLED_FFMPEG=1` skips it for diagnostics/tests.
pub fn bundled() -> Option<PathBuf> {
    if std::env::var_os(DISABLE_ENV).is_some() {
        return None;
    }
    let rel = relative_path();
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        for folder in exe.ancestors().take(6) {
            roots.push(folder.to_path_buf());
        }
    }
    if let Ok(dir) = std::env::current_dir() {
        roots.push(dir);
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    roots
        .into_iter()
        .map(|r| r.join(&rel))
        .find(|c| c.is_file())
}

/// Confirm that an executable is a real FFmpeg we can run, not just a file with the right name.
pub fn usable(path: &Path) -> bool {
    command(path)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The first line of `ffmpeg -version`, or a short note when it doesn't run.
pub fn version(exe: &Path) -> String {
    match command(exe).arg("-version").stderr(Stdio::null()).output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or("FFmpeg")
            .trim()
            .to_string(),
        _ => "does not run".into(),
    }
}

fn on_path() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|d| d.join(exe_name()))
        .find(|c| c.is_file())
}

/// The decoder for export and import: the person's choice, then the bundled copy, then PATH.
/// No probe process is started for the common case, so finding it costs nothing.
pub fn find() -> Option<PathBuf> {
    selected()
        .or_else(bundled)
        .or_else(|| on_path().filter(|p| usable(p)))
}

/// Where downloaded decoders are installed (one sub-folder per build).
pub fn install_root() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("AudioCraft").join("decoders")
}

/// A free decoder build that can be downloaded from inside the app.
pub struct Download {
    pub id: &'static str,
    pub name: &'static str,
    pub licence: &'static str,
    pub url: &'static str,
}

/// Free FFmpeg builds published by their maintainers. LGPL builds first (the default family).
pub fn catalogue() -> &'static [Download] {
    if cfg!(windows) {
        &[
            Download {
                id: "btbn-lgpl",
                name: "FFmpeg (BtbN, LGPL, latest)",
                licence: "LGPL-2.1+",
                url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-lgpl.zip",
            },
            Download {
                id: "btbn-gpl",
                name: "FFmpeg (BtbN, GPL, more codecs)",
                licence: "GPL-3.0",
                url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
            },
            Download {
                id: "gyan-essentials",
                name: "FFmpeg (gyan.dev essentials release)",
                licence: "GPL-3.0",
                url: "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
            },
        ]
    } else {
        &[
            Download {
                id: "btbn-lgpl",
                name: "FFmpeg (BtbN, LGPL, latest)",
                licence: "LGPL-2.1+",
                url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-lgpl.tar.xz",
            },
            Download {
                id: "btbn-gpl",
                name: "FFmpeg (BtbN, GPL, more codecs)",
                licence: "GPL-3.0",
                url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz",
            },
        ]
    }
}

/// Find `ffmpeg` up to three folders deep (archives usually hold one versioned folder with bin/).
pub fn exe_below(folder: &Path, depth: usize) -> Option<PathBuf> {
    let direct = folder.join(exe_name());
    if direct.is_file() {
        return Some(direct);
    }
    if depth == 0 {
        return None;
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.iter().find_map(|d| exe_below(d, depth - 1))
}

/// One decoder AudioCraft can use, for the Preferences list.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub label: String,
    pub path: PathBuf,
}

/// Every decoder found: bundled, downloaded ones, PATH. Cheap: only checks that files exist.
pub fn installed() -> Vec<Installed> {
    let mut out: Vec<Installed> = Vec::new();
    let mut push = |label: String, path: PathBuf| {
        if !out.iter().any(|i| i.path == path) {
            out.push(Installed { label, path });
        }
    };
    if let Some(p) = bundled() {
        push("Bundled FFmpeg (LGPL)".into(), p);
    }
    let root = install_root();
    for d in catalogue() {
        if let Some(p) = exe_below(&root.join(d.id), 3) {
            push(format!("{} — downloaded", d.name), p);
        }
    }
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if catalogue().iter().any(|d| d.id == name) {
                continue;
            }
            if let Some(p) = exe_below(&e.path(), 3) {
                push(format!("{name} — installed"), p);
            }
        }
    }
    if let Some(p) = on_path() {
        push("System FFmpeg (PATH)".into(), p);
    }
    out
}

/// Download and unpack a catalogue build into the decoders folder. Runs on a worker thread;
/// uses the curl and tar programs that ship with Windows 10+ and every Linux desktop.
pub fn download(id: &str) -> Result<PathBuf, String> {
    let d = catalogue()
        .iter()
        .find(|d| d.id == id)
        .ok_or("unknown decoder")?;
    let dest = install_root().join(d.id);
    std::fs::create_dir_all(&dest)
        .map_err(|e| format!("Could not create {}: {e}", dest.display()))?;
    let archive = dest.join(if d.url.ends_with(".zip") {
        "download.zip"
    } else {
        "download.tar.xz"
    });
    let curl = if cfg!(windows) { "curl.exe" } else { "curl" };
    let got = command(curl)
        .args(["-L", "--fail", "--silent", "--show-error", "-o"])
        .arg(&archive)
        .arg(d.url)
        .output()
        .map_err(|e| format!("Could not start curl: {e}"))?;
    if !got.status.success() {
        return Err(format!(
            "Download failed: {}",
            String::from_utf8_lossy(&got.stderr).trim()
        ));
    }
    let tar = if cfg!(windows) { "tar.exe" } else { "tar" };
    let unpacked = command(tar)
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&dest)
        .output()
        .map_err(|e| format!("Could not start tar: {e}"))?;
    let _ = std::fs::remove_file(&archive);
    if !unpacked.status.success() {
        return Err(format!(
            "Could not unpack the decoder: {}",
            String::from_utf8_lossy(&unpacked.stderr).trim()
        ));
    }
    let exe = exe_below(&dest, 3).ok_or("The download did not contain ffmpeg")?;
    if !usable(&exe) {
        return Err(format!(
            "{} was installed but does not run on this computer",
            exe.display()
        ));
    }
    Ok(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_decoder_is_found_and_runs() {
        if !cfg!(windows) {
            return; // a Linux build uses the system decoder instead
        }
        let exe = bundled().expect("AudioCraft must ship a bundled FFmpeg on Windows");
        assert!(exe.is_file(), "{}", exe.display());
        assert!(
            exe.to_string_lossy()
                .replace('\\', "/")
                .ends_with("vendor/ffmpeg/bin/ffmpeg.exe")
        );
        assert!(usable(&exe), "bundled FFmpeg must run: {}", exe.display());
        assert!(version(&exe).starts_with("ffmpeg version"));
        set_selected(None);
        assert_eq!(
            find(),
            Some(exe),
            "export and import should use the bundled decoder"
        );
    }

    #[test]
    fn diagnostic_switch_can_hide_the_bundled_decoder() {
        let before = std::env::var_os(DISABLE_ENV);
        unsafe { std::env::set_var(DISABLE_ENV, "1") }
        let hidden = bundled();
        unsafe {
            match before {
                Some(v) => std::env::set_var(DISABLE_ENV, v),
                None => std::env::remove_var(DISABLE_ENV),
            }
        }
        assert!(hidden.is_none());
    }

    #[test]
    fn a_selected_decoder_wins_and_nested_archives_are_found() {
        let root = std::env::temp_dir().join(format!("audiocraft-ffsel-{}", std::process::id()));
        let bin = root.join("ffmpeg-7.1-full").join("bin");
        let _ = std::fs::create_dir_all(&bin);
        let exe = bin.join(exe_name());
        let _ = std::fs::write(&exe, b"x");
        assert_eq!(exe_below(&root, 3), Some(exe.clone()));
        set_selected(Some(exe.clone()));
        assert_eq!(find(), Some(exe.clone()));
        set_selected(None);
        assert_ne!(find(), Some(exe));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_catalogue_only_lists_free_builds() {
        assert!(!catalogue().is_empty());
        for d in catalogue() {
            assert!(d.url.starts_with("https://"));
            assert!(d.licence.contains("GPL"));
        }
    }
}
