//! Keyframe curves: optional Bezier handles on envelope keyframes, how a curved envelope is
//! evaluated, and how it is turned back into plain straight-line keyframes ("baked") for players
//! and file formats that only know straight lines between keyframes.
//!
//! Envelopes stay `Vec<(time, value)>` everywhere; the handles live in a parallel `Vec<Handles>`
//! (same length, same order). A handles list of another length is ignored (straight lines), so
//! older sessions and code paths that never heard of handles keep working unchanged.

use serde::{Deserialize, Serialize};

/// The two direction handles of one keyframe, as offsets from the keyframe in (seconds, value).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Handles {
    /// The keyframe has handles (a curve); off = a corner with straight lines.
    pub on: bool,
    /// The two handles move independently (a broken / cusp point).
    pub broken: bool,
    /// In handle (towards the previous keyframe): dt <= 0.
    pub in_dt: f64,
    pub in_dv: f32,
    /// Out handle (towards the next keyframe): dt >= 0.
    pub out_dt: f64,
    pub out_dv: f32,
    /// Hold interpolation: the value stays put until the next keyframe, then jumps.
    pub hold: bool,
}

/// How the line leaves a keyframe (the keyframe right-click menu's Interpolation choices).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interp {
    Linear,
    Hold,
    /// Bezier handles that stay in line (smooth).
    Bezier,
    /// Bezier handles that move independently.
    BezierBroken,
}

impl Handles {
    pub fn interp(&self) -> Interp {
        if self.hold {
            Interp::Hold
        } else if self.on && self.broken {
            Interp::BezierBroken
        } else if self.on {
            Interp::Bezier
        } else {
            Interp::Linear
        }
    }
}

/// Give keyframe `i` an interpolation (Bezier keeps handles it already has, or makes new ones).
pub fn set_interp(env: &[(f64, f32)], hs: &mut Vec<Handles>, i: usize, m: Interp) {
    fit(hs, env.len());
    let Some(h) = hs.get_mut(i) else { return };
    match m {
        Interp::Linear => *h = Handles::default(),
        Interp::Hold => {
            *h = Handles {
                hold: true,
                ..Handles::default()
            }
        }
        Interp::Bezier | Interp::BezierBroken => {
            let mut n = if h.on { *h } else { auto_handles(env, i) };
            n.hold = false;
            n.broken = m == Interp::BezierBroken;
            *h = n;
        }
    }
}

#[cfg(test)]
impl Handles {
    /// Symmetric handles reaching `dt` seconds and `dv` either side (tests).
    pub fn smooth(dt: f64, dv: f32) -> Handles {
        Handles {
            on: true,
            broken: false,
            in_dt: -dt.abs(),
            in_dv: -dv,
            out_dt: dt.abs(),
            out_dv: dv,
            hold: false,
        }
    }
}

/// Which side of a keyframe a handle is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    In,
    Out,
}

/// The handles of keyframe `i` (none when the list doesn't match the envelope).
#[inline]
pub fn h_at(hs: &[Handles], n: usize, i: usize) -> Handles {
    if hs.len() == n {
        hs.get(i).copied().unwrap_or_default()
    } else {
        Handles::default()
    }
}

/// Whether any keyframe of the envelope has handles or holds its value.
pub fn has_curves(env_len: usize, hs: &[Handles]) -> bool {
    hs.len() == env_len && hs.iter().any(|h| h.on || h.hold)
}

/// Keep the handles list in step with an envelope of `n` keyframes (only when it already is or
/// is empty; a mismatched list is dropped).
pub fn fit(hs: &mut Vec<Handles>, n: usize) {
    if hs.len() != n {
        if hs.iter().any(|h| h.on || h.hold) && hs.len() > n {
            hs.truncate(n);
        } else {
            hs.clear();
            hs.resize(n, Handles::default());
        }
    }
}

/// Insert a keyframe at its place in time; returns its index. The handles list follows.
pub fn insert_key(env: &mut Vec<(f64, f32)>, hs: &mut Vec<Handles>, t: f64, v: f32) -> usize {
    fit(hs, env.len());
    let idx = env.iter().position(|e| e.0 > t).unwrap_or(env.len());
    env.insert(idx, (t, v));
    hs.insert(idx, Handles::default());
    idx
}

/// Remove keyframe `i` (and its handles).
pub fn remove_key(env: &mut Vec<(f64, f32)>, hs: &mut Vec<Handles>, i: usize) {
    fit(hs, env.len());
    if i < env.len() {
        env.remove(i);
        hs.remove(i);
    }
}

/// Straight-line value of an envelope (held at both ends; `empty` when it has no keyframes).
fn linear(env: &[(f64, f32)], t: f64, empty: f32) -> f32 {
    match env.len() {
        0 => empty,
        1 => env[0].1,
        _ => {
            if t <= env[0].0 {
                return env[0].1;
            }
            for w in env.windows(2) {
                let (t0, d0) = w[0];
                let (t1, d1) = w[1];
                if t <= t1 {
                    let f = if t1 - t0 > 1e-12 {
                        ((t - t0) / (t1 - t0)) as f32
                    } else {
                        1.0
                    };
                    return d0 + (d1 - d0) * f;
                }
            }
            env[env.len() - 1].1
        }
    }
}

/// Monotone (Fritsch-Carlson) tangents for a smooth curve through the points that never overshoots.
fn monotone_tangents(p: &[(f64, f32)]) -> Vec<f64> {
    let n = p.len();
    if n < 2 {
        return vec![0.0; n];
    }
    let d: Vec<f64> = p
        .windows(2)
        .map(|w| {
            if w[1].0 - w[0].0 > 1e-12 {
                (w[1].1 - w[0].1) as f64 / (w[1].0 - w[0].0)
            } else {
                0.0
            }
        })
        .collect();
    let mut m = vec![0.0; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        m[i] = if d[i - 1] * d[i] <= 0.0 {
            0.0
        } else {
            (d[i - 1] + d[i]) * 0.5
        };
    }
    for i in 0..n - 1 {
        if d[i].abs() < 1e-12 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
            continue;
        }
        let a = m[i] / d[i];
        let b = m[i + 1] / d[i];
        let s = a * a + b * b;
        if s > 9.0 {
            let k = 3.0 / s.sqrt();
            m[i] = k * a * d[i];
            m[i + 1] = k * b * d[i];
        }
    }
    m
}

/// The cubic Bezier control points of the segment from keyframe `i` to `i + 1`, in (t, v).
/// A side without handles uses the straight line's third point (or, when `smooth`, the
/// monotone spline's tangent), so a curve meets a corner cleanly.
fn controls(
    env: &[(f64, f32)],
    hs: &[Handles],
    i: usize,
    smooth_m: Option<&[f64]>,
) -> [(f64, f64); 4] {
    let (t0, v0) = (env[i].0, env[i].1 as f64);
    let (t1, v1) = (env[i + 1].0, env[i + 1].1 as f64);
    let seg = (t1 - t0).max(1e-12);
    let a = h_at(hs, env.len(), i);
    let b = h_at(hs, env.len(), i + 1);
    let p1 = if a.on {
        let dt = a.out_dt.clamp(0.0, seg);
        // a handle pulled past the next keyframe keeps its direction, shortened to fit
        let k = if a.out_dt > seg && a.out_dt > 0.0 {
            seg / a.out_dt
        } else {
            1.0
        };
        (t0 + dt, v0 + a.out_dv as f64 * k)
    } else if let Some(m) = smooth_m {
        (t0 + seg / 3.0, v0 + m[i] * seg / 3.0)
    } else {
        (t0 + seg / 3.0, v0 + (v1 - v0) / 3.0)
    };
    let p2 = if b.on {
        let dt = b.in_dt.clamp(-seg, 0.0);
        let k = if -b.in_dt > seg && b.in_dt < 0.0 {
            seg / -b.in_dt
        } else {
            1.0
        };
        (t1 + dt, v1 + b.in_dv as f64 * k)
    } else if let Some(m) = smooth_m {
        (t1 - seg / 3.0, v1 - m[i + 1] * seg / 3.0)
    } else {
        (t1 - seg / 3.0, v1 - (v1 - v0) / 3.0)
    };
    [(t0, v0), p1, p2, (t1, v1)]
}

#[inline]
fn bez(c: &[(f64, f64); 4], u: f64) -> (f64, f64) {
    let w = 1.0 - u;
    let a = w * w * w;
    let b = 3.0 * w * w * u;
    let cc = 3.0 * w * u * u;
    let d = u * u * u;
    (
        a * c[0].0 + b * c[1].0 + cc * c[2].0 + d * c[3].0,
        a * c[0].1 + b * c[1].1 + cc * c[2].1 + d * c[3].1,
    )
}

/// Value of the segment's curve at time `t` (the time control points are kept inside the
/// segment, so time only goes forward and bisection finds the one matching point).
fn seg_value(c: &[(f64, f64); 4], t: f64) -> f64 {
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..40 {
        let m = 0.5 * (lo + hi);
        if bez(c, m).0 < t {
            lo = m;
        } else {
            hi = m;
        }
    }
    bez(c, 0.5 * (lo + hi)).1
}

/// Value of a (possibly curved) envelope at `t`, held at both ends; `empty` without keyframes.
/// `smooth`: segments without handles follow a monotone spline instead of straight lines (fades).
pub fn eval(env: &[(f64, f32)], hs: &[Handles], t: f64, empty: f32, smooth: bool) -> f32 {
    let curved = has_curves(env.len(), hs);
    if (!curved && !smooth) || env.len() < 2 {
        return linear(env, t, empty);
    }
    if t <= env[0].0 {
        return env[0].1;
    }
    let n = env.len();
    if t >= env[n - 1].0 {
        return env[n - 1].1;
    }
    let i = env
        .partition_point(|e| e.0 <= t)
        .saturating_sub(1)
        .min(n - 2);
    let a = h_at(hs, n, i);
    let b = h_at(hs, n, i + 1);
    if a.hold {
        return env[i].1;
    }
    if !smooth && !a.on && !b.on {
        let (t0, d0) = env[i];
        let (t1, d1) = env[i + 1];
        let f = if t1 - t0 > 1e-12 {
            ((t - t0) / (t1 - t0)) as f32
        } else {
            1.0
        };
        return d0 + (d1 - d0) * f;
    }
    let m = if smooth {
        Some(monotone_tangents(env))
    } else {
        None
    };
    let c = controls(env, hs, i, m.as_deref());
    seg_value(&c, t) as f32
}

/// Points along the curve for drawing: (t, v), dense where it bends.
pub fn polyline(
    env: &[(f64, f32)],
    hs: &[Handles],
    smooth: bool,
    per_seg: usize,
) -> Vec<(f64, f32)> {
    if env.len() < 2 || (!smooth && !has_curves(env.len(), hs)) {
        return env.to_vec();
    }
    let m = if smooth {
        Some(monotone_tangents(env))
    } else {
        None
    };
    let n = env.len();
    let mut out = vec![env[0]];
    for i in 0..n - 1 {
        let a = h_at(hs, n, i);
        let b = h_at(hs, n, i + 1);
        if a.hold {
            // a step: flat to the next keyframe's time, then straight up or down
            out.push((env[i + 1].0, env[i].1));
            out.push(env[i + 1]);
            continue;
        }
        if !smooth && !a.on && !b.on {
            out.push(env[i + 1]);
            continue;
        }

        let c = controls(env, hs, i, m.as_deref());
        for k in 1..=per_seg.max(2) {
            let u = k as f64 / per_seg.max(2) as f64;
            let (t, v) = bez(&c, u);
            out.push((t, v as f32));
        }
    }
    out
}

/// Straight-line keyframes that follow the curve to within `tol` (value units). The result has
/// no handles and is what a player or file format without curves needs. Plain envelopes come
/// back unchanged.
pub fn bake(env: &[(f64, f32)], hs: &[Handles], smooth: bool, tol: f32) -> Vec<(f64, f32)> {
    if env.len() < 2 || (!smooth && !has_curves(env.len(), hs)) {
        return env.to_vec();
    }
    let dense = polyline(env, hs, smooth, 64);
    simplify(&dense, tol.max(1e-4))
}

/// Ramer-Douglas-Peucker on (t, v), measuring the vertical (value) error at each point.
pub(crate) fn simplify(p: &[(f64, f32)], tol: f32) -> Vec<(f64, f32)> {
    if p.len() < 3 {
        return p.to_vec();
    }
    let mut keep = vec![false; p.len()];
    keep[0] = true;
    keep[p.len() - 1] = true;
    let mut stack = vec![(0usize, p.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (ta, va) = p[a];
        let (tb, vb) = p[b];
        let mut worst = (0.0f32, 0usize);
        for (k, q) in p.iter().enumerate().take(b).skip(a + 1) {
            let f = if tb - ta > 1e-12 {
                ((q.0 - ta) / (tb - ta)) as f32
            } else {
                0.0
            };
            let e = (q.1 - (va + (vb - va) * f)).abs();
            if e > worst.0 {
                worst = (e, k);
            }
        }
        if worst.0 > tol {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    p.iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(q, _)| *q)
        .collect()
}

/// Default handles for keyframe `i`: along the line through its neighbours (Catmull-Rom like),
/// a third of the way to each neighbour, so turning handles on keeps the curve's shape close.
pub fn auto_handles(env: &[(f64, f32)], i: usize) -> Handles {
    let n = env.len();
    if n == 0 || i >= n {
        return Handles::default();
    }
    let prev = if i > 0 { env[i - 1] } else { env[i] };
    let next = if i + 1 < n { env[i + 1] } else { env[i] };
    let span = next.0 - prev.0;
    let slope = if span > 1e-12 {
        (next.1 - prev.1) as f64 / span
    } else {
        0.0
    };
    // ends get a flat handle
    let slope = if i == 0 || i + 1 == n { 0.0 } else { slope };
    let li = if i > 0 {
        (env[i].0 - prev.0) / 3.0
    } else {
        (next.0 - env[i].0) / 3.0
    };
    let lo = if i + 1 < n {
        (next.0 - env[i].0) / 3.0
    } else {
        li
    };
    let li = li.max(1e-4);
    let lo = lo.max(1e-4);
    Handles {
        on: true,
        broken: false,
        in_dt: -li,
        in_dv: (-slope * li) as f32,
        out_dt: lo,
        out_dv: (slope * lo) as f32,
        hold: false,
    }
}

/// Fade points: (fraction through the fade from the silent end, gain 0..1). The implied ends are
/// (0, 0) and (1, 1). Returns the gain at `x` (0..1); without points the plain fade curve is used.
pub fn fade_gain(pts: &[(f64, f32)], hs: &[Handles], x: f64, base: &dyn Fn(f64) -> f32) -> f32 {
    if pts.is_empty() {
        return base(x);
    }
    let (full, fh) = fade_full(pts, hs);
    eval(&full, &fh, x.clamp(0.0, 1.0), 1.0, true).clamp(0.0, 1.0)
}

/// The fade points with their implied ends, and the handles to match.
pub fn fade_full(pts: &[(f64, f32)], hs: &[Handles]) -> (Vec<(f64, f32)>, Vec<Handles>) {
    let mut full = Vec::with_capacity(pts.len() + 2);
    full.push((0.0, 0.0));
    full.extend(
        pts.iter()
            .map(|(x, g)| (x.clamp(1e-4, 1.0 - 1e-4), g.clamp(0.0, 1.0))),
    );
    full.push((1.0, 1.0));
    let mut fh = vec![Handles::default()];
    if hs.len() == pts.len() {
        fh.extend(hs.iter().copied());
    } else {
        fh.extend(std::iter::repeat_n(Handles::default(), pts.len()));
    }
    fh.push(Handles::default());
    (full, fh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_envelope_is_unchanged_by_empty_handles() {
        let env = vec![(0.0, 0.0), (1.0, -12.0)];
        assert!((eval(&env, &[], 0.5, 0.0, false) + 6.0).abs() < 1e-5);
        assert_eq!(bake(&env, &[], false, 0.1), env);
    }

    #[test]
    fn handles_bend_the_curve_and_bake_follows_it() {
        let env = vec![(0.0, 0.0), (1.0, -12.0)];
        // flat handles at both ends: an S-curve that is still 0 near the start
        let hs = vec![Handles::smooth(0.4, 0.0), Handles::smooth(0.4, 0.0)];
        let early = eval(&env, &hs, 0.1, 0.0, false);
        assert!(early > -1.5, "{early}");
        let mid = eval(&env, &hs, 0.5, 0.0, false);
        assert!((mid + 6.0).abs() < 0.3, "{mid}");
        let baked = bake(&env, &hs, false, 0.05);
        assert!(baked.len() > 2);
        for k in 0..=20 {
            let t = k as f64 / 20.0;
            let a = eval(&env, &hs, t, 0.0, false);
            let b = eval(&baked, &[], t, 0.0, false);
            assert!((a - b).abs() < 0.1, "{t} {a} {b}");
        }
    }

    #[test]
    fn mismatched_handles_are_ignored_and_keys_keep_them_in_step() {
        let mut env = vec![(0.0, 0.0), (2.0, -6.0)];
        let mut hs = vec![Handles::default(); 5];
        assert!((eval(&env, &hs, 1.0, 0.0, false) + 3.0).abs() < 1e-5);
        hs.clear();
        let i = insert_key(&mut env, &mut hs, 1.0, -10.0);
        assert_eq!(i, 1);
        assert_eq!(hs.len(), 3);
        hs[1] = auto_handles(&env, 1);
        remove_key(&mut env, &mut hs, 0);
        assert_eq!((env.len(), hs.len()), (2, 2));
        assert!(hs[0].on);
    }

    #[test]
    fn fade_points_shape_the_fade_and_stay_in_range() {
        let base = |x: f64| x as f32;
        assert!((fade_gain(&[], &[], 0.25, &base) - 0.25).abs() < 1e-6);
        let pts = vec![(0.5, 0.9)];
        let g = fade_gain(&pts, &[], 0.5, &base);
        assert!((g - 0.9).abs() < 1e-3);
        for k in 0..=50 {
            let g = fade_gain(&pts, &[], k as f64 / 50.0, &base);
            assert!((0.0..=1.0).contains(&g));
        }
        // rising all the way (monotone: no dip below an earlier value)
        let mut last = -1.0;
        for k in 0..=50 {
            let g = fade_gain(&pts, &[], k as f64 / 50.0, &base);
            assert!(g >= last - 1e-5);
            last = g;
        }
    }
}
