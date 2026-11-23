//! Bridge to the vendored SoundCraft plug-in layer (ArtCraft, MIT OR Apache-2.0):
//! SoundCraft's built-in effects (`soundcraft_dsp`) and third-party CLAP/VST3 plug-ins
//! (`soundcraft_clap_host`, `soundcraft_vst3_host`) all share one `Plugin` trait.
//!
//! Instances are created on the UI thread (VST3 requires it). The audio thread only processes.

use crate::dsp::{Effect, Processor};
use soundcraft_dsp::{Category, ParamInfo, Plugin, PluginInfo, Unit};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once a hosted (CLAP/VST3) binary has been loaded into this process.
static HOSTED_LOADED: AtomicBool = AtomicBool::new(false);

/// Hosting of third-party plug-ins is off in tests (they must never load the machine's real
/// plug-ins) and when `AUDIOCRAFT_NO_PLUGIN_SCAN=1` is set.
pub fn hosting_disabled() -> bool {
    static D: OnceLock<bool> = OnceLock::new();
    *D.get_or_init(|| {
        cfg!(test)
            || std::env::var("AUDIOCRAFT_NO_PLUGIN_SCAN")
                .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
                .unwrap_or(false)
    })
}

/// The folders to scan instead of the standard CLAP/VST3 folders: `AUDIOCRAFT_PLUGIN_PATH`
/// (a PATH-style list), or an empty private folder in tests. `None` means the standard folders.
pub fn search_override() -> Option<Vec<PathBuf>> {
    if cfg!(test) {
        let d = std::env::temp_dir().join(format!("audiocraft-no-plugins-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        return Some(vec![d]);
    }
    let v = std::env::var_os("AUDIOCRAFT_PLUGIN_PATH")?;
    Some(
        std::env::split_paths(&v)
            .filter(|p| !p.as_os_str().is_empty())
            .collect(),
    )
}

/// Apply the search-folder policy to both hosts (once per process).
pub fn configure() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        if let Some(dirs) = search_override() {
            soundcraft_clap_host::set_search_paths(Some(dirs.clone()));
            soundcraft_vst3_host::set_search_paths(Some(dirs));
        }
        // the folders the person added in the Plug-in Manager
        set_scan_folds(&crate::prefs::load().vst_scan_folders);
    });
}

/// The user-added VST3 folders now in effect, and the ones removed during this run (their
/// plug-ins are hidden from the lists until the next start, when they are no longer scanned).
fn user_folders() -> &'static std::sync::Mutex<(Vec<PathBuf>, Vec<PathBuf>)> {
    static F: OnceLock<std::sync::Mutex<(Vec<PathBuf>, Vec<PathBuf>)>> = OnceLock::new();
    F.get_or_init(|| std::sync::Mutex::new((vec![], vec![])))
}

/// What changed between the folders in effect and the wanted ones: (added, removed).
pub fn folder_changes(old: &[PathBuf], new: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut added: Vec<PathBuf> = vec![];
    for d in new {
        if !old.contains(d) && !added.contains(d) {
            added.push(d.clone());
        }
    }
    let removed = old.iter().filter(|d| !new.contains(d)).cloned().collect();
    (added, removed)
}

/// Point VST3 scanning at the given user-added folders: they are scanned in addition to the
/// standard folders. Only `add_search_dir` (part of every version of the VST3 host) is used,
/// so this builds against the original SoundCraft host as well as the vendored one.
pub fn set_scan_folds(dirs: &[PathBuf]) {
    let mut g = user_folders()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (added, removed) = folder_changes(&g.0, dirs);
    if added.is_empty() && removed.is_empty() {
        return;
    }
    g.1.retain(|d| !added.contains(d));
    g.1.extend(removed);
    g.0 = dirs.to_vec();
    drop(g);
    if hosting_disabled() || added.is_empty() {
        return;
    }
    // the standard folders first (a plain add would otherwise start the list with only the
    // new folder), then each new folder is scanned and merged in
    let _ = soundcraft_vst3_host::scan();
    for d in &added {
        soundcraft_vst3_host::add_search_dir(d);
    }
}

/// Whether a VST3 bundle lives in a folder the person removed during this run.
fn in_removed_folder(path: &std::path::Path) -> bool {
    let g = user_folders()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    g.1.iter().any(|d| path.starts_with(d)) && !g.0.iter().any(|d| path.starts_with(d))
}

/// True once a hosted plug-in binary was loaded into this process.
pub fn hosted_loaded() -> bool {
    HOSTED_LOADED.load(Ordering::Acquire)
}

/// Gate every call that may load a plug-in binary.
fn allow_hosting() -> Result<(), String> {
    if hosting_disabled() {
        return Err(
            "plug-in scanning is disabled (AUDIOCRAFT_NO_PLUGIN_SCAN or a test run)".into(),
        );
    }
    configure();
    HOSTED_LOADED.store(true, Ordering::Release);
    Ok(())
}

/// Orderly shutdown on the main thread, after every plug-in instance has been dropped (each
/// instance's Drop already calls `terminate()` on its component and controller): release the
/// VST3 factories and run each module's exit (`ExitDll`) while the process is still fully alive,
/// instead of from an exit handler. Plug-in libraries are never unloaded (never FreeLibrary).
pub fn shutdown_hosting() {
    if hosted_loaded() {
        soundcraft_vst3_host::shutdown();
    }
}

/// After `shutdown_hosting`, when plug-in binaries were loaded: end AudioCraft's own process
/// at once, so the system does not run the plug-ins' DLL detach code or C++ static destructors
/// on the way out (some deadlock there). Every file save is already atomic and synced, so
/// nothing is pending. Not used when no plug-in was ever loaded.
pub fn finish_without_teardown() -> ! {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut core::ffi::c_void;
            fn TerminateProcess(process: *mut core::ffi::c_void, exit_code: u32) -> i32;
        }
        // SAFETY: Win32 calls on this process's own pseudo-handle; does not return on success.
        unsafe {
            TerminateProcess(GetCurrentProcess(), 0);
        }
    }
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn _exit(code: i32) -> !;
        }
        // SAFETY: `_exit` ends this process immediately without running exit handlers.
        unsafe { _exit(0) }
    }
    #[allow(unreachable_code)]
    std::process::exit(0)
}

/// One plug-in that can be inserted into a rack.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginEntry {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub format: &'static str,
    pub category: &'static str,
}

fn cat_name(c: Category) -> &'static str {
    match c {
        Category::Eq => "Filter and EQ",
        Category::Dynamics => "Amplitude and Compression",
        Category::PitchShift => "Time and Pitch",
        Category::Reverb => "Reverb",
        Category::Delay => "Delay and Echo",
        Category::Modulation => "Modulation",
        Category::Harmonic => "Special",
        Category::Dither => "Special",
        Category::Instrument => "Instrument",
        Category::Other => "Special",
    }
}

pub fn is_hosted(id: &str) -> bool {
    id.starts_with("clap:") || id.starts_with("vst3:")
}

/// SoundCraft's built-in realtime effects (instruments and generators excluded).
pub fn builtin_list() -> Vec<PluginEntry> {
    soundcraft_dsp::plugins()
        .iter()
        .filter(|i| !i.is_instrument && !soundcraft_dsp::is_generator(i.id))
        .map(|i| PluginEntry {
            id: i.id.to_string(),
            name: i.name.to_string(),
            vendor: "SoundCraft".into(),
            format: "Built-in",
            category: cat_name(i.category),
        })
        .collect()
}

/// Scan the standard CLAP and VST3 folders (loads plug-in binaries' entry points; no instances).
pub fn scan_hosted(rescan: bool) -> Vec<PluginEntry> {
    let mut v = vec![];
    if allow_hosting().is_err() {
        return v;
    }
    let clap = if rescan {
        soundcraft_clap_host::rescan()
    } else {
        soundcraft_clap_host::scan()
    };
    for d in clap.into_iter().filter(|d| !d.is_instrument) {
        v.push(PluginEntry {
            id: d.id,
            name: d.name,
            vendor: d.vendor,
            format: "CLAP",
            category: cat_name(d.category),
        });
    }
    let vst = if rescan {
        soundcraft_vst3_host::rescan()
    } else {
        soundcraft_vst3_host::scan()
    };
    for d in vst
        .into_iter()
        .filter(|d| !d.is_instrument && !in_removed_folder(&d.path))
    {
        v.push(PluginEntry {
            id: d.id,
            name: d.name,
            vendor: d.vendor,
            format: "VST3",
            category: cat_name(d.category),
        });
    }
    v.sort_by(|a, b| {
        a.format
            .cmp(b.format)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    v
}

pub fn info(id: &str) -> Option<&'static PluginInfo> {
    if is_hosted(id) {
        allow_hosting().ok()?;
    }
    if id.starts_with("clap:") {
        soundcraft_clap_host::plugin_info(id)
    } else if id.starts_with("vst3:") {
        soundcraft_vst3_host::plugin_info(id)
    } else {
        soundcraft_dsp::plugin_info(id)
    }
}

/// Create a plug-in instance by id. Errors are returned, never panics.
pub fn create(id: &str) -> Result<Box<dyn Plugin>, String> {
    if is_hosted(id) {
        allow_hosting()?;
    }
    if id.starts_with("clap:") {
        soundcraft_clap_host::instantiate(id).map_err(|e| e.to_string())
    } else if id.starts_with("vst3:") {
        soundcraft_vst3_host::instantiate(id).map_err(|e| e.to_string())
    } else {
        soundcraft_dsp::create(id).ok_or_else(|| format!("no built-in effect `{id}`"))
    }
}

/// A rack effect for a plug-in, with its default parameter values.
pub fn effect_for(entry: &PluginEntry) -> Effect {
    let params = info(&entry.id)
        .map(|i| {
            i.params
                .iter()
                .map(|p| (p.id.to_string(), p.default))
                .collect()
        })
        .unwrap_or_default();
    Effect::Plugin {
        id: entry.id.clone(),
        name: entry.name.clone(),
        params,
        state: None,
    }
}

/// The block size plug-ins are prepared for; longer buffers are fed in pieces of this size.
pub const PLUGIN_BLOCK: usize = 1024;

/// A rack processor running a plug-in. If the plug-in cannot be created the audio passes through.
///
/// The plug-in is created *and activated* here, on the UI thread (VST3 and CLAP expect
/// activation there; some plug-ins stay silent when it happens on the audio thread). It is
/// prepared for stereo: mono audio is fed to both of its inputs and its outputs are averaged,
/// so the audio thread never has to re-activate it. Settings and saved states arriving later
/// are applied to this same instance, so a live edit never rebuilds it.
pub struct PluginProc {
    plugin: Option<Box<dyn Plugin>>,
    id: String,
    max_block: usize,
    /// The saved state last loaded into the instance.
    state: Option<Vec<u8>>,
    /// Why the plug-in could not be created (the audio then passes through unchanged).
    #[cfg_attr(not(test), allow(dead_code))]
    pub error: Option<String>,
    scratch: Vec<Vec<f32>>,
}

impl PluginProc {
    pub fn new(id: &str, params: &[(String, f32)], state: Option<&[u8]>, rate: f32) -> Self {
        let (plugin, error) = match create(id) {
            Ok(mut p) => {
                if let Some(s) = state {
                    p.load_state(s);
                }
                for (k, v) in params {
                    p.set_param(k, *v);
                }
                p.prepare(rate, PLUGIN_BLOCK, 2);
                (Some(p), None)
            }
            Err(e) => (None, Some(e)),
        };
        PluginProc {
            plugin,
            id: id.to_string(),
            max_block: PLUGIN_BLOCK,
            state: state.map(|s| s.to_vec()),
            error,
            scratch: vec![],
        }
    }
}

impl Processor for PluginProc {
    fn process(&mut self, buf: &mut [Vec<f32>]) {
        let Some(p) = self.plugin.as_mut() else {
            return;
        };
        let n = buf.first().map(|c| c.len()).unwrap_or(0);
        if n == 0 {
            return;
        }
        // feed in chunks no larger than the prepared block size
        let mut pos = 0;
        while pos < n {
            let len = (n - pos).min(self.max_block);
            if pos == 0 && len == n {
                p.process(buf, n);
            } else {
                self.scratch.resize(buf.len(), vec![]);
                for (s, c) in self.scratch.iter_mut().zip(buf.iter()) {
                    s.clear();
                    s.extend_from_slice(&c[pos..pos + len]);
                }
                p.process(&mut self.scratch, len);
                for (s, c) in self.scratch.iter().zip(buf.iter_mut()) {
                    c[pos..pos + len].copy_from_slice(&s[..len]);
                }
            }
            pos += len;
        }
        for c in buf.iter_mut() {
            for s in c.iter_mut() {
                if !s.is_finite() {
                    *s = 0.0;
                }
            }
        }
    }

    /// Live changes (UI thread): a new saved state is loaded into the running instance and the
    /// settings are handed over, without rebuilding it (which would cut the sound and make a
    /// turned knob unheard until playback stops).
    fn update(&mut self, e: &Effect) -> bool {
        let Effect::Plugin {
            id, params, state, ..
        } = e
        else {
            return false;
        };
        if *id != self.id {
            return false;
        }
        if let Some(p) = self.plugin.as_mut() {
            if state.is_some() && *state != self.state {
                if let Some(s) = state {
                    p.load_state(s);
                }
                self.state = state.clone();
            }
            for (k, v) in params {
                p.set_param(k, *v);
            }
        }
        true
    }

    fn live_editor(
        &mut self,
    ) -> Option<(
        Box<dyn soundcraft_dsp::PluginEditor>,
        Option<Box<dyn soundcraft_dsp::PluginStateReader>>,
    )> {
        let p = self.plugin.as_mut()?;
        let ed = p.editor()?;
        Some((ed, p.state_reader()))
    }

    fn adopt_state(&mut self, s: &[u8]) {
        self.state = Some(s.to_vec());
    }

    fn tail(&self) -> usize {
        self.plugin.as_ref().map(|p| p.tail_samples()).unwrap_or(0)
    }
}

/// Generic parameter editor for a plug-in slot. Returns true when a value was committed.
pub fn params_ui(ui: &mut egui::Ui, id: &str, params: &mut Vec<(String, f32)>) -> bool {
    let Some(info) = info(id) else {
        ui.colored_label(
            egui::Color32::from_rgb(235, 170, 80),
            "This plug-in is not installed or failed to load. Audio passes through unchanged.",
        );
        return false;
    };
    let mut changed = false;
    if info.params.is_empty() {
        ui.label("This plug-in has no parameters.");
    }
    egui::Grid::new(ui.next_auto_id())
        .num_columns(3)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for pi in info.params.iter() {
                let cur = params
                    .iter()
                    .find(|(k, _)| k == pi.id)
                    .map(|(_, v)| *v)
                    .unwrap_or(pi.default);
                ui.label(pi.name);
                let mut v = cur;
                let r = param_widget(ui, pi, &mut v);
                ui.label(
                    egui::RichText::new(pi.format(v))
                        .monospace()
                        .color(crate::theme::time_blue()),
                );
                ui.end_row();
                let _ = r;
                if (v - cur).abs() > f32::EPSILON {
                    match params.iter_mut().find(|(k, _)| k == pi.id) {
                        Some(e) => e.1 = v,
                        None => params.push((pi.id.to_string(), v)),
                    }
                    changed = true;
                }
            }
        });
    changed
}

fn param_widget(ui: &mut egui::Ui, pi: &ParamInfo, v: &mut f32) -> egui::Response {
    match pi.unit {
        Unit::Toggle => {
            let mut on = *v >= 0.5;
            let r = ui.checkbox(&mut on, "");
            *v = if on { 1.0 } else { 0.0 };
            r
        }
        Unit::Choice if !pi.choices.is_empty() => {
            let mut idx = (*v).round().max(0.0) as usize;
            let r = egui::ComboBox::from_id_salt(ui.next_auto_id())
                .selected_text(pi.choices.get(idx).copied().unwrap_or("?"))
                .show_ui(ui, |ui| {
                    for (i, c) in pi.choices.iter().enumerate() {
                        ui.selectable_value(&mut idx, i, *c);
                    }
                })
                .response;
            let before = *v;
            *v = idx as f32;
            let mut r = r;
            if (before - *v).abs() > f32::EPSILON {
                r.mark_changed();
            }
            r
        }
        _ => {
            let mut t = pi.to_normalized(*v);
            let r = ui.add(egui::Slider::new(&mut t, 0.0..=1.0).show_value(false));
            *v = pi.from_normalized(t);
            r
        }
    }
}

/// An editor (GUI) window for a hosted plug-in, owned by the UI.
///
/// Normally it drives the very instance that is playing (`shared`): the plug-in's own window
/// then changes the sound at once, during playback too, because its controller and processor
/// are one plug-in talking to each other directly (formats allow GUI calls on the main thread
/// while the instance processes on the audio thread). When the effect is not running (bypassed,
/// another file, another editor mode) it falls back to a UI-owned instance whose edits are
/// copied to the slot.
pub struct OpenEditor {
    /// The UI-owned instance (fallback only).
    instance: Option<Box<dyn Plugin>>,
    editor: Option<Box<dyn soundcraft_dsp::PluginEditor>>,
    /// Reads the live instance's state when the window closes (shared editors).
    reader: Option<Box<dyn soundcraft_dsp::PluginStateReader>>,
}

impl OpenEditor {
    /// Open the plug-in's own window on a UI-owned instance (main thread).
    pub fn open(id: &str, params: &[(String, f32)], state: Option<&[u8]>) -> Result<Self, String> {
        let mut p = create(id)?;
        if let Some(s) = state {
            p.load_state(s);
        }
        for (k, v) in params {
            p.set_param(k, *v);
        }
        p.prepare(48_000.0, PLUGIN_BLOCK, 2);
        let mut editor = p.editor();
        match editor.as_mut() {
            Some(e) => e.open()?,
            None => p.open_editor()?,
        }
        Ok(OpenEditor {
            instance: Some(p),
            editor,
            reader: None,
        })
    }

    /// Open the window of the instance that is playing (from `Processor::live_editor`).
    pub fn open_live(
        mut editor: Box<dyn soundcraft_dsp::PluginEditor>,
        reader: Option<Box<dyn soundcraft_dsp::PluginStateReader>>,
    ) -> Result<Self, String> {
        editor.open()?;
        Ok(OpenEditor {
            instance: None,
            editor: Some(editor),
            reader,
        })
    }

    /// Whether this window drives the playing instance.
    pub fn is_live(&self) -> bool {
        self.instance.is_none()
    }

    /// Service the window; returns parameter edits made in it.
    pub fn poll(&mut self) -> Vec<(String, f32)> {
        self.editor.as_mut().map(|e| e.idle()).unwrap_or_default()
    }
    pub fn is_open(&self) -> bool {
        self.editor.as_ref().map(|e| e.is_open()).unwrap_or(true)
    }
    /// Close the window and return the plug-in's full state for the session.
    pub fn close(mut self) -> Option<Vec<u8>> {
        match (self.editor.as_mut(), self.instance.as_mut()) {
            (Some(e), _) => e.close(),
            (None, Some(p)) => p.close_editor(),
            (None, None) => {}
        }
        match (self.reader.as_mut(), self.instance.as_mut()) {
            (Some(r), _) => r.save_state(),
            (None, Some(p)) => p.save_state(),
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_process_without_nan() {
        let list = builtin_list();
        assert!(list.len() > 5);
        for e in &list {
            let mut p = PluginProc::new(&e.id, &[], None, 48000.0);
            assert!(p.error.is_none(), "{}: {:?}", e.id, p.error);
            let mut buf = vec![
                (0..3000)
                    .map(|i| ((i as f32) * 0.05).sin() * 0.5)
                    .collect::<Vec<f32>>();
                2
            ];
            p.process(&mut buf);
            assert!(
                buf.iter().flat_map(|c| c.iter()).all(|v| v.is_finite()),
                "{}",
                e.id
            );
        }
    }

    /// Regression for the `cargo test` hang: tests loaded the machine's real VST3s and the test
    /// process then deadlocked while those DLLs were torn down at exit.
    #[test]
    fn tests_never_scan_or_load_real_plugins() {
        assert!(hosting_disabled());
        assert!(scan_hosted(false).is_empty());
        assert!(scan_hosted(true).is_empty());
        assert!(info("vst3:00000000000000000000000000000000").is_none());
        assert!(info("clap:com.example.none").is_none());
        assert!(create("vst3:00000000000000000000000000000000").is_err());
        assert!(!hosted_loaded(), "a test loaded a plug-in binary");
        let dirs = search_override().unwrap_or_default();
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].starts_with(std::env::temp_dir()));
        assert!(
            std::fs::read_dir(&dirs[0])
                .map(|mut d| d.next().is_none())
                .unwrap_or(true)
        );
    }

    /// Loads the machine's real plug-ins. Never part of a normal run; start it by hand with
    /// `cargo test --target-dir target-alt -- --ignored --test-threads=1`.
    #[test]
    #[ignore]
    fn real_plugins_scan_and_load() {
        let all = soundcraft_vst3_host::scan();
        for d in all.iter().take(3) {
            let _ = soundcraft_vst3_host::instantiate(&d.id);
        }
        soundcraft_vst3_host::shutdown();
    }

    #[test]
    fn missing_plugins_pass_audio_through() {
        let mut p = PluginProc::new("vst3:00000000000000000000000000000000", &[], None, 48000.0);
        assert!(p.error.as_deref().unwrap_or("").contains("disabled"));
        assert!(!hosted_loaded());
        let mut buf = vec![vec![0.25f32; 64]; 2];
        p.process(&mut buf);
        assert!(buf[0].iter().all(|v| (*v - 0.25).abs() < 1e-7));
        let mut q = PluginProc::new("no_such_effect", &[], None, 48000.0);
        q.process(&mut buf);
        assert!(q.error.is_some());
    }
}
