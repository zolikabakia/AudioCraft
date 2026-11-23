//! The Vocal Tract tab of Generate Speech: the tube's shape, two drag pads (tongue and
//! constriction), the voice settings and a few ready sounds.

use egui::{Align2, Color32, FontId, RichText, Sense, Stroke, pos2, vec2};

#[allow(non_snake_case)]
fn PANEL() -> Color32 {
    crate::theme::gray(29)
}
#[allow(non_snake_case)]
fn PLOT() -> Color32 {
    crate::theme::gray(16)
}
#[allow(non_snake_case)]
fn GRID() -> Color32 {
    crate::theme::gray(48)
}
#[allow(non_snake_case)]
fn LABEL() -> Color32 {
    crate::theme::gray(150)
}
const VALUE: Color32 = Color32::from_rgb(78, 168, 255);

pub fn tract_ui(ui: &mut egui::Ui, p: &mut crate::tract::TractParams) {
    // the tract seen from the side: glottis at the left, lips at the right, nose above
    let (oral, nose) = crate::tract::profile(p);
    let (r, _) =
        ui.allocate_exact_size(vec2(ui.available_width().min(430.0), 96.0), Sense::hover());
    let painter = ui.painter_at(r);
    painter.rect_filled(r, 0.0, PLOT());
    let mid = r.center().y + 14.0;
    let sx = r.width() / oral.len().max(1) as f32;
    let scale = 9.0;
    let mut top = Vec::new();
    let mut bot = Vec::new();
    for (i, d) in oral.iter().enumerate() {
        let x = r.left() + (i as f32 + 0.5) * sx;
        top.push(pos2(x, mid - d * scale));
        bot.push(pos2(x, mid + d * scale * 0.35));
    }
    painter.add(egui::Shape::line(
        top,
        Stroke::new(2.0, Color32::from_rgb(230, 140, 160)),
    ));
    painter.add(egui::Shape::line(
        bot,
        Stroke::new(2.0, Color32::from_rgb(230, 140, 160)),
    ));
    let nose_start = oral.len().saturating_sub(nose.len()) + 1;
    let nl: Vec<egui::Pos2> = nose
        .iter()
        .enumerate()
        .map(|(i, d)| {
            pos2(
                r.left() + ((nose_start + i) as f32 + 0.5) * sx,
                r.top() + 18.0 - d * 4.0,
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        nl,
        Stroke::new(1.5, Color32::from_rgb(200, 120, 200)),
    ));
    painter.text(
        r.left_top() + vec2(4.0, 2.0),
        Align2::LEFT_TOP,
        "glottis",
        FontId::proportional(9.5),
        LABEL(),
    );
    painter.text(
        r.right_top() + vec2(-4.0, 2.0),
        Align2::RIGHT_TOP,
        "lips / nose",
        FontId::proportional(9.5),
        LABEL(),
    );
    ui.horizontal(|ui| {
        pad(
            ui,
            "Tongue",
            (&mut p.tongue_index, 12.0, 29.0),
            (&mut p.tongue_diameter, 2.0, 3.5),
            "Drag: back \u{2194} front, high \u{2195} low",
        );
        pad(
            ui,
            "Constriction",
            (&mut p.constriction_index, 10.0, 43.0),
            (&mut p.constriction_diameter, 0.0, 3.5),
            "Drag: where the tract narrows and how much (top = open)",
        );
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Voice:").small().color(LABEL()));
        for (i, v) in crate::tract::VOICES.iter().enumerate() {
            let on =
                (p.pitch - v.1).abs() < 0.5 && (p.formant - v.5).abs() < 0.01 && p.whisper == v.6;
            if ui
                .selectable_label(on, v.0)
                .on_hover_text("Use this ready voice (pitch, tone, size); the words stay")
                .clicked()
            {
                p.set_voice(i);
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        let sounds: [(&str, f32, f32, f32, f32, f32); 7] = [
            ("ah", 13.5, 2.6, 40.0, 3.5, 0.01),
            ("eh", 22.0, 2.4, 40.0, 3.5, 0.01),
            ("ee", 27.5, 2.05, 40.0, 3.5, 0.01),
            ("oh", 14.0, 2.1, 41.0, 1.6, 0.01),
            ("oo", 19.5, 2.0, 41.0, 1.0, 0.01),
            ("mm", 13.5, 2.6, 41.0, 0.0, 0.4),
            ("ss", 22.0, 2.4, 36.5, 0.35, 0.01),
        ];
        ui.label(RichText::new("Hold:").small().color(LABEL()));
        for (name, ti, td, ci, cd, velum) in sounds {
            if ui
                .small_button(name)
                .on_hover_text("Hold this sound (clears the text)")
                .clicked()
            {
                p.tongue_index = ti;
                p.tongue_diameter = td;
                p.constriction_index = ci;
                p.constriction_diameter = cd;
                p.velum = velum;
                p.script.clear();
            }
        }
    });
    egui::Grid::new("tract_grid")
        .num_columns(2)
        .spacing([8.0, 3.0])
        .show(ui, |ui| {
            ui.label("Say");
            ui.add(
                egui::TextEdit::singleline(&mut p.script)
                    .desired_width(300.0)
                    .hint_text("e.g. hello world (empty = hold the shape)"),
            );
            ui.end_row();
            ui.label("Pitch");
            ui.add(
                crate::theme::RSlider::new(&mut p.pitch, 50.0..=500.0)
                    .logarithmic(true)
                    .suffix(" Hz"),
            );
            ui.end_row();
            ui.label("Voice size");
            ui.add(
                crate::theme::RSlider::new(&mut p.formant, 0.7..=1.5)
                    .text("larger \u{2192} smaller")
                    .fixed_decimals(2),
            )
            .on_hover_text("Formants: a smaller tract sounds younger or more feminine, a larger one deeper; the pitch stays");
            ui.end_row();
            ui.label("Whisper");
            ui.checkbox(&mut p.whisper, "breath instead of voice");
            ui.end_row();
            ui.label("Tenseness");
            ui.add(crate::theme::RSlider::new(&mut p.tenseness, 0.0..=1.0).text("breathy \u{2192} pressed"));
            ui.end_row();
            ui.label("Vibrato");
            ui.add(crate::theme::RSlider::new(&mut p.vibrato, 0.0..=1.0).suffix(" st"));
            ui.end_row();
            ui.label("Nasal (velum)");
            ui.add(crate::theme::RSlider::new(&mut p.velum, 0.01..=0.4));
            ui.end_row();
            ui.label("Speed");
            ui.add(crate::theme::RSlider::new(&mut p.speed, 0.25..=3.0));
            ui.end_row();
            ui.label("Intonation");
            ui.add(crate::theme::RSlider::new(&mut p.intonation, 0.0..=8.0).suffix(" st fall"));
            ui.end_row();
            ui.label("Held length");
            ui.add(crate::theme::RSlider::new(&mut p.seconds, 0.1..=10.0).suffix(" s"));
            ui.end_row();
        });
    ui.label(RichText::new("A physical model of the voice (glottis, vocal tract and nose) after the Pink Trombone model. Words are read with simple English spelling rules (ee, oo, ou, ng, ph, th, sh, ch...); end with ? for a question's rising pitch.").small().color(LABEL()));
}

/// A small XY pad: x and y map onto two ranges (y grows upward).
fn pad(
    ui: &mut egui::Ui,
    label: &str,
    x: (&mut f32, f32, f32),
    y: (&mut f32, f32, f32),
    tip: &str,
) {
    let (xv, x0, x1) = x;
    let (yv, y0, y1) = y;
    ui.vertical(|ui| {
        ui.label(RichText::new(label).small().color(LABEL()));
        let (r, resp) = ui.allocate_exact_size(vec2(205.0, 78.0), Sense::click_and_drag());
        let painter = ui.painter_at(r);
        painter.rect_filled(r, 0.0, PANEL());
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, GRID()), egui::StrokeKind::Inside);
        if let Some(pp) = resp
            .interact_pointer_pos()
            .filter(|_| resp.dragged() || resp.clicked())
        {
            let fx = ((pp.x - r.left()) / r.width()).clamp(0.0, 1.0);
            let fy = ((r.bottom() - pp.y) / r.height()).clamp(0.0, 1.0);
            *xv = x0 + fx * (x1 - x0);
            *yv = y0 + fy * (y1 - y0);
        }
        let px = r.left() + ((*xv - x0) / (x1 - x0)).clamp(0.0, 1.0) * r.width();
        let py = r.bottom() - ((*yv - y0) / (y1 - y0)).clamp(0.0, 1.0) * r.height();
        painter.circle_filled(pos2(px, py), 6.0, VALUE);
        painter.text(
            r.right_bottom() - vec2(4.0, 2.0),
            Align2::RIGHT_BOTTOM,
            format!("{:.1} / {:.2}", xv, yv),
            FontId::proportional(9.5),
            LABEL(),
        );
        let _ = resp.on_hover_text(tip);
    });
}
