//! Multitrack session XML (`.sesx`) import and export.
//!
//! The layout follows the publicly known structure of `.sesx` session files: a `<sesx>` root, a
//! `<session sampleRate=…>` with `<tracks>` of `<audioTrack>` elements holding `<audioClip>`s
//! (positions in samples: `startPoint`, `endPoint`, `sourceInPoint`, `sourceOutPoint`), fades,
//! and fader/panner components, plus a `<files>` list. Reading is tolerant: unknown elements are
//! ignored, numbers that don't parse fall back to defaults, and nothing panics on hostile input.

use crate::dsp::{Effect, EffectSlot, EqBand, FilterKind, RackOpts};
use crate::session::{Clip, Marker, Session, TRACK_COLORS, Track};
use std::path::{Path, PathBuf};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn attr_f64(n: roxmltree::Node, names: &[&str]) -> Option<f64> {
    names
        .iter()
        .find_map(|k| n.attribute(*k))
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children()
        .find(|c| c.is_element() && c.tag_name().name().eq_ignore_ascii_case(name))
}

fn children<'a, 'i>(
    n: roxmltree::Node<'a, 'i>,
    name: &'static str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    n.children()
        .filter(move |c| c.is_element() && c.tag_name().name().eq_ignore_ascii_case(name))
}

fn gain_to_db(g: f64) -> f32 {
    if g <= 1e-9 {
        -96.0
    } else {
        (20.0 * g.log10()) as f32
    }
}

/// Volume values may be stored as linear gain (0..~4) or as dB; pick the sensible reading.
fn volume_db(v: f64) -> f32 {
    if (0.0..=4.0).contains(&v) {
        gain_to_db(v)
    } else {
        (v as f32).clamp(-96.0, 24.0)
    }
}

/// Volume envelope keyframes are stored as a position on the envelope (0 = silence, 0.65 = the
/// clip's own level, 1 = top), not as gain. Taper: gain = (v / 0.65)^K, so the top is +6 dB.
const ENV_UNITY: f64 = 0.65;
const ENV_TAPER: f64 = 1.609_640_474;

pub fn env_key_db(v: f64) -> f32 {
    if v <= 1e-6 {
        return -96.0;
    }
    ((20.0 * ENV_TAPER * (v / ENV_UNITY).log10()) as f32).clamp(-96.0, 24.0)
}

pub fn env_key_value(db: f32) -> f64 {
    if db <= -95.9 {
        return 0.0;
    }
    (ENV_UNITY * 10f64.powf(db as f64 / (20.0 * ENV_TAPER))).clamp(0.0, 1.0)
}

/// Pan stored as -100..100 or -1..1.
fn pan_norm(v: f64) -> f32 {
    let p = if v.abs() > 1.0 { v / 100.0 } else { v };
    (p as f32).clamp(-1.0, 1.0)
}

#[derive(Default)]
struct Params {
    volume: Option<f64>,
    pan: Option<f64>,
    mute: Option<bool>,
    /// Volume keyframes (sample offset, value).
    keys: Vec<(f64, f64)>,
    /// Pan keyframes (sample offset, value).
    pan_keys: Vec<(f64, f64)>,
}

/// Keyframes (clip seconds, raw value) cut to `0..=len`: points inside stay, and where the
/// envelope runs past an edge a point with the value interpolated at that edge is added, so a
/// trimmed clip keeps the ramp it had in the original instead of piling points on its edges.
pub(crate) fn crop_keys(keys: &[(f64, f64)], len: f64) -> Vec<(f64, f64)> {
    let mut k: Vec<(f64, f64)> = keys
        .iter()
        .copied()
        .filter(|(t, v)| t.is_finite() && v.is_finite())
        .collect();
    k.sort_by(|a, b| a.0.total_cmp(&b.0));
    if k.is_empty() || len <= 0.0 {
        return vec![];
    }
    let at = |t: f64| -> f64 {
        match k.iter().position(|p| p.0 >= t) {
            None => k[k.len() - 1].1,
            Some(0) => k[0].1,
            Some(i) => {
                let (a, b) = (k[i - 1], k[i]);
                if b.0 - a.0 <= 1e-12 {
                    b.1
                } else {
                    a.1 + (b.1 - a.1) * (t - a.0) / (b.0 - a.0)
                }
            }
        }
    };
    let mut out: Vec<(f64, f64)> = vec![];
    if k[0].0 < 0.0 {
        out.push((0.0, at(0.0)));
    }
    out.extend(k.iter().copied().filter(|(t, _)| *t >= 0.0 && *t <= len));
    if k[k.len() - 1].0 > len {
        out.push((len, at(len)));
    }
    out.dedup_by(|b, a| (a.0 - b.0).abs() < 1e-9);
    out
}

/// The fade shape a saved "log" fade opens with (an estimate of how the original draws it).
pub(crate) const LOG_FADE_SHAPE: f32 = 0.5;

/// `shape` / `type` attributes for a fade: bent up is written as the original's "log" fade
/// (with the shape numbers seen in its sessions), bent down as "exp", plain as linear.
fn fade_type_attrs(shape: f32, fade_in: bool) -> String {
    if shape > 0.05 {
        format!("shape=\"{}\" type=\"log\"", if fade_in { 19 } else { 18 })
    } else if shape < -0.05 {
        "shape=\"0\" type=\"exp\"".to_string()
    } else {
        "shape=\"0\" type=\"linear\"".to_string()
    }
}

fn keyframes(par: roxmltree::Node) -> Vec<(f64, f64)> {
    par.descendants()
        .filter(|k| k.is_element() && *k != par)
        .filter_map(|k| {
            Some((
                attr_f64(
                    k,
                    &[
                        "sampleOffset",
                        "offset",
                        "time",
                        "position",
                        "samplePosition",
                    ],
                )?,
                attr_f64(k, &["value", "parameterValue"])?,
            ))
        })
        .collect()
}

/// Read the fader/panner/mute components under a track or clip.
fn read_params(n: roxmltree::Node) -> Params {
    let mut p = Params::default();
    for comp in children(n, "component") {
        let cid = format!(
            "{} {} {}",
            comp.attribute("componentID").unwrap_or(""),
            comp.attribute("id").unwrap_or(""),
            comp.attribute("name").unwrap_or("")
        )
        .to_lowercase();
        for par in children(comp, "parameter") {
            let pname = par.attribute("name").unwrap_or("").to_lowercase();
            let val = attr_f64(par, &["parameterValue", "value"]);
            let is_vol =
                pname.contains("volume") || pname.contains("gain") && !pname.contains("static");
            let is_pan = pname.contains("pan") || cid.contains("pan") && pname.is_empty();
            let is_mute = pname.contains("mute") || cid.contains("mute") && !pname.contains("solo");
            if is_vol && (cid.contains("fader") || cid.contains("volume") || cid.contains("gain")) {
                if let Some(v) = val {
                    p.volume = Some(v);
                }
                p.keys.extend(keyframes(par));
            } else if is_pan {
                if let Some(v) = val {
                    p.pan = Some(v);
                }
                p.pan_keys.extend(keyframes(par));
            } else if is_mute {
                if let Some(v) = val {
                    p.mute = Some(v >= 0.5);
                }
            }
        }
    }
    p.keys.sort_by(|a, b| a.0.total_cmp(&b.0));
    p.pan_keys.sort_by(|a, b| a.0.total_cmp(&b.0));
    p
}

/// Read the `<effectsRack>` of a track or clip: its slots and its rack controls.
fn read_rack(n: roxmltree::Node) -> (Vec<EffectSlot>, RackOpts) {
    let mut opts = RackOpts::default();
    let mut slots = vec![];
    let Some(rack) = child(n, "effectsRack") else {
        return (slots, opts);
    };
    if let Some(v) = rack.attribute("preFader") {
        opts.pre_fader = v != "false";
    }
    if let Some(v) = rack.attribute("powered") {
        opts.power = v != "false";
    }
    for comp in children(rack, "component") {
        let p0 = children(comp, "parameter")
            .next()
            .and_then(|p| attr_f64(p, &["parameterValue"]));
        match comp.attribute("id").unwrap_or("") {
            "rackInput" => opts.in_db = p0.map(gain_to_db).unwrap_or(0.0).clamp(-48.0, 24.0),
            "rackOutput" => opts.out_db = p0.map(gain_to_db).unwrap_or(0.0).clamp(-48.0, 24.0),
            // the stored value is the dry share: 0 means 100 % wet
            "rackMix" => opts.mix = (1.0 - p0.unwrap_or(0.0) as f32).clamp(0.0, 1.0),
            _ => {}
        }
    }
    let mut found: Vec<(usize, EffectSlot)> = vec![];
    for slot in children(rack, "slot") {
        let Some(comp) = child(slot, "component") else {
            continue;
        };
        let idx = attr_f64(slot, &["slotIndex"])
            .map(|v| v as usize)
            .unwrap_or(found.len());
        let mut enabled = slot
            .attribute("powered")
            .map(|v| v != "false")
            .unwrap_or(true);
        // AudioCraft's own exports carry the exact effect as JSON
        let effect = comp
            .attribute("audiocraft")
            .and_then(|j| serde_json::from_str::<Effect>(j).ok())
            .unwrap_or_else(|| {
                let mut params: Vec<(String, f64)> = vec![];
                for par in children(comp, "parameter") {
                    if let (Some(nm), Some(v)) =
                        (par.attribute("name"), attr_f64(par, &["parameterValue"]))
                    {
                        params.push((nm.to_string(), v));
                    }
                }
                if params.iter().any(|(k, v)| k == "Bypass" && *v >= 0.5) {
                    enabled = false;
                }
                map_effect(comp.attribute("name").unwrap_or("Effect"), &params)
            });
        found.push((
            idx,
            EffectSlot {
                effect,
                enabled,
                mix: 1.0,
            },
        ));
    }
    found.sort_by_key(|f| f.0);
    slots.extend(found.into_iter().map(|f| f.1).take(crate::rack::MAX_SLOTS));
    (slots, opts)
}

/// Translate a session effect (normalised 0..1 parameters) into an AudioCraft effect.
/// Effects AudioCraft has are mapped with their settings (estimated scales); the rest are
/// kept by name as an imported placeholder that passes audio through.
fn map_effect(name: &str, params: &[(String, f64)]) -> Effect {
    let get = |k: &str| {
        params
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(k))
            .map(|(_, v)| *v)
    };
    let freq = |v: f64| (v * 24_000.0).clamp(10.0, 22_000.0) as f32;
    let gain = |v: f64| ((v - 0.5) * 30.0) as f32;
    let lname = name.to_lowercase();
    let wet_mix = |dry: Option<f64>, wet: Option<f64>| -> f32 {
        match (dry, wet) {
            (Some(d), Some(w)) if d + w > 1e-6 => (w / (d + w)) as f32,
            _ => 0.3,
        }
    };
    if lname.contains("parametric") || lname == "eq" || lname.contains("equalizer") {
        let mut bands = vec![];
        let on = |k: &str| get(k).map(|v| v >= 0.5).unwrap_or(false);
        bands.push(EqBand {
            kind: FilterKind::HighPass,
            freq: get("High-Pass Cutoff Frequency")
                .map(freq)
                .unwrap_or(30.0)
                .max(20.0),
            gain_db: 0.0,
            q: 0.707,
            on: on("High-Pass Filter Enable"),
        });
        bands.push(EqBand {
            kind: FilterKind::LowShelf,
            freq: get("Low Shelf Frequency").map(freq).unwrap_or(40.0),
            gain_db: get("Low Shelf Gain").map(gain).unwrap_or(0.0),
            q: 0.707,
            on: get("Low Shelf Enable").map(|v| v >= 0.5).unwrap_or(true),
        });
        for b in 1..=5 {
            bands.push(EqBand {
                kind: FilterKind::Peak,
                freq: get(&format!("EQ Band {b} Center Frequency"))
                    .map(freq)
                    .unwrap_or(1000.0),
                gain_db: get(&format!("EQ Band {b} Gain")).map(gain).unwrap_or(0.0),
                q: 1.2,
                on: get(&format!("EQ Band {b} Enable"))
                    .map(|v| v >= 0.5)
                    .unwrap_or(true),
            });
        }
        bands.push(EqBand {
            kind: FilterKind::HighShelf,
            freq: get("High Shelf Frequency").map(freq).unwrap_or(18_000.0),
            gain_db: get("High Shelf Gain").map(gain).unwrap_or(0.0),
            q: 0.707,
            on: get("High Shelf Enable").map(|v| v >= 0.5).unwrap_or(true),
        });
        bands.push(EqBand {
            kind: FilterKind::LowPass,
            freq: get("Low-Pass Cutoff Frequency")
                .map(freq)
                .unwrap_or(20_000.0),
            gain_db: 0.0,
            q: 0.707,
            on: on("Low-Pass Filter Enable"),
        });
        let master_db = get("Master Gain")
            .map(|v| (v * 36.0 - 24.0) as f32)
            .unwrap_or(0.0);
        return Effect::ParametricEq { bands, master_db };
    }
    if lname == "full reverb" || lname == "studio reverb" {
        let wet = match (
            get("Wet Output Level"),
            get("Output Level (Reverberation)"),
            get("Output Level (Early Reflections)"),
        ) {
            (Some(w), _, _) => w,
            (None, Some(r), e) => r + e.unwrap_or(0.0),
            _ => 0.3,
        };
        let dry = get("Dry Output Level")
            .or(get("Output Level (Dry)"))
            .unwrap_or(0.8);
        return Effect::StudioReverb {
            full: lname == "full reverb",
            decay: get("Reverberation Decay Time")
                .or(get("Decay"))
                .or(get("Room Size"))
                .unwrap_or(0.6)
                .clamp(0.0, 1.0) as f32,
            predelay_ms: get("Reverberation Pre-Delay Time")
                .map(|v| (v * 200.0) as f32)
                .unwrap_or(20.0),
            damping: 0.5,
            width: get("Width").unwrap_or(1.0).clamp(0.0, 1.0) as f32,
            early: get("Output Level (Early Reflections)")
                .map(|v| (v * 3.0).min(1.0) as f32)
                .unwrap_or(0.4),
            dry: dry.clamp(0.0, 1.0) as f32,
            wet: wet.clamp(0.0, 1.0) as f32,
        };
    }
    // effects AudioCraft has under the same name (default settings; their stored scales aren't known yet)
    if !matches!(lname.as_str(), "reverb" | "chorus" | "delay" | "flanger") {
        if let Some(e) = crate::menus::effect_named(name) {
            return e;
        }
    }
    if lname.contains("reverb") {
        let wet = match (
            get("Wet Output Level"),
            get("Output Level (Reverberation)"),
            get("Output Level (Early Reflections)"),
        ) {
            (Some(w), _, _) => Some(w),
            (None, Some(r), e) => Some(r + e.unwrap_or(0.0)),
            _ => None,
        };
        let dry = get("Dry Output Level").or(get("Output Level (Dry)"));
        let room = get("Room Size")
            .or(get("Decay Time"))
            .or(get("Reverberation Decay Time"))
            .unwrap_or(0.6) as f32;
        return Effect::Reverb {
            room: room.clamp(0.0, 1.0),
            damping: 0.5,
            width: 1.0,
            mix: wet_mix(dry, wet).clamp(0.0, 1.0),
        };
    }
    if lname.contains("chorus") {
        return Effect::Chorus {
            rate: (0.1 + get("Modulation Rate").unwrap_or(0.1) * 5.0) as f32,
            depth_ms: (2.0 + get("Delay Time").unwrap_or(0.3) * 15.0) as f32,
            mix: wet_mix(get("Dry Output Level"), get("Wet Output Level")).clamp(0.0, 1.0),
        };
    }
    if lname.contains("flanger") {
        return Effect::Flanger {
            rate: 0.25,
            depth_ms: 3.0,
            feedback: get("Feedback").unwrap_or(0.5).clamp(0.0, 0.95) as f32,
            mix: 0.5,
        };
    }
    if lname.contains("delay") || lname.contains("echo") {
        return Effect::Delay {
            time_ms: get("Delay Time")
                .map(|v| (v * 2000.0).max(1.0) as f32)
                .unwrap_or(350.0),
            feedback: get("Feedback").unwrap_or(0.35).clamp(0.0, 0.95) as f32,
            mix: get("Wet Output Level")
                .or(get("Mix"))
                .unwrap_or(0.35)
                .clamp(0.0, 1.0) as f32,
        };
    }
    let defaults = Effect::all_defaults();
    let by = |pred: &dyn Fn(&Effect) -> bool| defaults.iter().find(|e| pred(e)).cloned();
    let pick = if lname.contains("compress") || lname.contains("dynamics") {
        by(&|e| matches!(e, Effect::Compressor { .. }))
    } else if lname.contains("limit") {
        by(&|e| matches!(e, Effect::HardLimiter { .. }))
    } else if lname.contains("gate") {
        by(&|e| matches!(e, Effect::NoiseGate { .. }))
    } else if lname.contains("hum") {
        by(&|e| matches!(e, Effect::DeHum { .. }))
    } else if lname.contains("amplify") || lname == "gain" {
        by(&|e| matches!(e, Effect::Amplify { .. }))
    } else if lname.contains("distortion") {
        by(&|e| matches!(e, Effect::Distortion { .. }))
    } else if lname.contains("stereo expander") || lname.contains("widen") {
        by(&|e| matches!(e, Effect::StereoExpander { .. }))
    } else if lname.contains("filter") {
        by(&|e| matches!(e, Effect::Filter { .. }))
    } else {
        None
    };
    pick.unwrap_or_else(|| Effect::Imported {
        name: name.to_string(),
        params: params.iter().map(|(k, v)| (k.clone(), *v as f32)).collect(),
    })
}

/// Import a session. Clip `file_id`s are indexes into the returned file list.
pub fn import(path: &Path) -> Result<(Session, Vec<PathBuf>), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let text = text.trim_start_matches('\u{feff}');
    let opt = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(text, opt)
        .map_err(|e| format!("not a valid session XML file: {e}"))?;
    let root = doc.root_element();
    let sess_node = if root.tag_name().name().eq_ignore_ascii_case("session") {
        root
    } else {
        child(root, "session").ok_or("the file has no <session> element")?
    };
    let rate = attr_f64(sess_node, &["sampleRate"])
        .map(|r| r as u32)
        .filter(|r| (1000..=768_000).contains(r))
        .unwrap_or(48000);
    let name = child(sess_node, "name")
        .and_then(|n| n.text())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or("Session".into())
        });
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();

    // files
    let mut file_ids: Vec<String> = vec![];
    let mut files: Vec<PathBuf> = vec![];
    let files_node = child(root, "files").or_else(|| child(sess_node, "files"));
    if let Some(fnode) = files_node {
        for f in children(fnode, "file") {
            let id = f.attribute("id").unwrap_or("").to_string();
            let abs = f.attribute("absolutePath").map(PathBuf::from);
            let rel = f
                .attribute("relativePath")
                .map(|r| dir.join(r.replace('\\', "/")));
            let by_name = abs
                .as_ref()
                .and_then(|a| a.file_name())
                .map(|n| dir.join(n));
            let chosen = [abs.clone(), rel.clone(), by_name]
                .into_iter()
                .flatten()
                .find(|p| p.exists())
                .or(abs)
                .or(rel)
                .unwrap_or_default();
            file_ids.push(id);
            files.push(chosen);
        }
    }
    let r = rate as f64;
    let mut s = Session::new(&name, rate, 0);
    let tracks_node = child(sess_node, "tracks").ok_or("the session has no <tracks>")?;
    let mut next_id = 100u64;
    for (ti, tn) in tracks_node
        .children()
        .filter(|c| c.is_element())
        .enumerate()
    {
        let tag = tn.tag_name().name().to_lowercase();
        if tag == "mastertrack" || tag == "mastertrackparameters" {
            let p = read_params(tn);
            if let Some(v) = p.volume {
                s.master_db = volume_db(v);
            }
            continue;
        }
        if !tag.contains("track") {
            continue;
        }
        let tname = child(tn, "trackParameters")
            .and_then(|tp| child(tp, "name"))
            .and_then(|n| n.text())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Track {}", ti + 1));
        let ap = child(tn, "trackAudioParameters");
        let stereo = ap
            .and_then(|a| a.attribute("audioChannelType"))
            .map(|t| !t.eq_ignore_ascii_case("mono"))
            .unwrap_or(true);
        let solo = ap
            .and_then(|a| a.attribute("solo"))
            .map(|v| v == "true")
            .unwrap_or(false);
        let arm = ap
            .and_then(|a| a.attribute("recordArmed"))
            .map(|v| v == "true")
            .unwrap_or(false);
        let height = child(tn, "trackParameters")
            .and_then(|tp| attr_f64(tp, &["trackHeight"]))
            .map(|h| (h as f32).clamp(50.0, 300.0))
            .unwrap_or(96.0);
        // fader, mute and pan live inside <trackAudioParameters> (older files: on the track)
        let mut tp = ap.map(read_params).unwrap_or_default();
        let direct = read_params(tn);
        tp.volume = tp.volume.or(direct.volume);
        tp.pan = tp.pan.or(direct.pan);
        tp.mute = tp.mute.or(direct.mute);
        let (rack, fx) = read_rack(tn);
        next_id += 1;
        let mut track = Track {
            id: next_id,
            name: tname,
            volume_db: tp.volume.map(volume_db).unwrap_or(0.0),
            pan: tp.pan.map(pan_norm).unwrap_or(0.0),
            mute: tp.mute.unwrap_or(false),
            solo,
            arm,
            stereo,
            clips: vec![],
            rack,
            fx,
            prerender: false,
            color: TRACK_COLORS[s.tracks.len() % TRACK_COLORS.len()],
            height,
            io: crate::session::TrackIo {
                monitor: ap
                    .and_then(|a| a.attribute("monitoring"))
                    .map(|v| v == "true")
                    .unwrap_or(false),
                automation: ap
                    .and_then(|a| attr_f64(a, &["automationMode"]))
                    .map(|v| (v as i64).clamp(0, 4) as u8)
                    .unwrap_or(1),
                ..Default::default()
            },
            minimized: false,
            send: Default::default(),
            eq_on: false,
            eq: crate::equi::standard_bands(),
            eq_master: 0.0,
            hidden: false,
            vol_env: vec![],
            show_vol_env: false,
            auto: Default::default(),
        };
        for cn in children(tn, "audioClip") {
            let fid = cn.attribute("fileID").unwrap_or("");
            let Some(fi) = file_ids.iter().position(|x| x == fid) else {
                continue;
            };
            let start = attr_f64(cn, &["startPoint"]).unwrap_or(0.0).max(0.0);
            let end = attr_f64(cn, &["endPoint"]).unwrap_or(start);
            let sin = attr_f64(cn, &["sourceInPoint"]).unwrap_or(0.0).max(0.0);
            let sout = attr_f64(cn, &["sourceOutPoint"]).unwrap_or(sin + (end - start));
            let len_samples = (end - start)
                .max(sout - sin)
                .max(1.0)
                .min((end - start).max(1.0));
            let fade_len = |tag: &str| -> f64 {
                child(cn, tag)
                    .map(|f| {
                        let a = attr_f64(f, &["startPoint"]).unwrap_or(0.0);
                        let b = attr_f64(f, &["endPoint"]).unwrap_or(a);
                        ((b - a).abs() / r).min(len_samples / r)
                    })
                    .unwrap_or(0.0)
            };
            let cp = read_params(cn);
            let (crack, cfx) = read_rack(cn);
            let (fin, fout) = (fade_len("fadeIn"), fade_len("fadeOut"));
            // the fade's curve type: "log" bends it up (estimate of the original's look);
            // cosine/linear are the plain curve
            let fade_shape = |tag: &str| -> f32 {
                match child(cn, tag)
                    .and_then(|f| f.attribute("type"))
                    .map(|t| t.to_ascii_lowercase())
                {
                    Some(t) if t.starts_with("log") => LOG_FADE_SHAPE,
                    Some(t) if t.starts_with("exp") => -LOG_FADE_SHAPE,
                    _ => 0.0,
                }
            };
            next_id += 1;
            // clip keyframes are placed in source samples (counted from the file's start, not
            // the trimmed clip's start): move them by the trim and cut them at the clip's edges
            let speed_now = {
                let src = sout - sin;
                let tl = (end - start).max(1.0);
                if src > 1.0 && ((src - tl) / tl).abs() > 0.005 {
                    (src / tl).clamp(0.1, 10.0)
                } else {
                    1.0
                }
            };
            let to_clip = |keys: &[(f64, f64)]| -> Vec<(f64, f64)> {
                keys.iter()
                    .map(|(t, v)| ((t - sin) / speed_now / r, *v))
                    .collect()
            };
            let env: Vec<(f64, f32)> = crop_keys(&to_clip(&cp.keys), len_samples / r)
                .into_iter()
                .map(|(t, v)| (t, env_key_db(v)))
                .collect();
            let pan_env: Vec<(f64, f32)> = crop_keys(&to_clip(&cp.pan_keys), len_samples / r)
                .into_iter()
                .map(|(t, v)| (t, pan_norm(v)))
                .collect();
            track.clips.push(Clip {
                id: next_id,
                file_id: fi as u64,
                name: cn.attribute("name").unwrap_or("Clip").to_string(),
                start: start / r,
                offset: sin / r,
                len: len_samples / r,
                gain_db: if env.is_empty() {
                    cp.volume.map(volume_db).unwrap_or(0.0)
                } else {
                    0.0
                },
                muted: cp.mute.unwrap_or(false),
                fade_in: fin,
                fade_out: fout,
                // the session states its fades (crossfades included); keep them as they are
                auto_in: fin <= 0.0,
                auto_out: fout <= 0.0,
                curves: crate::session::ClipCurves {
                    fin_shape: fade_shape("fadeIn"),
                    fout_shape: fade_shape("fadeOut"),
                    ..Default::default()
                },
                env,
                pan_env,
                rack: crack,
                fx: cfx,
                // a stretched clip covers a different stretch of source than of timeline
                speed: speed_now,
            });
        }
        s.tracks.push(track);
    }
    // markers
    for mk in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("marker"))
    {
        let t = attr_f64(mk, &["startPoint", "sampleOffset", "position"]).unwrap_or(0.0) / r;
        let d = attr_f64(mk, &["duration", "length"]).unwrap_or(0.0) / r;
        let nm = mk
            .attribute("name")
            .map(|s| s.to_string())
            .or_else(|| {
                child(mk, "name")
                    .and_then(|n| n.text())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| format!("Marker {:02}", s.markers.len() + 1));
        s.markers.push(Marker {
            name: nm,
            time: t.max(0.0),
            duration: d.max(0.0),
        });
    }
    s.next_id = next_id + 1;
    s.path = Some(path.to_path_buf());
    Ok((s, files))
}

/// Write an `<effectsRack>`: the rack controls and each slot (with the exact effect as JSON).
fn write_rack(x: &mut String, ind: &str, slots: &[EffectSlot], o: &RackOpts) {
    if slots.is_empty() && *o == RackOpts::default() {
        return;
    }
    let lin = |db: f32| 10f64.powf(db as f64 / 20.0);
    x.push_str(&format!(
        "{ind}<effectsRack preFader=\"{}\" powered=\"{}\">\n",
        o.pre_fader, o.power
    ));
    x.push_str(&format!("{ind}  <component id=\"rackMix\" powered=\"true\">\n{ind}    <parameter index=\"0\" parameterValue=\"{}\"/>\n{ind}  </component>\n", 1.0 - o.mix.clamp(0.0, 1.0)));
    x.push_str(&format!("{ind}  <component id=\"rackInput\" powered=\"true\">\n{ind}    <parameter index=\"0\" parameterValue=\"{}\"/>\n{ind}  </component>\n", lin(o.in_db)));
    x.push_str(&format!("{ind}  <component id=\"rackOutput\" powered=\"true\">\n{ind}    <parameter index=\"0\" parameterValue=\"{}\"/>\n{ind}  </component>\n", lin(o.out_db)));
    for (i, s) in slots.iter().enumerate() {
        let json = serde_json::to_string(&s.effect).unwrap_or_default();
        x.push_str(&format!("{ind}  <slot powered=\"{}\" slotIndex=\"{i}\">\n{ind}    <component name=\"{}\" audiocraft=\"{}\"/>\n{ind}  </slot>\n", s.enabled, esc(s.effect.name()), esc(&json)));
    }
    x.push_str(&format!("{ind}</effectsRack>\n"));
}

/// Export a session. `files[i]` is the audio path for clips whose `file_id == i`.
pub fn export(s: &Session, files: &[String], path: &Path) -> Result<(), String> {
    let r = s.rate as f64;
    let smp = |t: f64| (t * r).round() as i64;
    let lin = |db: f32| {
        if db <= -95.9 {
            0.0
        } else {
            10f64.powf(db as f64 / 20.0)
        }
    };
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<!DOCTYPE sesx>\n<sesx version=\"1.9\">\n");
    x.push_str(&format!(
        "  <session appBuild=\"AudioCraft 1.0\" audioChannelType=\"stereo\" bitDepth=\"32\" duration=\"{}\" sampleRate=\"{}\">\n",
        smp(s.length()),
        s.rate
    ));
    x.push_str(&format!(
        "    <name>{}</name>\n    <tracks>\n",
        esc(&s.name)
    ));
    for (ti, t) in s.tracks.iter().enumerate() {
        x.push_str(&format!(
            "      <audioTrack id=\"{}\" index=\"{}\" select=\"{}\" visible=\"true\">\n",
            10001 + ti,
            ti + 1,
            ti == s.selected_track
        ));
        x.push_str(&format!(
            "        <trackParameters trackHeight=\"{}\" trackHue=\"-1\" trackMinimized=\"false\">\n          <name>{}</name>\n        </trackParameters>\n",
            t.height.round(),
            esc(&t.name)
        ));
        x.push_str(&format!(
            "        <trackAudioParameters audioChannelType=\"{}\" automationMode=\"{}\" monitoring=\"{}\" recordArmed=\"{}\" solo=\"{}\">\n          <trackOutput outputID=\"10000\" type=\"trackID\"/>\n          <trackInput inputID=\"-1\"/>\n        </trackAudioParameters>\n",
            if t.stereo { "stereo" } else { "mono" },
            t.io.automation,
            t.io.monitor,
            t.arm,
            t.solo
        ));
        write_rack(&mut x, "        ", &t.rack, &t.fx);
        let eff = crate::session::effective_fades(t);
        for (ci, c) in t.clips.iter().enumerate() {
            let (fin, fout, _, _) = eff
                .get(ci)
                .copied()
                .unwrap_or((c.fade_in, c.fade_out, false, false));
            let fid = c.file_id as usize;
            if fid >= files.len() {
                continue;
            }
            x.push_str(&format!(
                "        <audioClip fileID=\"{}\" id=\"{}\" name=\"{}\" startPoint=\"{}\" endPoint=\"{}\" sourceInPoint=\"{}\" sourceOutPoint=\"{}\" select=\"false\" zOrder=\"{}\">\n",
                fid,
                ci,
                esc(&c.name),
                smp(c.start),
                smp(c.end()),
                smp(c.offset),
                smp(c.offset + c.len * c.speed),
                ci
            ));
            write_rack(&mut x, "          ", &c.rack, &c.fx);
            if fin > 0.0 {
                x.push_str(&format!(
                    "          <fadeIn startPoint=\"0\" endPoint=\"{}\" {}/>\n",
                    smp(fin),
                    fade_type_attrs(c.curves.fin_shape, true)
                ));
            }
            if fout > 0.0 {
                x.push_str(&format!(
                    "          <fadeOut startPoint=\"{}\" endPoint=\"{}\" {}/>\n",
                    smp(c.len - fout),
                    smp(c.len),
                    fade_type_attrs(c.curves.fout_shape, false)
                ));
            }
            x.push_str("          <component componentID=\"Audition.Fader\" id=\"clipGain\" name=\"volume\" powered=\"true\">\n");
            if c.env.is_empty() {
                x.push_str(&format!(
                    "            <parameter index=\"0\" name=\"volume\" parameterValue=\"{}\"/>\n",
                    lin(c.gain_db)
                ));
            } else {
                x.push_str(&format!(
                    "            <parameter index=\"0\" name=\"volume\" parameterValue=\"{}\">\n",
                    lin(c.gain_db)
                ));
                for (et, ed) in &c.env {
                    x.push_str(&format!(
                        "              <parameterKeyframe sampleOffset=\"{}\" value=\"{}\"/>\n",
                        smp(c.offset + *et * c.speed),
                        env_key_value(*ed + c.gain_db)
                    ));
                }
                x.push_str("            </parameter>\n");
            }
            x.push_str("          </component>\n");
            if !c.pan_env.is_empty() {
                x.push_str("          <component componentID=\"Audition.StereoPanner\" id=\"clipPan\" name=\"StereoPanner\" powered=\"true\">\n            <parameter index=\"1\" name=\"Pan\" parameterValue=\"0\">\n");
                for (et, ep) in &c.pan_env {
                    x.push_str(&format!(
                        "              <parameterKeyframe sampleOffset=\"{}\" value=\"{}\"/>\n",
                        smp(c.offset + *et * c.speed),
                        (ep * 100.0).round()
                    ));
                }
                x.push_str("            </parameter>\n          </component>\n");
            }
            if c.muted {
                x.push_str("          <component componentID=\"Audition.Mute\" id=\"clipMute\" name=\"Mute\" powered=\"true\">\n            <parameter index=\"0\" name=\"mute\" parameterValue=\"1\"/>\n          </component>\n");
            }
            x.push_str("        </audioClip>\n");
        }
        x.push_str(&format!(
            "        <component componentID=\"Audition.Fader\" id=\"trackVolume\" name=\"volume\" powered=\"true\">\n          <parameter index=\"0\" name=\"volume\" parameterValue=\"{}\"/>\n        </component>\n",
            lin(t.volume_db)
        ));
        x.push_str(&format!(
            "        <component componentID=\"Audition.StereoPanner\" id=\"trackPan\" name=\"StereoPanner\" powered=\"true\">\n          <parameter index=\"1\" name=\"Pan\" parameterValue=\"{}\"/>\n        </component>\n",
            (t.pan * 100.0).round()
        ));
        x.push_str(&format!(
            "        <component componentID=\"Audition.Mute\" id=\"trackMute\" name=\"Mute\" powered=\"true\">\n          <parameter index=\"0\" name=\"mute\" parameterValue=\"{}\"/>\n        </component>\n",
            if t.mute { 1 } else { 0 }
        ));
        x.push_str("      </audioTrack>\n");
    }
    x.push_str(&format!(
        "      <masterTrack id=\"10000\" index=\"{}\" select=\"false\" visible=\"true\">\n        <trackParameters trackHeight=\"60\" trackHue=\"-1\" trackMinimized=\"false\">\n          <name>Master</name>\n        </trackParameters>\n        <component componentID=\"Audition.Fader\" id=\"trackVolume\" name=\"volume\" powered=\"true\">\n          <parameter index=\"0\" name=\"volume\" parameterValue=\"{}\"/>\n        </component>\n      </masterTrack>\n",
        s.tracks.len() + 1,
        lin(s.master_db)
    ));
    x.push_str("    </tracks>\n");
    if !s.markers.is_empty() {
        x.push_str("    <markers>\n");
        for m in &s.markers {
            x.push_str(&format!(
                "      <marker name=\"{}\" startPoint=\"{}\" duration=\"{}\" type=\"Cue\"/>\n",
                esc(&m.name),
                smp(m.time),
                smp(m.duration)
            ));
        }
        x.push_str("    </markers>\n");
    }
    x.push_str("  </session>\n  <files>\n");
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    for (i, f) in files.iter().enumerate() {
        let p = PathBuf::from(f);
        let rel = p
            .strip_prefix(&dir)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| {
                p.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
        x.push_str(&format!("    <file absolutePath=\"{}\" id=\"{}\" mediaHandler=\"AmioWav\" relativePath=\"{}\"/>\n", esc(&f.replace('\\', "/")), i, esc(&rel)));
    }
    x.push_str("  </files>\n</sesx>\n");
    crate::audio::write_atomic(path, x.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_tracks_clips_fades_and_envelopes() {
        let dir = std::env::temp_dir().join("audiocraft_sesx");
        let _ = std::fs::create_dir_all(&dir);
        let mut s = Session::new("Round & Trip", 48000, 2);
        s.tracks[0].volume_db = -6.0;
        s.tracks[0].pan = -0.5;
        s.tracks[1].mute = true;
        s.tracks[1].stereo = false;
        s.markers.push(Marker {
            name: "Intro".into(),
            time: 1.0,
            duration: 0.0,
        });
        s.tracks[0].clips.push(Clip {
            id: 1,
            file_id: 0,
            name: "Vox <1>".into(),
            start: 2.0,
            offset: 0.5,
            len: 3.0,
            gain_db: 0.0,
            muted: false,
            fade_in: 0.25,
            fade_out: 0.5,
            env: vec![(0.0, 0.0), (1.5, -12.0)],
            pan_env: vec![],
            rack: vec![],
            speed: 1.0,
            fx: Default::default(),
            auto_in: true,
            auto_out: true,
            curves: Default::default(),
        });
        s.tracks[1].clips.push(Clip {
            id: 2,
            file_id: 1,
            name: "Beat".into(),
            start: 0.0,
            offset: 0.0,
            len: 4.0,
            gain_db: -3.0,
            muted: false,
            fade_in: 0.0,
            fade_out: 0.0,
            env: vec![],
            pan_env: vec![],
            rack: vec![],
            speed: 1.0,
            fx: Default::default(),
            auto_in: true,
            auto_out: true,
            curves: Default::default(),
        });
        let p = dir.join("t.sesx");
        export(&s, &["C:/a/vox.wav".into(), "C:/a/beat.wav".into()], &p).unwrap();
        let (r, files) = import(&p).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(r.name, "Round & Trip");
        assert_eq!(r.tracks.len(), 2);
        assert!((r.tracks[0].volume_db + 6.0).abs() < 0.01);
        assert!((r.tracks[0].pan + 0.5).abs() < 0.01);
        assert!(r.tracks[1].mute && !r.tracks[1].stereo);
        let c = &r.tracks[0].clips[0];
        assert_eq!(c.name, "Vox <1>");
        assert!(
            (c.start - 2.0).abs() < 1e-4
                && (c.offset - 0.5).abs() < 1e-4
                && (c.len - 3.0).abs() < 1e-4
        );
        assert!((c.fade_in - 0.25).abs() < 1e-4 && (c.fade_out - 0.5).abs() < 1e-4);
        assert_eq!(c.env.len(), 2);
        assert!((c.env[1].1 + 12.0).abs() < 0.01);
        assert!((r.tracks[1].clips[0].gain_db + 3.0).abs() < 0.01);
        assert_eq!(r.markers.len(), 1);
    }

    #[test]
    fn envelope_keyframes_are_positions_not_gain() {
        // 0.65 is the clip's own level, 0 is silence, 1 is the top of the envelope
        assert!(env_key_db(0.65).abs() < 0.01);
        assert!(env_key_db(0.0) <= -95.0);
        assert!((env_key_db(1.0) - 6.02).abs() < 0.05);
        assert!(env_key_db(0.2466386) < -10.0 && env_key_db(0.2466386) > -20.0);
        for d in [-30.0f32, -6.0, 0.0, 3.0] {
            assert!((env_key_db(env_key_value(d)) - d).abs() < 0.01);
        }
    }

    /// The person's session: a clip trimmed 23225 samples into its file keeps a volume ramp
    /// whose keyframes Audition stores from the file's start. They must land inside the clip
    /// (not pile up on its right edge as a flat line).
    #[test]
    fn trimmed_clip_keyframes_count_from_the_source_start() {
        let dir = std::env::temp_dir().join("audiocraft_sesx");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("trimmed_keys.sesx");
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<sesx version="1.9"><session sampleRate="48000"><tracks>
<audioTrack id="10001" index="1"><trackParameters><name>Track 1</name></trackParameters>
<audioClip fileID="0" id="3" name="Rain" startPoint="1054773" endPoint="1164536" sourceInPoint="23225" sourceOutPoint="132988">
  <component componentID="Audition.Fader" id="clipGain" name="volume" powered="true">
    <parameter index="0" name="volume" parameterValue="0">
      <parameterKeyframes enableSplines="false">
        <parameterKeyframe sampleOffset="26571" type="linear" value="0.65"/>
        <parameterKeyframe sampleOffset="100193" type="linear" value="0.2466386"/>
        <parameterKeyframe sampleOffset="121610" type="linear" value="0.2466386"/>
        <parameterKeyframe sampleOffset="131649" type="linear" value="0"/>
      </parameterKeyframes>
    </parameter>
  </component>
</audioClip>
<audioClip fileID="0" id="4" name="Ramp cut at both ends" startPoint="0" endPoint="48000" sourceInPoint="48000" sourceOutPoint="96000">
  <component componentID="Audition.Fader" id="clipGain" name="volume" powered="true">
    <parameter index="0" name="volume" parameterValue="0">
      <parameterKeyframe sampleOffset="0" value="0"/>
      <parameterKeyframe sampleOffset="144000" value="0.9"/>
    </parameter>
  </component>
</audioClip>
</audioTrack></tracks></session>
<files><file absolutePath="C:/x/rain.wav" id="0" relativePath="rain.wav"/></files></sesx>"#;
        std::fs::write(&p, xml).unwrap();
        let (s, _) = import(&p).unwrap();
        let c = &s.tracks[0].clips[0];
        let r = 48000.0;
        assert_eq!(c.env.len(), 4, "every keyframe is kept: {:?}", c.env);
        let want = [
            26571.0 - 23225.0,
            100193.0 - 23225.0,
            121610.0 - 23225.0,
            131649.0 - 23225.0,
        ];
        for (k, w) in c.env.iter().zip(want) {
            assert!((k.0 - w / r).abs() < 1e-6, "{:?} vs {}", k, w / r);
            assert!(k.0 < c.len, "inside the clip");
        }
        assert!(c.env[0].1.abs() < 0.01 && c.env[1].1 < -10.0 && c.env[3].1 <= -95.0);
        // a ramp running past both ends is cut with the in-between values at the edges
        let c = &s.tracks[0].clips[1];
        assert_eq!(c.env.len(), 2, "{:?}", c.env);
        assert!(c.env[0].0.abs() < 1e-9 && (c.env[1].0 - 1.0).abs() < 1e-6);
        assert!(
            (env_key_db(0.3) - c.env[0].1).abs() < 0.05,
            "start = value 0.3: {:?}",
            c.env
        );
        assert!(
            (env_key_db(0.6) - c.env[1].1).abs() < 0.05,
            "end = value 0.6: {:?}",
            c.env
        );
        // saving puts them back where Audition expects them
        let out = dir.join("trimmed_keys_out.sesx");
        export(&s, &["C:/x/rain.wav".into()], &out).unwrap();
        let (back, _) = import(&out).unwrap();
        let (a, b) = (&s.tracks[0].clips[0].env, &back.tracks[0].clips[0].env);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert!(
                (x.0 - y.0).abs() < 1e-4 && (x.1 - y.1).abs() < 0.05,
                "{x:?} vs {y:?}"
            );
        }
    }

    #[test]
    fn crop_keys_handles_points_outside_the_clip() {
        assert!(crop_keys(&[], 1.0).is_empty());
        assert_eq!(
            crop_keys(&[(-2.0, 0.5), (-1.0, 0.7)], 1.0),
            vec![(0.0, 0.7)]
        );
        assert_eq!(crop_keys(&[(2.0, 0.5), (3.0, 0.7)], 1.0), vec![(1.0, 0.5)]);
        assert_eq!(
            crop_keys(&[(-1.0, 0.0), (1.0, 1.0)], 2.0),
            vec![(0.0, 0.5), (1.0, 1.0)]
        );
        assert_eq!(crop_keys(&[(0.5, f64::NAN)], 1.0), vec![]);
    }

    #[test]
    fn bad_input_is_an_error_not_a_crash() {
        let dir = std::env::temp_dir().join("audiocraft_sesx");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("bad.sesx");
        std::fs::write(&p, "<sesx><session sampleRate=\"abc\"><tracks><audioTrack><audioClip fileID=\"9\" startPoint=\"x\"/></audioTrack></tracks></session></sesx>").unwrap();
        let (s, files) = import(&p).unwrap();
        assert!(files.is_empty());
        assert_eq!(s.rate, 48000);
        assert!(s.tracks[0].clips.is_empty());
        std::fs::write(&p, "not xml at all <<<").unwrap();
        assert!(import(&p).is_err());
        assert!(import(&dir.join("missing.sesx")).is_err());
    }
}
