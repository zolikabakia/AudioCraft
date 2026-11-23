//! The operating system's own windows: file dialogs and the file browser / default app.
//! Every such call goes through here, so tests (which drive the whole app, command by
//! command) never block on a native dialog or start another program.

use std::path::{Path, PathBuf};

/// The native file dialogs, answered with "cancel" in tests.
pub trait Ask {
    fn ask_file(self) -> Option<PathBuf>;
    fn ask_files(self) -> Option<Vec<PathBuf>>;
    fn ask_save(self) -> Option<PathBuf>;
    fn ask_folder(self) -> Option<PathBuf>;
}

impl Ask for rfd::FileDialog {
    fn ask_file(self) -> Option<PathBuf> {
        if cfg!(test) { None } else { self.pick_file() }
    }
    fn ask_files(self) -> Option<Vec<PathBuf>> {
        if cfg!(test) { None } else { self.pick_files() }
    }
    fn ask_save(self) -> Option<PathBuf> {
        if cfg!(test) { None } else { self.save_file() }
    }
    fn ask_folder(self) -> Option<PathBuf> {
        if cfg!(test) { None } else { self.pick_folder() }
    }
}

/// Start the system's opener: `(program, arguments)` per platform.
fn launch(
    windows: (&str, Vec<String>),
    macos: (&str, Vec<String>),
    other: (&str, Vec<String>),
) -> std::io::Result<()> {
    if cfg!(test) {
        return Ok(());
    }
    let (prog, args) = if cfg!(target_os = "windows") {
        windows
    } else if cfg!(target_os = "macos") {
        macos
    } else {
        other
    };
    std::process::Command::new(prog)
        .args(args)
        .spawn()
        .map(|_| ())
}

/// Open a file (or folder) with the operating system's default application.
pub fn open_external(p: &Path) -> std::io::Result<()> {
    let s = p.display().to_string();
    launch(
        ("explorer", vec![s.clone()]),
        ("open", vec![s.clone()]),
        ("xdg-open", vec![s]),
    )
}

/// Show a file in the system's file browser (its folder, on systems that cannot select it).
pub fn reveal(p: &Path) -> std::io::Result<()> {
    let s = p.display().to_string();
    let dir = p.parent().unwrap_or(p).display().to_string();
    launch(
        ("explorer", vec![format!("/select,{s}")]),
        ("open", vec!["-R".into(), s]),
        ("xdg-open", vec![dir]),
    )
}
