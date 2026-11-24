//! Visual style: palette, egui style setup, painted icons and panel tab headers.

use egui::{Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};

/// The two built-in colour schemes. Track clip colours live in `session::TRACK_COLORS` and stay
/// the same in both: the reference picture uses the usual clip colours on dark surfaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub panel_header: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub wave_bg: Color32,
    pub wave: Color32,
    pub zero_line: Color32,
    pub wave_sel_bg: Color32,
    pub playhead: Color32,
    /// The snapping indicator line: the theme's contrast colour for it — a vivid red that is
    /// light on dark themes and deep on light ones, so the vertical line a dragged edge will
    /// snap to always stands out from the timeline.
    pub snap_indicator: Color32,
    pub time_blue: Color32,
    pub marker: Color32,
    pub ruler: Color32,
    pub track_head: Color32,
    pub track_head_sel: Color32,
    pub track_lane_a: Color32,
    pub track_lane_b: Color32,
    pub auto_lane: Color32,
    pub track_divider: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub selected: Color32,
    pub power_on: Color32,
    pub control: Color32,
    pub menu_fill: Color32,
    pub menu_edge: Color32,
    pub menu_hover: Color32,
    pub menu_pressed: Color32,
    pub menu_open: Color32,
    pub primary: Color32,
}

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(29, 29, 29),
    panel: Color32::from_rgb(35, 35, 35),
    panel_header: Color32::from_rgb(27, 27, 27),
    text: Color32::from_rgb(240, 240, 240),
    text_dim: Color32::from_rgb(170, 170, 170),
    accent: Color32::from_rgb(45, 140, 235),
    wave_bg: Color32::from_rgb(30, 30, 30),
    wave: Color32::from_rgb(106, 220, 180),
    zero_line: Color32::from_rgb(215, 38, 44),
    wave_sel_bg: Color32::from_rgb(55, 105, 95),
    playhead: Color32::from_rgb(235, 64, 80),
    snap_indicator: Color32::from_rgb(255, 64, 64),
    time_blue: Color32::from_rgb(78, 168, 255),
    marker: Color32::from_rgb(240, 200, 70),
    ruler: Color32::from_rgb(40, 40, 40),
    track_head: Color32::from_rgb(44, 44, 44),
    track_head_sel: Color32::from_rgb(56, 60, 66),
    track_lane_a: Color32::from_rgb(26, 26, 26),
    track_lane_b: Color32::from_rgb(31, 31, 31),
    auto_lane: Color32::from_rgb(44, 44, 46),
    track_divider: Color32::from_rgb(72, 72, 72),
    hover: Color32::from_rgb(44, 72, 108),
    pressed: Color32::from_rgb(34, 62, 98),
    selected: Color32::from_rgb(38, 90, 150),
    power_on: Color32::from_rgb(49, 164, 126),
    control: Color32::from_rgb(48, 48, 48),
    menu_fill: Color32::from_rgb(40, 40, 40),
    menu_edge: Color32::from_rgb(66, 66, 66),
    menu_hover: Color32::from_rgb(62, 62, 62),
    menu_pressed: Color32::from_rgb(54, 54, 54),
    menu_open: Color32::from_rgb(70, 70, 70),
    primary: Color32::from_rgb(52, 108, 240),
};

/// A deeper charcoal version of the same layout, sampled from the supplied Audition picture:
/// near-black panel backgrounds, quiet grey text and the same blue time/transport accents.
const DARK: Palette = Palette {
    bg: Color32::from_rgb(22, 22, 22),
    panel: Color32::from_rgb(28, 28, 28),
    panel_header: Color32::from_rgb(20, 20, 20),
    text: Color32::from_rgb(232, 232, 232),
    text_dim: Color32::from_rgb(145, 145, 145),
    accent: Color32::from_rgb(45, 135, 220),
    wave_bg: Color32::from_rgb(20, 20, 20),
    wave: Color32::from_rgb(106, 220, 180),
    zero_line: Color32::from_rgb(215, 38, 44),
    wave_sel_bg: Color32::from_rgb(43, 85, 78),
    playhead: Color32::from_rgb(235, 64, 80),
    snap_indicator: Color32::from_rgb(255, 64, 64),
    time_blue: Color32::from_rgb(78, 168, 255),
    marker: Color32::from_rgb(240, 200, 70),
    ruler: Color32::from_rgb(32, 32, 32),
    track_head: Color32::from_rgb(38, 38, 38),
    track_head_sel: Color32::from_rgb(50, 54, 60),
    track_lane_a: Color32::from_rgb(20, 20, 20),
    track_lane_b: Color32::from_rgb(24, 24, 24),
    auto_lane: Color32::from_rgb(36, 36, 38),
    track_divider: Color32::from_rgb(64, 64, 64),
    hover: Color32::from_rgb(36, 60, 92),
    pressed: Color32::from_rgb(28, 52, 84),
    selected: Color32::from_rgb(32, 78, 130),
    power_on: Color32::from_rgb(43, 145, 112),
    control: Color32::from_rgb(40, 40, 40),
    menu_fill: Color32::from_rgb(32, 32, 32),
    menu_edge: Color32::from_rgb(58, 58, 58),
    menu_hover: Color32::from_rgb(52, 52, 52),
    menu_pressed: Color32::from_rgb(46, 46, 46),
    menu_open: Color32::from_rgb(60, 60, 60),
    primary: Color32::from_rgb(44, 96, 210),
};

/// The surface colours a Craft-suite theme (PhotoCraft, VectorCraft; MIT OR Apache-2.0) is drawn
/// with, mapped onto AudioCraft's palette by [`craft_palette`].
struct Craft {
    dark: bool,
    chrome: (u8, u8, u8),
    canvas: (u8, u8, u8),
    card: (u8, u8, u8),
    border: (u8, u8, u8),
    field: (u8, u8, u8),
    field_border: (u8, u8, u8),
    hover: (u8, u8, u8),
    pressed: (u8, u8, u8),
    text: (u8, u8, u8),
    text_dim: (u8, u8, u8),
    accent: (u8, u8, u8),
    tab_strip: (u8, u8, u8),
}

const fn hex(v: u32) -> (u8, u8, u8) {
    ((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn rgb(c: (u8, u8, u8)) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

fn craft_palette(c: &Craft) -> Palette {
    let card = rgb(c.card);
    let canvas = rgb(c.canvas);
    let accent = rgb(c.accent);
    let toward = |a: Color32, b: Color32, t: f32| {
        let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
    };
    // the timeline is a little off the canvas so the alternating lanes still show
    let shade = if c.dark {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    Palette {
        bg: rgb(c.chrome),
        panel: card,
        panel_header: rgb(c.tab_strip),
        text: rgb(c.text),
        text_dim: rgb(c.text_dim),
        accent,
        wave_bg: canvas,
        wave: if c.dark {
            Color32::from_rgb(106, 220, 180)
        } else {
            Color32::from_rgb(18, 140, 100)
        },
        zero_line: Color32::from_rgb(215, 38, 44),
        wave_sel_bg: toward(canvas, accent, if c.dark { 0.3 } else { 0.25 }),
        playhead: Color32::from_rgb(235, 64, 80),
        // the snapping indicator follows the theme's contrast: a bright red on dark canvases,
        // a deep red on light ones
        snap_indicator: if c.dark {
            Color32::from_rgb(255, 64, 64)
        } else {
            Color32::from_rgb(158, 12, 24)
        },
        time_blue: if c.dark {
            toward(accent, Color32::WHITE, 0.2)
        } else {
            toward(accent, Color32::BLACK, 0.25)
        },
        marker: if c.dark {
            Color32::from_rgb(240, 200, 70)
        } else {
            Color32::from_rgb(190, 130, 10)
        },
        ruler: rgb(c.chrome),
        track_head: card,
        track_head_sel: toward(card, accent, 0.18),
        track_lane_a: canvas,
        track_lane_b: toward(canvas, shade, 0.03),
        auto_lane: rgb(c.field),
        track_divider: rgb(c.field_border),
        hover: rgb(c.hover),
        pressed: rgb(c.pressed),
        selected: toward(card, accent, if c.dark { 0.5 } else { 0.3 }),
        power_on: Color32::from_rgb(49, 164, 126),
        control: rgb(c.field),
        menu_fill: card,
        menu_edge: rgb(c.border),
        menu_hover: rgb(c.hover),
        menu_pressed: rgb(c.pressed),
        menu_open: rgb(c.pressed),
        primary: accent,
    }
}

/// Every theme as (name, palette source); index 0 and 1 are AudioCraft's own and keep their
/// numbers, so saved preferences still pick the same theme.
const CRAFT_THEMES: [(&str, Craft); 22] = [
    // PhotoCraft (crates/ui-egui/src/theme.rs)
    (
        "PhotoCraft Pro (Dark)",
        Craft {
            dark: true,
            chrome: (50, 50, 50),
            canvas: (40, 40, 40),
            card: (50, 50, 50),
            border: (30, 30, 30),
            field: (36, 36, 36),
            field_border: (74, 74, 74),
            hover: (66, 66, 66),
            pressed: (78, 78, 78),
            text: (222, 222, 222),
            text_dim: (178, 178, 178),
            accent: (55, 142, 240),
            tab_strip: (38, 38, 38),
        },
    ),
    (
        "PhotoCraft Pro (Medium Gray)",
        Craft {
            dark: true,
            chrome: (83, 83, 83),
            canvas: (40, 40, 40),
            card: (83, 83, 83),
            border: (66, 66, 66),
            field: (69, 69, 69),
            field_border: (104, 104, 104),
            hover: (98, 98, 98),
            pressed: (112, 112, 112),
            text: (238, 238, 238),
            text_dim: (212, 212, 212),
            accent: (55, 142, 240),
            tab_strip: (74, 74, 74),
        },
    ),
    (
        "PhotoCraft Pro (Light Gray)",
        Craft {
            dark: false,
            chrome: (184, 184, 184),
            canvas: (160, 160, 160),
            card: (184, 184, 184),
            border: (160, 160, 160),
            field: (214, 214, 214),
            field_border: (140, 140, 140),
            hover: (198, 198, 198),
            pressed: (170, 170, 170),
            text: (20, 20, 20),
            text_dim: (52, 52, 52),
            accent: (55, 142, 240),
            tab_strip: (170, 170, 170),
        },
    ),
    (
        "PhotoCraft Pro (Light)",
        Craft {
            dark: false,
            chrome: (240, 240, 240),
            canvas: (214, 214, 214),
            card: (240, 240, 240),
            border: (214, 214, 214),
            field: (255, 255, 255),
            field_border: (186, 186, 186),
            hover: (226, 226, 226),
            pressed: (206, 206, 206),
            text: (20, 20, 20),
            text_dim: (64, 64, 64),
            accent: (55, 142, 240),
            tab_strip: (226, 226, 226),
        },
    ),
    (
        "PhotoCraft Studio (Dark)",
        Craft {
            dark: true,
            chrome: (20, 20, 21),
            canvas: (14, 14, 15),
            card: (26, 26, 28),
            border: (40, 40, 44),
            field: (35, 35, 38),
            field_border: (52, 52, 57),
            hover: (44, 44, 48),
            pressed: (56, 56, 62),
            text: (236, 236, 240),
            text_dim: (150, 150, 158),
            accent: (139, 124, 246),
            tab_strip: (17, 17, 18),
        },
    ),
    (
        "PhotoCraft Studio (Light)",
        Craft {
            dark: false,
            chrome: (246, 246, 248),
            canvas: (226, 226, 230),
            card: (252, 252, 253),
            border: (222, 222, 228),
            field: (242, 242, 245),
            field_border: (214, 214, 220),
            hover: (232, 232, 237),
            pressed: (220, 220, 226),
            text: (24, 24, 28),
            text_dim: (96, 96, 106),
            accent: (108, 92, 231),
            tab_strip: (240, 240, 243),
        },
    ),
    (
        "PhotoCraft Classic",
        Craft {
            dark: false,
            chrome: (212, 208, 200),
            canvas: (128, 128, 128),
            card: (212, 208, 200),
            border: (128, 128, 128),
            field: (255, 255, 255),
            field_border: (128, 128, 128),
            hover: (226, 222, 214),
            pressed: (190, 186, 178),
            text: (0, 0, 0),
            text_dim: (64, 64, 64),
            accent: (10, 36, 106),
            tab_strip: (212, 208, 200),
        },
    ),
    (
        "PhotoCraft Solarized Dark",
        Craft {
            dark: true,
            chrome: (7, 54, 66),
            canvas: (0, 43, 54),
            card: (7, 54, 66),
            border: (0, 37, 47),
            field: (0, 37, 47),
            field_border: (21, 80, 95),
            hover: (14, 74, 90),
            pressed: (18, 89, 107),
            text: (131, 148, 150),
            text_dim: (101, 123, 131),
            accent: (38, 139, 210),
            tab_strip: (0, 37, 47),
        },
    ),
    (
        "PhotoCraft Adwaita (Light)",
        Craft {
            dark: false,
            chrome: (255, 255, 255),
            canvas: (222, 222, 223),
            card: (255, 255, 255),
            border: (218, 218, 221),
            field: (235, 235, 237),
            field_border: (218, 218, 221),
            hover: (226, 226, 229),
            pressed: (212, 212, 216),
            text: (50, 50, 54),
            text_dim: (100, 100, 104),
            accent: (53, 132, 228),
            tab_strip: (235, 235, 237),
        },
    ),
    (
        "PhotoCraft Adwaita (Dark)",
        Craft {
            dark: true,
            chrome: (46, 46, 50),
            canvas: (29, 29, 32),
            card: (54, 54, 58),
            border: (64, 64, 68),
            field: (64, 64, 68),
            field_border: (76, 76, 80),
            hover: (72, 72, 76),
            pressed: (86, 86, 90),
            text: (255, 255, 255),
            text_dim: (192, 192, 194),
            accent: (53, 132, 228),
            tab_strip: (34, 34, 38),
        },
    ),
    (
        "PhotoCraft Breeze Light",
        Craft {
            dark: false,
            chrome: (219, 222, 226),
            canvas: (219, 222, 226),
            card: (236, 238, 241),
            border: (203, 208, 213),
            field: (249, 250, 252),
            field_border: (184, 191, 198),
            hover: (226, 230, 235),
            pressed: (208, 214, 221),
            text: (33, 37, 42),
            text_dim: (54, 63, 72),
            accent: (58, 172, 232),
            tab_strip: (224, 227, 231),
        },
    ),
    (
        "PhotoCraft Breeze Dark",
        Craft {
            dark: true,
            chrome: (38, 42, 48),
            canvas: (17, 20, 24),
            card: (29, 33, 38),
            border: (52, 56, 61),
            field: (38, 42, 48),
            field_border: (68, 73, 79),
            hover: (50, 54, 59),
            pressed: (58, 63, 69),
            text: (250, 251, 253),
            text_dim: (200, 206, 213),
            accent: (58, 172, 232),
            tab_strip: (26, 29, 34),
        },
    ),
    (
        "PhotoCraft Soft Dark",
        Craft {
            dark: true,
            chrome: (58, 58, 68),
            canvas: (40, 40, 48),
            card: (54, 54, 64),
            border: (70, 70, 82),
            field: (64, 64, 75),
            field_border: (84, 84, 98),
            hover: (68, 68, 80),
            pressed: (74, 74, 88),
            text: (238, 238, 246),
            text_dim: (212, 212, 224),
            accent: (138, 154, 232),
            tab_strip: (47, 47, 56),
        },
    ),
    (
        "PhotoCraft Soft Light",
        Craft {
            dark: false,
            chrome: (232, 230, 236),
            canvas: (218, 216, 224),
            card: (240, 238, 243),
            border: (208, 205, 215),
            field: (250, 249, 252),
            field_border: (190, 187, 201),
            hover: (228, 226, 234),
            pressed: (212, 209, 221),
            text: (44, 42, 56),
            text_dim: (62, 60, 76),
            accent: (110, 128, 216),
            tab_strip: (226, 224, 231),
        },
    ),
    // VectorCraft (crates/ui-egui/src/theme.rs): Modern scheme with its coral accent, and Classic
    (
        "VectorCraft Dark",
        Craft {
            dark: true,
            chrome: hex(0x0b0d12),
            canvas: hex(0x101217),
            card: hex(0x181b22),
            border: hex(0x06070a),
            field: hex(0x0f1116),
            field_border: hex(0x2c323e),
            hover: hex(0x262b35),
            pressed: hex(0x2f3541),
            text: hex(0xdfe4ec),
            text_dim: hex(0x969faf),
            accent: hex(0xf06a52),
            tab_strip: hex(0x0f1116),
        },
    ),
    (
        "VectorCraft Charcoal",
        Craft {
            dark: true,
            chrome: hex(0x1b1f27),
            canvas: hex(0x1e222a),
            card: hex(0x272c36),
            border: hex(0x11141a),
            field: hex(0x1a1e26),
            field_border: hex(0x3a414e),
            hover: hex(0x353c49),
            pressed: hex(0x3d4553),
            text: hex(0xe4e9f1),
            text_dim: hex(0x99a2b2),
            accent: hex(0xf06a52),
            tab_strip: hex(0x1e222a),
        },
    ),
    (
        "VectorCraft Medium Light",
        Craft {
            dark: false,
            chrome: hex(0xc4cad4),
            canvas: hex(0xb9c0cb),
            card: hex(0xd3d8e0),
            border: hex(0xa9b0bc),
            field: hex(0xe9ecf1),
            field_border: hex(0x9aa2b0),
            hover: hex(0xc5ccd6),
            pressed: hex(0xb6bec9),
            text: hex(0x1b1f27),
            text_dim: hex(0x4a5160),
            accent: hex(0xd9472f),
            tab_strip: hex(0xc4cad4),
        },
    ),
    (
        "VectorCraft Light",
        Craft {
            dark: false,
            chrome: hex(0xe6e9ee),
            canvas: hex(0xdfe3e9),
            card: hex(0xf3f5f8),
            border: hex(0xd0d5dd),
            field: hex(0xffffff),
            field_border: hex(0xc3c9d3),
            hover: hex(0xe3e7ed),
            pressed: hex(0xd5dbe3),
            text: hex(0x1b1f27),
            text_dim: hex(0x58606e),
            accent: hex(0xd9472f),
            tab_strip: hex(0xe6e9ee),
        },
    ),
    (
        "VectorCraft Classic (Dark)",
        Craft {
            dark: true,
            chrome: hex(0x262626),
            canvas: hex(0x1e1e1e),
            card: hex(0x323232),
            border: hex(0x1b1b1b),
            field: hex(0x1f1f1f),
            field_border: hex(0x464646),
            hover: hex(0x444444),
            pressed: hex(0x1d1d1d),
            text: hex(0xd8d8d8),
            text_dim: hex(0xa8a8a8),
            accent: hex(0x378ef0),
            tab_strip: hex(0x282828),
        },
    ),
    (
        "VectorCraft Classic (Medium Dark)",
        Craft {
            dark: true,
            chrome: hex(0x464646),
            canvas: hex(0x333333),
            card: hex(0x535353),
            border: hex(0x383838),
            field: hex(0x454545),
            field_border: hex(0x5f5f5f),
            hover: hex(0x606060),
            pressed: hex(0x303030),
            text: hex(0xd1d1d1),
            text_dim: hex(0xb0b0b0),
            accent: hex(0x378ef0),
            tab_strip: hex(0x424242),
        },
    ),
    (
        "VectorCraft Classic (Medium Light)",
        Craft {
            dark: false,
            chrome: hex(0xa9a9a9),
            canvas: hex(0xa0a0a0),
            card: hex(0xb8b8b8),
            border: hex(0x969696),
            field: hex(0xd6d6d6),
            field_border: hex(0x8c8c8c),
            hover: hex(0xc8c8c8),
            pressed: hex(0x969696),
            text: hex(0x1f1f1f),
            text_dim: hex(0x3c3c3c),
            accent: hex(0x1473e6),
            tab_strip: hex(0xa6a6a6),
        },
    ),
    (
        "VectorCraft Classic (Light)",
        Craft {
            dark: false,
            chrome: hex(0xe4e4e4),
            canvas: hex(0xdcdcdc),
            card: hex(0xf0f0f0),
            border: hex(0xcfcfcf),
            field: hex(0xffffff),
            field_border: hex(0xb4b4b4),
            hover: hex(0xdddddd),
            pressed: hex(0xcdcdcd),
            text: hex(0x1f1f1f),
            text_dim: hex(0x4b4b4b),
            accent: hex(0x1473e6),
            tab_strip: hex(0xdddddd),
        },
    ),
];

struct ThemeEntry {
    name: &'static str,
    dark: bool,
    palette: Palette,
    /// Body text is under 7:1 against the panels (still at least 4.5:1): shown on its card.
    low_contrast: bool,
}

/// Relative luminance (WCAG 2) of an sRGB colour.
fn luminance(c: Color32) -> f32 {
    let f = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
}

/// WCAG contrast ratio of two colours (1 to 21).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Move `fg` towards white or black (whichever is further from `bg`) until it reaches `min`
/// contrast against every colour in `bgs`.
fn lift(fg: Color32, bgs: &[Color32], min: f32, dark: bool) -> Color32 {
    let target = if dark { Color32::WHITE } else { Color32::BLACK };
    let ok = |c: Color32| bgs.iter().all(|b| contrast(c, *b) >= min);
    if ok(fg) {
        return fg;
    }
    for k in 1..=20 {
        let c = lerp_color(fg, target, k as f32 / 20.0);
        if ok(c) {
            return c;
        }
    }
    target
}

/// Readable text in every theme: body text at least 4.5:1 and dim text at least 3:1 against
/// the panels, headers, controls and hover rows they are drawn on.
fn with_contrast(mut p: Palette, dark: bool) -> (Palette, bool) {
    let surfaces = [p.panel, p.panel_header, p.bg, p.control, p.track_head];
    p.text = lift(p.text, &surfaces, 4.5, dark);
    p.text_dim = lift(p.text_dim, &surfaces, 3.0, dark);
    // the snapping indicator keeps its red but stays clearly visible on the timeline
    p.snap_indicator = lift(
        p.snap_indicator,
        &[p.wave_bg, p.track_lane_a, p.track_lane_b],
        3.0,
        dark,
    );
    let low = contrast(p.text, p.panel) < 7.0;
    (p, low)
}

fn themes() -> &'static [ThemeEntry] {
    static ALL: std::sync::LazyLock<Vec<ThemeEntry>> = std::sync::LazyLock::new(|| {
        let mut v = vec![
            ThemeEntry {
                name: "AudioCraft",
                dark: true,
                palette: LIGHT,
                low_contrast: false,
            },
            ThemeEntry {
                name: "Audition Dark",
                dark: true,
                palette: DARK,
                low_contrast: false,
            },
        ];
        for (name, c) in CRAFT_THEMES.iter() {
            let (palette, low_contrast) = with_contrast(craft_palette(c), c.dark);
            v.push(ThemeEntry {
                name,
                dark: c.dark,
                palette,
                low_contrast,
            });
        }
        v
    });
    &ALL
}

/// Whether theme `i`'s body text is lower contrast (under 7:1; every theme keeps at least 4.5:1).
pub fn theme_low_contrast(i: usize) -> bool {
    themes().get(i).map(|t| t.low_contrast).unwrap_or(false)
}

/// Body-text and dim-text contrast of theme `i` against its panels.
pub fn theme_contrast(i: usize) -> (f32, f32) {
    themes()
        .get(i)
        .map(|t| {
            (
                contrast(t.palette.text, t.palette.panel),
                contrast(t.palette.text_dim, t.palette.panel),
            )
        })
        .unwrap_or((21.0, 21.0))
}

/// Whether theme `i` has dark surfaces.
pub fn theme_is_dark(i: usize) -> bool {
    themes().get(i).map(|t| t.dark).unwrap_or(true)
}

/// A clickable preview card of theme `i` (as in PhotoCraft's Appearance settings): a tiny
/// window in the theme's colours (title strip, a side panel, a waveform lane with the accent)
/// and the theme's name under it; the chosen one has an accent outline.
pub fn theme_card(ui: &mut egui::Ui, i: usize, chosen: bool) -> egui::Response {
    let Some(t) = themes().get(i) else {
        return ui.allocate_response(vec2(0.0, 0.0), Sense::hover());
    };
    let pal = &t.palette;
    let (r, resp) = ui.allocate_exact_size(vec2(124.0, 98.0), Sense::click());
    let p = ui.painter();
    let pic = Rect::from_min_size(r.min, vec2(r.width(), 70.0));
    let outline = if chosen {
        Stroke::new(2.0, accent())
    } else if resp.hovered() {
        Stroke::new(1.0, text_dim())
    } else {
        Stroke::new(1.0, menu_edge())
    };
    p.rect_filled(pic, 6.0, pal.bg);
    let inner = pic.shrink(4.0);
    // title strip with three dots
    let bar = Rect::from_min_size(inner.min, vec2(inner.width(), 8.0));
    p.rect_filled(bar, 2.0, pal.panel_header);
    for k in 0..3 {
        p.circle_filled(
            pos2(bar.left() + 5.0 + k as f32 * 6.0, bar.center().y),
            1.6,
            pal.text_dim,
        );
    }
    // side panel with text lines and a selected row
    let side = Rect::from_min_max(
        pos2(inner.left(), bar.bottom() + 3.0),
        pos2(inner.left() + 34.0, inner.bottom()),
    );
    p.rect_filled(side, 2.0, pal.panel);
    for k in 0..4 {
        let y = side.top() + 6.0 + k as f32 * 9.0;
        let row = Rect::from_min_size(
            pos2(side.left() + 3.0, y - 3.0),
            vec2(side.width() - 6.0, 6.0),
        );
        if k == 1 {
            p.rect_filled(row, 1.0, pal.selected);
        }
        p.line_segment(
            [
                pos2(row.left() + 2.0, y),
                pos2(row.right() - 6.0 - k as f32 * 3.0, y),
            ],
            Stroke::new(1.5, if k == 1 { pal.text } else { pal.text_dim }),
        );
    }
    // editor: a lane with a waveform and the accent playhead
    let ed = Rect::from_min_max(pos2(side.right() + 3.0, side.top()), inner.max);
    p.rect_filled(ed, 2.0, pal.wave_bg);
    let mid = ed.center().y;
    let mut x = ed.left() + 3.0;
    let mut k = 0.0f32;
    while x < ed.right() - 3.0 {
        let a = ((k * 0.9).sin().abs() * 0.7 + 0.15) * ed.height() * 0.38;
        p.line_segment(
            [pos2(x, mid - a), pos2(x, mid + a)],
            Stroke::new(1.0, pal.wave),
        );
        x += 2.0;
        k += 1.0;
    }
    let px = ed.left() + ed.width() * 0.62;
    p.line_segment(
        [pos2(px, ed.top()), pos2(px, ed.bottom())],
        Stroke::new(1.5, pal.accent),
    );
    p.rect_stroke(pic, 6.0, outline, egui::StrokeKind::Outside);
    // the name under the picture, and which app it comes from
    let name = t
        .name
        .trim_start_matches("PhotoCraft ")
        .trim_start_matches("VectorCraft ");
    let maker = if t.name.starts_with("PhotoCraft") {
        "PhotoCraft"
    } else if t.name.starts_with("VectorCraft") {
        "VectorCraft"
    } else {
        "AudioCraft"
    };
    p.text(
        pos2(r.center().x, pic.bottom() + 5.0),
        egui::Align2::CENTER_TOP,
        name,
        FontId::proportional(11.5),
        if chosen { strong() } else { text() },
    );
    let (note, note_col) = if t.low_contrast {
        (
            format!("{maker} \u{2022} Lower contrast"),
            Color32::from_rgb(232, 176, 70),
        )
    } else {
        (maker.to_string(), text_dim())
    };
    p.text(
        pos2(r.center().x, pic.bottom() + 18.0),
        egui::Align2::CENTER_TOP,
        note,
        FontId::proportional(9.5),
        note_col,
    );
    if t.low_contrast {
        p.circle_filled(
            pic.right_top() + vec2(-8.0, 8.0),
            4.0,
            Color32::from_rgb(232, 176, 70),
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let (body, dim) = theme_contrast(i);
    resp.on_hover_text(format!(
        "{}\nText contrast {body:.1}:1, dim text {dim:.1}:1{}",
        t.name,
        if t.low_contrast {
            "\nLower contrast: text is softer than in the other themes (still readable)."
        } else {
            ""
        }
    ))
}

/// How many themes there are (AudioCraft's two, then PhotoCraft's and VectorCraft's).
pub fn theme_count() -> usize {
    themes().len()
}

/// The name of theme `i` (the first theme for an unknown index).
pub fn theme_name(i: usize) -> &'static str {
    themes().get(i).unwrap_or(&themes()[0]).name
}

static THEME: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Preferences call this on start-up and whenever the theme changes; unknown indexes fall back
/// to the default theme.
pub fn set_theme(kind: u8) {
    let k = if (kind as usize) < theme_count() {
        kind
    } else {
        0
    };
    THEME.store(k, std::sync::atomic::Ordering::Relaxed);
}

pub fn theme_kind() -> u8 {
    THEME.load(std::sync::atomic::Ordering::Relaxed)
}

/// `own` for AudioCraft's two themes (which keep their exact greys), the theme's colour otherwise.
pub fn own_or(own: Color32, other: Color32) -> Color32 {
    if theme_kind() <= 1 {
        // AudioCraft's own surfaces, brightened and saturated like the rest of the palette
        let (b, s) = adjust_now();
        if adjusting() {
            saturate(brighten(own, b), s)
        } else {
            own
        }
    } else {
        other
    }
}

/// Whether the theme shown has dark surfaces (light themes take dark text).
pub fn is_dark() -> bool {
    if adjusting() {
        return luminance(palette().panel) < DARK_SWITCH;
    }
    themes()
        .get(theme_kind() as usize)
        .map(|t| t.dark)
        .unwrap_or(true)
}

/// Text that has to stand out (hovered or chosen items, titles): white on dark themes,
/// near-black on light ones.
pub fn strong() -> Color32 {
    if is_dark() {
        Color32::WHITE
    } else {
        Color32::from_rgb(10, 10, 12)
    }
}

/// Appearance > Brightness (-1..1) and Saturation (0..1.5), stored as f32 bits.
static BRIGHTNESS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static SATURATION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);

/// Set Appearance > Brightness and Saturation (they change every theme's colours).
pub fn set_adjust(brightness: f32, saturation: f32) {
    use std::sync::atomic::Ordering::Relaxed;
    let b = if brightness.is_finite() {
        brightness.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let s = if saturation.is_finite() {
        saturation.clamp(0.0, 1.5)
    } else {
        1.0
    };
    BRIGHTNESS.store(b.to_bits(), Relaxed);
    SATURATION.store(s.to_bits(), Relaxed);
}

fn adjust_now() -> (f32, f32) {
    use std::sync::atomic::Ordering::Relaxed;
    (
        f32::from_bits(BRIGHTNESS.load(Relaxed)),
        f32::from_bits(SATURATION.load(Relaxed)),
    )
}

/// `c` with its colourfulness scaled by `s` (0 = grey, 1 = as is).
fn saturate(c: Color32, s: f32) -> Color32 {
    let l = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    let f = |v: u8| (l + (v as f32 - l) * s).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(f(c.r()), f(c.g()), f(c.b()), c.a())
}

/// The theme's palette with Brightness (surfaces toward white or black) and Saturation applied.
fn adjusted(base: &Palette, b: f32, s: f32) -> Palette {
    let mut p = *base;
    let light = |c: Color32| brighten(c, b);
    for c in [
        &mut p.bg,
        &mut p.panel,
        &mut p.panel_header,
        &mut p.wave_bg,
        &mut p.wave_sel_bg,
        &mut p.ruler,
        &mut p.track_head,
        &mut p.track_head_sel,
        &mut p.track_lane_a,
        &mut p.track_lane_b,
        &mut p.auto_lane,
        &mut p.track_divider,
        &mut p.hover,
        &mut p.pressed,
        &mut p.selected,
        &mut p.control,
        &mut p.menu_fill,
        &mut p.menu_edge,
        &mut p.menu_hover,
        &mut p.menu_pressed,
        &mut p.menu_open,
    ] {
        *c = light(*c);
    }
    for c in [
        &mut p.bg,
        &mut p.panel,
        &mut p.panel_header,
        &mut p.text,
        &mut p.text_dim,
        &mut p.accent,
        &mut p.wave_bg,
        &mut p.wave,
        &mut p.zero_line,
        &mut p.wave_sel_bg,
        &mut p.playhead,
        &mut p.time_blue,
        &mut p.marker,
        &mut p.ruler,
        &mut p.track_head,
        &mut p.track_head_sel,
        &mut p.track_lane_a,
        &mut p.track_lane_b,
        &mut p.auto_lane,
        &mut p.track_divider,
        &mut p.hover,
        &mut p.pressed,
        &mut p.selected,
        &mut p.power_on,
        &mut p.control,
        &mut p.menu_fill,
        &mut p.menu_edge,
        &mut p.menu_hover,
        &mut p.menu_pressed,
        &mut p.menu_open,
        &mut p.primary,
    ] {
        *c = saturate(*c, s);
    }
    // the text follows the surfaces: once they are light it turns dark (and the other way),
    // so every brightness stays readable
    let dark = luminance(p.panel) < DARK_SWITCH;
    let surfaces = [p.panel, p.panel_header, p.bg, p.control, p.track_head];
    p.text = lift(p.text, &surfaces, 4.5, dark);
    p.text_dim = lift(p.text_dim, &surfaces, 3.0, dark);
    // blue labels (time readout, active tabs, links) stay visible on the new surfaces too
    p.time_blue = lift(p.time_blue, &surfaces, 3.0, dark);
    p.accent = lift(p.accent, &[p.panel, p.panel_header, p.bg], 3.0, dark);
    // so does the snapping indicator on the timeline
    p.snap_indicator = lift(
        p.snap_indicator,
        &[p.wave_bg, p.track_lane_a, p.track_lane_b],
        3.0,
        dark,
    );
    p
}

/// Panel luminance below which text is light: at about 0.18 both white and black text reach
/// 4.5:1, so switching here keeps every brightness readable.
const DARK_SWITCH: f32 = 0.179;

/// Appearance > Brightness on one surface colour (-1 darker .. +1 much lighter).
/// A trim, not a theme change: at full it moves surfaces about a fifth of the way toward white
/// (or a quarter of the way toward black), so a dark theme stays dark and a light one stays light.
fn brighten(c: Color32, b: f32) -> Color32 {
    if b > 0.0 {
        lerp_color(c, Color32::WHITE, b * 0.18)
    } else {
        lerp_color(c, Color32::BLACK, -b * 0.25)
    }
}

/// Whether Brightness or Saturation is away from neutral.
fn adjusting() -> bool {
    let (b, s) = adjust_now();
    b.abs() >= 0.001 || (s - 1.0).abs() >= 0.001
}

fn palette() -> Palette {
    let base = &themes()
        .get(theme_kind() as usize)
        .unwrap_or(&themes()[0])
        .palette;
    let (b, s) = adjust_now();
    if b.abs() < 0.001 && (s - 1.0).abs() < 0.001 {
        *base
    } else {
        adjusted(base, b, s)
    }
}

/// A colour (for example a clip's) with Appearance > Saturation applied.
pub fn saturated(c: Color32) -> Color32 {
    let (_, s) = adjust_now();
    if (s - 1.0).abs() < 0.001 {
        c
    } else {
        saturate(c, s)
    }
}

pub fn bg() -> Color32 {
    palette().bg
}
pub fn panel() -> Color32 {
    palette().panel
}
pub fn panel_header() -> Color32 {
    palette().panel_header
}
pub fn text() -> Color32 {
    palette().text
}
pub fn text_dim() -> Color32 {
    palette().text_dim
}
pub fn accent() -> Color32 {
    palette().accent
}
pub fn wave_bg() -> Color32 {
    palette().wave_bg
}
pub fn wave() -> Color32 {
    palette().wave
}
pub fn zero_line() -> Color32 {
    palette().zero_line
}
pub fn wave_sel_bg() -> Color32 {
    palette().wave_sel_bg
}
pub fn playhead() -> Color32 {
    palette().playhead
}

/// The colour of the snapping point line: the theme's contrast colour for the snapping
/// indicator — a vivid red, light on dark themes and deep on light ones, so the vertical
/// line a dragged edge is about to snap to stands out from the timeline in every theme.
pub fn snap_line() -> Color32 {
    palette().snap_indicator
}
/// The pre-render gauge's colours, taken from the theme so the bar matches every theme: the
/// rendered part uses the theme's "on" colour (like a render bar's cached frames), the part
/// still rendering the theme's marker colour faded into the ruler, and the moving edge the
/// marker colour itself.
pub fn render_done() -> Color32 {
    palette().power_on
}
pub fn render_todo() -> Color32 {
    let p = palette();
    let m = |a: u8, b: u8| ((a as u16 * 2 + b as u16) / 3) as u8;
    Color32::from_rgb(
        m(p.ruler.r(), p.marker.r()),
        m(p.ruler.g(), p.marker.g()),
        m(p.ruler.b(), p.marker.b()),
    )
}
pub fn render_edge() -> Color32 {
    palette().marker
}
pub fn time_blue() -> Color32 {
    palette().time_blue
}
pub fn marker() -> Color32 {
    palette().marker
}
pub fn ruler() -> Color32 {
    palette().ruler
}
pub fn track_head() -> Color32 {
    palette().track_head
}
pub fn track_head_sel() -> Color32 {
    palette().track_head_sel
}
pub fn track_lane_a() -> Color32 {
    palette().track_lane_a
}
pub fn track_lane_b() -> Color32 {
    palette().track_lane_b
}
pub fn auto_lane() -> Color32 {
    palette().auto_lane
}
pub fn track_divider() -> Color32 {
    palette().track_divider
}
pub fn hover() -> Color32 {
    palette().hover
}
pub fn pressed() -> Color32 {
    palette().pressed
}
fn pressed_col() -> Color32 {
    palette().menu_pressed
}
pub fn selected() -> Color32 {
    palette().selected
}
pub fn power_on() -> Color32 {
    palette().power_on
}
pub fn control() -> Color32 {
    palette().control
}
pub fn menu_fill() -> Color32 {
    palette().menu_fill
}
pub fn menu_edge() -> Color32 {
    palette().menu_edge
}
pub fn menu_hover() -> Color32 {
    palette().menu_hover
}
pub fn menu_pressed() -> Color32 {
    palette().menu_pressed
}
pub fn menu_open() -> Color32 {
    palette().menu_open
}
pub fn primary() -> Color32 {
    palette().primary
}
/// Corner rounding of every control and popup.
pub const ROUND: u8 = 5;
/// Rounding of popups (menus, drop-down lists) and of floating windows and dialogs.
pub const POPUP_ROUND: u8 = 6;
pub const WINDOW_ROUND: u8 = 8;
/// The background of an icon or toggle button in its state.
pub fn control_fill(on: bool, on_col: Color32, resp: &egui::Response, framed: bool) -> Color32 {
    if on {
        on_col
    } else if resp.is_pointer_button_down_on() {
        pressed()
    } else if resp.hovered() {
        hover()
    } else if framed {
        control()
    } else {
        Color32::TRANSPARENT
    }
}

pub fn setup(ctx: &egui::Context) {
    let dark = is_dark();
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = panel();
    v.window_fill = menu_fill();
    // text boxes: AudioCraft's near-black wells, or the theme's field colour (also whenever
    // Brightness/Saturation are moved, so dark text never sits in a black box)
    v.extreme_bg_color = if theme_kind() <= 1 && !adjusting() {
        Color32::from_rgb(22, 22, 22)
    } else if is_dark() {
        lerp_color(control(), Color32::BLACK, 0.35)
    } else {
        lerp_color(control(), Color32::WHITE, 0.6)
    };
    v.faint_bg_color = menu_fill();
    v.window_stroke = Stroke::new(1.0, menu_edge());
    v.window_corner_radius = CornerRadius::same(WINDOW_ROUND);
    v.selection.bg_fill = if theme_kind() <= 1 && !adjusting() {
        Color32::from_rgb(38, 110, 190)
    } else {
        selected()
    };
    v.selection.stroke = Stroke::new(1.0, strong());
    v.hyperlink_color = accent();
    v.text_cursor.stroke = Stroke::new(2.0, strong());
    v.widgets.noninteractive.bg_fill = panel();
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, text());
    v.widgets.noninteractive.bg_stroke = Stroke::new(
        1.0,
        if theme_kind() <= 1 {
            Color32::from_rgb(50, 50, 50)
        } else {
            track_divider()
        },
    );
    v.widgets.inactive.bg_fill = control();
    v.widgets.inactive.weak_bg_fill = control();
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, text());
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    // under the pointer and pressed: quiet grey highlights (menus, buttons and list rows);
    // the painted transport/tool buttons keep their own blue tints
    v.widgets.hovered.bg_fill = menu_hover();
    v.widgets.hovered.weak_bg_fill = menu_hover();
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, strong());
    v.widgets.active.bg_fill = menu_pressed();
    v.widgets.active.weak_bg_fill = menu_pressed();
    v.widgets.active.bg_stroke = Stroke::NONE;
    v.widgets.active.fg_stroke = Stroke::new(1.5, strong());
    // a menu title or drop-down whose list is open: a slightly lighter grey, not a blue block
    v.widgets.open.bg_fill = menu_open();
    v.widgets.open.weak_bg_fill = menu_open();
    v.widgets.open.bg_stroke = Stroke::NONE;
    v.widgets.open.fg_stroke = Stroke::new(1.0, strong());
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(ROUND);
        w.expansion = 0.0;
    }
    // popups (menus, drop-down lists, right-click menus): one dark panel style
    v.menu_corner_radius = CornerRadius::same(POPUP_ROUND);
    v.window_fill = menu_fill();
    v.window_stroke = Stroke::new(1.0, menu_edge());
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = vec2(6.0, 4.0);
        s.spacing.button_padding = vec2(6.0, 2.0);
        // menus keep a gutter at the left for check marks and radio dots (see `check_item`),
        // so ticked and plain items line up in every menu
        s.spacing.menu_margin = egui::Margin {
            left: MENU_GUTTER as i8,
            right: 6,
            top: 4,
            bottom: 4,
        };
        s.spacing.interact_size.y = 20.0;
        // thin, always-present scroll bars beside the content (never floating over buttons)
        let mut sc = egui::style::ScrollStyle::solid();
        sc.bar_width = 6.0;
        sc.bar_inner_margin = 1.0;
        sc.bar_outer_margin = 1.0;
        s.spacing.scroll = sc;
        // labels are not selectable text (no accidental highlighting)
        s.interaction.selectable_labels = false;
        s.text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(12.5));
        s.text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(12.5));
        s.text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(10.5));
        s.text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(12.0));
    });
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Stop,
    Play,
    Pause,
    ToStart,
    Rewind,
    Forward,
    ToEnd,
    Record,
    Loop,
    ZoomInAmp,
    ZoomOutAmp,
    ZoomIn,
    ZoomOut,
    ZoomFull,
    ZoomSelIn,
    Move,
    /// Direct Selection: the white arrow.
    Direct,
    Razor,
    Slip,
    TimeSel,
    Marquee,
    Lasso,
    Brush,
    Heal,
    Pitch,
    Spectral,
    Waveform,
    Multitrack,
    Folder,
    NewFile,
    Trash,
    Marker,
    Power,
    Up,
    Insert,
    Menu,
    /// Show/hide the rack's Input/Output/Mix controls.
    List,
    /// Rack before the fader (arrow into a bar) / after it.
    PreFader,
    PostFader,
    /// Pre-render.
    Lightning,
    /// Auto-play: a speaker with a play mark.
    AutoPlay,
}

pub fn paint_icon(p: &egui::Painter, r: Rect, icon: Icon, col: Color32) {
    let line = match icon {
        Icon::Folder => Some(&crate::lucide::FOLDER_OPEN),
        Icon::NewFile => Some(&crate::lucide::FILE_PLUS),
        Icon::Insert => Some(&crate::lucide::INSERT),
        Icon::Trash => Some(&crate::lucide::TRASH),
        Icon::Loop => Some(&crate::lucide::LOOP),
        Icon::AutoPlay => Some(&crate::lucide::AUTOPLAY),
        _ => None,
    };
    if let Some(d) = line {
        crate::lucide::draw(p, r, d, col);
        return;
    }
    let c = r.center();
    let s = r.height().min(r.width()) * 0.5;
    let st = Stroke::new(1.5, col);
    let tri = |pts: Vec<Pos2>| egui::Shape::convex_polygon(pts, col, Stroke::NONE);
    match icon {
        Icon::Stop => {
            p.rect_filled(Rect::from_center_size(c, vec2(s * 1.1, s * 1.1)), 0.0, col);
        }
        Icon::Play => {
            p.add(tri(vec![
                pos2(c.x - s * 0.5, c.y - s * 0.65),
                pos2(c.x + s * 0.65, c.y),
                pos2(c.x - s * 0.5, c.y + s * 0.65),
            ]));
        }
        Icon::Pause => {
            p.rect_filled(
                Rect::from_center_size(pos2(c.x - s * 0.3, c.y), vec2(s * 0.35, s * 1.2)),
                0.0,
                col,
            );
            p.rect_filled(
                Rect::from_center_size(pos2(c.x + s * 0.3, c.y), vec2(s * 0.35, s * 1.2)),
                0.0,
                col,
            );
        }
        Icon::ToStart | Icon::ToEnd => {
            let d = if icon == Icon::ToEnd { 1.0 } else { -1.0 };
            p.rect_filled(
                Rect::from_center_size(pos2(c.x + d * s * 0.55, c.y), vec2(s * 0.2, s * 1.2)),
                0.0,
                col,
            );
            p.add(tri(vec![
                pos2(c.x - d * s * 0.5, c.y - s * 0.6),
                pos2(c.x + d * s * 0.4, c.y),
                pos2(c.x - d * s * 0.5, c.y + s * 0.6),
            ]));
        }
        Icon::Rewind | Icon::Forward => {
            let d = if icon == Icon::Forward { 1.0 } else { -1.0 };
            for o in [-0.45, 0.4] {
                let x = c.x + d * o * s;
                p.add(tri(vec![
                    pos2(x - d * s * 0.45, c.y - s * 0.55),
                    pos2(x + d * s * 0.45, c.y),
                    pos2(x - d * s * 0.45, c.y + s * 0.55),
                ]));
            }
        }
        Icon::Record => {
            p.circle_filled(c, s * 0.6, Color32::from_rgb(220, 60, 60));
        }
        Icon::Loop => {
            let rr = Rect::from_center_size(c, vec2(s * 1.5, s * 1.0));
            p.rect_stroke(rr, 3.0, st, egui::StrokeKind::Middle);
            p.add(tri(vec![
                pos2(rr.right() - s * 0.1, rr.top() - s * 0.3),
                pos2(rr.right() + s * 0.3, rr.top()),
                pos2(rr.right() - s * 0.1, rr.top() + s * 0.3),
            ]));
        }
        Icon::ZoomInAmp
        | Icon::ZoomOutAmp
        | Icon::ZoomIn
        | Icon::ZoomOut
        | Icon::ZoomFull
        | Icon::ZoomSelIn => {
            let lc = pos2(c.x - s * 0.15, c.y - s * 0.15);
            p.circle_stroke(lc, s * 0.55, st);
            p.line_segment(
                [
                    pos2(lc.x + s * 0.4, lc.y + s * 0.4),
                    pos2(c.x + s * 0.8, c.y + s * 0.8),
                ],
                Stroke::new(2.0, col),
            );
            let k = s * 0.3;
            match icon {
                Icon::ZoomIn | Icon::ZoomInAmp | Icon::ZoomSelIn => {
                    p.line_segment([pos2(lc.x - k, lc.y), pos2(lc.x + k, lc.y)], st);
                    p.line_segment([pos2(lc.x, lc.y - k), pos2(lc.x, lc.y + k)], st);
                }
                Icon::ZoomOut | Icon::ZoomOutAmp => {
                    p.line_segment([pos2(lc.x - k, lc.y), pos2(lc.x + k, lc.y)], st);
                }
                _ => {
                    p.rect_stroke(
                        Rect::from_center_size(lc, vec2(k * 1.6, k * 1.6)),
                        0.0,
                        Stroke::new(1.0, col),
                        egui::StrokeKind::Middle,
                    );
                }
            }
            if matches!(icon, Icon::ZoomInAmp | Icon::ZoomOutAmp) {
                p.line_segment(
                    [
                        pos2(r.left() + 1.0, r.top() + 2.0),
                        pos2(r.left() + 1.0, r.bottom() - 2.0),
                    ],
                    Stroke::new(1.0, col),
                );
            }
            if icon == Icon::ZoomSelIn {
                p.line_segment(
                    [
                        pos2(r.left() + 1.0, r.bottom() - 2.0),
                        pos2(r.left() + 6.0, r.bottom() - 2.0),
                    ],
                    Stroke::new(1.0, col),
                );
            }
        }
        Icon::AutoPlay => {
            // a small play mark, then a speaker with its sound waves
            let k = s / 9.0;
            p.add(tri(vec![
                pos2(c.x - 8.5 * k, c.y - 4.0 * k),
                pos2(c.x - 4.5 * k, c.y - 1.5 * k),
                pos2(c.x - 8.5 * k, c.y + 1.0 * k),
            ]));
            p.rect_filled(
                Rect::from_min_max(
                    pos2(c.x - 3.0 * k, c.y - 2.0 * k),
                    pos2(c.x - 0.5 * k, c.y + 2.0 * k),
                ),
                0.0,
                col,
            );
            p.add(tri(vec![
                pos2(c.x - 0.5 * k, c.y - 2.0 * k),
                pos2(c.x + 3.5 * k, c.y - 5.5 * k),
                pos2(c.x + 3.5 * k, c.y + 5.5 * k),
                pos2(c.x - 0.5 * k, c.y + 2.0 * k),
            ]));
            for (r, a) in [(5.5f32, 0.75f32), (8.0, 0.85)] {
                let pts: Vec<Pos2> = (0..=10)
                    .map(|i| {
                        let t = -a + 2.0 * a * i as f32 / 10.0;
                        pos2(c.x + 1.0 * k + r * k * t.cos(), c.y + r * k * t.sin())
                    })
                    .collect();
                p.add(egui::Shape::line(pts, Stroke::new(1.2, col)));
            }
        }
        Icon::Move | Icon::Direct => {
            // a pointer arrow (head and tail); Move is the dark "black" arrow, Direct Selection
            // the white one, as in vector editors
            let k = s / 9.0;
            let o = c + vec2(-0.5 * k, 0.0);
            let q = |x: f32, y: f32| pos2(o.x + (x - 12.0) * k, o.y + (y - 12.0) * k);
            let outline = vec![
                q(6.0, 3.0),
                q(6.0, 19.0),
                q(10.0, 15.0),
                q(13.0, 21.0),
                q(15.5, 20.0),
                q(12.6, 14.0),
                q(18.0, 14.0),
            ];
            let (fill, edge) = if icon == Icon::Move {
                (Color32::from_gray(12), col)
            } else {
                (Color32::WHITE, Color32::from_gray(150))
            };
            p.add(egui::Shape::convex_polygon(
                vec![q(6.0, 3.0), q(6.0, 19.0), q(10.0, 15.0)],
                fill,
                Stroke::NONE,
            ));
            p.add(egui::Shape::convex_polygon(
                vec![q(6.0, 3.0), q(10.0, 15.0), q(12.6, 14.0), q(18.0, 14.0)],
                fill,
                Stroke::NONE,
            ));
            p.add(egui::Shape::convex_polygon(
                vec![q(10.0, 15.0), q(13.0, 21.0), q(15.5, 20.0), q(12.6, 14.0)],
                fill,
                Stroke::NONE,
            ));
            p.add(egui::Shape::closed_line(
                outline.clone(),
                Stroke::new(1.2, edge),
            ));
        }
        Icon::Razor => {
            // a razor blade slicing down through a clip: the tilted blade (bright cutting edge,
            // dark centre slot) and the dashed cut line below its tip
            let k = s / 9.0;
            let o = c + vec2(2.0 * k, -2.5 * k);
            let rot = |x: f32, y: f32| {
                let (sn, cs) = (-0.785f32).sin_cos();
                pos2(o.x + (x * cs - y * sn) * k, o.y + (x * sn + y * cs) * k)
            };
            let blade = vec![
                rot(-7.5, -3.6),
                rot(7.5, -3.6),
                rot(7.5, 3.6),
                rot(-7.5, 3.6),
            ];
            p.add(egui::Shape::convex_polygon(blade, col, Stroke::NONE));
            p.line_segment(
                [rot(-7.5, 3.6), rot(7.5, 3.6)],
                Stroke::new(1.6, Color32::WHITE),
            );
            p.add(egui::Shape::convex_polygon(
                vec![
                    rot(-4.5, -1.0),
                    rot(4.5, -1.0),
                    rot(4.5, 1.0),
                    rot(-4.5, 1.0),
                ],
                Color32::from_gray(25),
                Stroke::NONE,
            ));
            let tip = rot(-7.5, 3.6);
            let mut y = tip.y + 1.5 * k;
            while y < c.y + 9.0 * k {
                p.line_segment(
                    [
                        pos2(tip.x, y),
                        pos2(tip.x, (y + 1.8 * k).min(c.y + 9.0 * k)),
                    ],
                    Stroke::new(1.5, col),
                );
                y += 3.2 * k;
            }
        }
        Icon::Slip => {
            p.line_segment([pos2(c.x - s * 0.8, c.y), pos2(c.x + s * 0.8, c.y)], st);
            p.add(tri(vec![
                pos2(c.x - s * 0.8, c.y),
                pos2(c.x - s * 0.4, c.y - s * 0.4),
                pos2(c.x - s * 0.4, c.y + s * 0.4),
            ]));
            p.add(tri(vec![
                pos2(c.x + s * 0.8, c.y),
                pos2(c.x + s * 0.4, c.y + s * 0.4),
                pos2(c.x + s * 0.4, c.y - s * 0.4),
            ]));
            p.rect_stroke(
                Rect::from_center_size(c, vec2(s * 0.6, s * 1.4)),
                0.0,
                Stroke::new(1.0, col),
                egui::StrokeKind::Middle,
            );
        }
        Icon::TimeSel => {
            p.line_segment([pos2(c.x, c.y - s * 0.8), pos2(c.x, c.y + s * 0.8)], st);
            p.line_segment(
                [
                    pos2(c.x - s * 0.35, c.y - s * 0.8),
                    pos2(c.x + s * 0.35, c.y - s * 0.8),
                ],
                st,
            );
            p.line_segment(
                [
                    pos2(c.x - s * 0.35, c.y + s * 0.8),
                    pos2(c.x + s * 0.35, c.y + s * 0.8),
                ],
                st,
            );
        }
        Icon::Marquee => {
            let r2 = Rect::from_center_size(c, vec2(s * 1.6, s * 1.2));
            let mut x = r2.left();
            while x < r2.right() {
                let x2 = (x + s * 0.25).min(r2.right());
                p.line_segment([pos2(x, r2.top()), pos2(x2, r2.top())], st);
                p.line_segment([pos2(x, r2.bottom()), pos2(x2, r2.bottom())], st);
                x += s * 0.45;
            }
            let mut y = r2.top();
            while y < r2.bottom() {
                let y2 = (y + s * 0.25).min(r2.bottom());
                p.line_segment([pos2(r2.left(), y), pos2(r2.left(), y2)], st);
                p.line_segment([pos2(r2.right(), y), pos2(r2.right(), y2)], st);
                y += s * 0.45;
            }
        }
        Icon::Lasso => {
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = i as f32 / 20.0 * std::f32::consts::TAU;
                    pos2(c.x + a.cos() * s * 0.8, c.y - s * 0.2 + a.sin() * s * 0.5)
                })
                .collect();
            p.add(egui::Shape::line(pts, st));
            p.add(egui::Shape::line(
                vec![
                    pos2(c.x - s * 0.3, c.y + s * 0.25),
                    pos2(c.x - s * 0.5, c.y + s * 0.6),
                    pos2(c.x - s * 0.2, c.y + s * 0.9),
                ],
                Stroke::new(1.2, col),
            ));
        }
        Icon::Brush => {
            // a paintbrush: slim handle, metal ferrule, a full bristle tip ending in a point,
            // and a short painted stroke under it
            let along = vec2(0.707, -0.707); // up and to the right
            let across = vec2(0.707, 0.707);
            let base = c + vec2(-s * 0.25, s * 0.25);
            let handle_end = base + along * s * 1.15;
            p.line_segment(
                [base + along * s * 0.35, handle_end],
                Stroke::new(s * 0.28, col),
            );
            // ferrule
            let f0 = base + along * s * 0.1;
            let f1 = base + along * s * 0.38;
            p.add(egui::Shape::convex_polygon(
                vec![
                    f0 + across * s * 0.2,
                    f1 + across * s * 0.17,
                    f1 - across * s * 0.17,
                    f0 - across * s * 0.2,
                ],
                col,
                Stroke::NONE,
            ));
            // bristles: a teardrop towards the tip
            let tip = base - along * s * 0.55;
            p.add(egui::Shape::convex_polygon(
                vec![
                    f0 + across * s * 0.24,
                    base - along * s * 0.25 + across * s * 0.2,
                    tip,
                    base - along * s * 0.25 - across * s * 0.2,
                    f0 - across * s * 0.24,
                ],
                col,
                Stroke::NONE,
            ));
            // the stroke it paints
            let pts: Vec<Pos2> = (0..=8)
                .map(|i| {
                    pos2(
                        c.x - s * 0.95 + i as f32 * s * 0.12,
                        c.y + s * 0.88 - (i as f32 * 0.8).sin() * s * 0.08,
                    )
                })
                .collect();
            p.add(egui::Shape::line(
                pts,
                Stroke::new(1.3, col.gamma_multiply(0.7)),
            ));
        }
        Icon::Heal => {
            // a sticking plaster (band-aid) on the slant, with its pad and air holes, and a
            // small sparkle: "heal this spot"
            let along = vec2(0.82, -0.57);
            let across = vec2(0.57, 0.82);
            let ctr = c + vec2(-s * 0.08, s * 0.08);
            let half_l = s * 0.95;
            let half_w = s * 0.38;
            let mut outline = vec![];
            for i in 0..=10 {
                let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 10.0;
                outline.push(
                    ctr - along * (half_l - half_w) + (along * a.cos() + across * a.sin()) * half_w,
                );
            }
            for i in 0..=10 {
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 10.0;
                outline.push(
                    ctr + along * (half_l - half_w) + (along * a.cos() + across * a.sin()) * half_w,
                );
            }
            p.add(egui::Shape::closed_line(outline, Stroke::new(1.4, col)));
            // the pad in the middle
            let pad = [
                ctr - along * s * 0.3 - across * s * 0.26,
                ctr + along * s * 0.3 - across * s * 0.26,
                ctr + along * s * 0.3 + across * s * 0.26,
                ctr - along * s * 0.3 + across * s * 0.26,
            ];
            p.add(egui::Shape::convex_polygon(
                pad.to_vec(),
                col.gamma_multiply(0.85),
                Stroke::NONE,
            ));
            for k in [-0.62f32, 0.62] {
                p.circle_filled(ctr + along * s * k, s * 0.06, col);
            }
            // sparkle
            let sp = c + vec2(s * 0.72, -s * 0.72);
            p.line_segment(
                [sp - vec2(s * 0.22, 0.0), sp + vec2(s * 0.22, 0.0)],
                Stroke::new(1.2, col),
            );
            p.line_segment(
                [sp - vec2(0.0, s * 0.22), sp + vec2(0.0, s * 0.22)],
                Stroke::new(1.2, col),
            );
        }
        Icon::Pitch => {
            // a pitch curve over a dim spectrum
            p.rect_filled(
                Rect::from_center_size(c, vec2(s * 1.8, s * 1.4)),
                0.0,
                Color32::from_rgb(40, 40, 90),
            );
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let x = c.x - s * 0.8 + i as f32 * s * 0.1333;
                    pos2(
                        x,
                        c.y + (i as f32 * 0.7).sin() * s * 0.35 - (i as f32) * s * 0.03,
                    )
                })
                .collect();
            p.add(egui::Shape::line(
                pts,
                Stroke::new(1.6, Color32::from_rgb(120, 200, 255)),
            ));
        }
        Icon::Spectral => {
            for i in 0..5 {
                let x = c.x - s * 0.8 + i as f32 * s * 0.4;
                let h = [0.5, 1.1, 0.8, 1.4, 0.6][i] * s * 0.6;
                let colr =
                    Color32::from_rgb(160 + i as u8 * 18, 80 + i as u8 * 30, 200 - i as u8 * 30);
                p.rect_filled(
                    Rect::from_min_max(
                        pos2(x, c.y + s * 0.7 - h),
                        pos2(x + s * 0.3, c.y + s * 0.7),
                    ),
                    0.0,
                    colr,
                );
            }
        }
        Icon::Waveform => {
            let mut pts = vec![];
            for i in 0..=16 {
                let x = c.x - s * 0.9 + i as f32 * s * 0.1125 * 1.0;
                let a = ((i as f32) * 1.3).sin() * s * 0.6 * if i % 2 == 0 { 1.0 } else { -0.7 };
                pts.push(pos2(x, c.y + a));
            }
            p.add(egui::Shape::line(pts, Stroke::new(1.3, col)));
        }
        Icon::Multitrack => {
            for i in 0..3 {
                let y = c.y - s * 0.6 + i as f32 * s * 0.6;
                let off = [0.0, 0.3, -0.2][i] * s;
                p.rect_filled(
                    Rect::from_min_size(
                        pos2(c.x - s * 0.8 + off, y - s * 0.2),
                        vec2(s * 1.1, s * 0.4),
                    ),
                    0.0,
                    col,
                );
            }
        }
        Icon::Folder => {
            let rr = Rect::from_center_size(pos2(c.x, c.y + s * 0.1), vec2(s * 1.6, s * 1.1));
            p.rect_filled(
                Rect::from_min_size(pos2(rr.left(), rr.top() - s * 0.2), vec2(s * 0.7, s * 0.3)),
                0.0,
                col,
            );
            p.rect_filled(rr, 0.0, col);
        }
        Icon::NewFile => {
            let rr = Rect::from_center_size(c, vec2(s * 1.1, s * 1.5));
            p.rect_stroke(rr, 0.0, st, egui::StrokeKind::Middle);
            p.line_segment([pos2(c.x - s * 0.3, c.y), pos2(c.x + s * 0.3, c.y)], st);
            p.line_segment([pos2(c.x, c.y - s * 0.3), pos2(c.x, c.y + s * 0.3)], st);
        }
        Icon::Trash => {
            let rr = Rect::from_center_size(pos2(c.x, c.y + s * 0.15), vec2(s * 1.0, s * 1.2));
            p.rect_stroke(rr, 0.0, st, egui::StrokeKind::Middle);
            p.line_segment(
                [
                    pos2(c.x - s * 0.7, rr.top() - s * 0.15),
                    pos2(c.x + s * 0.7, rr.top() - s * 0.15),
                ],
                st,
            );
        }
        Icon::Marker => {
            p.add(tri(vec![
                pos2(c.x - s * 0.5, c.y - s * 0.7),
                pos2(c.x + s * 0.5, c.y - s * 0.7),
                pos2(c.x + s * 0.5, c.y + s * 0.2),
                pos2(c.x, c.y + s * 0.7),
                pos2(c.x - s * 0.5, c.y + s * 0.2),
            ]));
        }
        Icon::Power => {
            p.circle_stroke(c, s * 0.6, st);
            p.line_segment(
                [pos2(c.x, c.y - s * 0.85), pos2(c.x, c.y)],
                Stroke::new(2.0, col),
            );
        }
        Icon::Up => {
            p.add(tri(vec![
                pos2(c.x, c.y - s * 0.6),
                pos2(c.x + s * 0.6, c.y + s * 0.3),
                pos2(c.x - s * 0.6, c.y + s * 0.3),
            ]));
        }
        Icon::Insert => {
            p.rect_stroke(
                Rect::from_center_size(pos2(c.x + s * 0.2, c.y), vec2(s * 1.1, s * 1.2)),
                0.0,
                st,
                egui::StrokeKind::Middle,
            );
            p.add(tri(vec![
                pos2(c.x - s * 0.8, c.y - s * 0.4),
                pos2(c.x - s * 0.1, c.y),
                pos2(c.x - s * 0.8, c.y + s * 0.4),
            ]));
        }
        Icon::List => {
            for i in -1..=1 {
                let y = c.y + i as f32 * s * 0.5;
                p.circle_filled(pos2(c.x - s * 0.6, y), 1.3, col);
                p.line_segment(
                    [pos2(c.x - s * 0.25, y), pos2(c.x + s * 0.7, y)],
                    Stroke::new(1.4, col),
                );
            }
        }
        Icon::PreFader | Icon::PostFader => {
            // a fader bar with an arrow entering it (pre) or leaving it (post)
            let bar_x = if icon == Icon::PreFader {
                c.x + s * 0.45
            } else {
                c.x - s * 0.45
            };
            p.line_segment(
                [pos2(bar_x, c.y - s * 0.8), pos2(bar_x, c.y + s * 0.8)],
                Stroke::new(1.6, col),
            );
            p.rect_filled(
                Rect::from_center_size(pos2(bar_x, c.y - s * 0.15), vec2(s * 0.5, s * 0.3)),
                0.0,
                col,
            );
            let (x0, x1) = if icon == Icon::PreFader {
                (c.x - s * 0.8, c.x + s * 0.2)
            } else {
                (c.x - s * 0.2, c.x + s * 0.8)
            };
            p.line_segment([pos2(x0, c.y + s * 0.35), pos2(x1, c.y + s * 0.35)], st);
            p.add(tri(vec![
                pos2(x1 + s * 0.1, c.y + s * 0.35),
                pos2(x1 - s * 0.25, c.y + s * 0.1),
                pos2(x1 - s * 0.25, c.y + s * 0.6),
            ]));
        }
        Icon::Lightning => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(c.x + s * 0.25, c.y - s * 0.9),
                    pos2(c.x - s * 0.5, c.y + s * 0.1),
                    pos2(c.x + s * 0.05, c.y + s * 0.1),
                ],
                col,
                Stroke::NONE,
            ));
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(c.x - s * 0.05, c.y - s * 0.1),
                    pos2(c.x + s * 0.5, c.y - s * 0.1),
                    pos2(c.x - s * 0.25, c.y + s * 0.9),
                ],
                col,
                Stroke::NONE,
            ));
        }
        Icon::Menu => {
            for i in -1..=1 {
                let y = c.y + i as f32 * s * 0.45;
                p.line_segment(
                    [pos2(c.x - s * 0.6, y), pos2(c.x + s * 0.6, y)],
                    Stroke::new(1.2, col),
                );
            }
        }
    }
}

/// A small square icon button.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, selected: bool, tip: &str) -> egui::Response {
    icon_button_sized(ui, icon, selected, tip, vec2(24.0, 22.0))
}

pub fn icon_button_sized(
    ui: &mut egui::Ui,
    icon: Icon,
    selected: bool,
    tip: &str,
    size: Vec2,
) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    if !ui.is_enabled() {
        // greyed out: nothing to press here right now
        paint_icon(p, icon_box(r), icon, Color32::from_gray(78));
        return if tip.is_empty() {
            resp
        } else {
            resp.on_disabled_hover_text(tip)
        };
    }
    p.rect_filled(
        r,
        ROUND as f32,
        control_fill(selected, crate::theme::selected(), &resp, false),
    );
    let col = if selected || resp.hovered() {
        strong()
    } else {
        text()
    };
    paint_icon(p, icon_box(r), icon, col);
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// Width of the check-mark gutter at the left of every popup menu.
pub const MENU_GUTTER: f32 = 20.0;

/// A menu item with a check mark in the menu's gutter when `on`.
pub fn check_item(
    ui: &mut egui::Ui,
    enabled: bool,
    on: bool,
    label: &str,
    shortcut: &str,
) -> egui::Response {
    mark_item(ui, enabled, on, label, shortcut, false)
}

/// A menu item that is one of a set (a filled dot in the gutter marks the current one).
pub fn radio_item(
    ui: &mut egui::Ui,
    enabled: bool,
    on: bool,
    label: &str,
    shortcut: &str,
) -> egui::Response {
    mark_item(ui, enabled, on, label, shortcut, true)
}

/// A plain menu item, drawn exactly like the items in the main menu bar: no pill, no border,
/// just the label, its shortcut and the command's help text on hover. Right-click menus use
/// this too, so every menu in the app speaks the same visual language.
pub fn menu_button(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    shortcut: &str,
    doc: &str,
) -> egui::Response {
    let b = if shortcut.is_empty() {
        egui::Button::new(label)
    } else {
        egui::Button::new(label).shortcut_text(shortcut)
    };
    let r = ui.add_enabled(enabled, b);
    if doc.is_empty() {
        r
    } else {
        r.on_hover_text(doc)
    }
}

/// A menu item that turns a setting on and off (the menu stays open).
pub fn check_toggle(
    ui: &mut egui::Ui,
    v: &mut bool,
    label: &str,
    shortcut: &str,
) -> egui::Response {
    let r = check_item(ui, true, *v, label, shortcut);
    if r.clicked() {
        *v = !*v;
    }
    r
}

fn mark_item(
    ui: &mut egui::Ui,
    enabled: bool,
    on: bool,
    label: &str,
    shortcut: &str,
    dot: bool,
) -> egui::Response {
    let b = egui::Button::new(label);
    let b = if shortcut.is_empty() {
        b
    } else {
        b.shortcut_text(shortcut)
    };
    let r = ui.add_enabled(enabled, b);
    if on {
        let c = pos2(r.rect.left() - MENU_GUTTER / 2.0 + 1.0, r.rect.center().y);
        let col = if enabled {
            text()
        } else {
            Color32::from_gray(110)
        };
        if dot {
            ui.painter().circle_filled(c, 3.0, col);
        } else {
            ui.painter().line_segment(
                [c + vec2(-4.0, 0.0), c + vec2(-1.2, 3.0)],
                Stroke::new(1.6, col),
            );
            ui.painter().line_segment(
                [c + vec2(-1.2, 3.0), c + vec2(4.0, -3.5)],
                Stroke::new(1.6, col),
            );
        }
    }
    r
}

/// A dialog's footer button, in the rounded "pill" shape of the original's dialogs: the default
/// action filled blue, the others outlined. Every dialog uses these, so they all match.
pub fn dialog_button(ui: &mut egui::Ui, text: &str, primary: bool) -> egui::Response {
    // painted by hand so hover and press always show (a fixed fill hid egui's hover state)
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font, Color32::WHITE);
    let size = vec2((galley.size().x + 24.0).max(96.0), 26.0);
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let t = ui
        .ctx()
        .animate_bool(resp.id.with("hover"), resp.hovered() && enabled);
    let pressed = resp.is_pointer_button_down_on() && enabled;
    let fill = if primary {
        let base = if pressed {
            Color32::from_rgb(28, 92, 170)
        } else {
            crate::theme::primary()
        };
        lerp_color(
            base,
            Color32::from_rgb(70, 150, 240),
            if pressed { 0.0 } else { t },
        )
    } else {
        // the secondary buttons (Cancel, Close, Don't Save...) are flat filled faces of the same
        // size as the main one: never outlined, never larger
        if pressed {
            pressed_col()
        } else {
            lerp_color(control(), menu_hover(), t)
        }
    };
    let p = ui.painter();
    p.rect_filled(
        r,
        ROUND as f32,
        if enabled {
            fill
        } else {
            fill.gamma_multiply(0.5)
        },
    );
    let col = if enabled {
        if primary { Color32::WHITE } else { strong() }
    } else {
        Color32::from_gray(120)
    };
    p.galley_with_override_text_color(r.center() - galley.size() / 2.0, galley, col);
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        m(a.r(), b.r()),
        m(a.g(), b.g()),
        m(a.b(), b.b()),
        m(a.a(), b.a()),
    )
}

/// A framed toggle with an icon (rack and panel strips): grey at rest, blue when on.
pub fn icon_toggle(ui: &mut egui::Ui, icon: Icon, on: bool, tip: &str) -> egui::Response {
    framed_icon(ui, icon, on, selected(), tip)
}

/// The green power switch (an effect, a rack, a plug-in).
pub fn power_toggle(ui: &mut egui::Ui, on: bool, tip: &str) -> egui::Response {
    framed_icon(ui, Icon::Power, on, power_on(), tip)
}

fn framed_icon(
    ui: &mut egui::Ui,
    icon: Icon,
    on: bool,
    on_col: Color32,
    tip: &str,
) -> egui::Response {
    framed_icon_sense(ui, icon, on, on_col, tip, Sense::click())
}

fn framed_icon_sense(
    ui: &mut egui::Ui,
    icon: Icon,
    on: bool,
    on_col: Color32,
    tip: &str,
    sense: Sense,
) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 20.0), sense);
    let p = ui.painter();
    if !ui.is_enabled() {
        p.rect_filled(r.shrink(1.0), 0.0, control().gamma_multiply(0.6));
        paint_icon(p, icon_box(r), icon, Color32::from_gray(78));
        return if tip.is_empty() {
            resp
        } else {
            resp.on_disabled_hover_text(tip)
        };
    }
    p.rect_filled(
        r.shrink(1.0),
        ROUND as f32,
        control_fill(on, on_col, &resp, true),
    );
    let col = if on || resp.hovered() {
        strong()
    } else {
        text_dim()
    };
    paint_icon(p, icon_box(r), icon, col);
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// Header strip with tabs. Returns true if a tab changed.
pub fn tab_header<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    tabs: &[(T, &str)],
    active: &mut T,
) -> bool {
    // inside a docked group the group draws the tabs; the panel's own header stays out
    if crate::dock::skip_headers(ui.ctx()) {
        return false;
    }
    let mut changed = false;
    let h = 24.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, panel_header());
    let mut x = rect.left() + 4.0;
    for (id, name) in tabs {
        let galley =
            ui.painter()
                .layout_no_wrap(name.to_string(), FontId::proportional(12.0), text());
        let w = galley.size().x + 18.0;
        let tr = Rect::from_min_size(pos2(x, rect.top()), vec2(w, h));
        let resp = ui.interact(tr, ui.scope_id().with(("tab", *name)), Sense::click());
        let is_active = *id == *active;
        let col = if is_active {
            strong()
        } else if resp.hovered() {
            text()
        } else {
            text_dim()
        };
        if is_active {
            ui.painter().rect_filled(tr, 0.0, panel());
            ui.painter().line_segment(
                [
                    pos2(tr.left() + 2.0, tr.bottom() - 1.5),
                    pos2(tr.right() - 2.0, tr.bottom() - 1.5),
                ],
                Stroke::new(2.0, accent()),
            );
        }
        ui.painter().text(
            tr.center(),
            egui::Align2::CENTER_CENTER,
            *name,
            FontId::proportional(12.0),
            col,
        );
        if resp.clicked() && !is_active {
            *active = *id;
            changed = true;
        }
        x += w + 2.0;
    }
    // panel menu (the ≡ at the right of the header)
    if x < rect.right() - 24.0 {
        let mr = Rect::from_min_size(
            pos2(rect.right() - 24.0, rect.top() + 3.0),
            vec2(20.0, 18.0),
        );
        let key = tabs
            .iter()
            .find(|t| t.0 == *active)
            .or(tabs.first())
            .map(|t| t.1)
            .unwrap_or("");
        let key = if key.starts_with("Editor") {
            "Editor"
        } else {
            key
        };
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(mr)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let btn = egui::Button::new(egui::RichText::new("\u{2261}").size(15.0).color(text_dim()))
            .frame(false)
            .min_size(vec2(20.0, 18.0));
        let _ = egui::containers::menu::MenuButton::from_button(btn)
            .ui(&mut child, |ui| panel_menu(ui, key));
    }
    changed
}

/// Ask the app to run something from a panel menu (picked up once per frame).
fn panel_request(ui: &egui::Ui, what: String) {
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new("audiocraft_panel_cmd"), what));
    ui.close();
}

/// Take the panel-menu request of this frame, if any.
pub fn take_panel_request(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::new("audiocraft_panel_cmd")))
}

/// The panel menu: the docking items every panel has, then the panel's own items.
fn panel_menu(ui: &mut egui::Ui, key: &str) {
    ui.set_min_width(200.0);
    let req = |ui: &mut egui::Ui, label: &str, sc: &str, what: String| {
        if ui.add(egui::Button::new(label).shortcut_text(sc)).clicked() {
            panel_request(ui, what);
        }
    };
    // panel-specific items
    match key {
        "Files" | "Favorites" => {
            req(ui, "Open...", "Ctrl+O", "file.open".into());
            req(ui, "New Audio File...", "Ctrl+N", "file.new".into());
            req(
                ui,
                "New Multitrack Session...",
                "Ctrl+Shift+N",
                "file.new_session".into(),
            );
            req(ui, "Close File", "Ctrl+W", "file.close".into());
            ui.separator();
        }
        "Media Browser" | "Effects Rack" | "Markers" | "Properties" => {
            req(ui, "Show Effects Rack", "", "window.effects_rack".into());
            req(ui, "Show Markers", "", "window.markers".into());
            req(ui, "Clip Effects", "", "window.clip_effects".into());
            req(ui, "Track Effects", "", "window.track_effects".into());
            ui.separator();
        }
        "History" => {
            req(ui, "Undo", "Ctrl+Z", "edit.undo".into());
            req(ui, "Redo", "Ctrl+Shift+Z", "edit.redo".into());
            ui.separator();
        }
        "Levels" => {
            req(ui, "Reset Indicators", "", "panel.levels_reset".into());
            req(
                ui,
                "Show Amplitude Statistics",
                "",
                "window.amplitude_statistics".into(),
            );
            req(
                ui,
                "Show Frequency Analysis",
                "",
                "window.frequency_analysis".into(),
            );
            ui.separator();
        }
        "Selection/View" => {
            req(ui, "Time Display: Decimal", "", "panel.time_decimal".into());
            req(ui, "Time Display: Samples", "", "panel.time_samples".into());
            req(ui, "Time Display: Seconds", "", "panel.time_seconds".into());
            req(ui, "Zoom to Selection", "", "view.zoom_selection".into());
            ui.separator();
        }
        "Editor" | "Mixer" => {
            req(
                ui,
                "Spectral Frequency Display",
                "Shift+D",
                "view.spectral".into(),
            );
            req(
                ui,
                "Show Clip Volume Envelopes",
                "",
                "view.envelopes".into(),
            );
            req(
                ui,
                "Show Clip Pan Envelopes",
                "",
                "view.pan_envelopes".into(),
            );
            req(
                ui,
                "Show/Hide Volume Automation Lane",
                "Alt+A",
                "view.volume_automation".into(),
            );
            req(
                ui,
                "Add Volume Automation Point",
                "",
                "track.automation_point".into(),
            );
            req(
                ui,
                "Clear Volume Automation",
                "",
                "track.automation_clear".into(),
            );
            req(ui, "Zoom Out Full", "\\", "view.zoom_full".into());
            req(
                ui,
                "Reset Frequency Zoom",
                "",
                "view.spectral_zoom_reset".into(),
            );
            ui.separator();
        }
        "Essential Sound" => {
            req(
                ui,
                "Close Essential Sound",
                "",
                "window.essential_sound".into(),
            );
            ui.separator();
        }
        "Plug-ins" => {
            req(
                ui,
                "Pop Out / Dock Plug-in Window",
                "",
                "plugins.pop_out".into(),
            );
            req(
                ui,
                "Close All Plug-in Windows",
                "",
                "plugins.close_all".into(),
            );
            req(
                ui,
                "Dock Plug-in Windows in the Panel (on/off)",
                "",
                "plugins.toggle_dock".into(),
            );
            req(
                ui,
                "Audio Plug-In Manager...",
                "",
                "window.plugin_manager".into(),
            );
            req(ui, "Close Plug-ins Panel", "", "window.plugins".into());
            ui.separator();
        }
        _ => {}
    }
    // the docking items: a panel (or its whole group) can move to a window of its own
    let closable = !matches!(key, "Editor" | "Mixer");
    let movable = closable && key != "Plug-ins";
    if ui
        .add_enabled(movable, egui::Button::new("Undock Panel"))
        .on_hover_text("Move this panel to a window of its own")
        .clicked()
    {
        panel_request(ui, format!("panel.undock:{key}"));
    }
    if ui
        .add_enabled(movable, egui::Button::new("Undock Panel Group"))
        .on_hover_text("Move this panel's group to a window of its own")
        .clicked()
    {
        panel_request(ui, format!("panel.undock_group:{key}"));
    }
    if ui
        .add_enabled(closable, egui::Button::new("Close Panel"))
        .clicked()
    {
        panel_request(ui, format!("panel.close:{key}"));
    }
    if ui
        .add_enabled(closable, egui::Button::new("Close Other Panels in Group"))
        .clicked()
    {
        panel_request(ui, format!("panel.close_others:{key}"));
    }
    if ui
        .add(egui::Button::new("Maximize Panel Group").shortcut_text("`"))
        .clicked()
    {
        panel_request(ui, format!("panel.max:{key}"));
    }
    if ui.button("Reset Workspace").clicked() {
        panel_request(ui, "panel.reset".into());
    }
}

/// A single-title header (non-tabbed panel).
pub fn title_header(ui: &mut egui::Ui, title: &str) {
    let mut a = 0u8;
    tab_header(ui, &[(0u8, title)], &mut a);
}

/// Rotary knob. Returns response; value changes on vertical drag.
/// Pointer angle of a knob (screen radians, y down). The sweep runs 270 degrees from lower left
/// to lower right; 0 always points straight up when the range spans it (each side of 0 gets
/// its own half of the sweep, so -60..+15 dB still has 0 dB at the top).
pub fn knob_angle(v: f32, min: f32, max: f32) -> f32 {
    use std::f32::consts::PI;
    let start = PI * 0.75; // lower left
    let up = PI * 1.5; // straight up
    let end = PI * 2.25; // lower right
    if max <= min {
        return up;
    }
    let v = v.clamp(min, max);
    if min < 0.0 && max > 0.0 {
        if v < 0.0 {
            start + (v - min) / (0.0 - min) * (up - start)
        } else {
            up + v / max * (end - up)
        }
    } else {
        start + (v - min) / (max - min) * (end - start)
    }
}

pub fn knob(
    ui: &mut egui::Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    size: f32,
    tip: &str,
) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    let mut resp = resp;
    if resp.dragged() {
        let d = -resp.drag_delta().y + resp.drag_delta().x;
        let range = max - min;
        *value = (*value + d * range / 150.0).clamp(min, max);
        resp.mark_changed();
    }
    if resp.double_clicked() {
        *value = if min < 0.0 && max > 0.0 { 0.0 } else { min };
        resp.mark_changed();
    }
    let p = ui.painter();
    let c = r.center();
    let rad = size * 0.42;
    p.circle_filled(c, rad, Color32::from_rgb(58, 58, 58));
    p.circle_stroke(c, rad, Stroke::new(1.0, Color32::from_rgb(90, 90, 90)));
    let ang = knob_angle(*value, min, max);
    let zero_ang = knob_angle(if min < 0.0 && max > 0.0 { 0.0 } else { min }, min, max);
    let (a0, a1) = if ang < zero_ang {
        (ang, zero_ang)
    } else {
        (zero_ang, ang)
    };
    let steps = 16;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / steps as f32;
            c + vec2(a.cos(), a.sin()) * (rad + 2.0)
        })
        .collect();
    if a1 - a0 > 0.01 {
        p.add(egui::Shape::line(pts, Stroke::new(2.0, accent())));
    }
    p.line_segment(
        [c, c + vec2(ang.cos(), ang.sin()) * rad * 0.85],
        Stroke::new(2.0, Color32::WHITE),
    );
    let resp = resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// "Hot text": a blue number that changes when dragged left/right (cursor shows the direction),
/// double-click resets to `reset`.
pub fn hot_value(
    ui: &mut egui::Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    per_px: f32,
    reset: f32,
    text: &str,
    tip: &str,
) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_string(), FontId::proportional(11.5), time_blue());
    let (r, mut resp) = ui.allocate_exact_size(
        vec2(galley.size().x.max(18.0), 18.0),
        Sense::click_and_drag(),
    );
    if resp.dragged() {
        let d = resp.drag_delta().x - resp.drag_delta().y;
        if d != 0.0 {
            *value = (*value + d * per_px).clamp(min, max);
            resp.mark_changed();
        }
    }
    if resp.double_clicked() {
        *value = reset;
        resp.mark_changed();
    }
    let col = if resp.hovered() || resp.dragged() {
        Color32::from_rgb(120, 190, 255)
    } else {
        time_blue()
    };
    ui.painter().galley_with_override_text_color(
        pos2(r.left(), r.center().y - galley.size().y / 2.0),
        galley,
        col,
    );
    let resp = resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// Whether a level is a real over (above 0 dBFS). A file can only *touch* full scale: a 16-bit
/// -32768 sample reads exactly -1.0, and that is not clipping, so it must not light the clip
/// indicator. Only a mix, gain or effect can push the signal past 0 dBFS.
pub fn is_over(level: f32) -> bool {
    level.is_finite() && level > 1.0 + 1e-5
}

pub fn meter_color(dbv: f32) -> Color32 {
    if dbv > -3.0 {
        Color32::from_rgb(235, 60, 60)
    } else if dbv > -12.0 {
        Color32::from_rgb(235, 210, 60)
    } else {
        Color32::from_rgb(60, 200, 90)
    }
}

/// Continuous meter colour: green up to about -18 dB, through yellow, to red at 0.
fn meter_grad(d: f32) -> Color32 {
    let lerp = |a: (u8, u8, u8), b: (u8, u8, u8), t: f32| {
        let t = t.clamp(0.0, 1.0);
        let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
        Color32::from_rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
    };
    let green = (40, 190, 70);
    let yellow = (225, 215, 40);
    let red = (230, 45, 35);
    if d < -18.0 {
        lerp(green, green, 0.0)
    } else if d < -6.0 {
        lerp(green, yellow, (d + 18.0) / 12.0)
    } else {
        lerp(yellow, red, (d + 6.0) / 6.0)
    }
}

/// Horizontal level meter: one thin bar per channel along the top, the dB scale underneath,
/// and a clip light per channel at the right end. Returns the rect of the clip lights.
pub fn hmeter(p: &egui::Painter, r: Rect, levels: &[f32], holds: &[f32], clipped: &[bool]) -> Rect {
    let min_db = -60.0f32;
    let n = levels.len().clamp(1, 2);
    let bar_h = ((r.height() - 18.0) / n as f32 - 2.0).clamp(4.0, 9.0);
    let led_w = 12.0;
    let left = r.left() + 18.0;
    let right = r.right() - led_w - 6.0;
    let top = r.top() + 2.0;
    let bars_bottom = top + n as f32 * (bar_h + 2.0);
    let x_of = |d: f32| left + ((d - min_db) / (0.0 - min_db)).clamp(0.0, 1.0) * (right - left);
    p.rect_filled(
        Rect::from_min_max(pos2(left - 1.0, top - 1.0), pos2(right + 1.0, bars_bottom)),
        0.0,
        Color32::from_rgb(12, 12, 12),
    );
    for (i, l) in levels.iter().take(n).enumerate() {
        let y0 = top + i as f32 * (bar_h + 2.0);
        let br = Rect::from_min_max(pos2(left, y0), pos2(right, y0 + bar_h));
        p.rect_filled(br, 0.0, Color32::from_rgb(28, 28, 28));
        let xe = x_of(crate::audio::db(*l));
        // continuous gradient, drawn in 1 dB steps
        let mut d = min_db;
        while d < 0.0 {
            let a = x_of(d);
            let b = x_of(d + 1.0).min(xe);
            if b <= a {
                break;
            }
            p.rect_filled(
                Rect::from_min_max(pos2(a, br.top()), pos2(b + 0.5, br.bottom())),
                0.0,
                meter_grad(d + 0.5),
            );
            d += 1.0;
        }
        if let Some(h) = holds.get(i) {
            let hd = crate::audio::db(*h);
            if hd > min_db {
                let hx = x_of(hd);
                p.line_segment(
                    [pos2(hx, br.top()), pos2(hx, br.bottom())],
                    Stroke::new(2.0, meter_grad(hd)),
                );
            }
        }
        // clip light
        let led = Rect::from_min_size(pos2(right + 5.0, y0), vec2(led_w, bar_h));
        let on = clipped.get(i).copied().unwrap_or(false);
        p.rect_filled(
            led,
            0.0,
            if on {
                Color32::from_rgb(235, 40, 30)
            } else {
                Color32::from_rgb(50, 22, 22)
            },
        );
    }
    let marks = [
        -57, -54, -51, -48, -45, -42, -39, -36, -33, -30, -27, -24, -21, -18, -15, -12, -9, -6, -3,
        0,
    ];
    let sy = bars_bottom + 1.0;
    // from 0 dB down, so the top of the scale always gets its numbers; skip crowded ones
    let mut last_x = f32::MAX;
    for m in marks.iter().rev() {
        let x = x_of(*m as f32);
        p.line_segment(
            [pos2(x, sy), pos2(x, sy + 3.0)],
            Stroke::new(1.0, text_dim()),
        );
        if last_x - x >= 22.0 && x - left > 16.0 {
            p.text(
                pos2(x, sy + 3.0),
                egui::Align2::CENTER_TOP,
                format!("{m}"),
                FontId::proportional(9.5),
                text_dim(),
            );
            last_x = x;
        }
    }
    p.text(
        pos2(r.left() + 1.0, sy + 3.0),
        egui::Align2::LEFT_TOP,
        "dB",
        FontId::proportional(9.5),
        text_dim(),
    );
    Rect::from_min_max(
        pos2(right + 4.0, top - 1.0),
        pos2(right + 6.0 + led_w, bars_bottom),
    )
}

/// Vertical meter (used by the mixer).
pub fn vmeter(p: &egui::Painter, r: Rect, levels: &[f32]) {
    let min_db = -60.0f32;
    p.rect_filled(r, 0.0, Color32::from_rgb(20, 20, 20));
    let n = levels.len().max(1);
    let bw = r.width() / n as f32;
    let y_of = |d: f32| r.bottom() - ((d - min_db) / (0.0 - min_db)).clamp(0.0, 1.0) * r.height();
    for (i, l) in levels.iter().enumerate() {
        let x0 = r.left() + i as f32 * bw + 1.0;
        let d = crate::audio::db(*l);
        let ye = y_of(d);
        for (lo, hi) in [(-60.0, -12.0), (-12.0, -3.0), (-3.0, 0.0)] {
            let a = y_of(lo);
            let b = y_of(hi).max(ye);
            if b < a {
                p.rect_filled(
                    Rect::from_min_max(pos2(x0, b), pos2(x0 + bw - 2.0, a)),
                    0.0,
                    meter_color(hi - 0.5),
                );
            }
        }
    }
}

/// Where an icon sits in its button: a centred square about three quarters of the button's
/// height (the size of the original's toolbar and panel icons).
pub fn icon_box(r: Rect) -> Rect {
    let side = (r.height() * 0.74).min(r.width() - 4.0).max(6.0);
    Rect::from_center_size(r.center(), vec2(side, side))
}

/// A grey from AudioCraft's own dark scale, in the theme on screen. AudioCraft's theme gets the
/// grey itself; any other theme gets the colour of the surface or text that grey plays in
/// AudioCraft (lanes, panels, controls, hover, dividers, dim and normal text), blended between
/// neighbours, so fixed greys in the effect editors, meters and mixer follow every theme.
pub fn gray(v: u8) -> Color32 {
    if theme_kind() == 0 && !adjusting() {
        return Color32::from_gray(v);
    }
    gray_in(&palette(), is_dark(), v)
}

fn gray_in(p: &Palette, dark: bool, v: u8) -> Color32 {
    // the darkest greys (meter wells, plot backgrounds) sit a step below the lanes
    let well = if dark {
        lerp_color(p.track_lane_a, Color32::BLACK, 0.55)
    } else {
        lerp_color(p.track_lane_a, Color32::BLACK, 0.12)
    };
    let top = if dark {
        Color32::WHITE
    } else {
        Color32::from_rgb(10, 10, 12)
    };
    let anchors: [(u8, Color32); 12] = [
        (
            0,
            if dark {
                Color32::BLACK
            } else {
                lerp_color(p.track_lane_a, Color32::BLACK, 0.25)
            },
        ),
        (15, well),
        (26, p.track_lane_a),
        (31, p.track_lane_b),
        (35, p.panel),
        (40, p.ruler),
        (44, p.track_head),
        (48, p.control),
        (62, p.menu_hover),
        (72, p.track_divider),
        (170, p.text_dim),
        (240, p.text),
    ];
    if v >= 240 {
        return lerp_color(p.text, top, (v - 240) as f32 / 15.0);
    }
    for w in anchors.windows(2) {
        let (a, ca) = w[0];
        let (b, cb) = w[1];
        if v >= a && v <= b {
            return lerp_color(ca, cb, (v - a) as f32 / (b - a).max(1) as f32);
        }
    }
    Color32::from_gray(v)
}

/// A grey with transparency (see [`gray`]).
pub fn gray_a(v: u8, a: u8) -> Color32 {
    let c = gray(v);
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// The native title bar in the theme's colours (Windows 11: caption and text colour; older
/// Windows: at least dark or light). Runs when the theme changes.
pub fn apply_title_bar(hwnd: isize) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
        if hwnd == 0 {
            return;
        }
        let dark: i32 = if is_dark() { 1 } else { 0 };
        let rgb = |c: Color32| -> u32 { c.r() as u32 | (c.g() as u32) << 8 | (c.b() as u32) << 16 };
        let caption = rgb(panel_header());
        let text = rgb(text());
        let border = rgb(menu_edge());
        // SAFETY: plain attribute writes on our own window with correctly sized values.
        unsafe {
            let h = hwnd as windows_sys::Win32::Foundation::HWND;
            DwmSetWindowAttribute(h, 20, &dark as *const i32 as *const _, 4);
            DwmSetWindowAttribute(h, 35, &caption as *const u32 as *const _, 4);
            DwmSetWindowAttribute(h, 36, &text as *const u32 as *const _, 4);
            DwmSetWindowAttribute(h, 34, &border as *const u32 as *const _, 4);
        }
    }
    #[cfg(not(windows))]
    let _ = hwnd;
}

static TITLE_THEME: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(u16::MAX);

/// Re-colour the title bar once after each theme change (cheap to call every frame).
pub fn sync_title_bar(hwnd: isize) {
    use std::sync::atomic::Ordering;
    let k = theme_kind() as u16;
    if hwnd != 0 && TITLE_THEME.swap(k, Ordering::Relaxed) != k {
        apply_title_bar(hwnd);
    }
}

/// egui's slider with one addition: double-click the slider or its name to put the value back
/// to its default. The default is the one given with [`RSlider::default_value`], otherwise the
/// value the slider had the first time it was shown (normally the setting's default).
pub struct RSlider<'a, N: egui::emath::Numeric> {
    cell: &'a std::cell::Cell<N>,
    inner: egui::Slider<'a>,
    text: Option<egui::WidgetText>,
    default: Option<N>,
    vertical: bool,
}

impl<'a, N: egui::emath::Numeric> RSlider<'a, N> {
    pub fn new(value: &'a mut N, range: std::ops::RangeInclusive<N>) -> Self {
        let cell = std::cell::Cell::from_mut(value);
        let r = range.start().to_f64()..=range.end().to_f64();
        let inner = egui::Slider::from_get_set(r, move |v: Option<f64>| {
            if let Some(v) = v {
                cell.set(N::from_f64(v));
            }
            cell.get().to_f64()
        });
        let inner = if N::INTEGRAL { inner.integer() } else { inner };
        RSlider {
            cell,
            inner,
            text: None,
            default: None,
            vertical: false,
        }
    }
    /// The value a double-click restores.
    pub fn default_value(mut self, d: N) -> Self {
        self.default = Some(d);
        self
    }
    pub fn text(mut self, t: impl Into<egui::WidgetText>) -> Self {
        self.text = Some(t.into());
        self
    }
    pub fn suffix(mut self, s: impl ToString) -> Self {
        self.inner = self.inner.suffix(s.to_string());
        self
    }
    pub fn logarithmic(mut self, on: bool) -> Self {
        self.inner = self.inner.logarithmic(on);
        self
    }
    pub fn step_by(mut self, s: f64) -> Self {
        self.inner = self.inner.step_by(s);
        self
    }
    pub fn show_value(mut self, on: bool) -> Self {
        self.inner = self.inner.show_value(on);
        self
    }
    pub fn fixed_decimals(mut self, n: usize) -> Self {
        self.inner = self.inner.fixed_decimals(n);
        self
    }
    pub fn vertical(mut self) -> Self {
        self.inner = self.inner.vertical();
        self.vertical = true;
        self
    }
}

impl<N: egui::emath::Numeric> egui::Widget for RSlider<'_, N> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let RSlider {
            cell,
            inner,
            text,
            default,
            vertical,
        } = self;
        let before = cell.get();
        let body = |ui: &mut egui::Ui| {
            let mut r = ui.add(inner);
            let label = text.map(|t| ui.add(egui::Label::new(t).sense(Sense::click())));
            let key = egui::Id::new(("rslider_default", r.id));
            let first: Option<f64> = ui.data(|d| d.get_temp(key));
            if first.is_none() {
                ui.data_mut(|d| d.insert_temp(key, before.to_f64()));
            }
            let reset_to = default
                .map(|d| d.to_f64())
                .or(first)
                .unwrap_or(before.to_f64());
            let wants = r.double_clicked() || label.as_ref().is_some_and(|l| l.double_clicked());
            if wants && (cell.get().to_f64() - reset_to).abs() > f64::EPSILON {
                cell.set(N::from_f64(reset_to));
                r.mark_changed();
            }
            if let Some(l) = label {
                let _ = l.on_hover_text("Double-click to reset to the default");
            }
            r
        };
        if vertical {
            ui.vertical(body).inner
        } else {
            ui.horizontal(body).inner
        }
    }
}

/// Blender-style toggling by dragging. Press a toggle: it flips, and while the button stays
/// down every other toggle of the same `group` the pointer passes over takes that same new
/// state (ones already in it stay as they are). Call it for each toggle every frame with the
/// toggle's response (sense click-and-drag, so the drag stays with the toggles) instead of
/// flipping on `clicked()`. Returns true when `value` changed.
pub fn drag_toggle(ui: &egui::Ui, group: &str, resp: &egui::Response, value: &mut bool) -> bool {
    let key = egui::Id::new("ac_drag_toggle");
    let gid = egui::Id::new(("ac_drag_toggle_group", group));
    let (pressed, down, pos) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.interact_pos(),
        )
    });
    let over = pos.is_some_and(|p| resp.rect.contains(p)) && ui.is_enabled();
    let active: Option<(egui::Id, bool)> = ui.data(|d| d.get_temp(key));
    if pressed && over && resp.is_pointer_button_down_on() {
        let target = !*value;
        *value = target;
        ui.data_mut(|d| d.insert_temp(key, (gid, target)));
        return true;
    }
    match active {
        Some(_) if !down => {
            ui.data_mut(|d| d.remove::<(egui::Id, bool)>(key));
            false
        }
        Some((g, target)) if g == gid && over && *value != target => {
            *value = target;
            true
        }
        _ => false,
    }
}

/// The green power switch, which can be swept across its neighbours (see [`drag_toggle`]).
pub fn power_drag(ui: &mut egui::Ui, group: &str, on: &mut bool, tip: &str) -> bool {
    let resp = framed_icon_sense(
        ui,
        Icon::Power,
        *on,
        power_on(),
        tip,
        Sense::click_and_drag(),
    );
    drag_toggle(ui, group, &resp, on)
}

#[cfg(test)]
mod snap_indicator_tests {
    use super::*;

    /// The snapping indicator is the theme's contrast colour: red in every theme, and at
    /// least 3:1 against the timeline surfaces so a drag's snap line is always visible.
    #[test]
    fn snap_indicator_contrasts_with_the_timeline() {
        for i in 0..theme_count() {
            set_theme(i as u8);
            let ind = snap_line();
            let bg = wave_bg();
            assert!(
                contrast(ind, bg) >= 3.0,
                "theme {i}: snap indicator {:?} only {:.1}:1 on wave bg {:?}",
                ind,
                contrast(ind, bg),
                bg
            );
            assert!(
                ind.r() > ind.g() + 30 && ind.r() > ind.b() + 30,
                "theme {i}: snap indicator {:?} is not red",
                ind
            );
        }
        set_theme(0);
    }
}
