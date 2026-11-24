//! Workspaces: where the panels sit around the editor. Each workspace is a layout of panel
//! groups (tabbed stacks) in columns on the left and right, a row above the editor, a row under
//! it and an optional full-width row at the very bottom. The built-in workspaces follow the
//! classic audio editor arrangements; the person can save changes or new workspaces.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub enum PanelId {
    Files,
    Favorites,
    MediaBrowser,
    EffectsRack,
    Markers,
    Properties,
    History,
    Video,
    Tracks,
    Levels,
    SelectionView,
    Essential,
    Metadata,
    FreqAnalysis,
    PhaseAnalysis,
    PhaseMeter,
    AmpStats,
    MatchLoudness,
    Diagnostics,
    Mixer,
    Transport,
    Time,
    Zoom,
    PlugIns,
}

impl PanelId {
    pub const ALL: [PanelId; 24] = [
        PanelId::Files,
        PanelId::Favorites,
        PanelId::MediaBrowser,
        PanelId::EffectsRack,
        PanelId::Markers,
        PanelId::Properties,
        PanelId::History,
        PanelId::Video,
        PanelId::Tracks,
        PanelId::Levels,
        PanelId::SelectionView,
        PanelId::Essential,
        PanelId::Metadata,
        PanelId::FreqAnalysis,
        PanelId::PhaseAnalysis,
        PanelId::PhaseMeter,
        PanelId::AmpStats,
        PanelId::MatchLoudness,
        PanelId::Diagnostics,
        PanelId::Mixer,
        PanelId::Transport,
        PanelId::Time,
        PanelId::Zoom,
        PanelId::PlugIns,
    ];

    pub fn title(self) -> &'static str {
        match self {
            PanelId::Files => "Files",
            PanelId::Favorites => "Favorites",
            PanelId::MediaBrowser => "Media Browser",
            PanelId::EffectsRack => "Effects Rack",
            PanelId::Markers => "Markers",
            PanelId::Properties => "Properties",
            PanelId::History => "History",
            PanelId::Video => "Video",
            PanelId::Tracks => "Tracks",
            PanelId::Levels => "Levels",
            PanelId::SelectionView => "Selection/View",
            PanelId::Essential => "Essential Sound",
            PanelId::Metadata => "Metadata",
            PanelId::FreqAnalysis => "Frequency Analysis",
            PanelId::PhaseAnalysis => "Phase Analysis",
            PanelId::PhaseMeter => "Phase Meter",
            PanelId::AmpStats => "Amplitude Statistics",
            PanelId::MatchLoudness => "Match Loudness",
            PanelId::Diagnostics => "Diagnostics",
            PanelId::Mixer => "Mixer",
            PanelId::Transport => "Transport",
            PanelId::Time => "Time",
            PanelId::Zoom => "Zoom",
            PanelId::PlugIns => "Plug-ins",
        }
    }

    pub fn from_title(t: &str) -> Option<PanelId> {
        PanelId::ALL.into_iter().find(|p| p.title() == t)
    }
}

/// A stack of tabbed panels. `weight` > 0 shares the space in proportion; < 0 is a fixed size
/// in points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub tabs: Vec<PanelId>,
    pub active: usize,
    pub weight: f32,
}

fn g(tabs: &[PanelId], weight: f32) -> Group {
    Group {
        tabs: tabs.to_vec(),
        active: 0,
        weight,
    }
}

fn ga(tabs: &[PanelId], active: usize, weight: f32) -> Group {
    Group {
        tabs: tabs.to_vec(),
        active,
        weight,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Layout {
    /// Columns left of the editor (outermost first) and right of it (outermost first).
    pub left: Vec<Group>,
    pub left_w: f32,
    pub left2: Vec<Group>,
    pub left2_w: f32,
    pub right: Vec<Group>,
    pub right_w: f32,
    pub right2: Vec<Group>,
    pub right2_w: f32,
    /// A row above the editor and one under it (between the columns).
    pub top: Vec<Group>,
    pub top_h: f32,
    pub bottom: Vec<Group>,
    pub bottom_h: f32,
    /// A row across the whole window at the bottom.
    pub bottom2: Vec<Group>,
    pub bottom2_h: f32,
    /// Panels in windows of their own (Window > New Window, Undock Panel, the dual-monitor
    /// workspace's second screen).
    #[serde(default)]
    pub windows: Vec<FloatWin>,
}

/// A separate window holding panel groups stacked top to bottom.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FloatWin {
    pub id: u64,
    pub groups: Vec<Group>,
    pub size: [f32; 2],
}

impl Default for FloatWin {
    fn default() -> Self {
        FloatWin {
            id: 0,
            groups: vec![],
            size: [520.0, 640.0],
        }
    }
}

impl Layout {
    fn columns_mut(&mut self) -> Vec<&mut Vec<Group>> {
        let mut v = vec![
            &mut self.left,
            &mut self.left2,
            &mut self.right,
            &mut self.right2,
            &mut self.top,
            &mut self.bottom,
            &mut self.bottom2,
        ];
        for w in self.windows.iter_mut() {
            v.push(&mut w.groups);
        }
        v
    }

    pub fn contains(&self, p: PanelId) -> bool {
        [
            &self.left,
            &self.left2,
            &self.right,
            &self.right2,
            &self.top,
            &self.bottom,
            &self.bottom2,
        ]
        .into_iter()
        .chain(self.windows.iter().map(|w| &w.groups))
        .any(|c| c.iter().any(|g| g.tabs.contains(&p)))
    }

    /// Open a new, empty window for panels; returns its id.
    pub fn new_window(&mut self, tabs: Vec<PanelId>) -> u64 {
        let id = self.windows.iter().map(|w| w.id).max().unwrap_or(0) + 1;
        let groups = if tabs.is_empty() {
            vec![]
        } else {
            vec![Group {
                tabs,
                active: 0,
                weight: 1.0,
            }]
        };
        self.windows.push(FloatWin {
            id,
            groups,
            ..Default::default()
        });
        id
    }

    /// Move a panel out of the main window into a new window of its own.
    pub fn undock(&mut self, p: PanelId) -> u64 {
        self.remove(p);
        self.new_window(vec![p])
    }

    /// Move a panel's whole group into a new window.
    pub fn undock_group(&mut self, p: PanelId) -> u64 {
        let mut tabs = vec![p];
        for col in self.columns_mut() {
            if let Some(g) = col.iter().find(|g| g.tabs.contains(&p)) {
                tabs = g.tabs.clone();
                break;
            }
        }
        for t in &tabs {
            self.remove(*t);
        }
        self.new_window(tabs)
    }

    /// Put a window's panels back where they usually live and close the window.
    pub fn dock_window(&mut self, id: u64) {
        let Some(i) = self.windows.iter().position(|w| w.id == id) else {
            return;
        };
        let w = self.windows.remove(i);
        for g in w.groups {
            for t in g.tabs {
                self.show(t);
            }
        }
    }

    /// Add a panel to a window (into its last group).
    pub fn add_to_window(&mut self, id: u64, p: PanelId) {
        self.remove(p);
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            match w.groups.last_mut() {
                Some(g) => {
                    g.tabs.push(p);
                    g.active = g.tabs.len() - 1;
                }
                None => w.groups.push(Group {
                    tabs: vec![p],
                    active: 0,
                    weight: 1.0,
                }),
            }
        }
    }

    /// Bring a panel to the front of its group; returns false when it is not in the layout.
    pub fn activate(&mut self, p: PanelId) -> bool {
        for col in self.columns_mut() {
            for grp in col.iter_mut() {
                if let Some(i) = grp.tabs.iter().position(|t| *t == p) {
                    grp.active = i;
                    return true;
                }
            }
        }
        false
    }

    /// Take a panel out (an emptied group goes too).
    pub fn remove(&mut self, p: PanelId) {
        for col in self.columns_mut() {
            for grp in col.iter_mut() {
                grp.tabs.retain(|t| *t != p);
                if grp.active >= grp.tabs.len() {
                    grp.active = grp.tabs.len().saturating_sub(1);
                }
            }
            col.retain(|grp| !grp.tabs.is_empty());
        }
        // a window whose last panel left stays open (empty) so panels can be added to it
    }

    /// Show a panel: bring it forward, or add it where it usually lives.
    pub fn show(&mut self, p: PanelId) {
        if self.activate(p) {
            return;
        }
        use PanelId::*;
        if p == PlugIns {
            // plug-in editors are wide: a column of their own at the right edge (or, when a
            // narrow meter column is already there, the next column in), at least 440 wide
            if self.right2.is_empty() {
                self.right2.push(g(&[p], 1.0));
                self.right2_w = self.right2_w.max(440.0);
            } else {
                self.right.push(g(&[p], 1.0));
                self.right_w = self.right_w.max(440.0);
            }
            return;
        }
        let col = match p {
            Levels | SelectionView | PhaseMeter | Transport | Time | Zoom => &mut self.bottom,
            Essential | Metadata | FreqAnalysis | PhaseAnalysis | AmpStats | Mixer => {
                &mut self.right
            }
            MatchLoudness => &mut self.top,
            _ => &mut self.left,
        };
        if col.is_empty() {
            col.push(g(&[p], 1.0));
        } else {
            // join the middle group of that place
            let i = col.len() / 2;
            col[i].tabs.push(p);
            col[i].active = col[i].tabs.len() - 1;
        }
        if self.left_w <= 0.0 {
            self.left_w = 330.0;
        }
        if self.right_w <= 0.0 {
            self.right_w = 300.0;
        }
        if self.bottom_h <= 0.0 {
            self.bottom_h = 118.0;
        }
        if self.top_h <= 0.0 {
            self.top_h = 300.0;
        }
    }

    /// Close every other panel of `p`'s group (the panel menu's "Close Other Panels in
    /// Group"); the editor-like Mixer stays. Returns how many closed.
    pub fn close_others_in_group(&mut self, p: PanelId) -> usize {
        for col in self.columns_mut() {
            for grp in col.iter_mut() {
                if grp.tabs.contains(&p) {
                    let before = grp.tabs.len();
                    grp.tabs.retain(|t| *t == p || *t == PanelId::Mixer);
                    grp.active = grp.tabs.iter().position(|t| *t == p).unwrap_or(0);
                    return before - grp.tabs.len();
                }
            }
        }
        0
    }

    pub fn toggle(&mut self, p: PanelId) {
        if self.contains(p) {
            self.remove(p);
        } else {
            self.show(p);
        }
    }
}

/// Built-in workspaces, in menu order (the first three are shown as tabs in the toolbar).
pub const BUILT_IN: [&str; 11] = [
    "Default",
    "Edit Audio to Video",
    "Radio Production",
    "Essential Video Mixing",
    "Advanced Mixing",
    "Simple Editing",
    "Mastering and Analysis",
    "Loudness",
    "Restoration",
    "Classic",
    "Maximum Editing (Dual Monitor)",
];

pub fn preset(name: &str) -> Layout {
    use PanelId::*;
    let lv_sv = vec![g(&[Levels], 1.0), g(&[SelectionView], -370.0)];
    let base = Layout {
        left_w: 330.0,
        left2_w: 140.0,
        right_w: 300.0,
        right2_w: 80.0,
        top_h: 300.0,
        bottom_h: 118.0,
        bottom2_h: 104.0,
        ..Default::default()
    };
    match name {
        "Edit Audio to Video" => Layout {
            left: vec![
                g(&[Video, EffectsRack, Properties], 0.33),
                g(&[MediaBrowser, Files, Markers], 0.37),
                g(&[History], 0.3),
            ],
            left_w: 370.0,
            left2: vec![g(&[Tracks], 1.0)],
            bottom: lv_sv,
            ..base
        },
        "Radio Production" => Layout {
            left: vec![
                g(&[Files], 0.3),
                ga(&[Properties, EffectsRack], 1, 0.37),
                g(&[History], 0.33),
            ],
            right: vec![g(&[Metadata], 1.0)],
            right_w: 280.0,
            bottom: lv_sv,
            ..base
        },
        "Essential Video Mixing" => Layout {
            // the Media Browser shares the Files group as a tab: a squeezed group of its own
            // used to spill over the Effects Rack
            left: vec![
                g(&[Files, MediaBrowser, Markers], 0.3),
                g(&[EffectsRack], 0.45),
                g(&[SelectionView], 0.25),
            ],
            left_w: 310.0,
            top: vec![g(&[Properties], 1.0), g(&[Video], 1.0)],
            top_h: 330.0,
            left2: vec![g(&[Tracks], 1.0)],
            right: vec![g(&[Essential], 1.0)],
            right_w: 280.0,
            right2: vec![g(&[Levels], 1.0)],
            ..base
        },
        "Advanced Mixing" => Layout {
            left: vec![
                g(&[Video], 0.3),
                g(&[Files, Markers], 0.3),
                g(&[MediaBrowser], 0.25),
                g(&[SelectionView], 0.15),
            ],
            left_w: 260.0,
            top: vec![g(&[Tracks], 1.0)],
            top_h: 200.0,
            bottom: vec![g(&[Mixer], 1.0)],
            bottom_h: 300.0,
            right: vec![
                g(&[Metadata], 0.45),
                g(&[PhaseAnalysis], 0.42),
                g(&[PhaseMeter], 0.13),
            ],
            right_w: 260.0,
            right2: vec![g(&[Levels], 1.0)],
            ..base
        },
        "Simple Editing" => Layout {
            left: vec![
                g(&[Files, Markers], 0.3),
                ga(&[EffectsRack, Properties], 1, 0.4),
                ga(&[History, Video], 1, 0.3),
                g(&[SelectionView], -110.0),
            ],
            left_w: 350.0,
            right2: vec![g(&[Levels], 1.0)],
            ..base
        },
        "Mastering and Analysis" => Layout {
            left: vec![
                g(&[Files], 0.3),
                g(&[MediaBrowser, EffectsRack, Diagnostics, Properties], 0.4),
                g(&[History, Video], 0.3),
            ],
            right: vec![
                g(&[FreqAnalysis], 0.42),
                g(&[AmpStats, PhaseAnalysis, Markers], 0.58),
            ],
            right_w: 420.0,
            bottom: vec![
                g(&[Levels], 1.0),
                g(&[PhaseMeter], 1.4),
                g(&[SelectionView], -330.0),
            ],
            ..base
        },
        "Loudness" => Layout {
            left: vec![
                g(&[Files], 0.3),
                ga(&[MediaBrowser, Markers, Properties], 2, 0.37),
                g(&[AmpStats], 0.33),
            ],
            left_w: 440.0,
            top: vec![g(&[MatchLoudness], 1.0)],
            top_h: 300.0,
            bottom: lv_sv,
            ..base
        },
        "Restoration" => Layout {
            left: vec![
                g(&[Files, Favorites], 0.3),
                ga(
                    &[MediaBrowser, Diagnostics, EffectsRack, Markers, Properties],
                    1,
                    0.48,
                ),
                g(&[History], 0.22),
            ],
            top: vec![
                g(&[FreqAnalysis], 1.0),
                g(&[PhaseAnalysis], 1.3),
                g(&[AmpStats], 1.0),
            ],
            top_h: 320.0,
            bottom: lv_sv,
            ..base
        },
        "Classic" => Layout {
            left: vec![g(&[Files, EffectsRack, Favorites], 1.0)],
            bottom2: vec![
                g(&[Transport], -380.0),
                g(&[Time], -430.0),
                g(&[Zoom], -300.0),
                g(&[SelectionView], 1.0),
            ],
            bottom2_h: 120.0,
            ..Layout {
                bottom: vec![],
                ..base.clone()
            }
        }
        .with_levels_row(),
        // the editor fills the main window; the panels go to a second window that can be
        // moved to the other monitor
        "Maximum Editing (Dual Monitor)" => Layout {
            bottom: lv_sv,
            windows: vec![FloatWin {
                id: 1,
                groups: vec![
                    g(&[Files], 0.25),
                    g(&[MediaBrowser, Markers, EffectsRack, Properties], 0.45),
                    ga(&[History, Video, Mixer, Metadata], 0, 0.3),
                ],
                size: [900.0, 900.0],
            }],
            ..base
        },
        // Default
        _ => Layout {
            left: vec![
                // the rack gets the most room, as in the original's Default workspace
                ga(&[Files, Favorites], 0, 0.26),
                ga(&[MediaBrowser, EffectsRack, Markers, Properties], 1, 0.56),
                g(&[History], 0.18),
            ],
            bottom: lv_sv,
            ..base
        },
    }
}

impl Layout {
    /// Classic: the Levels meter runs across the whole window under the transport row.
    fn with_levels_row(mut self) -> Self {
        // kept as its own full-width row by putting it last in bottom2 rows is not possible
        // with one row, so the editor's bottom row holds it
        self.bottom = vec![g(&[PanelId::Levels], 1.0)];
        self.bottom_h = 90.0;
        self
    }
}

/// The workspaces: built-ins (with any saved changes) and the person's own, and the live layout.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workspaces {
    /// Saved layouts by name (built-ins appear here once their changes are saved).
    pub saved: Vec<(String, Layout)>,
    /// The person's own workspaces, in order.
    pub custom: Vec<String>,
    pub current: String,
    /// What is on screen now (may differ from the saved layout until saved or reset).
    pub layout: Layout,
    /// Layout before "Maximize Panel Group", to restore.
    #[serde(skip)]
    pub maximized: Option<Layout>,
}

impl Default for Workspaces {
    fn default() -> Self {
        Workspaces {
            saved: vec![],
            custom: vec![],
            current: "Default".into(),
            layout: preset("Default"),
            maximized: None,
        }
    }
}

fn store_path() -> Option<PathBuf> {
    Some(crate::prefs::config_dir()?.join("workspaces.json"))
}

impl Workspaces {
    pub fn load() -> Self {
        if cfg!(test) {
            return Workspaces::default();
        }
        store_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Workspaces>(&t).ok())
            .unwrap_or_default()
    }

    pub fn persist(&self) {
        if cfg!(test) {
            return;
        }
        if let Some(p) = store_path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(t) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(p, t);
            }
        }
    }

    /// All workspace names in menu order.
    pub fn names(&self) -> Vec<String> {
        BUILT_IN
            .iter()
            .map(|s| s.to_string())
            .chain(self.custom.iter().cloned())
            .collect()
    }

    /// The saved layout of a workspace (built-in preset unless changes were saved).
    pub fn saved_layout(&self, name: &str) -> Layout {
        self.saved
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, l)| l.clone())
            .unwrap_or_else(|| preset(name))
    }

    pub fn switch(&mut self, name: &str) {
        self.current = name.to_string();
        self.layout = self.saved_layout(name);
        self.maximized = None;
        self.persist();
    }

    pub fn reset(&mut self) {
        self.layout = self.saved_layout(&self.current.clone());
        self.maximized = None;
    }

    pub fn save_current(&mut self) {
        let name = self.current.clone();
        self.saved.retain(|(n, _)| *n != name);
        self.saved.push((name, self.layout.clone()));
        self.persist();
    }

    pub fn save_as(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if !self.names().iter().any(|n| n == name) {
            self.custom.push(name.to_string());
        }
        self.current = name.to_string();
        self.save_current();
    }

    pub fn delete(&mut self, name: &str) {
        self.custom.retain(|n| n != name);
        self.saved.retain(|(n, _)| n != name);
        if self.current == name {
            self.switch("Default");
        }
        self.persist();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_workspace_has_an_editor_layout_and_panels_can_move() {
        for n in BUILT_IN {
            let l = preset(n);
            assert!(
                l.contains(PanelId::Levels) || n.starts_with("Maximum"),
                "{n} shows the levels"
            );
        }
        let mut l = preset("Default");
        assert!(l.contains(PanelId::EffectsRack));
        l.remove(PanelId::EffectsRack);
        assert!(!l.contains(PanelId::EffectsRack));
        l.show(PanelId::EffectsRack);
        assert!(l.contains(PanelId::EffectsRack));
        l.toggle(PanelId::Metadata);
        assert!(l.contains(PanelId::Metadata));
        let mut w = Workspaces::default();
        w.switch("Loudness");
        assert!(w.layout.contains(PanelId::MatchLoudness));
        w.layout.remove(PanelId::Files);
        w.reset();
        assert!(w.layout.contains(PanelId::Files));
        w.save_as("Mine");
        assert_eq!(w.current, "Mine");
        assert!(w.names().contains(&"Mine".to_string()));
        w.delete("Mine");
        assert_eq!(w.current, "Default");
    }

    #[test]
    fn panels_can_live_in_their_own_windows() {
        let dual = preset("Maximum Editing (Dual Monitor)");
        assert_eq!(
            dual.windows.len(),
            1,
            "the dual-monitor workspace opens a second window"
        );
        assert!(dual.contains(PanelId::Files));
        let mut l = preset("Default");
        let id = l.undock(PanelId::Markers);
        assert!(l.contains(PanelId::Markers));
        assert!(
            l.windows
                .iter()
                .any(|w| w.id == id && w.groups[0].tabs == vec![PanelId::Markers])
        );
        let empty = l.new_window(vec![]);
        l.add_to_window(empty, PanelId::History);
        assert!(
            l.windows
                .iter()
                .find(|w| w.id == empty)
                .map(|w| w.groups[0].tabs.contains(&PanelId::History))
                .unwrap_or(false)
        );
        l.dock_window(id);
        assert!(l.contains(PanelId::Markers) && !l.windows.iter().any(|w| w.id == id));
        // the saved layout still reads without the new field
        let old = serde_json::to_string(&preset("Default"))
            .unwrap()
            .replace(",\"windows\":[]", "");
        assert!(serde_json::from_str::<Layout>(&old).is_ok());
    }
}
