#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]
//! AudioCraft: an audio editor with waveform editing, multitrack mixing and effects.

mod app;
mod audio;
mod commands;
mod compat;
mod curves;
mod dialogs;
mod dock;
mod dsp;
mod engine;
mod equi;
mod export;
mod extras;
mod ffmpeg;
mod filecmds;
mod fx2;
mod fx3;
mod fx_ui;
mod icon;
mod imports;
mod keys;
mod lanes;
mod layout;
mod lucide;
mod menus;
mod menus_more;
mod panels;
mod plugdock;
mod plugins;
mod prefs;
mod prefs_ui;
mod probe;
mod procs;
mod rack;
mod session;
mod sesx;
mod spectral;
mod speech;
mod sysinfo;
mod sysui;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_app;
mod theme;
mod tract;
mod tract_ui;
mod ui_multi;
mod ui_wave;
mod video;
mod workflows;
mod workspace;

use std::path::PathBuf;

const HELP: &str = "AudioCraft - audio editor and multitrack mixer

Usage:
  audiocraft [files...]                      open files in the editor
  audiocraft --list-commands                 print every command (id, title, shortcut, doc) as JSON
  audiocraft --inspect-session FILE.sesx     print what a session imports as (tracks, clips, fades, effects)
  audiocraft --batch IN --do STEP [--do STEP...] [--out OUT.wav]
                                             process a file without opening a window
  audiocraft --screenshot OUT.png [--demo] [--multitrack | --mixer | --spectral]
                                             [--fxshot EFFECT] [--shot-dialog file|session|close|prefs] [--autoshot]
                                             open a panel or dialog (diagnostic);
                                             --autoshot opens volume automation lanes for the shot

Batch steps:";

/// When the process started (for `AUDIOCRAFT_STARTUP_TIME=1`, which prints the time to the first frame).
pub static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn main() -> eframe::Result<()> {
    STARTED.get_or_init(std::time::Instant::now);
    let mut args = std::env::args().skip(1);
    let mut screenshot: Option<PathBuf> = None;
    let mut demo = false;
    let mut multi = false;
    let mut files = vec![];
    let mut batch: Option<PathBuf> = None;
    let mut steps: Vec<String> = vec![];
    let mut out: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            // app.rs reads these names again from the process arguments
            "--fxshot" | "--shot-dialog" => {
                let _ = args.next();
            }
            "--demo" => demo = true,
            "--multitrack" => multi = true,
            "--mixer" => multi = true,
            "--spectral" | "--pitch" | "--autoshot" => {}
            "--workspace" => {
                // read again by the app; skip the name here
                let _ = args.next();
            }
            "--probe-plugin" => {
                let name = args.next().unwrap_or_default();
                let code = probe::probe(&name);
                use std::io::Write;
                let _ = std::io::stdout().flush();
                if code != 0 || !plugins::hosted_loaded() {
                    std::process::exit(code);
                }
                plugins::finish_without_teardown();
            }
            "--list-commands" => {
                println!("{}", commands::list_json());
                return Ok(());
            }
            "--help" | "-h" => {
                println!("{HELP}");
                for (s, d) in commands::BATCH_STEPS {
                    println!("  {s:32} {d}");
                }
                return Ok(());
            }
            "--inspect-session" => {
                let Some(p) = args.next() else {
                    eprintln!("error: --inspect-session needs a .sesx file");
                    std::process::exit(2);
                };
                match sesx::import(std::path::Path::new(&p)) {
                    Ok((s, files)) => {
                        println!(
                            "{}  ({} Hz, {} tracks, {} files)",
                            s.name,
                            s.rate,
                            s.tracks.len(),
                            files.len()
                        );
                        for f in &files {
                            println!(
                                "  file {} {}",
                                if f.exists() { "ok     " } else { "MISSING" },
                                f.display()
                            );
                        }
                        for t in &s.tracks {
                            let fx: Vec<String> = t
                                .rack
                                .iter()
                                .map(|e| {
                                    format!(
                                        "{}{}",
                                        e.effect.name(),
                                        if e.enabled { "" } else { " (off)" }
                                    )
                                })
                                .collect();
                            println!(
                                "  {}: vol {:+.1} dB, pan {:+.0}, mute {}, {} clips, rack [{}] {} mix {:.0}%",
                                t.name,
                                t.volume_db,
                                t.pan * 100.0,
                                t.mute,
                                t.clips.len(),
                                fx.join(", "),
                                if t.fx.pre_fader {
                                    "pre-fader"
                                } else {
                                    "post-fader"
                                },
                                t.fx.mix * 100.0
                            );
                            let eff = session::effective_fades(t);
                            for (c, e) in t.clips.iter().zip(eff) {
                                let cfx: Vec<&str> =
                                    c.rack.iter().map(|e| e.effect.name()).collect();
                                println!(
                                    "    clip '{}' {:.3}-{:.3} s, src +{:.3}, speed {:.3}, gain {:+.1}, fades {:.3}/{:.3}{}{}, env {} pts{}",
                                    c.name,
                                    c.start,
                                    c.end(),
                                    c.offset,
                                    c.speed,
                                    c.gain_db,
                                    e.0,
                                    e.1,
                                    if e.2 { " xin" } else { "" },
                                    if e.3 { " xout" } else { "" },
                                    c.env.len(),
                                    if cfx.is_empty() {
                                        String::new()
                                    } else {
                                        format!(", clip fx [{}]", cfx.join(", "))
                                    }
                                );
                            }
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        std::process::exit(2);
                    }
                }
            }
            "--batch" => batch = args.next().map(PathBuf::from),
            "--do" => {
                if let Some(s) = args.next() {
                    steps.push(s);
                }
            }
            "--out" => out = args.next().map(PathBuf::from),
            other => files.push(PathBuf::from(other)),
        }
    }

    if let Some(input) = batch {
        let stem = input
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let output = out.unwrap_or_else(|| input.with_file_name(format!("{stem}_processed.wav")));
        match commands::run_batch(&input, &steps, &output) {
            Ok(msg) => {
                println!("{msg}");
                return Ok(());
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(2);
            }
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("AudioCraft")
        .with_icon(std::sync::Arc::new(app_icon(256)))
        .with_inner_size([1440.0, 880.0])
        .with_min_inner_size([960.0, 600.0]);
    if screenshot.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(windows)]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    #[allow(unused_mut)]
    // OpenGL starts in a fraction of the time the wgpu renderer takes on Windows;
    // AUDIOCRAFT_RENDERER=wgpu switches back.
    let renderer = if std::env::var("AUDIOCRAFT_RENDERER")
        .map(|v| v.eq_ignore_ascii_case("wgpu"))
        .unwrap_or(false)
    {
        eframe::Renderer::Wgpu
    } else {
        eframe::Renderer::Glow
    };
    // changed below on Linux only
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport,
        renderer,
        ..Default::default()
    };
    // Linux: winit cannot receive files dragged in from a file manager on Wayland, so under a
    // Wayland desktop the window opens through XWayland (X11), where dropping files works.
    // AUDIOCRAFT_WAYLAND=1 keeps native Wayland.
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_some() && std::env::var_os("AUDIOCRAFT_WAYLAND").is_none() {
        use winit::platform::x11::EventLoopBuilderExtX11;
        options.event_loop_builder = Some(Box::new(|b| {
            b.with_x11();
        }));
    }
    #[cfg(target_os = "macos")]
    if screenshot.is_some() {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        options.event_loop_builder = Some(Box::new(|b| {
            b.with_activate_ignoring_other_apps(false);
        }));
    }
    eframe::run_native(
        "AudioCraft",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, screenshot, demo, multi, files)))),
    )
}

/// The window and taskbar icon (the same drawing the .exe and the package icons use).
fn app_icon(size: u32) -> egui::IconData {
    egui::IconData {
        rgba: icon::rgba(size),
        width: size,
        height: size,
    }
}
