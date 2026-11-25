//! Line icons in the Lucide style (24-unit grid, round 2-unit strokes), drawn from SVG path
//! data so they stay crisp at every size. Shapes marked "Lucide" are Lucide icons (ISC licence,
//! © Lucide Icons and Contributors; shipped with PhotoCraft/VectorCraft, see README); the others
//! are AudioCraft's own drawings on the same grid.

use egui::{Color32, Pos2, Rect, Shape, Stroke, pos2};

/// One icon: SVG path strings and circles (cx, cy, r) on a 24×24 grid.
pub struct Def {
    pub paths: &'static [&'static str],
    pub circles: &'static [(f32, f32, f32)],
}

macro_rules! def {
    ($name:ident, [$($p:expr),* $(,)?]) => {
        pub const $name: Def = Def { paths: &[$($p),*], circles: &[] };
    };
    ($name:ident, [$($p:expr),* $(,)?], [$($c:expr),* $(,)?]) => {
        pub const $name: Def = Def { paths: &[$($p),*], circles: &[$($c),*] };
    };
}

// Lucide: link
def!(
    LINK,
    [
        "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71",
        "M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"
    ]
);
// Lucide: arrow-left-right
def!(
    IO,
    ["M8 3 4 7l4 4", "M4 7h16", "m16 21 4-4-4-4", "M20 17H4"]
);
// own: one signal split to two sends
def!(
    SENDS,
    [
        "M3 12h6",
        "M9 12 19 5",
        "M9 12l10 7",
        "M15 5h4v4",
        "M15 19h4v-4"
    ]
);
// own: three EQ sliders
def!(
    EQ,
    [
        "M5 21v-7",
        "M5 10V3",
        "M12 21v-9",
        "M12 8V3",
        "M19 21v-5",
        "M19 12V3",
        "M2 14h6",
        "M9 8h6",
        "M16 16h6"
    ]
);
// own: metronome
def!(METRONOME, ["M9.5 3h5L19 21H5Z", "M8 16h8", "m12 16 6-10"]);
// own: clip stretching (a clip pulled at both ends)
def!(
    STRETCH,
    [
        "M8 7v10",
        "M16 7v10",
        "M3 12h18",
        "m6 9-3 3 3 3",
        "m18 9 3 3-3 3"
    ]
);
// own: horseshoe magnet
def!(
    MAGNET,
    [
        "M5 3h4v9a3 3 0 0 0 6 0V3h4v9a7 7 0 0 1-14 0Z",
        "M5 7h4",
        "M15 7h4"
    ]
);
// own: follow the playhead
def!(AUTOSCROLL, ["M4 4v16", "M8 12h12", "m16 8 4 4-4 4"]);
// Lucide: chevron-down (the snapping-targets unfold)
def!(CHEVRON_DOWN, ["m6 9 6 6 6-6"]);
// Lucide: folder-open
def!(
    FOLDER_OPEN,
    [
        "m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"
    ]
);
// Lucide: file-plus
def!(
    FILE_PLUS,
    [
        "M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z",
        "M14 2v5a1 1 0 0 0 1 1h5",
        "M9 15h6",
        "M12 18v-6"
    ]
);
// own: put into the multitrack (an arrow down onto two track lanes)
def!(INSERT, ["M12 2v10", "m8 8 4 4 4-4", "M3 16h18", "M3 21h18"]);
// Lucide: trash-2
def!(
    TRASH,
    [
        "M10 11v6",
        "M14 11v6",
        "M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6",
        "M3 6h18",
        "M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"
    ]
);
// Lucide: rotate-cw (loop playback)
def!(
    LOOP,
    [
        "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8",
        "M21 3v5h-5"
    ]
);
// own: a speaker that plays by itself
def!(AUTOPLAY, ["M11 5 6 9H3v6h3l5 4Z", "m15 9 6 3-6 3Z"]);
// own: an open eye / a closed eye (track visibility)
def!(
    EYE,
    ["M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z"],
    [(12.0, 12.0, 3.0)]
);
def!(
    EYE_OFF,
    [
        "M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z",
        "M3 3l18 18"
    ]
);

/// Draw `d` centred in `r` (a square of the shorter side) in `col`.
pub fn draw(p: &egui::Painter, r: Rect, d: &Def, col: Color32) {
    let side = r.width().min(r.height());
    let s = side / 24.0;
    let o = r.center() - egui::vec2(12.0 * s, 12.0 * s);
    let map = |x: f32, y: f32| pos2(o.x + x * s, o.y + y * s);
    let stroke = Stroke::new((2.0 * s).max(1.25), col);
    for path in d.paths {
        for (pts, closed) in parse(path) {
            let pts: Vec<Pos2> = pts.into_iter().map(|q| map(q.x, q.y)).collect();
            if pts.len() < 2 {
                continue;
            }
            if closed {
                p.add(Shape::closed_line(pts, stroke));
            } else {
                p.add(Shape::line(pts, stroke));
            }
        }
    }
    for (cx, cy, cr) in d.circles {
        p.circle_stroke(map(*cx, *cy), cr * s, stroke);
    }
}

/// Split an SVG path into polylines (24-unit coordinates) with whether each is closed.
/// Supports M L H V Z C S Q T A in both cases, implicit repeats included.
pub fn parse(d: &str) -> Vec<(Vec<Pos2>, bool)> {
    let toks = tokens(d);
    let mut out: Vec<(Vec<Pos2>, bool)> = vec![];
    let mut cur: Vec<Pos2> = vec![];
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    // last control point for S/T
    let mut last_c: Option<(f32, f32)> = None;
    let mut i = 0;
    let mut cmd = 'M';
    let num = |i: &mut usize| -> Option<f32> {
        match toks.get(*i) {
            Some(Tok::Num(v)) => {
                *i += 1;
                Some(*v)
            }
            _ => None,
        }
    };
    let flush = |cur: &mut Vec<Pos2>, out: &mut Vec<(Vec<Pos2>, bool)>, closed: bool| {
        if cur.len() >= 2 {
            out.push((std::mem::take(cur), closed));
        } else {
            cur.clear();
        }
    };
    while i < toks.len() {
        if let Tok::Cmd(c) = toks[i] {
            cmd = c;
            i += 1;
            if c == 'Z' || c == 'z' {
                flush(&mut cur, &mut out, true);
                x = sx;
                y = sy;
                last_c = None;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let (bx, by) = if rel { (x, y) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let (Some(a), Some(b)) = (num(&mut i), num(&mut i)) else {
                    break;
                };
                flush(&mut cur, &mut out, false);
                x = bx + a;
                y = by + b;
                sx = x;
                sy = y;
                cur.push(pos2(x, y));
                // further pairs are line-tos
                cmd = if rel { 'l' } else { 'L' };
                last_c = None;
            }
            'L' => {
                let (Some(a), Some(b)) = (num(&mut i), num(&mut i)) else {
                    break;
                };
                x = bx + a;
                y = by + b;
                cur.push(pos2(x, y));
                last_c = None;
            }
            'H' => {
                let Some(a) = num(&mut i) else { break };
                x = bx + a;
                cur.push(pos2(x, y));
                last_c = None;
            }
            'V' => {
                let Some(b) = num(&mut i) else { break };
                y = (if rel { y } else { 0.0 }) + b;
                cur.push(pos2(x, y));
                last_c = None;
            }
            'C' | 'S' => {
                let smooth = cmd.to_ascii_uppercase() == 'S';
                let (c1x, c1y) = if smooth {
                    match last_c {
                        Some((lx, ly)) => (2.0 * x - lx, 2.0 * y - ly),
                        None => (x, y),
                    }
                } else {
                    let (Some(a), Some(b)) = (num(&mut i), num(&mut i)) else {
                        break;
                    };
                    (bx + a, by + b)
                };
                let (Some(c), Some(d2), Some(e), Some(f)) =
                    (num(&mut i), num(&mut i), num(&mut i), num(&mut i))
                else {
                    break;
                };
                let (c2x, c2y, ex, ey) = (bx + c, by + d2, bx + e, by + f);
                if cur.is_empty() {
                    cur.push(pos2(x, y));
                }
                for k in 1..=12 {
                    let t = k as f32 / 12.0;
                    let u = 1.0 - t;
                    let px = u * u * u * x
                        + 3.0 * u * u * t * c1x
                        + 3.0 * u * t * t * c2x
                        + t * t * t * ex;
                    let py = u * u * u * y
                        + 3.0 * u * u * t * c1y
                        + 3.0 * u * t * t * c2y
                        + t * t * t * ey;
                    cur.push(pos2(px, py));
                }
                last_c = Some((c2x, c2y));
                x = ex;
                y = ey;
            }
            'Q' | 'T' => {
                let smooth = cmd.to_ascii_uppercase() == 'T';
                let (qx, qy) = if smooth {
                    match last_c {
                        Some((lx, ly)) => (2.0 * x - lx, 2.0 * y - ly),
                        None => (x, y),
                    }
                } else {
                    let (Some(a), Some(b)) = (num(&mut i), num(&mut i)) else {
                        break;
                    };
                    (bx + a, by + b)
                };
                let (Some(e), Some(f)) = (num(&mut i), num(&mut i)) else {
                    break;
                };
                let (ex, ey) = (bx + e, by + f);
                if cur.is_empty() {
                    cur.push(pos2(x, y));
                }
                for k in 1..=10 {
                    let t = k as f32 / 10.0;
                    let u = 1.0 - t;
                    cur.push(pos2(
                        u * u * x + 2.0 * u * t * qx + t * t * ex,
                        u * u * y + 2.0 * u * t * qy + t * t * ey,
                    ));
                }
                last_c = Some((qx, qy));
                x = ex;
                y = ey;
            }
            'A' => {
                let (Some(rx), Some(ry), Some(rot), Some(large), Some(sweep), Some(e), Some(f)) = (
                    num(&mut i),
                    num(&mut i),
                    num(&mut i),
                    num(&mut i),
                    num(&mut i),
                    num(&mut i),
                    num(&mut i),
                ) else {
                    break;
                };
                let (ex, ey) = (bx + e, by + f);
                if cur.is_empty() {
                    cur.push(pos2(x, y));
                }
                arc(
                    &mut cur,
                    (x, y),
                    (ex, ey),
                    rx,
                    ry,
                    rot,
                    large != 0.0,
                    sweep != 0.0,
                );
                x = ex;
                y = ey;
                last_c = None;
            }
            _ => {
                i += 1;
            }
        }
    }
    flush(&mut cur, &mut out, false);
    out
}

/// An SVG elliptical arc as points (endpoint to centre parameterisation, SVG spec F.6.5).
#[allow(clippy::too_many_arguments)]
fn arc(
    out: &mut Vec<Pos2>,
    (x1, y1): (f32, f32),
    (x2, y2): (f32, f32),
    rx: f32,
    ry: f32,
    rot_deg: f32,
    large: bool,
    sweep: bool,
) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-6 || ry < 1e-6 || ((x1 - x2).abs() < 1e-6 && (y1 - y2).abs() < 1e-6) {
        out.push(pos2(x2, y2));
        return;
    }
    let phi = rot_deg.to_radians();
    let (cp, sp) = (phi.cos(), phi.sin());
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cp * dx + sp * dy;
    let y1p = -sp * dx + cp * dy;
    let lam = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lam > 1.0 {
        rx *= lam.sqrt();
        ry *= lam.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut co = if den > 0.0 {
        (num / den).max(0.0).sqrt()
    } else {
        0.0
    };
    if large == sweep {
        co = -co;
    }
    let cxp = co * rx * y1p / ry;
    let cyp = -co * ry * x1p / rx;
    let cx = cp * cxp - sp * cyp + (x1 + x2) / 2.0;
    let cy = sp * cxp + cp * cyp + (y1 + y2) / 2.0;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        a
    };
    let t1 = ang(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dt = ang(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    if !sweep && dt > 0.0 {
        dt -= std::f32::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f32::consts::TAU;
    }
    let steps = ((dt.abs() / std::f32::consts::TAU) * 32.0).ceil().max(2.0) as usize;
    for k in 1..=steps {
        let t = t1 + dt * k as f32 / steps as f32;
        let (ct, st) = (t.cos(), t.sin());
        out.push(pos2(
            cx + rx * ct * cp - ry * st * sp,
            cy + rx * ct * sp + ry * st * cp,
        ));
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok {
    Cmd(char),
    Num(f32),
}

fn tokens(d: &str) -> Vec<Tok> {
    let b = d.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            out.push(Tok::Cmd(c));
            i += 1;
        } else if c == '-' || c == '.' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            let mut dot = c == '.';
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == '.' && !dot {
                    dot = true;
                    i += 1;
                } else {
                    break;
                }
            }
            if let Ok(v) = d[start..i].parse::<f32>() {
                out.push(Tok::Num(v));
            }
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_parse_into_lines_arcs_and_curves() {
        let p = parse("M8 3 4 7l4 4");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].0.len(), 3);
        assert_eq!(p[0].0[2], pos2(8.0, 11.0));
        // packed numbers: "1.5-2.9" and ".81"
        let p = parse("m6 14 1.5-2.9l.81 1.2");
        assert!((p[0].0[1].x - 7.5).abs() < 1e-4 && (p[0].0[1].y - 11.1).abs() < 1e-4);
        assert!((p[0].0[2].x - 8.31).abs() < 1e-4);
        // a half circle arc ends where it should
        let p = parse("M2 12a10 10 0 0 1 20 0");
        let last = *p[0].0.last().unwrap();
        assert!((last.x - 22.0).abs() < 1e-3 && (last.y - 12.0).abs() < 1e-3);
        // every built-in icon parses
        for d in [
            &IO,
            &SENDS,
            &EQ,
            &METRONOME,
            &STRETCH,
            &MAGNET,
            &AUTOSCROLL,
            &FOLDER_OPEN,
            &FILE_PLUS,
            &INSERT,
            &TRASH,
            &LOOP,
            &AUTOPLAY,
            &EYE,
            &EYE_OFF,
        ] {
            for path in d.paths {
                assert!(!parse(path).is_empty(), "{path}");
            }
        }
    }
}
