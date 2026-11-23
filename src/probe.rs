//! `audiocraft --probe-plugin NAME`: a diagnostic that runs an installed VST3/CLAP effect the
//! way playback does (built on this, the main thread, processed on a separate audio thread in
//! 512-frame blocks) and prints the input and output levels, then changes every parameter live
//! (as an edit in the plug-in's window would) and prints the levels again. Nothing is saved.

use crate::dsp::{Chain, Effect, EffectSlot};

fn rms(b: &[Vec<f32>]) -> f32 {
    let n: usize = b.iter().map(|c| c.len()).sum();
    if n == 0 {
        return 0.0;
    }
    (b.iter()
        .flat_map(|c| c.iter())
        .map(|v| (*v as f64).powi(2))
        .sum::<f64>()
        / n as f64)
        .sqrt() as f32
}

/// Process `secs` of a 440 Hz tone through `chain` on a thread named "audio"; returns the chain
/// and (input rms, output rms) of the last half.
fn run(chain: Chain, rate: f32, secs: f32) -> (Chain, f32, f32) {
    let h = std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || {
            let mut chain = chain;
            let blocks = (secs * rate / 512.0) as usize;
            let (mut i_acc, mut o_acc) = (vec![], vec![]);
            for b in 0..blocks {
                let buf: Vec<Vec<f32>> = (0..2)
                    .map(|_| {
                        (0..512)
                            .map(|k| {
                                ((b * 512 + k) as f32 * 440.0 * std::f32::consts::TAU / rate).sin()
                                    * 0.3
                            })
                            .collect()
                    })
                    .collect();
                let mut out = buf.clone();
                chain.process(&mut out);
                if b >= blocks / 2 {
                    i_acc.extend(buf);
                    o_acc.extend(out);
                }
            }
            (chain, rms(&i_acc), rms(&o_acc))
        });
    match h.map(|h| h.join()) {
        Ok(Ok(r)) => r,
        _ => (Chain::default(), 0.0, 0.0),
    }
}

pub fn probe(name: &str) -> i32 {
    let list = crate::plugins::scan_hosted(false);
    let want = name.to_lowercase();
    let Some(e) = list.iter().find(|e| e.name.to_lowercase().contains(&want)) else {
        eprintln!("no installed effect matches '{name}'");
        return 2;
    };
    println!("plug-in: {} ({}, {})", e.name, e.format, e.id);
    let rate = 48_000.0;
    let mut slot = EffectSlot::new(crate::plugins::effect_for(e));
    if let Some(info) = crate::plugins::info(&e.id) {
        println!("{} parameters", info.params.len());
        for p in info.params.iter().take(12) {
            println!(
                "  {} '{}' {}..{} default {}",
                p.id, p.name, p.min, p.max, p.default
            );
        }
    }
    let chain = Chain::new(std::slice::from_ref(&slot), rate);
    let (mut chain, i1, o1) = run(chain, rate, 2.0);
    println!("as inserted: in {:.4} rms, out {:.4} rms", i1, o1);
    // a live edit: every continuous parameter to its minimum (on this, the UI, thread)
    if let (Effect::Plugin { params, .. }, Some(info)) =
        (&mut slot.effect, crate::plugins::info(&e.id))
    {
        for p in info.params.iter() {
            match params.iter_mut().find(|(k, _)| k == p.id) {
                Some(x) => x.1 = p.min,
                None => params.push((p.id.to_string(), p.min)),
            }
        }
    }
    chain.update(std::slice::from_ref(&slot), rate);
    let (mut chain, i2, o2) = run(chain, rate, 2.0);
    println!(
        "after a live edit (all parameters at minimum): in {:.4} rms, out {:.4} rms",
        i2, o2
    );
    // the same with a saved state, as after closing its window once
    if let Effect::Plugin { state, .. } = &mut slot.effect {
        *state = Some(vec![]);
    }
    chain.update(std::slice::from_ref(&slot), rate);
    let (chain, _, o3) = run(chain, rate, 1.0);
    println!("with a stored state: out {:.4} rms", o3);
    drop(chain);
    // as in the app: a fresh slot, its editor open on a second, UI-owned instance, its edits
    // polled on this thread while the audio thread plays
    let slot = EffectSlot::new(crate::plugins::effect_for(e));
    let Effect::Plugin { id, params, .. } = &slot.effect else {
        return 1;
    };
    let ed = crate::plugins::OpenEditor::open(id, params, None);
    println!(
        "editor: {}",
        if ed.is_ok() {
            "open".to_string()
        } else {
            format!("{:?}", ed.as_ref().err())
        }
    );
    let chain = Chain::new(std::slice::from_ref(&slot), rate);
    let (chain, i4, o4) = run(chain, rate, 2.0);
    println!("with its editor open: in {:.4} rms, out {:.4} rms", i4, o4);
    if let Ok(mut ed) = ed {
        let edits = ed.poll();
        println!(
            "edits reported by the editor after opening: {}",
            edits.len()
        );
        for (k, v) in edits.iter().take(8) {
            println!("  {k} = {v}");
        }
        let _ = ed.close();
    }
    drop(chain);
    crate::plugins::shutdown_hosting();
    0
}
