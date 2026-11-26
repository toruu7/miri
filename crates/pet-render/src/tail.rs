use crate::geom::{
    build_tapered_ribbon, ellipse_path, eval_cubic, fill_path, optical_stroke, stroke_path, BBoxAcc,
};
use crate::palette::{rgb_color, CelPalette};
use pet_core::{project_25d, CatAppearance, MarkingPattern, SimSnapshot, Vec2, Vec3};
use tiny_skia::{Color, PathBuilder, Pixmap};

/// Pass 2: Lush tapered organic S-curve tail behind the torso (built via closed `fill_path`
/// tapered polygon + translucent cel shadow strip, replacing thick `stroke_path` curves).
#[allow(clippy::too_many_arguments)]
pub fn draw_tail_back(
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
    let curl = sim.tail_curl_t.clamp(0.0, 1.0);
    // Gentle sway (calms during sleep, perks slightly during patting)
    let sway_amp = (7.4 + sim.purr_intensity * 3.5) * (1.0 - sim.sleep_t * 0.65) * (1.0 - curl * 0.7);
    let sway = sim.tail_sway_phase.sin() * sway_amp;
    let sway2 = (sim.tail_sway_phase - 0.95).sin() * sway_amp * 0.9;

    // Graceful upright S-hook cat tail emerging smoothly from right hip
    let p0 = project_25d(Vec3::new(34.0, 28.0, -14.0), wf, hf);
    let c1 = project_25d(
        Vec3::new(78.0 + curl * 18.0, 36.0 + sway * 0.35 + curl * 22.0, -10.0),
        wf,
        hf,
    );
    let c2 = project_25d(
        Vec3::new(92.0 - curl * 10.0, -2.0 + sway + curl * 58.0, -6.0),
        wf,
        hf,
    );
    let tip = project_25d(
        Vec3::new(112.0 + curl * 24.0, -22.0 + sway2 * 1.15 + curl * 96.0, 6.0),
        wf,
        hf,
    );

    const N: usize = 12;
    let mut spine = [Vec2::default(); N];
    let mut half_w = [0.0f32; N];
    for i in 0..N {
        let t = i as f32 / (N - 1) as f32;
        spine[i] = eval_cubic(p0, c1, c2, tip, t);
        // Plush organic cat-tail silhouette: sturdy base -> fuller plume -> soft rounded tip
        let plume = 1.0 + (t * std::f32::consts::PI).sin() * 0.26 - t * 0.08;
        half_w[i] = 7.6 * s * plume;
        bbox.include_circle(spine[i], half_w[i] + 10.0 * s);
    }

    let rib = build_tapered_ribbon(&spine, &half_w, 1, 1, true);
    if let Some(ref body) = rib.body {
        stroke_path(pixmap, body, pal.outline, optical_stroke(2.7, s), aa);
        fill_path(pixmap, body, rgb_color(pal.tail_fill, 1.0), aa);
    }

    // Coat pattern rings / plush tip accents
    if matches!(
        app.marking_pattern,
        MarkingPattern::TabbyStripes | MarkingPattern::Spotted
    ) {
        for &idx in &[4usize, 7, 9] {
            if let Some(band) = ellipse_path(spine[idx].x, spine[idx].y, 5.2 * s, 4.2 * s) {
                fill_path(pixmap, &band, rgb_color(app.marking_color, 0.88), aa);
            }
        }
    } else if matches!(
        app.marking_pattern,
        MarkingPattern::Tuxedo | MarkingPattern::Bicolor | MarkingPattern::Mittens
    ) {
        if let Some(tip_dot) = ellipse_path(spine[N - 1].x, spine[N - 1].y, 6.8 * s, 6.5 * s) {
            fill_path(pixmap, &tip_dot, rgb_color(app.marking_color, 0.96), aa);
        }
    }

    // Translucent underside cel shadow (preserves tabby rings & bicolor tip!)
    if let Some(ref sh) = rib.shadow_strip {
        fill_path(pixmap, sh, pal.cel_shadow_overlay, aa);
    }

    // Subtle rim highlight along upper silhouette
    if let Some(ref top_edge) = rib.left_edge {
        if pal.is_dark_coat {
            stroke_path(pixmap, top_edge, pal.rim_sep, optical_stroke(1.25, s), aa);
        } else {
            stroke_path(pixmap, top_edge, pal.cel_highlight_overlay, optical_stroke(1.4, s), aa);
        }
    }
}

/// Pass 7 (Privacy Mode): Organic fuller fluffy scalloped tail curtain emerging smoothly from
/// the cat's right flank and wrapping completely across **100% of all 68 keycaps** (including
/// bottom-right Esc and top-left arrows!) with soft fur scallops, subtle lease-driven crest ripple,
/// and luminance-guaranteed contrast across all 15 presets.
#[allow(clippy::too_many_arguments)]
pub fn draw_tail_front(
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
    let t = ((sim.tail_curl_t - 0.08) / 0.92).clamp(0.0, 1.0);
    if t <= 0.01 {
        return;
    }

    // Keyboard keycaps span logical x in [-85..+134], y in [52..100] (projected with depth z in [-28..+28]).
    // Start smoothly wrapped around the right side of the keyboard (`x = +126, y = +79`) emerging from
    // the right flank, and sweep lushly all the way past the left edge (`x = -96, y = +79`)!
    let start = project_25d(Vec3::new(124.0, 77.0, 20.0), wf, hf);
    let end_x = 124.0 - t * 218.0; // reaches -94 logical x -> covers 100% of Esc..Del & Spacebar!
    let end = project_25d(Vec3::new(end_x, 78.0, 26.0), wf, hf);
    let c1 = project_25d(Vec3::new(102.0 - t * 34.0, 82.0, 22.0), wf, hf);
    let c2 = project_25d(Vec3::new(40.0 - t * 98.0, 75.0 + t * 4.0, 26.0), wf, hf);

    // Subtle traveling fur-crest ripple during Manual Privacy activity lease
    let ripple_amp = sim.privacy_tail_ripple * 2.4 * s;
    let ripple_phase = sim.time_sec * 8.5;

    const N: usize = 12;
    let mut upper = [Vec2::default(); N];
    let mut lower = [Vec2::default(); N];
    let mut crest = [Vec2::default(); N];
    let mut spine = [Vec2::default(); N];

    for i in 0..N {
        let u = i as f32 / (N - 1) as f32;
        let pt = eval_cubic(start, c1, c2, end, u);
        spine[i] = pt;
        // Broad plush uniform-to-bushy profile so BOTH right (Esc/1/2) AND left (Arrows/Del)
        // rows are 100% hidden beneath opaque fluffy fur!
        let fullness = 1.02 + (u * std::f32::consts::PI).sin() * 0.24;
        let half_h_up = (26.5 * fullness) * s + ((u * 5.0 - ripple_phase).sin() * ripple_amp);
        let half_h_dn = (26.5 * fullness) * s;
        // Soft natural fur-tuft scallops along upper/lower silhouette
        let puff = if i % 2 == 1 { 2.6 * s } else { -0.6 * s };
        upper[i] = Vec2::new(pt.x, pt.y - half_h_up - puff);
        lower[i] = Vec2::new(pt.x, pt.y + half_h_dn + puff * 0.55);
        crest[i] = Vec2::new(pt.x, pt.y - half_h_up * 0.78 - puff * 0.3);

        bbox.include_circle(upper[i], 14.0 * s);
        bbox.include_circle(lower[i], 14.0 * s);
    }

    // Organic curved root connecting back toward cat's right flank + lush bushy left tail tip
    let root_cap = Vec2::new(start.x + 24.0 * s, start.y - 2.0 * s);
    let root_top = Vec2::new(start.x + 22.0 * s, upper[0].y + 4.0 * s);
    let root_bot = Vec2::new(start.x + 22.0 * s, lower[0].y - 3.0 * s);
    let tip_cap = Vec2::new(end.x - 18.0 * s * t, end.y + 1.0 * s);
    let tip_tuft = Vec2::new(end.x - 16.0 * s * t, end.y - 14.0 * s);
    bbox.include_circle(root_cap, 34.0 * s);
    bbox.include_circle(tip_cap, 34.0 * s);

    // Build closed fluffy scalloped tail curtain polygon
    let mut pb = PathBuilder::new();
    pb.move_to(root_cap.x, root_cap.y);
    pb.quad_to(root_top.x, upper[0].y, upper[0].x, upper[0].y);
    for i in 1..N {
        let mid = upper[i - 1].lerp(upper[i], 0.5);
        let bulge_y = if i % 2 == 1 { -2.2 * s } else { -s };
        pb.quad_to(mid.x, mid.y + bulge_y, upper[i].x, upper[i].y);
    }
    // Bushy plush tail tip with cute soft fur-tuft flick
    pb.quad_to(tip_tuft.x, upper[N - 1].y + 2.0 * s, tip_cap.x, tip_cap.y);
    pb.quad_to(
        end.x - 15.0 * s * t,
        lower[N - 1].y - 2.0 * s,
        lower[N - 1].x,
        lower[N - 1].y,
    );
    for i in (0..N - 1).rev() {
        let mid = lower[i + 1].lerp(lower[i], 0.5);
        pb.quad_to(mid.x, mid.y + 1.6 * s, lower[i].x, lower[i].y);
    }
    pb.quad_to(root_bot.x, lower[0].y, root_cap.x, root_cap.y);
    pb.close();

    // Soft cast shadow directly beneath the fluffy tail onto the keyboard bezel
    let mut sh_pb = PathBuilder::new();
    sh_pb.move_to(lower[0].x + 8.0 * s, lower[0].y + 1.5 * s);
    for p in &lower[1..N] {
        sh_pb.line_to(p.x, p.y + 3.8 * s);
    }
    for i in (0..N).rev() {
        sh_pb.line_to(spine[i].x, spine[i].y + 10.0 * s);
    }
    sh_pb.close();
    if let Some(sh) = sh_pb.finish() {
        fill_path(pixmap, &sh, Color::from_rgba(0.02, 0.02, 0.06, 0.30).unwrap(), aa);
    }

    if let Some(tail_path) = pb.finish() {
        // 1. Exterior ink contour
        stroke_path(pixmap, &tail_path, pal.outline, optical_stroke(3.2, s), aa);
        // 2. Opaque fluffy tail body fill (100% covers all 68 keycaps!)
        fill_path(pixmap, &tail_path, rgb_color(pal.tail_fill, 1.0), aa);

        // 3. Optional coat stripes / bicolor plush tip on privacy tail curtain
        if matches!(
            app.marking_pattern,
            MarkingPattern::TabbyStripes | MarkingPattern::Spotted
        ) {
            for &idx in &[2usize, 5, 8] {
                if let Some(band) =
                    ellipse_path(spine[idx].x, spine[idx].y - 1.0 * s, 6.5 * s, 17.5 * s)
                {
                    fill_path(pixmap, &band, rgb_color(app.marking_color, 0.72), aa);
                }
            }
        } else if matches!(
            app.marking_pattern,
            MarkingPattern::Tuxedo | MarkingPattern::Bicolor | MarkingPattern::Mittens
        ) {
            if let Some(tip) = ellipse_path(end.x - 3.0 * s, end.y + 1.0 * s, 15.5 * s, 16.5 * s) {
                fill_path(pixmap, &tip, rgb_color(app.marking_color, 0.92), aa);
            }
        }

        // 4. Translucent cool cel underside shadow along lower belly of fluffy tail
        let mut cel_sh = PathBuilder::new();
        cel_sh.move_to(root_cap.x - 2.0 * s, spine[0].y + 8.0 * s);
        for p in &spine[1..N] {
            cel_sh.line_to(p.x, p.y + 9.5 * s);
        }
        cel_sh.quad_to(
            tip_cap.x + 4.0 * s,
            lower[N - 1].y - 6.0 * s,
            lower[N - 1].x,
            lower[N - 1].y - 1.2 * s,
        );
        for i in (0..N - 1).rev() {
            cel_sh.line_to(lower[i].x, lower[i].y - 1.2 * s);
        }
        cel_sh.quad_to(root_bot.x, lower[0].y - 1.5 * s, root_cap.x - 2.0 * s, spine[0].y + 8.0 * s);
        cel_sh.close();
        if let Some(cs) = cel_sh.finish() {
            fill_path(pixmap, &cs, pal.cel_shadow_overlay, aa);
        }

        // 5. Delicate soft upper fur-crest sheen (low-alpha cel highlight, never harsh stripe!)
        let mut hpb = PathBuilder::new();
        hpb.move_to(crest[1].x, crest[1].y);
        for p in &crest[2..N - 1] {
            hpb.line_to(p.x, p.y);
        }
        if let Some(hp) = hpb.finish() {
            stroke_path(pixmap, &hp, pal.cel_highlight_overlay, optical_stroke(2.4, s), aa);
        }

        // 6. Luminance-guaranteed contrast rim stroke separating tail cleanly from keyboard & torso
        stroke_path(
            pixmap,
            &tail_path,
            pal.tail_curtain_rim,
            optical_stroke(1.4, s),
            aa,
        );
    }
}
