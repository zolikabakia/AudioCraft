//! Offline media and the Relink Media window.
//!
//! A session whose media cannot be found keeps its clips: each missing file becomes an
//! *offline* placeholder (silent, marked "Offline" in the Files panel) that remembers where it
//! was expected, so saving keeps the link and Relink Media can point it at the real file later.
//! Relink Media searches a folder (only that folder, or it and every folder inside it) by file
//! name with detector options, or lets the person locate each file by hand.

use crate::app::{App, AudioFile};
use crate::library::{self, Detect};
use crate::theme;
use egui::RichText;
use std::path::{Path, PathBuf};

/// A media file the session uses but that could not be found.
#[derive(Clone, Debug, Default)]
pub struct Offline {
    /// Where it is expected (relative to the session's library, see [`crate::library`]).
    pub expected: PathBuf,
}

/// One row of the Relink Media window.
#[derive(Clone, Debug)]
pub struct RelinkRow {
    pub file_id: u64,
    pub name: String,
    pub expected: PathBuf,
    pub found: Vec<PathBuf>,
    /// The candidate to relink to (index into `found`).
    pub pick: usize,
    pub note: String,
}

/// The Relink Media window's state.
pub struct RelinkState {
    pub folder: String,
    pub detect: Detect,
    /// Only a candidate at least as long as the audio its clips use counts.
    pub check_length: bool,
    /// List every file the session uses, not only the offline ones.
    pub all_files: bool,
    pub rows: Vec<RelinkRow>,
    pub searching: Option<std::sync::mpsc::Receiver<Vec<Vec<PathBuf>>>>,
    pub message: String,
}

/// Directory entries one search may look at (a whole drive never hangs the window).
const SEARCH_LIMIT: usize = 400_000;

impl App {
    /// The ids of every file a clip of the session uses.
    fn session_file_ids(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self
            .session
            .iter()
            .flat_map(|s| s.tracks.iter())
            .flat_map(|t| t.clips.iter().map(|c| c.file_id))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Seconds of source audio the session's clips need from file `id`.
    pub fn needed_secs(&self, id: u64) -> f64 {
        self.session
            .iter()
            .flat_map(|s| s.tracks.iter())
            .flat_map(|t| t.clips.iter())
            .filter(|c| c.file_id == id)
            .map(|c| c.offset + c.len * c.speed.max(0.0))
            .fold(0.0, f64::max)
    }

    /// Make an offline placeholder for a file that is not where it is expected: a moment of
    /// silence with the expected path, so its clips stay in the session and saving keeps the
    /// link. Returns its file id.
    pub fn add_offline(&mut self, expected: PathBuf, rate: u32) -> u64 {
        let name = expected
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Missing media".into());
        let rate = rate.max(1);
        let data = crate::audio::AudioData::new(rate, 1, rate as usize);
        let id = self.add_file(name, Some(expected.clone()), data, 24, false, "Offline");
        self.offline.insert(id, Offline { expected });
        id
    }

    /// Open the Relink Media window (offline files listed; the session's library folder is
    /// where it searches first).
    pub fn open_relink(&mut self, message: &str) {
        let session_dir = self
            .session
            .as_ref()
            .and_then(|s| s.path.as_ref())
            .and_then(|p| p.parent())
            .map(Path::to_path_buf);
        let media: Vec<PathBuf> = self
            .session_file_ids()
            .iter()
            .filter_map(|id| self.file(*id).and_then(|f| f.path.clone()))
            .collect();
        let folder = match &session_dir {
            Some(d) => library::library_root(d, &media),
            None => media
                .first()
                .and_then(|p| p.parent())
                .map(Path::to_path_buf)
                .unwrap_or_default(),
        };
        let mut st = RelinkState {
            folder: folder.display().to_string(),
            detect: Detect::default(),
            check_length: true,
            all_files: false,
            rows: vec![],
            searching: None,
            message: message.to_string(),
        };
        st.all_files = self.offline.is_empty();
        self.relink = Some(st);
        self.relink_fill_rows();
    }

    fn relink_fill_rows(&mut self) {
        let Some(st) = self.relink.as_ref() else {
            return;
        };
        let all = st.all_files;
        let rows: Vec<RelinkRow> = self
            .session_file_ids()
            .into_iter()
            .filter(|id| all || self.offline.contains_key(id))
            .filter_map(|id| {
                let f = self.file(id)?;
                let expected = self
                    .offline
                    .get(&id)
                    .map(|o| o.expected.clone())
                    .or_else(|| f.path.clone())
                    .unwrap_or_default();
                Some(RelinkRow {
                    file_id: id,
                    name: expected
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| f.name.clone()),
                    expected,
                    found: vec![],
                    pick: 0,
                    note: String::new(),
                })
            })
            .collect();
        if let Some(st) = self.relink.as_mut() {
            st.rows = rows;
        }
    }

    /// Start a search in the window's folder with its detector settings (on a worker thread).
    pub fn relink_search(&mut self) {
        let Some(st) = self.relink.as_mut() else {
            return;
        };
        let root = PathBuf::from(st.folder.trim());
        if !root.is_dir() {
            st.message = format!("\"{}\" is not a folder.", root.display());
            return;
        }
        let names: Vec<String> = st.rows.iter().map(|r| r.name.clone()).collect();
        let d = st.detect;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(library::find(&root, &names, d, SEARCH_LIMIT));
        });
        st.searching = Some(rx);
        st.message = "Searching...".into();
    }

    /// Take a finished search's results (call every frame while the window is open).
    pub fn relink_poll(&mut self) {
        let Some(st) = self.relink.as_mut() else {
            return;
        };
        let Some(rx) = st.searching.as_ref() else {
            return;
        };
        let Ok(found) = rx.try_recv() else {
            return;
        };
        st.searching = None;
        let mut n = 0;
        for (row, f) in st.rows.iter_mut().zip(found) {
            if !f.is_empty() {
                n += 1;
            }
            row.found = f;
            row.pick = 0;
            row.note.clear();
        }
        st.message = format!("Found {n} of {} file(s).", st.rows.len());
    }

    /// Point file `id` at `path`: load it, keep the id (so every clip follows), and clear its
    /// offline mark. With `check_length`, a file shorter than its clips need is refused.
    pub fn relink_file(&mut self, id: u64, path: &Path, check_length: bool) -> Result<(), String> {
        let lf = crate::audio::load(path)?;
        let need = self.needed_secs(id);
        if check_length && lf.data.duration() + 0.01 < need {
            return Err(format!(
                "{} is {:.2} s long; its clips need {:.2} s",
                path.display(),
                lf.data.duration(),
                need
            ));
        }
        let idx = self
            .files
            .iter()
            .position(|f| f.id == id)
            .ok_or("that file is no longer open")?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let f = AudioFile::new(
            id,
            name,
            Some(path.to_path_buf()),
            lf.data,
            lf.bits,
            lf.float,
            "Relink",
        );
        self.files[idx] = f;
        self.offline.remove(&id);
        if let Some(s) = self.session.as_mut() {
            s.touch();
        }
        self.mix_key.0 = u64::MAX;
        self.refresh_mix(true);
        Ok(())
    }

    /// Relink every row that has a candidate picked.
    pub fn relink_apply(&mut self) {
        let Some(st) = self.relink.as_ref() else {
            return;
        };
        let check = st.check_length;
        let todo: Vec<(usize, u64, PathBuf)> = st
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.found.get(r.pick).map(|p| (i, r.file_id, p.clone())))
            .collect();
        let mut done = 0;
        let mut notes: Vec<(usize, String)> = vec![];
        for (i, id, p) in todo {
            match self.relink_file(id, &p, check) {
                Ok(()) => {
                    done += 1;
                    notes.push((i, "Relinked".into()));
                }
                Err(e) => notes.push((i, e)),
            }
        }
        let left = self.offline.len();
        if let Some(st) = self.relink.as_mut() {
            for (i, n) in notes {
                if let Some(r) = st.rows.get_mut(i) {
                    r.note = n;
                }
            }
            st.message = format!("Relinked {done} file(s); {left} still offline.");
        }
        if done > 0 {
            self.status = format!("Relinked {done} file(s)");
        }
    }

    /// The Relink Media window.
    pub fn relink_window(&mut self, ctx: &egui::Context) {
        if self.relink.is_none() {
            return;
        }
        self.relink_poll();
        let mut open = true;
        let mut search = false;
        let mut apply = false;
        let mut refill = false;
        let mut locate: Option<usize> = None;
        let mut browse = false;
        let offline_n = self.offline.len();
        egui::Window::new("Relink Media")
            .id(egui::Id::new("relink_media"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(680.0)
            .show(ctx, |ui| {
                let Some(st) = self.relink.as_mut() else {
                    return;
                };
                if !st.message.is_empty() {
                    ui.label(RichText::new(&st.message).color(theme::text()));
                }
                ui.label(
                    RichText::new(format!(
                        "{offline_n} file(s) offline. A session finds its media relative to its own folder, so moving the whole library folder (the one holding the session and all its media) keeps every link."
                    ))
                    .small()
                    .color(theme::text_dim()),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Search in:");
                    ui.add(
                        egui::TextEdit::singleline(&mut st.folder)
                            .desired_width((ui.available_width() - 90.0).max(120.0)),
                    );
                    if ui.button("Browse...").clicked() {
                        browse = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.radio_value(&mut st.detect.recursive, false, "This folder only");
                    ui.radio_value(
                        &mut st.detect.recursive,
                        true,
                        "This folder and every folder inside it",
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut st.detect.ignore_case, "Ignore upper/lower case");
                    ui.checkbox(
                        &mut st.detect.any_extension,
                        "Same name with any audio extension",
                    );
                    ui.checkbox(&mut st.check_length, "Must be long enough for its clips");
                    if ui
                        .checkbox(&mut st.all_files, "List all session files")
                        .changed()
                    {
                        refill = true;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        egui::Grid::new("relink_rows")
                            .num_columns(4)
                            .striped(true)
                            .spacing([10.0, 4.0])
                            .show(ui, |ui| {
                                ui.label(RichText::new("File").small().color(theme::text_dim()));
                                ui.label(
                                    RichText::new("Expected at").small().color(theme::text_dim()),
                                );
                                ui.label(RichText::new("Found").small().color(theme::text_dim()));
                                ui.label("");
                                ui.end_row();
                                for (i, r) in st.rows.iter_mut().enumerate() {
                                    ui.label(&r.name);
                                    ui.label(
                                        RichText::new(
                                            r.expected
                                                .parent()
                                                .map(|p| p.display().to_string())
                                                .unwrap_or_default(),
                                        )
                                        .small()
                                        .color(theme::text_dim()),
                                    )
                                    .on_hover_text(r.expected.display().to_string());
                                    if r.found.is_empty() {
                                        ui.label(
                                            RichText::new(if r.note.is_empty() {
                                                "-"
                                            } else {
                                                r.note.as_str()
                                            })
                                            .small()
                                            .color(theme::text_dim()),
                                        );
                                    } else {
                                        ui.vertical(|ui| {
                                            let shown = r
                                                .found
                                                .get(r.pick)
                                                .map(|p| p.display().to_string())
                                                .unwrap_or_default();
                                            egui::ComboBox::from_id_salt(("relink_pick", i))
                                                .width(260.0)
                                                .selected_text(shown)
                                                .show_ui(ui, |ui| {
                                                    for (k, p) in r.found.iter().enumerate() {
                                                        ui.selectable_value(
                                                            &mut r.pick,
                                                            k,
                                                            p.display().to_string(),
                                                        );
                                                    }
                                                });
                                            if !r.note.is_empty() {
                                                ui.label(
                                                    RichText::new(&r.note)
                                                        .small()
                                                        .color(theme::text_dim()),
                                                );
                                            }
                                        });
                                    }
                                    if ui.button("Locate...").clicked() {
                                        locate = Some(i);
                                    }
                                    ui.end_row();
                                }
                            });
                        if st.rows.is_empty() {
                            ui.label(
                                RichText::new("No media to relink.")
                                    .small()
                                    .color(theme::text_dim()),
                            );
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    let busy = st.searching.is_some();
                    if ui
                        .add_enabled(!busy && !st.rows.is_empty(), egui::Button::new("Search"))
                        .clicked()
                    {
                        search = true;
                    }
                    if busy {
                        ui.spinner();
                    }
                    let any = st.rows.iter().any(|r| !r.found.is_empty());
                    if ui
                        .add_enabled(any && !busy, egui::Button::new("Relink"))
                        .clicked()
                    {
                        apply = true;
                    }
                });
            });
        if browse {
            let start = self.relink.as_ref().map(|s| s.folder.clone()).unwrap_or_default();
            let mut dlg = rfd::FileDialog::new();
            if Path::new(&start).is_dir() {
                dlg = dlg.set_directory(&start);
            }
            if let Some(d) = dlg.pick_folder() {
                if let Some(st) = self.relink.as_mut() {
                    st.folder = d.display().to_string();
                }
            }
        }
        if refill {
            self.relink_fill_rows();
        }
        if search {
            self.relink_search();
        }
        if apply {
            self.relink_apply();
        }
        if let Some(i) = locate {
            let row = self.relink.as_ref().and_then(|s| s.rows.get(i).cloned());
            if let Some(row) = row {
                let mut dlg = rfd::FileDialog::new()
                    .set_title(format!("Locate {}", row.name))
                    .add_filter("Audio", library::AUDIO_EXTS);
                if let Some(d) = row.expected.parent().filter(|d| d.is_dir()) {
                    dlg = dlg.set_directory(d);
                }
                if let Some(p) = dlg.pick_file() {
                    let check = self.relink.as_ref().map(|s| s.check_length).unwrap_or(true);
                    let note = match self.relink_file(row.file_id, &p, check) {
                        Ok(()) => "Relinked".to_string(),
                        Err(e) => e,
                    };
                    if let Some(r) = self.relink.as_mut().and_then(|s| s.rows.get_mut(i)) {
                        r.note = note;
                    }
                }
            }
        }
        if self
            .relink
            .as_ref()
            .is_some_and(|s| s.searching.is_some())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if !open {
            self.relink = None;
        }
    }
}
