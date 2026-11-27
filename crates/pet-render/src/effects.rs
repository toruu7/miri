use crate::font::draw_text_centered;
use crate::geom::{fill_path_xf, optical_stroke, rounded_rect, stroke_path_xf, BBoxAcc};
use pet_core::{project_25d, SimSnapshot, Vec3};
use tiny_skia::{Color, Pixmap, Transform};

/// Pass 8a: Crisp compact tilted 2.5D white keycap pop-up badges (LittleOne style).
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
        let alpha = if t < 0.12 {
            t / 0.12
        } else if t > 0.72 {
            (1.0 - t) / 0.28
        } else {
            1.0
        }
        .clamp(0.0, 1.0);

        let pos = project_25d(Vec3::new(pop.x, pop.y, 28.0), wf, hf);
        let pop_scale = if t < 0.15 {
            0.78 + (t / 0.15) * 0.22
        } else {
            1.0 - t * 0.08
        };
        let cap_w = if pop.wide {
            42.0 * s * pop_scale
        } else if pop.label.len() > 2 {
            28.0 * s * pop_scale
        } else {
            18.5 * s * pop_scale
        };
        let cap_h = 17.5 * s * pop_scale;

        // Playful deterministic subtle tilt per popup (derived from horizontal velocity)
        let tilt_deg = (pop.vx * 0.9).clamp(-9.0, 9.0);
        let xf = Transform::from_rotate(tilt_deg).post_translate(pos.x, pos.y);

        let pad = 6.0 * s;
        bbox.include_rect(
            pos.x - cap_w * 0.5 - pad,
            pos.y - cap_h * 0.5 - pad,
            cap_w + pad * 2.0,
            cap_h + pad * 2.4,
        );

        if let Some(skirt) = rounded_rect(
            -cap_w * 0.5,
            -cap_h * 0.5 + 2.2 * s,
            cap_w,
            cap_h,
            4.2 * s,
        ) {
            fill_path_xf(
                pixmap,
                &skirt,
                Color::from_rgba(0.12, 0.13, 0.17, alpha * 0.90).unwrap(),
                xf,
                None,
                aa,
            );
        }
        if let Some(face) = rounded_rect(-cap_w * 0.5, -cap_h * 0.5, cap_w, cap_h, 4.2 * s) {
            fill_path_xf(
                pixmap,
                &face,
                Color::from_rgba(0.97, 0.98, 0.99, alpha).unwrap(),
                xf,
                None,
                aa,
            );
            stroke_path_xf(
                pixmap,
                &face,
                Color::from_rgba(0.14, 0.15, 0.19, alpha).unwrap(),
                optical_stroke(1.15, s),
                xf,
                None,
                aa,
            );
        }
        let font_h = if pop.label.len() > 2 {
            6.2 * s * pop_scale
        } else {
            9.2 * s * pop_scale
        };
        draw_text_centered(
            pixmap,
            &pop.label,
            pos.x,
            pos.y - 0.3 * s,
            font_h,
            Color::from_rgba(0.10, 0.12, 0.15, alpha).unwrap(),
            1.15,
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
