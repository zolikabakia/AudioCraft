//! Put the bundled FFmpeg and its licence beside the built program so decoding works from any
//! folder, and give every build the AudioCraft icon: inside the Windows .exe (a resource the
//! MSVC linker takes directly) and as PNG / ICO / ICNS / .desktop files beside the program for
//! Linux and macOS packages.

#[path = "src/icon.rs"]
mod icon;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn copy_if_changed(source: &Path, target: &Path) {
    let same_size = fs::metadata(source)
        .and_then(|s| fs::metadata(target).map(|t| s.len() == t.len()))
        .unwrap_or(false);
    if !same_size && fs::copy(source, target).is_err() {
        // A stale copy is not fatal: the source tree still contains the decoder.
        println!(
            "cargo:warning=could not place {} beside the executable",
            source.display()
        );
    }
}

/// Write the icon files into `dir` and, for Windows (MSVC) builds, link the icon into the .exe.
fn icons(dir: &std::path::Path) {
    println!("cargo:rerun-if-changed=src/icon.rs");
    let _ = fs::write(dir.join("audiocraft.png"), icon::png(256));
    let _ = fs::write(dir.join("audiocraft.ico"), icon::ico());
    let _ = fs::write(dir.join("audiocraft.icns"), icon::icns());
    let _ = fs::write(dir.join("audiocraft.desktop"), icon::DESKTOP);
    let windows = env::var("CARGO_CFG_TARGET_OS")
        .map(|o| o == "windows")
        .unwrap_or(false);
    let msvc = env::var("CARGO_CFG_TARGET_ENV")
        .map(|e| e == "msvc")
        .unwrap_or(false);
    if windows && msvc {
        if let Ok(out) = env::var("OUT_DIR") {
            let res = std::path::PathBuf::from(out).join("audiocraft-icon.res");
            if fs::write(&res, icon::windows_res()).is_ok() {
                println!("cargo:rustc-link-arg-bins={}", res.display());
            }
        }
    }
}

fn main() {
    for file in [
        "vendor/ffmpeg/bin/ffmpeg.exe",
        "vendor/ffmpeg/LICENSE.txt",
        "vendor/ffmpeg/COPYING.GPLv3.txt",
        "vendor/ffmpeg/NOTICE.md",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }

    let Ok(out_dir) = env::var("OUT_DIR") else {
        return;
    };
    // OUT_DIR is <target>/<profile>/build/<crate>/out; its third ancestor is the profile folder.
    let Some(profile) = PathBuf::from(out_dir)
        .ancestors()
        .nth(3)
        .map(|p| p.to_path_buf())
    else {
        return;
    };
    icons(&profile);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/ffmpeg");
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let exe = root.join("bin").join(name);
    if !exe.is_file() {
        return;
    }
    let target_root = profile.join("vendor/ffmpeg");
    let bin_dir = target_root.join("bin");
    if fs::create_dir_all(&bin_dir).is_err() {
        return;
    }
    copy_if_changed(&exe, &bin_dir.join(name));
    for file in ["LICENSE.txt", "COPYING.GPLv3.txt", "NOTICE.md"] {
        let source = root.join(file);
        if source.is_file() {
            copy_if_changed(&source, &target_root.join(file));
        }
    }
}
