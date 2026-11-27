use crate::geom::{
    ellipse_path, fill_path_xf, optical_stroke, stroke_path_xf, HeadRigTransform,
};
use crate::palette::{rgb_color, CelPalette};
use pet_core::{CatAppearance, ExpressionState, Rgb8, SimSnapshot, Vec2};
use tiny_skia::{PathBuilder, Pixmap, Transform};

/// Render eyes, brows, cheek blush, nose, expressive mouth/yawn, and whiskers 100% through
/// `HeadRigTransform` so zero facial elements ever shear under tilt or squash-and-stretch.
#[allow(clippy::too_many_arguments)]
pub fn draw_face(
    pixmap: &mut Pixmap,
    app: &CatAppearance,
    pal: &CelPalette,
    sim: &SimSnapshot,
    head_xf: HeadRigTransform,
    s: f32,
    aa: bool,
) {
    let xf = head_xf.to_transform();

    // 1. Coat-adaptive cheek blush ovals (warm glow during head patting!)
    if sim.blush_amount > 0.03 {
        let mut blush_col = pal.blush;
        blush_col.set_alpha(blush_col.alpha() * sim.blush_amount.clamp(0.0, 1.0));
        for side in [-1.0f32, 1.0] {
            if let Some(b) = ellipse_path(side * 27.5 * s, 9.5 * s, 8.8 * s, 4.6 * s) {
                fill_path_xf(pixmap, &b, blush_col, xf, None, aa);
            }
            // Delicate twin anime/illustrated blush hatch ticks inside cheek blush
            let mut hpb = PathBuilder::new();
            for dx in [-2.2f32, 2.2] {
                hpb.move_to((side * 27.5 + dx + 1.2) * s, 7.6 * s);
                hpb.line_to((side * 27.5 + dx - 1.2) * s, 11.4 * s);
            }
            if let Some(hp) = hpb.finish() {
                let mut tick_col = pal.blush;
                tick_col.set_alpha((tick_col.alpha() * 1.15).min(0.95) * sim.blush_amount);
                stroke_path_xf(pixmap, &hp, tick_col, optical_stroke(0.9, s), xf, None, aa);
            }
        }
    }

    // 2. Mutually exclusive eye silhouette matrix (zero collision even at 0.6x Mini scale!)
    // Viewer-left = cat's right eye; Viewer-right = cat's left eye (supports heterochromia!)
    let eyes = [
        (Vec2::new(-14.2 * s, -1.5 * s), app.right_eye_color, -1.0f32),
        (Vec2::new(14.2 * s, -1.5 * s), app.left_eye_color, 1.0f32),
    ];
    let rx = 10.6 * s;
    let ry = 9.4 * s;

    match sim.expression {
        ExpressionState::Sleeping => {
            // Serene downward-curved U-eyelids (`︶  ︶`)
            for (ec, _, side) in eyes {
                let mut pb = PathBuilder::new();
                pb.move_to(ec.x - rx * 0.82, ec.y - 0.8 * s);
                pb.quad_to(ec.x, ec.y + 6.4 * s, ec.x + rx * 0.82, ec.y - 0.8 * s);
                if let Some(p) = pb.finish() {
                    stroke_path_xf(pixmap, &p, pal.lid_stroke, optical_stroke(2.4, s), xf, None, aa);
                }
                // Delicate outer sleep lash tick
                let mut lpb = PathBuilder::new();
                lpb.move_to(ec.x + side * rx * 0.72, ec.y + 0.4 * s);
                lpb.line_to(ec.x + side * (rx * 0.96), ec.y + 2.4 * s);
                if let Some(lp) = lpb.finish() {
                    stroke_path_xf(pixmap, &lp, pal.lid_stroke, optical_stroke(1.4, s), xf, None, aa);
                }
            }
        }
        ExpressionState::HappyPat => {
            // Joyful upward-curved arched eyes (`^  ^`) — unmistakable from sleep & focus!
            for (ec, _, _) in eyes {
                let mut pb = PathBuilder::new();
                pb.move_to(ec.x - rx * 0.80, ec.y + 3.4 * s);
                pb.quad_to(ec.x, ec.y - 7.8 * s, ec.x + rx * 0.80, ec.y + 3.4 * s);
                if let Some(p) = pb.finish() {
                    stroke_path_xf(pixmap, &p, pal.lid_stroke, optical_stroke(2.6, s), xf, None, aa);
                }
            }
        }
        ExpressionState::Yawning => {
            // Drowsy pinched-shut crescent eyes (`>  <`) paired with big open yawn mouth
            let yt = sim.yawn_t.clamp(0.0, 1.0);
            for (ec, eye_rgb, side) in eyes {
                if yt > 0.30 {
                    let mut pb = PathBuilder::new();
                    let in_x = ec.x - side * rx * 0.62;
                    let out_x = ec.x + side * rx * 0.72;
                    pb.move_to(out_x, ec.y - 4.2 * s);
                    pb.quad_to(in_x, ec.y, out_x, ec.y + 4.2 * s);
                    if let Some(p) = pb.finish() {
                        stroke_path_xf(pixmap, &p, pal.lid_stroke, optical_stroke(2.4, s), xf, None, aa);
                    }
                } else {
                    draw_open_cel_eye(pixmap, ec, eye_rgb, rx, ry, yt * 0.9, sim.eye_gaze, pal, xf, s, aa);
                }
            }
        }
        ExpressionState::PrivacyAverted => {
            // Politely averted open eyes looking cleanly sideways/up-left away from the keyboard!
            // Never peeking down at keys, and visually distinct from sleep/pat/focus!
            let averted_gaze = Vec2::new(
                sim.eye_gaze.x.clamp(-0.94, -0.62),
                sim.eye_gaze.y.clamp(-0.40, -0.10),
            );
            for (ec, eye_rgb, _) in eyes {
                draw_open_cel_eye(pixmap, ec, eye_rgb, rx, ry * 0.96, 0.08, averted_gaze, pal, xf, s, aa);
                // Polite slightly-raised calm brow arc above each averted eye
                let mut lpb = PathBuilder::new();
                lpb.move_to(ec.x - rx * 0.72, ec.y - ry * 1.12);
                lpb.quad_to(ec.x, ec.y - ry * 1.32, ec.x + rx * 0.72, ec.y - ry * 1.06);
                if let Some(lp) = lpb.finish() {
                    stroke_path_xf(pixmap, &lp, pal.lid_stroke, optical_stroke(1.55, s), xf, None, aa);
                }
            }
        }
        ExpressionState::Focused => {
            // Wide alert eyes tracking the active keyboard zone + crisp inward-angled focus brows!
            let close = sim.eyelid_close.clamp(0.0, 1.0);
            let f = sim.focus_intensity.clamp(0.0, 1.0);
            for (ec, eye_rgb, side) in eyes {
                draw_open_cel_eye(pixmap, ec, eye_rgb, rx, ry, close, sim.eye_gaze, pal, xf, s, aa);
                // Subtle inward-sloped focused brow line above each eye
                let mut bpb = PathBuilder::new();
                let y_out = ec.y - ry * (1.02 - f * 0.08);
                let y_in = ec.y - ry * (0.76 - f * 0.18);
                bpb.move_to(ec.x + side * rx * 0.78, y_out);
                bpb.line_to(ec.x - side * rx * 0.62, y_in);
                if let Some(bp) = bpb.finish() {
                    stroke_path_xf(pixmap, &bp, pal.lid_stroke, optical_stroke(2.1, s), xf, None, aa);
                }
            }
        }
        ExpressionState::Neutral => {
            let close = sim.eyelid_close.clamp(0.0, 1.0);
            for (ec, eye_rgb, _) in eyes {
                if close >= 0.88 {
                    let mut pb = PathBuilder::new();
                    pb.move_to(ec.x - rx * 0.82, ec.y + 1.0 * s);
                    pb.quad_to(ec.x, ec.y + 3.2 * s, ec.x + rx * 0.82, ec.y + 1.0 * s);
                    if let Some(p) = pb.finish() {
                        stroke_path_xf(pixmap, &p, pal.lid_stroke, optical_stroke(2.2, s), xf, None, aa);
                    }
                } else {
                    draw_open_cel_eye(pixmap, ec, eye_rgb, rx, ry, close, sim.eye_gaze, pal, xf, s, aa);
                }
            }
        }
    }

    // 3. Minimalist subtle nose dot/micro-triangle (almost invisible on matte black coat, cute on light coats)
    if !pal.is_dark_coat || sim.expression == ExpressionState::HappyPat {
        let mut nb = PathBuilder::new();
        nb.move_to(-2.4 * s, 8.2 * s);
        nb.quad_to(0.0, 7.6 * s, 2.4 * s, 8.2 * s);
        nb.line_to(0.0, 10.8 * s);
        nb.close();
        if let Some(np) = nb.finish() {
            let mut n_col = pal.nose_fill;
            if pal.is_dark_coat {
                n_col.set_alpha(0.35);
            }
            fill_path_xf(pixmap, &np, n_col, xf, None, aa);
        }
    }

    // 4. Mouth: Minimalist deadpan-cute in Neutral/Focused; expressive yawn & happy pat smile!
    if sim.expression == ExpressionState::Yawning && sim.yawn_t > 0.08 {
        let yt = sim.yawn_t.clamp(0.0, 1.0);
        let m_rx = (3.0 + yt * 6.8) * s;
        let m_ry = (2.2 + yt * 8.4) * s;
        let m_cy = (13.2 + yt * 4.6) * s;
        if let Some(mouth) = ellipse_path(0.0, m_cy, m_rx, m_ry) {
            fill_path_xf(pixmap, &mouth, pal.yawn_fill, xf, None, aa);
            // Soft pink curled tongue inside lower half of open yawn
            if let Some(tongue) = ellipse_path(0.0, m_cy + m_ry * 0.42, m_rx * 0.70, m_ry * 0.44) {
                fill_path_xf(pixmap, &tongue, pal.yawn_tongue, xf, None, aa);
            }
            stroke_path_xf(pixmap, &mouth, pal.yawn_rim, optical_stroke(1.25, s), xf, None, aa);
        }
    } else if sim.expression == ExpressionState::HappyPat {
        // Warm happy `ω` smile during head patting
        let mut mpb = PathBuilder::new();
        mpb.move_to(-5.4 * s, 13.6 * s);
        mpb.quad_to(-2.7 * s, 17.0 * s, 0.0, 13.2 * s);
        mpb.quad_to(2.7 * s, 17.0 * s, 5.4 * s, 13.6 * s);
        if let Some(mp) = mpb.finish() {
            stroke_path_xf(pixmap, &mp, pal.mouth_stroke, optical_stroke(1.3, s), xf, None, aa);
        }
    }

    // 5. Three delicate downward-angled fine cheek whiskers per side (LittleOne silhouette)
    let mut wb = PathBuilder::new();
    for side in [-1.0f32, 1.0] {
        for (dy_in, dy_mid, dy_out, len) in [
            (7.5f32, 8.8, 10.6, 15.0),
            (11.2, 13.0, 15.6, 16.5),
            (15.0, 17.4, 20.6, 14.0),
        ] {
            wb.move_to(side * 25.0 * s, dy_in * s);
            wb.quad_to(
                side * (25.0 + len * 0.55) * s,
                dy_mid * s,
                side * (25.0 + len) * s,
                dy_out * s,
            );
        }
    }
    if let Some(wp) = wb.finish() {
        let w_alpha = if pal.is_dark_coat { 0.54 } else { 0.72 };
        stroke_path_xf(
            pixmap,
            &wp,
            rgb_color(app.whisker_color, w_alpha),
            optical_stroke(0.85, s),
            xf,
            None,
            aa,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_open_cel_eye(
    pixmap: &mut Pixmap,
    ec: Vec2,
    eye_rgb: Rgb8,
    rx: f32,
    ry: f32,
    close: f32,
    gaze: Vec2,
    pal: &CelPalette,
    xf: Transform,
    s: f32,
    aa: bool,
) {
    let open_ry = (ry * (1.0 - close * 0.86)).max(1.8 * s);
    if let Some(eye_oval) = ellipse_path(ec.x, ec.y, rx, open_ry) {
        // Luminous flat chartreuse/iris fill (crisp LittleOne glow)
        fill_path_xf(pixmap, &eye_oval, rgb_color(eye_rgb, 1.0), xf, None, aa);

        // Subtle crisp dark iris rim on light coats so eyes stay sharp on white fur
        if !pal.is_dark_coat {
            stroke_path_xf(
                pixmap,
                &eye_oval,
                pal.outline,
                optical_stroke(0.95, s),
                xf,
                None,
                aa,
            );
        }

        // Clean vertical-oval matte black pupil tracking pointer gaze
        if close < 0.58 {
            let px = ec.x + gaze.x.clamp(-1.0, 1.0) * (rx * 0.44);
            let py = ec.y + gaze.y.clamp(-1.0, 1.0) * (open_ry * 0.38);
            let prx = 3.7 * s;
            let pry = (open_ry * 0.68).min(6.6 * s);
            if let Some(pupil) = ellipse_path(px, py, prx, pry) {
                fill_path_xf(pixmap, &pupil, rgb_color(Rgb8::new(10, 11, 14), 1.0), xf, None, aa);
            }
        }
    }
}
