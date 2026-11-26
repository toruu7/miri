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
        sim.ear_relax,
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

    // 1. Exterior ink silhouette under-stroke for crisp unified illustrated contour
    let out_w = optical_stroke(3.0, s);
    if let Some(ref tp) = torso_path {
        stroke_path(pixmap, tp, pal.outline, out_w, aa);
    }
    if let Some(ref hp) = head_local_path {
        stroke_path_xf(pixmap, hp, pal.outline, out_w, head_mat, None, aa);
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

    // 5. Crisp cel-shading volume bands (lower-right torso crescent, soft chin cast shadow,
    //    lower cheek shadow, and gentle crown rim highlight) clipped cleanly inside the body mask!
    draw_cel_body_shading(pixmap, mask, pal, torso_c, head_c, head_mat, s, aa);

    // 6. Tucked back paws with cel shading and delicate toe-notch marks
    draw_back_paws(pixmap, app, pal, foot_l, foot_r, s, aa);

    // 7. Inner ear triangles (follow `head_mat` cleanly!)
    let inner_col = rgb_color(app.inner_ear_color, 1.0);
    let inner_sh = rgb_color(app.inner_ear_color.scale(0.80), 0.72);
    if let Some(ref el) = ear_l_inner {
        fill_path_xf(pixmap, el, inner_col, head_mat, None, aa);
        stroke_path_xf(pixmap, el, inner_sh, optical_stroke(0.95, s), head_mat, None, aa);
    }
    if let Some(ref er) = ear_r_inner {
        fill_path_xf(pixmap, er, inner_col, head_mat, None, aa);
        stroke_path_xf(pixmap, er, inner_sh, optical_stroke(0.95, s), head_mat, None, aa);
    }

    // 8. Expressive face drawn 100% through `head_xf` (zero shear under tilt/squash!)
    draw_face(pixmap, app, pal, sim, head_xf, s, aa);

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

        // Cozy organic elbow curve (gentle outward+downward plush bend when reaching to mouse!)
        let reach_dx = (paw.x - shoulder.x).abs() / s.max(0.1);
        let elbow_drop = if reach_dx > 40.0 { 7.5 * s } else { -1.5 * s };
        let mid = Vec2::new(
            (shoulder.x + paw.x) * 0.5 + side * 8.5 * s,
            (shoulder.y + paw.y) * 0.5 + elbow_drop,
        );

        const N: usize = 10;
        let mut spine = [Vec2::default(); N];
        let mut half_w = [0.0f32; N];
        for i in 0..N {
            let t = i as f32 / (N - 1) as f32;
            spine[i] = eval_quad(shoulder, mid, paw, t);
            // Smooth plush organic forearm taper: sturdy shoulder -> soft wrist (never thin stick!)
            half_w[i] = (9.0 - t * 1.9) * s;
            bbox.include_circle(spine[i], half_w[i] + 6.0 * s);
        }

        // Outer silhouette edge starts near shoulder (idx 1), inner chest-fold edge starts lower (idx 3)
        let (l_start, r_start) = if side < 0.0 { (3, 1) } else { (1, 3) };
        let rib = build_tapered_ribbon(&spine, &half_w, l_start, r_start, side > 0.0);
        if let Some(ref body) = rib.body {
            fill_path(pixmap, body, pal.base, aa);
        }
        if let Some(ref sh) = rib.shadow_strip {
            fill_path(pixmap, sh, pal.cel_shadow_overlay, aa);
        }

        // Open side-only exterior contour + dark-coat silver separation rim (NO round shoulder cap!)
        let edge_w = optical_stroke(1.45, s);
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

        // Sculpted front paw pad (subtle squash-and-stretch on strike/floor rest + mouse roll!)
        let paw_sx = 1.0 + strike * 0.10 + sim.floor_rest_t * 0.08;
        let paw_sy = 1.0 - strike * 0.08 - sim.floor_rest_t * 0.06;
        let paw_rx = 9.4 * s * paw_sx;
        let paw_ry = 7.4 * s * paw_sy;
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
            // Subtle lower paw underside shadow
            if let Some(p_sh) = ellipse_path(0.0, paw_ry * 0.32, paw_rx * 0.82, paw_ry * 0.45) {
                fill_path_xf(
                    pixmap,
                    &p_sh,
                    Color::from_rgba(0.04, 0.05, 0.10, 0.16).unwrap(),
                    paw_xf,
                    None,
                    aa,
                );
            }
            stroke_path_xf(
                pixmap,
                &paw_ball,
                edge_col,
                optical_stroke(1.2, s),
                paw_xf,
                None,
                aa,
            );
            // Two tiny curved toe-separation ticks on front paw tip
            let mut tpb = PathBuilder::new();
            for tx in [-2.6f32, 2.6] {
                tpb.move_to(tx * s, paw_ry * 0.25);
                tpb.line_to(tx * s, paw_ry * 0.88);
            }
            if let Some(tp) = tpb.finish() {
                stroke_path_xf(
                    pixmap,
                    &tp,
                    edge_col,
                    optical_stroke(0.9, s),
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

    // 2. Soft curved highlight hugging the upper-left flank silhouette
    let mut lhi = PathBuilder::new();
    lhi.move_to(torso_c.x - 30.0 * s, torso_c.y - 24.0 * s);
    lhi.quad_to(
        torso_c.x - 42.0 * s,
        torso_c.y + 2.0 * s,
        torso_c.x - 36.0 * s,
        torso_c.y + 24.0 * s,
    );
    if let Some(hp) = lhi.finish() {
        stroke_path_xf(
            pixmap,
            &hp,
            pal.cel_highlight_overlay,
            optical_stroke(4.5, s),
            Transform::identity(),
            Some(mask),
            aa,
        );
    }

    // 3. Soft tight contact shadow tucked directly beneath the chin curve
    if let Some(chin_ao) = ellipse_path(head_c.x + 1.0 * s, head_c.y + 32.5 * s, 26.0 * s, 5.5 * s) {
        fill_path_xf(
            pixmap,
            &chin_ao,
            pal.contact_ao,
            Transform::identity(),
            Some(mask),
            aa,
        );
    }

    // 4. Tight rim cel shadow hugging ONLY the outer right cheek & lower chin (never cuts across ears!)
    let mut hsh = PathBuilder::new();
    hsh.move_to(41.0 * s, -12.0 * s);
    hsh.cubic_to(42.0 * s, 14.0 * s, 26.0 * s, 32.0 * s, -22.0 * s, 34.0 * s);
    hsh.line_to(58.0 * s, 48.0 * s);
    hsh.line_to(58.0 * s, -18.0 * s);
    hsh.close();
    if let Some(p) = hsh.finish() {
        fill_path_xf(pixmap, &p, pal.cel_shadow_overlay, head_mat, Some(mask), aa);
    }

    // 5. Delicate curved crown highlight hugging upper-left head contour (never a solid floating blob!)
    let mut crown = PathBuilder::new();
    crown.move_to(-22.0 * s, -31.0 * s);
    crown.quad_to(-6.0 * s, -35.5 * s, 10.0 * s, -33.5 * s);
    if let Some(cp) = crown.finish() {
        stroke_path_xf(
            pixmap,
            &cp,
            pal.cel_highlight_overlay,
            optical_stroke(3.8, s),
            head_mat,
            Some(mask),
            aa,
        );
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
        // Subtle contact AO underneath back paw
        if let Some(ao) = ellipse_path(fp.x, fp.y + 4.5 * s, 13.5 * s, 5.2 * s) {
            fill_path(pixmap, &ao, Color::from_rgba(0.03, 0.04, 0.08, 0.22).unwrap(), aa);
        }
        if let Some(p) = ellipse_path(fp.x, fp.y, 13.2 * s, 9.6 * s) {
            fill_path(pixmap, &p, foot_col, aa);
            if let Some(sh) = ellipse_path(fp.x, fp.y + 3.0 * s, 10.6 * s, 4.6 * s) {
                fill_path(pixmap, &sh, Color::from_rgba(0.04, 0.05, 0.10, 0.16).unwrap(), aa);
            }
            stroke_path(pixmap, &p, edge_col, optical_stroke(1.25, s), aa);
            // Tiny subtle toe notch
            let mut tpb = PathBuilder::new();
            tpb.move_to(fp.x - 2.8 * s, fp.y + 3.2 * s);
            tpb.line_to(fp.x - 2.8 * s, fp.y + 8.6 * s);
            tpb.move_to(fp.x + 2.8 * s, fp.y + 3.2 * s);
            tpb.line_to(fp.x + 2.8 * s, fp.y + 8.6 * s);
            if let Some(tp) = tpb.finish() {
                stroke_path(pixmap, &tp, edge_col, optical_stroke(0.9, s), aa);
            }
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
    // Cozy rounded pear silhouette proportioned harmoniously beneath the head
    let w_top = (34.5 + breath * 0.45) * s;
    let w_bot = (54.0 + sleep_t * 6.5 + breath * 0.85) * s;
    let h_top = (-36.0 + sleep_t * 8.0) * s;
    let h_bot = 46.0 * s;

    let mut pb = PathBuilder::new();
    pb.move_to(c.x - w_top, c.y + h_top);
    pb.cubic_to(
        c.x - w_bot * 0.94,
        c.y - 2.0 * s,
        c.x - w_bot,
        c.y + h_bot * 0.86,
        c.x - w_bot * 0.50,
        c.y + h_bot,
    );
    pb.quad_to(c.x, c.y + h_bot + 2.5 * s, c.x + w_bot * 0.56, c.y + h_bot);
    pb.cubic_to(
        c.x + w_bot * 1.08,
        c.y + h_bot * 0.84,
        c.x + w_bot * 0.96,
        c.y - 4.0 * s,
        c.x + w_top * 0.94,
        c.y + h_top,
    );
    pb.close();
    pb.finish()
}

/// Build head + expressive ears in head-local coordinates centered at `(0, 0)`.
fn build_head_and_ears_local(
    s: f32,
    ear_relax: f32,
    ear_twitch_l: f32,
    ear_twitch_r: f32,
) -> (Option<Path>, Option<Path>, Option<Path>) {
    let rx = 47.5 * s;
    let ry = 37.5 * s;

    let l_relax = (ear_relax + ear_twitch_l * 0.45).clamp(-0.35, 1.15);
    let r_relax = (ear_relax + ear_twitch_r * 0.45).clamp(-0.35, 1.15);
    let lx_off = l_relax * 8.5 * s;
    let ly_off = l_relax * 7.5 * s;
    let rx_off = r_relax * 8.5 * s;
    let ry_off = r_relax * 7.5 * s;

    let l_base_out = Vec2::new(-rx * 0.82, -ry * 0.34);
    let l_tip = Vec2::new(-rx * 0.74 - lx_off, -ry * 1.56 + ly_off);
    let l_base_in = Vec2::new(-rx * 0.16, -ry * 0.94);

    let r_base_in = Vec2::new(rx * 0.16, -ry * 0.94);
    let r_tip = Vec2::new(rx * 0.74 + rx_off, -ry * 1.56 + ry_off);
    let r_base_out = Vec2::new(rx * 0.82, -ry * 0.34);

    let cheek_r = Vec2::new(rx * 1.05, ry * 0.18);
    let chin = Vec2::new(0.0, ry * 1.02);
    let cheek_l = Vec2::new(-rx * 1.05, ry * 0.18);

    let mut pb = PathBuilder::new();
    pb.move_to(l_base_out.x, l_base_out.y);
    pb.quad_to(
        -rx * 0.90 - lx_off * 0.5,
        -ry * 1.12 + ly_off * 0.5,
        l_tip.x,
        l_tip.y,
    );
    pb.quad_to(
        -rx * 0.42 - lx_off * 0.15,
        -ry * 1.38 + ly_off * 0.42,
        l_base_in.x,
        l_base_in.y,
    );
    pb.quad_to(0.0, -ry * 1.04, r_base_in.x, r_base_in.y);
    pb.quad_to(
        rx * 0.42 + rx_off * 0.15,
        -ry * 1.38 + ry_off * 0.42,
        r_tip.x,
        r_tip.y,
    );
    pb.quad_to(
        rx * 0.90 + rx_off * 0.5,
        -ry * 1.12 + ry_off * 0.5,
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
        let a = Vec2::new(side * rx * 0.66 + side * x_off * 0.72, -ry * 1.32 + y_off * 0.75);
        let b = Vec2::new(side * rx * 0.70, -ry * 0.66);
        let d = Vec2::new(side * rx * 0.28, -ry * 0.90);
        let mut eb = PathBuilder::new();
        eb.move_to(a.x, a.y);
        eb.quad_to(side * rx * 0.74 + side * x_off * 0.25, -ry * 0.98 + y_off * 0.3, b.x, b.y);
        eb.quad_to(side * rx * 0.48, -ry * 0.76, d.x, d.y);
        eb.quad_to(side * rx * 0.44 + side * x_off * 0.3, -ry * 1.14 + y_off * 0.4, a.x, a.y);
        eb.close();
        eb.finish()
    };

    (
        pb.finish(),
        inner_ear(-1.0, lx_off, ly_off),
        inner_ear(1.0, rx_off, ry_off),
    )
}
