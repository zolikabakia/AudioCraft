//! The Effects menu, laid out like the original: rack items, quick processes, then the effect
//! categories, then plug-in formats.

use crate::app::{App, Dialog, Mode};
use crate::dsp::{self, Effect, FadeCurve};
use crate::theme;
use egui::RichText;

/// An effect by its menu name (the defaults from `Effect::all_defaults`).
pub fn effect_named(name: &str) -> Option<Effect> {
    match name {
        "Graphic Equalizer (10 Bands)" => {
            return Some(Effect::GraphicEq {
                gains: vec![0.0; 10],
                master_db: 0.0,
            });
        }
        "Graphic Equalizer (20 Bands)" => {
            return Some(Effect::GraphicEq {
                gains: vec![0.0; 20],
                master_db: 0.0,
            });
        }
        "Graphic Equalizer (30 Bands)" => {
            return Some(Effect::GraphicEq {
                gains: vec![0.0; 30],
                master_db: 0.0,
            });
        }
        "Pitch Shifter" => {
            return crate::plugins::builtin_list()
                .into_iter()
                .find(|e| e.id == "pitch_shifter")
                .map(|e| crate::plugins::effect_for(&e));
        }
        _ => {}
    }
    Effect::all_defaults()
        .into_iter()
        .find(|e| e.name() == name)
}

/// What a menu entry does.
enum Item {
    Fx(&'static str),
    Proc(&'static str),
}

const CATEGORIES: &[(&str, &[Item])] = &[
    (
        "Amplitude and Compression",
        &[
            Item::Fx("Amplify"),
            Item::Fx("Channel Mixer"),
            Item::Fx("DeEsser"),
            Item::Fx("Dynamics"),
            Item::Fx("Dynamics Processing"),
            Item::Proc("Fade Envelope (process)..."),
            Item::Proc("Gain Envelope (process)..."),
            Item::Fx("Hard Limiter"),
            Item::Fx("Multiband Compressor"),
            Item::Proc("Normalize (process)..."),
            Item::Fx("Single-band Compressor"),
            Item::Fx("Speech Volume Leveler"),
            Item::Fx("Tube-modeled Compressor"),
            Item::Fx("Noise Gate"),
        ],
    ),
    (
        "Delay and Echo",
        &[
            Item::Fx("Analog Delay"),
            Item::Fx("Delay"),
            Item::Fx("Echo"),
        ],
    ),
    (
        "Diagnostics",
        &[
            Item::Proc("DC Offset (process)"),
            Item::Proc("Click/Pop Eliminator (process)..."),
            Item::Proc("Delete Silence..."),
            Item::Proc("Mark Audio..."),
            Item::Proc("Amplitude Statistics"),
            Item::Proc("Frequency Analysis"),
        ],
    ),
    (
        "Filter and EQ",
        &[
            Item::Fx("FFT Filter"),
            Item::Fx("Graphic Equalizer (10 Bands)"),
            Item::Fx("Graphic Equalizer (20 Bands)"),
            Item::Fx("Graphic Equalizer (30 Bands)"),
            Item::Fx("Notch Filter"),
            Item::Fx("Parametric Equalizer"),
            Item::Fx("Scientific Filter"),
            Item::Fx("Simple Filter"),
        ],
    ),
    (
        "Modulation",
        &[
            Item::Fx("Chorus"),
            Item::Fx("Chorus/Flanger"),
            Item::Fx("Flanger"),
            Item::Fx("Phaser"),
        ],
    ),
    (
        "Noise Reduction / Restoration",
        &[
            Item::Fx("Adaptive Noise Reduction"),
            Item::Fx("Automatic Click Remover"),
            Item::Proc("Automatic Phase Correction (process)"),
            Item::Proc("Capture Noise Print"),
            Item::Fx("DeHummer"),
            Item::Fx("DeNoise"),
            Item::Fx("DeReverb"),
            Item::Fx("Hiss Reduction"),
            Item::Proc("Noise Reduction (process)..."),
            Item::Proc("Sound Remover (process)..."),
        ],
    ),
    (
        "Reverb",
        &[
            Item::Fx("Convolution Reverb"),
            Item::Fx("Full Reverb"),
            Item::Fx("Reverb"),
            Item::Fx("Studio Reverb"),
            Item::Fx("Surround Reverb"),
        ],
    ),
    (
        "Special",
        &[
            Item::Fx("Distortion"),
            Item::Proc("Doppler Shifter (process)..."),
            Item::Fx("Guitar Suite"),
            Item::Fx("Loudness Meter"),
            Item::Fx("Loudness Radar Meter"),
            Item::Fx("Mastering"),
            Item::Fx("Vocal Enhancer"),
        ],
    ),
    (
        "Stereo Imagery",
        &[
            Item::Fx("Center Channel Extractor"),
            Item::Fx("Graphic Phase Shifter"),
            Item::Fx("Stereo Expander"),
        ],
    ),
    (
        "Time and Pitch",
        &[
            Item::Fx("Automatic Pitch Correction"),
            Item::Proc("Manual Pitch Correction (process)..."),
            Item::Proc("Pitch Bender (process)..."),
            Item::Fx("Pitch Shifter"),
            Item::Proc("Stretch and Pitch (process)..."),
        ],
    ),
];

impl App {
    fn wave_open(&self) -> bool {
        self.mode == Mode::Waveform && self.cur().is_some()
    }

    /// Open a realtime effect: a preview/apply window in Waveform, a rack slot in Multitrack.
    fn open_effect(&mut self, e: Effect) {
        if self.wave_open() {
            self.dialog = Some(Dialog::Effect {
                effect: e,
                preview: false,
            });
        } else if self.mode == Mode::Multitrack {
            if let Some(t) = self.rack_target() {
                self.show_panel(crate::workspace::PanelId::EffectsRack);
                self.rack_insert(t, None, e);
            } else {
                self.message("Select a clip or track", "Select a track (or a clip, with Clip Effects showing) to add the effect to its rack.");
            }
        }
    }

    fn run_proc(&mut self, name: &str) {
        match name {
            "Fade Envelope (process)..." => {
                self.dialog = Some(Dialog::Envelope {
                    gain: false,
                    points: vec![(0.0, 1.0), (1.0, 1.0)],
                })
            }
            "Gain Envelope (process)..." => {
                self.dialog = Some(Dialog::Envelope {
                    gain: true,
                    points: vec![(0.0, 0.0), (1.0, 0.0)],
                })
            }
            "Normalize (process)..." => self.dialog = Some(Dialog::Normalize { db: -0.1 }),
            "DC Offset (process)" => self.process("DC Offset", dsp::remove_dc),
            "Click/Pop Eliminator (process)..." => {
                self.dialog = Some(Dialog::ClickPop { sensitivity: 0.5 })
            }
            "Delete Silence..." => {
                self.dialog = Some(Dialog::DeleteSilence {
                    thr_db: -50.0,
                    min_s: 0.5,
                    keep_s: 0.2,
                })
            }
            "Mark Audio..." => {
                self.dialog = Some(Dialog::MarkAudio {
                    thr_db: -50.0,
                    min_s: 0.5,
                })
            }
            "Amplitude Statistics" => self.panels.stats = true,
            "Frequency Analysis" => self.panels.freq = true,
            "Automatic Phase Correction (process)" => {
                let mut lag = 0;
                self.process("Automatic Phase Correction", |d, a, b| {
                    lag = crate::procs::phase_correct(d, a, b, 5.0)
                });
                self.status =
                    format!("Automatic Phase Correction: right channel moved {lag} samples");
            }
            "Capture Noise Print" => {
                if let Some(f) = self.cur() {
                    let (a, b) = f.range();
                    self.noise_profile = Some(dsp::capture_noise(&f.data, a, b));
                    self.status = "Noise print captured".into();
                }
            }
            "Noise Reduction (process)..." => {
                self.dialog = Some(Dialog::NoiseReduction {
                    amount: 0.8,
                    reduce_db: 20.0,
                })
            }
            "Manual Pitch Correction (process)..." => {
                self.dialog = Some(Dialog::Speed {
                    ratio: 1.0,
                    semis: 0.0,
                    mode: 2,
                })
            }
            "Pitch Bender (process)..." => {
                self.dialog = Some(Dialog::PitchBender {
                    start_st: 0.0,
                    end_st: 2.0,
                })
            }
            "Stretch and Pitch (process)..." => {
                self.dialog = Some(Dialog::Speed {
                    ratio: 1.0,
                    semis: 0.0,
                    mode: 1,
                })
            }
            "Sound Remover (process)..." => {
                self.dialog = Some(Dialog::SoundRemover {
                    reduce_db: 30.0,
                    strength: 0.8,
                })
            }
            "Doppler Shifter (process)..." => {
                self.dialog = Some(Dialog::Doppler(Default::default()))
            }
            _ => {}
        }
    }

    /// Auto Heal Selection: rebuild a short selection from the audio around it.
    pub fn auto_heal(&mut self) {
        let ok = self.cur().map(|f| {
            let (a, b) = f.range();
            f.selection.is_some() && b > a && (b - a) <= f.data.rate as usize * 2
        });
        if ok != Some(true) {
            self.message("Auto Heal Selection", "Select a short problem area first (up to about 2 seconds), such as a click or a cough.");
            return;
        }
        let mut healed = false;
        self.process("Auto Heal", |d, a, b| {
            healed = crate::procs::auto_heal(d, a, b)
        });
        if !healed {
            self.undo();
            self.message(
                "Auto Heal Selection",
                "There isn't enough audio around the selection to heal it from.",
            );
        }
    }

    pub fn effects_menu(&mut self, ui: &mut egui::Ui) {
        let wave = self.wave_open();
        let multi = self.mode == Mode::Multitrack;
        let showing = self
            .ws
            .layout
            .contains(crate::workspace::PanelId::EffectsRack);
        if crate::theme::check_item(ui, true, showing, "Show Effects Rack", "Alt+0").clicked() {
            if showing {
                self.ws
                    .layout
                    .remove(crate::workspace::PanelId::EffectsRack);
            } else {
                self.show_panel(crate::workspace::PanelId::EffectsRack);
            }
            ui.close();
        }
        for (clip, label) in [
            (true, "Edit Clip Effects in Effects Rack"),
            (false, "Edit Track Effects in Effects Rack"),
        ] {
            if ui.add_enabled(multi, egui::Button::new(label)).clicked() {
                self.show_panel(crate::workspace::PanelId::EffectsRack);
                self.rack_clip_tab = clip;
                ui.close();
            }
        }
        ui.separator();
        ui.add_enabled_ui(wave, |ui| {
            if ui.button("Invert").clicked() {
                self.process("Invert", dsp::invert);
                ui.close();
            }
            if ui.button("Reverse").clicked() {
                self.process("Reverse", dsp::reverse);
                ui.close();
            }
            if ui.button("Silence").clicked() {
                self.process("Silence", dsp::silence);
                ui.close();
            }
        });
        ui.menu_button("Generate", |ui| {
            let ctx = ui.ctx().clone();
            if ui.button("Speech...").clicked() {
                let _ = self.exec("generate.speech", &ctx);
                ui.close();
            }
            if ui.button("Noise...").clicked() {
                self.dialog = Some(Dialog::Noise {
                    color: dsp::NoiseColor::Pink,
                    db: -12.0,
                    secs: 5.0,
                });
                ui.close();
            }
            if ui.button("DTMF Tones...").clicked() {
                self.dialog = Some(Dialog::Dtmf {
                    digits: "0123456789".into(),
                    tone_ms: 120.0,
                    pause_ms: 80.0,
                    db: -6.0,
                });
                ui.close();
            }
            if ui.button("Tones...").clicked() {
                self.dialog = Some(Dialog::Tone {
                    freq: 440.0,
                    wave: dsp::Wave::Sine,
                    db: -6.0,
                    secs: 5.0,
                });
                ui.close();
            }
            if ui.button("Silence...").clicked() {
                self.dialog = Some(Dialog::InsertSilence { secs: 1.0 });
                ui.close();
            }
        });
        ui.separator();
        ui.add_enabled_ui(wave, |ui| {
            if ui.button("Match Loudness...").clicked() {
                self.dialog = Some(Dialog::Loudness { target: -23.0 });
                ui.close();
            }
            if ui
                .add(egui::Button::new("Auto Heal Selection").shortcut_text("Ctrl+U"))
                .clicked()
            {
                self.auto_heal();
                ui.close();
            }
        });
        ui.separator();
        for (cat, items) in CATEGORIES {
            ui.menu_button(*cat, |ui| {
                for it in items.iter() {
                    match it {
                        Item::Fx(name) => {
                            let label = format!("{name}...");
                            if ui
                                .add_enabled(wave || multi, egui::Button::new(label))
                                .clicked()
                            {
                                if let Some(e) = effect_named(name) {
                                    self.open_effect(e);
                                }
                                ui.close();
                            }
                        }
                        Item::Proc(name) => {
                            let enabled = wave
                                || matches!(*name, "Amplitude Statistics" | "Frequency Analysis");
                            if ui
                                .add_enabled(enabled, egui::Button::new(*name))
                                .on_disabled_hover_text(
                                    "Processes work on a file in the Waveform editor.",
                                )
                                .clicked()
                            {
                                self.run_proc(name);
                                ui.close();
                            }
                        }
                    }
                }
            });
        }
        ui.separator();
        ui.menu_button("VST", |ui| {
            ui.label(
                RichText::new("VST 2 plug-ins aren't supported.\nUse their VST3 or CLAP versions.")
                    .small()
                    .color(theme::text_dim()),
            );
        });
        for (fmt, label) in [("VST3", "VST 3"), ("CLAP", "CLAP")] {
            ui.menu_button(label, |ui| {
                if crate::plugins::hosting_disabled() {
                    ui.label(
                        RichText::new("Plug-in scanning is turned off.").color(theme::text_dim()),
                    );
                    return;
                }
                if self.hosted.is_none() && ui.button("Scan for Plug-ins").clicked() {
                    self.hosted = Some(crate::plugins::scan_hosted(false));
                }
                let list: Vec<_> = self
                    .hosted
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|e| e.format == fmt)
                    .collect();
                if self.hosted.is_some() && list.is_empty() {
                    ui.label(
                        RichText::new(format!("No {label} effects found."))
                            .color(theme::text_dim()),
                    );
                }
                for e in list {
                    if ui
                        .add_enabled(wave || multi, egui::Button::new(format!("{}...", e.name)))
                        .clicked()
                    {
                        self.open_effect(crate::plugins::effect_for(&e));
                        ui.close();
                    }
                }
            });
        }
        if ui.button("Audio Plug-In Manager...").clicked() {
            self.dialog = Some(Dialog::PluginManager);
            ui.close();
        }
        let _ = FadeCurve::Linear;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effect_menu_entry_resolves() {
        for (_, items) in CATEGORIES {
            for it in items.iter() {
                if let Item::Fx(name) = it {
                    assert!(
                        effect_named(name).is_some(),
                        "menu effect '{name}' has no implementation"
                    );
                }
            }
        }
    }
}
