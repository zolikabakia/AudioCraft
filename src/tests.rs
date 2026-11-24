//! Core audio tests (run with `cargo test`).

use crate::audio::{self, AudioData};
use crate::dsp::{self, Effect, EffectSlot, NoiseColor, Wave};
use crate::session::{Clip, Session, mixdown};
use std::sync::Arc;

fn energy(d: &AudioData) -> f64 {
    d.ch.iter()
        .flat_map(|c| c.iter())
        .map(|v| (*v as f64).powi(2))
        .sum()
}

#[test]
fn wav_roundtrip() {
    let dir = std::env::temp_dir().join("audiocraft_test");
    std::fs::create_dir_all(&dir).unwrap();
    let t = dsp::gen_tone(44100, 2, 0.5, 440.0, Wave::Sine, -6.0);
    for (bits, float) in [(16u16, false), (24, false), (32, true)] {
        let p = dir.join(format!("rt_{bits}_{float}.wav"));
        audio::save_wav(&p, &t, bits, float).unwrap();
        let l = audio::load(&p).unwrap();
        assert_eq!(l.data.rate, 44100);
        assert_eq!(l.data.channels(), 2);
        assert_eq!(l.data.frames(), t.frames());
        let err = t.ch[0]
            .iter()
            .zip(l.data.ch[0].iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(err < 1e-3, "bits {bits}: err {err}");
    }
}

#[test]
fn every_effect_is_finite() {
    let base = dsp::gen_noise(48000, 2, 0.5, NoiseColor::White, -6.0);
    for e in Effect::all_defaults() {
        let mut d = base.clone();
        let n = d.frames();
        dsp::apply_chain(&mut d, 0, n, &[EffectSlot::new(e.clone())]);
        assert!(
            d.ch.iter().flat_map(|c| c.iter()).all(|v| v.is_finite()),
            "{} produced NaN",
            e.name()
        );
        assert_eq!(d.frames(), n);
    }
}

#[test]
fn edit_operations() {
    let d = dsp::gen_tone(48000, 1, 1.0, 100.0, Wave::Sine, 0.0);
    let cut = d.without(0, 24000);
    assert_eq!(cut.frames(), 24000);
    let ins = cut.inserted(0, &d.slice(0, 100));
    assert_eq!(ins.frames(), 24100);
    let mut n = d.clone();
    dsp::normalize(&mut n, 0, 48000, -6.0);
    assert!((n.peak() - audio::undb(-6.0)).abs() < 1e-3);
    let fast = dsp::change_speed(&d, 0, 48000, 2.0);
    assert!((fast.frames() as i64 - 24000).abs() < 4);
    let r = audio::resample(&d, 44100);
    assert!((r.frames() as i64 - 44100).abs() < 2);
}

#[test]
fn noise_reduction_reduces_noise() {
    let rate = 48000;
    let noise = dsp::gen_noise(rate, 1, 2.0, NoiseColor::White, -30.0);
    let prof = dsp::capture_noise(&noise, 0, rate as usize);
    let mut d = noise.clone();
    let n = d.frames();
    dsp::noise_reduce(&mut d, 0, n, &prof, 1.0, 30.0);
    assert!(
        energy(&d) < energy(&noise) * 0.25,
        "{} vs {}",
        energy(&d),
        energy(&noise)
    );
}

#[test]
fn session_mixdown() {
    let rate = 48000;
    let tone = Arc::new(dsp::gen_tone(rate, 2, 1.0, 220.0, Wave::Sine, -6.0));
    let mut s = Session::new("t", rate, 2);
    s.tracks[0].clips.push(Clip {
        id: 100,
        file_id: 1,
        name: "a".into(),
        start: 0.5,
        offset: 0.0,
        len: 1.0,
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
        fade_in: 0.1,
        fade_out: 0.1,
    });
    s.tracks[1].clips.push(Clip {
        id: 101,
        file_id: 1,
        name: "b".into(),
        start: 0.0,
        offset: 0.25,
        len: 0.5,
        gain_db: -6.0,
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
    });
    assert!((s.length() - 1.5).abs() < 1e-9);
    let t2 = tone.clone();
    let mix = s.snapshot(&move |_| Some(t2.clone()));
    let d = mixdown(&mix, rate, 0.0, s.length());
    assert_eq!(d.channels(), 2);
    assert_eq!(d.frames(), (1.5 * rate as f64) as usize);
    assert!(d.peak() > 0.1 && d.peak() <= 1.0);
    // split keeps total length
    let right = s.split_clip(100, 1.0).unwrap();
    let (t, c) = s.find_clip(right).unwrap();
    assert!((s.tracks[t].clips[c].offset - 0.5).abs() < 1e-9);
    assert!((s.length() - 1.5).abs() < 1e-9);
    // muting a track silences it
    s.tracks[0].mute = true;
    s.tracks[1].mute = true;
    let t3 = tone.clone();
    let mix = s.snapshot(&move |_| Some(t3.clone()));
    let d = mixdown(&mix, rate, 0.0, s.length());
    assert!(d.peak() < 1e-6);
}

#[test]
fn stats_and_spectrum() {
    let d = dsp::gen_tone(48000, 2, 0.5, 1000.0, Wave::Sine, -6.0);
    let rows = dsp::stats(&d, 0, d.frames());
    assert!(rows[0].1[0].starts_with("-6.0"));
    let spec = dsp::average_spectrum(&d.ch[0], 0, d.frames(), 4096);
    let peak_bin = spec
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0;
    let hz = peak_bin as f32 * 24000.0 / 2048.0;
    assert!((hz - 1000.0).abs() < 20.0, "peak at {hz}");
}

#[test]
fn engine_plays_and_stops() {
    use crate::engine::{Shared, Source};
    let mut s = Shared::new();
    let d = Arc::new(dsp::gen_tone(48000, 1, 0.1, 440.0, Wave::Sine, -6.0));
    s.out_rate = 48000;
    s.source = Source::File(d.clone());
    s.pos = 0.0;
    s.end = d.duration();
    s.playing = true;
    let mut total = 0.0f32;
    for _ in 0..10 {
        s.render(512);
        total += s.scratch[1].iter().map(|v| v.abs()).sum::<f32>();
    }
    assert!(total > 1.0, "mono should be heard on both sides");
    assert!(!s.playing, "playback should stop at the end");
    // looping keeps playing
    s.looping = true;
    s.loop_start = 0.0;
    s.pos = 0.0;
    s.playing = true;
    for _ in 0..30 {
        s.render(512);
    }
    assert!(s.playing && s.pos <= d.duration() + 0.02);
}

#[test]
fn scrubbing_plays_a_short_grain_while_stopped() {
    use crate::engine::{Shared, Source};
    let mut s = Shared::new();
    let d = Arc::new(dsp::gen_tone(48000, 2, 2.0, 440.0, Wave::Sine, -6.0));
    s.out_rate = 48000;
    s.source = Source::File(d.clone());
    s.pos = 1.0;
    s.scrub = Some(1.0);
    let mut heard = 0.0f32;
    for _ in 0..4 {
        s.render(512);
        heard += s.scratch[0].iter().map(|v| v.abs()).sum::<f32>();
    }
    assert!(heard > 1.0, "scrubbing should be audible");
    assert!(!s.playing, "scrubbing never starts playback");
    // the pointer stays put: the grain runs out and the output falls silent
    for _ in 0..8 {
        s.render(512);
    }
    assert!(
        s.scratch[0].iter().all(|v| *v == 0.0),
        "no sound once the grain is played"
    );
    // dragging far ahead jumps there instead of playing everything in between
    s.scrub = Some(1.9);
    s.render(256);
    assert!(s.pos > 1.8, "jumped to the pointer, pos {}", s.pos);
    s.scrub = None;
    s.render(256);
    assert!(s.scratch[0].iter().all(|v| *v == 0.0));
}

#[test]
fn track_polarity_invert_flips_the_mix_and_old_sessions_still_load() {
    let mut s = crate::session::Session::new("P", 48000, 1);
    let m = s.snapshot(&|_| None);
    assert!(m.tracks[0].gain_l > 0.0);
    s.tracks[0].io.invert = true;
    let m = s.snapshot(&|_| None);
    assert!(m.tracks[0].gain_l < 0.0 && m.tracks[0].gain_r < 0.0);
    // a track saved before the input/output settings existed gets the defaults
    let mut v = serde_json::to_value(&s.tracks[0]).unwrap();
    v.as_object_mut().unwrap().remove("io");
    let t: crate::session::Track = serde_json::from_value(v).unwrap();
    assert_eq!(t.io, crate::session::TrackIo::default());
    assert_eq!(
        crate::session::AUTOMATION_MODES[t.io.automation as usize],
        "Read"
    );
}

#[test]
fn volume_automation_is_carried_into_playback_but_off_ignores_it() {
    let mut s = crate::session::Session::new("Auto", 48000, 1);
    let env = vec![(0.0, 0.0), (2.0, -12.0)];
    let g = crate::session::automation_gain(&env, 1.0, 1);
    assert!((g - crate::audio::undb(-6.0)).abs() < 1e-5);
    assert!((crate::session::automation_gain(&env, 1.0, 0) - 1.0).abs() < 1e-6);
    s.tracks[0].vol_env = env.clone();
    s.master.vol_env = vec![(0.0, -3.0)];
    let m = s.snapshot(&|_| None);
    assert_eq!(m.tracks[0].vol_env, env);
    assert_eq!(m.tracks[0].automation, 1);
    assert_eq!(m.master_automation, 1);
    assert_eq!(m.master_env.len(), 1);
}

#[test]
fn razor_cut_adds_no_handles_on_a_flat_envelope() {
    let mut s = crate::session::Session::new("R", 48000, 1);
    let mut c = crate::session::Clip {
        id: 7,
        file_id: 1,
        name: "c".into(),
        start: 0.0,
        offset: 0.0,
        len: 4.0,
        gain_db: 0.0,
        muted: false,
        fade_in: 0.0,
        fade_out: 0.0,
        env: vec![(0.0, -6.0), (1.0, 0.0), (3.5, 0.0)],
        pan_env: vec![],
        rack: vec![],
        speed: 1.0,
        fx: Default::default(),
        auto_in: true,
        auto_out: true,
        curves: Default::default(),
    };
    s.tracks[0].clips.push(c.clone());
    let right = s.split_clip(7, 2.0).unwrap();
    let r = s.clip_mut(right).unwrap();
    // the line is flat at the cut: no new point appears there
    assert_eq!(r.env.len(), 1, "{:?}", r.env);
    assert!((crate::session::env_db(&r.env, 0.0)).abs() < 1e-6);
    // where the line slopes, one point keeps its level
    c.id = 8;
    c.start = 10.0;
    s.tracks[0].clips.push(c);
    let r2 = s.split_clip(8, 10.5).unwrap();
    let r2 = s.clip_mut(r2).unwrap();
    assert!((r2.env[0].1 + 3.0).abs() < 1e-4 && r2.env[0].0 == 0.0);
    // a razor cut can cut the right-hand part again
    assert!(s.split_clip(right, 3.0).is_some());
}

#[test]
fn master_mute_metronome_and_track_eq_reach_the_mix() {
    let mut s = crate::session::Session::new("M", 48000, 1);
    s.master.mute = true;
    assert_eq!(s.snapshot(&|_| None).master, 0.0);
    s.master.mute = false;
    s.metronome.on = true;
    let m = s.snapshot(&|_| None);
    let click = m.metronome.expect("metronome on");
    assert!(click.at(0.001).abs() > 0.01, "a click on the first beat");
    assert_eq!(click.at(0.25), 0.0, "silence between beats at 120 BPM");
    assert!(m.tracks[0].rack.is_empty());
    s.tracks[0].eq_on = true;
    let m = s.snapshot(&|_| None);
    assert!(matches!(
        m.tracks[0].rack.last().map(|e| &e.effect),
        Some(crate::dsp::Effect::ParametricEq { .. })
    ));
    // minimised tracks take one row; new tracks have no input, so they cannot be armed
    s.tracks[0].minimized = true;
    assert_eq!(s.tracks[0].shown_height(), crate::session::MIN_TRACK_H);
    assert!(!s.tracks[0].can_record());
}

#[test]
fn stretch_and_pitch_changes_length_and_pitch_as_asked() {
    let d = dsp::gen_tone(48000, 1, 1.0, 440.0, Wave::Sine, -12.0);
    let n = d.frames();
    // independent: 200% stretch doubles the length, no pitch change requested
    let s = crate::dialogs::stretch_and_pitch(&d, 0, n, 0.5, 0.0, 1);
    assert!(
        (s.frames() as f64 / n as f64 - 2.0).abs() < 0.05,
        "{}",
        s.frames()
    );
    // pitch only: same length
    let p = crate::dialogs::stretch_and_pitch(&d, 0, n, 1.0, 12.0, 1);
    assert!((p.frames() as i64 - n as i64).abs() < 500);
    let hz = crate::fx3::detect_pitch(&p.ch[0][20000..22000], 48000.0, 100.0).unwrap_or(0.0);
    assert!((hz - 880.0).abs() < 30.0, "{hz}");
    // locked (resample): faster and higher together
    let v = crate::dialogs::stretch_and_pitch(&d, 0, n, 2.0, 12.0, 0);
    assert!((v.frames() as f64 / n as f64 - 0.5).abs() < 0.05);
}

#[test]
fn phase_curve_flat_passes_audio_and_180_inverts() {
    use crate::dsp::Processor;
    let d = dsp::gen_tone(48000, 1, 0.5, 1000.0, Wave::Sine, -12.0);
    let mut flat = crate::fx3::PhaseCurve::new(vec![(20.0, 0.0), (20000.0, 0.0)], 0, 2048);
    flat.set_rate(48000.0);
    let mut a = vec![d.ch[0].clone()];
    flat.process(&mut a);
    // a delay of one frame, otherwise the same
    let lag = 2048;
    let err: f32 = (6000..20000)
        .map(|i| (a[0][i] - d.ch[0][i - lag]).abs())
        .fold(0.0, f32::max);
    assert!(err < 0.01, "flat curve keeps the sound, err {err}");
    let mut inv = crate::fx3::PhaseCurve::new(vec![(20.0, 180.0), (20000.0, 180.0)], 0, 2048);
    inv.set_rate(48000.0);
    let mut b = vec![d.ch[0].clone()];
    inv.process(&mut b);
    let err: f32 = (6000..20000)
        .map(|i| (b[0][i] + d.ch[0][i - lag]).abs())
        .fold(0.0, f32::max);
    assert!(err < 0.01, "180 degrees turns it over, err {err}");
}

#[test]
fn brush_stamping_matches_point_weights() {
    let b = crate::spectral::SpecShape::Brush {
        pts: (0..200).map(|i| (1.0 + i as f64 * 0.01, 1000.0)).collect(),
        rt: 0.05,
        ro: 0.5,
        opacity: 1.0,
    };
    let times: Vec<f64> = (0..400).map(|i| i as f64 * 0.01).collect();
    let freqs: Vec<f32> = (1..200).map(|i| i as f32 * 50.0).collect();
    let g = b.grid_weights(&times, &freqs);
    for (ti, t) in times.iter().enumerate().step_by(7) {
        for (fi, f) in freqs.iter().enumerate().step_by(5) {
            assert!((g[ti * freqs.len() + fi] - b.weight(*t, *f)).abs() < 1e-5);
        }
    }
}

#[test]
fn wav_metadata_is_written_and_read_back() {
    let dir = std::env::temp_dir().join("audiocraft_meta_test");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("m.wav");
    let d = dsp::gen_tone(8000, 1, 0.1, 440.0, Wave::Sine, -12.0);
    let mut meta = std::collections::BTreeMap::new();
    meta.insert("Title".to_string(), "Hello".to_string());
    meta.insert("Artist".to_string(), "Someone".to_string());
    crate::audio::save_with_meta(&p, &d, 16, false, true, &meta).unwrap();
    let back = crate::audio::read_wav_meta(&p);
    assert_eq!(back.get("Title").map(|s| s.as_str()), Some("Hello"));
    assert_eq!(back.get("Artist").map(|s| s.as_str()), Some("Someone"));
    // the audio still loads
    let l = crate::audio::load(&p).unwrap();
    assert_eq!(l.data.frames(), d.frames());
}

#[test]
fn bundled_ffmpeg_writes_mp3_and_aac_exports() {
    if !cfg!(windows) {
        return; // the Windows build ships FFmpeg; other platforms use a system copy
    }
    let Some(exe) = crate::ffmpeg::bundled() else {
        return;
    };
    assert_eq!(
        audio::ffmpeg(),
        Some(exe),
        "export must prefer the bundled decoder"
    );
    let dir = std::env::temp_dir().join(format!("audiocraft-ffmpeg-export-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let d = dsp::gen_tone(44100, 1, 0.5, 440.0, Wave::Sine, -12.0);
    let mut meta = std::collections::BTreeMap::new();
    meta.insert("Title".to_string(), "Bundled".to_string());
    for name in ["export.mp3", "export.m4a"] {
        let p = dir.join(name);
        audio::save_with_meta(&p, &d, 16, false, false, &meta).expect(name);
        assert!(
            std::fs::metadata(&p)
                .map(|m| m.len() > 1000)
                .unwrap_or(false),
            "{name} is empty"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pitch_analysis_follows_a_tone() {
    let d = dsp::gen_tone(48000, 1, 0.6, 220.0, Wave::Sine, -12.0);
    let (hop, tr) = crate::ui_wave::analyse_pitch(&d);
    assert!(hop > 0.0 && tr.len() > 20);
    let voiced: Vec<f32> = tr.iter().flatten().copied().collect();
    assert!(voiced.len() > tr.len() / 2);
    let med = voiced[voiced.len() / 2];
    assert!((med - 220.0).abs() < 8.0, "{med}");
}

#[test]
fn knobs_point_up_at_zero() {
    let up = std::f32::consts::PI * 1.5;
    assert!((crate::theme::knob_angle(0.0, -60.0, 15.0) - up).abs() < 1e-6);
    assert!((crate::theme::knob_angle(0.0, -100.0, 100.0) - up).abs() < 1e-6);
    assert!(
        crate::theme::knob_angle(-60.0, -60.0, 15.0) < up
            && crate::theme::knob_angle(15.0, -60.0, 15.0) > up
    );
}

#[test]
fn parametric_eq_has_the_nine_standard_bands() {
    let bands = crate::equi::standard_bands();
    assert_eq!(bands.len(), 9);
    assert_eq!(bands[0].kind, dsp::FilterKind::HighPass);
    assert_eq!(bands[8].kind, dsp::FilterKind::LowPass);
    // flat at 0 dB gain everywhere
    let f: Vec<dsp::Biquad> = bands
        .iter()
        .filter(|b| b.on)
        .map(|b| dsp::Biquad::new(b.kind, 48000.0, b.freq, b.q, b.gain_db))
        .collect();
    for hz in [100.0, 1000.0, 10000.0] {
        let m: f32 = f.iter().map(|b| b.response(hz, 48000.0)).product();
        assert!((m - 1.0).abs() < 0.01, "flat at {hz}: {m}");
    }
}

#[test]
fn stretch_and_pitch() {
    let d = dsp::gen_tone(48000, 1, 1.0, 440.0, Wave::Sine, -6.0);
    let n = d.frames();
    let s = dsp::stretch_range(&d, 0, n, 1.5);
    assert!((s.frames() as i64 - 72000).abs() < 4);
    // pitch kept: spectral peak still near 440 Hz
    let spec = dsp::average_spectrum(&s.ch[0], 4800, 60000, 8192);
    let pk = spec
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0 as f32
        * 24000.0
        / 4096.0;
    assert!((pk - 440.0).abs() < 15.0, "stretch peak {pk}");
    let p = dsp::pitch_range(&d, 0, n, 12.0);
    assert_eq!(p.frames(), n);
    let spec = dsp::average_spectrum(&p.ch[0], 4800, 43000, 8192);
    let pk = spec
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0 as f32
        * 24000.0
        / 4096.0;
    assert!((pk - 880.0).abs() < 25.0, "pitch peak {pk}");
}

#[test]
fn clip_envelope_and_split() {
    use crate::session::env_db;
    let env = vec![(0.0, 0.0f32), (1.0, -20.0)];
    assert!((env_db(&env, 0.5) + 10.0).abs() < 1e-4);
    assert_eq!(env_db(&env, 5.0), -20.0);
    let mut s = Session::new("t", 48000, 1);
    s.tracks[0].clips.push(Clip {
        id: 9,
        file_id: 1,
        name: "a".into(),
        start: 0.0,
        offset: 0.0,
        len: 2.0,
        gain_db: 0.0,
        muted: false,
        env: env.clone(),
        fade_in: 0.0,
        fade_out: 0.0,
        pan_env: vec![],
        rack: vec![],
        speed: 1.0,
        fx: Default::default(),
        auto_in: true,
        auto_out: true,
        curves: Default::default(),
    });
    let r = s.split_clip(9, 0.5).unwrap();
    let (t, c) = s.find_clip(r).unwrap();
    let right = &s.tracks[t].clips[c];
    assert!((env_db(&right.env, 0.0) + 10.0).abs() < 1e-3);
    assert!((env_db(&right.env, 0.5) + 20.0).abs() < 1e-3);
}

#[test]
fn file_undo_redo_history() {
    use crate::app::AudioFile;
    let d = dsp::gen_tone(48000, 1, 0.5, 440.0, Wave::Sine, -6.0);
    let mut f = AudioFile::new(1, "t.wav".into(), None, d.clone(), 32, true, "New File");
    let mut e = d.clone();
    dsp::invert(&mut e, 0, 100);
    f.push("Invert", e.clone());
    let cut = e.without(0, 1000);
    f.push("Delete", cut.clone());
    assert_eq!(f.data.frames(), d.frames() - 1000);
    f.goto_history(1);
    assert_eq!(f.data.frames(), d.frames());
    assert!((f.data.ch[0][5] + d.ch[0][5]).abs() < 1e-7);
    f.goto_history(0);
    assert!((f.data.ch[0][5] - d.ch[0][5]).abs() < 1e-7);
    f.goto_history(2);
    assert_eq!(f.data.frames(), cut.frames());
    // a new edit after undo drops the redo branch
    f.goto_history(0);
    f.push("Reverse", d.clone());
    assert_eq!(f.history.len(), 2);
    // out-of-range history jumps are ignored, not panics
    f.goto_history(99);
    assert_eq!(f.hist_pos, 1);
}

#[test]
fn clip_pan_envelope_and_clip_effects_render() {
    use crate::session::ChainCache;
    let rate = 48000;
    let tone = Arc::new(dsp::gen_tone(rate, 2, 1.0, 300.0, Wave::Sine, -6.0));
    let mut s = Session::new("t", rate, 1);
    s.tracks[0].clips.push(Clip {
        id: 7,
        file_id: 1,
        name: "a".into(),
        start: 0.0,
        offset: 0.0,
        len: 1.0,
        gain_db: 0.0,
        muted: false,
        fade_in: 0.0,
        fade_out: 0.0,
        env: vec![],
        pan_env: vec![(0.0, -1.0)],
        rack: vec![],
        speed: 1.0,
        fx: Default::default(),
        auto_in: true,
        auto_out: true,
        curves: Default::default(),
    });
    let t2 = tone.clone();
    let mix = s.snapshot(&move |_| Some(t2.clone()));
    let d = mixdown(&mix, rate, 0.0, 1.0);
    assert!(d.ch[0].iter().any(|v| v.abs() > 0.1), "left should sound");
    assert!(
        d.ch[1].iter().all(|v| v.abs() < 1e-6),
        "right should be silent when panned hard left"
    );
    // a clip effect is applied before the track: -40 dB amplify makes it quiet
    s.tracks[0].clips[0].pan_env.clear();
    s.tracks[0].clips[0].rack = vec![EffectSlot::new(Effect::Amplify { gain_db: -40.0 })];
    let t3 = tone.clone();
    let mix = s.snapshot(&move |_| Some(t3.clone()));
    let mut cache = ChainCache::default();
    cache.sync(&mix, rate as f32);
    assert_eq!(cache.clips.len(), 1);
    let d = mixdown(&mix, rate, 0.0, 1.0);
    assert!(d.peak() < 0.01 && d.peak() > 1e-5, "peak {}", d.peak());
}

#[test]
fn stretched_sesx_clip_keeps_its_speed() {
    let dir = std::env::temp_dir().join("audiocraft_sesx_speed");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("s.sesx");
    let xml = r#"<?xml version="1.0"?><sesx><session sampleRate="48000"><tracks><audioTrack><trackParameters><name>T</name></trackParameters>
      <audioClip fileID="0" name="c" startPoint="48000" endPoint="96000" sourceInPoint="0" sourceOutPoint="96000"/></audioTrack></tracks></session>
      <files><file id="0" absolutePath="C:/nowhere/x.wav"/></files></sesx>"#;
    std::fs::write(&p, xml).unwrap();
    let (s, files) = crate::sesx::import(&p).unwrap();
    assert_eq!(files.len(), 1);
    let c = &s.tracks[0].clips[0];
    assert!((c.start - 1.0).abs() < 1e-9 && (c.len - 1.0).abs() < 1e-9);
    assert!((c.speed - 2.0).abs() < 1e-9, "speed {}", c.speed);
}

#[test]
fn sesx_effect_racks_track_params_and_crossfades_import() {
    let dir = std::env::temp_dir().join("audiocraft_sesx_racks");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("r.sesx");
    let xml = r#"<?xml version="1.0"?><!DOCTYPE sesx><sesx version="1.0"><session sampleRate="48000"><tracks>
      <audioTrack id="10001" index="1"><trackParameters trackHeight="134"><name>Track 1</name></trackParameters>
        <trackAudioParameters audioChannelType="stereo" solo="false">
          <component componentID="Audition.Fader" id="trackFader" name="volume"><parameter index="0" name="volume" parameterValue="0.5"/><parameter index="1" name="static gain" parameterValue="1"/></component>
          <component componentID="Audition.Mute" id="trackMute" name="Mute"><parameter index="0" parameterValue="0"/><parameter index="1" name="mute" parameterValue="1"/></component>
          <component componentID="Audition.StereoPanner" id="trackPan" name="StereoPanner"><parameter index="0" name="Pan" parameterValue="-50"/></component>
          <component componentID="Audition.EQ" id="trackEQ" name="EQ" powered="false"><parameter index="24" name="Master Gain" parameterValue="0.6666667"/></component>
        </trackAudioParameters>
        <effectsRack preFader="false">
          <component id="rackMix" powered="true"><parameter index="0" parameterValue="0.25"/></component>
          <component id="rackInput" powered="true"><parameter index="0" parameterValue="0.5"/></component>
          <component id="rackOutput" powered="true"><parameter index="0" parameterValue="1"/></component>
          <slot powered="true" slotIndex="0"><component componentID="x" name="Parametric Equalizer">
            <parameter index="10" name="EQ Band 2 Gain" parameterValue="0.8"/><parameter index="8" name="EQ Band 2 Center Frequency" parameterValue="0.0085976"/>
            <parameter index="24" name="Master Gain" parameterValue="0.6666667"/></component></slot>
          <slot powered="false" slotIndex="1"><component componentID="y" name="Full Reverb"><parameter index="0" name="Room Size" parameterValue="0.47"/>
            <parameter index="1" name="Output Level (Dry)" parameterValue="0.4"/><parameter index="2" name="Output Level (Reverberation)" parameterValue="0.1"/></component></slot>
          <slot powered="true" slotIndex="2"><component componentID="z" name="Mystery Box"><parameter index="0" name="Amount" parameterValue="0.3"/></component></slot>
        </effectsRack>
        <audioClip endPoint="96000" fileID="0" id="0" name="a" sourceInPoint="0" sourceOutPoint="96000" startPoint="0">
          <effectsRack preFader="true"><slot powered="true" slotIndex="0"><component name="Chorus"><parameter name="Dry Output Level" parameterValue="0.35"/><parameter name="Wet Output Level" parameterValue="0.15"/></component></slot></effectsRack>
          <fadeOut endPoint="96000" startPoint="72000" type="log"/>
        </audioClip>
        <audioClip endPoint="168000" fileID="0" id="1" name="b" sourceInPoint="0" sourceOutPoint="96000" startPoint="72000"/>
      </audioTrack></tracks></session>
      <files><file id="0" absolutePath="C:/nowhere/x.wav"/></files></sesx>"#;
    std::fs::write(&p, xml).unwrap();
    let (s, _) = crate::sesx::import(&p).unwrap();
    let t = &s.tracks[0];
    assert!(
        (t.volume_db + 6.02).abs() < 0.05,
        "fader inside trackAudioParameters: {}",
        t.volume_db
    );
    assert!(t.mute);
    assert!((t.pan + 0.5).abs() < 1e-6);
    assert_eq!(t.rack.len(), 3, "track rack slots");
    assert!(matches!(t.rack[0].effect, Effect::ParametricEq { .. }));
    if let Effect::ParametricEq { bands, master_db } = &t.rack[0].effect {
        assert!(
            bands
                .iter()
                .any(|b| (b.gain_db - 9.0).abs() < 0.01 && (b.freq - 206.3).abs() < 1.0),
            "band 2 mapped"
        );
        assert!(master_db.abs() < 0.01);
    }
    assert!(
        matches!(t.rack[1].effect, Effect::StudioReverb { full: true, .. }) && !t.rack[1].enabled
    );
    assert_eq!(t.rack[1].effect.name(), "Full Reverb");
    assert!(matches!(&t.rack[2].effect, Effect::Imported { name, .. } if name == "Mystery Box"));
    assert!(!t.fx.pre_fader);
    assert!((t.fx.mix - 0.75).abs() < 1e-6);
    assert!((t.fx.in_db + 6.02).abs() < 0.05);
    let a = &t.clips[0];
    assert_eq!(a.rack.len(), 1);
    assert!(matches!(a.rack[0].effect, Effect::Chorus { .. }));
    assert!((a.fade_out - 0.5).abs() < 1e-9 && !a.auto_out);
    // clip b overlaps a by 0.5 s and has no fade of its own: it gets the automatic crossfade
    let b = &t.clips[1];
    assert!(b.auto_in && b.fade_in == 0.0);
    let eff = crate::session::effective_fades(t);
    assert!(
        (eff[1].0 - 0.5).abs() < 1e-9 && eff[1].2,
        "auto crossfade spans the overlap"
    );
    assert!(eff[0].3, "a's fade-out is part of the crossfade");
}

#[test]
fn auto_crossfade_is_balanced_and_drag_overrides_it() {
    let rate = 48000;
    let one = Arc::new(AudioData {
        rate,
        ch: vec![vec![0.5; rate as usize * 2]; 2],
    });
    let mut s = Session::new("x", rate, 1);
    let mk = |id: u64, start: f64| Clip {
        id,
        file_id: 1,
        name: "c".into(),
        start,
        offset: 0.0,
        len: 2.0,
        gain_db: 0.0,
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
    };
    s.tracks[0].clips = vec![mk(1, 0.0), mk(2, 1.0)];
    let eff = crate::session::effective_fades(&s.tracks[0]);
    assert!((eff[0].1 - 1.0).abs() < 1e-9 && (eff[1].0 - 1.0).abs() < 1e-9);
    let o = one.clone();
    let d = mixdown(&s.snapshot(&move |_| Some(o.clone())), rate, 0.0, 3.0);
    // halfway through the crossfade an equal-power fade sums to about +3 dB (0.5 * 2 * 0.707)
    let mid = d.ch[0][(1.5 * rate as f64) as usize];
    assert!((mid - 0.7071).abs() < 0.01, "balanced crossfade, got {mid}");
    // dragging the fade sets the person's own length
    s.tracks[0].clips[1].auto_in = false;
    s.tracks[0].clips[1].fade_in = 0.25;
    let eff = crate::session::effective_fades(&s.tracks[0]);
    assert!((eff[1].0 - 0.25).abs() < 1e-9);
}

#[test]
fn rack_power_mix_and_post_fader() {
    use crate::dsp::{Chain, RackOpts, run_rack};
    let slots = vec![EffectSlot::new(Effect::Amplify { gain_db: -20.0 })];
    let mut ch = Chain::new(&slots, 48000.0);
    let mut dry = vec![];
    let mut buf = vec![vec![1.0f32; 64]; 2];
    run_rack(
        &mut ch,
        &RackOpts {
            power: false,
            ..Default::default()
        },
        &mut buf,
        &mut dry,
    );
    assert!(
        (buf[0][0] - 1.0).abs() < 1e-6,
        "power off bypasses every effect"
    );
    run_rack(
        &mut ch,
        &RackOpts {
            mix: 0.5,
            ..Default::default()
        },
        &mut buf,
        &mut dry,
    );
    assert!(
        (buf[0][0] - 0.55).abs() < 1e-3,
        "half dry, half -20 dB: {}",
        buf[0][0]
    );
    let mut buf = vec![vec![1.0f32; 64]; 2];
    run_rack(
        &mut ch,
        &RackOpts {
            in_db: 6.0,
            out_db: -6.0,
            ..Default::default()
        },
        &mut buf,
        &mut dry,
    );
    assert!((buf[0][0] - 0.1).abs() < 1e-3);
}

#[test]
fn sesx_export_round_trips_effect_racks() {
    let dir = std::env::temp_dir().join("audiocraft_sesx_racks_rt");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("rt.sesx");
    let mut s = Session::new("rt", 48000, 1);
    s.tracks[0].rack = vec![EffectSlot::new(Effect::Delay {
        time_ms: 123.0,
        feedback: 0.2,
        mix: 0.4,
    })];
    s.tracks[0].fx.pre_fader = false;
    s.tracks[0].fx.mix = 0.6;
    s.tracks[0].clips.push(Clip {
        id: 5,
        file_id: 0,
        name: "c".into(),
        start: 0.0,
        offset: 0.0,
        len: 1.0,
        gain_db: 0.0,
        muted: false,
        fade_in: 0.0,
        fade_out: 0.0,
        env: vec![],
        pan_env: vec![],
        rack: vec![EffectSlot::new(Effect::StereoExpander { width: 2.0 })],
        speed: 1.0,
        fx: Default::default(),
        auto_in: true,
        auto_out: true,
        curves: Default::default(),
    });
    crate::sesx::export(&s, &["C:/a.wav".into()], &p).unwrap();
    let (r, _) = crate::sesx::import(&p).unwrap();
    assert_eq!(r.tracks[0].rack, s.tracks[0].rack);
    assert!(!r.tracks[0].fx.pre_fader && (r.tracks[0].fx.mix - 0.6).abs() < 1e-4);
    assert_eq!(r.tracks[0].clips[0].rack, s.tracks[0].clips[0].rack);
}

#[test]
fn new_session_dialog_defaults_are_complete() {
    match crate::dialogs::new_session_dialog(44100) {
        crate::app::Dialog::NewSession {
            name,
            rate,
            tracks,
            folder,
            bits,
            mix,
            ..
        } => {
            assert_eq!(name, "Untitled Session 1");
            assert_eq!(rate, 44100);
            assert_eq!(tracks, 6);
            assert!(folder.ends_with("AudioCraft"));
            assert_eq!(bits, 24);
            assert_eq!(mix, 1);
        }
        other => panic!(
            "New Session command opened {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

#[test]
fn session_paths_sanitize_names() {
    use crate::dialogs::{session_name_stem, session_paths};
    assert_eq!(
        session_name_stem("  My:Session?  "),
        Some("My_Session_".into())
    );
    assert_eq!(session_name_stem("   ..  "), None);
    let (dir, file) = session_paths("C:/sessions", "Podcast").unwrap();
    assert_eq!(dir, std::path::Path::new("C:/sessions").join("Podcast"));
    assert_eq!(file, dir.join("Podcast.sesx"));
    let (dir, file) = session_paths("", "Take").unwrap();
    assert_eq!(dir, std::path::Path::new("Take"));
    assert_eq!(file, std::path::Path::new("Take/Take.sesx"));
}

#[test]
fn track_header_and_timeline_are_visually_separate() {
    let lum = |c: egui::Color32| (c.r() as i32 + c.g() as i32 + c.b() as i32) / 3;
    assert!(
        lum(crate::theme::track_head()) - lum(crate::theme::track_lane_a()) >= 10,
        "header must be lighter than the lane"
    );
    assert!(
        lum(crate::theme::track_divider()) > lum(crate::theme::track_head()),
        "divider must be visible against the header"
    );
    assert_ne!(crate::theme::track_lane_a(), crate::theme::track_lane_b());
}

#[test]
fn mode_buttons_ask_for_a_new_project_only_when_empty() {
    use crate::app::{Dialog, Mode};
    use crate::dialogs::dialog_for_mode;

    // An empty project: both buttons show the reference creation dialogs.
    assert!(matches!(
        dialog_for_mode(Mode::Multitrack, 44100, false, false, 0),
        Some(Dialog::NewSession {
            rate: 44100,
            tracks: 6,
            bits: 24,
            ..
        })
    ));
    assert!(matches!(
        dialog_for_mode(Mode::Waveform, 48000, false, false, 2),
        Some(Dialog::NewFile { name, rate: 48000, channels: 2, bits: 32, .. }) if name == "Untitled 3"
    ));

    // A user-made or opened session/file (even with no current file) just switches.
    assert!(dialog_for_mode(Mode::Multitrack, 48000, false, true, 0).is_none());
    assert!(dialog_for_mode(Mode::Waveform, 48000, false, true, 0).is_none());
    assert!(dialog_for_mode(Mode::Waveform, 48000, true, false, 1).is_none());
}

#[test]
fn opening_the_mixer_makes_a_ready_session() {
    let s = crate::app::mixer_session();
    assert_eq!(s.tracks.len(), 6);
    assert_eq!(s.rate, 48000);
    assert_eq!(s.bits, 24);
    assert!(!s.dirty, "a fresh session must not look unsaved");
}

#[test]
fn level_meter_matches_a_quiet_file_and_can_be_reset() {
    use crate::engine::{Shared, Source};
    let d = Arc::new(dsp::gen_tone(48000, 2, 0.1, 440.0, Wave::Sine, -24.0));
    let mut s = Shared::new();
    s.out_rate = 48000;
    s.source = Source::File(d.clone());
    s.pos = 0.0;
    s.end = d.duration();
    s.playing = true;
    s.render(1024);
    let expected = d.peak();
    assert!(
        (s.meter[0] - expected).abs() < 0.01,
        "meter {} != file peak {}",
        s.meter[0],
        expected
    );
    assert!(
        (audio::db(s.meter[0]) + 24.0).abs() < 0.5,
        "a -24 dB file must read about -24 dB"
    );
    assert!(
        s.meter[0] < 1.0,
        "a quiet file must not light a clip indicator"
    );
    s.reset_meters();
    assert_eq!(s.meter, [0.0; 2]);
    assert!(s.track_meters.is_empty());
}

#[test]
fn levels_meter_matches_a_file_that_does_not_peak() {
    use crate::engine::{Shared, Source};
    let d = Arc::new(dsp::gen_tone(48000, 2, 0.05, 440.0, Wave::Sine, -60.0));
    let mut s = Shared::new();
    s.out_rate = 48000;
    s.source = Source::File(d.clone());
    s.pos = 0.0;
    s.end = d.duration();
    s.playing = true;
    s.render(1024);
    let peak = d.peak();
    assert!(
        peak <= 0.002,
        "controlled probe should not peak: peak={peak}"
    );
    assert!(
        (s.meter[0] - peak).abs() < 0.002,
        "meter {} != file peak {peak}",
        s.meter[0]
    );
    assert!(
        s.meter[0] < 1.0,
        "a file that does not peak must not be shown as peaking"
    );
    assert!(
        audio::db(s.meter[0]) <= -59.0,
        "Levels meter must not read above the file peak: {} dB",
        audio::db(s.meter[0])
    );
}

#[test]
fn loudness_reset_clears_a_held_true_peak() {
    use crate::dsp::Processor;
    let d = dsp::gen_tone(48000, 2, 0.1, 440.0, Wave::Sine, 0.0);
    let mut buf = d.ch.clone();
    let mut m = crate::fx3::LoudnessMeter::new(48000.0);
    m.process(&mut buf);
    assert!(
        crate::fx3::loudness_readings().3 > -6.0,
        "loud tone should show a peak"
    );
    crate::fx3::reset_loudness();
    let (_, _, _, peak) = crate::fx3::loudness_readings();
    assert_eq!(peak, -100.0, "a new file must not keep the old true peak");
}

#[test]
fn master_automation_changes_within_one_block() {
    use crate::dsp::RackOpts;
    use crate::session::{ChainCache, Mix, MixClip, MixTrack, render_mix};
    let d = Arc::new(dsp::gen_tone(48000, 2, 0.2, 1000.0, Wave::Sine, -6.0));
    let clip = MixClip {
        data: d.clone(),
        start: 0.0,
        offset: 0.0,
        len: 0.2,
        gain: 1.0,
        fade_in: 0.0,
        fade_out: 0.0,
        xin: false,
        xout: false,
        env: vec![],
        id: 7,
        pan_env: vec![],
        rack: vec![],
        fx: RackOpts::default(),
        speed: 1.0,
        fin_curve: vec![],
        fout_curve: vec![],
    };
    let mix = Mix {
        tracks: vec![MixTrack {
            clips: vec![clip],
            gain_l: 1.0,
            gain_r: 1.0,
            audible: true,
            rack: vec![],
            fx: RackOpts::default(),
            mono: false,
            vol_env: vec![],
            automation: 0,
            pan_env: vec![],
            vol: 1.0,
            polarity: 1.0,
            extra: Default::default(),
            pre: None,
        }],
        master: 1.0,
        master_env: vec![(0.0, 0.0), (0.1, -60.0)],
        master_automation: 1,
        length: 0.2,
        master_rack: vec![],
        master_fx: RackOpts::default(),
        metronome: None,
        mono_out: false,
    };
    let mut cache = ChainCache::default();
    cache.sync(&mix, 48000.0);
    let mut out = vec![];
    let mut tmp = vec![];
    let mut peaks = vec![];
    render_mix(
        &mix, &mut cache, 48000.0, 0.0, 9600, &mut out, &mut tmp, &mut peaks,
    );
    let pk = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let early = pk(&out[0][..4800]);
    let late = pk(&out[0][4800..]);
    assert!(
        early > late * 100.0,
        "master automation must follow each frame: early {early}, late {late}"
    );
}

fn dc_clip(id: u64, len: f64) -> Clip {
    Clip {
        id,
        file_id: 1,
        name: "dc".into(),
        start: 0.0,
        offset: 0.0,
        len,
        gain_db: 0.0,
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
    }
}

/// Constant 0.5 audio at 48 kHz (shared with the automation tests).
pub(crate) fn dc_file(secs: f64) -> Arc<AudioData> {
    let n = (48000.0 * secs) as usize;
    let mut d = AudioData::new(48000, 1, n);
    for v in d.ch[0].iter_mut() {
        *v = 0.5;
    }
    Arc::new(d)
}

#[test]
fn track_pan_automation_moves_the_sound_between_sides() {
    let data = dc_file(1.0);
    let mut s = Session::new("pan", 48000, 1);
    s.tracks[0].clips.push(dc_clip(5, 1.0));
    s.tracks[0].auto.pan_env = vec![(0.0, -1.0), (1.0, 1.0)];
    let mix = s.snapshot(&|_| Some(data.clone()));
    let out = mixdown(&mix, 48000, 0.0, 1.0);
    let (l0, r0) = (out.ch[0][100], out.ch[1][100]);
    let (l1, r1) = (out.ch[0][47900], out.ch[1][47900]);
    assert!(l0 > 0.4 && r0 < 0.01, "start is left: {l0} {r0}");
    assert!(r1 > 0.4 && l1 < 0.01, "end is right: {l1} {r1}");
    // with automation off the fixed (centre) pan is used
    s.tracks[0].io.automation = 0;
    let out = mixdown(&s.snapshot(&|_| Some(data.clone())), 48000, 0.0, 1.0);
    assert!((out.ch[0][47900] - out.ch[1][47900]).abs() < 1e-4);
}

#[test]
fn fade_points_and_curved_keyframes_are_heard() {
    let data = dc_file(2.0);
    let mut s = Session::new("fade", 48000, 1);
    let mut c = dc_clip(6, 2.0);
    c.fade_in = 1.0;
    c.auto_in = false;
    s.tracks[0].clips.push(c.clone());
    let plain = mixdown(&s.snapshot(&|_| Some(data.clone())), 48000, 0.0, 2.0);
    // a point at 25 % through the fade at 90 % gain: much louder early in the fade
    s.tracks[0].clips[0].curves.fin_pts = vec![(0.25, 0.9)];
    let shaped = mixdown(&s.snapshot(&|_| Some(data.clone())), 48000, 0.0, 2.0);
    let i = 12000;
    assert!(plain.ch[0][i] < 0.15, "{}", plain.ch[0][i]);
    assert!(shaped.ch[0][i] > 0.4, "{}", shaped.ch[0][i]);
    // curved volume keyframes play as curves
    let mut s2 = Session::new("curve", 48000, 1);
    let mut c2 = dc_clip(7, 2.0);
    c2.env = vec![(0.0, 0.0), (2.0, -40.0)];
    c2.curves.env_h = vec![
        crate::curves::Handles {
            on: true,
            out_dt: 1.5,
            out_dv: 0.0,
            ..Default::default()
        },
        Default::default(),
    ];
    s2.tracks[0].clips.push(c2);
    let curved = mixdown(&s2.snapshot(&|_| Some(data.clone())), 48000, 0.0, 2.0);
    // linear would be -10 dB at 0.5 s; the flat handle holds it near 0 dB
    let g = curved.ch[0][24000] / 0.5;
    assert!(g > audio::undb(-6.0), "{g}");
}

#[test]
fn splitting_and_trimming_keep_curves_in_step() {
    let mut s = Session::new("split", 48000, 1);
    let mut c = dc_clip(8, 4.0);
    c.env = vec![(0.0, 0.0), (2.0, -12.0), (4.0, 0.0)];
    c.curves.env_h = vec![
        Default::default(),
        crate::curves::auto_handles(&c.env, 1),
        Default::default(),
    ];
    c.fade_out = 1.0;
    c.curves.fout_pts = vec![(0.5, 0.2)];
    s.tracks[0].clips.push(c);
    let right = s.split_clip(8, 1.0).unwrap();
    for cl in &s.tracks[0].clips {
        assert_eq!(
            cl.env.len(),
            cl.curves.env_h.len(),
            "handles follow the keyframes"
        );
    }
    let r = s.clip_mut(right).unwrap();
    assert!(r.curves.fin_pts.is_empty());
    assert_eq!(
        r.curves.fout_pts.len(),
        1,
        "the fade-out stays with the right part"
    );
}

#[test]
fn spectral_axis_zoom_scroll_and_proportional_move() {
    use crate::spectral::SpecShape;
    use crate::ui_wave::FreqAxis;
    let lin = FreqAxis {
        lo: 0.0,
        hi: 24000.0,
        nyq: 24000.0,
        log: false,
    };
    let (a, b) = lin.zoomed(0.25, 0.5);
    // the frequency at the pointer stays put
    let f = 6000.0;
    assert!((a + 0.25 * (b - a) - f).abs() < 1.0, "{a} {b}");
    let log = FreqAxis {
        lo: 100.0,
        hi: 1600.0,
        nyq: 24000.0,
        log: true,
    };
    assert!((log.frac(400.0) - 0.5).abs() < 1e-4, "two of four octaves");
    let (a, b) = log.scrolled(0.25);
    assert!(
        (b / a - 16.0).abs() < 0.01,
        "scrolling keeps the octaves: {a} {b}"
    );
    let (a, b) = log.scrolled(100.0);
    assert!(
        b <= 24000.0 && (b / a - 16.0).abs() < 0.05,
        "stops at the top: {a} {b}"
    );
    // proportional move on the log scale: the selection keeps its height on screen
    let mut s = SpecShape::Rect {
        t0: 0.0,
        t1: 1.0,
        f0: 200.0,
        f1: 400.0,
    };
    let d = log.frac(800.0) - log.frac(400.0);
    s.translate_with(0.0, &|x| log.freq(log.frac(x) + d), 1.0, 24000.0);
    let (_, _, f0, f1) = s.bounds();
    assert!(
        (f0 - 400.0).abs() < 0.5 && (f1 - 800.0).abs() < 1.0,
        "{f0} {f1}"
    );
    // a move past the top leaves the frequencies where they were
    let mut s2 = SpecShape::Rect {
        t0: 0.0,
        t1: 1.0,
        f0: 10000.0,
        f1: 20000.0,
    };
    s2.translate_with(0.5, &|x| x * 4.0, 1.0, 24000.0);
    assert_eq!(s2.bounds(), (0.5, 1.5, 10000.0, 20000.0));
}

#[test]
fn pitch_scale_names_notes_like_the_original() {
    use crate::ui_wave::{midi_of, note_label};
    assert_eq!(note_label(69), "A4 (440)");
    assert_eq!(note_label(61), "C#/Db4");
    assert_eq!(note_label(33), "A1 (55)");
    assert!((midi_of(880.0) - 81.0).abs() < 1e-4);
}

#[test]
fn spectrogram_shows_a_tone_at_its_frequency_on_either_scale() {
    use crate::ui_wave::{FreqAxis, SpecParams, spectrogram_image};
    let d = dsp::gen_tone(48000, 1, 1.0, 1000.0, Wave::Sine, -6.0);
    for log in [false, true] {
        let axis = FreqAxis {
            lo: if log { 100.0 } else { 0.0 },
            hi: 4000.0,
            nyq: 24000.0,
            log,
        };
        let img = spectrogram_image(
            &d,
            Some(0),
            0.2,
            0.8,
            32,
            200,
            axis,
            SpecParams {
                n: 2048,
                db_range: 100.0,
                window: 0,
                palette: 0,
            },
        );
        // the brightest row in the middle column sits where 1 kHz is on the scale
        let x = 16;
        let bright = (0..200)
            .max_by_key(|y| {
                let c = img.pixels[y * 32 + x];
                c.r() as u32 + c.g() as u32 + c.b() as u32
            })
            .unwrap();
        let want = ((1.0 - axis.frac(1000.0)) * 200.0) as i32;
        assert!(
            (bright as i32 - want).abs() <= 3,
            "log={log}: row {bright} vs {want}"
        );
    }
}

#[test]
fn key_clip_covers_clip_edge_behind_visible_area() {
    use crate::ui_multi::key_clip;
    use egui::{Rect, pos2};
    // The geometry multitrack_view builds: the track area, the track lanes and a clip
    // scrolled so its left edge is behind the visible clip area.
    let tracks_rect = Rect::from_min_max(pos2(0.0, 100.0), pos2(780.0, 700.0));
    let lanes = Rect::from_min_max(pos2(222.0, 100.0), pos2(780.0, 700.0));
    let cr = Rect::from_min_max(pos2(200.0, 100.0), pos2(500.0, 700.0));
    let vis = cr.intersect(lanes).intersect(tracks_rect);
    assert!(
        !vis.expand(1.0).contains(pos2(cr.left(), 400.0)),
        "the edge is outside the visible-only rect"
    );
    // The clip's volume/pan keyframes are interactive in key_clip: the keyframe at the
    // clip's left edge is inside it.
    assert!(
        key_clip(cr, tracks_rect)
            .expand(1.0)
            .contains(pos2(cr.left(), 400.0))
    );
    // A fully visible clip: the keyframe clip covers the clip, as before.
    let cr2 = Rect::from_min_max(pos2(222.0, 100.0), pos2(500.0, 700.0));
    assert!(key_clip(cr2, tracks_rect).contains(pos2(cr2.left(), 400.0)));
}

#[test]
fn clip_edge_zones_reach_visible_boundary_when_scrolled() {
    use crate::ui_multi::{on_left_edge, on_right_edge};
    use egui::{Rect, pos2};
    let lanes = Rect::from_min_max(pos2(222.0, 100.0), pos2(780.0, 700.0));
    // A clip scrolled so its left edge is 20px behind the visible area and its right edge
    // past the visible right edge.
    let r = Rect::from_min_max(pos2(202.0, 100.0), pos2(790.0, 700.0));
    assert!(on_left_edge(r, lanes, pos2(222.0, 400.0))); // at the visible boundary: trims
    assert!(on_left_edge(r, lanes, pos2(228.0, 400.0))); // a little further in: trims
    assert!(!on_left_edge(r, lanes, pos2(230.0, 400.0))); // back inside the clip body: moves
    assert!(on_right_edge(r, lanes, pos2(780.0, 400.0))); // at the visible boundary: trims
    assert!(!on_right_edge(r, lanes, pos2(770.0, 400.0))); // 8px inside: moves
    // A fully visible clip keeps the usual zones.
    let r2 = Rect::from_min_max(pos2(222.0, 100.0), pos2(500.0, 700.0));
    assert!(on_left_edge(r2, lanes, pos2(222.0, 400.0)));
    assert!(on_left_edge(r2, lanes, pos2(226.0, 400.0))); // within the usual 7px
    assert!(!on_left_edge(r2, lanes, pos2(233.0, 400.0))); // beyond the usual 7px
    assert!(on_right_edge(r2, lanes, pos2(500.0, 400.0)));
    assert!(!on_right_edge(r2, lanes, pos2(480.0, 400.0))); // 20px from the edge
}

#[test]
fn clip_and_gap_selection_follow_the_row_clicks() {
    let mut s = Session::new("sel", 48000, 2);
    let mut ids: Vec<u64> = vec![];
    // five clips on track 0, each one second long with a gap after it
    for i in 0..5 {
        let id = s.id();
        let mut c = dc_clip(id, 0.6);
        c.start = i as f64;
        s.tracks[0].clips.push(c);
        ids.push(id);
    }
    // a plain click selects one clip and drops the selected empty spans
    s.select_clips(ids[2], false);
    assert_eq!(s.selected_clips, vec![ids[2]]);
    assert_eq!(s.selected_clip, Some(ids[2]));
    // Ctrl+click toggles clips one by one: each grab joins the set, and picking a
    // selected clip again drops it
    s.select_clips(ids[0], true);
    assert_eq!(s.selected_clips, vec![ids[2], ids[0]]);
    assert_eq!(s.selected_clip, Some(ids[0]));
    s.select_clips(ids[2], true);
    assert_eq!(s.selected_clips, vec![ids[0]]);
    // Shift+click selects every clip in the clicked clip's row (what is picked stays)
    s.select_row_clips(ids[4]);
    assert_eq!(s.selected_clips.len(), 5, "the whole row is selected");
    assert!(ids.iter().all(|id| s.selected_clips.contains(id)));
    assert_eq!(
        s.selected_clip,
        Some(ids[4]),
        "the clip clicked is the active one"
    );
    // Shift+click on an already-picked clip drops that clip (the person's rule: it
    // un-picks what is already selected) and the active role hands over
    s.select_row_clips(ids[4]);
    assert_eq!(s.selected_clips.len(), 4, "the picked clip leaves the set");
    assert!(!s.selected_clips.contains(&ids[4]));
    assert_eq!(s.selected_clip, Some(ids[3]), "the active role hands over");
    // a clip outside the set brings its row back in
    s.select_row_clips(ids[4]);
    assert_eq!(s.selected_clips.len(), 5);
    assert_eq!(s.selected_clip, Some(ids[4]));

    // the empty spans of track 0: between each pair of clips, and the open end
    let gaps = s.gaps_on(0);
    assert_eq!(
        gaps,
        vec![
            (0.6, 1.0),
            (1.6, 2.0),
            (2.6, 3.0),
            (3.6, 4.0),
            (4.6, f64::INFINITY)
        ]
    );
    // a track without clips is one span from 0 on
    assert_eq!(s.gaps_on(1), vec![(0.0, f64::INFINITY)]);
    // Shift+click on empty space selects every span of that row
    s.select_row_gaps(0, 1.8);
    assert_eq!(s.selected_gaps.len(), 5);
    assert_eq!(s.selected_gaps[0].a, 0.6);
    assert!(s.selected_gaps.iter().all(|g| g.trk == s.tracks[0].id));
    // Shift+click on an already-picked span drops that one span
    s.select_row_gaps(0, 1.8);
    assert_eq!(s.selected_gaps.len(), 4, "the span clicked leaves the set");
    assert!(!s.selected_gaps.iter().any(|g| g.a == 1.6));
    s.select_row_gaps(0, 1.8);
    assert_eq!(
        s.selected_gaps.len(),
        5,
        "an unpicked span brings the row back"
    );
    // Ctrl+click toggles the one span clicked, in and out
    s.toggle_gap(0, 1.8);
    assert_eq!(s.selected_gaps.len(), 4, "the span (1.6, 2.0) left the set");
    assert!(!s.selected_gaps.iter().any(|g| g.a == 1.6));
    s.toggle_gap(0, 1.8);
    assert_eq!(s.selected_gaps.len(), 5, "the same click brings it back");
    // the open end belongs to the last span, wherever it is clicked
    s.toggle_gap(0, 99.0);
    assert_eq!(s.selected_gaps.len(), 4, "the open end toggles too");
    // clicking empty ground clears clips and spans together
    s.clear_selected_clips();
    assert!(s.selected_clips.is_empty() && s.selected_clip.is_none() && s.selected_gaps.is_empty());
    // a plain click on a clip also drops the spans; a Ctrl+click keeps them
    s.select_row_gaps(0, 1.8);
    s.select_clips(ids[1], false);
    assert!(s.selected_gaps.is_empty(), "a plain click resets the spans");
    s.select_row_gaps(1, 0.5);
    s.select_clips(ids[1], true);
    assert_eq!(
        s.selected_gaps.len(),
        1,
        "Ctrl+click leaves the spans alone"
    );

    // overlapping clips leave no span between them
    let mut s2 = Session::new("ov", 48000, 1);
    let mut a = dc_clip(s2.id(), 2.0);
    a.start = 0.0;
    let mut b = dc_clip(s2.id(), 2.0);
    b.start = 1.0;
    s2.tracks[0].clips = vec![a, b];
    assert_eq!(s2.gaps_on(0), vec![(3.0, f64::INFINITY)]);
}

/// The person's delete rules: Delete / Backspace removes every picked clip and leaves
/// the space it occupied; Shift+Delete / Shift+Backspace ripples, closing the gap so the
/// later clips of the track pull left; and picked empty space closes up whichever key
/// pressed it, because empty ground has no content to remove.
#[test]
fn delete_plain_and_ripple_and_picked_space() {
    use crate::filecmds::{delete_clips_plain, ripple_delete_clips, ripple_delete_gaps};
    use crate::session::GapSel;

    // three clips on one track: 0..1, 2..3, 4..5
    let row = |s: &mut Session| -> Vec<u64> {
        let mut ids = vec![];
        for st in [0.0, 2.0, 4.0] {
            let id = s.id();
            let mut c = dc_clip(id, 1.0);
            c.start = st;
            s.tracks[0].clips.push(c);
            ids.push(id);
        }
        ids
    };
    let near = |a: f64, b: f64| (a - b).abs() < 1e-9;

    // plain delete: the picked clips go, the space they occupied stays
    let mut s = Session::new("plain", 48000, 1);
    let ids = row(&mut s);
    assert_eq!(delete_clips_plain(&mut s, &[ids[0], ids[2]]), 2);
    assert_eq!(s.tracks[0].clips.len(), 1);
    assert!(
        near(s.tracks[0].clips[0].start, 2.0),
        "the middle clip did not move: the space stays"
    );
    assert!(s.selected_clips.is_empty() && s.selected_clip.is_none() && s.selected_gaps.is_empty());

    // ripple delete: the picked clip goes and the later clips of the track pull left
    let mut s = Session::new("rip", 48000, 1);
    let ids = row(&mut s);
    assert_eq!(ripple_delete_clips(&mut s, &[ids[1]]), 1);
    assert_eq!(s.tracks[0].clips.len(), 2);
    assert!(near(s.tracks[0].clips[0].start, 0.0));
    assert!(
        near(s.tracks[0].clips[1].start, 3.0),
        "the last clip pulled left into the closed gap"
    );

    // picked empty space closes: the span 1..2 between the first two clips
    let mut s = Session::new("gap", 48000, 1);
    row(&mut s);
    let trk = s.tracks[0].id;
    assert_eq!(
        ripple_delete_gaps(
            &mut s,
            &[GapSel {
                trk,
                a: 1.0,
                b: 2.0
            }]
        ),
        1
    );
    assert!(near(s.tracks[0].clips[0].start, 0.0));
    assert!(
        near(s.tracks[0].clips[1].start, 1.0),
        "the middle clip pulled next to the first"
    );
    assert!(
        near(s.tracks[0].clips[2].start, 3.0),
        "and the last clip with it"
    );
    assert!(s.selected_gaps.is_empty(), "the span selection is spent");

    // the open tail span has no end to pull to: nothing happens
    let mut s = Session::new("tail", 48000, 1);
    row(&mut s);
    let tail = GapSel {
        trk: s.tracks[0].id,
        a: 5.0,
        b: f64::INFINITY,
    };
    assert_eq!(ripple_delete_gaps(&mut s, &[tail]), 0);
    assert_eq!(s.tracks[0].clips.len(), 3, "the clips stay");
    assert!(!s.dirty, "nothing changed, so the session is not touched");

    // two picked spans on one track: both close, each later clip pulls left by both
    let mut s = Session::new("two", 48000, 1);
    row(&mut s);
    let trk = s.tracks[0].id;
    assert_eq!(
        ripple_delete_gaps(
            &mut s,
            &[
                GapSel {
                    trk,
                    a: 1.0,
                    b: 2.0
                },
                GapSel {
                    trk,
                    a: 3.0,
                    b: 4.0
                }
            ]
        ),
        2
    );
    assert!(
        near(s.tracks[0].clips[1].start, 1.0),
        "the first gap closed first"
    );
    assert!(
        near(s.tracks[0].clips[2].start, 2.0),
        "then the second: the last clip sits right after the middle one"
    );

    // a clip that moved into a picked span is cut by it, like Ripple Delete in time
    let mut s = Session::new("moved", 48000, 1);
    let ids = row(&mut s);
    if let Some(c) = s.clip_mut(ids[1]) {
        c.start = 1.5;
    }
    let span = GapSel {
        trk: s.tracks[0].id,
        a: 1.0,
        b: 2.0,
    };
    assert_eq!(ripple_delete_gaps(&mut s, &[span]), 1);
    assert_eq!(
        s.tracks[0].clips.len(),
        3,
        "the straddling clip is cut, not lost"
    );
    let m = &s.tracks[0].clips[1];
    assert!(near(m.start, 1.0), "the part inside the span went");
    assert!(near(m.len, 0.5), "the part after it stayed and pulled left");
    assert!(near(s.tracks[0].clips[2].start, 3.0), "the last clip too");
}

#[test]
fn snap_dir8_locks_the_eight_main_directions() {
    use crate::keys::snap_dir8;
    use egui::{Pos2, pos2};
    let o = pos2(100.0, 100.0);
    let near = |a: Pos2, b: Pos2| (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3;
    // straight up and straight right stay where they are
    assert!(near(snap_dir8(o, pos2(100.0, 40.0)), pos2(100.0, 40.0)));
    assert!(near(snap_dir8(o, pos2(160.0, 100.0)), pos2(160.0, 100.0)));
    // a nearly-upward drag (11° off the vertical) moves straight up, same distance
    assert!(near(snap_dir8(o, pos2(112.0, 40.0)), pos2(100.0, 40.0)));
    // a slightly-downward, mostly-right drag keeps only its rightward part
    assert!(near(snap_dir8(o, pos2(160.0, 104.0)), pos2(160.0, 100.0)));
    // the exact diagonal (45°) is already one of the eight
    assert!(near(snap_dir8(o, pos2(160.0, 40.0)), pos2(160.0, 40.0)));
    // 50° off the horizontal snaps to the 45° diagonal, keeping the distance travelled
    // (40, -48) has angle -50.2°: nearest is -45°, and the projection lands at (44, -44)
    let p = snap_dir8(o, pos2(140.0, 52.0));
    assert!(
        near(p, pos2(144.0, 56.0)),
        "the drag snaps onto the 45° line at its own length: {p:?}"
    );
    // left and down: the diagonal towards the lower left
    let p = snap_dir8(o, pos2(80.0, 130.0));
    assert!(near(p, pos2(75.0, 125.0)), "{p:?}");
    // a tiny movement still locks to the nearest direction: (0.2, -0.1) is 27° from
    // horizontal, closer to the 45° diagonal, and lands on it at its own length
    assert!(near(snap_dir8(o, pos2(100.2, 99.9)), pos2(100.15, 99.85)));
    // no movement at all: the position is handed back untouched
    assert_eq!(snap_dir8(o, o), o);
}

#[test]
fn spectral_selection_moves_proportionally_on_both_scales() {
    use crate::spectral::SpecShape;
    // an octave (1-2 kHz) dragged from 1 kHz to 4 kHz becomes 4-8 kHz: still an octave, so on the
    // linear scale it is four times as tall and on the log scale it is the same height
    let mut s = SpecShape::Rect {
        t0: 0.0,
        t1: 1.0,
        f0: 1000.0,
        f1: 2000.0,
    };
    let k = s.move_proportional(0.25, 4.0, 1.0, 24000.0);
    assert!((k - 4.0).abs() < 1e-6);
    let (t0, _, f0, f1) = s.bounds();
    assert!(
        (t0 - 0.25).abs() < 1e-9 && (f0 - 4000.0).abs() < 0.01 && (f1 - 8000.0).abs() < 0.01,
        "{f0} {f1}"
    );
    // dragging far past the top stops at the top with the shape kept (no drift, no squash)
    let k = s.move_proportional(0.0, 100.0, 1.0, 24000.0);
    let (_, _, f0, f1) = s.bounds();
    assert!(
        (f1 - 24000.0).abs() < 0.5 && (f1 / f0 - 2.0).abs() < 1e-3,
        "{f0} {f1} k={k}"
    );
    // and back down past the bottom stops at 1 Hz
    s.move_proportional(0.0, 1e-9, 1.0, 24000.0);
    let (_, _, f0, f1) = s.bounds();
    assert!(
        (f0 - 1.0).abs() < 1e-3 && (f1 / f0 - 2.0).abs() < 1e-3,
        "{f0} {f1}"
    );
    // a nonsense factor moves nothing
    assert_eq!(s.move_proportional(0.0, f32::NAN, 1.0, 24000.0), 1.0);
}

#[test]
fn rack_effect_drag_moves_like_a_track() {
    use crate::rack::{drop_index, move_slot};
    use egui::{Rect, pos2, vec2};
    let rows: Vec<Rect> = (0..4)
        .map(|i| Rect::from_min_size(pos2(0.0, i as f32 * 23.0), vec2(200.0, 23.0)))
        .collect();
    assert_eq!(drop_index(&rows, 5.0), 0, "above the first row's middle");
    assert_eq!(drop_index(&rows, 30.0), 1);
    assert_eq!(drop_index(&rows, 500.0), 4, "below the last row: after it");
    let mut v = vec!["a", "b", "c", "d"];
    // the first effect dropped after the third
    assert_eq!(move_slot(&mut v, 0, 3), Some(2));
    assert_eq!(v, ["b", "c", "a", "d"]);
    // dropped on its own gap (above or below itself): nothing moves
    assert_eq!(move_slot(&mut v, 2, 2), None);
    assert_eq!(move_slot(&mut v, 2, 3), None);
    // the last one to the top
    assert_eq!(move_slot(&mut v, 3, 0), Some(0));
    assert_eq!(v, ["d", "b", "c", "a"]);
    // out of range never panics
    assert_eq!(move_slot(&mut v, 9, 0), None);
    assert_eq!(move_slot(&mut v, 0, 9), None);
}

#[test]
fn rack_effect_arrow_is_larger_and_outside_the_drag_region() {
    use crate::rack::effect_row_drag_rect;
    use egui::{Rect, pos2, vec2};
    let row = Rect::from_min_size(pos2(10.0, 20.0), vec2(500.0, 23.0));
    let drag = effect_row_drag_rect(row);
    let arrow_centre = pos2(row.right() - 16.0, row.center().y);
    assert!(
        drag.width() < row.width() - 30.0,
        "the enlarged arrow gets its own area"
    );
    assert!(
        !drag.contains(arrow_centre),
        "the arrow must never start an effect drag"
    );
    assert!(drag.contains(pos2(drag.right() - 1.0, row.center().y)));
    // Even a pathologically narrow row cannot produce an inverted interaction rectangle.
    let tiny = Rect::from_min_size(pos2(4.0, 8.0), vec2(12.0, 23.0));
    let tiny_drag = effect_row_drag_rect(tiny);
    assert!(tiny_drag.left() <= tiny_drag.right() && tiny_drag.top() <= tiny_drag.bottom());
}

#[test]
fn spectral_selection_keeps_its_hz_range_when_moved() {
    use crate::spectral::SpecShape;
    use crate::ui_wave::FreqAxis;
    // 60-1000 Hz is 940 Hz wide; moved up to 18 kHz it is still 940 Hz wide, so on the log scale
    // it takes much less of the screen than it did down low
    let mut s = SpecShape::Rect {
        t0: 0.0,
        t1: 1.0,
        f0: 60.0,
        f1: 1000.0,
    };
    s.translate(0.0, 17940.0);
    let (_, _, f0, f1) = s.bounds();
    assert!(
        (f0 - 18000.0).abs() < 0.01 && (f1 - 18940.0).abs() < 0.01,
        "{f0} {f1}"
    );
    let log = FreqAxis {
        lo: 20.0,
        hi: 24000.0,
        nyq: 24000.0,
        log: true,
    };
    let low = log.frac(1000.0) - log.frac(60.0);
    let high = log.frac(18940.0) - log.frac(18000.0);
    assert!(high < low / 2.0, "thinner higher up: {high} vs {low}");
    // the default preference keeps the Hz range
    assert!(!crate::prefs::Prefs::default().spec_move_keep_octaves);
}

#[test]
fn spectral_display_is_logarithmic_by_default_so_a_moved_selection_looks_thinner_up_high() {
    use crate::ui_wave::FreqAxis;
    let p = crate::prefs::Prefs::default();
    assert!(p.spec_log_scale && !p.spec_move_keep_octaves);
    let axis = FreqAxis {
        lo: 0.0,
        hi: 24000.0,
        nyq: 24000.0,
        log: p.spec_log_scale,
    };
    // 940 Hz down low vs the same 940 Hz up at 18-19 kHz: less than half the height on screen
    let low = axis.frac(1000.0) - axis.frac(60.0);
    let high = axis.frac(18940.0) - axis.frac(18000.0);
    assert!(high < low / 2.0, "{high} vs {low}");
}

#[test]
fn files_panel_names_shorten_in_the_middle() {
    use crate::panels::middle_ellipsis_by as cut;
    let w = |s: &str| s.chars().count() as f32 * 6.0;
    let long = "How_To_Destroy_Angels_-_How_Long 48000 1.wav *";
    assert_eq!(cut("Beat.wav", 400.0, w), "Beat.wav");
    let s = cut(long, 120.0, w);
    assert!(s.contains("...") && w(&s) <= 120.0, "{s}");
    assert!(s.starts_with("How_"), "{s}");
    assert!(s.ends_with(".wav *"), "keeps the unsaved mark: {s}");
    // never panics on tiny or zero widths
    assert_eq!(cut(long, 1.0, w), "...");
    assert_eq!(cut("", 0.0, w), "");
}

/// Diagnostic (run by hand): mixes a real session and prints its peaks, to tell real overs
/// from meter mistakes. `AUDIOCRAFT_PROBE_SESX=path cargo test probe_session_peaks -- --ignored --nocapture`
#[test]
#[ignore]
fn probe_session_peaks() {
    let Ok(p) = std::env::var("AUDIOCRAFT_PROBE_SESX") else {
        return;
    };
    let (s, files) = crate::sesx::import(std::path::Path::new(&p)).unwrap();
    let data: Vec<Option<Arc<AudioData>>> = files
        .iter()
        .map(|f| audio::load(f).ok().map(|l| Arc::new(l.data)))
        .collect();
    for (f, d) in files.iter().zip(&data) {
        if let Some(d) = d {
            println!("file {:>7.2} dBFS  {}", audio::db(d.peak()), f.display());
        }
    }
    let lookup = |id: u64| data.get(id as usize).cloned().flatten();
    let len = s
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .map(|c| c.end())
        .fold(0.0, f64::max);
    let mix = s.snapshot(&lookup);
    let out = crate::session::mixdown(&mix, s.rate, 0.0, len);
    let over = out
        .ch
        .iter()
        .flat_map(|c| c.iter())
        .filter(|v| v.abs() >= 1.0)
        .count();
    println!(
        "MIX peak {:.2} dBFS, {} samples at/over 0 dBFS",
        audio::db(out.peak()),
        over
    );
    for ti in 0..s.tracks.len() {
        let mut solo = s.clone();
        for (k, t) in solo.tracks.iter_mut().enumerate() {
            t.solo = k == ti;
        }
        let m = solo.snapshot(&lookup);
        let o = crate::session::mixdown(&m, s.rate, 0.0, len);
        println!(
            "  {:<10} peak {:>7.2} dBFS (mute {})",
            s.tracks[ti].name,
            audio::db(o.peak()),
            s.tracks[ti].mute
        );
    }
}

#[test]
fn clip_light_needs_a_real_over_not_a_full_scale_sample() {
    use crate::theme::is_over;
    // a 16-bit file's most negative sample reads exactly -1.0: full scale, not clipping
    let mut d = AudioData::new(48000, 1, 4800);
    d.ch[0][100] = -32768.0 / 32768.0;
    d.ch[0][200] = 32767.0 / 32768.0;
    let mut s = crate::engine::Shared::new();
    s.out_rate = 48000;
    let d = Arc::new(d);
    s.source = crate::engine::Source::File(d.clone());
    s.end = d.duration();
    s.playing = true;
    s.render(1024);
    assert!(
        (s.meter[0] - 1.0).abs() < 1e-6,
        "the meter still shows the full-scale level"
    );
    assert!(
        !is_over(s.meter[0]),
        "touching 0 dBFS must not light the clip indicator"
    );
    assert!(!is_over(0.5) && !is_over(1.0) && !is_over(f32::NAN));
    assert!(is_over(1.05), "a mix that goes past 0 dBFS lights it");
}

#[test]
fn saved_session_does_not_ask_to_save_again() {
    let mut s = Session::new("S", 48000, 2);
    s.touch();
    assert!(s.unsaved(), "a never-saved, changed session asks");
    s.mark_saved();
    assert!(!s.unsaved());
    // a control that only "touches" the session (no real change) must not bring the prompt back
    s.touch();
    s.view = (3.0, 9.0);
    s.selected_track = 1;
    assert!(s.dirty && !s.unsaved(), "nothing that is saved changed");
    // a real change does
    s.tracks[0].volume_db = -3.0;
    s.touch();
    assert!(s.unsaved());
    // and putting it back (like undo) clears it again
    s.tracks[0].volume_db = 0.0;
    assert!(!s.unsaved());
}

#[test]
fn recordings_go_to_their_folder_without_overwriting() {
    let dir = std::env::temp_dir().join(format!("audiocraft_takes_{}", std::process::id()));
    // a reused process id must not meet an earlier run's takes
    let _ = std::fs::remove_dir_all(&dir);
    let a = crate::app::unique_take_path(&dir, "Track 1_Recorded 001").unwrap();
    assert!(dir.is_dir(), "the folder is created");
    assert_eq!(a.file_name().unwrap(), "Track 1_Recorded 001.wav");
    std::fs::write(&a, b"x").unwrap();
    let b = crate::app::unique_take_path(&dir, "Track 1_Recorded 001").unwrap();
    assert_eq!(
        b.file_name().unwrap(),
        "Track 1_Recorded 001 2.wav",
        "an earlier take is never overwritten"
    );
    let c = crate::app::unique_take_path(&dir, "a/b:c?").unwrap();
    assert_eq!(c.file_name().unwrap(), "a_b_c_.wav");
    let mut p = crate::prefs::Prefs::default();
    assert!(p.recordings_dir().ends_with("Recordings"));
    p.recordings_folder = dir.display().to_string();
    assert_eq!(p.recordings_dir(), dir);
}

/// The person's report: listening, stopping, then moving the playhead (even to an empty spot)
/// still played the previous sound. Effect tails (echo, reverb) held from before the stop must
/// be gone once the playhead sounds somewhere else.
#[test]
fn stopped_and_moved_playhead_does_not_replay_old_effect_tails() {
    use crate::engine::{Shared, Source};
    let run = |flush: bool| -> f32 {
        let mut d = dsp::gen_tone(48000, 2, 2.0, 440.0, Wave::Sine, -6.0);
        for c in d.ch.iter_mut() {
            for v in c[24000..].iter_mut() {
                *v = 0.0; // sound only in the first half second
            }
        }
        let d = Arc::new(d);
        let echo = Effect::all_defaults()
            .into_iter()
            .find(|e| e.name() == "Delay" || e.name() == "Echo")
            .expect("an echo effect");
        let mut s = Shared::new();
        s.out_rate = 48000;
        s.source = Source::File(d.clone());
        s.set_rack(&[EffectSlot::new(echo)], 0.0, 0.0, 1.0, true);
        s.pos = 0.0;
        s.end = d.duration();
        s.playing = true;
        for _ in 0..20 {
            s.render(1024); // play into the tone
        }
        s.playing = false; // stop
        if flush {
            s.flush_effects();
        }
        // move the playhead to the silent part and let it sound (scrub)
        s.pos = 1.6;
        s.scrub = Some(1.6);
        s.reset_meters();
        let mut pk = 0.0f32;
        for _ in 0..4 {
            s.render(1024);
            pk = pk.max(s.meter[0]);
        }
        pk
    };
    assert!(
        run(false) > 0.01,
        "control: the held echo would sound without clearing"
    );
    assert!(
        run(true) < 1e-4,
        "after stopping and moving, nothing from before may play"
    );
}

#[test]
fn fade_handle_up_down_bends_the_curve_and_playback_follows() {
    use crate::session::{fade_at, shape_table, shaped_fade};
    use crate::ui_multi::{clip_gain_drag, fade_shape_drag};
    // plain, up (louder sooner), down (quieter longer); ends fixed at 0 and 1
    let mid = |s: f32| shaped_fade(0.5, false, s);
    assert!((mid(0.0) - 0.5).abs() < 1e-6);
    assert!(
        mid(1.0) > 0.7 && mid(-1.0) < 0.3,
        "up {} down {}",
        mid(1.0),
        mid(-1.0)
    );
    for s in [-1.0f32, -0.3, 0.0, 0.4, 1.0, f32::NAN] {
        assert!(
            shaped_fade(0.0, true, s).abs() < 1e-6
                && (shaped_fade(1.0, true, s) - 1.0).abs() < 1e-6
        );
    }
    assert!(
        shape_table(false, 0.0).is_empty(),
        "plain curve needs no table"
    );
    let tb = shape_table(false, 0.8);
    assert!((fade_at(&tb, 0.5, false) - shaped_fade(0.5, false, 0.8)).abs() < 0.01);
    // dragging: 80 px up is fully bent up, it stops at the ends, and nonsense can't get in
    assert!((fade_shape_drag(0.0, 200.0, 120.0) - 1.0).abs() < 1e-6);
    assert!((fade_shape_drag(0.0, 200.0, 400.0) + 1.0).abs() < 1e-6);
    assert_eq!(fade_shape_drag(0.0, f32::NAN, 1.0), 0.0);
    // the volume icon: up is louder, 0.1 dB a pixel, kept within -96..+24
    assert!((clip_gain_drag(0.0, 100.0, 70.0) - 3.0).abs() < 1e-4);
    assert!((clip_gain_drag(-2.0, 100.0, 120.0) + 4.0).abs() < 1e-4);
    assert_eq!(clip_gain_drag(20.0, 100.0, -1000.0), 24.0);
    // a session clip with a bent fade-in plays the bent curve
    let d = Arc::new(dsp::gen_tone(48000, 1, 2.0, 100.0, Wave::Square, -6.0));
    let mut s = Session::new("S", 48000, 1);
    s.tracks[0].clips.push(Clip {
        id: 1,
        file_id: 0,
        name: "c".into(),
        start: 0.0,
        offset: 0.0,
        len: 2.0,
        gain_db: 0.0,
        muted: false,
        fade_in: 1.0,
        fade_out: 0.0,
        env: vec![],
        pan_env: vec![],
        rack: vec![],
        speed: 1.0,
        fx: Default::default(),
        auto_in: false,
        auto_out: true,
        curves: Default::default(),
    });
    let level_mid = |s: &Session| {
        let m = s.snapshot(&|_| Some(d.clone()));
        let out = mixdown(&m, 48000, 0.49, 0.51);
        out.ch[0].iter().fold(0.0f32, |a, v| a.max(v.abs()))
    };
    let plain = level_mid(&s);
    s.tracks[0].clips[0].curves.fin_shape = 1.0;
    let bent = level_mid(&s);
    assert!(
        bent > plain * 1.3,
        "bent up is louder mid-fade: {bent} vs {plain}"
    );
}

#[test]
fn sesx_log_fades_open_bent_and_save_back_as_log() {
    let dir = std::env::temp_dir().join("audiocraft_sesx");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("log_fades.sesx");
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<sesx version="1.9"><session sampleRate="48000"><tracks>
<audioTrack id="10001" index="1"><trackParameters><name>Track 1</name></trackParameters>
<audioClip fileID="0" id="0" name="A" startPoint="0" endPoint="96000" sourceInPoint="0" sourceOutPoint="96000">
  <fadeIn crossFadeLinkType="linkedAsymmetric" endPoint="0" shape="0" startPoint="0" type="cosine"/>
  <fadeOut crossFadeLinkType="linkedAsymmetric" endPoint="96000" shape="18" startPoint="72000" type="log"/>
</audioClip>
</audioTrack></tracks></session>
<files><file absolutePath="C:/x/a.wav" id="0" relativePath="a.wav"/></files></sesx>"#;
    std::fs::write(&p, xml).unwrap();
    let (s, _) = crate::sesx::import(&p).unwrap();
    let c = &s.tracks[0].clips[0];
    assert!(
        c.curves.fout_shape > 0.1 && c.curves.fin_shape == 0.0,
        "{:?}",
        c.curves
    );
    assert!((c.fade_out - 0.5).abs() < 1e-6);
    let out = dir.join("log_fades_out.sesx");
    crate::sesx::export(&s, &["C:/x/a.wav".into()], &out).unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(
        text.contains("type=\"log\""),
        "the bent fade is saved as a log fade"
    );
    let (back, _) = crate::sesx::import(&out).unwrap();
    assert!((back.tracks[0].clips[0].curves.fout_shape - c.curves.fout_shape).abs() < 1e-6);
}

#[test]
fn plugin_folder_changes_are_worked_out_without_the_host() {
    use crate::plugins::folder_changes;
    use std::path::PathBuf;
    let a = PathBuf::from("/plug/a");
    let b = PathBuf::from("/plug/b");
    let c = PathBuf::from("/plug/c");
    assert_eq!(
        folder_changes(&[], &[a.clone(), a.clone()]),
        (vec![a.clone()], vec![]),
        "duplicates added once"
    );
    assert_eq!(
        folder_changes(&[a.clone(), b.clone()], &[a.clone(), b.clone()]),
        (vec![], vec![]),
        "unchanged: nothing to do"
    );
    assert_eq!(
        folder_changes(&[a.clone(), b.clone()], &[b.clone(), c.clone()]),
        (vec![c.clone()], vec![a.clone()])
    );
    // in tests hosting is off: setting folders must never load anything, and must not panic
    crate::plugins::set_scan_folds(&[a.clone()]);
    crate::plugins::set_scan_folds(&[]);
}

/// Strength of one exact frequency in the settled second half of a sound.
fn tone_strength(d: &AudioData, hz: f32) -> f64 {
    let rate = d.rate as f64;
    let start = d.frames() / 4;
    let end = d.frames() / 2 + start;
    let mut re = 0.0f64;
    let mut im = 0.0f64;
    for i in start..end {
        let a = std::f64::consts::TAU * hz as f64 * i as f64 / rate;
        let v = d.ch.iter().map(|c| c[i] as f64).sum::<f64>() / d.channels().max(1) as f64;
        re += v * a.cos();
        im += v * a.sin();
    }
    re * re + im * im
}

#[test]
fn frequency_band_splitter_separates_low_and_high_tones() {
    let rate = 48000;
    let n = rate as usize;
    let mut src = AudioData::new(rate, 1, n);
    for i in 0..n {
        let t = i as f32 / rate as f32;
        src.ch[0][i] = (std::f32::consts::TAU * 200.0 * t).sin() * 0.4
            + (std::f32::consts::TAU * 6000.0 * t).sin() * 0.4;
    }
    let bands = dsp::split_bands(&src, &[1000.0]);
    assert_eq!(bands.len(), 2);
    assert_eq!(bands[0].frames(), n);
    assert_eq!(bands[1].frames(), n);
    let low = tone_strength(&bands[0], 200.0);
    let low_leak = tone_strength(&bands[0], 6000.0);
    let high = tone_strength(&bands[1], 6000.0);
    let high_leak = tone_strength(&bands[1], 200.0);
    assert!(low > low_leak * 10.0, "low band {low} vs {low_leak}");
    assert!(high > high_leak * 10.0, "high band {high} vs {high_leak}");
    assert!(low > 0.0 && high > 0.0, "both bands carry audio");
}

/// Undo history follows Preferences > Memory (80 steps until the preferences are applied) and
/// never drops the step being shown or the saved position's bookkeeping.
#[test]
fn undo_history_keeps_the_configured_number_of_steps() {
    let mut f = crate::app::AudioFile::new(
        1,
        "t".into(),
        None,
        AudioData::new(48000, 1, 480),
        32,
        true,
        "Open",
    );
    let limit = crate::prefs::undo_limit();
    for i in 0..limit + 25 {
        let mut d = AudioData::new(48000, 1, 480);
        d.ch[0][0] = i as f32 / 1000.0;
        f.push("Edit", d);
    }
    assert_eq!(
        f.history.len(),
        limit + 1,
        "the limit's steps plus the current state"
    );
    assert_eq!(f.hist_pos, f.history.len() - 1);
    assert!((f.data.ch[0][0] - (limit + 24) as f32 / 1000.0).abs() < 1e-6);
}

#[test]
fn new_markers_use_the_name_from_preferences() {
    let mut p = crate::prefs::Prefs::default();
    assert_eq!(p.marker_name(3), "Marker 03");
    p.marker_name_prefix = "  Cue ".into();
    assert_eq!(p.marker_name(12), "Cue 12");
    p.marker_name_prefix = "   ".into();
    assert_eq!(p.marker_name(1), "Marker 01");
}

#[test]
fn close_other_panels_in_group_keeps_only_that_panel() {
    use crate::workspace::PanelId;
    let mut l = crate::workspace::preset("Default");
    let n = l.close_others_in_group(PanelId::Markers);
    assert_eq!(n, 3, "Media Browser, Effects Rack and Properties close");
    assert!(l.contains(PanelId::Markers));
    assert!(!l.contains(PanelId::EffectsRack));
    assert!(l.contains(PanelId::Files), "other groups stay");
    assert_eq!(
        l.close_others_in_group(PanelId::PlugIns),
        0,
        "a panel not shown closes nothing"
    );
}

/// Bypassing or adding an effect keeps the other effects running (a plug-in keeps its
/// instance and sound): a delay's echo still comes out after another slot is switched off.
#[test]
fn bypassing_one_effect_keeps_the_others_running() {
    let delay = EffectSlot::new(Effect::Delay {
        time_ms: 100.0,
        feedback: 0.0,
        mix: 1.0,
    });
    let amp = EffectSlot::new(Effect::Amplify { gain_db: 0.0 });
    let mut slots = vec![delay, amp];
    let mut ch = dsp::Chain::new(&slots, 48000.0);
    let mut imp = vec![vec![0.0f32; 2400]; 2];
    imp[0][0] = 1.0;
    imp[1][0] = 1.0;
    ch.process(&mut imp);
    slots[1].enabled = false;
    ch.update(&slots, 48000.0);
    let mut rest = vec![vec![0.0f32; 4800]; 2];
    ch.process(&mut rest);
    assert!(
        energy(&AudioData {
            rate: 48000,
            ch: rest
        }) > 0.1,
        "the echo of the impulse still arrives"
    );
    slots[1].enabled = true;
    ch.update(&slots, 48000.0);
    let mut x = vec![vec![0.25f32; 256]; 2];
    ch.process(&mut x);
    assert!(x.iter().flatten().all(|v| v.is_finite()));
}

/// Match Clip Loudness brings a quiet and a loud clip to the same heard level; silent clips
/// and missing sources are left alone.
#[test]
fn match_clip_loudness_evens_out_clip_levels() {
    let mut s = Session::new("t", 48000, 1);
    let quiet = Arc::new(AudioData {
        rate: 48000,
        ch: vec![vec![0.05; 48000]; 2],
    });
    let loud = Arc::new(AudioData {
        rate: 48000,
        ch: vec![vec![0.5; 48000]; 2],
    });
    let silent = Arc::new(AudioData {
        rate: 48000,
        ch: vec![vec![0.0; 48000]; 2],
    });
    for (fid, start) in [(1u64, 0.0), (2, 2.0), (3, 4.0), (99, 6.0)] {
        s.tracks[0].clips.push(Clip {
            id: fid + 100,
            file_id: fid,
            name: "c".into(),
            start,
            offset: 0.0,
            len: 1.0,
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
        });
    }
    let src = |fid: u64| match fid {
        1 => Some(quiet.clone()),
        2 => Some(loud.clone()),
        3 => Some(silent.clone()),
        _ => None,
    };
    assert_eq!(crate::session::match_clip_loudness(&mut s, src), 2);
    let g: Vec<f32> = s.tracks[0].clips.iter().map(|c| c.gain_db).collect();
    // 0.05 is 20 dB under 0.5: each moves 10 dB towards the middle
    assert!(
        (g[0] - 10.0).abs() < 0.05 && (g[1] + 10.0).abs() < 0.05,
        "{g:?}"
    );
    assert_eq!((g[2], g[3]), (0.0, 0.0));
    // an empty session or track: nothing to match, no panic
    let mut e = Session::new("e", 48000, 0);
    assert_eq!(crate::session::match_clip_loudness(&mut e, |_| None), 0);
}

/// The live plug-in lookup only ever hands out a hosted plug-in's own processor, and only
/// when the slot asked about is the one running there.
#[test]
fn live_processor_lookup_matches_only_the_same_plugin() {
    let amp = EffectSlot::new(Effect::Amplify { gain_db: 0.0 });
    let plug = EffectSlot::new(Effect::Plugin {
        id: "vst3:00000000000000000000000000000000".into(),
        name: "x".into(),
        params: vec![],
        state: None,
    });
    let other = EffectSlot::new(Effect::Plugin {
        id: "clap:other".into(),
        name: "y".into(),
        params: vec![],
        state: None,
    });
    let mut ch = dsp::Chain::new(&[amp.clone(), plug.clone()], 48000.0);
    assert!(
        ch.proc_for(0, &amp).is_none(),
        "built-in effects have no plug-in window"
    );
    assert!(
        ch.proc_for(1, &other).is_none(),
        "another plug-in in that place is not it"
    );
    assert!(ch.proc_for(5, &plug).is_none(), "past the end");
    // the plug-in itself (not loadable in tests) is found, and has no editor to give
    let found = ch.proc_for(1, &plug).map(|p| p.live_editor().is_none());
    assert_eq!(found, Some(true));
}

#[test]
fn colour_themes_are_selectable_without_changing_clip_colours() {
    crate::theme::set_theme(0);
    let normal = crate::theme::bg();
    let track_colours = crate::session::TRACK_COLORS;
    crate::theme::set_theme(1);
    assert_ne!(
        normal,
        crate::theme::bg(),
        "the darker theme must look different"
    );
    assert_eq!(
        track_colours,
        crate::session::TRACK_COLORS,
        "clip colours stay the same"
    );
    assert_eq!(crate::theme::theme_name(0), "AudioCraft");
    assert_eq!(crate::theme::theme_name(1), "Audition Dark");
    assert!(
        crate::theme::theme_count() >= 24,
        "PhotoCraft and VectorCraft themes are listed"
    );
    assert!(
        (0..crate::theme::theme_count())
            .any(|i| crate::theme::theme_name(i) == "PhotoCraft Pro (Medium Gray)")
    );
    assert!(
        (0..crate::theme::theme_count())
            .any(|i| crate::theme::theme_name(i) == "VectorCraft Charcoal")
    );
    crate::theme::set_theme(0);
}

#[test]
fn track_io_button_height_has_a_sensible_default() {
    let p = crate::prefs::Prefs::default();
    assert!((14.0..=40.0).contains(&p.track_io_button_height));
    crate::prefs::set_live(&p);
    assert_eq!(crate::prefs::io_button_height(), p.track_io_button_height);
}

/// Every theme keeps its text readable: body text at least 4.5:1 and dim text at least 3:1
/// against the panels; softer themes are flagged on their card.
#[test]
fn every_theme_has_readable_text() {
    for i in 0..crate::theme::theme_count() {
        let (body, dim) = crate::theme::theme_contrast(i);
        assert!(body >= 4.5, "{} body {body}", crate::theme::theme_name(i));
        assert!(dim >= 3.0, "{} dim {dim}", crate::theme::theme_name(i));
        if body < 7.0 {
            assert!(crate::theme::theme_low_contrast(i));
        }
    }
}

/// Fixed greys follow the theme, while AudioCraft's own theme keeps them exactly.
#[test]
fn fixed_greys_follow_the_theme() {
    crate::theme::set_theme(0);
    assert_eq!(crate::theme::gray(48), egui::Color32::from_gray(48));
    crate::theme::set_theme(5);
    assert_ne!(crate::theme::gray(35), egui::Color32::from_gray(35));
    crate::theme::set_theme(0);
}

/// Blender-style drag toggling: pressing one toggle flips it, and sweeping over the others
/// gives them that new state; ones already in it stay; releasing ends the sweep.
#[test]
fn drag_toggle_sweeps_across_toggles() {
    let ctx = egui::Context::default();
    let mut vals = [false, true, false, false];
    let pos = |i: usize| egui::pos2(20.0, 30.0 + i as f32 * 30.0);
    let mut frame = |events: Vec<egui::Event>, vals: &mut [bool; 4]| {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(200.0, 200.0),
            )),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                for (i, v) in vals.iter_mut().enumerate() {
                    let r = egui::Rect::from_center_size(pos(i), egui::vec2(20.0, 20.0));
                    let resp =
                        ui.interact(r, egui::Id::new(("t", i)), egui::Sense::click_and_drag());
                    crate::theme::drag_toggle(ui, "test", &resp, v);
                }
            });
        });
        out.textures_delta.clear();
    };
    let btn = |p, pressed| egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(vec![egui::Event::PointerMoved(pos(0))], &mut vals);
    frame(vec![btn(pos(0), true)], &mut vals);
    assert_eq!(
        vals,
        [true, true, false, false],
        "the pressed toggle turned on"
    );
    for i in 1..=2 {
        frame(vec![egui::Event::PointerMoved(pos(i))], &mut vals);
    }
    assert_eq!(
        vals,
        [true, true, true, false],
        "swept ones turned on, on ones stayed on"
    );
    frame(vec![btn(pos(2), false)], &mut vals);
    frame(vec![egui::Event::PointerMoved(pos(3))], &mut vals);
    assert_eq!(
        vals,
        [true, true, true, false],
        "after release nothing else changes"
    );
}

/// Brightness and Saturation never make text unreadable, in any theme: body text keeps at
/// least 4.5:1 against the panels (it turns dark once the surfaces become light).
#[test]
fn brightness_and_saturation_keep_text_readable() {
    for t in 0..crate::theme::theme_count() {
        crate::theme::set_theme(t as u8);
        for b in [-1.0f32, -0.5, 0.0, 0.3, 0.5, 0.75, 1.0] {
            for s in [0.0f32, 1.0, 1.5] {
                crate::theme::set_adjust(b, s);
                let c = crate::theme::contrast(crate::theme::text(), crate::theme::panel());
                assert!(
                    c >= 4.5,
                    "{} b={b} s={s}: {c:.2}",
                    crate::theme::theme_name(t)
                );
            }
        }
    }
    crate::theme::set_adjust(0.0, 1.0);
    crate::theme::set_theme(0);
}

/// Brightness trims a theme; it never turns a dark theme grey or a light one dark.
#[test]
fn brightness_keeps_dark_themes_dark() {
    for t in 0..crate::theme::theme_count() {
        crate::theme::set_theme(t as u8);
        crate::theme::set_adjust(0.0, 1.0);
        let dark = crate::theme::is_dark();
        for b in [-1.0f32, 1.0] {
            crate::theme::set_adjust(b, 1.5);
            assert_eq!(
                crate::theme::is_dark(),
                dark,
                "{} b={b}",
                crate::theme::theme_name(t)
            );
        }
    }
    crate::theme::set_adjust(0.0, 1.0);
    crate::theme::set_theme(0);
}

/// A plug-in window's own edits are recorded in the playing chain without being sent back to
/// the plug-in: the next update keeps the same running processor and finds nothing to change.
#[test]
fn plugin_window_edits_are_not_echoed_back() {
    let slot = |v: f32| {
        EffectSlot::new(Effect::Plugin {
            id: "vst3:00000000000000000000000000000000".into(),
            name: "Test".into(),
            params: vec![("gain".into(), v)],
            state: None,
        })
    };
    let mut chain = crate::dsp::Chain::new(&[slot(0.2)], 48000.0);
    let before = chain
        .proc_for(0, &slot(0.2))
        .map(|p| &**p as *const dyn crate::dsp::Processor as *const u8 as usize);
    assert!(chain.adopt_slot(0, &slot(0.7)), "same plug-in: adopted");
    chain.update(&[slot(0.7)], 48000.0);
    let after = chain
        .proc_for(0, &slot(0.7))
        .map(|p| &**p as *const dyn crate::dsp::Processor as *const u8 as usize);
    assert_eq!(before, after, "the running plug-in is kept");
    assert!(
        !chain.adopt_slot(0, &EffectSlot::new(Effect::Amplify { gain_db: 0.0 })),
        "another effect is not adopted"
    );
}

/// Automated effect settings glide: changing a reverb's settings block by block keeps its tail
/// (it used to be rebuilt empty on every change, so it fell silent while its settings moved).
#[test]
fn automated_reverb_keeps_its_tail() {
    let rev = |room: f32| {
        EffectSlot::new(Effect::Reverb {
            room,
            damping: 0.3,
            width: 1.0,
            mix: 1.0,
        })
    };
    let mut chain = crate::dsp::Chain::new(&[rev(0.8)], 48000.0);
    let mut block = vec![vec![0.0f32; 512]; 2];
    block[0][0] = 1.0;
    block[1][0] = 1.0;
    chain.process(&mut block);
    let mut tail = 0.0f32;
    for k in 0..40 {
        // the automation moves the room size on every block
        chain.update(&[rev(0.8 - k as f32 * 0.01)], 48000.0);
        let mut b = vec![vec![0.0f32; 512]; 2];
        chain.process(&mut b);
        if k > 10 {
            tail += b[0].iter().map(|v| v * v).sum::<f32>();
        }
    }
    assert!(
        tail > 1e-4,
        "the reverb kept ringing while its settings moved ({tail})"
    );
}

/// A file too big for the computer's memory is refused with a message (it used to end the
/// program); WAV files still load channel by channel after the leaner loader change.
#[test]
fn oversized_audio_is_refused_and_wav_still_loads() {
    if crate::sysinfo::available_memory().is_some() {
        let e = crate::audio::ensure_room(u64::MAX / 4).unwrap_err();
        assert!(e.contains("Not enough memory"), "{e}");
    }
    assert!(crate::audio::ensure_room(1024).is_ok());
    let dir = std::env::temp_dir().join(format!("ac-wav-load-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join("t.wav");
    let mut d = crate::audio::AudioData::new(48000, 2, 1000);
    for i in 0..1000 {
        d.ch[0][i] = (i as f32 / 1000.0) - 0.5;
        d.ch[1][i] = -d.ch[0][i];
    }
    crate::audio::save_wav(&p, &d, 32, true).expect("write");
    let l = crate::audio::load(&p).expect("load");
    assert_eq!(l.data.channels(), 2);
    assert_eq!(l.data.frames(), 1000);
    assert!((l.data.ch[0][250] - d.ch[0][250]).abs() < 1e-6);
    assert!((l.data.ch[1][250] - d.ch[1][250]).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The effects people automate glide (keep their sound while settings move); the menu warns
/// about the few that restart.
#[test]
fn common_effects_glide_when_automated() {
    for e in Effect::all_defaults() {
        let n = e.name().to_string();
        // the effects people automate (exact names; DeReverb or Convolution Reverb are others)
        if [
            "Reverb",
            "Studio Reverb",
            "Delay",
            "Analog Delay",
            "Echo",
            "Chorus",
            "Flanger",
            "Chorus/Flanger",
            "Phaser",
            "Parametric Equalizer",
            "Graphic Equalizer",
            "Compressor",
            "Hard Limiter",
            "Guitar Suite",
            "Mastering",
        ]
        .contains(&n.as_str())
        {
            assert!(e.glides_when_automated(), "{n} should glide");
        }
    }
}

#[test]
#[ignore]
fn print_effects_that_restart_when_automated() {
    for e in Effect::all_defaults() {
        if !e.glides_when_automated() && !e.auto_params().is_empty() {
            println!("restarts: {}", e.name());
        }
    }
}

#[test]
fn snap_targets_follow_their_settings() {
    use crate::prefs::Prefs;
    use crate::ui_multi::collect_snap_targets;

    let mut s = Session::new("T", 48000, 1);
    s.tracks[0].clips.push(Clip {
        id: 1,
        file_id: 0,
        name: "c".into(),
        start: 1.25,
        offset: 0.0,
        len: 2.0,
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
    });
    s.markers.push(crate::session::Marker {
        name: "m".into(),
        time: 0.75,
        duration: 0.0,
    });
    let view = (0.0, 30.0);
    // everything on: the session start, the playhead, the clip's edges, the marker and the
    // ruler's labelled marks are all targets (major step is 5 s at this zoom)
    let pts = collect_snap_targets(&s, &Prefs::default(), 3.5, 30.0, 1000.0, view);
    assert!(pts.contains(&0.0), "session start missing");
    assert!(pts.contains(&3.5), "playhead missing");
    assert!(pts.contains(&1.25), "clip start missing");
    assert!(pts.contains(&3.25), "clip end missing");
    assert!(pts.contains(&0.75), "marker missing");
    assert!(pts.contains(&5.0), "time display mark missing");
    // each target can be turned off on its own
    let mut p = Prefs::default();
    p.snap_session_start = false;
    p.snap_playhead = false;
    p.snap_clip_ends = false;
    p.snap_clip_starts = false;
    p.marker_snap = false;
    let pts = collect_snap_targets(&s, &p, 3.5, 30.0, 1000.0, view);
    assert!(!pts.contains(&3.5), "playhead should be off");
    assert!(!pts.contains(&1.25), "clip start should be off");
    assert!(!pts.contains(&3.25), "clip end should be off");
    assert!(!pts.contains(&0.75), "marker should be off");
    assert!(
        pts.iter().filter(|t| **t != 0.0).count() > 0,
        "time display marks stay on"
    );
    p.snap_time_display = false;
    assert!(
        collect_snap_targets(&s, &p, 3.5, 30.0, 1000.0, view).is_empty(),
        "every target off leaves nothing to snap to"
    );
    // with only the clip's ends on, ends are targets and starts are not
    let mut p = Prefs::default();
    p.snap_session_start = false;
    p.snap_playhead = false;
    p.snap_clip_starts = false;
    p.snap_time_display = false;
    p.marker_snap = false;
    let pts = collect_snap_targets(&s, &p, 3.5, 30.0, 1000.0, view);
    assert!(pts.contains(&3.25) && !pts.contains(&1.25));
}
