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
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                events: events.take().unwrap_or_default(),
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
    let s = h.app.session.as_mut().unwrap();
    assert_eq!(s.group_of(a).len(), 2);
    let start_b = {
        let (t, i) = s.find_clip(b).unwrap();
        s.tracks[t].clips[i].start
    };
    s.shift_group(a, 1.5);
    let moved_b = {
        let (t, i) = s.find_clip(b).unwrap();
        s.tracks[t].clips[i].start
    };
    assert!(
        (moved_b - start_b - 1.5).abs() < 1e-9,
        "the other clip of the group moved"
    );
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
                assert_eq!(s.selected_clip, None, "{tool:?}: no clip stays selected with a keyframe");
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
            assert!(s.find_clip(target).is_none(), "{tool:?} click_b={click_b}: the clip was deleted");
            if !click_b {
                let (bt, bi) = s.find_clip(b).unwrap();
                assert_eq!(s.tracks[bt].clips[bi].env.len(), 2, "{tool:?}: the keyframe stayed");
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
        (rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width(), spot0.y)
    };
    h.click(Pos2::new(x, y));
    {
        let s = h.app.session.as_ref().unwrap();
        assert_eq!(s.tracks[0].clips[0].env.len(), 3, "a keyframe was added on the line");
        assert_eq!(h.app.multi_drag.key_sel.target, Some(EnvTarget::ClipVol(b)));
        assert_eq!(s.selected_clip, None, "the clip picked before is no longer selected");
        assert!(s.selected_clips.is_empty());
    }
    let ctx = h.ctx.clone();
    h.app.run("delete", &ctx);
    {
        let s = h.app.session.as_ref().unwrap();
        assert!(s.find_clip(a).is_some(), "clip A stays");
        assert_eq!(s.tracks[0].clips[0].env.len(), 2, "only the new keyframe went");
    }
    // pick a keyframe, then select the track's clips from the menu: the keyframe is dropped
    h.app.multi_drag.key_sel.only(EnvTarget::ClipVol(b), 1);
    h.frames(2);
    h.app.exec("edit.select_track_clips", &ctx).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.as_ref().unwrap().selected_clip, Some(b));
    assert!(h.app.multi_drag.key_sel.is_empty(), "selecting clips drops the keyframes");
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
    h.app.multi_drag.key_sel.only(crate::keys::EnvTarget::ClipVol(b), 0);
    h.frames(2);
    let s = h.app.session.as_ref().unwrap();
    assert_eq!(s.selected_clip, None, "picking a keyframe deselects every clip");
    assert!(s.selected_clips.is_empty());
    assert_eq!(s.selected_track, 0, "the keyframe's track is the current one");
    // click clip A in its body, away from the visible pan/envelope line (clicking that line
    // correctly adds a new keyframe instead of just picking the clip)
    let (rect, rows, view) = h.app.multi_drag.geom.clone().expect("layout");
    let t = a_start + a_len * 0.5;
    let x = rect.left() + ((t - view.0) / (view.1 - view.0)) as f32 * rect.width();
    let y = rows[1].center().y + 32.0;
    h.click(Pos2::new(x, y));
    assert!(h.app.multi_drag.key_sel.is_empty(), "clicking the clip dropped the keyframe");
    assert_eq!(h.app.session.as_ref().unwrap().selected_clip, Some(a));
    // Delete removes clip A, and B keeps its keyframes
    let ctx = h.ctx.clone();
    h.app.run("delete", &ctx);
    let s = h.app.session.as_ref().unwrap();
    assert!(s.find_clip(a).is_none(), "clip A was deleted");
    let (bt, bi) = s.find_clip(b).unwrap();
    assert_eq!(s.tracks[bt].clips[bi].env.len(), 2, "the keyframe was left alone");
}
