use crate::font::draw_text_centered;
use crate::geom::{fill_path, optical_stroke, rounded_rect, stroke_path, BBoxAcc};
use pet_core::{project_25d, SimSnapshot, Vec3};
use tiny_skia::{Color, Pixmap};

/// Pass 8a: Floating Showcase white keycap pop-ups (opt-in Showcase Mode only).
pub fn draw_popups(
    pixmap: &mut Pixmap,
    sim: &SimSnapshot,
    wf: f32,
    hf: f32,
    s: f32,
    aa: bool,
    bbox: &mut BBoxAcc,
) {
    for pop in &sim.popups {
        let t = (pop.age / pop.ttl).clamp(0.0, 1.0);
        let alpha = if t < 0.15 {
            t / 0.15
        } else if t > 0.72 {
            (1.0 - t) / 0.28
        } else {
            1.0
        }
        .clamp(0.0, 1.0);

        let pos = project_25d(Vec3::new(pop.x, pop.y, 28.0), wf, hf);
        let cap_w = if pop.wide {
            56.0 * s
        } else if pop.label.len() > 2 {
            34.0 * s
        } else {
            23.0 * s
        };
        let cap_h = 22.0 * s;

        bbox.include_rect(
            pos.x - cap_w * 0.5 - 3.0 * s,
            pos.y - cap_h * 0.5 - 3.0 * s,
            cap_w + 6.0 * s,
            cap_h + 8.0 * s,
        );

        if let Some(skirt) = rounded_rect(
            pos.x - cap_w * 0.5,
            pos.y - cap_h * 0.5 + 2.6 * s,
            cap_w,
            cap_h,
            5.0 * s,
        ) {
            fill_path(
                pixmap,
                &skirt,
                Color::from_rgba(0.14, 0.15, 0.20, alpha * 0.86).unwrap(),
                aa,
            );
        }
        if let Some(face) =
            rounded_rect(pos.x - cap_w * 0.5, pos.y - cap_h * 0.5, cap_w, cap_h, 5.0 * s)
        {
            fill_path(
                pixmap,
                &face,
                Color::from_rgba(0.97, 0.98, 0.99, alpha).unwrap(),
                aa,
            );
            stroke_path(
                pixmap,
                &face,
                Color::from_rgba(0.18, 0.20, 0.24, alpha).unwrap(),
                optical_stroke(1.3, s),
                aa,
            );
        }
        let font_h = if pop.label.len() > 2 { 7.5 * s } else { 11.5 * s };
        draw_text_centered(
            pixmap,
            &pop.label,
            pos.x,
            pos.y,
            font_h,
            Color::from_rgba(0.12, 0.14, 0.18, alpha).unwrap(),
            1.1,
        );
    }
}

/// Pass 8b: Crisp outlined floating Drowsy `Z` sleep particles.
pub fn draw_sleep_particles(
    pixmap: &mut Pixmap,
    sim: &SimSnapshot,
    wf: f32,
    hf: f32,
    s: f32,
    bbox: &mut BBoxAcc,
) {
    for zp in &sim.sleep_particles {
        let t = (zp.age / zp.ttl).clamp(0.0, 1.0);
        let alpha = ((t / 0.2).min(1.0) * ((1.0 - t) / 0.35).min(1.0)).clamp(0.0, 1.0);
        let pos = project_25d(Vec3::new(zp.x, zp.y, 18.0), wf, hf);
        let size = 13.5 * zp.scale * s;
        bbox.include_circle(pos, size * 1.15);

        draw_text_centered(
            pixmap,
            "Z",
            pos.x,
            pos.y,
            size,
            Color::from_rgba(0.14, 0.16, 0.22, alpha * 0.90).unwrap(),
            2.3,
        );
        draw_text_centered(
            pixmap,
            "Z",
            pos.x,
            pos.y,
            size,
            Color::from_rgba(0.95, 0.96, 1.0, alpha * 0.98).unwrap(),
            1.15,
        );
    }
}
