use tiny_skia::{Color, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// Pure-Rust geometric vector stroke font for crisp, resolution-independent keycap legends,
/// Showcase floating keycap pop-ups, and sleep `z` particles (zero external font license/tofu risk).
///
/// Each glyph is defined in normalized `[0.0..1.0]` X by `[0.0..1.0]` Y (top-down) coordinates.
pub fn draw_text_centered(
    pixmap: &mut Pixmap,
    text: &str,
    cx: f32,
    cy: f32,
    glyph_h: f32,
    color: Color,
    stroke_scale: f32,
) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || glyph_h < 2.0 {
        return;
    }

    let glyph_w = glyph_h * 0.62;
    let gap = glyph_h * 0.18;
    let total_w = chars.len() as f32 * glyph_w + (chars.len().saturating_sub(1)) as f32 * gap;
    let start_x = cx - total_w * 0.5;
    let start_y = cy - glyph_h * 0.5;

    let mut pb = PathBuilder::new();
    for (i, ch) in chars.iter().enumerate() {
        let ox = start_x + i as f32 * (glyph_w + gap);
        append_glyph(&mut pb, ch.to_ascii_uppercase(), ox, start_y, glyph_w, glyph_h);
    }

    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        let stroke = Stroke {
            width: (glyph_h * 0.17 * stroke_scale).clamp(0.85, 4.5),
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

#[allow(clippy::too_many_arguments)]
fn seg(pb: &mut PathBuilder, ox: f32, oy: f32, w: f32, h: f32, x0: f32, y0: f32, x1: f32, y1: f32) {
    pb.move_to(ox + x0 * w, oy + y0 * h);
    pb.line_to(ox + x1 * w, oy + y1 * h);
}

fn poly(pb: &mut PathBuilder, ox: f32, oy: f32, w: f32, h: f32, pts: &[(f32, f32)]) {
    if let Some(&(x0, y0)) = pts.first() {
        pb.move_to(ox + x0 * w, oy + y0 * h);
        for &(x, y) in &pts[1..] {
            pb.line_to(ox + x * w, oy + y * h);
        }
    }
}

fn append_glyph(pb: &mut PathBuilder, ch: char, ox: f32, oy: f32, w: f32, h: f32) {
    match ch {
        'A' => {
            poly(pb, ox, oy, w, h, &[(0.05, 1.0), (0.5, 0.0), (0.95, 1.0)]);
            seg(pb, ox, oy, w, h, 0.22, 0.68, 0.78, 0.68);
        }
        'B' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[
                    (0.1, 1.0),
                    (0.1, 0.0),
                    (0.68, 0.0),
                    (0.88, 0.14),
                    (0.88, 0.34),
                    (0.68, 0.48),
                    (0.1, 0.48),
                ],
            );
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[
                    (0.68, 0.48),
                    (0.92, 0.62),
                    (0.92, 0.85),
                    (0.68, 1.0),
                    (0.1, 1.0),
                ],
            );
        }
        'C' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[
                    (0.9, 0.2),
                    (0.65, 0.0),
                    (0.32, 0.0),
                    (0.1, 0.25),
                    (0.1, 0.75),
                    (0.32, 1.0),
                    (0.65, 1.0),
                    (0.9, 0.8),
                ],
            );
        }
        'D' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[
                    (0.1, 1.0),
                    (0.1, 0.0),
                    (0.58, 0.0),
                    (0.9, 0.24),
                    (0.9, 0.76),
                    (0.58, 1.0),
                    (0.1, 1.0),
                ],
            );
        }
        'E' => {
            poly(pb, ox, oy, w, h, &[(0.88, 0.0), (0.12, 0.0), (0.12, 1.0), (0.88, 1.0)]);
            seg(pb, ox, oy, w, h, 0.12, 0.48, 0.72, 0.48);
        }
        'F' => {
            poly(pb, ox, oy, w, h, &[(0.88, 0.0), (0.12, 0.0), (0.12, 1.0)]);
            seg(pb, ox, oy, w, h, 0.12, 0.48, 0.72, 0.48);
        }
        'G' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[
                    (0.9, 0.2),
                    (0.65, 0.0),
                    (0.32, 0.0),
                    (0.1, 0.25),
                    (0.1, 0.75),
                    (0.32, 1.0),
                    (0.75, 1.0),
                    (0.9, 0.8),
                    (0.9, 0.54),
                    (0.55, 0.54),
                ],
            );
        }
        'H' => {
            seg(pb, ox, oy, w, h, 0.12, 0.0, 0.12, 1.0);
            seg(pb, ox, oy, w, h, 0.88, 0.0, 0.88, 1.0);
            seg(pb, ox, oy, w, h, 0.12, 0.5, 0.88, 0.5);
        }
        'I' => {
            seg(pb, ox, oy, w, h, 0.25, 0.0, 0.75, 0.0);
            seg(pb, ox, oy, w, h, 0.5, 0.0, 0.5, 1.0);
            seg(pb, ox, oy, w, h, 0.25, 1.0, 0.75, 1.0);
        }
        'J' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[(0.8, 0.0), (0.8, 0.78), (0.58, 1.0), (0.28, 1.0), (0.12, 0.82)],
            );
        }
        'K' => {
            seg(pb, ox, oy, w, h, 0.12, 0.0, 0.12, 1.0);
            seg(pb, ox, oy, w, h, 0.88, 0.0, 0.15, 0.52);
            seg(pb, ox, oy, w, h, 0.32, 0.42, 0.9, 1.0);
        }
        'L' => {
            poly(pb, ox, oy, w, h, &[(0.14, 0.0), (0.14, 1.0), (0.88, 1.0)]);
        }
        'M' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[(0.08, 1.0), (0.08, 0.0), (0.5, 0.58), (0.92, 0.0), (0.92, 1.0)],
            );
        }
        'N' => {
            poly(pb, ox, oy, w, h, &[(0.12, 1.0), (0.12, 0.0), (0.88, 1.0), (0.88, 0.0)]);
        }
        'O' | '0' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[
                    (0.32, 0.0),
                    (0.68, 0.0),
                    (0.92, 0.24),
                    (0.92, 0.76),
                    (0.68, 1.0),
                    (0.32, 1.0),
                    (0.08, 0.76),
                    (0.08, 0.24),
                    (0.32, 0.0),
                ],
            );
        }
        'P' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[
                    (0.12, 1.0),
                    (0.12, 0.0),
                    (0.68, 0.0),
                    (0.9, 0.16),
                    (0.9, 0.4),
                    (0.68, 0.55),
                    (0.12, 0.55),
                ],
            );
        }
        'Q' => {
            append_glyph(pb, 'O', ox, oy, w, h);
            seg(pb, ox, oy, w, h, 0.62, 0.72, 0.96, 1.04);
        }
        'R' => {
            append_glyph(pb, 'P', ox, oy, w, h);
            seg(pb, ox, oy, w, h, 0.54, 0.55, 0.9, 1.0);
        }
        'S' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[
                    (0.86, 0.16),
                    (0.66, 0.0),
                    (0.3, 0.0),
                    (0.12, 0.16),
                    (0.12, 0.34),
                    (0.32, 0.48),
                    (0.68, 0.54),
                    (0.9, 0.68),
                    (0.9, 0.86),
                    (0.68, 1.0),
                    (0.3, 1.0),
                    (0.1, 0.84),
                ],
            );
        }
        'T' => {
            seg(pb, ox, oy, w, h, 0.06, 0.0, 0.94, 0.0);
            seg(pb, ox, oy, w, h, 0.5, 0.0, 0.5, 1.0);
        }
        'U' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[(0.12, 0.0), (0.12, 0.78), (0.34, 1.0), (0.66, 1.0), (0.88, 0.78), (0.88, 0.0)],
            );
        }
        'V' => {
            poly(pb, ox, oy, w, h, &[(0.08, 0.0), (0.5, 1.0), (0.92, 0.0)]);
        }
        'W' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[(0.04, 0.0), (0.26, 1.0), (0.5, 0.42), (0.74, 1.0), (0.96, 0.0)],
            );
        }
        'X' => {
            seg(pb, ox, oy, w, h, 0.1, 0.0, 0.9, 1.0);
            seg(pb, ox, oy, w, h, 0.9, 0.0, 0.1, 1.0);
        }
        'Y' => {
            poly(pb, ox, oy, w, h, &[(0.1, 0.0), (0.5, 0.52), (0.9, 0.0)]);
            seg(pb, ox, oy, w, h, 0.5, 0.52, 0.5, 1.0);
        }
        'Z' => {
            poly(pb, ox, oy, w, h, &[(0.1, 0.0), (0.9, 0.0), (0.1, 1.0), (0.9, 1.0)]);
        }
        '1' => {
            poly(pb, ox, oy, w, h, &[(0.28, 0.22), (0.54, 0.0), (0.54, 1.0)]);
            seg(pb, ox, oy, w, h, 0.22, 1.0, 0.84, 1.0);
        }
        '2' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[(0.12, 0.24), (0.32, 0.0), (0.7, 0.0), (0.88, 0.22), (0.88, 0.38), (0.1, 1.0), (0.9, 1.0)],
            );
        }
        '3' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[(0.14, 0.14), (0.34, 0.0), (0.7, 0.0), (0.88, 0.2), (0.88, 0.36), (0.64, 0.48), (0.4, 0.48)],
            );
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[(0.64, 0.48), (0.9, 0.64), (0.9, 0.84), (0.68, 1.0), (0.32, 1.0), (0.12, 0.84)],
            );
        }
        '4' => {
            poly(pb, ox, oy, w, h, &[(0.72, 1.0), (0.72, 0.0), (0.08, 0.68), (0.92, 0.68)]);
        }
        '5' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[(0.86, 0.0), (0.16, 0.0), (0.14, 0.46), (0.66, 0.46), (0.88, 0.64), (0.88, 0.84), (0.66, 1.0), (0.28, 1.0), (0.12, 0.86)],
            );
        }
        '6' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[(0.82, 0.12), (0.6, 0.0), (0.32, 0.0), (0.12, 0.28), (0.12, 0.82), (0.32, 1.0), (0.68, 1.0), (0.88, 0.82), (0.88, 0.6), (0.68, 0.46), (0.16, 0.46)],
            );
        }
        '7' => {
            poly(pb, ox, oy, w, h, &[(0.1, 0.0), (0.9, 0.0), (0.38, 1.0)]);
        }
        '8' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[(0.32, 0.0), (0.68, 0.0), (0.86, 0.18), (0.86, 0.34), (0.68, 0.48), (0.32, 0.48), (0.14, 0.34), (0.14, 0.18), (0.32, 0.0)],
            );
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[(0.32, 0.48), (0.68, 0.48), (0.9, 0.66), (0.9, 0.84), (0.68, 1.0), (0.32, 1.0), (0.1, 0.84), (0.1, 0.66), (0.32, 0.48)],
            );
        }
        '9' => {
            poly(
                pb,
                ox,
                oy,
                w,
                h,
                &[(0.86, 0.52), (0.32, 0.52), (0.12, 0.36), (0.12, 0.16), (0.32, 0.0), (0.68, 0.0), (0.88, 0.18), (0.88, 0.76), (0.66, 1.0), (0.28, 1.0)],
            );
        }
        '-' => seg(pb, ox, oy, w, h, 0.14, 0.5, 0.86, 0.5),
        '=' => {
            seg(pb, ox, oy, w, h, 0.14, 0.36, 0.86, 0.36);
            seg(pb, ox, oy, w, h, 0.14, 0.66, 0.86, 0.66);
        }
        '[' => poly(pb, ox, oy, w, h, &[(0.7, 0.0), (0.3, 0.0), (0.3, 1.0), (0.7, 1.0)]),
        ']' => poly(pb, ox, oy, w, h, &[(0.3, 0.0), (0.7, 0.0), (0.7, 1.0), (0.3, 1.0)]),
        '\\' => seg(pb, ox, oy, w, h, 0.18, 0.0, 0.82, 1.0),
        '/' => seg(pb, ox, oy, w, h, 0.82, 0.0, 0.18, 1.0),
        ';' => {
            seg(pb, ox, oy, w, h, 0.5, 0.25, 0.5, 0.32);
            seg(pb, ox, oy, w, h, 0.52, 0.68, 0.4, 0.94);
        }
        '\'' => seg(pb, ox, oy, w, h, 0.5, 0.08, 0.46, 0.36),
        ',' => seg(pb, ox, oy, w, h, 0.54, 0.72, 0.42, 0.98),
        '.' => seg(pb, ox, oy, w, h, 0.48, 0.86, 0.52, 0.92),
        '^' => poly(pb, ox, oy, w, h, &[(0.15, 0.68), (0.5, 0.25), (0.85, 0.68)]),
        '<' => poly(pb, ox, oy, w, h, &[(0.72, 0.18), (0.26, 0.5), (0.72, 0.82)]),
        '>' => poly(pb, ox, oy, w, h, &[(0.28, 0.18), (0.74, 0.5), (0.28, 0.82)]),
        _ => {}
    }
}
