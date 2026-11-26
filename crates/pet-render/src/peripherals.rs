use crate::cache::StaticMouseSprite;
use crate::font::draw_text_centered;
use crate::geom::{fill_path, optical_stroke, rounded_quad, rounded_rect, stroke_path, BBoxAcc};
use crate::palette::rgb_color;
use pet_core::{
    keycap_specs, keyboard_local_to_world, mouse_world_pos, project_25d, KeyCapSpec,
    PeripheralStyle, Rgb8, SimSnapshot, Vec2,
};
use tiny_skia::{Color, PathBuilder, Pixmap};

/// Render a single sculpted bevelled 3-tone keycap (used both in static desk cache baking
/// and for dynamic pressed/glowing keycaps).
#[allow(clippy::too_many_arguments)]
pub fn draw_single_keycap_sculpted(
    pixmap: &mut Pixmap,
    spec: &KeyCapSpec,
    per: &PeripheralStyle,
    depress: f32,
    glow: f32,
    wf: f32,
    hf: f32,
    s: f32,
    origin_offset: Vec2,
    show_legends: bool,
    aa: bool,
) {
    let u_half = (spec.width_u / 16.0) * 0.93;
    let y_half = 0.165;
    let x0 = spec.kb_x - u_half;
    let x1 = spec.kb_x + u_half;
    let y0 = spec.kb_y - y_half;
    let y1 = spec.kb_y + y_half;

    let shift = |p: Vec2| Vec2::new(p.x - origin_offset.x, p.y - origin_offset.y);

    // Bug #6 fix: when an active keycap is depressed over the cached unpressed keycap,
    // first fill its unpressed footprint with dark recessed switch-well shade so zero white
    // ghost rim peeks out along the top edge!
    if depress > 0.02 {
        let u_tl = shift(project_25d(keyboard_local_to_world(x0, y0, 0.0), wf, hf));
        let u_tr = shift(project_25d(keyboard_local_to_world(x1, y0, 0.0), wf, hf));
        let u_br = shift(project_25d(keyboard_local_to_world(x1, y1, 0.0), wf, hf));
        let u_bl = shift(project_25d(keyboard_local_to_world(x0, y1, 0.0), wf, hf));
        if let Some(well) = rounded_quad(
            Vec2::new(u_tl.x - 0.5 * s, u_tl.y - 0.5 * s),
            Vec2::new(u_tr.x + 0.5 * s, u_tr.y - 0.5 * s),
            Vec2::new(u_br.x + 0.5 * s, u_br.y + 1.8 * s),
            Vec2::new(u_bl.x - 0.5 * s, u_bl.y + 1.8 * s),
            1.6 * s,
        ) {
            fill_path(
                pixmap,
                &well,
                rgb_color(per.keyboard_case.scale(0.46), 1.0),
                aa,
            );
        }
    }

    let tl = shift(project_25d(keyboard_local_to_world(x0, y0, depress), wf, hf));
    let tr = shift(project_25d(keyboard_local_to_world(x1, y0, depress), wf, hf));
    let br = shift(project_25d(keyboard_local_to_world(x1, y1, depress), wf, hf));
    let bl = shift(project_25d(keyboard_local_to_world(x0, y1, depress), wf, hf));

    let base_rgb = if spec.is_accent {
        per.keycap_accent
    } else {
        per.keycap_base
    };
    let face_rgb = base_rgb.lerp(per.keycap_highlight, glow.clamp(0.0, 1.0));
    let side_rgb = face_rgb.scale(0.68).lerp(Rgb8::new(26, 28, 42), 0.14);
    let dish_rgb = face_rgb.lerp(Rgb8::new(255, 255, 255), 0.24);

    // 1. Sloped keycap side skirt
    let drop_y = (2.1 - depress * 1.4).max(0.45) * s;
    if let Some(p) = rounded_quad(
        Vec2::new(tl.x, tl.y + drop_y),
        Vec2::new(tr.x, tr.y + drop_y),
        Vec2::new(br.x, br.y + drop_y),
        Vec2::new(bl.x, bl.y + drop_y),
        1.7 * s,
    ) {
        fill_path(pixmap, &p, rgb_color(side_rgb, 1.0), aa);
    }

    // 2. Main sculpted keycap body
    if let Some(p) = rounded_quad(tl, tr, br, bl, 1.6 * s) {
        fill_path(pixmap, &p, rgb_color(face_rgb, 1.0), aa);
    }

    // 3. Subtle sculpted top dish bevel (makes keys read as tactile mechanical caps!)
    let dish_inset = 0.14;
    let d_tl = tl.lerp(br, dish_inset);
    let d_tr = tr.lerp(bl, dish_inset);
    let d_br = br.lerp(tl, dish_inset * 1.25);
    let d_bl = bl.lerp(tr, dish_inset * 1.25);
    if let Some(dish) = rounded_quad(d_tl, d_tr, d_br, d_bl, 1.0 * s) {
        fill_path(pixmap, &dish, rgb_color(dish_rgb, 0.55), aa);
    }

    if show_legends && spec.label.len() <= 2 {
        let cx = (tl.x + tr.x + br.x + bl.x) * 0.25;
        let cy = (tl.y + tr.y + br.y + bl.y) * 0.25;
        draw_text_centered(
            pixmap,
            spec.label,
            cx,
            cy,
            5.1 * s,
            rgb_color(per.keycap_legend_color, 0.86),
            0.95,
        );
    }
}

/// Draw ONLY depressed/glowing keycaps on top of the tight-cropped static desk cache.
/// Bug #3 guard: completely skipped while `sim.privacy_active` is true!
#[allow(clippy::too_many_arguments)]
pub fn draw_active_keycaps(
    pixmap: &mut Pixmap,
    per: &PeripheralStyle,
    sim: &SimSnapshot,
    wf: f32,
    hf: f32,
    s: f32,
    show_legends: bool,
    aa: bool,
    bbox: &mut BBoxAcc,
) {
    if sim.privacy_active {
        return;
    }
    for row in (0..=4u8).rev() {
        for spec in keycap_specs().iter().filter(|k| k.row == row) {
            let idx = spec.key.index();
            let press = sim.key_press_amounts[idx];
            let glow = sim.key_glow_amounts[idx];
            if press > 0.01 || glow > 0.01 {
                draw_single_keycap_sculpted(
                    pixmap,
                    spec,
                    per,
                    press,
                    glow,
                    wf,
                    hf,
                    s,
                    Vec2::default(),
                    show_legends,
                    aa,
                );
                let c = project_25d(keyboard_local_to_world(spec.kb_x, spec.kb_y, press), wf, hf);
                let rw = (spec.width_u / 16.0 * 112.0 + 6.0) * s;
                let rh = 14.0 * s;
                bbox.include_rect(c.x - rw, c.y - rh, rw * 2.0, rh * 2.0);
            }
        }
    }
}

/// Draw the cel-shaded miniature mouse sprite + unoccluded front LMB/RMB click depressions
/// + continuous rolling scroll-wheel tread notches (legible down to `0.6x` Mini scale).
#[allow(clippy::too_many_arguments)]
pub fn draw_mouse(
    pixmap: &mut Pixmap,
    sprite: &mut StaticMouseSprite,
    per: &PeripheralStyle,
    sim: &SimSnapshot,
    wf: f32,
    hf: f32,
    s: f32,
    chroma_hard_edge: bool,
    bbox: &mut BBoxAcc,
) {
    let aa = !chroma_hard_edge;
    let raw_pos = project_25d(mouse_world_pos(sim.mouse_offset.x, sim.mouse_offset.y), wf, hf);
    let pos = Vec2::new(raw_pos.x.round(), raw_pos.y.round());
    let rx = 19.5 * s;
    let ry = 23.0 * s;

    // 1. Blit pre-baked tight mouse shell sprite
    sprite.ensure_and_blit(pixmap, pos, s, per, chroma_hard_edge);
    bbox.include_rect(
        pos.x - rx - 8.0 * s,
        pos.y - ry - 8.0 * s,
        (rx + 8.0 * s) * 2.0,
        (ry + 10.0 * s) * 2.0,
    );

    // 2. Unoccluded front LMB (viewer-right = cat's index finger) & RMB (viewer-left = cat's middle finger)
    // Drawn on the lower/front half (`y > pos.y`) facing the viewer, cleanly outside the paw on the upper palm dome!
    let lmb = sim.mouse_left_press.clamp(0.0, 1.0);
    if lmb > 0.02 {
        draw_mouse_button_lobe(
            pixmap,
            pos,
            rx,
            ry,
            1.0, // viewer-right = cat's Left Click (LMB)
            lmb,
            per.mouse_body,
            per.keycap_highlight,
            s,
            aa,
        );
    }

    let rmb = sim.mouse_right_press.clamp(0.0, 1.0);
    if rmb > 0.02 {
        draw_mouse_button_lobe(
            pixmap,
            pos,
            rx,
            ry,
            -1.0, // viewer-left = cat's Right Click (RMB)
            rmb,
            per.mouse_body,
            per.keycap_accent, // distinct warm accent tint vs LMB!
            s,
            aa,
        );
    }

    // 3. Recessed scroll wheel with continuous rolling tread notches (`mouse_scroll_angle`)
    let well_x = pos.x;
    let well_top = pos.y + 3.6 * s;
    let wheel_w = 4.2 * s;
    let wheel_h = 10.6 * s;
    let pulse_shift = sim.mouse_scroll_pulse.clamp(-1.0, 1.0) * 1.1 * s;
    let scroll_glow = sim.mouse_scroll_pulse.abs().clamp(0.0, 1.0);

    // Wheel cylinder body
    let wheel_col = per
        .mouse_wheel
        .lerp(per.keycap_highlight, scroll_glow * 0.55);
    if let Some(wheel) = rounded_rect(
        well_x - wheel_w * 0.5,
        well_top + pulse_shift * 0.35,
        wheel_w,
        wheel_h,
        1.8 * s,
    ) {
        fill_path(pixmap, &wheel, rgb_color(wheel_col, 1.0), aa);
        stroke_path(
            pixmap,
            &wheel,
            rgb_color(per.mouse_wheel.scale(0.35), 0.92),
            optical_stroke(0.95, s),
            aa,
        );
    }

    // 3 rolling horizontal tread notches moving continuously with `mouse_scroll_angle`
    let notch_col = rgb_color(
        wheel_col.lerp(Rgb8::new(255, 255, 255), 0.55 + scroll_glow * 0.25),
        0.92,
    );
    let notch_w = optical_stroke(1.15, s);
    let mut npb = PathBuilder::new();
    for i in 0..3 {
        let phase = ((i as f32) + sim.mouse_scroll_angle).rem_euclid(3.0) / 3.0;
        let ny = well_top + 1.6 * s + phase * (wheel_h - 3.2 * s);
        npb.move_to(well_x - wheel_w * 0.38, ny);
        npb.line_to(well_x + wheel_w * 0.38, ny);
    }
    if let Some(np) = npb.finish() {
        stroke_path(pixmap, &np, notch_col, notch_w, aa);
    }

    // Subtle directional micro-chevron cue beside scroll wheel only while actively scrolling
    if scroll_glow > 0.15 {
        let dir = if sim.mouse_scroll_pulse >= 0.0 { -1.0 } else { 1.0 };
        let cy = well_top + wheel_h * 0.5 + dir * (wheel_h * 0.56);
        let mut cpb = PathBuilder::new();
        cpb.move_to(well_x - 2.4 * s, cy - dir * 1.4 * s);
        cpb.line_to(well_x, cy + dir * 1.1 * s);
        cpb.line_to(well_x + 2.4 * s, cy - dir * 1.4 * s);
        if let Some(cp) = cpb.finish() {
            stroke_path(
                pixmap,
                &cp,
                rgb_color(per.keycap_highlight, scroll_glow * 0.90),
                optical_stroke(1.25, s),
                aa,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_mouse_button_lobe(
    pixmap: &mut Pixmap,
    pos: Vec2,
    rx: f32,
    ry: f32,
    side: f32, // +1.0 = viewer-right (LMB), -1.0 = viewer-left (RMB)
    press: f32,
    mouse_body: Rgb8,
    accent: Rgb8,
    s: f32,
    aa: bool,
) {
    let build_lobe = |dy: f32, inset: f32| -> Option<tiny_skia::Path> {
        let x_in = pos.x + side * (2.6 * s + inset);
        let x_out = pos.x + side * (rx * 0.90 - inset);
        let y_top = pos.y + 0.6 * s + dy + inset * 0.5;
        let y_bot = pos.y + ry * 0.90 + dy - inset;
        let mut pb = PathBuilder::new();
        pb.move_to(x_in, y_top);
        pb.line_to(x_out, y_top + 0.5 * s);
        pb.quad_to(
            pos.x + side * (rx * 0.88 - inset),
            pos.y + ry * 0.72 + dy,
            x_in,
            y_bot,
        );
        pb.close();
        pb.finish()
    };

    // Dark recessed inner switch-well rim revealed as the button lobe depresses
    if let Some(recess) = build_lobe(0.0, 0.0) {
        fill_path(
            pixmap,
            &recess,
            Color::from_rgba(0.08, 0.10, 0.14, press * 0.55).unwrap(),
            aa,
        );
    }

    // Depressed sculpted front button cap tinted cleanly with LMB highlight or RMB accent
    let drop = 1.75 * s * press;
    let btn_rgb = mouse_body.lerp(accent, (0.35 + press * 0.48).min(0.88));
    if let Some(cap) = build_lobe(drop, 0.6 * s) {
        fill_path(pixmap, &cap, rgb_color(btn_rgb, 0.96), aa);
        stroke_path(
            pixmap,
            &cap,
            rgb_color(accent.scale(0.62), press * 0.95),
            optical_stroke(1.25, s),
            aa,
        );
    }
}
