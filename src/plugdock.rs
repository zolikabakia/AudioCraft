//! The Plug-ins panel (Window > Plug-ins). A hosted VST3 or CLAP effect opens its own editor
//! window here, docked in the workspace (the right-hand column by default), instead of the
//! generic value sliders. Several editors can be open at once: each has a tab, the front one is
//! shown and the others wait out of sight. Closing the panel (its ≡ menu or the Window menu)
//! only hides it; the editors come back when it is shown again. Each tab's × closes that editor.
//!
//! On Windows the plug-in's host window is turned into a child window of AudioCraft's main
//! window, inside a clipping container placed over the panel every frame. Elsewhere (and when
//! Preferences > Effects turns docking off) the editors keep their own floating windows and the
//! panel lists them.

use crate::app::App;
use crate::dsp::{Effect, EffectSlot};
use crate::plugins::{self, OpenEditor};
use crate::rack::RackTarget;
use crate::theme;
use crate::workspace::PanelId;
use egui::{Color32, FontId, Rect, RichText, Sense, Stroke, pos2, vec2};

/// One open plug-in editor.
pub struct PlugWin {
    pub target: RackTarget,
    pub slot: usize,
    pub name: String,
    pub ed: OpenEditor,
    /// The docked window; `None` when the editor floats in its own window.
    pub embed: Option<native::Embed>,
    /// The editor's host window (Windows; 0 when unknown, e.g. a CLAP plug-in's own window):
    /// what Pop Out and Dock in Panel move.
    pub host: isize,
    /// Which running instance a live editor drives (its address; 0 = a separate copy). When
    /// the engine replaces that instance (playback started at another sample rate, the rack was
    /// rebuilt) the editor is reconnected to the new one, so edits are heard while playing.
    pub live: usize,
    /// The dark cover shown over the editor while playing (0 = none yet).
    pub shield: isize,
}

/// The open editors and where the front one goes this frame.
#[derive(Default)]
pub struct PlugDock {
    pub wins: Vec<PlugWin>,
    pub active: usize,
    /// (visible part, whole editor) of the front editor, in points, when the panel drew it
    /// this frame. Cleared after every frame.
    pub area: Option<(Rect, Rect)>,
    /// The main window's native handle (0 until known).
    pub main: isize,
    /// Width (physical pixels) the Plug-ins column should take once, to fit a new editor.
    pub fit_px: Option<i32>,
}

impl PlugDock {
    pub fn find(&self, t: RackTarget, slot: usize) -> Option<usize> {
        self.wins
            .iter()
            .position(|w| w.target == t && w.slot == slot)
    }
    pub fn is_open(&self, t: RackTarget, slot: usize) -> bool {
        self.find(t, slot).is_some()
    }
    /// The editor in front (index kept in range).
    pub fn front(&self) -> Option<usize> {
        (!self.wins.is_empty()).then(|| self.active.min(self.wins.len() - 1))
    }
}

/// Short label for the rack a plug-in sits in.
fn target_label(app: &App, t: RackTarget) -> String {
    app.rack_get(t)
        .map(|(l, _, _)| l)
        .unwrap_or_else(|| "(gone)".into())
}

/// Whether an egui window, menu or popup lies over `area` (the native editor would hide it,
/// so the editor steps aside while it is there).
fn covered(ctx: &egui::Context, area: Rect) -> bool {
    ctx.memory(|m| {
        m.areas()
            .visible_layer_ids()
            .into_iter()
            .filter(|l| matches!(l.order, egui::Order::Middle | egui::Order::Foreground))
            .any(|l| m.area_rect(l.id).is_some_and(|r| r.intersects(area)))
    })
}

/// A rect in points as whole physical pixels (x0, y0, x1, y1).
pub fn to_px(r: Rect, ppp: f32) -> [i32; 4] {
    let f = |v: f32| (v * ppp).round() as i32;
    [f(r.left()), f(r.top()), f(r.right()), f(r.bottom())]
}

impl App {
    /// Remember the main window's native handle (once; needed to dock editors).
    /// (Windows only: elsewhere editors float, so no handle is needed and no window-handle
    /// crate is compiled.)
    pub fn plug_note_window(&mut self, frame: &eframe::Frame) {
        if self.plug_dock.main != 0 {
            return;
        }
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(h) = frame.window_handle() {
                if let RawWindowHandle::Win32(w) = h.as_raw() {
                    self.plug_dock.main = w.hwnd.get();
                }
            }
        }
        #[cfg(not(windows))]
        let _ = frame;
    }

    /// Where the pointer really is when files are dropped from outside the app.
    /// On Windows, the window gets no mouse-move events during a drag from Explorer, so
    /// egui's pointer still holds the spot where the mouse last moved inside the window.
    /// Asking the OS for the cursor gives the actual drop spot (Files panel vs. tracks).
    pub fn drop_pointer_pos(&self, ctx: &egui::Context) -> Option<egui::Pos2> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::{HWND, POINT};
            use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
            use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
            if self.plug_dock.main != 0 {
                let mut p = POINT { x: 0, y: 0 };
                // SAFETY: plain Win32 calls on a valid out-pointer and our own window handle.
                let ok = unsafe {
                    GetCursorPos(&mut p) != 0
                        && ScreenToClient(self.plug_dock.main as HWND, &mut p) != 0
                };
                if ok {
                    let ppp = ctx.pixels_per_point().max(0.1);
                    return Some(egui::pos2(p.x as f32 / ppp, p.y as f32 / ppp));
                }
            }
        }
        ctx.input(|i| i.pointer.latest_pos().or(i.pointer.hover_pos()))
    }

    /// Whether a rack slot holds a hosted (VST3/CLAP) plug-in.
    pub fn slot_is_hosted(&self, t: RackTarget, slot: usize) -> bool {
        self.rack_get(t)
            .and_then(|(_, s, _)| {
                s.get(slot).map(
                    |s| matches!(&s.effect, Effect::Plugin { id, .. } if plugins::is_hosted(id)),
                )
            })
            .unwrap_or(false)
    }

    /// Run `f` on the processor that is playing rack slot `slot` of `t` (bypassed slots and racks
    /// not playing now have none). With `sync`, the playing chain is first brought up to date
    /// with the rack (a just-inserted effect is then already running).
    pub fn with_live_proc<R>(
        &mut self,
        t: RackTarget,
        slot: usize,
        sync: bool,
        f: impl FnOnce(&mut Box<dyn crate::dsp::Processor>) -> R,
    ) -> Option<R> {
        let (which, nth, s) = self.live_place(t, slot, sync)?;
        let mut e = self.engine.lock();
        let chain = e.live_chain(which)?;
        chain.proc_for(nth, &s).map(f)
    }

    /// Record `new` as the playing chain's copy of rack slot `slot` of `t` without sending it to
    /// the running plug-in (it made the change itself in its own window). Without this, every
    /// knob turned in a plug-in's window during playback was sent back to it a moment late, and
    /// plug-ins that show their running values snapped the knob back: the controls looked stuck
    /// while playing.
    pub fn adopt_live_slot(&mut self, t: RackTarget, slot: usize, new: &EffectSlot) -> bool {
        let Some((which, nth, _)) = self.live_place(t, slot, false) else {
            return false;
        };
        let mut e = self.engine.lock();
        e.live_chain(which)
            .map(|c| c.adopt_slot(nth, new))
            .unwrap_or(false)
    }

    /// Which running chain plays rack slot `slot` of `t`, its position among the enabled
    /// effects, and the slot (None when the effect is not running now).
    #[allow(clippy::type_complexity)]
    fn live_place(
        &mut self,
        t: RackTarget,
        slot: usize,
        sync: bool,
    ) -> Option<(Option<Result<usize, u64>>, usize, EffectSlot)> {
        let (_, slots, _) = self.rack_get(t)?;
        let s = slots.get(slot)?.clone();
        if !s.enabled {
            return None;
        }
        let nth = slots[..slot].iter().filter(|x| x.enabled).count();
        let which = match t {
            RackTarget::File(id) => {
                if self.mode != crate::app::Mode::Waveform
                    || self.current != Some(id)
                    || self.preview_audio.is_some()
                {
                    return None;
                }
                if sync {
                    let f = self.file(id)?;
                    let (r, a, b, c, d) = (
                        f.rack.clone(),
                        f.rack_in_db,
                        f.rack_out_db,
                        f.rack_mix,
                        f.rack_power,
                    );
                    self.engine.lock().set_rack(&r, a, b, c, d);
                }
                None
            }
            RackTarget::Track(i) => {
                if self.mode != crate::app::Mode::Multitrack {
                    return None;
                }
                if sync {
                    self.refresh_mix(true);
                }
                Some(Ok(i))
            }
            RackTarget::Clip(id) => {
                if self.mode != crate::app::Mode::Multitrack {
                    return None;
                }
                if sync {
                    self.refresh_mix(true);
                }
                Some(Err(id))
            }
        };
        Some((which, nth, s))
    }

    /// The address of the processor now playing rack slot `slot` of `t` (0 = none running).
    pub fn live_addr(&mut self, t: RackTarget, slot: usize) -> usize {
        self.with_live_proc(t, slot, false, |p| {
            let r: &dyn crate::dsp::Processor = &**p;
            r as *const dyn crate::dsp::Processor as *const u8 as usize
        })
        .unwrap_or(0)
    }

    /// Editors whose playing instance was replaced, or that drive a separate copy while the
    /// effect now runs: reopen them on the instance that plays (their settings carry over).
    fn plug_reattach(&mut self) {
        let mut redo: Vec<(usize, RackTarget, usize)> = vec![];
        for i in 0..self.plug_dock.wins.len() {
            let (t, slot, was) = {
                let w = &self.plug_dock.wins[i];
                (w.target, w.slot, w.live)
            };
            let now = self.live_addr(t, slot);
            if now != 0 && now != was {
                redo.push((i, t, slot));
            }
        }
        for (i, t, slot) in redo.into_iter().rev() {
            let front = self.plug_dock.active == i;
            self.plug_close(i);
            if let Err(e) = self.plug_open(t, slot) {
                self.status = e;
            } else if !front && self.plug_dock.wins.len() > 1 {
                // keep the editor that was in front there
                self.plug_dock.active = self.plug_dock.active.min(self.plug_dock.wins.len() - 1);
            }
        }
    }

    /// Open a hosted plug-in's own editor: docked in the Plug-ins panel (shown if it was
    /// closed), or floating when docking is off or not possible here.
    pub fn plug_open(&mut self, t: RackTarget, slot: usize) -> Result<(), String> {
        if let Some(i) = self.plug_dock.find(t, slot) {
            self.plug_dock.active = i;
            if self.plug_dock.wins[i].embed.is_some() {
                self.show_panel(PanelId::PlugIns);
            }
            return Ok(());
        }
        let Some((_, slots, _)) = self.rack_get(t) else {
            return Err("That effects rack is gone.".into());
        };
        let Some(EffectSlot {
            effect:
                Effect::Plugin {
                    id,
                    name,
                    params,
                    state,
                },
            ..
        }) = slots.get(slot)
        else {
            return Err("That slot does not hold a VST3 or CLAP plug-in.".into());
        };
        let dock = self.prefs.dock_plugin_editors && native::SUPPORTED && self.plug_dock.main != 0;
        let before = native::editor_windows();
        // the playing instance's own window when the effect is running; otherwise a separate one
        let live = self
            .with_live_proc(t, slot, true, |p| p.live_editor())
            .flatten();
        let ed = match live {
            Some((e, r)) => OpenEditor::open_live(e, r)?,
            None => OpenEditor::open(id, params, state.as_deref())?,
        };
        native::give_focus_back(self.plug_dock.main);
        let host = native::editor_windows()
            .into_iter()
            .find(|h| !before.contains(h))
            .unwrap_or(0);
        let embed = if dock && host != 0 {
            native::Embed::adopt(self.plug_dock.main, host)
        } else {
            None
        };
        let docked = embed.is_some();
        if let Some(e) = &embed {
            self.plug_dock.fit_px = Some(e.size().0);
        }
        let live = if ed.is_live() {
            self.live_addr(t, slot)
        } else {
            0
        };
        self.plug_dock.wins.push(PlugWin {
            target: t,
            slot,
            name: name.clone(),
            ed,
            embed,
            host,
            live,
            shield: 0,
        });
        self.plug_dock.active = self.plug_dock.wins.len() - 1;
        if docked {
            self.show_panel(PanelId::PlugIns);
            self.status = format!("{name} opened in the Plug-ins panel");
        } else if dock {
            self.status = format!("{name} uses its own floating window");
        }
        Ok(())
    }

    /// Close one editor and keep the plug-in's full state in its rack slot.
    pub fn plug_close(&mut self, i: usize) {
        if i >= self.plug_dock.wins.len() {
            return;
        }
        let w = self.plug_dock.wins.remove(i);
        let PlugWin {
            target,
            slot,
            ed,
            embed,
            shield,
            ..
        } = w;
        native::shield_drop(shield);
        // the editor detaches from its window first; then the dock's container can go
        let live = ed.is_live();
        let st = ed.close();
        drop(embed);
        if let (Some(st), true) = (&st, live) {
            // read from the playing instance itself: storing it in the slot must not reload it
            let _ = self.with_live_proc(target, slot, false, |p| p.adopt_state(st));
        }
        if let (Some(st), Some((_, mut slots, _))) = (st, self.rack_get(target)) {
            if let Some(EffectSlot {
                effect: Effect::Plugin { state, .. },
                ..
            }) = slots.get_mut(slot)
            {
                *state = Some(st);
            }
            self.rack_set(target, slots, None);
        }
        if self.plug_dock.active >= i && self.plug_dock.active > 0 {
            self.plug_dock.active -= 1;
        }
    }

    /// Pop a docked editor out into its own floating window (it stays open and keeps its tab).
    pub fn plug_pop_out(&mut self, i: usize) -> Result<(), String> {
        let main = self.plug_dock.main;
        let w = self
            .plug_dock
            .wins
            .get_mut(i)
            .ok_or("No plug-in window is open")?;
        let e = w
            .embed
            .take()
            .ok_or_else(|| format!("{} already has its own window", w.name))?;
        w.host = e.release(main);
        self.status = format!("{} popped out (Dock in Panel puts it back)", w.name);
        Ok(())
    }

    /// Put a floating editor back into the Plug-ins panel.
    pub fn plug_dock_in(&mut self, i: usize) -> Result<(), String> {
        let main = self.plug_dock.main;
        let w = self
            .plug_dock
            .wins
            .get_mut(i)
            .ok_or("No plug-in window is open")?;
        if w.embed.is_some() {
            return Ok(());
        }
        if !native::SUPPORTED || main == 0 || w.host == 0 {
            return Err(format!("{} can only float in its own window here", w.name));
        }
        let e = native::Embed::adopt(main, w.host)
            .ok_or_else(|| format!("{} could not be docked", w.name))?;
        self.plug_dock.fit_px = Some(e.size().0);
        w.embed = Some(e);
        native::give_focus_back(main);
        let name = w.name.clone();
        self.plug_dock.active = i;
        self.show_panel(PanelId::PlugIns);
        self.status = format!("{name} docked in the Plug-ins panel");
        Ok(())
    }

    /// Pop the front editor out, or dock it back when it floats (command `plugins.pop_out`).
    pub fn plug_toggle_pop(&mut self) -> Result<(), String> {
        let i = self.plug_dock.front().ok_or("No plug-in window is open")?;
        if self.plug_dock.wins[i].embed.is_some() {
            self.plug_pop_out(i)
        } else {
            self.plug_dock_in(i)
        }
    }

    /// Whether the editor can be moved between the panel and its own window.
    pub fn plug_can_pop(&self, i: usize) -> bool {
        self.plug_dock.wins.get(i).is_some_and(|w| {
            w.embed.is_some() || (native::SUPPORTED && self.plug_dock.main != 0 && w.host != 0)
        })
    }

    pub fn plug_close_all(&mut self) {
        while !self.plug_dock.wins.is_empty() {
            self.plug_close(self.plug_dock.wins.len() - 1);
        }
    }

    /// Once per frame, after everything is drawn: hand the editors' edits to their slots,
    /// drop editors whose window was closed, and place the front docked editor over the panel
    /// (the others, and the front one while a menu or dialog covers it, wait out of sight).
    pub fn plug_service(&mut self, ctx: &egui::Context) {
        if !self.plug_dock.wins.is_empty() {
            native::watch_keys();
            // Space pressed while a plug-in editor had the keyboard: play / stop as usual
            if native::take_space() {
                self.run("play", ctx);
            }
            // a click back in AudioCraft takes the keyboard back from the editor
            if ctx.input(|i| i.pointer.any_pressed()) && native::focus_in_plugin() {
                native::give_focus_back(self.plug_dock.main);
            }
            // keep looking (the editor's own window gets the events, not egui)
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        // while AudioCraft is minimised the system hides its overlays: don't take that for the
        // person closing the editors
        if native::minimized(self.plug_dock.main) {
            return;
        }
        if !self.plug_dock.wins.is_empty() {
            self.plug_reattach();
        }
        let mut closed: Vec<usize> = vec![];
        for i in 0..self.plug_dock.wins.len() {
            native::pump_editor(self.plug_dock.wins[i].host);
            let (t, slot, edits, open, live) = {
                let w = &mut self.plug_dock.wins[i];
                (
                    w.target,
                    w.slot,
                    w.ed.poll(),
                    w.ed.is_open(),
                    w.ed.is_live(),
                )
            };
            if !edits.is_empty() {
                if let Some((_, mut slots, _)) = self.rack_get(t) {
                    if let Some(EffectSlot {
                        effect: Effect::Plugin { params, .. },
                        ..
                    }) = slots.get_mut(slot)
                    {
                        for (k, v) in edits {
                            match params.iter_mut().find(|(pk, _)| *pk == k) {
                                Some(e) => e.1 = v,
                                None => params.push((k, v)),
                            }
                        }
                    }
                    // the window drives the playing instance: it already has these values, so
                    // the session records them without sending them back to it
                    if live {
                        if let Some(new) = slots.get(slot).cloned() {
                            self.adopt_live_slot(t, slot, &new);
                        }
                    }
                    self.rack_set(t, slots, None);
                }
            }
            // an editor whose rack or slot went away closes too
            if !open || !self.slot_is_hosted(t, slot) {
                closed.push(i);
            }
        }
        for i in closed.into_iter().rev() {
            self.plug_close(i);
        }
        let area = self.plug_dock.area.take();
        let front = self.plug_dock.front();
        let ppp = ctx.pixels_per_point();
        let place = area
            .filter(|(vis, _)| vis.width() >= 1.0 && vis.height() >= 1.0 && !covered(ctx, *vis));
        for (i, w) in self.plug_dock.wins.iter_mut().enumerate() {
            let Some(e) = w.embed.as_mut() else { continue };
            match place {
                Some((vis, whole)) if Some(i) == front => {
                    let v = to_px(vis, ppp);
                    let o = to_px(whole, ppp);
                    e.place(v, (o[0] - v[0], o[1] - v[1]));
                }
                _ => e.park(),
            }
        }
        // while playing, plug-in windows are covered: they cannot be edited then
        let busy = self.is_playing() || self.is_recording();
        let main = self.plug_dock.main;
        for w in self.plug_dock.wins.iter_mut() {
            let over = w.embed.as_ref().map(|e| e.container()).unwrap_or(w.host);
            native::shield(&mut w.shield, main, over, busy);
        }
        // while a docked editor shows, keep the placement in step with panel moves
        if place.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// The Plug-ins panel: one tab per open editor, a slot strip, then the editor itself.
    pub fn plugins_panel(&mut self, ui: &mut egui::Ui) {
        if self.plug_dock.wins.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new("No plug-in windows are open.").color(theme::text()));
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Insert a VST3 or CLAP effect in the Effects Rack (or click one that is already there) and its own window opens here.",
                )
                .color(theme::text_dim()),
            );
            if !native::SUPPORTED {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "On this system plug-in windows float; they are listed here while open.",
                    )
                    .small()
                    .color(theme::text_dim()),
                );
            }
            return;
        }
        if self.is_playing() || self.is_recording() {
            ui.label(
                RichText::new(
                    "\u{23f8} Plug-in editing is paused while playing. Stop to change settings.",
                )
                .small()
                .color(Color32::from_rgb(232, 176, 70)),
            );
        }
        let front = self.plug_dock.front().unwrap_or(0);
        // the editors' tabs
        let mut pick: Option<usize> = None;
        let mut close: Option<usize> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
            for (i, w) in self.plug_dock.wins.iter().enumerate() {
                let on = i == front;
                let g = ui.painter().layout_no_wrap(
                    w.name.clone(),
                    FontId::proportional(12.0),
                    theme::text(),
                );
                let (r, resp) =
                    ui.allocate_exact_size(vec2(g.size().x + 34.0, 22.0), Sense::click());
                let p = ui.painter();
                p.rect_filled(
                    r,
                    theme::ROUND as f32,
                    theme::control_fill(on, theme::selected(), &resp, true),
                );
                p.text(
                    pos2(r.left() + 8.0, r.center().y),
                    egui::Align2::LEFT_CENTER,
                    &w.name,
                    FontId::proportional(12.0),
                    if on { Color32::WHITE } else { theme::text() },
                );
                let xr =
                    Rect::from_center_size(pos2(r.right() - 11.0, r.center().y), vec2(14.0, 14.0));
                let xh = ui.rect_contains_pointer(xr);
                if xh {
                    p.rect_filled(xr, 0.0, crate::theme::gray(80));
                }
                let c = if xh {
                    Color32::WHITE
                } else {
                    theme::text_dim()
                };
                p.line_segment(
                    [xr.min + vec2(4.0, 4.0), xr.max - vec2(4.0, 4.0)],
                    Stroke::new(1.3, c),
                );
                p.line_segment(
                    [
                        pos2(xr.right() - 4.0, xr.top() + 4.0),
                        pos2(xr.left() + 4.0, xr.bottom() - 4.0),
                    ],
                    Stroke::new(1.3, c),
                );
                let resp = resp.on_hover_text(format!(
                    "{} - {}, slot {}",
                    w.name,
                    target_label(self, w.target),
                    w.slot + 1
                ));
                if resp.clicked() {
                    if xh {
                        close = Some(i);
                    } else {
                        pick = Some(i);
                    }
                }
                if resp.middle_clicked() {
                    close = Some(i);
                }
            }
        });
        if let Some(i) = pick {
            self.plug_dock.active = i;
            // the column fits the editor brought to the front
            self.plug_dock.fit_px = self.plug_dock.wins[i].embed.as_ref().map(|e| e.size().0);
        }
        if let Some(i) = close {
            self.plug_close(i);
            return;
        }
        let Some(front) = self.plug_dock.front() else {
            return;
        };
        let (t, slot, name, docked) = {
            let w = &self.plug_dock.wins[front];
            (w.target, w.slot, w.name.clone(), w.embed.is_some())
        };
        // the slot strip: power (bypass), where it sits, generic controls
        let enabled = self
            .rack_get(t)
            .and_then(|(_, s, _)| s.get(slot).map(|s| s.enabled))
            .unwrap_or(true);
        let mut pop = false;
        ui.horizontal(|ui| {
            if theme::icon_button(
                ui,
                theme::Icon::Power,
                enabled,
                if enabled {
                    "Bypass this effect"
                } else {
                    "Turn this effect on"
                },
            )
            .clicked()
            {
                if let Some((_, mut s, _)) = self.rack_get(t) {
                    if let Some(x) = s.get_mut(slot) {
                        x.enabled = !x.enabled;
                    }
                    self.rack_set(t, s, None);
                }
            }
            ui.label(
                RichText::new(format!("{}  |  Slot {}", target_label(self, t), slot + 1))
                    .small()
                    .color(theme::text_dim()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Generic Controls")
                    .on_hover_text("Show the plug-in's parameters as sliders in a floating window")
                    .clicked()
                {
                    self.rack_generic = Some((t, slot));
                    self.rack_window = Some((t, slot));
                }
                if self.plug_can_pop(front) {
                    let (label, tip) = if docked {
                        ("Pop Out", "Move this plug-in into its own floating window")
                    } else {
                        (
                            "Dock in Panel",
                            "Put this plug-in's window back into the Plug-ins panel",
                        )
                    };
                    if ui.small_button(label).on_hover_text(tip).clicked() {
                        pop = true;
                    }
                }
            });
        });
        if pop {
            if let Err(e) = self.plug_toggle_pop() {
                self.status = e;
            }
            return;
        }
        ui.add_space(2.0);
        let Some(e) = self.plug_dock.wins[front].embed.as_ref().filter(|_| docked) else {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("{name} shows its own floating window."))
                    .color(theme::text()),
            );
            let hint = if self.plug_can_pop(front) {
                "Dock in Panel brings it back here; close it there or with the × on its tab."
            } else {
                "Close it there, or with the × on its tab."
            };
            ui.label(RichText::new(hint).small().color(theme::text_dim()));
            return;
        };
        let ppp = ui.ctx().pixels_per_point().max(0.1);
        let (pw, ph) = e.size();
        let size = vec2(pw.max(1) as f32 / ppp, ph.max(1) as f32 / ppp);
        let mut area: Option<(Rect, Rect)> = None;
        egui::ScrollArea::both()
            .id_salt("plug_editor_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (r, _) = ui.allocate_exact_size(size, Sense::hover());
                ui.painter().rect_filled(r, 0.0, crate::theme::gray(18));
                let vis = ui.clip_rect().intersect(r);
                area = Some((vis, r));
            });
        self.plug_dock.area = area;
    }
}

#[cfg(windows)]
pub mod native {
    //! Win32: plug-in host windows become child windows of the main window.
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, EnumThreadWindows, GWL_EXSTYLE, GWL_STYLE,
        GetClassNameW, GetClientRect, GetWindowLongPtrW, IsWindow, RegisterClassExW, SW_HIDE,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetParent,
        SetWindowLongPtrW, SetWindowPos, ShowWindow, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN,
        WS_CLIPSIBLINGS, WS_VISIBLE,
    };

    pub const SUPPORTED: bool = true;

    /// The window classes of SoundCraft's VST3 and CLAP editor host windows.
    const HOST_CLASSES: [&str; 2] = ["SoundCraftVst3Editor", "SoundCraftClapEditor"];

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn class_of(h: HWND) -> String {
        let mut buf = [0u16; 128];
        // SAFETY: `buf` is writable for its whole length; any handle value is accepted.
        let n = unsafe { GetClassNameW(h, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..n.clamp(0, 128) as usize])
    }

    unsafe extern "system" fn collect(h: HWND, l: LPARAM) -> i32 {
        // SAFETY: `l` is the `Vec` passed by `editor_windows` for the duration of the call.
        let v = unsafe { &mut *(l as *mut Vec<isize>) };
        v.push(h as isize);
        1
    }

    /// Keep a plug-in editor drawing while AudioCraft repaints every frame. Editors move their
    /// knobs from timer messages, and Windows hands those out only when no repaint is waiting;
    /// during playback a repaint nearly always is, so the plug-in's controls stopped moving on
    /// screen. Once per frame: deliver the editor windows' own timer messages (only theirs,
    /// AudioCraft's window is left to its event loop) and flush their pending repaints.
    pub fn pump_editor(host: isize) {
        use windows_sys::Win32::Graphics::Gdi::{RDW_ALLCHILDREN, RDW_UPDATENOW, RedrawWindow};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, EnumChildWindows, MSG, PM_REMOVE, PeekMessageW, WM_TIMER,
        };
        if host == 0 {
            return;
        }
        let mut wins: Vec<isize> = vec![host];
        // SAFETY: the callback only pushes into `wins`, which outlives the call; every other
        // call takes a window of this thread (a dead one makes them fail harmlessly).
        unsafe {
            if IsWindow(host as HWND) == 0 {
                return;
            }
            EnumChildWindows(
                host as HWND,
                Some(collect),
                &mut wins as *mut Vec<isize> as LPARAM,
            );
            for &w in wins.iter().take(256) {
                let mut msg: MSG = std::mem::zeroed();
                let mut n = 0;
                while n < 8 && PeekMessageW(&mut msg, w as HWND, WM_TIMER, WM_TIMER, PM_REMOVE) != 0
                {
                    DispatchMessageW(&msg);
                    n += 1;
                }
            }
            RedrawWindow(
                host as HWND,
                std::ptr::null(),
                std::ptr::null_mut(),
                RDW_UPDATENOW | RDW_ALLCHILDREN,
            );
        }
    }

    /// Cover the editor window `over` with a dark see-through shield (`show`) or take the
    /// shield away. The shield dims the editor and catches its clicks: plug-ins cannot be edited
    /// while playing. It sits directly above `over` (never above other programs); `slot` keeps
    /// its handle between frames.
    pub fn shield(slot: &mut isize, main: isize, over: isize, show: bool) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GW_HWNDPREV, GetWindow, GetWindowRect, HWND_TOP, LWA_ALPHA, SetLayeredWindowAttributes,
            WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
        };
        // SAFETY: plain Win32 calls on windows of this thread; dead handles fail harmlessly.
        unsafe {
            let hide = |s: isize| {
                if s != 0 && IsWindow(s as HWND) != 0 {
                    ShowWindow(s as HWND, SW_HIDE);
                }
            };
            if !show || over == 0 || IsWindow(over as HWND) == 0 {
                hide(*slot);
                return;
            }
            let mut r = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            GetWindowRect(over as HWND, &mut r);
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            if w < 4 || h < 4 {
                // a parked editor (out of sight): nothing to cover
                hide(*slot);
                return;
            }
            if *slot == 0 || IsWindow(*slot as HWND) == 0 {
                let Some(class) = container_class() else {
                    return;
                };
                let s = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                    class.as_ptr(),
                    std::ptr::null(),
                    WS_POPUP,
                    r.left,
                    r.top,
                    w,
                    h,
                    main as HWND,
                    std::ptr::null_mut(),
                    GetModuleHandleW(std::ptr::null()),
                    std::ptr::null(),
                );
                if s.is_null() {
                    return;
                }
                SetLayeredWindowAttributes(s, 0, 150, LWA_ALPHA);
                *slot = s as isize;
            }
            // right above the editor: the window that is above it now goes above the shield
            let above = GetWindow(over as HWND, GW_HWNDPREV);
            let after = if above.is_null() || above as isize == *slot {
                HWND_TOP
            } else {
                above
            };
            SetWindowPos(
                *slot as HWND,
                after,
                r.left,
                r.top,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    /// Remove a shield for good (its editor closed).
    pub fn shield_drop(slot: isize) {
        // SAFETY: our own window, or a dead handle (fails harmlessly).
        unsafe {
            if slot != 0 && IsWindow(slot as HWND) != 0 {
                DestroyWindow(slot as HWND);
            }
        }
    }

    /// The plug-in editor host windows of this (the UI) thread.
    pub fn editor_windows() -> Vec<isize> {
        let mut v: Vec<isize> = vec![];
        // SAFETY: the callback only pushes into `v`, which outlives the call.
        unsafe {
            EnumThreadWindows(
                GetCurrentThreadId(),
                Some(collect),
                &mut v as *mut Vec<isize> as LPARAM,
            )
        };
        v.into_iter()
            .filter(|h| HOST_CLASSES.contains(&class_of(*h as HWND).as_str()))
            .collect()
    }

    fn container_class() -> Option<&'static [u16]> {
        static C: OnceLock<Option<Vec<u16>>> = OnceLock::new();
        C.get_or_init(|| {
            let name = wide("AudioCraftPlugDock");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(DefWindowProcW),
                cbClsExtra: 0,
                cbWndExtra: 0,
                // SAFETY: null asks for this process's module handle.
                hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                // SAFETY: a stock object, never freed.
                hbrBackground: unsafe { GetStockObject(BLACK_BRUSH) },
                lpszMenuName: std::ptr::null(),
                lpszClassName: name.as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            // SAFETY: a complete class description whose name lives for the process.
            (unsafe { RegisterClassExW(&wc) } != 0).then_some(name)
        })
        .as_deref()
    }

    /// Whether `h` is (inside) a plug-in editor: one of the hosts' windows or the dock's
    /// container, or a window within them.
    fn in_plugin(mut h: HWND) -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GA_PARENT, GetAncestor};
        for _ in 0..32 {
            if h.is_null() {
                return false;
            }
            let c = class_of(h);
            if HOST_CLASSES.contains(&c.as_str()) || c == "AudioCraftPlugDock" {
                return true;
            }
            // SAFETY: any handle value is accepted.
            h = unsafe { GetAncestor(h, GA_PARENT) };
        }
        false
    }

    /// The main window is minimised.
    pub fn minimized(main: isize) -> bool {
        // SAFETY: any handle value is accepted.
        main != 0
            && unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsIconic(main as HWND) } != 0
    }

    /// Keyboard focus is in a plug-in's editor.
    pub fn focus_in_plugin() -> bool {
        // SAFETY: no arguments; the focus window of this thread (or null).
        in_plugin(unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus() })
    }

    /// Give the keyboard back to AudioCraft's window (after an editor opened or docked, or a
    /// click outside the editor), so its shortcuts work again.
    pub fn give_focus_back(main: isize) {
        // SAFETY: `IsWindow` accepts any value; `main` is our own window on this thread.
        unsafe {
            if !main.eq(&0) && IsWindow(main as HWND) != 0 {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(main as HWND);
            }
        }
    }

    static SPACE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    unsafe extern "system" fn key_hook(
        code: i32,
        w: windows_sys::Win32::Foundation::WPARAM,
        l: LPARAM,
    ) -> windows_sys::Win32::Foundation::LRESULT {
        use windows_sys::Win32::UI::WindowsAndMessaging::CallNextHookEx;
        // Space (play/stop) while a plug-in editor has the keyboard: AudioCraft's, not the editor's
        if code >= 0 && w == 0x20 && focus_in_plugin() {
            let up = (l >> 31) & 1 == 1;
            let repeat = (l >> 30) & 1 == 1;
            if !up && !repeat {
                SPACE.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            return 1;
        }
        // SAFETY: passing the event on, as every hook procedure must.
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, w, l) }
    }

    /// Watch this (the UI) thread's keyboard once, so Space keeps playing and stopping while a
    /// plug-in editor has the focus.
    pub fn watch_keys() {
        static HOOKED: OnceLock<()> = OnceLock::new();
        HOOKED.get_or_init(|| {
            use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WH_KEYBOARD};
            // SAFETY: a thread-local hook on this thread with a procedure that lives for the process.
            unsafe {
                SetWindowsHookExW(
                    WH_KEYBOARD,
                    Some(key_hook),
                    std::ptr::null_mut(),
                    GetCurrentThreadId(),
                )
            };
        });
    }

    /// True once after Space was pressed in a plug-in editor.
    pub fn take_space() -> bool {
        SPACE.swap(false, std::sync::atomic::Ordering::Relaxed)
    }

    /// Give a host window its own floating window again (caption and close box, kept above
    /// AudioCraft's window), with its top-left corner at `at` (screen pixels).
    pub fn float_host(main: isize, host: isize, at: (i32, i32)) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            AdjustWindowRectEx, GWLP_HWNDPARENT, SW_SHOW, SetForegroundWindow, WS_CAPTION,
            WS_OVERLAPPED, WS_SYSMENU,
        };
        let (main, host) = (main as HWND, host as HWND);
        // SAFETY: any handle values; calls on a dead window fail harmlessly.
        unsafe {
            if IsWindow(host) == 0 {
                return;
            }
            let mut r = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            GetClientRect(host, &mut r);
            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
            SetParent(host, std::ptr::null_mut());
            SetWindowLongPtrW(host, GWL_STYLE, style as isize);
            SetWindowLongPtrW(host, GWLP_HWNDPARENT, main as isize);
            let mut o = RECT {
                left: 0,
                top: 0,
                right: (r.right - r.left).max(1),
                bottom: (r.bottom - r.top).max(1),
            };
            AdjustWindowRectEx(&mut o, style, 0, 0);
            SetWindowPos(
                host,
                std::ptr::null_mut(),
                at.0,
                at.1,
                o.right - o.left,
                o.bottom - o.top,
                SWP_NOZORDER | SWP_FRAMECHANGED,
            );
            ShowWindow(host, SW_SHOW);
            SetForegroundWindow(host);
        }
    }

    /// Where a popped-out editor goes: over the panel it left (screen pixels).
    fn screen_pos(h: HWND) -> Option<(i32, i32)> {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: any handle value; `r` is writable.
        (unsafe { GetWindowRect(h, &mut r) } != 0 && r.right > r.left).then_some((r.left, r.top))
    }

    /// A plug-in host window docked in the main window, inside a clipping container.
    pub struct Embed {
        container: HWND,
        host: HWND,
        /// The main window (the container is its owned overlay).
        main: HWND,
        last: Option<[i32; 6]>,
    }

    impl Embed {
        /// The window the docked editor shows in.
        pub fn container(&self) -> isize {
            self.container as isize
        }

        /// Turn the host window `host` into a child of `main` (both of this thread).
        pub fn adopt(main: isize, host: isize) -> Option<Embed> {
            let (main, host) = (main as HWND, host as HWND);
            // SAFETY: `IsWindow` accepts any value.
            if unsafe { IsWindow(main) == 0 || IsWindow(host) == 0 } {
                return None;
            }
            let class = container_class()?;
            // SAFETY: live windows of this thread; the main window keeps its own drawing out of
            // its child windows.
            unsafe {
                let st = GetWindowLongPtrW(main, GWL_STYLE);
                SetWindowLongPtrW(main, GWL_STYLE, st | (WS_CLIPCHILDREN as isize));
            }
            // SAFETY: registered class; `main` is a live window of this thread. The container is
            // an owned overlay of its own (not a child): the main window's OpenGL frames can then
            // never paint over the plug-in, and clicking it never takes the keyboard or the active
            // title bar away from AudioCraft.
            let container = unsafe {
                CreateWindowExW(
                    windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE
                        | windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW,
                    class.as_ptr(),
                    std::ptr::null(),
                    windows_sys::Win32::UI::WindowsAndMessaging::WS_POPUP
                        | WS_VISIBLE
                        | WS_CLIPCHILDREN,
                    0,
                    0,
                    0,
                    0,
                    main,
                    std::ptr::null_mut(),
                    GetModuleHandleW(std::ptr::null()),
                    std::ptr::null(),
                )
            };
            if container.is_null() {
                return None;
            }
            let mut r = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: live windows of this thread; `r` is writable. The caption goes, the client
            // area (the plug-in's view) keeps its size.
            unsafe {
                GetClientRect(host, &mut r);
                SetWindowLongPtrW(
                    host,
                    GWL_STYLE,
                    (WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS) as isize,
                );
                SetWindowLongPtrW(host, GWL_EXSTYLE, 0);
                SetParent(host, container);
                SetWindowPos(
                    host,
                    std::ptr::null_mut(),
                    0,
                    0,
                    (r.right - r.left).max(1),
                    (r.bottom - r.top).max(1),
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
                );
            }
            Some(Embed {
                container,
                host,
                main,
                last: None,
            })
        }

        /// (container rect, host rect) in screen pixels and whether the host counts as visible
        /// (diagnostics).
        pub fn describe(&self) -> String {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GetParent, GetWindowRect, IsWindowVisible,
            };
            let mut a = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            let mut b = a;
            // SAFETY: any handle values; the rects are writable.
            unsafe {
                GetWindowRect(self.container, &mut a);
                GetWindowRect(self.host, &mut b);
                format!(
                    "container {:?} host {:?} host-visible {} parent-ok {}",
                    (a.left, a.top, a.right, a.bottom),
                    (b.left, b.top, b.right, b.bottom),
                    IsWindowVisible(self.host) != 0,
                    GetParent(self.host) == self.container
                )
            }
        }

        /// The overlay window's handle (diagnostic captures).
        pub fn window(&self) -> isize {
            self.container as isize
        }

        /// Pop the editor out into its own floating window (it stays open); returns its handle.
        pub fn release(self, main: isize) -> isize {
            let me = std::mem::ManuallyDrop::new(self);
            let at = screen_pos(me.container)
                .or_else(|| screen_pos(main as HWND).map(|(x, y)| (x + 120, y + 120)))
                .unwrap_or((120, 120));
            float_host(main, me.host as isize, (at.0 - 8, (at.1 - 30).max(0)));
            // SAFETY: our own container, now empty.
            unsafe {
                if IsWindow(me.container) != 0 {
                    DestroyWindow(me.container);
                }
            }
            me.host as isize
        }

        /// The editor's size in physical pixels (it may resize itself).
        pub fn size(&self) -> (i32, i32) {
            let mut r = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: any handle value is accepted; `r` is writable.
            if unsafe { IsWindow(self.host) } == 0
                || unsafe { GetClientRect(self.host, &mut r) } == 0
            {
                return (400, 300);
            }
            (r.right - r.left, r.bottom - r.top)
        }

        /// Show the editor: the container covers `vis` (main-window pixels), the editor sits at
        /// `off` inside it (≤ 0 when scrolled).
        pub fn place(&mut self, vis: [i32; 4], off: (i32, i32)) {
            // main-window pixels to screen pixels (the overlay follows the window when it moves)
            let mut o = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
            // SAFETY: any handle value; `o` is writable.
            unsafe { windows_sys::Win32::Graphics::Gdi::ClientToScreen(self.main, &mut o) };
            let vis = if vis[2] > vis[0] && vis[3] > vis[1] {
                [vis[0] + o.x, vis[1] + o.y, vis[2] + o.x, vis[3] + o.y]
            } else {
                [o.x, o.y, o.x, o.y]
            };
            let key = [vis[0], vis[1], vis[2], vis[3], off.0, off.1];
            if self.last == Some(key) {
                return;
            }
            self.last = Some(key);
            // SAFETY: our own windows (calls on destroyed handles fail harmlessly).
            unsafe {
                SetWindowPos(
                    self.container,
                    std::ptr::null_mut(),
                    vis[0],
                    vis[1],
                    (vis[2] - vis[0]).max(0),
                    (vis[3] - vis[1]).max(0),
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                SetWindowPos(
                    self.host,
                    std::ptr::null_mut(),
                    off.0,
                    off.1,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }

        /// Out of sight while its tab is not in front, the panel is hidden or something covers
        /// it. The editor stays open (a zero-size container, not a hidden window, which the
        /// hosts would take for "closed by the user").
        pub fn park(&mut self) {
            self.place([0, 0, 0, 0], (0, 0));
        }
    }

    /// The main window's client area as drawn on screen, child windows (docked editors)
    /// included: (width, height, RGBA). For the screenshot diagnostic.
    pub fn capture(main: isize) -> Option<(u32, u32, Vec<u8>)> {
        use windows_sys::Win32::Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
            DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject,
        };
        use windows_sys::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
        let main = main as HWND;
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: any handle value is accepted; `r` is writable.
        if unsafe { IsWindow(main) } == 0 || unsafe { GetClientRect(main, &mut r) } == 0 {
            return None;
        }
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        if w <= 0 || h <= 0 {
            return None;
        }
        let mut px = vec![0u8; (w * h * 4) as usize];
        // SAFETY: GDI objects created here are released before returning; `px` holds w*h
        // 32-bit pixels, the size GetDIBits is told to write.
        let ok = unsafe {
            let wdc = GetDC(main);
            let mdc = CreateCompatibleDC(wdc);
            let bmp = CreateCompatibleBitmap(wdc, w, h);
            let old = SelectObject(mdc, bmp);
            // PW_CLIENTONLY | PW_RENDERFULLCONTENT
            let printed = PrintWindow(main, mdc, 1 | 2 as PRINT_WINDOW_FLAGS) != 0;
            SelectObject(mdc, old);
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let lines = GetDIBits(
                mdc,
                bmp,
                0,
                h as u32,
                px.as_mut_ptr().cast(),
                &mut bi,
                DIB_RGB_COLORS,
            );
            DeleteObject(bmp);
            DeleteDC(mdc);
            ReleaseDC(main, wdc);
            printed && lines == h
        };
        if !ok {
            return None;
        }
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
            p[3] = 255;
        }
        Some((w as u32, h as u32, px))
    }

    impl Drop for Embed {
        fn drop(&mut self) {
            // SAFETY: our own windows; the editor was closed before (its host window is gone).
            // A host window that is still there floats again, hidden.
            unsafe {
                if IsWindow(self.host) != 0 {
                    ShowWindow(self.host, SW_HIDE);
                    SetParent(self.host, std::ptr::null_mut());
                }
                if IsWindow(self.container) != 0 {
                    DestroyWindow(self.container);
                }
            }
        }
    }
}

#[cfg(not(windows))]
pub mod native {
    //! Editors float in their own windows on this system.
    pub const SUPPORTED: bool = false;

    pub fn editor_windows() -> Vec<isize> {
        vec![]
    }

    pub fn pump_editor(_host: isize) {}

    pub fn shield(_slot: &mut isize, _main: isize, _over: isize, _show: bool) {}

    pub fn shield_drop(_slot: isize) {}

    pub fn capture(_main: isize) -> Option<(u32, u32, Vec<u8>)> {
        None
    }
    pub fn focus_in_plugin() -> bool {
        false
    }
    pub fn minimized(_main: isize) -> bool {
        false
    }
    pub fn give_focus_back(_main: isize) {}
    pub fn watch_keys() {}
    pub fn take_space() -> bool {
        false
    }

    pub struct Embed;

    impl Embed {
        pub fn adopt(_main: isize, _host: isize) -> Option<Embed> {
            None
        }
        pub fn container(&self) -> isize {
            0
        }
        pub fn size(&self) -> (i32, i32) {
            (400, 300)
        }
        pub fn describe(&self) -> String {
            String::new()
        }
        pub fn window(&self) -> isize {
            0
        }
        pub fn place(&mut self, _vis: [i32; 4], _off: (i32, i32)) {}
        pub fn park(&mut self) {}
        pub fn release(self, _main: isize) -> isize {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_become_whole_pixels() {
        let r = Rect::from_min_max(pos2(10.2, 20.6), pos2(110.4, 220.5));
        assert_eq!(to_px(r, 1.0), [10, 21, 110, 221]);
        assert_eq!(to_px(r, 1.5), [15, 31, 166, 331]);
    }

    #[test]
    fn plugins_panel_opens_and_closes_from_the_window_menu_and_its_own_menu() {
        let mut l = crate::workspace::preset("Default");
        assert!(!l.contains(PanelId::PlugIns));
        l.toggle(PanelId::PlugIns);
        assert!(l.contains(PanelId::PlugIns));
        // it docks in a wide column of its own at the right edge
        assert!(l.right2.iter().any(|g| g.tabs == vec![PanelId::PlugIns]));
        assert!(l.right2_w >= 440.0);
        // next to a meter column it takes the next column in
        let mut m = crate::workspace::preset("Simple Editing");
        m.show(PanelId::PlugIns);
        assert!(m.right.iter().any(|g| g.tabs == vec![PanelId::PlugIns]));
        assert!(m.right2.iter().all(|g| !g.tabs.contains(&PanelId::PlugIns)));
        // the ≡ menu's Close Panel takes it out by its title
        let p = PanelId::from_title(PanelId::PlugIns.title()).unwrap();
        l.remove(p);
        assert!(!l.contains(PanelId::PlugIns));
        l.toggle(PanelId::PlugIns);
        assert!(l.contains(PanelId::PlugIns));
    }

    #[test]
    fn an_empty_dock_has_no_front_editor_and_finds_nothing() {
        let d = PlugDock {
            active: 5,
            ..Default::default()
        };
        assert!(d.front().is_none());
        assert!(!d.is_open(RackTarget::File(1), 0));
        // tests never load plug-ins, so an editor can never be opened here
        assert!(OpenEditor::open("vst3:00000000000000000000000000000000", &[], None).is_err());
        assert!(native::editor_windows().is_empty() || native::SUPPORTED);
    }
}
