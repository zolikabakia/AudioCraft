//! Tests for track parameter automation (Show Envelopes), keyframe interpolation and the
//! generic effect-parameter access the automation uses.

use crate::audio::AudioData;
use crate::curves::{self, Handles, Interp};
use crate::dsp::{Effect, EffectSlot};
use crate::session::{Clip, Session, mixdown};
use std::sync::Arc;

use crate::tests::dc_file as dc;

fn clip(len: f64) -> Clip {
    Clip { id: 3, file_id: 1, name: "c".into(), start: 0.0, offset: 0.0, len, gain_db: 0.0, muted: false, fade_in: 0.0, fade_out: 0.0, env: vec![], pan_env: vec![], rack: vec![], speed: 1.0, fx: Default::default(), auto_in: true, auto_out: true, curves: Default::default() }
}

#[test]
fn every_effect_lists_settable_parameters() {
    for e in Effect::all_defaults() {
        let ps = e.auto_params();
        for (path, label, v, lo, hi) in ps {
            assert!(!label.is_empty() && lo <= v && v <= hi, "{} {path}", e.name());
            let mut e2 = e.clone();
            assert!(e2.set_auto_param(&path, (lo + hi) * 0.5), "{} {path} can be set", e.name());
            let after = e2.auto_params().into_iter().find(|p| p.0 == path).map(|p| p.2).unwrap_or(f32::NAN);
            assert!((after - (lo + hi) * 0.5).abs() <= 1e-3 * (hi - lo).abs().max(1.0), "{} {path}: {after}", e.name());
        }
    }
    let mut d = Effect::Delay { time_ms: 300.0, feedback: 0.3, mix: 0.5 };
    assert!(!d.set_auto_param("nope", 1.0));
    assert!(!d.set_auto_param("mix", f32::NAN));
    assert_eq!(crate::dsp::param_label("bands.1.gain_db"), "Band 2 Gain (dB)");
}

#[test]
fn mute_rack_and_effect_automation_are_heard() {
    let data = dc(2.0);
    let mut s = Session::new("auto", 48000, 1);
    s.tracks[0].clips.push(clip(2.0));
    // Amplify +0 dB in the rack; its gain is automated from 0 dB to -20 dB at 1 s (held)
    s.tracks[0].rack.push(EffectSlot::new(Effect::Amplify { gain_db: 0.0 }));
    let tr = &mut s.tracks[0];
    let i = tr.auto.param_mut("fx.0.gain_db", "Amplify: Gain (dB)", "Amplify", -40.0, 24.0);
    tr.auto.params[i].env = vec![(0.0, 0.0), (1.0, -20.0)];
    tr.auto.params[i].hs = vec![Handles { hold: true, ..Default::default() }, Default::default()];
    let out = mixdown(&s.snapshot(&|_| Some(data.clone())), 48000, 0.0, 2.0);
    let early = out.ch[0][24000].abs();
    let late = out.ch[0][72000].abs();
    assert!(early > 0.3, "{early}");
    assert!(late < early * 0.2, "the automated gain is heard: {early} -> {late}");
    // automation off: the effect's own setting
    s.tracks[0].io.automation = 0;
    let out = mixdown(&s.snapshot(&|_| Some(data.clone())), 48000, 0.0, 2.0);
    assert!(out.ch[0][72000].abs() > 0.3);
    s.tracks[0].io.automation = 1;
    // mute automation: silent after 1.5 s
    let tr = &mut s.tracks[0];
    tr.auto.params.clear();
    let m = tr.auto.param_mut("mute", "Mute", "", 0.0, 1.0);
    tr.auto.params[m].env = vec![(0.0, 0.0), (1.5, 1.0)];
    tr.auto.params[m].hs = vec![Handles { hold: true, ..Default::default() }, Default::default()];
    let out = mixdown(&s.snapshot(&|_| Some(data.clone())), 48000, 0.0, 2.0);
    assert!(out.ch[0][48000].abs() > 0.3 && out.ch[0][90000].abs() < 1e-6);
    // rack output automation
    let tr = &mut s.tracks[0];
    tr.auto.params.clear();
    let o = tr.auto.param_mut("rack.out", "Rack Output", "", -48.0, 24.0);
    tr.auto.params[o].env = vec![(0.0, -12.0)];
    let out = mixdown(&s.snapshot(&|_| Some(data.clone())), 48000, 0.0, 1.0);
    let g = out.ch[0][24000].abs();
    assert!((g - 0.5 * crate::audio::undb(-12.0)).abs() < 0.02, "{g}");
}

#[test]
fn hold_and_interpolation_choices() {
    let env = vec![(0.0, 0.0), (1.0, -12.0), (2.0, 0.0)];
    let mut hs = vec![];
    curves::set_interp(&env, &mut hs, 0, Interp::Hold);
    assert_eq!(curves::eval(&env, &hs, 0.9, 0.0, false), 0.0, "held until the next keyframe");
    assert!((curves::eval(&env, &hs, 1.5, 0.0, false) + 6.0).abs() < 1e-4, "then linear again");
    curves::set_interp(&env, &mut hs, 1, Interp::BezierBroken);
    assert!(hs[1].on && hs[1].broken && !hs[1].hold);
    assert_eq!(hs[1].interp(), Interp::BezierBroken);
    curves::set_interp(&env, &mut hs, 1, Interp::Linear);
    assert_eq!(hs[1], Handles::default());
    // a held keyframe plays as a step once baked
    let baked = curves::bake(&env, &hs, false, 0.01);
    assert!((crate::session::env_db(&baked, 0.99) - 0.0).abs() < 1e-3);
    assert!((crate::session::env_db(&baked, 1.0) + 12.0).abs() < 1e-3);
}
