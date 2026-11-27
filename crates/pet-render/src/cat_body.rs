use crate::cache::CroppedMaskScratch;
use crate::cat_face::draw_face;
use crate::geom::{
    build_tapered_ribbon, ellipse_path, eval_quad, fill_path, fill_path_xf, optical_stroke,
    stroke_path, stroke_path_xf, BBoxAcc, HeadRigTransform,
};
use crate::palette::{rgb_color, CelPalette};
use pet_core::{
    project_25d, CatAppearance, HitRect, MarkingPattern, RigAnchors, SimSnapshot, Vec2, Vec3,
};
use tiny_skia::{
    Color, FillRule, LineCap, Mask, Paint, Path, PathBuilder, Pixmap, Stroke, Transform,
};

/// Pass 5: Unified cel-shaded pear torso, re-staged tucked back paws, head + expressive ears,
/// zero-shear clipped coat markings, and face. Returns a stable neutral `head_rect_px` hitbox.
#[allow(clippy::too_many_arguments)]
pub fn draw_cat_body_and_head(
    pixmap: &mut Pixmap,
    mask_scratch: &mut CroppedMaskScratch,
    app: &CatAppearance,
    pal: &CelPalette,
    sim: &SimSnapshot,
    wf: f32,
    hf: f32,
    s: f32,
    aa: bool,
    bbox: &mut BBoxAcc,
) -> HitRect {
    let purr_swell = sim.purr_intensity * (0.5 + 0.5 * sim.purr_phase.sin()) * 2.4;
    let breath = sim.breath_phase.sin() * 1.4 * (1.0 - sim.sleep_t * 0.4) + purr_swell;
    let bounce = -sim.torso_bounce * 3.4;
    let sleep_drop = sim.sleep_t * 14.0;
    let squash_y = sim.head_squash * 4.2;

    let torso_c = project_25d(
        Vec3::new(
            RigAnchors::TORSO_CENTER.x,
            RigAnchors::TORSO_CENTER.y + bounce + sleep_drop * 0.5,
            RigAnchors::TORSO_CENTER.z,
        ),
        wf,
        hf,
    );
    let head_c = project_25d(
        Vec3::new(
            RigAnchors::HEAD_BASE_CENTER.x + sim.head_offset.x,
            RigAnchors::HEAD_BASE_CENTER.y + sim.head_offset.y + bounce + sleep_drop + squash_y,
            RigAnchors::HEAD_BASE_CENTER.z,
        ),
        wf,
        hf,
    );
    let head_xf = HeadRigTransform::new(head_c, sim.head_tilt_rad, sim.head_squash);
    let head_mat = head_xf.to_transform();

    // Update dynamic bounding box for torso + head + ears + whiskers
    bbox.include_rect(
        torso_c.x - 68.0 * s,
        torso_c.y - 46.0 * s,
        136.0 * s,
        104.0 * s,
    );
    bbox.include_rect(
        head_c.x - 68.0 * s,
        head_c.y - 78.0 * s,
        136.0 * s,
        128.0 * s,
    );

    // Torso path in canvas space; head + inner ears in head-local space transformed by `head_mat`
    let torso_path = build_torso_path(torso_c, s, breath, sim.sleep_t);
    let (head_local_path, ear_l_inner, ear_r_inner) = build_head_and_ears_local(
        s,
        sim.ear_relax_l,
        sim.ear_relax_r,
        sim.ear_twitch_l,
        sim.ear_twitch_r,
    );

    // Re-staged tucked back paws shifted slightly wider at hips (`x = +-38, y = 40`)
    // so front paws rest flat on the desk behind Spacebar with zero overlap!
    let foot_l = project_25d(
        Vec3::new(
            RigAnchors::BACK_PAW_LEFT.x,
            RigAnchors::BACK_PAW_LEFT.y + sleep_drop * 0.22,
            RigAnchors::BACK_PAW_LEFT.z,
        ),
        wf,
        hf,
    );
    let foot_r = project_25d(
        Vec3::new(
            RigAnchors::BACK_PAW_RIGHT.x,
            RigAnchors::BACK_PAW_RIGHT.y + sleep_drop * 0.22,
            RigAnchors::BACK_PAW_RIGHT.z,
        ),
        wf,
        hf,
    );
    bbox.include_circle(foot_l, 18.0 * s);
    bbox.include_circle(foot_r, 18.0 * s);

    // 1. Crisp exterior contour (omitted on dark coats for LittleOne's clean matte silhouette)
    if !pal.is_dark_coat {
        let out_w = optical_stroke(1.8, s);
        if let Some(ref tp) = torso_path {
            stroke_path(pixmap, tp, pal.outline, out_w, aa);
        }
        if let Some(ref hp) = head_local_path {
            stroke_path_xf(pixmap, hp, pal.outline, out_w, head_mat, None, aa);
        }
    }

    // 2. Base coat fills
    if let Some(ref tp) = torso_path {
        fill_path(pixmap, tp, pal.base, aa);
    }
    if let Some(ref hp) = head_local_path {
        fill_path_xf(pixmap, hp, pal.base, head_mat, None, aa);
    }

    // 3. Prepare row-cropped mask covering torso + head for zero-seam cel shading & coat markings
    let mask_y0 = ((head_c.y - 78.0 * s).floor().max(0.0)) as u32;
    let mask_y1 = ((torso_c.y + 62.0 * s).ceil().min(hf)) as u32;
    let w = pixmap.width();
    let h = pixmap.height();
    let mask = mask_scratch.prepare_rows(w, h, mask_y0, mask_y1);
    if let Some(ref tp) = torso_path {
        mask.fill_path(tp, FillRule::Winding, aa, Transform::identity());
    }
    if let Some(ref hp) = head_local_path {
        mask.fill_path(hp, FillRule::Winding, aa, head_mat);
    }

    // 4. Coat markings clipped cleanly inside the unified mask (head markings follow `head_mat`!)
    if app.marking_pattern != MarkingPattern::Solid
        && app.marking_pattern != MarkingPattern::Unknown
    {
        draw_clipped_coat_markings(pixmap, mask, app, torso_c, head_mat, s, aa);
    }

    // 5. Soft minimal cel-shading volume bands clipped cleanly inside the body mask
    draw_cel_body_shading(pixmap, mask, pal, torso_c, head_c, head_mat, s, aa);

    // 6. Subtle tucked back paws
    draw_back_paws(pixmap, app, pal, foot_l, foot_r, s, aa);

    // 7. Inner ear triangles (very subtle on dark coats so black cat reads as clean matte silhouette)
    let inner_alpha = if pal.is_dark_coat { 0.30 } else { 0.92 };
    let inner_col = rgb_color(app.inner_ear_color, inner_alpha);
    if let Some(ref el) = ear_l_inner {
        fill_path_xf(pixmap, el, inner_col, head_mat, None, aa);
    }
    if let Some(ref er) = ear_r_inner {
        fill_path_xf(pixmap, er, inner_col, head_mat, None, aa);
    }

    // 8. Expressive face drawn 100% through `head_xf` (zero shear under tilt/squash!)
    draw_face(pixmap, app, pal, sim, head_xf, s, aa);

    // 8b. Minimalist tactile contact feedback at the exact scratched spot (works even when click-through locked!)
    if sim.scratch_intensity > 0.02 {
        if let Some(spot) = sim.scratch_spot {
            let sx = spot.x.clamp(-1.0, 1.0) * 34.0 * s;
            let sy = spot.y.clamp(-1.0, 1.0) * 28.0 * s;
            let pulse = 0.5 + 0.5 * sim.purr_phase.sin();
            let alpha = sim.scratch_intensity.clamp(0.0, 1.0);
            let glow_r = (8.5 + pulse * 3.0) * s;
            if let Some(glow) = ellipse_path(sx, sy, glow_r, glow_r * 0.85) {
                let mut g_col = pal.blush;
                g_col.set_alpha(0.26 * alpha);
                fill_path_xf(pixmap, &glow, g_col, head_mat, None, aa);
            }
            let ring_r = (6.0 + pulse * 7.5) * s;
            if let Some(ring) = ellipse_path(sx, sy, ring_r, ring_r * 0.85) {
                let ring_col = if pal.is_dark_coat {
                    Color::from_rgba(0.95, 0.96, 0.90, 0.36 * alpha * (1.0 - pulse * 0.45)).unwrap()
                } else {
                    Color::from_rgba(0.94, 0.52, 0.66, 0.40 * alpha * (1.0 - pulse * 0.45)).unwrap()
                };
                stroke_path_xf(
                    pixmap,
                    &ring,
                    ring_col,
                    optical_stroke(1.1, s),
                    head_mat,
                    None,
                    aa,
                );
            }
            bbox.include_circle(Vec2::new(head_c.x + sx, head_c.y + sy), ring_r + 6.0 * s);
        }
    }

    // 9. Stable neutral head hitbox (Bug #6 fix: anchored to neutral head center so vertical
    //    `head_squash` during rubbing never pushes the head out from under the pointer!)
    let neutral_head_c = project_25d(RigAnchors::HEAD_BASE_CENTER, wf, hf);
    let hr_w = 100.0 * s;
    let hr_h = 90.0 * s;
    HitRect {
        x: neutral_head_c.x - hr_w * 0.5,
        y: neutral_head_c.y - hr_h * 0.58,
        w: hr_w,
        h: hr_h,
    }
}

/// Pass 6: Filled tapered Bezier organic forearms & sculpted paws (replacing expensive thick
/// `stroke_path` curves with `fill_path` ribbons + open side-only contour strokes).
#[allow(clippy::too_many_arguments)]
pub fn draw_arms_and_paws(
    pixmap: &mut Pixmap,
    app: &CatAppearance,
    pal: &CelPalette,
    sim: &SimSnapshot,
    wf: f32,
    hf: f32,
    s: f32,
    aa: bool,
    bbox: &mut BBoxAcc,
) {
    let bounce = -sim.torso_bounce * 3.1;
    let sleep_drop = sim.sleep_t * 10.5;

    let left_shoulder = project_25d(
        Vec3::new(
            RigAnchors::LEFT_SHOULDER.x,
            RigAnchors::LEFT_SHOULDER.y + 5.0 + bounce + sleep_drop,
            RigAnchors::LEFT_SHOULDER.z,
        ),
        wf,
        hf,
    );
    let right_shoulder = project_25d(
        Vec3::new(
            RigAnchors::RIGHT_SHOULDER.x,
            RigAnchors::RIGHT_SHOULDER.y + 5.0 + bounce + sleep_drop,
            RigAnchors::RIGHT_SHOULDER.z,
        ),
        wf,
        hf,
    );

    let left_paw = project_25d(sim.left_paw_world, wf, hf);
    let right_paw = project_25d(sim.right_paw_world, wf, hf);

    let arm_specs = [
        (
            right_shoulder,
            right_paw,
            -1.0f32,
            sim.right_paw_strike,
            sim.right_paw_on_mouse * sim.mouse_paw_roll,
        ),
        (left_shoulder, left_paw, 1.0f32, sim.left_paw_strike, 0.0f32),
    ];

    for (shoulder, paw, side, strike, paw_roll) in arm_specs {
        // Grounded desk contact shadow directly under front paws during idle floor-rest!
        if sim.floor_rest_t > 0.12 {
            if let Some(ao) = ellipse_path(
                paw.x,
                paw.y + 4.4 * s,
                11.0 * s * (0.65 + 0.35 * sim.floor_rest_t),
                4.2 * s,
            ) {
                fill_path(
                    pixmap,
                    &ao,
                    Color::from_rgba(0.03, 0.04, 0.08, 0.28 * sim.floor_rest_t).unwrap(),
                    aa,
                );
            }
        }

        // Smooth continuous forearm curve (zero binary elbow-drop or side-bow snapping!)
        let reach_dx = (paw.x - shoulder.x).abs() / s.max(0.1);
        let reach_u = ((reach_dx - 16.0) / 36.0).clamp(0.0, 1.0);
        let reach_smooth = reach_u * reach_u * (3.0 - 2.0 * reach_u);
        let elbow_drop = (-1.0 + reach_smooth * 7.0) * s;
        let idle_blend = (sim.floor_rest_t * (1.0 - reach_smooth)).clamp(0.0, 1.0);
        let side_bow = side * (6.5 - idle_blend * 3.5) * s;
        let mid = Vec2::new(
            (shoulder.x + paw.x) * 0.5 + side_bow,
            (shoulder.y + paw.y) * 0.5 + elbow_drop,
        );

        const N: usize = 10;
        let mut spine = [Vec2::default(); N];
        let mut half_w = [0.0f32; N];
        for i in 0..N {
            let t = i as f32 / (N - 1) as f32;
            spine[i] = eval_quad(shoulder, mid, paw, t);
            // Slender tubular LittleOne limbs tapering softly toward rounded paw tip
            half_w[i] = (7.1 - t * 1.4) * s;
            bbox.include_circle(spine[i], half_w[i] + 6.0 * s);
        }

        // Subtle interior contour lines separating limbs cleanly on dark coats
        let (l_start, r_start) = if side < 0.0 { (3, 1) } else { (1, 3) };
        let rib = build_tapered_ribbon(&spine, &half_w, l_start, r_start, side > 0.0);
        if let Some(ref body) = rib.body {
            fill_path(pixmap, body, pal.base, aa);
        }
        if let Some(ref sh) = rib.shadow_strip {
            fill_path(pixmap, sh, pal.cel_shadow_overlay, aa);
        }

        let edge_w = optical_stroke(1.0, s);
        let edge_col = if pal.is_dark_coat {
            pal.rim_sep
        } else {
            pal.outline
        };
        if let Some(ref le) = rib.left_edge {
            stroke_path(pixmap, le, edge_col, edge_w, aa);
        }
        if let Some(ref re) = rib.right_edge {
            stroke_path(pixmap, re, edge_col, edge_w, aa);
        }

        // Smooth rounded stubby paw tip (no chunky toe-line claws!)
        let paw_sx = 1.0 + strike * 0.08 + sim.floor_rest_t * 0.05;
        let paw_sy = 1.0 - strike * 0.06 - sim.floor_rest_t * 0.05;
        let paw_rx = 6.6 * s * paw_sx;
        let paw_ry = 5.4 * s * paw_sy;
        let paw_col = match app.marking_pattern {
            MarkingPattern::Tuxedo
            | MarkingPattern::Bicolor
            | MarkingPattern::Mittens
            | MarkingPattern::Siamese => rgb_color(app.marking_color, 1.0),
            _ => pal.base,
        };
        let paw_xf = Transform::from_rotate(paw_roll.to_degrees()).post_translate(paw.x, paw.y);
        if let Some(paw_ball) = ellipse_path(0.0, 0.0, paw_rx, paw_ry) {
            fill_path_xf(pixmap, &paw_ball, paw_col, paw_xf, None, aa);
            // Stroke only the lower/front paw arc so the forearm flows seamlessly into the paw tip (no wrist seam ring)
            let mut arc = PathBuilder::new();
            arc.move_to(-paw_rx * 0.94, paw_ry * 0.05);
            arc.cubic_to(
                -paw_rx * 0.86,
                paw_ry * 1.12,
                paw_rx * 0.86,
                paw_ry * 1.12,
                paw_rx * 0.94,
                paw_ry * 0.05,
            );
            if let Some(ap) = arc.finish() {
                stroke_path_xf(
                    pixmap,
                    &ap,
                    edge_col,
                    optical_stroke(0.95, s),
                    paw_xf,
                    None,
                    aa,
                );
            }
        }
        bbox.include_circle(paw, paw_rx + 5.0 * s);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_cel_body_shading(
    pixmap: &mut Pixmap,
    mask: &Mask,
    pal: &CelPalette,
    torso_c: Vec2,
    head_c: Vec2,
    head_mat: Transform,
    s: f32,
    aa: bool,
) {
    // 1. Tight translucent cool cel shadow hugging lower-right torso flank (preserves markings!)
    let mut tsh = PathBuilder::new();
    tsh.move_to(torso_c.x + 28.0 * s, torso_c.y - 34.0 * s);
    tsh.cubic_to(
        torso_c.x + 44.0 * s,
        torso_c.y + 8.0 * s,
        torso_c.x + 34.0 * s,
        torso_c.y + 38.0 * s,
        torso_c.x - 36.0 * s,
        torso_c.y + 42.0 * s,
    );
    tsh.line_to(torso_c.x + 76.0 * s, torso_c.y + 58.0 * s);
    tsh.line_to(torso_c.x + 76.0 * s, torso_c.y - 44.0 * s);
    tsh.close();
    if let Some(p) = tsh.finish() {
        fill_path_xf(
            pixmap,
            &p,
            pal.cel_shadow_overlay,
            Transform::identity(),
            Some(mask),
            aa,
        );
    }

    // 2. Soft curved highlight hugging the upper-left flank silhouette (light coats only)
    if !pal.is_dark_coat {
        let mut lhi = PathBuilder::new();
        lhi.move_to(torso_c.x - 24.0 * s, torso_c.y - 22.0 * s);
        lhi.quad_to(
            torso_c.x - 34.0 * s,
            torso_c.y + 2.0 * s,
            torso_c.x - 30.0 * s,
            torso_c.y + 22.0 * s,
        );
        if let Some(hp) = lhi.finish() {
            stroke_path_xf(
                pixmap,
                &hp,
                pal.cel_highlight_overlay,
                optical_stroke(3.6, s),
                Transform::identity(),
                Some(mask),
                aa,
            );
        }
    }

    // 3. Soft tight contact shadow tucked directly beneath the chin curve (light coats only)
    if !pal.is_dark_coat {
        if let Some(chin_ao) =
            ellipse_path(head_c.x + 1.0 * s, head_c.y + 26.5 * s, 21.0 * s, 4.2 * s)
        {
            fill_path_xf(
                pixmap,
                &chin_ao,
                pal.contact_ao,
                Transform::identity(),
                Some(mask),
                aa,
            );
        }
    }

    // 4. Tight rim cel shadow hugging outer right cheek & lower chin
    let mut hsh = PathBuilder::new();
    hsh.move_to(35.0 * s, -10.0 * s);
    hsh.cubic_to(36.0 * s, 12.0 * s, 22.0 * s, 27.0 * s, -18.0 * s, 29.0 * s);
    hsh.line_to(50.0 * s, 42.0 * s);
    hsh.line_to(50.0 * s, -16.0 * s);
    hsh.close();
    if let Some(p) = hsh.finish() {
        fill_path_xf(pixmap, &p, pal.cel_shadow_overlay, head_mat, Some(mask), aa);
    }

    // 5. Delicate curved crown highlight (light coats only; dark coats stay pure matte velvet)
    if !pal.is_dark_coat {
        let mut crown = PathBuilder::new();
        crown.move_to(-18.0 * s, -26.0 * s);
        crown.quad_to(-5.0 * s, -30.0 * s, 8.0 * s, -28.0 * s);
        if let Some(cp) = crown.finish() {
            stroke_path_xf(
                pixmap,
                &cp,
                pal.cel_highlight_overlay,
                optical_stroke(3.2, s),
                head_mat,
                Some(mask),
                aa,
            );
        }
    }
}

fn draw_back_paws(
    pixmap: &mut Pixmap,
    app: &CatAppearance,
    pal: &CelPalette,
    foot_l: Vec2,
    foot_r: Vec2,
    s: f32,
    aa: bool,
) {
    let foot_col = match app.marking_pattern {
        MarkingPattern::Tuxedo
        | MarkingPattern::Bicolor
        | MarkingPattern::Mittens
        | MarkingPattern::Siamese => rgb_color(app.marking_color, 1.0),
        _ => pal.base,
    };
    let edge_col = if pal.is_dark_coat {
        pal.rim_sep
    } else {
        pal.outline
    };

    for &fp in &[foot_l, foot_r] {
        if let Some(p) = ellipse_path(fp.x, fp.y, 7.6 * s, 5.4 * s) {
            fill_path(pixmap, &p, foot_col, aa);
            let mut soft_edge = edge_col;
            if pal.is_dark_coat {
                soft_edge.set_alpha(soft_edge.alpha() * 0.45);
            }
            stroke_path(pixmap, &p, soft_edge, optical_stroke(0.85, s), aa);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_clipped_coat_markings(
    pixmap: &mut Pixmap,
    mask: &Mask,
    app: &CatAppearance,
    torso_c: Vec2,
    head_mat: Transform,
    s: f32,
    aa: bool,
) {
    let mut p1 = Paint::default();
    p1.set_color(rgb_color(app.marking_color, 1.0));
    p1.anti_alias = aa;

    let mut p2 = Paint::default();
    p2.set_color(rgb_color(app.secondary_marking_color, 1.0));
    p2.anti_alias = aa;

    match app.marking_pattern {
        MarkingPattern::TabbyStripes => {
            let stroke = Stroke {
                width: 4.0 * s,
                line_cap: LineCap::Round,
                ..Stroke::default()
            };
            // Head stripes in head-local space (zero shear under tilt/squash!)
            let mut hpb = PathBuilder::new();
            for offset in [-10.0f32, 0.0, 10.0] {
                hpb.move_to(offset * s, -36.0 * s);
                hpb.line_to(offset * 0.72 * s, -18.0 * s);
            }
            for side in [-1.0f32, 1.0] {
                hpb.move_to(side * 45.0 * s, 4.0 * s);
                hpb.line_to(side * 27.0 * s, 7.0 * s);
                hpb.move_to(side * 43.0 * s, 13.0 * s);
                hpb.line_to(side * 28.0 * s, 15.0 * s);
            }
            if let Some(hp) = hpb.finish() {
                pixmap.stroke_path(&hp, &p1, &stroke, head_mat, Some(mask));
            }
            // Torso side stripes in torso space
            let mut tpb = PathBuilder::new();
            for dy in [-6.0f32, 8.0, 22.0] {
                tpb.move_to(torso_c.x + 54.0 * s, torso_c.y + dy * s);
                tpb.line_to(torso_c.x + 26.0 * s, torso_c.y + (dy + 3.0) * s);
            }
            if let Some(tp) = tpb.finish() {
                pixmap.stroke_path(&tp, &p1, &stroke, Transform::identity(), Some(mask));
            }
        }
        MarkingPattern::Tuxedo => {
            if let Some(bib) = ellipse_path(torso_c.x - 4.0 * s, torso_c.y + 12.0 * s, 26.0 * s, 36.0 * s) {
                pixmap.fill_path(&bib, &p1, FillRule::Winding, Transform::identity(), Some(mask));
            }
            if let Some(chin) = ellipse_path(0.0, 22.0 * s, 21.0 * s, 14.0 * s) {
                pixmap.fill_path(&chin, &p1, FillRule::Winding, head_mat, Some(mask));
            }
        }
        MarkingPattern::Bicolor => {
            let mut pb = PathBuilder::new();
            pb.move_to(0.0, -6.0 * s);
            pb.line_to(48.0 * s, 38.0 * s);
            pb.line_to(-48.0 * s, 38.0 * s);
            pb.close();
            if let Some(v) = pb.finish() {
                pixmap.fill_path(&v, &p1, FillRule::Winding, head_mat, Some(mask));
            }
            if let Some(belly) = ellipse_path(torso_c.x - 2.0 * s, torso_c.y + 14.0 * s, 35.0 * s, 40.0 * s) {
                pixmap.fill_path(&belly, &p1, FillRule::Winding, Transform::identity(), Some(mask));
            }
        }
        MarkingPattern::Siamese => {
            if let Some(face_mask) = ellipse_path(0.0, 4.0 * s, 31.0 * s, 22.5 * s) {
                pixmap.fill_path(&face_mask, &p1, FillRule::Winding, head_mat, Some(mask));
            }
            if let Some(ear_cap_l) = ellipse_path(-28.0 * s, -42.0 * s, 19.0 * s, 23.0 * s) {
                pixmap.fill_path(&ear_cap_l, &p1, FillRule::Winding, head_mat, Some(mask));
            }
            if let Some(ear_cap_r) = ellipse_path(28.0 * s, -42.0 * s, 19.0 * s, 23.0 * s) {
                pixmap.fill_path(&ear_cap_r, &p1, FillRule::Winding, head_mat, Some(mask));
            }
        }
        MarkingPattern::Calico => {
            for (cx, cy, rx, ry, second) in [
                (-26.0 * s, -24.0 * s, 26.0 * s, 24.0 * s, false),
                (28.0 * s, -26.0 * s, 22.0 * s, 20.0 * s, true),
            ] {
                if let Some(patch) = ellipse_path(cx, cy, rx, ry) {
                    pixmap.fill_path(
                        &patch,
                        if second { &p2 } else { &p1 },
                        FillRule::Winding,
                        head_mat,
                        Some(mask),
                    );
                }
            }
            for (cx, cy, rx, ry, second) in [
                (torso_c.x + 26.0 * s, torso_c.y + 10.0 * s, 26.0 * s, 24.0 * s, false),
                (torso_c.x - 28.0 * s, torso_c.y + 16.0 * s, 20.0 * s, 22.0 * s, true),
            ] {
                if let Some(patch) = ellipse_path(cx, cy, rx, ry) {
                    pixmap.fill_path(
                        &patch,
                        if second { &p2 } else { &p1 },
                        FillRule::Winding,
                        Transform::identity(),
                        Some(mask),
                    );
                }
            }
        }
        MarkingPattern::Tortie => {
            for (cx, cy, rx, ry, second) in [
                (-22.0 * s, -16.0 * s, 18.0 * s, 15.0 * s, false),
                (18.0 * s, 12.0 * s, 16.0 * s, 12.0 * s, true),
                (26.0 * s, -22.0 * s, 14.0 * s, 14.0 * s, false),
            ] {
                if let Some(patch) = ellipse_path(cx, cy, rx, ry) {
                    pixmap.fill_path(
                        &patch,
                        if second { &p2 } else { &p1 },
                        FillRule::Winding,
                        head_mat,
                        Some(mask),
                    );
                }
            }
            for (cx, cy, rx, ry, second) in [
                (torso_c.x - 16.0 * s, torso_c.y + 4.0 * s, 18.0 * s, 14.0 * s, true),
                (torso_c.x + 22.0 * s, torso_c.y + 18.0 * s, 20.0 * s, 16.0 * s, false),
            ] {
                if let Some(patch) = ellipse_path(cx, cy, rx, ry) {
                    pixmap.fill_path(
                        &patch,
                        if second { &p2 } else { &p1 },
                        FillRule::Winding,
                        Transform::identity(),
                        Some(mask),
                    );
                }
            }
        }
        MarkingPattern::Spotted => {
            for (cx, cy, rx, ry) in [
                (-14.0 * s, -24.0 * s, 5.5 * s, 4.0 * s),
                (2.0 * s, -27.0 * s, 6.0 * s, 4.2 * s),
                (16.0 * s, -23.0 * s, 5.5 * s, 4.0 * s),
            ] {
                if let Some(spot) = ellipse_path(cx, cy, rx, ry) {
                    pixmap.fill_path(&spot, &p1, FillRule::Winding, head_mat, Some(mask));
                }
            }
            for (cx, cy, rx, ry) in [
                (torso_c.x + 24.0 * s, torso_c.y - 4.0 * s, 7.5 * s, 5.5 * s),
                (torso_c.x + 16.0 * s, torso_c.y + 14.0 * s, 8.0 * s, 6.0 * s),
                (torso_c.x + 32.0 * s, torso_c.y + 24.0 * s, 7.0 * s, 5.5 * s),
                (torso_c.x - 24.0 * s, torso_c.y + 12.0 * s, 7.0 * s, 5.0 * s),
            ] {
                if let Some(spot) = ellipse_path(cx, cy, rx, ry) {
                    pixmap.fill_path(&spot, &p1, FillRule::Winding, Transform::identity(), Some(mask));
                }
            }
        }
        MarkingPattern::Mittens => {
            if let Some(chin) = ellipse_path(0.0, 22.0 * s, 15.0 * s, 9.5 * s) {
                pixmap.fill_path(&chin, &p1, FillRule::Winding, head_mat, Some(mask));
            }
        }
        MarkingPattern::Solid | MarkingPattern::Unknown => {}
    }
}

fn build_torso_path(c: Vec2, s: f32, breath: f32, sleep_t: f32) -> Option<Path> {
    // Sleek upright pear-shaped seated silhouette matching LittleOne (~60% keyboard width)
    let w_top = (28.5 + breath * 0.40) * s;
    let w_bot = (43.5 + sleep_t * 5.5 + breath * 0.70) * s;
    let h_top = (-35.0 + sleep_t * 7.5) * s;
    let h_bot = 46.0 * s;

    let mut pb = PathBuilder::new();
    pb.move_to(c.x - w_top, c.y + h_top);
    pb.cubic_to(
        c.x - w_bot * 0.90,
        c.y - 2.0 * s,
        c.x - w_bot,
        c.y + h_bot * 0.86,
        c.x - w_bot * 0.50,
        c.y + h_bot,
    );
    pb.quad_to(c.x, c.y + h_bot + 2.0 * s, c.x + w_bot * 0.54, c.y + h_bot);
    pb.cubic_to(
        c.x + w_bot * 1.04,
        c.y + h_bot * 0.84,
        c.x + w_bot * 0.92,
        c.y - 4.0 * s,
        c.x + w_top * 0.94,
        c.y + h_top,
    );
    pb.close();
    pb.finish()
}

/// Build compact rounded head + cute medium-small softly rounded ears in head-local space.
fn build_head_and_ears_local(
    s: f32,
    ear_relax_l: f32,
    ear_relax_r: f32,
    ear_twitch_l: f32,
    ear_twitch_r: f32,
) -> (Option<Path>, Option<Path>, Option<Path>) {
    let rx = 39.5 * s;
    let ry = 31.5 * s;

    let l_relax = (ear_relax_l + ear_twitch_l * 0.45).clamp(-0.35, 1.25);
    let r_relax = (ear_relax_r + ear_twitch_r * 0.45).clamp(-0.35, 1.25);
    let lx_off = l_relax * 7.0 * s;
    let ly_off = l_relax * 6.0 * s;
    let rx_off = r_relax * 7.0 * s;
    let ry_off = r_relax * 6.0 * s;

    let l_base_out = Vec2::new(-rx * 0.82, -ry * 0.30);
    let l_tip = Vec2::new(-rx * 0.72 - lx_off, -ry * 1.44 + ly_off);
    let l_base_in = Vec2::new(-rx * 0.18, -ry * 0.92);

    let r_base_in = Vec2::new(rx * 0.18, -ry * 0.92);
    let r_tip = Vec2::new(rx * 0.72 + rx_off, -ry * 1.44 + ry_off);
    let r_base_out = Vec2::new(rx * 0.82, -ry * 0.30);

    let cheek_r = Vec2::new(rx * 1.06, ry * 0.18);
    let chin = Vec2::new(0.0, ry * 1.02);
    let cheek_l = Vec2::new(-rx * 1.06, ry * 0.18);

    let mut pb = PathBuilder::new();
    pb.move_to(l_base_out.x, l_base_out.y);
    pb.quad_to(
        -rx * 0.88 - lx_off * 0.48,
        -ry * 1.06 + ly_off * 0.48,
        l_tip.x,
        l_tip.y,
    );
    pb.quad_to(
        -rx * 0.42 - lx_off * 0.15,
        -ry * 1.32 + ly_off * 0.40,
        l_base_in.x,
        l_base_in.y,
    );
    pb.quad_to(0.0, -ry * 1.02, r_base_in.x, r_base_in.y);
    pb.quad_to(
        rx * 0.42 + rx_off * 0.15,
        -ry * 1.32 + ry_off * 0.40,
        r_tip.x,
        r_tip.y,
    );
    pb.quad_to(
        rx * 0.88 + rx_off * 0.48,
        -ry * 1.06 + ry_off * 0.48,
        r_base_out.x,
        r_base_out.y,
    );
    pb.cubic_to(
        cheek_r.x,
        cheek_r.y,
        rx * 0.74,
        ry * 1.04,
        chin.x,
        chin.y,
    );
    pb.cubic_to(
        -rx * 0.74,
        ry * 1.04,
        cheek_l.x,
        cheek_l.y,
        l_base_out.x,
        l_base_out.y,
    );
    pb.close();

    // Rounded soft inner-ear triangles
    let inner_ear = |side: f32, x_off: f32, y_off: f32| -> Option<Path> {
        let a = Vec2::new(side * rx * 0.64 + side * x_off * 0.70, -ry * 1.24 + y_off * 0.72);
        let b = Vec2::new(side * rx * 0.68, -ry * 0.64);
        let d = Vec2::new(side * rx * 0.30, -ry * 0.88);
        let mut eb = PathBuilder::new();
        eb.move_to(a.x, a.y);
        eb.quad_to(side * rx * 0.72 + side * x_off * 0.25, -ry * 0.94 + y_off * 0.3, b.x, b.y);
        eb.quad_to(side * rx * 0.48, -ry * 0.74, d.x, d.y);
        eb.quad_to(side * rx * 0.44 + side * x_off * 0.3, -ry * 1.08 + y_off * 0.4, a.x, a.y);
        eb.close();
        eb.finish()
    };

    (
        pb.finish(),
        inner_ear(-1.0, lx_off, ly_off),
        inner_ear(1.0, rx_off, ry_off),
    )
}
