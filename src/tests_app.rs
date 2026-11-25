//! Whole-app edge-case tests: the real `App` drawn frame by frame through egui (no window,
//! no sound card, no native dialogs, no plug-ins), with every command run in several states.
//! A command may refuse (an error message is fine); it must never panic, and every frame drawn
//! after it (with whatever dialog it opened) must not panic either.

use crate::app::{App, CenterTab, Mode};
use crate::video::{VideoFrame, VideoInfo, VideoMedia, VideoState};
use egui::{Pos2, Rect, vec2};
use std::path::PathBuf;

pub struct Harness {
    pub ctx: egui::Context,
    pub app: App,
    frame: eframe::Frame,
    size: egui::Vec2,
    /// When set, each frame is 1/60 s after the last (pointer speed is then real).
    pub clock: Option<f64>,
}

/// The file handle egui receives when the operating system drops a file on the window.
#[derive(Debug)]
struct TestDroppedFile(PathBuf);

impl egui::DroppedFile for TestDroppedFile {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

impl Harness {
    pub fn new(demo: bool, multi: bool) -> Harness {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        // a screenshot path keeps the app from opening the sound card on its second frame
        let shot =
            std::env::temp_dir().join(format!("audiocraft-test-shot-{}.png", std::process::id()));
        let app = App::new(&cc, Some(shot), demo, multi, vec![]);
        let mut h = Harness {
            ctx,
            app,
            frame: eframe::Frame::_new_kittest(),
            size: vec2(1600.0, 1000.0),
            clock: None,
        };
        h.frames(2);
        h
    }

    pub fn with_size(mut self, w: f32, h: f32) -> Harness {
        self.size = vec2(w, h);
        self
    }

    /// Draw `n` frames with the given input events in the first one.
    pub fn frames_with(&mut self, n: usize, events: Vec<egui::Event>) {
        let mut events = Some(events);
        for _ in 0..n {
            if let Some(c) = self.clock.as_mut() {
                *c += 1.0 / 60.0;
            }
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                events: events.take().unwrap_or_default(),
                time: self.clock,
                ..Default::default()
            };
            let (app, frame) = (&mut self.app, &mut self.frame);
            let mut out = self.ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, frame));
            // no GPU here: the font atlas and image uploads are dropped on purpose
            out.textures_delta.clear();
        }
    }

    pub fn frames(&mut self, n: usize) {
        self.frames_with(n, vec![]);
    }

    /// Drop OS files at `p`, then draw one frame so the app handles the drop.
    pub fn frames_with_drop(&mut self, p: Pos2, paths: Vec<PathBuf>) {
        let dropped: Vec<egui::DroppedFileHandle> = paths
            .into_iter()
            .map(|path| std::sync::Arc::new(TestDroppedFile(path)) as egui::DroppedFileHandle)
            .collect();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            events: vec![egui::Event::PointerMoved(p)],
            dropped_files: dropped,
            ..Default::default()
        };
        let (app, frame) = (&mut self.app, &mut self.frame);
        let mut out = self.ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, frame));
        out.textures_delta.clear();
    }

    /// A click at `p` (press and release), then a frame for it to take effect.
    pub fn click(&mut self, p: Pos2) {
        let ev = |pressed| egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        self.frames_with(1, vec![egui::Event::PointerMoved(p), ev(true)]);
        self.frames_with(2, vec![ev(false)]);
    }

    /// A click at `p` with modifier keys held (press and release). The modifiers are set for
    /// both the press and the release, then cleared again.
    pub fn click_mods(&mut self, p: Pos2, mods: egui::Modifiers) {
        let ev = |pressed| egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: mods,
        };
        self.frames_with(
            1,
            vec![
                egui::Event::ModifiersChanged(mods),
                egui::Event::PointerMoved(p),
                ev(true),
            ],
        );
        self.frames_with(2, vec![egui::Event::ModifiersChanged(mods), ev(false)]);
        self.frames_with(1, vec![egui::Event::ModifiersChanged(Default::default())]);
    }

    /// Press at `from`, drag to `to` (with modifiers held the whole way), release there.
    pub fn drag_mods(&mut self, from: Pos2, to: Pos2, mods: egui::Modifiers) {
        let ev = |pos: Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: mods,
        };
        self.frames_with(
            1,
            vec![
                egui::Event::ModifiersChanged(mods),
                egui::Event::PointerMoved(from),
                ev(from, true),
            ],
        );
        self.frames_with(
            2,
            vec![
                egui::Event::ModifiersChanged(mods),
                egui::Event::PointerMoved(to),
            ],
        );
        self.frames_with(2, vec![ev(to, false)]);
        self.frames_with(1, vec![egui::Event::ModifiersChanged(Default::default())]);
    }

    pub fn run(&mut self, id: &str) -> Result<(), String> {
        let ctx = self.ctx.clone();
        self.app.exec(id, &ctx)
    }

    /// Close whatever the last command opened, so the next starts from the same state.
    pub fn settle(&mut self) {
        self.app.dialog = None;
        self.app.palette = None;
        self.app.prefs_edit = None;
        self.app.rack_window = None;
        self.frames(1);
    }
}

/// Commands that leave the app (quit) or that a test cannot meaningfully run.
fn skipped(id: &str) -> bool {
    matches!(id, "file.exit" | "file.quit")
}

/// Runs every command in one state, returning the ones that panicked.
fn every_command(demo: bool, multi: bool) -> Vec<String> {
    let mut failed = vec![];
    let ids: Vec<&'static str> = crate::commands::all().map(|c| c.id).collect();
    let mut h = Harness::new(demo, multi);
    for id in ids {
        if skipped(id) {
            continue;
        }
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = h.run(id);
            h.frames(2);
            h.settle();
        }));
        if r.is_err() {
            failed.push(id.to_string());
            // start the next command from a fresh app: this one may be half-changed
            h = Harness::new(demo, multi);
        }
    }
    failed
}

#[test]
fn every_command_with_nothing_open() {
    let f = every_command(false, false);
    assert!(
        f.is_empty(),
        "these commands panicked with nothing open: {f:?}"
    );
}

#[test]
fn every_command_with_a_file_open() {
    let f = every_command(true, false);
    assert!(
        f.is_empty(),
        "these commands panicked with a file open: {f:?}"
    );
}

#[test]
fn every_command_in_a_multitrack_session() {
    let f = every_command(true, true);
    assert!(
        f.is_empty(),
        "these commands panicked in a multitrack session: {f:?}"
    );
}

#[test]
fn opening_audio_returns_before_decoding_and_peaks_arrive_off_thread() {
    let dir = std::env::temp_dir().join(format!("audiocraft-import-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("tone.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut wav = hound::WavWriter::create(&path, spec).expect("wav");
        for i in 0..8000 {
            wav.write_sample((i % 200) as i16 - 100).expect("sample");
        }
    }
    let mut h = Harness::new(false, false);
    h.app.open_path(&path);
    assert_eq!(h.app.files.len(), 1, "the file appears immediately");
    assert_eq!(
        h.app.cur().map(|f| f.data.frames()),
        Some(0),
        "no decoding on the frame thread"
    );
    assert!(h.app.status.starts_with("Importing"), "{}", h.app.status);
    for _ in 0..200 {
        h.frames(1);
        if h.app.status.starts_with("Opened") {
            break;
        }
    }
    assert!(h.app.status.starts_with("Opened"), "{}", h.app.status);
    let f = h.app.cur().expect("finished file");
    assert_eq!(f.data.frames(), 8000);
    assert!(!f.peaks.levels.is_empty(), "peaks were built on the worker");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multitrack_placement_waits_for_the_real_clip_length() {
    let dir = std::env::temp_dir().join(format!(
        "audiocraft-multi-import-test-{}",
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("clip.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut wav = hound::WavWriter::create(&path, spec).expect("wav");
        for _ in 0..4000 {
            wav.write_sample(0i16).expect("sample");
        }
    }
    let mut h = Harness::new(false, true);
    h.app.ensure_mixer_session();
    if let Some(s) = h.app.session.as_mut() {
        s.rate = 8000;
        s.bits = 16;
        while s.tracks.is_empty() {
            s.add_track(true);
        }
    }
    h.app.open_path(&path);
    let id = h.app.cur().expect("placeholder").id;
    h.app.pending_placements.push((id, 0, 0.0));
    assert!(
        h.app
            .session
            .as_ref()
            .map(|s| s.tracks[0].clips.is_empty())
            .unwrap_or(true)
    );
    for _ in 0..200 {
        h.frames(1);
        if h.app.status.starts_with("Opened") {
            break;
        }
    }
    assert!(h.app.status.starts_with("Opened"), "{}", h.app.status);
    let len = h
        .app
        .session
        .as_ref()
        .and_then(|s| s.tracks.first())
        .and_then(|t| t.clips.first())
        .map(|c| c.len)
        .unwrap_or(0.0);
    assert!((len - 0.5).abs() < 0.01, "clip length {len}");
    assert!(h.app.pending_placements.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

fn drop_test_wav(dir: &std::path::Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut wav = hound::WavWriter::create(&path, spec).expect("wav");
    for _ in 0..4000 {
        wav.write_sample(0.0f32).expect("sample");
    }
    path
}

fn drop_test_session() -> Harness {
    let mut h = Harness::new(false, true);
    h.app.ensure_mixer_session();
    if let Some(s) = h.app.session.as_mut() {
        s.rate = 48000;
        s.bits = 32;
        while s.tracks.is_empty() {
            s.add_track(true);
        }
        s.cursor = 3.5;
    }
    h
}

#[test]
fn dropping_a_sound_into_the_files_browser_does_not_insert_a_clip() {
    let dir =
        std::env::temp_dir().join(format!("audiocraft-files-drop-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = drop_test_wav(&dir, "into_browser.wav");
    let mut h = drop_test_session();
    assert!(
        h.app.ws.layout.contains(crate::workspace::PanelId::Files),
        "the default workspace has a Files panel"
    );
    let point = h
        .app
        .files_drop_rects
        .first()
        .map(|r| r.center())
        .expect("the visible Files panel has a real drop target");
    let cursor_before = h.app.session.as_ref().map(|s| s.cursor);
    let current_before = h.app.current;
    h.app.center_tab = CenterTab::Mixer;

    h.frames_with_drop(point, vec![path.clone()]);

    assert!(
        h.app
            .files
            .iter()
            .any(|f| f.path.as_deref() == Some(path.as_path())),
        "the sound is in the browser"
    );
    assert!(
        h.app.pending_placements.is_empty(),
        "no clip was queued for the timeline"
    );
    assert!(
        h.app
            .session
            .as_ref()
            .map(|s| s.tracks.iter().all(|t| t.clips.is_empty()))
            .unwrap_or(false),
        "no clip was inserted"
    );
    assert_eq!(
        h.app.session.as_ref().map(|s| s.cursor),
        cursor_before,
        "the playhead did not move"
    );
    assert_eq!(h.app.mode, Mode::Multitrack, "the session stays open");
    assert_eq!(
        h.app.current, current_before,
        "the editor keeps its previous sound"
    );
    assert_eq!(
        h.app.center_tab,
        CenterTab::Mixer,
        "the centre view did not change"
    );
    for _ in 0..200 {
        h.frames(1);
        if h.app.status.starts_with("Opened") {
            break;
        }
    }
    assert!(
        h.app
            .session
            .as_ref()
            .map(|s| s.tracks.iter().all(|t| t.clips.is_empty()))
            .unwrap_or(false),
        "finishing the import still inserts nothing"
    );
    assert_eq!(
        h.app.current, current_before,
        "finishing the import does not activate the sound"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn re_dropping_an_open_sound_keeps_the_view_and_playback() {
    let dir = std::env::temp_dir().join(format!(
        "audiocraft-files-redrop-test-{}",
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let path = drop_test_wav(&dir, "already_open.wav");
    let mut h = drop_test_session();
    h.app.open_path(&path);
    for _ in 0..200 {
        h.frames(1);
        if h.app.status.starts_with("Opened") {
            break;
        }
    }
    assert!(h.app.status.starts_with("Opened"), "{}", h.app.status);
    let id = h
        .app
        .files
        .iter()
        .find(|f| f.path.as_deref() == Some(path.as_path()))
        .map(|f| f.id)
        .expect("the sound is already open");
    h.app.current = None;
    h.app.center_tab = CenterTab::Mixer;
    h.app.engine.lock().playing = true;
    let point = h
        .app
        .files_drop_rects
        .first()
        .map(|r| r.center())
        .expect("visible browser");
    let cursor_before = h.app.session.as_ref().map(|s| s.cursor);

    h.frames_with_drop(point, vec![path]);

    assert!(
        h.app.is_playing(),
        "re-dropping an open sound does not stop transport"
    );
    assert_eq!(
        h.app.current, None,
        "re-dropping does not activate the sound in an editor"
    );
    assert_eq!(
        h.app.center_tab,
        CenterTab::Mixer,
        "re-dropping does not change the centre view"
    );
    assert_eq!(h.app.mode, Mode::Multitrack);
    assert_eq!(
        h.app.selected_file,
        Some(id),
        "the existing browser entry is selected"
    );
    assert_eq!(h.app.session.as_ref().map(|s| s.cursor), cursor_before);
    assert!(h.app.pending_placements.is_empty());
    assert!(
        h.app
            .session
            .as_ref()
            .map(|s| s.tracks.iter().all(|t| t.clips.is_empty()))
            .unwrap_or(false)
    );
    h.app.engine.lock().playing = false;
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn browser_drop_targets_follow_visible_active_tabs() {
    let mut h = drop_test_session();
    assert_eq!(
        h.app.files_drop_rects.len(),
        1,
        "the default has one active Files browser"
    );
    h.app.ws.layout.left.push(crate::workspace::Group {
        tabs: vec![crate::workspace::PanelId::Favorites],
        active: 0,
        weight: 0.2,
    });
    h.frames(1);
    assert_eq!(
        h.app.files_drop_rects.len(),
        2,
        "separately visible Files and Favorites each have a target"
    );

    h.app.ws.layout.left[0].tabs = vec![crate::workspace::PanelId::Video];
    h.app.ws.layout.left[0].active = 0;
    h.frames(1);
    assert_eq!(
        h.app.files_drop_rects.len(),
        1,
        "hiding Files removes only its stale target"
    );
    let last = h.app.ws.layout.left.len() - 1;
    h.app.ws.layout.left[last].tabs = vec![crate::workspace::PanelId::Diagnostics];
    h.frames(1);
    assert!(
        h.app.files_drop_rects.is_empty(),
        "a workspace with no visible browser has no browser drop target"
    );
}

#[test]
fn dropping_a_sound_on_the_timeline_still_inserts_a_clip() {
    let dir = std::env::temp_dir().join(format!(
        "audiocraft-timeline-drop-test-{}",
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let path = drop_test_wav(&dir, "onto_timeline.wav");
    let mut h = drop_test_session();
    h.app.files_drop_rects.clear();
    let cursor = h.app.session.as_ref().map(|s| s.cursor).unwrap_or(0.0);

    h.frames_with_drop(Pos2::new(800.0, 400.0), vec![path]);
    // The timeline uses the spot under the pointer; a drop outside the lanes uses the playhead.
    let expected = h
        .app
        .pending_placements
        .first()
        .map(|(_, _, at)| *at)
        .unwrap_or(cursor);

    for _ in 0..200 {
        h.frames(1);
        if h.app.status.starts_with("Opened") {
            break;
        }
    }
    let clip = h
        .app
        .session
        .as_ref()
        .and_then(|s| s.tracks.iter().flat_map(|t| t.clips.iter()).next());
    let clip = clip.unwrap_or_else(|| {
        panic!(
            "a drop on the timeline still makes a clip; status={:?} pending={:?} mode={:?}",
            h.app.status, h.app.pending_placements, h.app.mode
        )
    });
    assert!(
        (clip.start - expected).abs() < 0.01,
        "clip starts where it was dropped: {} vs {}",
        clip.start,
        expected
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dropping_a_video_without_decoder_is_refused_cleanly() {
    let mut h = Harness::new(false, false);
    // Point at empty locations and hide the bundled copy so this test stays deterministic.
    h.app.prefs.video_decoder_exe.clear();
    h.app.prefs.video_decoder_folder = std::env::temp_dir()
        .join(format!("ac-no-decoder-{}", std::process::id()))
        .to_string_lossy()
        .to_string();
    h.app.prefs.video_decoder_repos.clear();
    let old_path = std::env::var_os("PATH");
    let old_bundled = std::env::var_os(crate::ffmpeg::DISABLE_ENV);
    unsafe {
        std::env::set_var("PATH", "");
        std::env::set_var(crate::ffmpeg::DISABLE_ENV, "1");
    }
    h.app.open_path(&PathBuf::from("holiday.mp4"));
    unsafe {
        match old_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
        match old_bundled {
            Some(v) => std::env::set_var(crate::ffmpeg::DISABLE_ENV, v),
            None => std::env::remove_var(crate::ffmpeg::DISABLE_ENV),
        }
    }
    assert!(
        h.app.files.is_empty(),
        "a video with no decoder must not be imported"
    );
    assert!(
        matches!(h.app.video, crate::video::VideoState::Error(_)),
        "missing decoder should be reported"
    );
    assert!(
        h.app.status.contains("not imported")
            || h.app.status.to_ascii_lowercase().contains("decoder"),
        "{}",
        h.app.status
    );
    h.frames(2);
}

#[test]
fn video_panel_draws_loading_error_and_ready_states() {
    let mut h = Harness::new(false, false);
    h.app.show_panel(crate::workspace::PanelId::Video);

    h.app.video = VideoState::Loading(PathBuf::from("clip.mp4"));
    h.frames(2);

    h.app.video = VideoState::Error(crate::video::VideoError {
        path: PathBuf::from("clip.mp4"),
        message: "No decoder found. Set the decoder folder in Preferences.".into(),
    });
    h.frames(2);

    let mut media = VideoMedia::new(
        PathBuf::from("clip.mp4"),
        VideoInfo {
            duration: 2.0,
            width: 4,
            height: 4,
            fps: 10.0,
        },
    );
    // still loading: no thumbnails yet
    media.complete = false;
    h.app.video = VideoState::Ready(Box::new(media.clone()));
    h.frames(2);
    media.frames = (0..4)
        .map(|i| VideoFrame {
            time: i as f64,
            width: 4,
            height: 4,
            rgba: vec![40; 4 * 4 * 4],
        })
        .collect();
    media.complete = true;
    h.app.video = VideoState::Ready(Box::new(media));
    h.frames(2);
}

/// Record never records without a target, gets ready first and starts with Play.
#[test]
fn record_waits_for_play_and_needs_a_target() {
    let mut h = Harness::new(false, false);
    // Waveform without a file: Record asks for a new file instead of recording
    h.app.toggle_record();
    assert!(!h.app.extra.record_ready);
    assert!(matches!(
        h.app.dialog,
        Some(crate::app::Dialog::NewFile { .. })
    ));
    h.app.dialog = None;
    // Multitrack with nothing armed: refused
    h.app.exec("file.new_session", &h.ctx.clone()).ok();
    h.app.dialog = None;
    h.app.session = Some(crate::app::mixer_session());
    h.app.switch_mode(Mode::Multitrack);
    h.app.toggle_record();
    assert!(
        !h.app.extra.record_ready,
        "nothing armed: nothing to record on"
    );
    h.app.dialog = None;
    if let Some(s) = h.app.session.as_mut() {
        s.tracks[0].io.input = "Default Stereo Input".into();
        s.tracks[0].arm = true;
    }
    h.app.toggle_record();
    assert!(h.app.extra.record_ready, "armed: Record gets ready");
    assert!(!h.app.is_recording(), "Record alone does not record");
    h.app.toggle_record();
    assert!(!h.app.extra.record_ready, "pressing Record again cancels");
    h.frames(2);
}

/// Panels can move to windows of their own and come back; the help search finds commands.
#[test]
fn undocked_panels_and_help_search_draw() {
    let mut h = Harness::new(true, false);
    h.app
        .exec("window.new_window", &h.ctx.clone())
        .expect("new window");
    h.app.ws.layout.undock(crate::workspace::PanelId::Markers);
    h.frames(3);
    h.app.extra.help_query = "fade".into();
    h.app.extra.help_topic = Some("help.search");
    h.frames(2);
    h.app
        .exec("window.dock_all", &h.ctx.clone())
        .expect("dock all");
    assert!(h.app.ws.layout.windows.is_empty());
    assert!(h.app.ws.layout.contains(crate::workspace::PanelId::Markers));
    h.app.set_workspace("Maximum Editing (Dual Monitor)");
    h.frames(2);
}

/// The handle fades stay adjustable: dragging again replaces the fade (one undo step).
#[test]
fn handle_fades_can_be_changed_after_release() {
    let mut h = Harness::new(true, false);
    let Some(f) = h.app.cur_mut() else { return };
    let before = f.hist_pos;
    f.set_fades([(0.5, 1.0), (0.0, 1.0)], "Fade In");
    assert_eq!(f.hist_pos, before + 1);
    assert_eq!(f.live_fades().map(|(_, x)| x[0].0), Some(0.5));
    f.set_fades([(1.0, 1.0), (0.3, 1.0)], "Fade In/Out");
    assert_eq!(
        f.hist_pos,
        before + 1,
        "changing the fade replaces it, it does not stack"
    );
    assert_eq!(f.data.ch[0][0], 0.0);
    f.set_fades([(0.0, 1.0), (0.0, 1.0)], "Fade In");
    assert_eq!(
        f.hist_pos, before,
        "both handles back at the edges removes the fade"
    );
    assert!(f.live_fades().is_none());
}

/// The Speech window previews without closing and adds files on request; the vocal tract tab
/// renders; the batch processor runs on a worker.
#[test]
fn speech_preview_keeps_the_window_and_batch_runs_in_the_background() {
    let mut h = Harness::new(false, false);
    h.app.dialog = Some(crate::app::Dialog::Speech {
        pitch: 0.0,
        text: String::new(),
        voice: String::new(),
        rate: 0,
        tab: 1,
        tract: Default::default(),
    });
    h.frames(2);
    let d = crate::tract::render(&Default::default(), 22050).expect("vocal tract");
    h.app.speech_finish(std::sync::Arc::new(d.clone()), 1);
    assert!(
        h.app.speech_previewing(),
        "preview plays on the separate preview player"
    );
    assert!(h.app.preview_audio.is_none(), "the transport is not used");
    assert!(!h.app.is_playing(), "the playhead and tracks do not play");
    assert!(h.app.files.is_empty(), "preview adds no file");
    // when the speech ends, the button goes back to Play by itself
    h.app.engine.lock().files_preview = None;
    assert!(!h.app.speech_previewing());
    h.app.speech_finish(std::sync::Arc::new(d), 2);
    assert_eq!(h.app.files.len(), 1);
    assert!(
        matches!(h.app.dialog, Some(crate::app::Dialog::Speech { .. })),
        "the window stays open"
    );
    h.frames(2);

    let dir = std::env::temp_dir().join(format!("ac-batch-bg-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let inp = dir.join("in.wav");
    let t = crate::dsp::gen_tone(8000, 1, 0.2, 440.0, crate::dsp::Wave::Sine, -12.0);
    crate::audio::save_wav(&inp, &t, 16, false).expect("write");
    h.app.batch_start(
        vec![inp],
        vec!["process.normalize=-1".into()],
        dir.join("out"),
        "_b".into(),
    );
    let mut msg = None;
    for _ in 0..400 {
        if let Some(m) = h.app.batch_poll() {
            msg = Some(m);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let msg = msg.expect("the batch finishes");
    assert!(msg.contains("Processed 1 of 1"), "{msg}");
    assert!(
        h.app.extra.batch_lines[0].starts_with("Done"),
        "{:?}",
        h.app.extra.batch_lines
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every panel drawn in every workspace, at a normal and a cramped window size.
#[test]
fn every_workspace_and_panel_draws_at_any_size() {
    for (w, hgt) in [(1600.0, 1000.0), (640.0, 400.0), (240.0, 160.0)] {
        let mut h = Harness::new(true, false).with_size(w, hgt);
        for name in crate::workspace::BUILT_IN {
            h.app.set_workspace(name);
            h.frames(2);
        }
        for p in crate::workspace::PanelId::ALL {
            h.app.show_panel(p);
            h.frames(1);
        }
        h.app.switch_mode(Mode::Multitrack);
        h.frames(2);
    }
}

/// Undo and redo past both ends, and edits on an empty selection, never panic.
#[test]
fn undo_redo_past_the_ends_and_edits_without_a_selection() {
    let mut h = Harness::new(true, false);
    for _ in 0..5 {
        let _ = h.run("edit.redo");
    }
    for id in [
        "edit.select_all",
        "process.reverse",
        "process.invert",
        "edit.deselect",
        "edit.delete",
        "edit.cut",
        "edit.paste",
        "edit.crop",
    ] {
        let _ = h.run(id);
        h.frames(1);
    }
    for _ in 0..20 {
        let _ = h.run("edit.undo");
    }
    for _ in 0..20 {
        let _ = h.run("edit.redo");
    }
    h.frames(2);
}

/// Clicking all over the window (menus, panels, editor, transport) never panics.
#[test]
fn clicking_everywhere_never_panics() {
    for multi in [false, true] {
        let mut h = Harness::new(true, multi);
        let mut y = 6.0;
        while y < 1000.0 {
            let mut x = 8.0;
            while x < 1600.0 {
                h.click(egui::pos2(x, y));
                // a click may open a dialog or menu; close it so the grid keeps testing the window
                h.frames_with(
                    1,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    }],
                );
                h.settle();
                x += 97.0;
            }
            y += 61.0;
        }
    }
}

/// The Effects Rack takes more than ten effects, and every slot keeps working.
#[test]
fn effects_rack_grows_past_ten_slots() {
    let mut h = Harness::new(true, false);
    let id = h.app.current.expect("demo file");
    let t = crate::rack::RackTarget::File(id);
    for _ in 0..crate::rack::MAX_SLOTS + 3 {
        h.app
            .rack_insert(t, None, crate::dsp::Effect::Amplify { gain_db: -1.0 });
    }
    let n = h.app.rack_get(t).map(|r| r.1.len()).unwrap_or(0);
    assert_eq!(
        n,
        crate::rack::MAX_SLOTS,
        "the rack fills up to its maximum and then refuses"
    );
    h.app.rack_window = None;
    h.frames(3);
    h.app.apply_rack(id);
    h.frames(1);
}

/// Odd audio: an empty file, a single sample, eight channels, a 2 kHz rate and samples that are
/// NaN, infinite or far beyond full scale. Every command runs on each without panicking.
#[test]
fn every_command_on_odd_audio() {
    use crate::audio::AudioData;
    let odd: Vec<(&str, AudioData)> = vec![
        ("empty", AudioData::new(48000, 2, 0)),
        ("one sample", AudioData::new(48000, 1, 1)),
        ("eight channels", AudioData::new(44100, 8, 4410)),
        ("2 kHz", AudioData::new(2000, 1, 3000)),
        ("broken samples", {
            let mut d = AudioData::new(48000, 2, 9600);
            for (i, s) in d.ch[0].iter_mut().enumerate() {
                *s = match i % 4 {
                    0 => f32::NAN,
                    1 => f32::INFINITY,
                    2 => -1e9,
                    _ => 0.5,
                };
            }
            d
        }),
    ];
    let ids: Vec<&'static str> = crate::commands::all().map(|c| c.id).collect();
    let mut failed: Vec<String> = vec![];
    for (label, data) in odd {
        let mut h = Harness::new(false, false);
        h.app
            .add_file(label.to_string(), None, data.clone(), 32, true, "Open");
        h.frames(2);
        for id in &ids {
            if skipped(id) {
                continue;
            }
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = h.run(id);
                h.frames(2);
                h.settle();
            }));
            if r.is_err() {
                failed.push(format!("{id} on {label}"));
                h = Harness::new(false, false);
                h.app
                    .add_file(label.to_string(), None, data.clone(), 32, true, "Open");
            }
        }
    }
    assert!(failed.is_empty(), "panicked: {failed:?}");
}

/// Every effect's defaults, run over odd audio and odd ranges, keep the output finite.
#[test]
fn every_effect_survives_odd_audio_and_ranges() {
    use crate::audio::AudioData;
    use crate::dsp::{Effect, EffectSlot};
    for e in Effect::all_defaults() {
        let name = e.name().to_string();
        let slots = vec![EffectSlot::new(e)];
        for (rate, chans, n) in [
            (48000u32, 2usize, 0usize),
            (48000, 1, 1),
            (8000, 6, 777),
            (192000, 2, 4096),
        ] {
            let mut d = AudioData::new(rate, chans, n);
            for c in d.ch.iter_mut() {
                for (i, s) in c.iter_mut().enumerate() {
                    *s = if i % 97 == 3 {
                        f32::NAN
                    } else {
                        ((i as f32) * 0.37).sin() * 1.5
                    };
                }
            }
            for (a, b) in [
                (0, n),
                (n / 2, n / 2),
                (n, n),
                (n / 3, n + 50),
                (n + 10, n + 5),
            ] {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut x = d.clone();
                    crate::dsp::apply_chain(&mut x, a, b, &slots);
                    x
                }));
                let x = r.unwrap_or_else(|_| {
                    panic!("{name} panicked on {chans} ch x {n} @ {rate} Hz, range {a}..{b}")
                });
                let (pa, pb) = x.span(a, b);
                assert!(
                    x.ch.iter().all(|c| c[pa..pb].iter().all(|v| v.is_finite())),
                    "{name} left NaN/inf on {chans} ch x {n} @ {rate} Hz, range {a}..{b}"
                );
            }
        }
    }
}

/// Every offline process takes any range (reversed, past the end, empty) on any audio
/// (empty, one sample, NaN) without panicking.
#[test]
fn every_process_takes_any_range() {
    use crate::audio::AudioData;
    use crate::dsp;
    use crate::procs;
    let mut failed: Vec<String> = vec![];
    for (chans, n) in [(2usize, 0usize), (1, 1), (2, 4800), (6, 1000)] {
        let mut d = AudioData::new(48000, chans, n);
        for c in d.ch.iter_mut() {
            for (i, s) in c.iter_mut().enumerate() {
                *s = if i % 53 == 7 {
                    f32::NAN
                } else {
                    ((i as f32) * 0.11).sin() * 0.8
                };
            }
        }
        let prof = dsp::capture_noise(&d, 0, n);
        for (a, b) in [
            (0, n),
            (n / 2, n / 2),
            (n, n),
            (n / 3, n + 99),
            (n + 10, n + 5),
            (n / 2 + 1, n / 4),
        ] {
            let mut run = |name: &str, f: &dyn Fn(&mut AudioData)| {
                let mut x = d.clone();
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut x))).is_err() {
                    failed.push(format!("{name} {chans}ch x {n} range {a}..{b}"));
                }
            };
            run("normalize", &|x| dsp::normalize(x, a, b, -1.0));
            run("fade", &|x| {
                dsp::fade(x, a, b, true, dsp::FadeCurve::Linear)
            });
            run("fade_pow", &|x| dsp::fade_pow(x, a, b, false, 2.0));
            run("invert", &|x| dsp::invert(x, a, b));
            run("reverse", &|x| dsp::reverse(x, a, b));
            run("silence", &|x| dsp::silence(x, a, b));
            run("remove_dc", &|x| dsp::remove_dc(x, a, b));
            run("change_speed", &|x| {
                let _ = dsp::change_speed(x, a, b, 1.5);
            });
            run("match_loudness", &|x| dsp::match_loudness(x, a, b, -23.0));
            run("capture_noise", &|x| {
                let _ = dsp::capture_noise(x, a, b);
            });
            run("noise_reduce", &|x| {
                dsp::noise_reduce(x, a, b, &prof, 0.8, 20.0)
            });
            run("stats", &|x| {
                let _ = dsp::stats(x, a, b);
            });
            run("stretch_range", &|x| {
                let _ = dsp::stretch_range(x, a, b, 1.3);
            });
            run("pitch_range", &|x| {
                let _ = dsp::pitch_range(x, a, b, 3.0);
            });
            run("envelope", &|x| {
                procs::envelope(x, a, b, &[(0.0, 1.0), (0.5, 0.2), (1.0, 1.0)], false)
            });
            run("quiet_spans", &|x| {
                let _ = procs::quiet_spans(x, a, b, -40.0, 0.01);
            });
            run("delete_silence", &|x| {
                let _ = procs::delete_silence(x, a, b, -40.0, 0.01, 0.005);
            });
            run("mark_audio", &|x| {
                let _ = procs::mark_audio(x, a, b, -40.0, 0.01);
            });
            run("phase_correct", &|x| {
                let _ = procs::phase_correct(x, a, b, 5.0);
            });
            run("pitch_bend", &|x| {
                let _ = procs::pitch_bend(x, a, b, 0.0, 2.0);
            });
            run("auto_heal", &|x| {
                let _ = procs::auto_heal(x, a, b);
            });
            run("declip", &|x| {
                let _ = crate::workflows::declip(x, a, b, 0.9);
            });
            run("analyze", &|x| {
                let _ = crate::workflows::analyze(x, a, b);
            });
        }
    }
    assert!(
        failed.is_empty(),
        "{} panicked:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

/// Frame-time check while a real clip plays:
/// `AUDIOCRAFT_VIDEO_TIMING=clip.mp4 cargo test ... video_playback_frame_times -- --ignored --nocapture`.
#[test]
#[ignore]
fn video_playback_frame_times() {
    let Some(p) = std::env::var_os("AUDIOCRAFT_VIDEO_TIMING") else {
        return;
    };
    let mut h = Harness::new(false, false);
    h.app.show_panel(crate::workspace::PanelId::Video);
    h.app.open_path(&PathBuf::from(&p));
    let t0 = std::time::Instant::now();
    while t0.elapsed().as_secs() < 60 {
        h.frames(1);
        if matches!(&h.app.video, VideoState::Ready(m) if m.complete)
            && h.app.cur().map(|f| f.data.frames() > 0).unwrap_or(false)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    eprintln!("imported in {} ms", t0.elapsed().as_millis());
    let start = 300.0;
    {
        let mut e = h.app.engine.lock();
        e.playing = true;
        e.pos = start;
    }
    let t1 = std::time::Instant::now();
    let (mut worst, mut total, mut n, mut restarts, mut last_start, mut shown) =
        (0.0f64, 0.0f64, 0usize, 0usize, f64::NAN, 0usize);
    let mut last_serial = 0;
    while t1.elapsed().as_secs_f64() < 6.0 {
        h.app.engine.lock().pos = start + t1.elapsed().as_secs_f64();
        let f0 = std::time::Instant::now();
        h.frames(1);
        let ms = f0.elapsed().as_secs_f64() * 1000.0;
        worst = worst.max(ms);
        total += ms;
        n += 1;
        if let Some(ps) = &h.app.extra.play_stream {
            let b = ps.behind(start + t1.elapsed().as_secs_f64());
            if ps.serial < last_serial || (ps.serial == 0 && last_serial > 0) {
                restarts += 1;
            }
            if ps.serial > last_serial {
                shown += ps.serial - last_serial;
            }
            last_serial = ps.serial;
            let _ = (b, &mut last_start);
            last_start = ps.fps;
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    let ps = h.app.extra.play_stream.as_ref();
    eprintln!(
        "frames {n}, mean {:.1} ms, worst {:.1} ms, restarts {restarts}, pictures shown {shown} in 6 s, stream {:?}",
        total / n as f64,
        worst,
        ps.map(|s| (s.fps, s.width))
    );
    h.app.engine.lock().playing = false;
    h.frames(1);
}

#[test]
fn a_dropped_folder_opens_the_media_inside_it() {
    let dir = std::env::temp_dir().join(format!("audiocraft-folder-test-{}", std::process::id()));
    let sub = dir.join("takes");
    let _ = std::fs::create_dir_all(&sub);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    for p in [dir.join("a.wav"), sub.join("b.wav")] {
        let mut wav = hound::WavWriter::create(&p, spec).expect("wav");
        for _ in 0..800 {
            wav.write_sample(0i16).expect("sample");
        }
        wav.finalize().expect("finalize");
    }
    let _ = std::fs::write(dir.join("notes.txt"), "not audio");
    let found = App::expand_folders(vec![dir.clone()]);
    assert_eq!(found, vec![dir.join("a.wav"), sub.join("b.wav")]);
    let mut h = Harness::new(false, false);
    h.app.open_path(&dir);
    assert_eq!(
        h.app.files.len(),
        2,
        "both files in the folder are imported, the text file is skipped"
    );
    for _ in 0..200 {
        h.frames(1);
        if h.app.imports.is_empty() {
            break;
        }
    }
    assert!(h.app.files.iter().all(|f| f.data.frames() == 800));
    let empty = dir.join("empty");
    let _ = std::fs::create_dir_all(&empty);
    h.app.open_path(&empty);
    assert_eq!(h.app.files.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Regression: the Files panel's Insert button used to switch the view and then unwrap a
/// session that was never created, crashing on a fresh app. It must now create the session,
/// place the clip and stay usable.
#[test]
fn inserting_a_file_with_no_session_creates_a_session_and_a_clip() {
    let mut h = Harness::new(true, false);
    let id = h.app.files.first().map(|f| f.id).expect("demo file");
    // The demo data includes an example session; the reported crash needs a truly empty app.
    h.app.session = None;
    h.app.session_hist.clear();
    h.app.session_hist_pos = 0;
    h.app.mode = Mode::Waveform;
    assert!(
        h.app.session.is_none(),
        "the harness starts without a session"
    );

    h.app.selected_file = Some(id);
    h.app.insert_into_multitrack(id);
    h.frames(2);

    let s = h
        .app
        .session
        .as_ref()
        .expect("a session was created for the insert");
    assert!(!s.tracks.is_empty(), "the new session has tracks");
    assert!(
        s.tracks
            .iter()
            .any(|t| t.clips.iter().any(|c| c.file_id == id)),
        "the file became a clip"
    );
    assert_eq!(h.app.mode, Mode::Multitrack);
}

/// The same path through the command palette / menu item must be safe too.
#[test]
fn insert_selected_command_works_without_a_session() {
    let mut h = Harness::new(true, false);
    let id = h.app.files.first().map(|f| f.id).expect("demo file");
    h.app.session = None;
    h.app.mode = Mode::Waveform;
    h.app.selected_file = Some(id);
    h.run("file.insert_selected").expect("the command succeeds");
    h.frames(2);
    assert!(h.app.session.is_some(), "the command created a session");
    assert_eq!(h.app.mode, Mode::Multitrack);
}

/// The Files right-click menu is made of the same registered menu items as the menu bar,
/// never the pill-shaped dialog buttons that made it look foreign.
#[test]
fn files_context_menu_uses_registered_menu_items() {
    for id in [
        "file.open_selected",
        "file.insert_selected",
        "file.close_selected",
    ] {
        let cmd = crate::commands::find(id).unwrap_or_else(|| panic!("{id} is registered"));
        assert!(!cmd.title.is_empty(), "{id} has a title");
        assert!(
            !cmd.doc.is_empty(),
            "{id} has help text for the menu tooltip"
        );
    }
    let ctx = egui::Context::default();
    let mut rects = Vec::new();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        rects.push(
            crate::theme::menu_button(
                ui,
                true,
                "Insert into Multitrack",
                "",
                "Put the file on a track",
            )
            .rect,
        );
        rects.push(
            crate::theme::menu_button(ui, true, "Close Selected File", "", "Close this file").rect,
        );
    });
    out.textures_delta.clear();
    assert!(
        rects.iter().all(|r| r.height() <= 22.0),
        "menu items are the normal 20 px height: {rects:?}"
    );
    assert!(
        rects.iter().all(|r| r.width() < 190.0),
        "menu items are not fixed-width dialog pills: {rects:?}"
    );
}

/// The Files preview is its own player: starting it must not change the editor's playhead,
/// the engine's source or position, nor mark the main transport as playing.
#[test]
fn file_preview_never_moves_the_playhead() {
    let mut h = Harness::new(true, false);
    let id = h.app.files.first().map(|f| f.id).expect("demo file");
    h.app.current = Some(id);
    h.app.selected_file = Some(id);
    if let Some(f) = h.app.files.iter_mut().find(|f| f.id == id) {
        f.cursor = 1.25;
    }
    let before_pos = h.app.engine.lock().pos;
    let before_source_kind = match h.app.engine.lock().source {
        crate::engine::Source::None => 0,
        crate::engine::Source::File(_) => 1,
        crate::engine::Source::Mix(_) => 2,
    };
    h.app.start_file_preview(id);
    h.frames(1);
    assert_eq!(h.app.cursor(), 1.25, "the editor playhead did not move");
    let s = h.app.engine.lock();
    assert!(!s.playing, "the main transport did not start");
    assert_eq!(s.pos, before_pos, "the engine's position did not jump");
    let after_source_kind = match &s.source {
        crate::engine::Source::None => 0,
        crate::engine::Source::File(_) => 1,
        crate::engine::Source::Mix(_) => 2,
    };
    assert_eq!(
        after_source_kind, before_source_kind,
        "the main source was not replaced"
    );
    assert!(s.files_preview.is_some(), "the browser has its own preview");
}

/// The stop control belongs to the file browser: it ends the preview without calling the
/// editor's transport stop (which would rewind the playhead when Return-on-Stop is set).
#[test]
fn file_preview_stop_is_manual_and_local() {
    let mut h = Harness::new(true, false);
    let id = h.app.files.first().map(|f| f.id).expect("demo file");
    h.app.selected_file = Some(id);
    h.app.start_file_preview(id);
    assert!(h.app.files_preview.is_some(), "preview started");
    assert!(
        h.app.engine.lock().files_preview.is_some(),
        "the independent player has the audio"
    );
    h.run("files.play").expect("Play toggles the preview off");
    assert!(
        h.app.files_preview.is_none(),
        "the browser stopped the preview"
    );
    assert!(
        h.app.engine.lock().files_preview.is_none(),
        "the audio stopped"
    );
}

/// With an effect picked last in the Effects Rack, Delete and Backspace remove that effect and
/// leave every clip in place.
#[test]
fn delete_and_backspace_remove_the_picked_effect_not_a_clip() {
    use crate::dsp::{Effect, EffectSlot};
    for key in ["delete", "backspace"] {
        let mut h = Harness::new(true, true);
        h.app.rack_clip_tab = false;
        let t = h.app.rack_target().expect("a track to hold effects");
        h.app.rack_set(
            t,
            vec![
                EffectSlot::new(Effect::Amplify { gain_db: -3.0 }),
                EffectSlot::new(Effect::Amplify { gain_db: -6.0 }),
            ],
            None,
        );
        let clips_before: usize = h
            .app
            .session
            .as_ref()
            .unwrap()
            .tracks
            .iter()
            .map(|t| t.clips.len())
            .sum();
        h.app.rack_selected = Some(0);
        h.app.rack_focus = true;
        let ctx = h.ctx.clone();
        h.app.run(key, &ctx);
        let (_, slots, _) = h.app.rack_get(t).unwrap();
        assert_eq!(slots.len(), 1, "{key} removed the picked effect");
        let clips_after: usize = h
            .app
            .session
            .as_ref()
            .unwrap()
            .tracks
            .iter()
            .map(|t| t.clips.len())
            .sum();
        assert_eq!(clips_before, clips_after, "{key} left the clips alone");
    }
}

/// Backspace after clicking in the Files panel clears its selection; it closes nothing.
#[test]
fn backspace_clears_the_files_selection() {
    let mut h = Harness::new(true, true);
    let ids: Vec<u64> = h.app.files.iter().map(|f| f.id).collect();
    assert!(!ids.is_empty());
    h.app.files_sel = ids.clone();
    h.app.selected_file = ids.first().copied();
    h.app.files_focus = true;
    let n = h.app.files.len();
    let ctx = h.ctx.clone();
    h.app.run("backspace", &ctx);
    assert!(h.app.files_sel.is_empty(), "the selection is cleared");
    assert_eq!(h.app.selected_file, None);
    assert_eq!(h.app.files.len(), n, "no file was closed");
}

/// Preferences show at once while the window is open, and Cancel puts the old ones back.
#[test]
fn preferences_apply_live_and_cancel_restores() {
    let mut h = Harness::new(false, false);
    let before = h.app.prefs.clone();
    h.app.open_prefs("Appearance");
    let mut p = before.clone();
    p.color_theme = 3;
    p.return_on_stop = !before.return_on_stop;
    if let Some((edit, _, _)) = &mut h.app.prefs_edit {
        *edit = p;
    }
    h.frames(1);
    assert_eq!(
        crate::theme::theme_kind(),
        3,
        "the theme changed without OK"
    );
    assert_eq!(h.app.prefs.return_on_stop, !before.return_on_stop);
    // Escape cancels
    h.frames_with(
        1,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    assert!(h.app.prefs_edit.is_none());
    assert_eq!(h.app.prefs, before, "Cancel restored the preferences");
    assert_eq!(crate::theme::theme_kind(), before.color_theme);
    crate::theme::set_theme(0);
}

/// Five clipboards: switching keeps what each one holds.
#[test]
fn clipboards_keep_their_own_audio() {
    let mut h = Harness::new(true, false);
    h.app.clipboard = Some(crate::audio::AudioData::new(48000, 1, 10));
    h.run("edit.clipboard_2").unwrap();
    assert!(h.app.clipboard.is_none(), "clipboard 2 starts empty");
    h.app.clipboard = Some(crate::audio::AudioData::new(48000, 2, 20));
    h.run("edit.clipboard_1").unwrap();
    assert_eq!(h.app.clipboard.as_ref().map(|d| d.frames()), Some(10));
    h.run("edit.clipboard_2").unwrap();
    assert_eq!(h.app.clipboard.as_ref().map(|d| d.frames()), Some(20));
}

/// Clip > Clip Gain and the Metronome submenu change the session and can be undone.
#[test]
fn clip_gain_and_metronome_commands() {
    let mut h = Harness::new(true, true);
    let cid = h.app.session.as_ref().unwrap().tracks[0].clips[0].id;
    h.app.session.as_mut().unwrap().selected_clip = Some(cid);
    let g0 = h.app.session.as_ref().unwrap().tracks[0].clips[0].gain_db;
    h.run("clip.gain_up").unwrap();
    let g1 = {
        let s = h.app.session.as_ref().unwrap();
        let (t, i) = s.find_clip(cid).unwrap();
        s.tracks[t].clips[i].gain_db
    };
    assert!((g1 - g0 - 1.0).abs() < 1e-4);
    h.app.undo();
    let g2 = {
        let s = h.app.session.as_ref().unwrap();
        let (t, i) = s.find_clip(cid).unwrap();
        s.tracks[t].clips[i].gain_db
    };
    assert!((g2 - g0).abs() < 1e-4, "undo restores the gain");
    h.run("clip.bring_front").unwrap();
    h.run("clip.fade_in_remove").unwrap();
    h.run("metronome.beats_3").unwrap();
    h.run("metronome.tempo_up").unwrap();
    let m = &h.app.session.as_ref().unwrap().metronome;
    assert_eq!(m.beats, 3);
    assert!((m.bpm - 125.0).abs() < 1e-3);
    h.frames(2);
}

/// Preview resolution: Native never asks for more pixels than the source has.
#[test]
fn video_preview_resolution_choices() {
    let info = crate::video::VideoInfo {
        duration: 1.0,
        width: 3840,
        height: 2160,
        fps: 30.0,
    };
    crate::video::set_preview_res(3);
    assert_eq!(crate::video::res_width(&info, 640), 3840);
    crate::video::set_preview_res(2);
    assert_eq!(crate::video::res_width(&info, 640), 1920);
    crate::video::set_preview_res(0);
    assert_eq!(crate::video::res_width(&info, 640), 640);
    let small = crate::video::VideoInfo { width: 320, ..info };
    crate::video::set_preview_res(3);
    assert_eq!(crate::video::res_width(&small, 640), 320);
    crate::video::set_preview_res(0);
}

/// Clip > Group: grouped clips select together, the group survives undo, and Ungroup ends it.
#[test]
fn grouped_clips_select_and_move_together() {
    let mut h = Harness::new(true, true);
    let (a, b) = {
        let s = h.app.session.as_ref().unwrap();
        (s.tracks[0].clips[0].id, s.tracks[1].clips[0].id)
    };
    {
        let s = h.app.session.as_mut().unwrap();
        s.selected_clips = vec![a, b];
        s.selected_clip = Some(a);
    }
    h.run("clip.group").unwrap();
    // (moving a group together is the clip drag's job: see selected_clips_drag_as_one_unit)
    let s = h.app.session.as_mut().unwrap();
    assert_eq!(s.group_of(a).len(), 2);
    s.selected_clips.clear();
    s.select_group_of(b);
    assert_eq!(s.selected_clips.len(), 2, "picking one selects the group");
    h.run("clip.ungroup").unwrap();
    assert_eq!(h.app.session.as_ref().unwrap().group_of(a), vec![a]);
}

/// Tracks panel buttons: show all, hide all, only the selected one, hide empty ones.
#[test]
fn track_visibility_commands() {
    let mut h = Harness::new(true, true);
    let n = h.app.session.as_ref().unwrap().tracks.len();
    h.run("tracks.hide_all").unwrap();
    assert!(
        h.app
            .session
            .as_ref()
            .unwrap()
            .tracks
            .iter()
            .all(|t| t.hidden)
    );
    h.run("tracks.show_all").unwrap();
    assert!(
        h.app
            .session
            .as_ref()
            .unwrap()
            .tracks
            .iter()
            .all(|t| !t.hidden)
    );
    h.app.session.as_mut().unwrap().selected_track = 1;
    h.run("tracks.only_selected").unwrap();
    let shown: Vec<usize> = h
        .app
        .session
        .as_ref()
        .unwrap()
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.hidden)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(shown, vec![1]);
    h.run("tracks.show_all").unwrap();
    h.run("tracks.hide_empty").unwrap();
    let s = h.app.session.as_ref().unwrap();
    assert!(s.tracks.iter().all(|t| t.hidden == t.clips.is_empty()));
    assert_eq!(s.tracks.len(), n, "hiding never removes a track");
    h.frames(2);
}

/// Essential Sound opened on a clip's file: Back (or Esc) returns to the session untouched.
#[test]
fn essential_sound_backs_out_to_the_session() {
    let mut h = Harness::new(true, true);
    let cid = h.app.session.as_ref().unwrap().tracks[0].clips[0].id;
    h.app.session.as_mut().unwrap().selected_clip = Some(cid);
    h.app.edit_selected_clip_source();
    assert_eq!(h.app.mode, Mode::Waveform);
    assert!(h.app.essential.from_session);
    h.frames_with(
        2,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    assert_eq!(h.app.mode, Mode::Multitrack, "Esc went back to the session");
    assert!(!h.app.essential.from_session);
}

/// Esc closes the topmost non-critical window (an effect's settings, Help, analysis), and a
/// dialog as Cancel; with nothing open it clears the selection as before.
#[test]
fn escape_closes_popups_first() {
    let esc = || egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    };
    let mut h = Harness::new(true, false);
    h.app.panels.stats = true;
    h.app.extra.help_topic = Some("");
    h.frames_with(1, vec![esc()]);
    assert!(!h.app.panels.stats, "the analysis window closed first");
    assert!(h.app.extra.help_topic.is_some(), "one window per press");
    h.frames_with(1, vec![esc()]);
    assert!(h.app.extra.help_topic.is_none());
    h.app.dialog = Some(crate::app::Dialog::Normalize { db: -1.0 });
    h.frames_with(2, vec![esc()]);
    assert!(h.app.dialog.is_none(), "Esc cancels a dialog");
}

/// Saving a session writes changed audio beside it; two sources with the same name, or an
/// unrelated file already there, are never overwritten.
#[test]
fn session_save_never_overwrites_same_named_audio() {
    let dir = std::env::temp_dir().join(format!("ac-save-names-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let other = dir.join("take.wav");
    std::fs::write(&other, b"not ours").unwrap();
    let mut h = Harness::new(false, true);
    for _ in 0..2 {
        let d = crate::dsp::gen_tone(8000, 1, 0.1, 440.0, crate::dsp::Wave::Sine, -12.0);
        h.app.add_file("take.wav".into(), None, d, 16, true, "test");
        let id = h.app.files.last().unwrap().id;
        h.app.insert_into_multitrack(id);
    }
    h.app.session.as_mut().unwrap().path = Some(dir.join("s.acsession"));
    h.app.save_session(false);
    assert_eq!(
        std::fs::read(&other).unwrap(),
        b"not ours",
        "the existing file is untouched"
    );
    assert!(dir.join("take (2).wav").exists());
    assert!(dir.join("take (3).wav").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A menu opened from the menu bar stays open while the pointer is on it or near it, and
/// closes about half a second after the pointer has left it.
#[test]
fn menu_bar_menus_close_after_hovering_away() {
    let mut h = Harness::new(false, false);
    let mut t = 10.0f64;
    let mut frame = |h: &mut Harness, events: Vec<egui::Event>, t: &mut f64| {
        *t += 0.1;
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))),
            events,
            time: Some(*t),
            ..Default::default()
        };
        let (app, fr) = (&mut h.app, &mut h.frame);
        let mut out = h.ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, fr));
        out.textures_delta.clear();
    };
    let file = Pos2::new(18.0, 12.0);
    let btn = |pressed| egui::Event::PointerButton {
        pos: file,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(&mut h, vec![egui::Event::PointerMoved(file)], &mut t);
    frame(&mut h, vec![btn(true)], &mut t);
    frame(&mut h, vec![btn(false)], &mut t);
    frame(&mut h, vec![], &mut t);
    assert!(egui::Popup::is_any_open(&h.ctx), "the File menu opened");
    // just beside the bar: stays open
    frame(
        &mut h,
        vec![egui::Event::PointerMoved(Pos2::new(18.0, 40.0))],
        &mut t,
    );
    for _ in 0..8 {
        frame(&mut h, vec![], &mut t);
    }
    assert!(egui::Popup::is_any_open(&h.ctx), "stays open near the menu");
    // far away: closes after the delay, not at once
    frame(
        &mut h,
        vec![egui::Event::PointerMoved(Pos2::new(1200.0, 800.0))],
        &mut t,
    );
    assert!(egui::Popup::is_any_open(&h.ctx), "not closed instantly");
    for _ in 0..8 {
        frame(&mut h, vec![], &mut t);
    }
    assert!(
        !egui::Popup::is_any_open(&h.ctx),
        "closed after hovering away"
    );
}

/// Every session edit is its own undo step: an edit that does not record itself is caught once
/// the mouse is let go, so one Ctrl+Z undoes it alone (not the action before it too).
#[test]
fn each_edit_is_its_own_undo_step() {
    let mut h = Harness::new(true, true);
    let cid = h.app.session.as_ref().unwrap().tracks[0].clips[0].id;
    h.app.session.as_mut().unwrap().selected_clip = Some(cid);
    let g0 = h.app.session.as_ref().unwrap().tracks[0].clips[0].gain_db;
    h.run("clip.gain_up").unwrap();
    h.frames(1);
    // an edit that records nothing itself (like adding an automation lane or a master keyframe)
    {
        let s = h.app.session.as_mut().unwrap();
        s.master.mute = !s.master.mute;
        s.master_db -= 3.0;
        s.touch();
    }
    h.frames(1);
    let (mute_edit, db_edit) = {
        let s = h.app.session.as_ref().unwrap();
        (s.master.mute, s.master_db)
    };
    h.app.undo();
    let s = h.app.session.as_ref().unwrap();
    assert_eq!(s.master.mute, !mute_edit, "the last edit was undone");
    assert!(
        (s.master_db - (db_edit + 3.0)).abs() < 1e-4,
        "master volume is undone too"
    );
    assert!(
        (s.tracks[0].clips[0].gain_db - g0 - 1.0).abs() < 1e-4,
        "the edit before it stays"
    );
    h.app.undo();
    assert!((h.app.session.as_ref().unwrap().tracks[0].clips[0].gain_db - g0).abs() < 1e-4);
}

/// The person's steps with real mouse clicks: select clip A, click a keyframe on clip B, click
/// a clip again (A, or B which the keyframe selected), press Delete: the clip goes, the keyframe
/// stays. Checked with both the Move and the Time Selection tool.
#[test]
fn clicking_a_clip_after_a_keyframe_makes_delete_remove_the_clip() {
    use crate::keys::EnvTarget;
    for tool in [crate::app::Tool::Move, crate::app::Tool::TimeSel] {
        for click_b in [false, true] {
            let mut h = Harness::new(true, true);
            h.app.tool = tool;
            h.frames(2);
            let (a, b) = {
                let s = h.app.session.as_ref().unwrap();
                (s.tracks[1].clips[0].id, s.tracks[0].clips[0].id)
            };
            {
                let s = h.app.session.as_mut().unwrap();
                let c = &mut s.tracks[0].clips[0];
                let l = c.len;
                c.env = vec![(l * 0.3, 0.0), (l * 0.6, -6.0)];
                c.curves.env_h.clear();
                s.selected_clip = Some(a);
            }
            h.frames(2);
            // click clip A first, the way the person selects it
            let body = |h: &Harness, id: u64| {
                let s = h.app.session.as_ref().unwrap();
                let (ti, ci) = s.find_clip(id).unwrap();
                let c = &s.tracks[ti].clips[ci];
                let (rect, rows, view) = h.app.multi_drag.geom.clone().expect("layout");
                let t = c.start + c.len * 0.15;
                let x = rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
                Pos2::new(x, rows[ti].center().y + 32.0)
            };
            let pa = body(&h, a);
            h.click(pa);
            assert_eq!(h.app.session.as_ref().unwrap().selected_clip, Some(a));
            let spot = h
                .app
                .multi_drag
                .key_spots
                .iter()
                .find(|(t, i, _)| *t == EnvTarget::ClipVol(b) && *i == 0)
                .map(|s| s.2)
                .expect("clip B's keyframe is drawn");
            h.click(spot);
            assert!(
                h.app.multi_drag.key_sel.has(EnvTarget::ClipVol(b), 0),
                "{tool:?}: the keyframe was picked"
            );
            {
                let s = h.app.session.as_ref().unwrap();
                assert_eq!(
                    s.selected_clip, None,
                    "{tool:?}: no clip stays selected with a keyframe"
                );
                assert!(s.selected_clips.is_empty());
            }
            let target = if click_b { b } else { a };
            let p = body(&h, target);
            h.click(p);
            assert!(
                h.app.multi_drag.key_sel.is_empty(),
                "{tool:?} click_b={click_b}: clicking the clip dropped the keyframe"
            );
            assert_eq!(h.app.session.as_ref().unwrap().selected_clip, Some(target));
            h.frames_with(
                2,
                vec![egui::Event::Key {
                    key: egui::Key::Delete,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
            );
            let s = h.app.session.as_ref().unwrap();
            assert!(
                s.find_clip(target).is_none(),
                "{tool:?} click_b={click_b}: the clip was deleted"
            );
            if !click_b {
                let (bt, bi) = s.find_clip(b).unwrap();
                assert_eq!(
                    s.tracks[bt].clips[bi].env.len(),
                    2,
                    "{tool:?}: the keyframe stayed"
                );
            }
        }
    }
}

/// Clicking another clip's volume line adds a keyframe there and that keyframe alone is the
/// selection (the clip picked before is no longer selected), so Delete removes only the new
/// keyframe; selecting clips afterwards (Select All) drops the keyframe again.
#[test]
fn a_keyframe_added_on_a_line_is_the_only_selection() {
    use crate::keys::EnvTarget;
    let mut h = Harness::new(true, true);
    h.app.tool = crate::app::Tool::Move;
    h.frames(2);
    let (a, b) = {
        let s = h.app.session.as_ref().unwrap();
        (s.tracks[1].clips[0].id, s.tracks[0].clips[0].id)
    };
    {
        let s = h.app.session.as_mut().unwrap();
        let c = &mut s.tracks[0].clips[0];
        let l = c.len;
        c.env = vec![(l * 0.3, 0.0), (l * 0.6, -6.0)];
        c.curves.env_h.clear();
        c.pan_env.clear();
        c.curves.pan_h.clear();
        s.selected_clip = Some(a);
    }
    h.frames(2);
    let spot0 = h
        .app
        .multi_drag
        .key_spots
        .iter()
        .find(|(t, i, _)| *t == EnvTarget::ClipVol(b) && *i == 0)
        .map(|s| s.2)
        .expect("clip B's first keyframe is drawn");
    // on the flat part of the line before the first keyframe
    let (x, y) = {
        let s = h.app.session.as_ref().unwrap();
        let c = &s.tracks[0].clips[0];
        let (rect, _, view) = h.app.multi_drag.geom.clone().unwrap();
        let t = c.start + c.len * 0.15;
        (
            rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width(),
            spot0.y,
        )
    };
    h.click(Pos2::new(x, y));
    {
        let s = h.app.session.as_ref().unwrap();
        assert_eq!(
            s.tracks[0].clips[0].env.len(),
            3,
            "a keyframe was added on the line"
        );
        assert_eq!(h.app.multi_drag.key_sel.target, Some(EnvTarget::ClipVol(b)));
        assert_eq!(
            s.selected_clip, None,
            "the clip picked before is no longer selected"
        );
        assert!(s.selected_clips.is_empty());
    }
    let ctx = h.ctx.clone();
    h.app.run("delete", &ctx);
    {
        let s = h.app.session.as_ref().unwrap();
        assert!(s.find_clip(a).is_some(), "clip A stays");
        assert_eq!(
            s.tracks[0].clips[0].env.len(),
            2,
            "only the new keyframe went"
        );
    }
    // pick a keyframe, then select the track's clips from the menu: the keyframe is dropped
    h.app.multi_drag.key_sel.only(EnvTarget::ClipVol(b), 1);
    h.frames(2);
    h.app.exec("edit.select_track_clips", &ctx).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.as_ref().unwrap().selected_clip, Some(b));
    assert!(
        h.app.multi_drag.key_sel.is_empty(),
        "selecting clips drops the keyframes"
    );
}

/// A clip and a keyframe are never selected together: picking a keyframe on clip B selects B
/// (not A), and clicking clip A afterwards drops the keyframe, so Delete removes clip A.
#[test]
fn clip_and_keyframe_selection_exclude_each_other() {
    let mut h = Harness::new(true, true);
    h.app.tool = crate::app::Tool::Move;
    h.frames(2);
    let (a, b, a_start, a_len) = {
        let s = h.app.session.as_ref().unwrap();
        let a = &s.tracks[1].clips[0];
        (a.id, s.tracks[0].clips[0].id, a.start, a.len)
    };
    h.app.session.as_mut().unwrap().selected_clip = Some(a);
    // a keyframe on the other clip (B) is picked
    h.app.session.as_mut().unwrap().tracks[0].clips[0].env = vec![(0.5, 0.0), (1.0, -6.0)];
    h.app
        .multi_drag
        .key_sel
        .only(crate::keys::EnvTarget::ClipVol(b), 0);
    h.frames(2);
    let s = h.app.session.as_ref().unwrap();
    assert_eq!(
        s.selected_clip, None,
        "picking a keyframe deselects every clip"
    );
    assert!(s.selected_clips.is_empty());
    assert_eq!(
        s.selected_track, 0,
        "the keyframe's track is the current one"
    );
    // click clip A in its body, away from the visible pan/envelope line (clicking that line
    // correctly adds a new keyframe instead of just picking the clip)
    let (rect, rows, view) = h.app.multi_drag.geom.clone().expect("layout");
    let t = a_start + a_len * 0.5;
    let x = rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
    let y = rows[1].center().y + 32.0;
    h.click(Pos2::new(x, y));
    assert!(
        h.app.multi_drag.key_sel.is_empty(),
        "clicking the clip dropped the keyframe"
    );
    assert_eq!(h.app.session.as_ref().unwrap().selected_clip, Some(a));
    // Delete removes clip A, and B keeps its keyframes
    let ctx = h.ctx.clone();
    h.app.run("delete", &ctx);
    let s = h.app.session.as_ref().unwrap();
    assert!(s.find_clip(a).is_none(), "clip A was deleted");
    let (bt, bi) = s.find_clip(b).unwrap();
    assert_eq!(
        s.tracks[bt].clips[bi].env.len(),
        2,
        "the keyframe was left alone"
    );
}

/// The person's request: Shift+click on a clip selects every clip in that clip's row,
/// Shift+click on empty lane space selects every empty span of the row, and Ctrl+click
/// toggles the one clip or span clicked. Both work with the black mouse tool (Move)
/// anywhere, and with any other tool only over a clip's top bar.
#[test]
fn shift_and_ctrl_clicks_select_rows_and_toggle_singles() {
    let mut h = Harness::new(true, true);
    h.app.tool = crate::app::Tool::Move;
    h.frames(2);
    let (a, b, c) = {
        let s = h.app.session.as_ref().unwrap();
        (
            s.tracks[1].clips[0].id,
            s.tracks[0].clips[0].id,
            s.tracks[1].clips[1].id,
        )
    };
    // a clip's body (deep enough to stay off its envelopes) and its top bar
    let body = |h: &Harness, id: u64| {
        let s = h.app.session.as_ref().unwrap();
        let (ti, ci) = s.find_clip(id).unwrap();
        let cl = &s.tracks[ti].clips[ci];
        let (rect, rows, view) = h.app.multi_drag.geom.clone().expect("layout");
        let t = cl.start + cl.len * 0.15;
        let x = rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
        Pos2::new(x, rows[ti].center().y + 32.0)
    };
    let top = |h: &Harness, id: u64| {
        let s = h.app.session.as_ref().unwrap();
        let (ti, ci) = s.find_clip(id).unwrap();
        let cl = &s.tracks[ti].clips[ci];
        let (rect, rows, view) = h.app.multi_drag.geom.clone().expect("layout");
        let t = cl.start + cl.len * 0.15;
        let x = rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
        Pos2::new(x, rows[ti].top() + 7.0)
    };
    // empty lane space of a row at time `t`
    let space = |h: &Harness, ti: usize, t: f64| {
        let (rect, rows, view) = h.app.multi_drag.geom.clone().expect("layout");
        let x = rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
        Pos2::new(x, rows[ti].center().y)
    };
    let sel = |h: &Harness| {
        let s = h.app.session.as_ref().unwrap();
        (s.selected_clips.clone(), s.selected_gaps.clone())
    };
    let (pa, pb) = (body(&h, a), body(&h, b));
    // a plain click picks one clip
    h.click(pb);
    assert_eq!(sel(&h).0, vec![b]);
    // Shift+click on a clip in the other row: that whole row joins, the first stays
    h.click_mods(pa, egui::Modifiers::SHIFT);
    {
        let s = h.app.session.as_ref().unwrap();
        assert_eq!(s.selected_clips.len(), 3, "both rows are picked");
        assert!(s.selected_clips.contains(&a) && s.selected_clips.contains(&c));
        assert_eq!(
            s.selected_clip,
            Some(a),
            "the clip clicked is the active one"
        );
    }
    // Shift+click on the same clip again: that clip leaves the selection (the person's
    // rule: Shift+click un-picks what is already selected)
    h.click_mods(pa, egui::Modifiers::SHIFT);
    {
        let s = h.app.session.as_ref().unwrap();
        assert!(
            !s.selected_clips.contains(&a),
            "the picked clip leaves the set"
        );
        assert_eq!(s.selected_clips.len(), 2);
        assert_eq!(s.selected_clip, Some(c), "the active role hands over");
    }
    // a clip outside the set brings its row back in
    h.click_mods(pa, egui::Modifiers::SHIFT);
    assert_eq!(sel(&h).0.len(), 3);
    // Ctrl+click toggles the one clip: out, then back in
    h.click_mods(pa, egui::Modifiers::CTRL);
    assert_eq!(
        sel(&h).0,
        vec![b, c],
        "the clip clicked alone leaves the set"
    );
    h.click_mods(pa, egui::Modifiers::CTRL);
    assert_eq!(sel(&h).0.len(), 3);
    // Shift+click on empty space: every empty span of that row (before the clip and the
    // open end after it), the clips stay picked
    h.click_mods(space(&h, 0, 0.2), egui::Modifiers::SHIFT);
    {
        let s = h.app.session.as_ref().unwrap();
        assert_eq!(s.selected_gaps.len(), 2, "the row's two empty spans");
        assert!(s.selected_gaps.iter().all(|g| g.trk == s.tracks[0].id));
        assert_eq!(s.selected_gaps[0].a, 0.0, "the span before the first clip");
        assert!(s.selected_gaps.iter().any(|g| g.b.is_infinite()));
        assert_eq!(s.selected_clips.len(), 3, "the clips stay picked");
    }
    // Shift+click on a span already picked: that span alone leaves the set
    h.click_mods(space(&h, 0, 0.2), egui::Modifiers::SHIFT);
    {
        let s = h.app.session.as_ref().unwrap();
        assert_eq!(s.selected_gaps.len(), 1, "the span clicked leaves the set");
        assert!(
            s.selected_gaps[0].b.is_infinite(),
            "the open end stays picked"
        );
        assert_eq!(s.selected_clips.len(), 3, "the clips stay picked");
    }
    // an unpicked span brings the row's spans back
    h.click_mods(space(&h, 0, 0.2), egui::Modifiers::SHIFT);
    assert_eq!(sel(&h).1.len(), 2);
    // Ctrl+click on empty space toggles the one span clicked
    h.click_mods(space(&h, 0, 0.2), egui::Modifiers::CTRL);
    {
        let s = h.app.session.as_ref().unwrap();
        assert_eq!(
            s.selected_gaps.len(),
            1,
            "the span clicked alone leaves the set"
        );
        assert!(s.selected_gaps[0].b.is_infinite());
    }
    // a plain click on empty ground clears clips and spans together
    h.click(space(&h, 0, 0.2));
    {
        let s = h.app.session.as_ref().unwrap();
        assert!(s.selected_clips.is_empty() && s.selected_gaps.is_empty());
    }
    // with the Time Selection tool, a Shift+click on a clip's body is the tool's own
    // click: one clip, not the row
    h.app.tool = crate::app::Tool::TimeSel;
    h.frames(2);
    h.click_mods(pa, egui::Modifiers::SHIFT);
    assert_eq!(sel(&h).0, vec![a], "the body keeps the tool's click");
    assert!(
        sel(&h).1.is_empty(),
        "empty space needs the black mouse tool"
    );
    // over the clip's top bar every tool grabs the clip: the row is selected there.
    // c is not picked yet, so its row joins — a Shift+click on the already-picked a
    // would instead un-pick a, the new rule
    h.click_mods(top(&h, c), egui::Modifiers::SHIFT);
    {
        let s = h.app.session.as_ref().unwrap();
        assert_eq!(s.selected_clips.len(), 2, "the top bar selects the row");
        assert!(s.selected_clips.contains(&a) && s.selected_clips.contains(&c));
    }
    // Ctrl+click on the top bar toggles the one clip with any tool
    h.click_mods(top(&h, c), egui::Modifiers::CTRL);
    {
        let s = h.app.session.as_ref().unwrap();
        assert_eq!(
            s.selected_clips,
            vec![a],
            "the toggled clip alone leaves the set"
        );
    }
}

/// The person's request: several selected clips drag as one unit. Both beat clips of the
/// demo's second track get picked with one Shift+click; dragging either of them moves
/// both by the same time and the same rows, and Alt+drag leaves copies of the whole unit
/// behind while the originals stay put.
#[test]
fn selected_clips_drag_as_one_unit() {
    let mut h = Harness::new(true, true);
    h.app.tool = crate::app::Tool::Move;
    h.app.snapping = false;
    h.frames(2);
    let (b1, b2, m) = {
        let s = h.app.session.as_ref().unwrap();
        (
            s.tracks[1].clips[0].id,
            s.tracks[1].clips[1].id,
            s.tracks[0].clips[0].id,
        )
    };
    // a point in a clip's body at time `t`, and one at the same time on another row
    let at = |h: &Harness, id: u64, off: f64| {
        let s = h.app.session.as_ref().unwrap();
        let (ti, ci) = s.find_clip(id).unwrap();
        let c = &s.tracks[ti].clips[ci];
        let (rect, rows, view) = h.app.multi_drag.geom.clone().expect("layout");
        let t = c.start + c.len * 0.15 + off;
        let x = rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
        Pos2::new(x, rows[ti].center().y + 32.0)
    };
    let row_at = |h: &Harness, id: u64, row: usize, off: f64| {
        let s = h.app.session.as_ref().unwrap();
        let (ti, ci) = s.find_clip(id).unwrap();
        let c = &s.tracks[ti].clips[ci];
        let (rect, rows, view) = h.app.multi_drag.geom.clone().expect("layout");
        let t = c.start + c.len * 0.15 + off;
        let x = rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
        Pos2::new(x, rows[row].center().y + 32.0)
    };
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
    let places = |h: &Harness| {
        let s = h.app.session.as_ref().unwrap();
        (
            s.find_clip(b1)
                .map(|(t, c)| (t, s.tracks[t].clips[c].start)),
            s.find_clip(b2)
                .map(|(t, c)| (t, s.tracks[t].clips[c].start)),
            s.find_clip(m).map(|(t, c)| (t, s.tracks[t].clips[c].start)),
        )
    };
    // Shift+click one beat: the whole row is picked
    h.click_mods(at(&h, b1, 0.0), egui::Modifiers::SHIFT);
    {
        let s = h.app.session.as_ref().unwrap();
        assert_eq!(s.selected_clips.len(), 2, "the row's two clips are picked");
    }
    let (o1, o2, om) = places(&h);
    // drag one of them a second right: both move by the same second
    h.drag_mods(at(&h, b1, 0.0), at(&h, b1, 1.0), Default::default());
    let (n1, n2, nm) = places(&h);
    {
        let s = h.app.session.as_ref().unwrap();
        assert_eq!(s.selected_clips.len(), 2, "the unit stays picked");
        assert!(
            near(n1.unwrap().1 - o1.unwrap().1, 1.0),
            "the grabbed clip moved 1s"
        );
        assert!(
            near(n2.unwrap().1 - o2.unwrap().1, 1.0),
            "the other picked clip moved the same 1s"
        );
        assert!(
            near(nm.unwrap().1, om.unwrap().1),
            "the melody on the other track did not move"
        );
        assert_eq!(n1.unwrap().0, o1.unwrap().0, "same rows as before");
    }
    // drag the unit one row down: both clips land on that row together
    h.drag_mods(at(&h, b2, 0.0), row_at(&h, b2, 2, 0.0), Default::default());
    let (n1, n2, _) = places(&h);
    {
        let s = h.app.session.as_ref().unwrap();
        assert!(
            n1.unwrap().0 == 2 && n2.unwrap().0 == 2,
            "both on the third row"
        );
        assert_eq!(s.selected_track, 2);
    }
    // Alt+drag the unit half a second right: the originals stay put and the copies go
    h.drag_mods(
        at(&h, b1, 0.0),
        at(&h, b1, 0.5),
        egui::Modifiers {
            alt: true,
            ..Default::default()
        },
    );
    {
        let s = h.app.session.as_ref().unwrap();
        let row = &s.tracks[2].clips;
        assert_eq!(row.len(), 4, "two originals and two copies");
        let mut starts: Vec<f64> = row.iter().map(|c| c.start).collect();
        starts.sort_by(|a, b| a.total_cmp(b));
        let want = [
            o1.unwrap().1 + 1.0,
            o1.unwrap().1 + 1.5,
            o2.unwrap().1 + 1.0,
            o2.unwrap().1 + 1.5,
        ];
        for (g, w) in starts.iter().zip(want.iter()) {
            assert!(
                near(*g, *w),
                "the copies sit half a second after the originals"
            );
        }
        assert_eq!(
            s.selected_clips.len(),
            2,
            "the copies are the selection now"
        );
    }
}

/// The person's delete rules, pressed like a person presses them: Delete/Backspace
/// removes every picked clip and leaves the space it occupied; Shift+Delete/Shift+
/// Backspace ripples, pulling the later clips of the track left into the closed gap.
#[test]
fn delete_key_removes_all_picked_and_shift_delete_ripples() {
    let mut h = Harness::new(true, true);
    h.app.tool = crate::app::Tool::Move;
    h.frames(2);
    let (b1, b2, m) = {
        let s = h.app.session.as_ref().unwrap();
        (
            s.tracks[1].clips[0].id,
            s.tracks[1].clips[1].id,
            s.tracks[0].clips[0].id,
        )
    };
    let body = |h: &Harness, id: u64, frac: f64| {
        let s = h.app.session.as_ref().unwrap();
        let (ti, ci) = s.find_clip(id).unwrap();
        let c = &s.tracks[ti].clips[ci];
        let (rect, rows, view) = h.app.multi_drag.geom.clone().expect("layout");
        let t = c.start + c.len * frac;
        let x = rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
        Pos2::new(x, rows[ti].center().y + 32.0)
    };
    let key = |h: &mut Harness, key: egui::Key, shift: bool| {
        let mods = if shift {
            egui::Modifiers::SHIFT
        } else {
            egui::Modifiers::NONE
        };
        h.frames_with(
            2,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: mods,
            }],
        );
    };
    let cmd_key = |h: &mut Harness, key: egui::Key| {
        // the real backend marks Ctrl as the command key; a bare `CTRL` would not match
        h.frames_with(
            2,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
        );
    };
    // pick both beats with one Shift+click, then Delete: every picked clip goes
    h.click_mods(body(&h, b1, 0.15), egui::Modifiers::SHIFT);
    key(&mut h, egui::Key::Delete, false);
    {
        let s = h.app.session.as_ref().unwrap();
        assert!(
            s.find_clip(b1).is_none() && s.find_clip(b2).is_none(),
            "both beats went"
        );
        assert!(s.find_clip(m).is_some(), "the melody stays");
        assert!(s.selected_clips.is_empty() && s.selected_clip.is_none());
    }
    // Ctrl+Z brings them back, then Shift+Delete on the first beat alone ripples
    cmd_key(&mut h, egui::Key::Z);
    {
        let s = h.app.session.as_ref().unwrap();
        assert!(
            s.find_clip(b1).is_some() && s.find_clip(b2).is_some(),
            "undo brought them back"
        );
        let (t, c) = s.find_clip(b2).unwrap();
        let old = s.tracks[t].clips[c].start;
        assert!(old > 3.0, "the second beat sits past the first (setup)");
    }
    // click the first beat back in — at a different spot than the Shift+click, so the
    // two presses do not count as one double-click (which would open the clip's source)
    h.click(body(&h, b1, 0.6));
    key(&mut h, egui::Key::Delete, true);
    {
        let s = h.app.session.as_ref().unwrap();
        assert!(s.find_clip(b1).is_none(), "the first beat went");
        let (t, c) = s.find_clip(b2).unwrap();
        let b = &s.tracks[t].clips[c];
        assert!(
            (b.start - 0.5).abs() < 1e-6,
            "the second beat pulled left into the closed gap: {}",
            b.start
        );
    }
    // Backspace works the same way, with and without Shift
    h.click(body(&h, b2, 0.15));
    key(&mut h, egui::Key::Backspace, true);
    assert!(
        h.app.session.as_ref().unwrap().find_clip(b2).is_none(),
        "Shift+Backspace too"
    );
    assert!(
        h.app.session.as_ref().unwrap().find_clip(m).is_some(),
        "the melody still there"
    );
}

/// The person's request: selecting empty space and pressing Delete ripple-deletes the
/// space too — the picked span closes and what follows on that track pulls left.
#[test]
fn delete_on_picked_empty_space_closes_the_gap() {
    let mut h = Harness::new(true, true);
    h.app.tool = crate::app::Tool::Move;
    h.frames(2);
    let (m, m_start) = {
        let s = h.app.session.as_ref().unwrap();
        let c = &s.tracks[0].clips[0];
        (c.id, c.start)
    };
    let space = |h: &Harness, t: f64| {
        let (rect, rows, view) = h.app.multi_drag.geom.clone().expect("layout");
        let x = rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
        Pos2::new(x, rows[0].center().y)
    };
    // the span before the melody and the open tail are the row's two empty spans
    h.click_mods(space(&h, 0.2), egui::Modifiers::SHIFT);
    {
        let s = h.app.session.as_ref().unwrap();
        assert_eq!(s.selected_gaps.len(), 2, "the row's two empty spans");
        assert!(s.selected_clips.is_empty(), "no clip is picked with them");
    }
    // Delete closes the picked space: the melody pulls to the start of the timeline
    h.frames_with(
        2,
        vec![egui::Event::Key {
            key: egui::Key::Delete,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    {
        let s = h.app.session.as_ref().unwrap();
        assert!(s.selected_gaps.is_empty(), "the span selection is spent");
        let (t, c) = s.find_clip(m).unwrap();
        let now = s.tracks[t].clips[c].start;
        assert!(
            now.abs() < 1e-6,
            "the melody pulled left by the closed span (was {}): {}",
            m_start,
            now
        );
    }
}

/// The person's request: holding Shift while dragging locks the move to the main 8
/// directions. A mostly-upward drag moves the clip straight up (its time stands still),
/// a mostly-rightward drag moves it along its own track only, and without Shift the drag
/// is as free as before.
#[test]
fn shift_dragging_a_clip_locks_it_to_the_axes() {
    let mut h = Harness::new(true, true);
    h.app.tool = crate::app::Tool::Move;
    h.app.snapping = false;
    h.frames(2);
    let id = h.app.session.as_ref().unwrap().tracks[1].clips[0].id;
    let body = |h: &Harness| {
        let s = h.app.session.as_ref().unwrap();
        let (ti, ci) = s.find_clip(id).unwrap();
        let c = &s.tracks[ti].clips[ci];
        let (rect, rows, view) = h.app.multi_drag.geom.clone().expect("layout");
        let t = c.start + c.len * 0.15;
        let x = rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
        Pos2::new(x, rows[ti].center().y + 32.0)
    };
    let pa = body(&h);
    let rate = h.app.session.as_ref().unwrap().rate as f64;
    // a mostly-upward drag (a little to the right, far up) with Shift: straight up only
    let up = {
        let (_, rows, _) = h.app.multi_drag.geom.clone().unwrap();
        Pos2::new(pa.x + 10.0, rows[0].center().y + 32.0)
    };
    h.drag_mods(pa, up, egui::Modifiers::SHIFT);
    {
        let s = h.app.session.as_ref().unwrap();
        let (ti, ci) = s.find_clip(id).expect("the clip is still there");
        assert_eq!(ti, 0, "the clip moved to the track above");
        assert!(
            (s.tracks[ti].clips[ci].start - 0.0).abs() < 2.0 / rate,
            "its start time did not move: {}",
            s.tracks[ti].clips[ci].start
        );
    }
    // a mostly-rightward drag (a little down, far right) with Shift: along the track only
    h.frames(2);
    let pb = body(&h);
    let (_, rows, _) = h.app.multi_drag.geom.clone().unwrap();
    let dt = {
        let (rect, _, view) = h.app.multi_drag.geom.clone().unwrap();
        60.0 / rect.width() as f64 * (view.1 - view.0)
    };
    let right = Pos2::new(pb.x + 60.0, pb.y + 10.0);
    h.drag_mods(pb, right, egui::Modifiers::SHIFT);
    {
        let s = h.app.session.as_ref().unwrap();
        let (ti, ci) = s.find_clip(id).unwrap();
        assert_eq!(ti, 0, "the clip stayed on its track");
        let start = s.tracks[ti].clips[ci].start;
        assert!(
            (start - dt).abs() < 2.0 / rate,
            "the clip moved right by the dragged time (want ~{dt}, got {start})"
        );
    }
    // the same diagonal drag without Shift moves both ways, as before
    h.frames(2);
    let pc = body(&h);
    let free = Pos2::new(
        pc.x + 60.0,
        pc.y + (rows[1].center().y - rows[0].center().y).abs(),
    );
    h.drag_mods(pc, free, egui::Modifiers::default());
    {
        let s = h.app.session.as_ref().unwrap();
        let (ti, ci) = s.find_clip(id).unwrap();
        let start = s.tracks[ti].clips[ci].start;
        assert!(ti > 0, "the free drag changed the track too");
        assert!(
            start > dt * 0.5,
            "the free drag changed the time too (got {start})"
        );
    }
}

/// Holding Shift while dragging a keyframe locks it to the main 8 directions as well: a
/// mostly-upward drag changes only its value (the time stands still), while the same drag
/// without Shift moves the keyframe diagonally as before.
#[test]
fn shift_dragging_a_keyframe_locks_it_to_the_axes() {
    use crate::keys::EnvTarget;
    let mut h = Harness::new(true, true);
    h.app.tool = crate::app::Tool::Move;
    h.frames(2);
    let b = h.app.session.as_ref().unwrap().tracks[0].clips[0].id;
    // the demo clip's own volume envelope has a keyframe at its left edge
    let spot = h
        .app
        .multi_drag
        .key_spots
        .iter()
        .find(|(t, i, _)| *t == EnvTarget::ClipVol(b) && *i == 0)
        .map(|s| s.2)
        .expect("clip B's first keyframe is drawn");
    h.drag_mods(
        spot,
        Pos2::new(spot.x + 10.0, spot.y - 40.0),
        egui::Modifiers::SHIFT,
    );
    {
        let s = h.app.session.as_ref().unwrap();
        let c = &s.tracks[0].clips[0];
        let rate = s.rate as f64;
        assert!(
            c.env[0].0.abs() < 2.0 / rate,
            "the keyframe's time did not move: {}",
            c.env[0].0
        );
        assert!(
            c.env[0].1 > 1.0,
            "the keyframe's value moved up instead: {}",
            c.env[0].1
        );
    }
    // and the same drag without Shift is free: both the time and the value change
    h.frames(2);
    let spot = h
        .app
        .multi_drag
        .key_spots
        .iter()
        .find(|(t, i, _)| *t == EnvTarget::ClipVol(b) && *i == 0)
        .map(|s| s.2)
        .expect("the keyframe is drawn again");
    h.drag_mods(
        spot,
        Pos2::new(spot.x + 10.0, spot.y - 40.0),
        egui::Modifiers::default(),
    );
    {
        let s = h.app.session.as_ref().unwrap();
        let c = &s.tracks[0].clips[0];
        assert!(
            c.env[0].0 > 0.02,
            "the free drag moved the time: {}",
            c.env[0].0
        );
        assert!(
            c.env[0].1 > 1.0,
            "the free drag moved the value: {}",
            c.env[0].1
        );
    }
}

// ---- the person's multitrack tools scenario ---------------------------------

fn tools_states(app: &App) -> Vec<(u8, bool)> {
    let metro = app
        .session
        .as_ref()
        .map(|s| s.metronome.on)
        .unwrap_or(false);
    vec![
        (0, app.header_view == 0),
        (1, app.header_view == 1),
        (2, app.header_view == 2),
        (3, app.header_view == 3),
        (4, metro),
        (5, app.stretch_mode),
        (6, app.snapping),
        (7, app.snap_menu_open),
        (8, app.auto_scroll),
    ]
}

/// The person's request: the Multitrack Tools back in the corner where they were, and however
/// tall the tracks grow (3x) and wherever they scroll (to the bottom), the tracks must never
/// make the tools' buttons unpressable. Every button answers after growing and scrolling, every
/// button sits inside the corner panel, and the panel ends where the tracks begin.
#[test]
fn multitrack_tools_stay_clickable_when_tracks_grow_and_scroll() {
    let mut h = Harness::new(true, true);
    h.frames(3);
    // first: every button answers with the tracks at their normal height
    let before = tools_states(&h.app);
    let spots = h.app.multi_drag.tool_spots.clone();
    assert_eq!(spots.len(), 9, "nine tool buttons drawn");
    for (i, (_, r)) in spots.iter().enumerate() {
        h.click(r.center());
        let after = tools_states(&h.app);
        let changed = (0..9u8).any(|k| {
            before.iter().find(|(j, _)| *j == k).unwrap().1
                != after.iter().find(|(j, _)| *j == k).unwrap().1
        });
        println!(
            "default pass button {i} ({}): changed={changed}",
            r.center()
        );
    }
    // grow the tracks 3x and scroll to the bottom, like the person did
    {
        let s = h.app.session.as_mut().unwrap();
        for t in &mut s.tracks {
            t.height = (t.height * 3.0).clamp(40.0, 600.0);
        }
    }
    h.frames(3);
    h.app.multi_drag.scroll_y = f32::MAX; // clamped to the bottom
    h.frames(3);
    println!("scroll_y after clamp: {}", h.app.multi_drag.scroll_y);
    // now every button must still answer, and every button must sit in the corner panel,
    // which ends where the tracks begin, so track rows can never run under it
    let panel = h.app.multi_drag.tool_panel.expect("corner panel recorded");
    let (lanes, _, _) = h.app.multi_drag.geom.clone().expect("layout");
    assert!(
        panel.bottom() <= lanes.top() + 0.5,
        "the corner panel ({panel:?}) ends below the visible track area (top {})",
        lanes.top()
    );
    let spots = h.app.multi_drag.tool_spots.clone();
    assert_eq!(spots.len(), 9, "nine tool buttons still drawn");
    for (i, (_, r)) in spots.iter().enumerate() {
        assert!(
            panel.contains_rect(*r),
            "button {i} ({r:?}) is outside the corner panel ({panel:?})"
        );
        let before = tools_states(&h.app);
        h.click(r.center());
        let after = tools_states(&h.app);
        let changed = (0..9u8).any(|k| {
            before.iter().find(|(j, _)| *j == k).unwrap().1
                != after.iter().find(|(j, _)| *j == k).unwrap().1
        });
        println!(
            "scrolled pass button {i} ({}): changed={changed}",
            r.center()
        );
        if !changed && i != 0 {
            panic!(
                "tools button {i} at {} did not answer after growing the tracks and scrolling to the bottom",
                r.center()
            );
        }
    }
    // and one more pass with the snap-target panel open (the corner hangs over the tracks)
    {
        let (_, r) = spots.iter().find(|(k, _)| *k == 7).unwrap().clone();
        h.click(r.center());
    }
    h.frames(3);
    assert!(h.app.snap_menu_open, "the snap-target panel opened");
    let spots = h.app.multi_drag.tool_spots.clone();
    for (i, (_, r)) in spots.iter().enumerate() {
        let before = tools_states(&h.app);
        h.click(r.center());
        let after = tools_states(&h.app);
        let changed = (0..9u8).any(|k| {
            before.iter().find(|(j, _)| *j == k).unwrap().1
                != after.iter().find(|(j, _)| *j == k).unwrap().1
        });
        println!(
            "menu-open pass button {i} ({}): changed={changed}",
            r.center()
        );
        if !changed && !matches!(i, 0 | 7) {
            panic!(
                "tools button {i} at {} did not answer while the snap-target panel was open",
                r.center()
            );
        }
    }
}

// ---- the pre-render gauge -------------------------------------------------------

fn prerender_on(h: &mut Harness, ti: usize) -> u64 {
    let s = h.app.session.as_mut().unwrap();
    s.tracks[ti].prerender = true;
    s.touch();
    s.tracks[ti].id
}

/// With room in the cache cap the whole session is pre-rendered, a little at a time (the gauge
/// fills while the track still plays live), and playback then uses the rendered audio.
#[test]
fn prerender_fills_the_whole_session_when_it_fits_the_cap() {
    use std::time::Duration;
    let mut h = Harness::new(true, true);
    h.frames(2);
    assert!(h.app.prerender_gauge().is_none(), "no gauge without Pre-render");
    assert!(h.app.multi_drag.prerender_bar.is_none());
    let tid = prerender_on(&mut h, 0);
    assert!(
        h.app.prerender_tick(Duration::ZERO),
        "a render started and has work left after one block"
    );
    let g0 = h.app.prerender_gauge().expect("gauge shown");
    assert!(g0.busy && g0.progress < 0.5, "{g0:?}");
    let first = h.app.session.as_ref().unwrap().tracks[0]
        .clips
        .iter()
        .map(|c| c.start)
        .fold(f64::INFINITY, f64::min);
    assert!(
        (g0.window.0 - first).abs() < 1e-9 && g0.window.1 > 8.0,
        "the whole track, from its first clip: {g0:?}"
    );
    h.app.finish_prerenders();
    let g = h.app.prerender_gauge().unwrap();
    let c = &h.app.prerender[&tid];
    assert!(!g.busy && g.progress > 0.999, "{g:?}");
    assert!(c.start < 1e-9, "starts at the session start");
    assert!(
        (c.rendered_to() - c.end).abs() < 1e-6,
        "rendered to the end: {} vs {}",
        c.rendered_to(),
        c.end
    );
    assert!(g.megabytes > 0.5 && g.megabytes < g.cap_megabytes, "{g:?}");
    // playback plays the rendered stretch, and it sounds like the live track
    let s = h.app.session.as_ref().unwrap();
    let files = &h.app.files;
    let pre = &h.app.prerender;
    let look = |id: u64| files.iter().find(|f| f.id == id).map(|f| f.data.clone());
    let with = s.snapshot_with(&look, &|t| pre.get(&t).map(|c| c.range()));
    assert!(with.tracks[0].pre.is_some(), "the mix carries the render");
    let live = s.snapshot(&look);
    let a = crate::session::mixdown(&with, s.rate, 0.0, 3.0);
    let b = crate::session::mixdown(&live, s.rate, 0.0, 3.0);
    let diff = a.ch[0]
        .iter()
        .zip(&b.ch[0])
        .fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
    assert!(diff < 1e-3, "rendered playback matches live playback (diff {diff})");
    // an edit drops the stale render at once
    {
        let s = h.app.session.as_mut().unwrap();
        s.tracks[0].clips[0].gain_db -= 3.0;
        s.touch();
    }
    h.app.update_prerenders();
    assert!(!h.app.prerender.contains_key(&tid), "stale render forgotten");
    // the bar is drawn on the ruler, and goes away with Pre-render off
    h.frames(2);
    assert!(h.app.multi_drag.prerender_bar.is_some(), "gauge drawn");
    h.app.session.as_mut().unwrap().tracks[0].prerender = false;
    h.frames(2);
    assert!(h.app.prerender_gauge().is_none());
    assert!(h.app.multi_drag.prerender_bar.is_none());
    assert!(h.app.prerender.is_empty());
}

/// A small cache cap: the render follows the playhead, the set amount ahead and behind, drops
/// what fell behind, and starts over where the playhead jumps to.
#[test]
fn prerender_follows_the_playhead_within_the_cap() {
    let mut h = Harness::new(true, true);
    h.frames(2);
    h.app.prefs.prerender_cap_mb = 2.0; // about 5.5 s at 48 kHz: less than the track
    h.app.prefs.prerender_ahead_s = 2.0;
    h.app.prefs.prerender_behind_s = 1.0;
    let tid = prerender_on(&mut h, 0);
    h.app.session.as_mut().unwrap().cursor = 0.0;
    h.app.finish_prerenders();
    let c = &h.app.prerender[&tid];
    assert!(c.start < 1e-9, "{}", c.start);
    let ready = c.rendered_to();
    assert!((2.0..2.5).contains(&ready), "2 s ahead of the playhead: {ready}");
    // the playhead moves on inside the rendered stretch: more ahead, the old part trimmed
    h.app.session.as_mut().unwrap().cursor = 2.0;
    h.app.finish_prerenders();
    let c = &h.app.prerender[&tid];
    assert!(c.rendered_to() >= 4.0 - 1e-6, "{}", c.rendered_to());
    assert!(c.start > 0.7 && c.start <= 1.0 + 1e-6, "trimmed to 1 s behind: {}", c.start);
    // a jump far ahead (more than the "ahead" distance) starts over AT the playhead
    h.app.session.as_mut().unwrap().cursor = 7.0;
    h.app.finish_prerenders();
    let c = &h.app.prerender[&tid];
    assert!((c.start - 7.0).abs() < 1e-3, "restarted at the playhead: {}", c.start);
    assert!(c.rendered_to() >= 8.5, "{}", c.rendered_to());
    let g = h.app.prerender_gauge().unwrap();
    assert!(
        g.megabytes <= g.cap_megabytes + 0.1,
        "stays within the cap: {g:?}"
    );
    assert!((g.window.0 - 6.0).abs() < 1e-3, "1 s behind is the aim: {g:?}");
    assert_eq!(g.ready.len(), 1, "{g:?}");
    assert!((g.ready[0].0 - 7.0).abs() < 1e-3, "ready from the playhead on: {g:?}");
}

/// The person's report: a pre-render that holds the whole session shows as one bar the
/// length of the session, from its first clip's start to its last clip's end, even when the
/// pre-rendered tracks end at different times.
#[test]
fn a_whole_prerender_shows_the_whole_session() {
    let mut h = Harness::new(true, true);
    h.frames(2);
    {
        let s = h.app.session.as_mut().unwrap();
        // start the first clip a little late, so "from the first clip" is not 0:00
        for t in &mut s.tracks {
            for c in &mut t.clips {
                c.start += 0.5;
            }
        }
        s.tracks[0].prerender = true;
        s.tracks[1].prerender = true;
        s.touch();
    }
    h.app.finish_prerenders();
    let s = h.app.session.as_ref().unwrap();
    let first = s.tracks.iter().flat_map(|t| t.clips.iter()).map(|c| c.start).fold(f64::INFINITY, f64::min);
    let last = s.tracks.iter().flat_map(|t| t.clips.iter()).map(|c| c.end()).fold(0.0, f64::max);
    let ends: Vec<f64> = s.tracks[..2]
        .iter()
        .map(|t| t.clips.iter().map(|c| c.end()).fold(0.0, f64::max))
        .collect();
    assert!((ends[0] - ends[1]).abs() > 0.3, "the two tracks end at different times: {ends:?}");
    let g = h.app.prerender_gauge().unwrap();
    assert!(!g.busy, "{g:?}");
    assert!((g.span.0 - first).abs() < 1e-6, "starts at the first clip: {g:?}");
    assert!((g.span.1 - last).abs() < 1e-3, "ends at the last clip: {g:?} vs {last}");
    assert_eq!(g.ready.len(), 1, "one bar: {g:?}");
    assert!((g.ready[0].0 - g.span.0).abs() < 1e-6 && (g.ready[0].1 - g.span.1).abs() < 1e-6, "{g:?}");
}

/// While playing, a following pre-render keeps up with the playhead: the playhead moving
/// through the session (as playback moves it) is always inside rendered audio after a frame or
/// two, and the render never keeps starting over.
#[test]
fn a_following_prerender_keeps_up_with_playback() {
    let mut h = Harness::new(true, true);
    h.frames(2);
    h.app.prefs.prerender_cap_mb = 2.0;
    h.app.prefs.prerender_ahead_s = 2.0;
    h.app.prefs.prerender_behind_s = 1.0;
    let tid = {
        let s = h.app.session.as_mut().unwrap();
        s.tracks[0].prerender = true;
        s.touch();
        s.tracks[0].id
    };
    let mut starts = vec![];
    let mut t = 0.0;
    while t < 8.0 {
        h.app.session.as_mut().unwrap().cursor = t;
        // one frame's worth of rendering (8 ms), as during playback
        for _ in 0..3 {
            h.app.prerender_tick(std::time::Duration::from_millis(8));
        }
        let c = &h.app.prerender[&tid];
        if starts.last() != Some(&c.start) && c.start > t - 0.01 {
            starts.push(c.start);
        }
        assert!(c.rendered_to() >= t, "at {t:.2} s the playhead is inside rendered audio ({})", c.rendered_to());
        t += 1.0 / 60.0 * 3.0;
    }
    assert!(starts.len() <= 1, "it never started over while following: {starts:?}");
}

/// The render runs past the last clip until the effects have died away: a delay's echoes
/// are pre-rendered, a dry track stops at its last clip.
#[test]
fn prerender_keeps_rendering_an_effect_tail() {
    let mut h = Harness::new(true, true);
    h.frames(2);
    let (dry, wet) = {
        let s = h.app.session.as_mut().unwrap();
        s.tracks[1].rack = vec![crate::dsp::EffectSlot::new(crate::dsp::Effect::Delay {
            time_ms: 300.0,
            feedback: 0.6,
            mix: 0.5,
        })];
        s.tracks[0].prerender = true;
        s.tracks[1].prerender = true;
        s.touch();
        (s.tracks[0].id, s.tracks[1].id)
    };
    h.app.finish_prerenders();
    let d = &h.app.prerender[&dry];
    assert!(d.tail_done && (d.end - d.content_end).abs() < 0.2, "dry: {} vs {}", d.end, d.content_end);
    let w = &h.app.prerender[&wet];
    assert!(w.tail_done, "the tail was measured");
    assert!(
        w.end > w.content_end + 1.0,
        "the echoes keep the render going: end {} after the clip end {}",
        w.end,
        w.content_end
    );
    assert!(
        w.end < w.content_end + h.app.prefs.prerender_max_tail_s as f64 - 1.0,
        "and it stops once they die away: {}",
        w.end
    );
    assert!((w.rendered_to() - w.end).abs() < 1e-6);
}

/// A mixdown made in steps is the same as one made at once (the pre-render's renderer keeps
/// its effect state between steps).
#[test]
fn stepped_mixdown_matches_a_whole_mixdown() {
    let h = Harness::new(true, true);
    let s = h.app.session.as_ref().unwrap();
    let files = &h.app.files;
    let mix = s.snapshot(&|id| files.iter().find(|f| f.id == id).map(|f| f.data.clone()));
    let whole = crate::session::mixdown(&mix, s.rate, 0.0, 2.0);
    let mut r = crate::session::MixRender::new(&mix, s.rate, 0.0, 2.0);
    let mut steps = 0;
    while !r.step(&mix, 1) {
        steps += 1;
    }
    assert!(steps > 3, "rendered in several steps");
    let stepped = r.finish();
    assert_eq!(whole.ch[0].len(), stepped.ch[0].len());
    assert_eq!(whole.ch[0], stepped.ch[0]);
    assert_eq!(whole.ch[1], stepped.ch[1]);
}

/// The person's request: a clip's edge snaps to the other clips' edges on the tracks above and
/// below too. Trimming the melody's end (track 1) near a beat clip's end (track 2) lands
/// exactly on it, holds there while the drag goes on, and only with Snapping off stays where
/// the pointer let go.
#[test]
fn a_trimmed_clip_edge_snaps_to_clips_on_other_tracks() {
    for snapping in [true, false] {
        let mut h = Harness::new(true, true);
        h.app.tool = crate::app::Tool::Move;
        h.app.snapping = snapping;
        // clip edges only, so nothing else can explain the landing spot
        h.app.prefs.snap_time_display = false;
        h.app.prefs.snap_playhead = false;
        h.app.prefs.snap_session_start = false;
        h.app.prefs.marker_snap = false;
        h.frames(2);
        let (mel, target) = {
            let s = h.app.session.as_ref().unwrap();
            (s.tracks[0].clips[0].id, s.tracks[1].clips[1].end())
        };
        let (rect, rows, view) = h.app.multi_drag.geom.clone().expect("layout");
        let x_of = |t: f64| rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
        let end0 = {
            let s = h.app.session.as_ref().unwrap();
            let (ti, ci) = s.find_clip(mel).unwrap();
            s.tracks[ti].clips[ci].end()
        };
        assert!(end0 > target + 0.3, "the melody ends after the beat: {end0} vs {target}");
        let from = Pos2::new(x_of(end0) - 2.0, rows[0].center().y + 20.0);
        let to = Pos2::new(x_of(target) + 3.0, from.y);
        // a person's drag: real time, the pointer slowing to a stop near the target
        h.clock = Some(0.0);
        let press = |pos: Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        h.frames_with(1, vec![egui::Event::PointerMoved(from), press(from, true)]);
        let steps = 40;
        for k in 1..=steps {
            let f = k as f32 / steps as f32;
            let f = 1.0 - (1.0 - f) * (1.0 - f); // ease out
            let p = from + (to - from) * f;
            h.frames_with(1, vec![egui::Event::PointerMoved(p)]);
        }
        h.frames_with(20, vec![]); // the hand rests before letting go
        h.frames_with(2, vec![press(to, false)]);
        let s = h.app.session.as_ref().unwrap();
        let (ti, ci) = s.find_clip(mel).unwrap();
        let end = s.tracks[ti].clips[ci].end();
        assert!(end < end0 - 0.2, "the edge was trimmed: {end0} -> {end}");
        if snapping {
            assert!(
                (end - target).abs() < 1e-6,
                "snapped to the clip end on the track below: {end} vs {target}"
            );
        } else {
            assert!(
                (end - target).abs() > 1e-3,
                "Snapping off leaves the edge where it was dropped: {end}"
            );
        }
    }
}

// ---- library-based media links and Relink Media ---------------------------------

/// The person's example: the sound sits in `smt/stm`, the session in `smt/stm/ggg`. Moving the
/// whole `smt` folder under `gugugaga/pleple` must not break the session (.acsession and .sesx):
/// it expects the sound relative to itself (`../rain.wav`), not at the old absolute path.
#[test]
fn a_session_opens_after_its_library_folder_moves() {
    for ext in ["acsession", "sesx"] {
        let base = std::env::temp_dir().join(format!("ac-lib-move-{}-{ext}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let ggg = base.join("smt/stm/ggg");
        std::fs::create_dir_all(&ggg).unwrap();
        let wav = base.join("smt/stm/rain.wav");
        let d = crate::dsp::gen_tone(8000, 1, 0.5, 440.0, crate::dsp::Wave::Sine, -12.0);
        crate::audio::save_wav(&wav, &d, 16, false).unwrap();
        let mut h = Harness::new(false, true);
        let lf = crate::audio::load(&wav).unwrap();
        let id = h.app.add_file("rain.wav".into(), Some(wav.clone()), lf.data, 16, false, "test");
        h.app.insert_into_multitrack(id);
        let sess = ggg.join(format!("s.{ext}"));
        h.app.session.as_mut().unwrap().path = Some(sess.clone());
        h.app.save_session(false);
        assert!(sess.exists(), "{ext} saved");
        let txt = std::fs::read_to_string(&sess).unwrap();
        assert!(txt.contains("../rain.wav"), "{ext} stores the link relative to itself:\n{txt}");
        // move the whole tree somewhere else; the old place is gone
        std::fs::create_dir_all(base.join("gugugaga/pleple")).unwrap();
        std::fs::rename(base.join("smt"), base.join("gugugaga/pleple/smt")).unwrap();
        assert!(!wav.exists());
        let moved = base.join("gugugaga/pleple/smt/stm/ggg").join(format!("s.{ext}"));
        let mut h = Harness::new(false, false);
        h.app.load_session(&moved);
        let s = h.app.session.as_ref().expect("opened");
        assert_eq!(s.tracks.iter().map(|t| t.clips.len()).sum::<usize>(), 1, "{ext}: clip kept");
        assert!(h.app.offline.is_empty(), "{ext}: nothing offline");
        let f = h.app.files.iter().find(|f| f.name == "rain.wav").expect("file open");
        let p = f.path.clone().unwrap();
        assert_eq!(
            crate::library::normalize(&p),
            crate::library::normalize(&base.join("gugugaga/pleple/smt/stm/rain.wav")),
            "{ext}: found at the new place"
        );
        assert!(f.data.duration() > 0.4);
        let _ = std::fs::remove_dir_all(&base);
    }
}

/// Missing media keeps its clips (offline placeholder, marked in Files, saved with its link),
/// and Relink Media finds it in a folder and everything inside it and puts it back.
#[test]
fn offline_media_keeps_its_clips_and_relinks() {
    let base = std::env::temp_dir().join(format!("ac-relink-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("proj")).unwrap();
    std::fs::create_dir_all(base.join("elsewhere/deep/er")).unwrap();
    let wav = base.join("proj/rain.wav");
    let d = crate::dsp::gen_tone(8000, 1, 0.5, 440.0, crate::dsp::Wave::Sine, -12.0);
    crate::audio::save_wav(&wav, &d, 16, false).unwrap();
    let mut h = Harness::new(false, true);
    let lf = crate::audio::load(&wav).unwrap();
    let id = h.app.add_file("rain.wav".into(), Some(wav.clone()), lf.data, 16, false, "test");
    h.app.insert_into_multitrack(id);
    let sess = base.join("proj/s.acsession");
    h.app.session.as_mut().unwrap().path = Some(sess.clone());
    h.app.save_session(false);
    // the media goes missing (moved somewhere unrelated)
    let away = base.join("elsewhere/deep/er/rain.wav");
    std::fs::rename(&wav, &away).unwrap();
    let mut h = Harness::new(false, false);
    h.app.load_session(&sess);
    let fid = {
        let s = h.app.session.as_ref().unwrap();
        assert_eq!(s.tracks.iter().map(|t| t.clips.len()).sum::<usize>(), 1, "clip kept");
        s.tracks.iter().flat_map(|t| t.clips.iter()).next().unwrap().file_id
    };
    assert!(h.app.offline.contains_key(&fid), "marked offline");
    // saving keeps the expected link
    h.app.save_session(false);
    let txt = std::fs::read_to_string(&sess).unwrap();
    assert!(txt.contains("rain.wav"), "the link survives a save");
    // Relink Media: this folder only finds nothing, the whole tree finds it
    h.app.open_relink("");
    {
        let st = h.app.relink.as_mut().unwrap();
        assert_eq!(st.rows.len(), 1);
        st.folder = base.join("elsewhere").display().to_string();
        st.detect.recursive = false;
    }
    h.app.relink_search();
    for _ in 0..200 {
        h.app.relink_poll();
        if h.app.relink.as_ref().unwrap().searching.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(h.app.relink.as_ref().unwrap().rows[0].found.is_empty(), "not in that folder itself");
    h.app.relink.as_mut().unwrap().detect.recursive = true;
    h.app.relink_search();
    for _ in 0..200 {
        h.app.relink_poll();
        if h.app.relink.as_ref().unwrap().searching.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(h.app.relink.as_ref().unwrap().rows[0].found.len(), 1, "found inside");
    h.app.relink_apply();
    assert!(h.app.offline.is_empty(), "relinked");
    let f = h.app.files.iter().find(|f| f.id == fid).expect("same file id");
    assert_eq!(f.path.as_deref(), Some(away.as_path()));
    assert!(f.data.duration() > 0.4, "real audio now");
    // the window draws without trouble
    h.frames(2);
    // a file too short for its clips is refused when the length check is on
    let short = base.join("short.wav");
    let d = crate::dsp::gen_tone(8000, 1, 0.1, 440.0, crate::dsp::Wave::Sine, -12.0);
    crate::audio::save_wav(&short, &d, 16, false).unwrap();
    assert!(h.app.relink_file(fid, &short, true).is_err());
    let _ = std::fs::remove_dir_all(&base);
}

/// The Relink Media button in the Files panel opens the window (and the File menu command too).
#[test]
fn the_relink_button_opens_relink_media() {
    let mut h = Harness::new(true, true);
    h.frames(2);
    assert!(h.app.relink.is_none());
    h.run("file.relink").unwrap();
    assert!(h.app.relink.is_some(), "the command opens it");
    h.frames(2);
    h.app.relink = None;
    h.app.pending_cmd = Some("cmd:file.relink");
    h.frames(2);
    assert!(h.app.relink.is_some(), "the Files panel button's command opens it");
}

fn ctrl_z(h: &mut Harness) {
    let m = egui::Modifiers::CTRL | egui::Modifiers::COMMAND;
    h.frames_with(
        2,
        vec![
            egui::Event::ModifiersChanged(m),
            egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: m,
            },
        ],
    );
    h.frames_with(1, vec![egui::Event::ModifiersChanged(Default::default())]);
}

/// The person's report: a moved keyframe (still selected) must go back with Ctrl+Z, and so
/// must a moved curve handle.
#[test]
fn ctrl_z_undoes_a_keyframe_move() {
    use crate::keys::EnvTarget;
    for tool in [crate::app::Tool::Move, crate::app::Tool::Direct] {
        let mut h = Harness::new(true, true);
        h.app.tool = tool;
        h.frames(2);
        let b = h.app.session.as_ref().unwrap().tracks[0].clips[0].id;
        let before = h.app.session.as_ref().unwrap().tracks[0].clips[0].env.clone();
        let spot = h
            .app
            .multi_drag
            .key_spots
            .iter()
            .find(|(t, i, _)| *t == EnvTarget::ClipVol(b) && *i == 1)
            .map(|s| s.2)
            .expect("keyframe drawn");
        h.drag_mods(spot, Pos2::new(spot.x + 30.0, spot.y - 30.0), Default::default());
        h.frames(3);
        let moved = h.app.session.as_ref().unwrap().tracks[0].clips[0].env.clone();
        assert_ne!(moved, before, "{tool:?}: the keyframe moved");
        assert!(!h.app.multi_drag.key_sel.is_empty(), "{tool:?}: it stays selected");
        ctrl_z(&mut h);
        h.frames(3);
        let after = h.app.session.as_ref().unwrap().tracks[0].clips[0].env.clone();
        assert_eq!(after, before, "{tool:?}: Ctrl+Z put the keyframe back");
    }
}

/// Ctrl+Z also puts back a dragged curve handle, and several keyframes moved together.
#[test]
fn ctrl_z_undoes_handle_and_group_moves() {
    use crate::keys::EnvTarget;
    // a curve handle (Direct tool)
    let mut h = Harness::new(true, true);
    h.app.tool = crate::app::Tool::Direct;
    h.frames(2);
    let b = {
        let s = h.app.session.as_mut().unwrap();
        let c = &mut s.tracks[0].clips[0];
        let n = c.env.len();
        c.curves.env_h = vec![crate::curves::Handles::smooth(0.3, 0.3); n];
        s.touch();
        c_id(s)
    };
    h.frames(3);
    h.app.multi_drag.key_sel.only(EnvTarget::ClipVol(b), 1);
    h.frames(3);
    let before = h.app.session.as_ref().unwrap().tracks[0].clips[0].curves.env_h.clone();
    let spot = h
        .app
        .multi_drag
        .handle_spots
        .iter()
        .find(|(t, i, inside, _)| *t == EnvTarget::ClipVol(b) && *i == 1 && !*inside)
        .map(|s| s.3)
        .expect("the out handle is drawn");
    h.drag_mods(spot, Pos2::new(spot.x + 25.0, spot.y - 25.0), Default::default());
    h.frames(3);
    let moved = h.app.session.as_ref().unwrap().tracks[0].clips[0].curves.env_h.clone();
    assert_ne!(moved, before, "the handle moved");
    ctrl_z(&mut h);
    h.frames(3);
    let after = h.app.session.as_ref().unwrap().tracks[0].clips[0].curves.env_h.clone();
    assert_eq!(after, before, "Ctrl+Z put the handle back");

    // two keyframes moved together
    let mut h = Harness::new(true, true);
    h.app.tool = crate::app::Tool::Move;
    h.frames(2);
    let b = c_id(h.app.session.as_mut().unwrap());
    let before = h.app.session.as_ref().unwrap().tracks[0].clips[0].env.clone();
    h.app.multi_drag.key_sel.only(EnvTarget::ClipVol(b), 1);
    h.app.multi_drag.key_sel.toggle(EnvTarget::ClipVol(b), 2);
    h.frames(3);
    let spot = h
        .app
        .multi_drag
        .key_spots
        .iter()
        .find(|(t, i, _)| *t == EnvTarget::ClipVol(b) && *i == 1)
        .map(|s| s.2)
        .unwrap();
    h.drag_mods(spot, Pos2::new(spot.x + 20.0, spot.y + 20.0), Default::default());
    h.frames(3);
    assert_ne!(h.app.session.as_ref().unwrap().tracks[0].clips[0].env, before);
    ctrl_z(&mut h);
    h.frames(3);
    assert_eq!(
        h.app.session.as_ref().unwrap().tracks[0].clips[0].env,
        before,
        "Ctrl+Z put both keyframes back"
    );
}

fn c_id(s: &mut crate::session::Session) -> u64 {
    s.tracks[0].clips[0].id
}

/// Idle frames never change the session (or an undo would be followed by an automatic
/// "Edit" step that redoes it): after drags, selections and Ctrl+Z, the history stands still.
#[test]
fn idle_frames_leave_the_session_alone() {
    use crate::keys::EnvTarget;
    for tool in [crate::app::Tool::Move, crate::app::Tool::Direct] {
        let mut h = Harness::new(true, true);
        h.app.tool = tool;
        h.frames(4);
        let b = c_id(h.app.session.as_mut().unwrap());
        let spot = h
            .app
            .multi_drag
            .key_spots
            .iter()
            .find(|(t, i, _)| *t == EnvTarget::ClipVol(b) && *i == 1)
            .map(|s| s.2)
            .unwrap();
        h.drag_mods(spot, Pos2::new(spot.x + 30.0, spot.y - 30.0), Default::default());
        h.frames(3);
        for step in 0..3 {
            let rev = h.app.session.as_ref().unwrap().revision;
            let n = h.app.session_hist.len();
            let pos = h.app.session_hist_pos;
            h.frames(5);
            let s = h.app.session.as_ref().unwrap();
            assert_eq!(
                (s.revision, h.app.session_hist.len(), h.app.session_hist_pos),
                (rev, n, pos),
                "{tool:?} step {step}: idle frames changed the session ({:?})",
                h.app.session_hist.iter().map(|x| x.0.clone()).collect::<Vec<_>>()
            );
            ctrl_z(&mut h);
            h.frames(2);
        }
    }
}

/// After Ctrl+Z the keyframe is also *drawn* back where it was (not only stored there).
#[test]
fn ctrl_z_redraws_the_keyframe_where_it_was() {
    use crate::keys::EnvTarget;
    for tool in [crate::app::Tool::Move, crate::app::Tool::Direct] {
        let mut h = Harness::new(true, true);
        h.app.tool = tool;
        h.frames(3);
        let b = c_id(h.app.session.as_mut().unwrap());
        let spot_of = |h: &Harness| {
            h.app
                .multi_drag
                .key_spots
                .iter()
                .find(|(t, i, _)| *t == EnvTarget::ClipVol(b) && *i == 1)
                .map(|s| s.2)
                .unwrap()
        };
        let spot = spot_of(&h);
        h.drag_mods(spot, Pos2::new(spot.x + 30.0, spot.y - 30.0), Default::default());
        h.frames(3);
        assert!((spot_of(&h) - spot).length() > 10.0, "{tool:?}: drawn moved");
        ctrl_z(&mut h);
        h.frames(3);
        assert!(
            (spot_of(&h) - spot).length() < 0.5,
            "{tool:?}: drawn back at {:?}, was {:?}",
            spot_of(&h),
            spot
        );
    }
}

/// A person's drag: real time, many small moves, a rest, then release.
fn slow_drag(h: &mut Harness, from: Pos2, to: Pos2) {
    if h.clock.is_none() {
        h.clock = Some(100.0);
    }
    let press = |pos: Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.frames_with(1, vec![egui::Event::PointerMoved(from)]);
    h.frames_with(3, vec![]);
    h.frames_with(1, vec![press(from, true)]);
    for k in 1..=30 {
        let p = from + (to - from) * (k as f32 / 30.0);
        h.frames_with(1, vec![egui::Event::PointerMoved(p)]);
    }
    h.frames_with(10, vec![]);
    h.frames_with(1, vec![press(to, false)]);
    h.frames_with(10, vec![]);
}

/// The same undo checks with a real-time drag (click-vs-drag timing as on screen), several
/// times in a row, for keyframes and handles.
#[test]
fn ctrl_z_undoes_real_time_keyframe_and_handle_drags() {
    use crate::keys::EnvTarget;
    for tool in [crate::app::Tool::Move, crate::app::Tool::Direct] {
        let mut h = Harness::new(true, true);
        h.app.tool = tool;
        h.clock = Some(100.0);
        h.frames(5);
        let b = c_id(h.app.session.as_mut().unwrap());
        if tool == crate::app::Tool::Direct {
            let s = h.app.session.as_mut().unwrap();
            let n = s.tracks[0].clips[0].env.len();
            s.tracks[0].clips[0].curves.env_h = vec![crate::curves::Handles::smooth(0.3, 0.3); n];
            s.touch();
            h.frames(5);
        }
        let start = h.app.session.as_ref().unwrap().tracks[0].clips[0].clone();
        let mut states = vec![(start.env.clone(), start.curves.env_h.clone())];
        for round in 0..3 {
            let spot = h
                .app
                .multi_drag
                .key_spots
                .iter()
                .find(|(t, i, _)| *t == EnvTarget::ClipVol(b) && *i == 1)
                .map(|s| s.2)
                .unwrap();
            // click to select first, like a person, then drag
            slow_drag(&mut h, spot, spot);
            let grab = if tool == crate::app::Tool::Direct && round == 1 {
                h.app
                    .multi_drag
                    .handle_spots
                    .iter()
                    .find(|(t, i, inside, _)| *t == EnvTarget::ClipVol(b) && *i == 1 && !*inside)
                    .map(|s| s.3)
                    .expect("handle drawn")
            } else {
                spot
            };
            slow_drag(&mut h, grab, grab + egui::vec2(20.0, -15.0 - round as f32 * 5.0));
            let c = &h.app.session.as_ref().unwrap().tracks[0].clips[0];
            let now = (c.env.clone(), c.curves.env_h.clone());
            assert_ne!(&now, states.last().unwrap(), "{tool:?} round {round}: it moved");
            states.push(now);
        }
        for k in (0..3).rev() {
            ctrl_z(&mut h);
            h.frames(5);
            let c = &h.app.session.as_ref().unwrap().tracks[0].clips[0];
            assert_eq!(
                (c.env.clone(), c.curves.env_h.clone()),
                states[k],
                "{tool:?}: Ctrl+Z #{} went back one move ({:?} at {})",
                3 - k,
                h.app.session_hist.iter().map(|x| x.0.clone()).collect::<Vec<_>>(),
                h.app.session_hist_pos
            );
        }
    }
}

/// Ctrl+Z after moving keyframes of every other envelope kind: clip pan, a track's automation
/// lane, the Master lane.
#[test]
fn ctrl_z_undoes_lane_and_pan_keyframe_moves() {
    use crate::keys::{EnvTarget, env_of};
    for which in 0..3 {
        let mut h = Harness::new(true, true);
        h.app.tool = crate::app::Tool::Move;
        h.clock = Some(100.0);
        h.frames(3);
        let target = {
            let s = h.app.session.as_mut().unwrap();
            let t0 = s.tracks[0].id;
            let cid = s.tracks[0].clips[0].id;
            match which {
                0 => {
                    let l = s.tracks[0].clips[0].len;
                    s.tracks[0].clips[0].pan_env = vec![(l * 0.2, 0.0), (l * 0.5, 0.5), (l * 0.8, 0.0)];
                    s.tracks[0].clips[0].curves.pan_h.clear();
                    EnvTarget::ClipPan(cid)
                }
                1 => {
                    s.tracks[0].show_vol_env = true;
                    s.tracks[0].io.automation = 1;
                    s.tracks[0].vol_env = vec![(1.0, 0.0), (3.0, -6.0), (5.0, 0.0)];
                    s.tracks[0].auto.vol_h.clear();
                    EnvTarget::TrackVol(t0)
                }
                _ => {
                    s.master.show_vol_env = true;
                    s.master.automation = 1;
                    s.master.vol_env = vec![(1.0, 0.0), (3.0, -6.0), (5.0, 0.0)];
                    s.master.vol_h.clear();
                    EnvTarget::MasterVol
                }
            }
        };
        h.app.session.as_mut().unwrap().touch();
        h.frames(5);
        let env = |h: &Harness| {
            env_of(h.app.session.as_ref().unwrap(), target)
                .map(|(e, hs)| (e.clone(), hs.clone()))
                .unwrap()
        };
        let before = env(&h);
        let Some(spot) = h
            .app
            .multi_drag
            .key_spots
            .iter()
            .find(|(t, i, _)| *t == target && *i == 1)
            .map(|s| s.2)
        else {
            panic!("{target:?}: keyframe not drawn")
        };
        slow_drag(&mut h, spot, spot + egui::vec2(15.0, -12.0));
        assert_ne!(env(&h), before, "{target:?}: moved");
        ctrl_z(&mut h);
        h.frames(5);
        assert_eq!(
            env(&h),
            before,
            "{target:?}: Ctrl+Z put it back ({:?} at {})",
            h.app.session_hist.iter().map(|x| x.0.clone()).collect::<Vec<_>>(),
            h.app.session_hist_pos
        );
    }
}

/// The person's report: in a thin window the workspace names and the search box ran over the
/// tools. Now the right end gives things up (app name, other tabs, search width, the search
/// box) and always stays inside the room the tools leave.
#[test]
fn a_thin_window_never_overlaps_the_toolbar() {
    let mut last_shown = usize::MAX;
    for w in [1800.0, 1300.0, 1100.0, 950.0, 850.0, 760.0, 700.0] {
        let mut h = Harness::new(true, true).with_size(w, 900.0);
        h.frames(4);
        let p = h.app.extra.toolbar_plan.clone().expect("toolbar drawn");
        assert!(p.fits, "width {w}: the right end runs over the tools: {p:?}");
        assert!(p.shown.len() <= last_shown, "width {w}: fewer tabs as it narrows");
        last_shown = p.shown.len();
        if w >= 1800.0 {
            assert!(p.label && !p.search_icon && p.shown.len() >= 3, "wide: everything: {p:?}");
        }
    }
    let mut h = Harness::new(true, true).with_size(700.0, 900.0);
    h.frames(4);
    let p = h.app.extra.toolbar_plan.clone().unwrap();
    assert!(!p.label, "a thin window drops the app name first: {p:?}");
}

/// The person's order for a shrinking track: the middle rows go first, then the automation
/// row (the lane's arrow and Read); the volume/pan row stays the longest. The arrow is there
/// down to about 84 points and answers; below that it has given way to volume/pan.
#[test]
fn the_automation_row_survives_short_tracks() {
    for height in [86.0f32, 100.0, 110.0] {
        let mut h = Harness::new(true, true);
        {
            let s = h.app.session.as_mut().unwrap();
            for t in &mut s.tracks {
                t.height = height;
            }
            s.touch();
        }
        h.frames(3);
        let (rect, rows, _) = h.app.multi_drag.geom.clone().expect("layout");
        // the arrow sits at the header's bottom-left, just right of the colour strip
        let p = Pos2::new(rect.left() - 222.0 + 8.0 + 8.0, rows[0].bottom() - 15.0);
        let before = h.app.session.as_ref().unwrap().tracks[0].show_vol_env;
        h.click(p);
        h.frames(2);
        let after = h.app.session.as_ref().unwrap().tracks[0].show_vol_env;
        assert_ne!(before, after, "height {height}: the lane arrow is there and answers");
    }
}

/// Saving never stacks extensions: a session whose path already has them doubled is written as
/// one clean name, and its name in the Files panel carries no extension.
#[test]
fn saving_never_stacks_session_extensions() {
    let dir = std::env::temp_dir().join(format!("ac-savename-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = Harness::new(false, true);
    let d = crate::dsp::gen_tone(8000, 1, 0.2, 440.0, crate::dsp::Wave::Sine, -12.0);
    let id = h.app.add_file("take.wav".into(), None, d, 16, true, "test");
    h.app.insert_into_multitrack(id);
    {
        let s = h.app.session.as_mut().unwrap();
        s.name = "xyz.sesx".into();
        s.path = Some(dir.join("xyz.sesx.sesx"));
    }
    h.app.save_session(false);
    assert!(dir.join("xyz.sesx").exists(), "written as xyz.sesx");
    assert!(!dir.join("xyz.sesx.sesx").exists(), "never xyz.sesx.sesx");
    let s = h.app.session.as_ref().unwrap();
    assert_eq!(s.name, "xyz");
    assert_eq!(s.path.as_deref(), Some(dir.join("xyz.sesx").as_path()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Below the automation row's room it gives way first (volume/pan keeps its row).
#[test]
fn the_automation_row_goes_before_volume_and_pan() {
    let mut h = Harness::new(true, true);
    {
        let s = h.app.session.as_mut().unwrap();
        for t in &mut s.tracks {
            t.height = 64.0;
        }
        s.touch();
    }
    h.frames(3);
    let (rect, rows, _) = h.app.multi_drag.geom.clone().expect("layout");
    let p = Pos2::new(rect.left() - 222.0 + 8.0 + 8.0, rows[0].bottom() - 15.0);
    let before = h.app.session.as_ref().unwrap().tracks[0].show_vol_env;
    h.click(p);
    h.frames(2);
    assert_eq!(
        h.app.session.as_ref().unwrap().tracks[0].show_vol_env,
        before,
        "a 64-point track shows volume/pan, not the lane arrow"
    );
}

/// Like the time after the session's end, there is blank room under the last track: the
/// tracks scroll up past it even when they only just fill the view.
#[test]
fn tracks_scroll_past_the_last_one() {
    let mut h = Harness::new(true, true);
    h.frames(3);
    h.app.multi_drag.scroll_y = f32::MAX;
    h.frames(3);
    let (_, rows, _) = h.app.multi_drag.geom.clone().expect("layout");
    let (lanes, _, _) = h.app.multi_drag.geom.clone().unwrap();
    let last = rows.last().copied().unwrap();
    assert!(h.app.multi_drag.scroll_y > 0.0, "it scrolls");
    assert!(
        last.bottom() < lanes.bottom() - 50.0,
        "blank room under the last track: {} vs {}",
        last.bottom(),
        lanes.bottom()
    );
}

/// The snapping line can have the person's own colour (Preferences > Multitrack), and goes
/// back to the theme's when that is cleared.
#[test]
fn the_snapping_line_colour_can_be_picked() {
    let theme = crate::theme::snap_line();
    crate::theme::set_snap_override(Some([10, 200, 30]));
    assert_eq!(crate::theme::snap_line(), egui::Color32::from_rgb(10, 200, 30));
    assert_eq!(crate::theme::theme_snap_line(), theme);
    crate::theme::set_snap_override(None);
    assert_eq!(crate::theme::snap_line(), theme);
    let mut h = Harness::new(true, true);
    let mut p = h.app.prefs.clone();
    p.snap_line_color = Some([1, 2, 3]);
    h.app.apply_prefs_live(p);
    assert_eq!(crate::theme::snap_line(), egui::Color32::from_rgb(1, 2, 3));
    let mut p = h.app.prefs.clone();
    p.snap_line_color = None;
    h.app.apply_prefs_live(p);
    assert_eq!(crate::theme::snap_line(), theme);
    crate::theme::set_snap_override(None);
}

/// The person's report: making the colour grey in the picker (dragging to the left of its
/// square) must not move the hue marker or turn the hue red; the picker keeps its hue.
#[test]
fn the_colour_picker_keeps_its_hue_when_grey() {
    let ctx = egui::Context::default();
    let mut rgb = [46u8, 170, 140]; // a teal
    let start_hue = egui::ecolor::Hsva::from(egui::Color32::from_rgb(46, 170, 140)).h;
    let id = egui::Id::new("pick_test");
    let mut top_left = Pos2::ZERO;
    let mut frame = |events: Vec<egui::Event>, rgb: &mut [u8; 3], tl: &mut Pos2| {
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                *tl = ui.cursor().min;
                crate::theme::colour_field(ui, id, rgb);
            });
        });
        // no GPU here: the font atlas upload is dropped on purpose
        out.textures_delta.clear();
    };
    frame(vec![], &mut rgb, &mut top_left);
    frame(vec![], &mut rgb, &mut top_left);
    // press inside the square, drag to its left edge (no saturation), release
    let a = top_left + vec2(150.0, 40.0);
    let b = top_left + vec2(-30.0, 40.0);
    let btn = |pos: Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(vec![egui::Event::PointerMoved(a), btn(a, true)], &mut rgb, &mut top_left);
    for k in 1..=10 {
        let p = a + (b - a) * (k as f32 / 10.0);
        frame(vec![egui::Event::PointerMoved(p)], &mut rgb, &mut top_left);
    }
    frame(vec![btn(b, false)], &mut rgb, &mut top_left);
    frame(vec![], &mut rgb, &mut top_left);
    assert!(rgb[0] == rgb[1] && rgb[1] == rgb[2], "fully grey now: {rgb:?}");
    let kept: Option<([u8; 3], [f32; 3])> = ctx.data(|d| d.get_temp(id.with("hsv")));
    let (_, [h, s, _]) = kept.expect("the picker keeps its state");
    assert!(s < 0.01);
    assert!(
        (h - start_hue).abs() < 0.02,
        "the hue stayed teal ({h} vs {start_hue}), it did not jump to red"
    );
}

/// The person's requests: Alt+wheel over the clips resizes every track (the plain wheel already
/// scrolls them), and scrolling is never logged in the undo history: Ctrl+Z after it neither
/// adds nor undoes anything about the heights.
#[test]
fn alt_wheel_resizes_the_tracks() {
    let mut h = Harness::new(true, true);
    h.clock = Some(10.0);
    h.frames(3);
    let (lanes, _, _) = h.app.multi_drag.geom.clone().expect("layout");
    let over = lanes.center();
    let heights = |h: &Harness| -> Vec<f32> {
        h.app.session.as_ref().unwrap().tracks.iter().map(|t| t.height).collect()
    };
    let before = heights(&h);
    let steps = h.app.session_hist.len();
    let wheel = |dy: f32| egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, dy),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: egui::Modifiers::ALT,
    };
    for _ in 0..4 {
        h.frames_with(
            1,
            vec![
                egui::Event::PointerMoved(over),
                egui::Event::ModifiersChanged(egui::Modifiers::ALT),
                wheel(60.0),
            ],
        );
        h.frames_with(4, vec![egui::Event::ModifiersChanged(egui::Modifiers::ALT)]);
    }
    h.frames_with(10, vec![egui::Event::ModifiersChanged(Default::default())]);
    let after = heights(&h);
    assert!(
        after.iter().zip(&before).all(|(a, b)| a > b),
        "every track grew: {before:?} -> {after:?}"
    );
    assert_eq!(h.app.multi_drag.scroll_y, 0.0, "Alt+wheel did not scroll");
    assert_eq!(h.app.session_hist.len(), steps, "scrolling is not an undo step");
    ctrl_z(&mut h);
    h.frames(2);
    assert_eq!(heights(&h), after, "Ctrl+Z leaves the scrolled heights alone");
    // an edit after scrolling, then Ctrl+Z: the edit goes, the heights stay
    {
        let s = h.app.session.as_mut().unwrap();
        s.tracks[0].clips[0].gain_db += 3.0;
        s.touch();
    }
    h.frames(3);
    assert_eq!(h.app.session_hist.len(), steps + 1, "the edit is one step");
    ctrl_z(&mut h);
    h.frames(2);
    let s = h.app.session.as_ref().unwrap();
    assert!(s.tracks[0].clips[0].gain_db.abs() < 1e-6, "the edit was undone");
    assert_eq!(heights(&h), after, "and the heights stayed as scrolled");
}

// ---- MCP (AI assistants) ------------------------------------------------------------

fn mcp_call(h: &mut Harness, id: u64, name: &str, args: serde_json::Value) -> (bool, String) {
    let ctx = h.ctx.clone();
    let msg = serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": name, "arguments": args}});
    let r = h.app.mcp_handle(&msg, &ctx).expect("an answer");
    let res = &r["result"];
    (
        res["isError"].as_bool().unwrap_or(true),
        res["content"][0]["text"].as_str().unwrap_or("").to_string(),
    )
}

/// The MCP protocol answers (initialize, tools/list, notifications), and every guardrail
/// holds: the permission level, commands that are never available, saving only to a place the
/// person chose, the rate limit, and edits that are one undo step.
#[test]
fn mcp_answers_and_keeps_its_guardrails() {
    use serde_json::json;
    let mut h = Harness::new(true, true);
    h.frames(2);
    let ctx = h.ctx.clone();
    let init = h
        .app
        .mcp_handle(
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}}),
            &ctx,
        )
        .unwrap();
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "AudioCraft");
    assert!(
        h.app
            .mcp_handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}), &ctx)
            .is_none()
    );
    let list = h
        .app
        .mcp_handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}), &ctx)
        .unwrap();
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for n in [
        "get_state",
        "list_tracks",
        "list_commands",
        "run_command",
        "set_playhead",
        "set_selection",
        "set_track",
    ] {
        assert!(names.contains(&n), "{n}");
    }
    let bad = h
        .app
        .mcp_handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "no/such"}), &ctx)
        .unwrap();
    assert_eq!(bad["error"]["code"], -32601);

    // read only: looking works, changing does not
    h.app.prefs.mcp_level = 0;
    let (err, text) = mcp_call(&mut h, 4, "get_state", json!({}));
    assert!(!err && text.contains("Multitrack"), "{text}");
    let (err, text) = mcp_call(&mut h, 5, "list_tracks", json!({}));
    assert!(!err && text.contains("Demo"), "{text}");
    let (err, text) = mcp_call(&mut h, 6, "set_playhead", json!({"seconds": 2.0}));
    assert!(err && text.contains("Navigate"), "{text}");

    // navigate & play
    h.app.prefs.mcp_level = 1;
    let (err, _) = mcp_call(&mut h, 7, "set_playhead", json!({"seconds": 2.0}));
    assert!(!err);
    assert!((h.app.cursor() - 2.0).abs() < 1e-6);
    let (err, text) = mcp_call(&mut h, 8, "set_track", json!({"track": 0, "volume_db": -6}));
    assert!(err && text.contains("Edit"), "{text}");
    let (err, text) = mcp_call(&mut h, 9, "set_selection", json!({"start": 3.0, "end": 1.0}));
    assert!(err && text.contains("after"), "{text}");

    // never: quitting, dialogs, unknown commands, at any level
    h.app.prefs.mcp_level = 3;
    for id in ["file.exit", "file.open", "nope.nothing"] {
        let (err, text) = mcp_call(&mut h, 10, "run_command", json!({ "id": id }));
        assert!(err, "{id}: {text}");
    }
    // saving only where the person already saved
    let (err, text) = mcp_call(&mut h, 11, "run_command", json!({"id": "file.save"}));
    assert!(err && text.contains("yourself"), "{text}");

    // edit: one undoable step
    h.app.prefs.mcp_level = 2;
    h.app.prefs.mcp_confirm = false;
    let before = h.app.session.as_ref().unwrap().tracks[0].volume_db;
    let steps = h.app.session_hist.len();
    let (err, text) = mcp_call(
        &mut h,
        12,
        "set_track",
        json!({"track": 0, "volume_db": -6.5, "name": "Lead\u{7}"}),
    );
    assert!(!err, "{text}");
    let t = &h.app.session.as_ref().unwrap().tracks[0];
    assert_eq!(t.volume_db, -6.5);
    assert_eq!(t.name, "Lead", "control characters are dropped");
    assert_eq!(h.app.session_hist.len(), steps + 1);
    let (err, _) = mcp_call(&mut h, 13, "run_command", json!({"id": "edit.undo"}));
    assert!(!err);
    assert_eq!(
        h.app.session.as_ref().unwrap().tracks[0].volume_db,
        before,
        "undone"
    );
    let (err, text) = mcp_call(&mut h, 14, "set_track", json!({"track": 99, "mute": true}));
    assert!(err && text.contains("99"), "{text}");

    // the rate limit
    h.app.prefs.mcp_rate_per_min = 3;
    h.app.mcp.calls.clear();
    for k in 0..3 {
        let (err, _) = mcp_call(&mut h, 20 + k, "get_state", json!({}));
        assert!(!err);
    }
    let (err, text) = mcp_call(&mut h, 30, "get_state", json!({}));
    assert!(err && text.contains("too many"), "{text}");
}

/// Over a real connection: the token is required, a web page (another Origin) is turned away,
/// only POST /mcp is served, and with "Ask me before each change" an edit waits for Allow.
#[test]
fn mcp_over_http_needs_the_token_and_waits_for_allow() {
    use serde_json::json;
    let mut h = Harness::new(true, true);
    h.app.prefs.mcp_enabled = true;
    h.app.prefs.mcp_port = 0; // any free port
    h.app.prefs.mcp_token = "t".repeat(40);
    h.app.prefs.mcp_level = 2;
    h.app.prefs.mcp_confirm = true;
    h.frames(2);
    let port = h.app.mcp.server.as_ref().expect("listening").port;
    assert!(h.app.mcp.status.contains("Listening"), "{}", h.app.mcp.status);
    let token = h.app.prefs.mcp_token.clone();
    let raw = |req: String| -> String {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    };
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}).to_string();
    // no token
    let r = raw(format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    ));
    assert!(r.starts_with("HTTP/1.1 401"), "{r}");
    // a web page's origin
    let r = raw(format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: https://web.page.example\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    ));
    assert!(r.starts_with("HTTP/1.1 403"), "{r}");
    // GET is not served
    let r = raw(format!(
        "GET /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n\r\n"
    ));
    assert!(r.starts_with("HTTP/1.1 405"), "{r}");
    // a good ping, answered by the app's frame loop
    let worker = std::thread::spawn({
        let token = token.clone();
        let body = body.clone();
        move || crate::mcp::post(port, &token, &body)
    });
    let mut answer = None;
    for _ in 0..400 {
        h.frames(1);
        if worker.is_finished() {
            answer = Some(worker.join().unwrap());
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let (code, text) = answer.expect("answered").unwrap();
    assert_eq!(code, 200, "{text}");
    assert!(text.contains("\"result\""), "{text}");
    // an edit waits for the person
    let edit = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "set_track", "arguments": {"track": 1, "mute": true}}})
    .to_string();
    let worker = std::thread::spawn({
        let token = token.clone();
        move || crate::mcp::post(port, &token, &edit)
    });
    for _ in 0..400 {
        h.frames(1);
        if !h.app.mcp.pending.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(h.app.mcp.pending.len(), 1, "waiting for Allow");
    assert!(
        !h.app.session.as_ref().unwrap().tracks[1].mute,
        "nothing changed yet"
    );
    let ctx = h.ctx.clone();
    h.app.mcp_decide(1, &ctx);
    let (code, text) = worker.join().unwrap().unwrap();
    assert_eq!(code, 200);
    assert!(text.contains("\"isError\":false"), "{text}");
    assert!(
        h.app.session.as_ref().unwrap().tracks[1].mute,
        "allowed and done"
    );
    // turning MCP off stops the server
    h.app.prefs.mcp_enabled = false;
    h.frames(2);
    assert!(h.app.mcp.server.is_none());
}

/// The newer MCP rules (2025-06-18 / 2025-11-25): version negotiation, tool titles and hints,
/// structured results with output schemas, an unknown tool as a protocol error, client
/// responses and bad ids handled.
#[test]
fn mcp_follows_the_newer_protocol() {
    use serde_json::json;
    let mut h = Harness::new(true, true);
    h.frames(2);
    let ctx = h.ctx.clone();
    let ask = |h: &mut Harness, m: serde_json::Value| h.app.mcp_handle(&m, &ctx);
    for (asked, given) in [
        ("2025-11-25", "2025-11-25"),
        ("2025-03-26", "2025-03-26"),
        ("1999-01-01", "2025-11-25"),
    ] {
        let r = ask(&mut h, json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": asked, "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}}))
        .unwrap();
        assert_eq!(r["result"]["protocolVersion"], given, "{asked}");
        assert!(r["result"]["serverInfo"]["title"].is_string());
    }
    let list = ask(&mut h, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
    let tools = list["result"]["tools"].as_array().unwrap().clone();
    let tool = |n: &str| tools.iter().find(|t| t["name"] == n).unwrap().clone();
    assert_eq!(tool("get_state")["annotations"]["readOnlyHint"], true);
    assert_eq!(tool("run_command")["annotations"]["destructiveHint"], true);
    assert_eq!(tool("set_playhead")["annotations"]["idempotentHint"], true);
    for n in ["get_state", "list_tracks", "list_commands"] {
        assert_eq!(tool(n)["outputSchema"]["type"], "object", "{n}");
    }
    // data comes as structured content too, matching the output schema's required fields
    h.app.prefs.mcp_level = 3;
    let call = |h: &mut Harness, n: &str, a: serde_json::Value| {
        h.app
            .mcp_handle(
                &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": n, "arguments": a}}),
                &h.ctx.clone(),
            )
            .unwrap()
    };
    let r = call(&mut h, "get_state", json!({}));
    assert_eq!(r["result"]["structuredContent"]["editor"], "Multitrack");
    for k in tool("get_state")["outputSchema"]["required"].as_array().unwrap() {
        assert!(r["result"]["structuredContent"].get(k.as_str().unwrap()).is_some(), "{k}");
    }
    let r = call(&mut h, "list_tracks", json!({}));
    assert!(r["result"]["structuredContent"]["tracks"].as_array().unwrap().len() > 1);
    let r = call(&mut h, "list_commands", json!({}));
    assert!(!r["result"]["structuredContent"]["commands"].as_array().unwrap().is_empty());
    // a plain text answer has no structured content
    let r = call(&mut h, "set_playhead", json!({"seconds": 1.0}));
    assert!(r["result"].get("structuredContent").is_none());
    assert_eq!(r["result"]["isError"], false);
    // an unknown tool or bad arguments: a protocol error
    let r = call(&mut h, "no_such_tool", json!({}));
    assert_eq!(r["error"]["code"], -32602);
    let r = call(&mut h, "set_playhead", json!([1, 2]));
    assert_eq!(r["error"]["code"], -32602);
    // a client's response, and requests with a bad id or version
    assert!(ask(&mut h, json!({"jsonrpc": "2.0", "id": 9, "result": {}})).is_none());
    let r = ask(&mut h, json!({"jsonrpc": "2.0", "id": null, "method": "ping"})).unwrap();
    assert_eq!(r["error"]["code"], -32600);
    let r = ask(&mut h, json!({"jsonrpc": "1.0", "id": 4, "method": "ping"})).unwrap();
    assert_eq!(r["error"]["code"], -32600);
    // an older client's batch still works, but never with initialize in it
    let r = ask(&mut h, json!([{"jsonrpc": "2.0", "id": 5, "method": "ping"},
        {"jsonrpc": "2.0", "method": "notifications/initialized"}]))
    .unwrap();
    assert_eq!(r.as_array().unwrap().len(), 1);
    let r = ask(&mut h, json!([{"jsonrpc": "2.0", "id": 6, "method": "initialize", "params": {}}])).unwrap();
    assert_eq!(r["error"]["code"], -32600);
}

/// Over HTTP: initialize gives a session id and the bridge sends it back with the version;
/// an unknown session is 404 (and the bridge recovers), an unknown version 400, batches from
/// newer clients 400, and DELETE ends a session.
#[test]
fn mcp_http_sessions_and_versions() {
    use serde_json::json;
    let mut h = Harness::new(true, true);
    h.app.prefs.mcp_enabled = true;
    h.app.prefs.mcp_port = 0;
    h.app.prefs.mcp_token = "s".repeat(40);
    h.app.prefs.mcp_level = 1;
    h.app.prefs.mcp_confirm = false;
    h.frames(2);
    let port = h.app.mcp.server.as_ref().expect("listening").port;
    let token = h.app.prefs.mcp_token.clone();
    // a request the app answers in its frame loop
    let pump = |h: &mut Harness, body: String, link: crate::mcp::Link| {
        let token = token.clone();
        let worker = std::thread::spawn(move || {
            let mut link = link;
            let r = crate::mcp::post_in(port, &token, &body, &mut link);
            (r, link)
        });
        for _ in 0..400 {
            h.frames(1);
            if worker.is_finished() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let (r, link) = worker.join().unwrap();
        (r.unwrap(), link)
    };
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}})
    .to_string();
    let ((code, text), link) = pump(&mut h, init, crate::mcp::Link::default());
    assert_eq!(code, 200, "{text}");
    let sid = link.session.clone().expect("a session id");
    assert_eq!(link.version.as_deref(), Some("2025-11-25"));
    // within the session
    let ping = json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}).to_string();
    let ((code, text), _) = pump(&mut h, ping.clone(), link.clone());
    assert_eq!(code, 200, "{text}");
    // answered before the app is asked: no frames needed
    let mut stale = crate::mcp::Link {
        session: Some("0".repeat(32)),
        version: Some("2025-11-25".into()),
    };
    let (code, _) = crate::mcp::post_in(port, &token, &ping, &mut stale).unwrap();
    assert_eq!(code, 404, "an unknown session");
    let mut odd = crate::mcp::Link {
        session: None,
        version: Some("1999-01-01".into()),
    };
    let (code, text) = crate::mcp::post_in(port, &token, &ping, &mut odd).unwrap();
    assert_eq!(code, 400, "an unknown version: {text}");
    let batch = json!([{"jsonrpc": "2.0", "id": 3, "method": "ping"}]).to_string();
    let (code, text) = crate::mcp::post_in(port, &token, &batch, &mut link.clone()).unwrap();
    assert_eq!(code, 400, "no batches in 2025-11-25: {text}");
    // DELETE ends the session
    let raw = |req: String| -> String {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    };
    let del = format!(
        "DELETE /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nMcp-Session-Id: {sid}\r\n\r\n"
    );
    assert!(raw(del.clone()).starts_with("HTTP/1.1 200"));
    assert!(raw(del).starts_with("HTTP/1.1 404"));
    let (code, _) = crate::mcp::post_in(port, &token, &ping, &mut link.clone()).unwrap();
    assert_eq!(code, 404, "the ended session is gone");
    // a page posting a form is turned away
    let r = raw(format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{ping}",
        ping.len()
    ));
    assert!(r.starts_with("HTTP/1.1 415"), "{r}");
    h.app.prefs.mcp_enabled = false;
    h.frames(2);
}

/// One track volume range everywhere (-60..+15 dB): a +15 dB track can be set by an assistant,
/// stays +15 dB with the Properties panel showing it, and louder asks stop at +15.
#[test]
fn every_track_volume_control_shares_one_range() {
    use serde_json::json;
    let mut h = Harness::new(true, true);
    h.frames(2);
    h.app.prefs.mcp_level = 2;
    h.app.prefs.mcp_confirm = false;
    let (err, text) = mcp_call(&mut h, 1, "set_track", json!({"track": 0, "volume_db": 15}));
    assert!(!err, "{text}");
    assert_eq!(h.app.session.as_ref().unwrap().tracks[0].volume_db, 15.0);
    let (err, _) = mcp_call(&mut h, 2, "set_track", json!({"track": 0, "volume_db": 40}));
    assert!(!err);
    assert_eq!(
        h.app.session.as_ref().unwrap().tracks[0].volume_db,
        crate::session::TRACK_DB_MAX
    );
    let ctx = h.ctx.clone();
    let list = h
        .app
        .mcp_handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}), &ctx)
        .unwrap();
    let max = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "set_track")
        .unwrap()["inputSchema"]["properties"]["volume_db"]["maximum"]
        .as_f64()
        .unwrap();
    assert_eq!(max, 15.0, "the tool says the same range");
    // the Properties panel showing the track never turns it down
    h.app.show_panel(crate::workspace::PanelId::Properties);
    h.frames(4);
    assert_eq!(h.app.session.as_ref().unwrap().tracks[0].volume_db, 15.0);
}

/// The person's request: while a keyframe is dragged a small floating bar says its value and
/// how far it moved (dB for volume, L/R for pan...) and where in time it is; it goes away on
/// release.
#[test]
fn dragging_a_keyframe_shows_its_value_and_change() {
    use crate::keys::EnvTarget;
    let mut h = Harness::new(true, true);
    h.app.tool = crate::app::Tool::Move;
    h.frames(3);
    let b = c_id(h.app.session.as_mut().unwrap());
    let spot = h
        .app
        .multi_drag
        .key_spots
        .iter()
        .find(|(t, i, _)| *t == EnvTarget::ClipVol(b) && *i == 1)
        .map(|s| s.2)
        .expect("keyframe drawn");
    assert!(h.app.multi_drag.readout.is_none(), "nothing before a drag");
    let press = |pos: Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.frames_with(1, vec![egui::Event::PointerMoved(spot), press(spot, true)]);
    let to = spot + vec2(12.0, -30.0);
    h.frames_with(1, vec![egui::Event::PointerMoved(spot + vec2(6.0, -15.0))]);
    h.frames_with(1, vec![egui::Event::PointerMoved(to)]);
    let (_, text) = h.app.multi_drag.readout.clone().expect("the readout shows while dragging");
    assert!(text.contains("dB"), "{text}");
    assert!(text.contains("(+"), "it says how far the point moved up: {text}");
    assert!(text.contains(" s)"), "and how far in time: {text}");
    h.frames_with(2, vec![press(to, false)]);
    h.frames(1);
    assert!(h.app.multi_drag.readout.is_none(), "gone after release");
    // the change text speaks each kind's unit
    use crate::keys::{VKind, delta_text};
    assert_eq!(delta_text(VKind::Vol, -3.04), "-3.0 dB");
    assert_eq!(delta_text(VKind::Pan, 0.25), "25 to R");
    assert_eq!(delta_text(VKind::Pan, -0.1), "10 to L");
    assert_eq!(delta_text(VKind::Fade, 0.5), "+50 %");
}

/// A drag with the given modifiers held all the way: press, move in steps, release.
fn drag_held(h: &mut Harness, from: Pos2, to: Pos2, mods: egui::Modifiers) {
    let press = |pos: Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: mods,
    };
    h.frames_with(
        1,
        vec![egui::Event::ModifiersChanged(mods), egui::Event::PointerMoved(from), press(from, true)],
    );
    for k in 1..=10 {
        let p = from + (to - from) * (k as f32 / 10.0);
        h.frames_with(1, vec![egui::Event::ModifiersChanged(mods), egui::Event::PointerMoved(p)]);
    }
    h.frames_with(2, vec![egui::Event::ModifiersChanged(mods), press(to, false)]);
    h.frames_with(1, vec![egui::Event::ModifiersChanged(Default::default())]);
}

/// The person's request: holding Ctrl while dragging a keyframe or a clip moves it at a
/// fifth of the mouse's speed, for precise placing.
#[test]
fn ctrl_slows_dragging_for_precision() {
    use crate::keys::EnvTarget;
    let ctrl = egui::Modifiers::CTRL | egui::Modifiers::COMMAND;
    // a keyframe: the same mouse travel moves its value about a fifth as far with Ctrl
    let key_move = |mods: egui::Modifiers| -> f32 {
        let mut h = Harness::new(true, true);
        h.app.tool = crate::app::Tool::Move;
        h.app.snapping = false;
        h.frames(3);
        let b = c_id(h.app.session.as_mut().unwrap());
        let before = h.app.session.as_ref().unwrap().tracks[0].clips[0].env[1].1;
        let spot = h
            .app
            .multi_drag
            .key_spots
            .iter()
            .find(|(t, i, _)| *t == EnvTarget::ClipVol(b) && *i == 1)
            .map(|s| s.2)
            .unwrap();
        drag_held(&mut h, spot, spot + vec2(0.0, -40.0), mods);
        h.frames(2);
        let _ = before;
        // how far it moved on screen (the dB scale is not linear, the screen is)
        let after = h
            .app
            .multi_drag
            .key_spots
            .iter()
            .find(|(t, i, _)| *t == EnvTarget::ClipVol(b) && *i == 1)
            .map(|s| s.2)
            .unwrap();
        spot.y - after.y
    };
    let free = key_move(Default::default());
    let fine = key_move(ctrl);
    assert!(free > 1.0, "the free drag moved the keyframe up: {free}");
    let ratio = fine / free;
    assert!((0.1..0.35).contains(&ratio), "Ctrl moves about a fifth as far: {fine} vs {free}");
    // a clip: the same along the timeline
    let clip_move = |mods: egui::Modifiers| -> f64 {
        let mut h = Harness::new(true, true);
        h.app.tool = crate::app::Tool::Move;
        h.app.snapping = false;
        h.frames(3);
        let s = h.app.session.as_ref().unwrap();
        let c = &s.tracks[1].clips[1];
        let (id, before) = (c.id, c.start);
        let (rect, rows, view) = h.app.multi_drag.geom.clone().unwrap();
        let t = c.start + c.len * 0.5;
        let x = rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
        let from = Pos2::new(x, rows[1].center().y + 20.0);
        drag_held(&mut h, from, from + vec2(100.0, 0.0), mods);
        let s = h.app.session.as_ref().unwrap();
        let (ti, ci) = s.find_clip(id).expect("the clip");
        s.tracks[ti].clips[ci].start - before
    };
    let free = clip_move(Default::default());
    let fine = clip_move(ctrl);
    assert!(free > 0.3, "the free drag moved the clip: {free}");
    let ratio = fine / free;
    assert!((0.1..0.35).contains(&ratio), "Ctrl moves the clip about a fifth as far: {fine} vs {free}");
}

// ---- inputs and outputs ---------------------------------------------------------------

/// The person's request: several inputs and outputs, kept simple. A track sent to a hardware
/// pair ("Out 3/4") plays out of those channels instead of the Master during playback, while a
/// mixdown still holds it; a track sent to "None" is silent.
#[test]
fn a_track_can_go_straight_to_a_hardware_output_pair() {
    let h = Harness::new(true, true);
    let mut s = h.app.session.clone().unwrap();
    assert_eq!(crate::session::output_pair("Out 3/4"), Some(2));
    assert_eq!(crate::session::output_pair("Master"), None);
    // only the beat track (track 2) plays, sent to outputs 3/4
    s.tracks[0].mute = true;
    s.tracks[1].io.output = "Out 3/4".into();
    let files = &h.app.files;
    let look = |id: u64| files.iter().find(|f| f.id == id).map(|f| f.data.clone());
    let mix = s.snapshot(&look);
    assert_eq!(mix.tracks[1].direct, Some(2));
    let peak = |b: &[f32]| b.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    // live playback: the Master is silent, channels 3/4 carry the track
    let mut live = crate::session::ChainCache::live();
    live.sync(&mix, s.rate as f32);
    let (mut out, mut tmp, mut pk) = (vec![], vec![], vec![]);
    crate::session::render_mix(&mix, &mut live, s.rate as f32, 0.5, 4096, &mut out, &mut tmp, &mut pk);
    assert!(peak(&out[0]) < 1e-6 && peak(&out[1]) < 1e-6, "not in the Master");
    assert!(live.direct.len() >= 4, "buffers for outputs 1-4");
    assert!(peak(&live.direct[2]) > 1e-3 && peak(&live.direct[3]) > 1e-3, "on outputs 3/4");
    // a mixdown keeps every track
    let d = crate::session::mixdown(&mix, s.rate, 0.5, 0.6);
    assert!(peak(&d.ch[0]) > 1e-3, "the export holds the track");
    // "None": silent
    s.tracks[1].io.output = "None".into();
    let mix = s.snapshot(&look);
    let d = crate::session::mixdown(&mix, s.rate, 0.5, 0.6);
    assert!(peak(&d.ch[0]) < 1e-6, "a track sent nowhere is silent");
}

/// Inputs: the menus list every stereo pair and every single input of the device, and a
/// multichannel recording becomes each track's own take (mono input: one channel; pair: two).
#[test]
fn tracks_record_from_any_input_or_pair() {
    let mut h = Harness::new(true, true);
    h.app.engine.in_known = 4;
    let ins = h.app.input_choices();
    assert_eq!(
        ins,
        vec![
            "Stereo In 1/2",
            "Stereo In 3/4",
            "Mono In 1",
            "Mono In 2",
            "Mono In 3",
            "Mono In 4"
        ]
    );
    assert_eq!(h.app.output_pairs(), vec!["Out 1/2"], "a stereo device has one pair");
    let mut rec = crate::audio::AudioData::new(48000, 4, 10);
    for (c, ch) in rec.ch.iter_mut().enumerate() {
        for v in ch.iter_mut() {
            *v = c as f32 + 1.0;
        }
    }
    let mono = crate::app::input_take(&rec, crate::extras::input_channels("Mono In 3"));
    assert_eq!(mono.channels(), 1);
    assert_eq!(mono.ch[0][0], 3.0, "input 3");
    let pair = crate::app::input_take(&rec, crate::extras::input_channels("Stereo In 3/4"));
    assert_eq!(pair.channels(), 2);
    assert_eq!((pair.ch[0][0], pair.ch[1][0]), (3.0, 4.0), "inputs 3 and 4");
    // a choice past the device's last input falls back to the last one
    let far = crate::app::input_take(&rec, crate::extras::input_channels("Mono In 9"));
    assert_eq!(far.ch[0][0], 4.0);
}

/// Preferences that used to be saved but read by nothing now take effect: the session's
/// history limit, new tracks' lane height and Pre-render, automatic crossfades, and how the
/// view follows playback.
#[test]
fn once_dead_preferences_now_take_effect() {
    let mut p = crate::prefs::Prefs::default();
    p.history_limit = 5;
    p.default_lane_height = 200.0;
    p.pre_render_tracks = true;
    p.auto_crossfade = false;
    p.auto_scroll_mode = 1;
    crate::prefs::set_live(&p);
    // new tracks
    let mut s = crate::session::Session::new("t", 48000, 0);
    s.add_track(true);
    assert!(s.tracks[0].prerender, "new tracks start pre-rendered when asked");
    assert_eq!(s.tracks[0].auto.lane_h, 200.0);
    // no automatic crossfade: two overlapping clips keep plain fades
    let mut h = Harness::new(true, true);
    // starting the app applies its own (default) preferences: ask for ours again
    crate::prefs::set_live(&p);
    {
        let s = h.app.session.as_mut().unwrap();
        let c0 = s.tracks[1].clips[0].clone();
        s.tracks[1].clips[1].start = c0.start + c0.len * 0.5;
    }
    let fades = crate::session::effective_fades(&h.app.session.as_ref().unwrap().tracks[1]);
    assert!(!fades[0].3 && !fades[1].2, "no crossfade while the setting is off: {fades:?}");
    crate::prefs::set_live(&crate::prefs::Prefs::default());
    let fades = crate::session::effective_fades(&h.app.session.as_ref().unwrap().tracks[1]);
    assert!(fades[0].3 && fades[1].2, "the automatic crossfade is back: {fades:?}");
    // the history limit
    crate::prefs::set_live(&p);
    for _ in 0..12 {
        h.app.session.as_mut().unwrap().tracks[0].volume_db -= 0.5;
        h.app.session_push("Edit");
    }
    assert!(h.app.session_hist.len() <= 5, "{}", h.app.session_hist.len());
    crate::prefs::set_live(&crate::prefs::Prefs::default());
    // following the playhead
    use crate::prefs::follow_view;
    assert_eq!(follow_view((0.0, 10.0), 4.0, None, 0), None, "page mode: still in view");
    assert_eq!(follow_view((0.0, 10.0), 11.0, None, 0), Some((11.0, 21.0)), "a new page");
    assert_eq!(follow_view((0.0, 10.0), 4.0, None, 1), None, "centred: before the middle");
    assert_eq!(follow_view((0.0, 10.0), 6.0, None, 1), Some((1.0, 11.0)), "centred: held mid");
    assert_eq!(follow_view((0.0, 10.0), 50.0, None, 2), None, "off");
    assert_eq!(follow_view((0.0, 10.0), 11.0, Some(8.0), 0), Some((8.0, 18.0)), "file end");
}

/// Auto Save: a backup copy of a changed session is written beside it, the newest few are
/// kept, and a backup opens with its clips.
#[test]
fn auto_save_writes_and_prunes_backups() {
    let dir = std::env::temp_dir().join(format!("ac-autosave-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = Harness::new(true, true);
    h.app.prefs.autosave_keep = 2;
    h.app.session.as_mut().unwrap().path = Some(dir.join("s.acsession"));
    // the demo audio gets a place on disk with the first save; then the session changes
    h.app.save_session(false);
    let clips: usize = h.app.session.as_ref().unwrap().tracks.iter().map(|t| t.clips.len()).sum();
    assert!(clips > 0);
    let p1 = h.app.write_backup().expect("backup 1");
    assert!(p1.exists() && p1.parent().unwrap().ends_with("Backups"), "{}", p1.display());
    let _p2 = h.app.write_backup().expect("backup 2");
    let p3 = h.app.write_backup().expect("backup 3");
    let left: Vec<_> = std::fs::read_dir(dir.join("Backups")).unwrap().flatten().collect();
    assert_eq!(left.len(), 2, "only the newest two are kept");
    assert!(!p1.exists(), "the oldest went");
    // the backup opens with every clip
    let mut h2 = Harness::new(false, false);
    h2.app.load_session(&p3);
    let s2 = h2.app.session.as_ref().expect("the backup opened");
    assert_eq!(s2.tracks.iter().map(|t| t.clips.len()).sum::<usize>(), clips);
    assert!(h2.app.offline.is_empty(), "its audio was found");
    // the timer: due, and the session changed since the last backup
    h.app.prefs.autosave_on = true;
    h.app.prefs.autosave_minutes = 1;
    h.app.autosave_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(61));
    {
        let s = h.app.session.as_mut().unwrap();
        s.tracks[0].volume_db = -3.0;
        s.touch();
    }
    // (the harness runs in screenshot mode, where timers like this stay quiet)
    let shot = h.app.screenshot.take();
    h.app.autosave_tick();
    assert!(h.app.status.starts_with("Backup saved"), "{}", h.app.status);
    // nothing changed: no second backup at the next due time
    let n_before = std::fs::read_dir(dir.join("Backups")).unwrap().count();
    h.app.autosave_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(61));
    h.app.status.clear();
    h.app.autosave_tick();
    assert!(!h.app.status.starts_with("Backup saved"), "{}", h.app.status);
    assert_eq!(std::fs::read_dir(dir.join("Backups")).unwrap().count(), n_before);
    h.app.screenshot = shot;
    let _ = std::fs::remove_dir_all(&dir);
}

/// Data preferences: a smooth delete crossfades the seam, dither is added to 16-bit files when
/// on (and not when off), and 32-bit float files above 0 dBFS are clipped on open when asked.
#[test]
fn data_preferences_dither_clip_and_smooth_delete() {
    // smooth delete
    let mut d = crate::audio::AudioData::new(1000, 1, 1000);
    for (i, v) in d.ch[0].iter_mut().enumerate() {
        *v = if i < 500 { 1.0 } else { -1.0 };
    }
    let plain = d.without_smooth(400, 600, 0);
    assert_eq!(plain.frames(), 800);
    assert_eq!((plain.ch[0][399], plain.ch[0][400]), (1.0, -1.0), "a plain cut jumps");
    let smooth = d.without_smooth(400, 600, 10);
    assert_eq!(smooth.frames(), 790, "the two sides overlap by the fade");
    let seam = &smooth.ch[0][390..400];
    assert!(seam.iter().all(|v| v.abs() < 1.0), "the seam passes through the middle: {seam:?}");
    assert!(seam[0] > 0.5 && seam[9] < -0.5, "from one side to the other: {seam:?}");
    // dither
    let tmp = std::env::temp_dir().join(format!("ac-dither-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let mut c = crate::audio::AudioData::new(8000, 1, 2000);
    for v in c.ch[0].iter_mut() {
        *v = 0.3;
    }
    let mut p = crate::prefs::Prefs::default();
    p.dither_on = false;
    crate::prefs::set_live(&p);
    crate::audio::save_wav(&tmp.join("flat.wav"), &c, 16, false).unwrap();
    let flat = crate::audio::load(&tmp.join("flat.wav")).unwrap().data;
    assert!(flat.ch[0].windows(2).all(|w| w[0] == w[1]), "no dither: every sample the same");
    p.dither_on = true;
    p.dither_depth = 1.0;
    crate::prefs::set_live(&p);
    crate::audio::save_wav(&tmp.join("dith.wav"), &c, 16, false).unwrap();
    let dith = crate::audio::load(&tmp.join("dith.wav")).unwrap().data;
    assert!(dith.ch[0].windows(2).any(|w| w[0] != w[1]), "dither: the samples vary");
    let lsb = 1.0 / 32767.0;
    assert!(dith.ch[0].iter().all(|v| (v - 0.3).abs() < 3.0 * lsb), "by a bit or two only");
    // float clip on open
    let mut hot = crate::audio::AudioData::new(8000, 1, 100);
    for v in hot.ch[0].iter_mut() {
        *v = 1.5;
    }
    crate::audio::save_wav(&tmp.join("hot.wav"), &hot, 32, true).unwrap();
    p.interpret_float_clip = false;
    crate::prefs::set_live(&p);
    assert_eq!(crate::audio::load(&tmp.join("hot.wav")).unwrap().data.ch[0][0], 1.5);
    p.interpret_float_clip = true;
    crate::prefs::set_live(&p);
    assert_eq!(crate::audio::load(&tmp.join("hot.wav")).unwrap().data.ch[0][0], 1.0);
    crate::prefs::set_live(&crate::prefs::Prefs::default());
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Effects preferences: a newly added effect starts at the default mix.
#[test]
fn new_effects_start_at_the_default_mix() {
    let mut h = Harness::new(true, true);
    let mut p = crate::prefs::Prefs::default();
    p.fx_default_mix = 40.0;
    crate::prefs::set_live(&p);
    h.app.rack_insert(
        crate::rack::RackTarget::Track(0),
        None,
        crate::dsp::Effect::Delay {
            time_ms: 100.0,
            feedback: 0.3,
            mix: 0.5,
        },
    );
    let slot = h.app.session.as_ref().unwrap().tracks[0].rack.last().cloned().unwrap();
    assert!((slot.mix - 0.4).abs() < 1e-6, "{}", slot.mix);
    crate::prefs::set_live(&crate::prefs::Prefs::default());
}

/// Multitrack Clips preferences: the default crossfade curve, smooth curves through fade
/// points, and "play overlapped portions" (off: a clip under a later one is not heard there).
#[test]
fn clip_preferences_shape_fades_and_overlaps() {
    let mut p = crate::prefs::Prefs::default();
    // the default crossfade curve
    p.default_crossfade_curve = 0;
    crate::prefs::set_live(&p);
    assert!(crate::session::default_fade_table(true, 0.0).is_empty(), "equal power: built in");
    p.default_crossfade_curve = 1;
    crate::prefs::set_live(&p);
    let lin = crate::session::default_fade_table(true, 0.0);
    assert!((crate::session::fade_at(&lin, 0.5, true) - 0.5).abs() < 1e-4, "linear");
    p.default_crossfade_curve = 2;
    crate::prefs::set_live(&p);
    let log = crate::session::default_fade_table(true, 0.0);
    let mid = crate::session::fade_at(&log, 0.5, true);
    assert!(mid < 0.1, "logarithmic: about -24 dB halfway ({mid})");
    assert!(crate::session::default_fade_table(false, 0.0).is_empty(), "plain fades untouched");
    // smooth curves through a fade point
    let pts = [(0.5, 0.2f32)];
    p.clip_fade_points_smooth = false;
    crate::prefs::set_live(&p);
    let straight = crate::curves::fade_gain(&pts, &[], 0.25, &|x| x as f32);
    assert!(straight > 0.0 && straight < 0.2, "between the ends when off: {straight}");
    p.clip_fade_points_smooth = true;
    crate::prefs::set_live(&p);
    let smooth = crate::curves::fade_gain(&pts, &[], 0.25, &|x| x as f32);
    assert!((smooth - straight).abs() > 1e-3, "a smooth curve when on: {smooth}");
    // overlaps
    let mut s = crate::session::Session::new("t", 48000, 0);
    s.add_track(true);
    let mk = |id: u64, start: f64, len: f64| crate::session::Clip {
        id,
        file_id: 1,
        name: String::new(),
        start,
        offset: 0.0,
        len,
        gain_db: 0.0,
        muted: false,
        env: vec![],
        pan_env: vec![],
        rack: vec![],
        speed: 1.0,
        fx: Default::default(),
        auto_in: true,
        auto_out: true,
        curves: Default::default(),
        fade_in: 0.0,
        fade_out: 0.0,
    };
    s.tracks[0].clips = vec![mk(1, 0.0, 4.0), mk(2, 1.0, 1.0)];
    p.play_overlap = false;
    crate::prefs::set_live(&p);
    let t = &s.tracks[0];
    let hidden = crate::session::hidden_ranges(t, &crate::session::effective_fades(t));
    assert_eq!(hidden[0], vec![(1.0, 2.0)], "the long clip is silent under the short one");
    assert!(hidden[1].is_empty());
    // a partial overlap with an automatic crossfade plays both
    s.tracks[0].clips = vec![mk(1, 0.0, 2.0), mk(2, 1.5, 2.0)];
    let t = &s.tracks[0];
    let hidden = crate::session::hidden_ranges(t, &crate::session::effective_fades(t));
    assert!(hidden.iter().all(|h| h.is_empty()), "crossfades play both: {hidden:?}");
    p.play_overlap = true;
    crate::prefs::set_live(&p);
    s.tracks[0].clips = vec![mk(1, 0.0, 4.0), mk(2, 1.0, 1.0)];
    let t = &s.tracks[0];
    let hidden = crate::session::hidden_ranges(t, &crate::session::effective_fades(t));
    assert!(hidden.iter().all(|h| h.is_empty()), "on: everything plays");
    crate::prefs::set_live(&crate::prefs::Prefs::default());
}

/// Playback preferences: Multitrack playback stops after the last clip once the sound has
/// died away (or runs on when that is off), and Play with Pre-roll and Post-roll plays the
/// selection with the extra time around it.
#[test]
fn playback_preferences_stop_at_end_and_preroll() {
    let mut h = Harness::new(true, true);
    h.frames(2);
    let len = h.app.session.as_ref().unwrap().length();
    h.app.prefs.stop_at_end = true;
    h.app.session.as_mut().unwrap().cursor = len - 0.5;
    h.app.play();
    assert_eq!(h.app.engine.lock().stop_after, Some(len));
    // the audio thread's work, by hand
    let rate = h.app.engine.lock().out_rate as f64;
    let mut played = 0.0;
    while h.app.engine.lock().playing && played < 10.0 {
        h.app.engine.lock().render(1024);
        played += 1024.0 / rate;
    }
    let pos = h.app.engine.lock().pos;
    assert!(!h.app.engine.lock().playing, "it stopped");
    assert!(pos > len && pos < len + 4.0, "after the last clip, once quiet: {pos} vs {len}");
    h.app.stop();
    h.app.prefs.stop_at_end = false;
    h.app.session.as_mut().unwrap().cursor = 0.0;
    h.app.play();
    assert_eq!(h.app.engine.lock().stop_after, None, "off: it runs on");
    h.app.stop();
    // pre-roll and post-roll around the selection
    h.app.prefs.preroll_s = 1.0;
    h.app.prefs.postroll_s = 0.5;
    {
        let s = h.app.session.as_mut().unwrap();
        s.selection = Some((2.0, 3.0));
        s.cursor = 2.0;
    }
    h.app.exec("transport.play_preroll", &h.ctx.clone()).unwrap();
    let e = h.app.engine.lock();
    assert!(e.playing);
    assert!((e.pos - 1.0).abs() < 1e-6 && (e.end - 3.5).abs() < 1e-6, "{} - {}", e.pos, e.end);
}

/// Media & Disk Cache: peaks of an opened file are saved in the media cache and read back the
/// next time; the cache keeps under its limit.
#[test]
fn peak_files_are_cached_and_pruned() {
    let dir = std::env::temp_dir().join(format!("ac-peaks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tone.wav");
    let d = crate::dsp::gen_tone(48000, 2, 0.5, 440.0, crate::dsp::Wave::Sine, -6.0);
    crate::audio::save_wav(&wav, &d, 16, false).unwrap();
    let data = crate::audio::load(&wav).unwrap().data;
    let mut p = crate::prefs::Prefs::default();
    p.save_peak_files = true;
    crate::prefs::set_live(&p);
    let built = crate::peakcache::peaks_for(&wav, &data);
    let cached = crate::peakcache::load(&wav, &data).expect("saved in the cache");
    assert_eq!(cached.levels, built.levels, "the same peaks come back");
    // a changed file is not mistaken for the old one
    let d2 = crate::dsp::gen_tone(48000, 2, 0.4, 440.0, crate::dsp::Wave::Sine, -6.0);
    std::thread::sleep(std::time::Duration::from_millis(20));
    crate::audio::save_wav(&wav, &d2, 16, false).unwrap();
    let data2 = crate::audio::load(&wav).unwrap().data;
    assert!(crate::peakcache::load(&wav, &data2).is_none(), "the changed file has no cache yet");
    // the limit
    crate::peakcache::prune(0);
    assert!(crate::peakcache::load(&wav, &data).is_none());
    crate::prefs::set_live(&crate::prefs::Prefs::default());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Bus tracks: a track's output can go into a bus (which then has the last word with its own
/// fader), a send adds a copy of the track into a bus, a soloed track's bus stays audible,
/// and renaming a bus keeps every route to it.
#[test]
fn busses_take_outputs_and_sends() {
    let h = Harness::new(true, true);
    let mut s = h.app.session.clone().unwrap();
    let files = &h.app.files;
    let look = |id: u64| files.iter().find(|f| f.id == id).map(|f| f.data.clone());
    let peak = |s: &crate::session::Session| -> f32 {
        let mix = s.snapshot(&look);
        let d = crate::session::mixdown(&mix, s.rate, 0.5, 0.7);
        d.ch[0].iter().fold(0.0f32, |m, v| m.max(v.abs()))
    };
    // only the beat track (track 2) plays
    s.tracks[0].mute = true;
    let alone = peak(&s);
    assert!(alone > 1e-3);
    let b = s.add_bus();
    let bid = s.tracks[b].id;
    let key = crate::session::Session::bus_name_for(bid);
    assert_eq!(s.busses(), vec![(bid, "Bus 1".to_string())]);
    // the whole track into the bus: the bus fader decides
    s.tracks[1].io.output = key.clone();
    assert!((peak(&s) - alone).abs() < 1e-3, "through a unity bus it sounds the same");
    s.tracks[b].volume_db = -60.0;
    assert!(peak(&s) < alone * 0.01, "the bus fader turns it down");
    s.tracks[b].volume_db = 0.0;
    // a send: the track to the Master and a copy into the bus
    s.tracks[1].io.output = "Master".into();
    s.tracks[1].send.on = true;
    s.tracks[1].send.target = key.clone();
    s.tracks[1].send.level_db = 0.0;
    let both = peak(&s);
    assert!((both / alone - 2.0).abs() < 0.05, "track + its send: twice as loud ({both} vs {alone})");
    s.tracks[1].send.on = false;
    assert!((peak(&s) - alone).abs() < 1e-3, "a send switched off adds nothing");
    s.tracks[1].send.on = true;
    // solo the track: its bus stays audible
    s.tracks[1].solo = true;
    assert!((peak(&s) / alone - 2.0).abs() < 0.05, "the soloed track's bus is still heard");
    // a renamed bus keeps its routes
    s.tracks[b].name = "Reverb Bus".into();
    assert_eq!(s.route_label(&s.tracks[1].send.target), "Reverb Bus");
    assert!((peak(&s) / alone - 2.0).abs() < 0.05);
    // a bus never feeds itself
    s.tracks[b].io.output = key.clone();
    let mix = s.snapshot(&look);
    assert_eq!(mix.tracks[b].to_bus, None);
}
