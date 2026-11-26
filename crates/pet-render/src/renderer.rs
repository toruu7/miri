use crate::font::draw_text_centered;
use pet_core::{
    keycap_specs, keyboard_local_to_world, mouse_world_pos, project_25d, BackdropMode, CatProfile,
    FrameHitboxes, HitRect, MarkingPattern, Rgb8, SimSnapshot, Vec2, Vec3, BASE_CANVAS_H,
    BASE_CANVAS_W, MAX_CANVAS_EDGE_PX,
};
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, Pixmap, PixmapPaint,
    Stroke, Transform,
};

#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub backdrop: BackdropMode,
    pub chroma_hard_edge: bool,
    pub show_keycap_legends: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            backdrop: BackdropMode::Transparent,
            chroma_hard_edge: false,
            show_keycap_legends: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct StaticCacheKey {
    w: u32,
    h: u32,
    backdrop: BackdropMode,
    chroma_hard_edge: bool,
    show_legends: bool,
    kb_case: Rgb8,
    key_base: Rgb8,
    key_accent: Rgb8,
    key_legend: Rgb8,
}

/// Pure-Rust 2.5D vector renderer with cached static desk/keyboard layer, `tiny_skia::Mask`
/// coat markings, contrast-adaptive rim outlines, and deterministic 8-pass painter's order.
pub struct Renderer {
    pixmap: Pixmap,
    static_desk_cache: Option<Pixmap>,
    static_cache_key: Option<StaticCacheKey>,
    mask_scratch: Option<Mask>,
    last_hitboxes: FrameHitboxes,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        let (w, h) = clamp_dimensions(width, height);
        Self {
            pixmap: Pixmap::new(w, h).expect("valid pixmap dimensions"),
            static_desk_cache: None,
            static_cache_key: None,
            mask_scratch: None,
            last_hitboxes: FrameHitboxes::default(),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = clamp_dimensions(width, height);
        if self.pixmap.width() != w || self.pixmap.height() != h {
            self.pixmap = Pixmap::new(w, h).expect("valid pixmap dimensions");
            self.static_desk_cache = None;
            self.static_cache_key = None;
            self.mask_scratch = None;
        }
    }

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }

    pub fn rgba_bytes(&self) -> &[u8] {
        self.pixmap.data()
    }

    pub fn last_hitboxes(&self) -> FrameHitboxes {
        self.last_hitboxes
    }

    /// Render a single frame into the internal premultiplied RGBA8 pixmap and return frame hitboxes.
    pub fn render(
        &mut self,
        profile: &CatProfile,
        sim: &SimSnapshot,
        opts: RenderOptions,
    ) -> FrameHitboxes {
        let w = self.pixmap.width();
        let h = self.pixmap.height();
        let wf = w as f32;
        let hf = h as f32;
        let s = (wf / BASE_CANVAS_W).min(hf / BASE_CANVAS_H);
        let aa = !opts.chroma_hard_edge;

        // Pass 1: Backdrop
        match opts.backdrop {
            BackdropMode::Transparent => self.pixmap.fill(Color::TRANSPARENT),
            BackdropMode::Solid(rgb) => self.pixmap.fill(rgb_color(rgb, 1.0)),
        }

        let app = &profile.appearance;
        let per = &profile.peripherals;
        let base_lum = app.base_coat.luminance();
        let is_dark_coat = base_lum < 0.22;
        let outline_color = if is_dark_coat {
            rgb_color(Rgb8::new(10, 10, 13), 1.0)
        } else {
            rgb_color(app.base_coat.scale(0.42), 1.0)
        };
        let rim_color = if is_dark_coat {
            // Subtle crisp silver-white edge separating dark arms/paws from dark torso (screenshots 1-6)
            rgb_color(Rgb8::new(196, 202, 214), 0.56)
        } else {
            rgb_color(app.base_coat.scale(0.40), 0.85)
        };

        // Pass 2: Desk shadow + TailBackSegment (behind torso)
        self.draw_tail_back(app, sim, wf, hf, s, outline_color, rim_color, is_dark_coat, aa);

        // Pass 3 & 4 (cached): Desk shadow, Keyboard chassis, unpressed 68 keycaps
        self.ensure_static_desk_cache(w, h, per, opts);
        if let Some(cache) = &self.static_desk_cache {
            self.pixmap.draw_pixmap(
                0,
                0,
                cache.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }

        // Dynamic pressed/glowing keycaps overlay on top of static keyboard cache
        self.draw_active_keycaps(per, sim, wf, hf, s, opts.show_keycap_legends || per.show_keycap_legends, aa);

        // Miniature oval mouse (viewer left = cat's right paw)
        self.draw_mouse(per, sim, wf, hf, s, aa);

        // Pass 5: Unified Torso + Tucked Back Paws + Head + Ears + Coat Markings + Face
        let head_rect = self.draw_cat_body_and_head(
            app,
            sim,
            wf,
            hf,
            s,
            outline_color,
            rim_color,
            is_dark_coat,
            aa,
        );

        // Pass 6: Forearms & Paws (rendered ABOVE keycaps and mouse so downward strokes overlap cleanly)
        self.draw_arms_and_paws(app, sim, wf, hf, s, outline_color, rim_color, is_dark_coat, aa);

        // Pass 7: TailFrontSegment (ONLY active during privacy curl over the keyboard)
        if sim.tail_curl_t > 0.08 {
            self.draw_tail_front(app, sim, wf, hf, s, outline_color, rim_color, is_dark_coat, aa);
        }

        // Pass 8: Floating Showcase keycap pop-ups + Drowsy sleep `z` particles
        self.draw_popups(sim, wf, hf, s, aa);
        self.draw_sleep_particles(sim, wf, hf, s);

        let body_bounds_px = HitRect {
            x: (wf * 0.10).max(0.0),
            y: (hf * 0.14).max(0.0),
            w: (wf * 0.80).min(wf),
            h: (hf * 0.82).min(hf),
        };
        let hitboxes = FrameHitboxes {
            head_rect_px: head_rect,
            body_bounds_px,
            dirty_rect_px: HitRect {
                x: 0.0,
                y: 0.0,
                w: wf,
                h: hf,
            },
        };
        self.last_hitboxes = hitboxes;
        hitboxes
    }

    fn ensure_static_desk_cache(
        &mut self,
        w: u32,
        h: u32,
        per: &pet_core::PeripheralStyle,
        opts: RenderOptions,
    ) {
        let show_legends = opts.show_keycap_legends || per.show_keycap_legends;
        let key = StaticCacheKey {
            w,
            h,
            backdrop: opts.backdrop,
            chroma_hard_edge: opts.chroma_hard_edge,
            show_legends,
            kb_case: per.keyboard_case,
            key_base: per.keycap_base,
            key_accent: per.keycap_accent,
            key_legend: per.keycap_legend_color,
        };
        if self.static_cache_key.as_ref() == Some(&key) && self.static_desk_cache.is_some() {
            return;
        }

        let mut cache = Pixmap::new(w, h).expect("cache pixmap");
        let wf = w as f32;
        let hf = h as f32;
        let s = (wf / BASE_CANVAS_W).min(hf / BASE_CANVAS_H);
        let aa = !opts.chroma_hard_edge;

        // Subtle grounded desk shadow (kept tight <= 6 px, alpha cut cleanly during BGRA export)
        let shadow_center = project_25d(Vec3::new(4.0, 92.0, 8.0), wf, hf);
        if let Some(p) = ellipse_path(shadow_center.x, shadow_center.y, 134.0 * s, 26.0 * s) {
            fill_path(&mut cache, &p, Color::from_rgba(0.0, 0.0, 0.0, 0.18).unwrap(), aa);
        }

        // 65% Keyboard chassis trapezoid (perspective tilted facing cat)
        let tl = project_25d(keyboard_local_to_world(-1.07, -1.16, 0.0), wf, hf);
        let tr = project_25d(keyboard_local_to_world(1.07, -1.16, 0.0), wf, hf);
        let br = project_25d(keyboard_local_to_world(1.09, 1.18, 0.0), wf, hf);
        let bl = project_25d(keyboard_local_to_world(-1.09, 1.18, 0.0), wf, hf);

        // Front case thickness skirt
        let skirt = 5.5 * s;
        if let Some(p) = rounded_quad(
            Vec2::new(tl.x, tl.y + skirt),
            Vec2::new(tr.x, tr.y + skirt),
            Vec2::new(br.x, br.y + skirt),
            Vec2::new(bl.x, bl.y + skirt),
            5.0 * s,
        ) {
            fill_path(&mut cache, &p, rgb_color(per.keyboard_case.scale(0.65), 1.0), aa);
        }
        if let Some(p) = rounded_quad(tl, tr, br, bl, 5.0 * s) {
            fill_path(&mut cache, &p, rgb_color(per.keyboard_case, 1.0), aa);
        }

        // All 68 unpressed keycaps sorted back-row (row 4, nearest cat) to front-row (row 0)
        for row in (0..=4u8).rev() {
            for spec in keycap_specs().iter().filter(|k| k.row == row) {
                draw_single_keycap(
                    &mut cache,
                    spec,
                    per,
                    0.0,
                    0.0,
                    wf,
                    hf,
                    s,
                    show_legends,
                    aa,
                );
            }
        }

        self.static_desk_cache = Some(cache);
        self.static_cache_key = Some(key);
    }

    fn draw_active_keycaps(
        &mut self,
        per: &pet_core::PeripheralStyle,
        sim: &SimSnapshot,
        wf: f32,
        hf: f32,
        s: f32,
        show_legends: bool,
        aa: bool,
    ) {
        for row in (0..=4u8).rev() {
            for spec in keycap_specs().iter().filter(|k| k.row == row) {
                let idx = spec.key.index();
                let press = sim.key_press_amounts[idx];
                let glow = sim.key_glow_amounts[idx];
                if press > 0.01 || glow > 0.01 {
                    draw_single_keycap(
                        &mut self.pixmap,
                        spec,
                        per,
                        press,
                        glow,
                        wf,
                        hf,
                        s,
                        show_legends,
                        aa,
                    );
                }
            }
        }
    }

    fn draw_mouse(
        &mut self,
        per: &pet_core::PeripheralStyle,
        sim: &SimSnapshot,
        wf: f32,
        hf: f32,
        s: f32,
        aa: bool,
    ) {
        let pos = project_25d(mouse_world_pos(sim.mouse_offset.x, sim.mouse_offset.y), wf, hf);
        let rx = 17.5 * s;
        let ry = 21.5 * s;

        // Subtle shadow + lower body rim
        if let Some(p) = ellipse_path(pos.x, pos.y + 3.2 * s, rx * 1.03, ry * 0.98) {
            fill_path(&mut self.pixmap, &p, Color::from_rgba(0.0, 0.0, 0.0, 0.28).unwrap(), aa);
        }
        if let Some(p) = ellipse_path(pos.x, pos.y, rx, ry) {
            fill_path(&mut self.pixmap, &p, rgb_color(per.mouse_body, 1.0), aa);
            stroke_path(
                &mut self.pixmap,
                &p,
                rgb_color(per.mouse_body.scale(0.62), 0.9),
                1.2 * s,
                aa,
            );
        }

        // Click highlight tints on upper-left / upper-right mouse buttons (facing cat = top half)
        if sim.mouse_left_press > 0.05 {
            if let Some(p) = ellipse_path(pos.x + rx * 0.32, pos.y - ry * 0.25, rx * 0.42, ry * 0.42) {
                fill_path(
                    &mut self.pixmap,
                    &p,
                    rgb_color(per.keycap_highlight, sim.mouse_left_press * 0.72),
                    aa,
                );
            }
        }
        if sim.mouse_right_press > 0.05 {
            if let Some(p) = ellipse_path(pos.x - rx * 0.32, pos.y - ry * 0.25, rx * 0.42, ry * 0.42) {
                fill_path(
                    &mut self.pixmap,
                    &p,
                    rgb_color(per.keycap_highlight, sim.mouse_right_press * 0.72),
                    aa,
                );
            }
        }

        // Scroll wheel pill (pulses vertically on scroll)
        let wheel_y = pos.y - ry * 0.22 + sim.mouse_scroll_pulse * 2.2 * s;
        let mut pb = PathBuilder::new();
        pb.move_to(pos.x, wheel_y - 4.2 * s);
        pb.line_to(pos.x, wheel_y + 4.2 * s);
        if let Some(p) = pb.finish() {
            stroke_path(
                &mut self.pixmap,
                &p,
                rgb_color(per.mouse_wheel, 0.95),
                2.6 * s,
                aa,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_tail_back(
        &mut self,
        app: &pet_core::CatAppearance,
        sim: &SimSnapshot,
        wf: f32,
        hf: f32,
        s: f32,
        outline_color: Color,
        rim_color: Color,
        is_dark_coat: bool,
        aa: bool,
    ) {
        let sway = (sim.tail_sway_phase).sin() * 7.0 * (1.0 - sim.sleep_t * 0.6);
        let curl = sim.tail_curl_t;

        // S-curve tail sticking to viewer's right (matches screenshots 1-6), tucking forward as curl increases
        let p0 = project_25d(Vec3::new(30.0, 26.0, -14.0), wf, hf);
        let c1 = project_25d(
            Vec3::new(66.0 + curl * 14.0, 34.0 + sway * 0.4 + curl * 12.0, -10.0),
            wf,
            hf,
        );
        let c2 = project_25d(
            Vec3::new(82.0 - curl * 26.0, -4.0 + sway + curl * 42.0, -8.0),
            wf,
            hf,
        );
        let tip = project_25d(
            Vec3::new(112.0 - curl * 58.0, -14.0 + sway * 1.1 + curl * 66.0, -4.0),
            wf,
            hf,
        );

        let mut pb = PathBuilder::new();
        pb.move_to(p0.x, p0.y);
        pb.cubic_to(c1.x, c1.y, c2.x, c2.y, tip.x, tip.y);
        if let Some(path) = pb.finish() {
            let tail_w = 13.5 * s;
            stroke_path(&mut self.pixmap, &path, outline_color, tail_w + 3.0 * s, aa);
            let tail_col = match app.marking_pattern {
                MarkingPattern::Siamese => app.marking_color,
                _ => app.base_coat,
            };
            stroke_path(&mut self.pixmap, &path, rgb_color(tail_col, 1.0), tail_w, aa);

            // Subtle tabby/calico tail rings
            if matches!(
                app.marking_pattern,
                MarkingPattern::TabbyStripes | MarkingPattern::Spotted
            ) {
                let mid = Vec2::new(
                    0.125 * p0.x + 0.375 * c1.x + 0.375 * c2.x + 0.125 * tip.x,
                    0.125 * p0.y + 0.375 * c1.y + 0.375 * c2.y + 0.125 * tip.y,
                );
                if let Some(dot) = ellipse_path(mid.x, mid.y, 4.5 * s, 4.5 * s) {
                    fill_path(&mut self.pixmap, &dot, rgb_color(app.marking_color, 0.85), aa);
                }
            }

            if is_dark_coat {
                let mut rim_pb = PathBuilder::new();
                rim_pb.move_to(p0.x + 10.0 * s, p0.y - 5.0 * s);
                rim_pb.cubic_to(c1.x, c1.y - 6.0 * s, c2.x, c2.y - 6.0 * s, tip.x, tip.y - 5.0 * s);
                if let Some(rp) = rim_pb.finish() {
                    stroke_path(&mut self.pixmap, &rp, rim_color, 1.1 * s, aa);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_tail_front(
        &mut self,
        app: &pet_core::CatAppearance,
        sim: &SimSnapshot,
        wf: f32,
        hf: f32,
        s: f32,
        outline_color: Color,
        rim_color: Color,
        is_dark_coat: bool,
        aa: bool,
    ) {
        // Privacy curl: emerges cleanly 2 px inside right flank and sweeps over the 65% keyboard!
        let t = ((sim.tail_curl_t - 0.08) / 0.92).clamp(0.0, 1.0);
        let start = project_25d(Vec3::new(52.0, 48.0, 12.0), wf, hf);
        let end_x = 52.0 - t * 145.0;
        let end = project_25d(Vec3::new(end_x, 75.0, 26.0), wf, hf);
        let c1 = project_25d(Vec3::new(56.0 - t * 22.0, 74.0, 20.0), wf, hf);
        let c2 = project_25d(Vec3::new(14.0 - t * 88.0, 62.0 + t * 14.0, 26.0), wf, hf);

        let mut pb = PathBuilder::new();
        pb.move_to(start.x, start.y);
        pb.cubic_to(c1.x, c1.y, c2.x, c2.y, end.x, end.y);
        if let Some(path) = pb.finish() {
            let w_px = 18.0 * s;
            stroke_path(&mut self.pixmap, &path, outline_color, w_px + 3.2 * s, aa);
            let col = match app.marking_pattern {
                MarkingPattern::Siamese => app.marking_color,
                _ => app.base_coat,
            };
            stroke_path(&mut self.pixmap, &path, rgb_color(col, 1.0), w_px, aa);

            // Top rim highlight along curled privacy tail so black tail stands out over black keyboard case
            let mut rim = PathBuilder::new();
            rim.move_to(start.x - 4.0 * s, start.y - 7.5 * s);
            rim.cubic_to(
                c1.x,
                c1.y - 8.5 * s,
                c2.x,
                c2.y - 8.5 * s,
                end.x + 2.0 * s,
                end.y - 7.5 * s,
            );
            if let Some(rp) = rim.finish() {
                stroke_path(
                    &mut self.pixmap,
                    &rp,
                    if is_dark_coat { rim_color } else { outline_color },
                    1.35 * s,
                    aa,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_cat_body_and_head(
        &mut self,
        app: &pet_core::CatAppearance,
        sim: &SimSnapshot,
        wf: f32,
        hf: f32,
        s: f32,
        outline_color: Color,
        rim_color: Color,
        is_dark_coat: bool,
        aa: bool,
    ) -> HitRect {
        let breath = (sim.breath_phase).sin() * 1.4 * (1.0 - sim.sleep_t * 0.4);
        let bounce = -sim.torso_bounce * 3.2;
        let sleep_drop = sim.sleep_t * 14.0;
        let squash_y = sim.head_squash * 4.5;

        let torso_c = project_25d(Vec3::new(-4.0, 2.0 + bounce + sleep_drop * 0.5, -8.0), wf, hf);
        let head_c = project_25d(
            Vec3::new(
                -10.0 + sim.head_offset.x,
                -60.0 + sim.head_offset.y + bounce + sleep_drop + squash_y,
                -4.0,
            ),
            wf,
            hf,
        );

        // Unified pear-shaped torso silhouette
        let torso_path = build_torso_path(torso_c, s, breath, sim.sleep_t);
        // Unified head + ears silhouette
        let (head_path, ear_left_inner, ear_right_inner) =
            build_head_and_ears_path(head_c, s, sim.head_squash, sim.ear_relax, sim.head_tilt_rad);

        // Two small tucked back paws at torso base (visible in screenshots 1-6)
        let foot_l = project_25d(Vec3::new(28.0, 44.0 + sleep_drop * 0.2, -3.0), wf, hf);
        let foot_r = project_25d(Vec3::new(-28.0, 44.0 + sleep_drop * 0.2, -3.0), wf, hf);

        // Under-stroke exterior silhouette for clean merged body+head contour
        let out_w = 2.8 * s;
        if let Some(tp) = &torso_path {
            stroke_path(&mut self.pixmap, tp, outline_color, out_w, aa);
        }
        if let Some(hp) = &head_path {
            stroke_path(&mut self.pixmap, hp, outline_color, out_w, aa);
        }

        // Fill torso & head
        let base_col = rgb_color(app.base_coat, 1.0);
        if let Some(tp) = &torso_path {
            fill_path(&mut self.pixmap, tp, base_col, aa);
        }
        if let Some(hp) = &head_path {
            fill_path(&mut self.pixmap, hp, base_col, aa);
        }

        // Coat markings clipped cleanly via `tiny_skia::Mask` to unified body+head silhouette (zero AA seams!)
        if app.marking_pattern != MarkingPattern::Solid && app.marking_pattern != MarkingPattern::Unknown {
            self.draw_clipped_coat_markings(app, torso_path.as_ref(), head_path.as_ref(), torso_c, head_c, s, aa);
        }

        // Tucked back paws (drawn cleanly at torso bottom with thin rim outline)
        for &fp in &[foot_l, foot_r] {
            if let Some(p) = ellipse_path(fp.x, fp.y, 14.0 * s, 10.5 * s) {
                let foot_col = match app.marking_pattern {
                    MarkingPattern::Tuxedo | MarkingPattern::Bicolor | MarkingPattern::Mittens => {
                        rgb_color(app.marking_color, 1.0)
                    }
                    MarkingPattern::Siamese => rgb_color(app.marking_color, 1.0),
                    _ => base_col,
                };
                fill_path(&mut self.pixmap, &p, foot_col, aa);
                stroke_path(
                    &mut self.pixmap,
                    &p,
                    if is_dark_coat { rim_color } else { outline_color },
                    1.25 * s,
                    aa,
                );
            }
        }

        // Inner ear triangles (independently tintable!)
        let inner_col = rgb_color(app.inner_ear_color, 1.0);
        if let Some(el) = ear_left_inner {
            fill_path(&mut self.pixmap, &el, inner_col, aa);
        }
        if let Some(er) = ear_right_inner {
            fill_path(&mut self.pixmap, &er, inner_col, aa);
        }

        // Eyes (heterochromia!), pupils, catchlights, eyelids (open / horizontal pat squint / sleep / privacy)
        self.draw_face(app, sim, head_c, s, aa);

        // Return head hitbox in physical pixels for hover-rub patting
        let hr_w = 96.0 * s;
        let hr_h = 86.0 * s;
        HitRect {
            x: head_c.x - hr_w * 0.5,
            y: head_c.y - hr_h * 0.58,
            w: hr_w,
            h: hr_h,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_clipped_coat_markings(
        &mut self,
        app: &pet_core::CatAppearance,
        torso: Option<&Path>,
        head: Option<&Path>,
        torso_c: Vec2,
        head_c: Vec2,
        s: f32,
        aa: bool,
    ) {
        let w = self.pixmap.width();
        let h = self.pixmap.height();
        let mask = self
            .mask_scratch
            .get_or_insert_with(|| Mask::new(w, h).expect("mask"));
        mask.clear();
        if let Some(tp) = torso {
            mask.fill_path(tp, FillRule::Winding, aa, Transform::identity());
        }
        if let Some(hp) = head {
            mask.fill_path(hp, FillRule::Winding, aa, Transform::identity());
        }

        let mut p1 = Paint::default();
        p1.set_color(rgb_color(app.marking_color, 1.0));
        p1.anti_alias = aa;

        let mut p2 = Paint::default();
        p2.set_color(rgb_color(app.secondary_marking_color, 1.0));
        p2.anti_alias = aa;

        match app.marking_pattern {
            MarkingPattern::TabbyStripes => {
                // Classic forehead M-stripes + cheek stripes + side torso stripes (Luna / Ginger Tabby)
                let stroke = Stroke {
                    width: 4.2 * s,
                    line_cap: LineCap::Round,
                    ..Stroke::default()
                };
                let mut pb = PathBuilder::new();
                // 3 forehead stripes
                for offset in [-10.0f32, 0.0, 10.0] {
                    pb.move_to(head_c.x + offset * s, head_c.y - 36.0 * s);
                    pb.line_to(head_c.x + offset * 0.72 * s, head_c.y - 18.0 * s);
                }
                // Cheek stripes
                for side in [-1.0f32, 1.0] {
                    pb.move_to(head_c.x + side * 44.0 * s, head_c.y + 4.0 * s);
                    pb.line_to(head_c.x + side * 26.0 * s, head_c.y + 7.0 * s);
                    pb.move_to(head_c.x + side * 42.0 * s, head_c.y + 13.0 * s);
                    pb.line_to(head_c.x + side * 27.0 * s, head_c.y + 15.0 * s);
                }
                // Torso side stripes
                for dy in [-6.0f32, 8.0, 22.0] {
                    pb.move_to(torso_c.x + 52.0 * s, torso_c.y + dy * s);
                    pb.line_to(torso_c.x + 26.0 * s, torso_c.y + (dy + 3.0) * s);
                }
                if let Some(path) = pb.finish() {
                    self.pixmap
                        .stroke_path(&path, &p1, &stroke, Transform::identity(), Some(mask));
                }
            }
            MarkingPattern::Tuxedo => {
                // Crisp white chest bib + lower chin/muzzle
                if let Some(bib) = ellipse_path(torso_c.x - 4.0 * s, torso_c.y + 12.0 * s, 26.0 * s, 36.0 * s) {
                    self.pixmap
                        .fill_path(&bib, &p1, FillRule::Winding, Transform::identity(), Some(mask));
                }
                if let Some(chin) = ellipse_path(head_c.x, head_c.y + 22.0 * s, 21.0 * s, 14.0 * s) {
                    self.pixmap
                        .fill_path(&chin, &p1, FillRule::Winding, Transform::identity(), Some(mask));
                }
            }
            MarkingPattern::Bicolor => {
                // Inverted-V white face mask + white belly
                let mut pb = PathBuilder::new();
                pb.move_to(head_c.x, head_c.y - 6.0 * s);
                pb.line_to(head_c.x + 48.0 * s, head_c.y + 38.0 * s);
                pb.line_to(head_c.x - 48.0 * s, head_c.y + 38.0 * s);
                pb.close();
                if let Some(v) = pb.finish() {
                    self.pixmap
                        .fill_path(&v, &p1, FillRule::Winding, Transform::identity(), Some(mask));
                }
                if let Some(belly) = ellipse_path(torso_c.x - 2.0 * s, torso_c.y + 14.0 * s, 35.0 * s, 40.0 * s) {
                    self.pixmap
                        .fill_path(&belly, &p1, FillRule::Winding, Transform::identity(), Some(mask));
                }
            }
            MarkingPattern::Siamese => {
                // Seal-point face mask + darker ear caps
                if let Some(face_mask) = ellipse_path(head_c.x, head_c.y + 4.0 * s, 30.0 * s, 22.0 * s) {
                    self.pixmap
                        .fill_path(&face_mask, &p1, FillRule::Winding, Transform::identity(), Some(mask));
                }
                if let Some(ear_cap_l) = ellipse_path(head_c.x - 28.0 * s, head_c.y - 42.0 * s, 18.0 * s, 22.0 * s) {
                    self.pixmap
                        .fill_path(&ear_cap_l, &p1, FillRule::Winding, Transform::identity(), Some(mask));
                }
                if let Some(ear_cap_r) = ellipse_path(head_c.x + 28.0 * s, head_c.y - 42.0 * s, 18.0 * s, 22.0 * s) {
                    self.pixmap
                        .fill_path(&ear_cap_r, &p1, FillRule::Winding, Transform::identity(), Some(mask));
                }
            }
            MarkingPattern::Calico => {
                // Organic orange & charcoal patches
                for (cx, cy, rx, ry, second) in [
                    (head_c.x - 26.0 * s, head_c.y - 24.0 * s, 26.0 * s, 24.0 * s, false),
                    (head_c.x + 28.0 * s, head_c.y - 26.0 * s, 22.0 * s, 20.0 * s, true),
                    (torso_c.x + 26.0 * s, torso_c.y + 10.0 * s, 26.0 * s, 24.0 * s, false),
                    (torso_c.x - 28.0 * s, torso_c.y + 16.0 * s, 20.0 * s, 22.0 * s, true),
                ] {
                    if let Some(patch) = ellipse_path(cx, cy, rx, ry) {
                        self.pixmap.fill_path(
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
                    (head_c.x - 22.0 * s, head_c.y - 16.0 * s, 18.0 * s, 15.0 * s, false),
                    (head_c.x + 18.0 * s, head_c.y + 12.0 * s, 16.0 * s, 12.0 * s, true),
                    (head_c.x + 26.0 * s, head_c.y - 22.0 * s, 14.0 * s, 14.0 * s, false),
                    (torso_c.x - 16.0 * s, torso_c.y + 4.0 * s, 18.0 * s, 14.0 * s, true),
                    (torso_c.x + 22.0 * s, torso_c.y + 18.0 * s, 20.0 * s, 16.0 * s, false),
                ] {
                    if let Some(patch) = ellipse_path(cx, cy, rx, ry) {
                        self.pixmap.fill_path(
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
                    (head_c.x - 14.0 * s, head_c.y - 24.0 * s, 5.5 * s, 4.0 * s),
                    (head_c.x + 2.0 * s, head_c.y - 27.0 * s, 6.0 * s, 4.2 * s),
                    (head_c.x + 16.0 * s, head_c.y - 23.0 * s, 5.5 * s, 4.0 * s),
                    (torso_c.x + 24.0 * s, torso_c.y - 4.0 * s, 7.5 * s, 5.5 * s),
                    (torso_c.x + 16.0 * s, torso_c.y + 14.0 * s, 8.0 * s, 6.0 * s),
                    (torso_c.x + 32.0 * s, torso_c.y + 24.0 * s, 7.0 * s, 5.5 * s),
                    (torso_c.x - 24.0 * s, torso_c.y + 12.0 * s, 7.0 * s, 5.0 * s),
                ] {
                    if let Some(spot) = ellipse_path(cx, cy, rx, ry) {
                        self.pixmap
                            .fill_path(&spot, &p1, FillRule::Winding, Transform::identity(), Some(mask));
                    }
                }
            }
            MarkingPattern::Mittens => {
                if let Some(chin) = ellipse_path(head_c.x, head_c.y + 22.0 * s, 14.0 * s, 9.0 * s) {
                    self.pixmap
                        .fill_path(&chin, &p1, FillRule::Winding, Transform::identity(), Some(mask));
                }
            }
            MarkingPattern::Solid | MarkingPattern::Unknown => {}
        }
    }

    fn draw_face(
        &mut self,
        app: &pet_core::CatAppearance,
        sim: &SimSnapshot,
        head_c: Vec2,
        s: f32,
        aa: bool,
    ) {
        // Left eye (viewer's right) & Right eye (viewer's left) — heterochromia!
        let eyes = [
            (Vec2::new(head_c.x - 17.5 * s, head_c.y - 2.0 * s), app.right_eye_color),
            (Vec2::new(head_c.x + 17.5 * s, head_c.y - 2.0 * s), app.left_eye_color),
        ];

        let rx = 13.5 * s;
        let ry = 11.2 * s;
        let close = sim.eyelid_close.clamp(0.0, 1.0);

        for (ec, eye_rgb) in eyes {
            if close >= 0.92 {
                // Tightly closed (privacy) or serene curved sleep lid
                let mut pb = PathBuilder::new();
                if sim.privacy_active {
                    pb.move_to(ec.x - rx * 0.88, ec.y + 1.0 * s);
                    pb.line_to(ec.x + rx * 0.88, ec.y + 1.0 * s);
                } else {
                    pb.move_to(ec.x - rx * 0.85, ec.y - 1.0 * s);
                    pb.quad_to(ec.x, ec.y + 6.0 * s, ec.x + rx * 0.85, ec.y - 1.0 * s);
                }
                if let Some(p) = pb.finish() {
                    let lid_col = if app.base_coat.luminance() < 0.22 {
                        rgb_color(eye_rgb.scale(0.85), 0.9)
                    } else {
                        rgb_color(Rgb8::new(34, 34, 42), 0.9)
                    };
                    stroke_path(&mut self.pixmap, &p, lid_col, 2.5 * s, aa);
                }
            } else {
                // Open or half-squinted oval eye (screenshot 2 shows flat horizontal slit squint!)
                let open_ry = (ry * (1.0 - close * 0.88)).max(1.6 * s);
                if let Some(eye_oval) = ellipse_path(ec.x, ec.y, rx, open_ry) {
                    fill_path(&mut self.pixmap, &eye_oval, rgb_color(eye_rgb, 1.0), aa);

                    // Vertical slit/oval black pupil tracking global gaze
                    if close < 0.55 {
                        let px = ec.x + sim.eye_gaze.x * (rx * 0.42);
                        let py = ec.y + sim.eye_gaze.y * (open_ry * 0.35);
                        let prx = 4.2 * s;
                        let pry = (open_ry * 0.72).min(8.2 * s);
                        if let Some(pupil) = ellipse_path(px, py, prx, pry) {
                            fill_path(&mut self.pixmap, &pupil, rgb_color(Rgb8::new(14, 14, 18), 1.0), aa);
                        }
                        // Crisp small specular highlight
                        if let Some(glint) = ellipse_path(px - 1.4 * s, py - 2.4 * s, 1.4 * s, 1.4 * s) {
                            fill_path(&mut self.pixmap, &glint, Color::from_rgba(1.0, 1.0, 1.0, 0.88).unwrap(), aa);
                        }
                    }
                }
            }
        }

        // Cute small triangle nose (visible on lighter coats, subtle on dark coats)
        if app.base_coat.luminance() >= 0.16 {
            let mut nb = PathBuilder::new();
            nb.move_to(head_c.x - 3.6 * s, head_c.y + 10.0 * s);
            nb.line_to(head_c.x + 3.6 * s, head_c.y + 10.0 * s);
            nb.line_to(head_c.x, head_c.y + 14.0 * s);
            nb.close();
            if let Some(np) = nb.finish() {
                fill_path(&mut self.pixmap, &np, rgb_color(app.nose_color, 0.95), aa);
            }
        }

        // 3 crisp whiskers per cheek (matching screenshots 1-6)
        let mut wb = PathBuilder::new();
        for side in [-1.0f32, 1.0] {
            for (dy_in, dy_out, len) in [(11.0, 8.0, 21.0), (15.5, 16.0, 23.0), (20.0, 24.5, 20.0)] {
                wb.move_to(head_c.x + side * 24.0 * s, head_c.y + dy_in * s);
                wb.line_to(head_c.x + side * (24.0 + len) * s, head_c.y + dy_out * s);
            }
        }
        if let Some(wp) = wb.finish() {
            stroke_path(
                &mut self.pixmap,
                &wp,
                rgb_color(app.whisker_color, 0.82),
                1.15 * s,
                aa,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_arms_and_paws(
        &mut self,
        app: &pet_core::CatAppearance,
        sim: &SimSnapshot,
        wf: f32,
        hf: f32,
        s: f32,
        outline_color: Color,
        rim_color: Color,
        is_dark_coat: bool,
        aa: bool,
    ) {
        let bounce = -sim.torso_bounce * 3.0;
        let sleep_drop = sim.sleep_t * 10.0;

        // Shoulders blend smoothly into the upper torso silhouette (no round socket ring across chest)
        let left_shoulder = project_25d(Vec3::new(24.0, -14.0 + bounce + sleep_drop, 4.0), wf, hf);
        let right_shoulder = project_25d(Vec3::new(-32.0, -14.0 + bounce + sleep_drop, 4.0), wf, hf);

        let left_paw = project_25d(sim.left_paw_world, wf, hf);
        let right_paw = project_25d(sim.right_paw_world, wf, hf);

        let arm_pairs = [
            (right_shoulder, right_paw, -1.0f32),
            (left_shoulder, left_paw, 1.0f32),
        ];

        let arm_w = 14.2 * s;
        let paw_r = 9.4 * s;

        for (shoulder, paw, side) in arm_pairs {
            // Slight outward cartoon bend for tubular arms (matching screenshots 1-2)
            let mid = Vec2::new(
                (shoulder.x + paw.x) * 0.5 + side * 7.5 * s,
                (shoulder.y + paw.y) * 0.5 - 3.5 * s,
            );
            // Start exterior outline slightly down the arm so no circular cap cuts into the chest
            let out_start = shoulder.lerp(mid, 0.24);
            let mut opb = PathBuilder::new();
            opb.move_to(out_start.x, out_start.y);
            opb.quad_to(mid.x, mid.y, paw.x, paw.y);
            if let Some(out_path) = opb.finish() {
                stroke_path(&mut self.pixmap, &out_path, outline_color, arm_w + 2.8 * s, aa);
            }

            let mut pb = PathBuilder::new();
            pb.move_to(shoulder.x, shoulder.y);
            pb.quad_to(mid.x, mid.y, paw.x, paw.y);
            if let Some(arm_path) = pb.finish() {
                stroke_path(&mut self.pixmap, &arm_path, rgb_color(app.base_coat, 1.0), arm_w, aa);

                // Smooth shoulder blend fill covering top of arm outline
                if let Some(sh_blend) = ellipse_path(shoulder.x, shoulder.y, arm_w * 0.62, arm_w * 0.58) {
                    fill_path(&mut self.pixmap, &sh_blend, rgb_color(app.base_coat, 1.0), aa);
                }

                // Delicate edge rim along foreground-separating arm silhouettes (matching screenshots 1-2)
                let rim_start = shoulder.lerp(mid, 0.28);
                let rim_off = side * (arm_w * 0.50);
                let mut rpb = PathBuilder::new();
                rpb.move_to(rim_start.x + rim_off, rim_start.y);
                rpb.quad_to(mid.x + rim_off, mid.y, paw.x + rim_off * 0.72, paw.y - 1.5 * s);
                if let Some(rp) = rpb.finish() {
                    stroke_path(
                        &mut self.pixmap,
                        &rp,
                        if is_dark_coat { rim_color } else { outline_color },
                        1.15 * s,
                        aa,
                    );
                }
                if is_dark_coat {
                    let inner_off = -side * (arm_w * 0.50);
                    let mut ipb = PathBuilder::new();
                    ipb.move_to(rim_start.x + inner_off, rim_start.y + 3.0 * s);
                    ipb.quad_to(mid.x + inner_off, mid.y, paw.x + inner_off * 0.65, paw.y);
                    if let Some(ip) = ipb.finish() {
                        stroke_path(&mut self.pixmap, &ip, rim_color, 1.1 * s, aa);
                    }
                }
            }

            // Rounded paw tip (with mittens/tuxedo/Siamese paw-tip coloring)
            if let Some(paw_ball) = ellipse_path(paw.x, paw.y, paw_r, paw_r * 0.80) {
                let paw_col = match app.marking_pattern {
                    MarkingPattern::Tuxedo
                    | MarkingPattern::Bicolor
                    | MarkingPattern::Mittens
                    | MarkingPattern::Siamese => rgb_color(app.marking_color, 1.0),
                    _ => rgb_color(app.base_coat, 1.0),
                };
                fill_path(&mut self.pixmap, &paw_ball, paw_col, aa);
                stroke_path(
                    &mut self.pixmap,
                    &paw_ball,
                    if is_dark_coat { rim_color } else { outline_color },
                    1.05 * s,
                    aa,
                );
            }
        }
    }

    fn draw_popups(&mut self, sim: &SimSnapshot, wf: f32, hf: f32, s: f32, aa: bool) {
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

            // Keycap skirt + crisp white keycap face (matching screenshots 3-6!)
            if let Some(skirt) = rounded_rect(
                pos.x - cap_w * 0.5,
                pos.y - cap_h * 0.5 + 2.4 * s,
                cap_w,
                cap_h,
                5.0 * s,
            ) {
                fill_path(
                    &mut self.pixmap,
                    &skirt,
                    Color::from_rgba(0.14, 0.15, 0.18, alpha * 0.85).unwrap(),
                    aa,
                );
            }
            if let Some(face) =
                rounded_rect(pos.x - cap_w * 0.5, pos.y - cap_h * 0.5, cap_w, cap_h, 5.0 * s)
            {
                fill_path(
                    &mut self.pixmap,
                    &face,
                    Color::from_rgba(0.97, 0.98, 0.99, alpha).unwrap(),
                    aa,
                );
                stroke_path(
                    &mut self.pixmap,
                    &face,
                    Color::from_rgba(0.18, 0.20, 0.24, alpha).unwrap(),
                    1.3 * s,
                    aa,
                );
            }
            let font_h = if pop.label.len() > 2 { 7.5 * s } else { 11.5 * s };
            draw_text_centered(
                &mut self.pixmap,
                &pop.label,
                pos.x,
                pos.y,
                font_h,
                Color::from_rgba(0.12, 0.14, 0.18, alpha).unwrap(),
                1.1,
            );
        }
    }

    fn draw_sleep_particles(&mut self, sim: &SimSnapshot, wf: f32, hf: f32, s: f32) {
        for zp in &sim.sleep_particles {
            let t = (zp.age / zp.ttl).clamp(0.0, 1.0);
            let alpha = ((t / 0.2).min(1.0) * ((1.0 - t) / 0.35).min(1.0)).clamp(0.0, 1.0);
            let pos = project_25d(Vec3::new(zp.x, zp.y, 18.0), wf, hf);
            // Crisp dark outline under-stroke so floating `Z`s remain legible on both dark & light wallpapers
            draw_text_centered(
                &mut self.pixmap,
                "Z",
                pos.x,
                pos.y,
                13.5 * zp.scale * s,
                Color::from_rgba(0.14, 0.16, 0.22, alpha * 0.90).unwrap(),
                2.3,
            );
            draw_text_centered(
                &mut self.pixmap,
                "Z",
                pos.x,
                pos.y,
                13.5 * zp.scale * s,
                Color::from_rgba(0.95, 0.96, 1.0, alpha * 0.98).unwrap(),
                1.15,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_single_keycap(
    pixmap: &mut Pixmap,
    spec: &pet_core::KeyCapSpec,
    per: &pet_core::PeripheralStyle,
    depress: f32,
    glow: f32,
    wf: f32,
    hf: f32,
    s: f32,
    show_legends: bool,
    aa: bool,
) {
    let u_half = (spec.width_u / 16.0) * 0.93;
    let y_half = 0.165;
    let x0 = spec.kb_x - u_half;
    let x1 = spec.kb_x + u_half;
    let y0 = spec.kb_y - y_half;
    let y1 = spec.kb_y + y_half;

    let tl = project_25d(keyboard_local_to_world(x0, y0, depress), wf, hf);
    let tr = project_25d(keyboard_local_to_world(x1, y0, depress), wf, hf);
    let br = project_25d(keyboard_local_to_world(x1, y1, depress), wf, hf);
    let bl = project_25d(keyboard_local_to_world(x0, y1, depress), wf, hf);

    let base_rgb = if spec.is_accent {
        per.keycap_accent
    } else {
        per.keycap_base
    };
    let face_rgb = base_rgb.lerp(per.keycap_highlight, glow.clamp(0.0, 1.0));

    // Tiny keycap shadow edge
    let drop_y = (1.8 - depress * 1.2).max(0.4) * s;
    if let Some(p) = rounded_quad(
        Vec2::new(tl.x, tl.y + drop_y),
        Vec2::new(tr.x, tr.y + drop_y),
        Vec2::new(br.x, br.y + drop_y),
        Vec2::new(bl.x, bl.y + drop_y),
        1.6 * s,
    ) {
        fill_path(pixmap, &p, rgb_color(face_rgb.scale(0.72), 1.0), aa);
    }

    if let Some(p) = rounded_quad(tl, tr, br, bl, 1.6 * s) {
        fill_path(pixmap, &p, rgb_color(face_rgb, 1.0), aa);
    }

    if show_legends && spec.label.len() <= 2 {
        let cx = (tl.x + tr.x + br.x + bl.x) * 0.25;
        let cy = (tl.y + tr.y + br.y + bl.y) * 0.25;
        draw_text_centered(
            pixmap,
            spec.label,
            cx,
            cy,
            5.2 * s,
            rgb_color(per.keycap_legend_color, 0.85),
            0.95,
        );
    }
}

fn build_torso_path(c: Vec2, s: f32, breath: f32, sleep_t: f32) -> Option<Path> {
    let w_top = (36.0 + breath * 0.5) * s;
    let w_bot = (56.0 + sleep_t * 6.0 + breath) * s;
    let h_top = (-38.0 + sleep_t * 8.0) * s;
    let h_bot = 48.0 * s;

    let mut pb = PathBuilder::new();
    pb.move_to(c.x - w_top, c.y + h_top);
    pb.cubic_to(
        c.x - w_bot * 0.92,
        c.y - 4.0 * s,
        c.x - w_bot,
        c.y + h_bot * 0.85,
        c.x - w_bot * 0.52,
        c.y + h_bot,
    );
    pb.line_to(c.x + w_bot * 0.62, c.y + h_bot);
    pb.cubic_to(
        c.x + w_bot * 1.12,
        c.y + h_bot * 0.82,
        c.x + w_bot * 0.96,
        c.y - 6.0 * s,
        c.x + w_top * 0.92,
        c.y + h_top,
    );
    pb.close();
    pb.finish()
}

fn build_head_and_ears_path(
    c: Vec2,
    s: f32,
    squash: f32,
    ear_relax: f32,
    tilt_rad: f32,
) -> (Option<Path>, Option<Path>, Option<Path>) {
    let rx = (47.0 + squash * 4.0) * s;
    let ry = (38.0 - squash * 3.0) * s;
    let relax_x = ear_relax * 8.0 * s;
    let relax_y = ear_relax * 7.0 * s;

    let rot = |dx: f32, dy: f32| -> Vec2 {
        let (sin_t, cos_t) = tilt_rad.sin_cos();
        Vec2::new(c.x + dx * cos_t - dy * sin_t, c.y + dx * sin_t + dy * cos_t)
    };

    // Left ear (viewer left) & Right ear (viewer right) merged smoothly with rounded head
    let l_base_out = rot(-rx * 0.82, -ry * 0.35);
    let l_tip = rot(-rx * 0.72 - relax_x, -ry * 1.56 + relax_y);
    let l_base_in = rot(-rx * 0.16, -ry * 0.94);

    let r_base_in = rot(rx * 0.16, -ry * 0.94);
    let r_tip = rot(rx * 0.72 + relax_x, -ry * 1.56 + relax_y);
    let r_base_out = rot(rx * 0.82, -ry * 0.35);

    let cheek_r = rot(rx * 1.02, ry * 0.15);
    let chin = rot(0.0, ry * 1.02);
    let cheek_l = rot(-rx * 1.02, ry * 0.15);

    let mut pb = PathBuilder::new();
    pb.move_to(l_base_out.x, l_base_out.y);
    pb.quad_to(rot(-rx * 0.88 - relax_x * 0.5, -ry * 1.1 + relax_y * 0.5).x,
               rot(-rx * 0.88 - relax_x * 0.5, -ry * 1.1 + relax_y * 0.5).y,
               l_tip.x, l_tip.y);
    pb.quad_to(rot(-rx * 0.42, -ry * 1.38 + relax_y * 0.4).x,
               rot(-rx * 0.42, -ry * 1.38 + relax_y * 0.4).y,
               l_base_in.x, l_base_in.y);
    pb.quad_to(rot(0.0, -ry * 1.04).x, rot(0.0, -ry * 1.04).y, r_base_in.x, r_base_in.y);
    pb.quad_to(rot(rx * 0.42, -ry * 1.38 + relax_y * 0.4).x,
               rot(rx * 0.42, -ry * 1.38 + relax_y * 0.4).y,
               r_tip.x, r_tip.y);
    pb.quad_to(rot(rx * 0.88 + relax_x * 0.5, -ry * 1.1 + relax_y * 0.5).x,
               rot(rx * 0.88 + relax_x * 0.5, -ry * 1.1 + relax_y * 0.5).y,
               r_base_out.x, r_base_out.y);
    pb.cubic_to(
        cheek_r.x,
        cheek_r.y,
        rot(rx * 0.72, ry * 1.04).x,
        rot(rx * 0.72, ry * 1.04).y,
        chin.x,
        chin.y,
    );
    pb.cubic_to(
        rot(-rx * 0.72, ry * 1.04).x,
        rot(-rx * 0.72, ry * 1.04).y,
        cheek_l.x,
        cheek_l.y,
        l_base_out.x,
        l_base_out.y,
    );
    pb.close();

    // Inner ear triangles
    let inner_ear = |side: f32| -> Option<Path> {
        let a = rot(side * rx * 0.66 + side * relax_x * 0.72, -ry * 1.34 + relax_y * 0.75);
        let b = rot(side * rx * 0.70, -ry * 0.68);
        let d = rot(side * rx * 0.28, -ry * 0.90);
        let mut eb = PathBuilder::new();
        eb.move_to(a.x, a.y);
        eb.line_to(b.x, b.y);
        eb.line_to(d.x, d.y);
        eb.close();
        eb.finish()
    };

    (pb.finish(), inner_ear(-1.0), inner_ear(1.0))
}

fn clamp_dimensions(w: u32, h: u32) -> (u32, u32) {
    let w = w.clamp(64, MAX_CANVAS_EDGE_PX);
    let h = h.clamp(64, MAX_CANVAS_EDGE_PX);
    (w, h)
}

fn rgb_color(rgb: Rgb8, alpha: f32) -> Color {
    Color::from_rgba8(
        rgb.r,
        rgb.g,
        rgb.b,
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn fill_path(pixmap: &mut Pixmap, path: &Path, color: Color, aa: bool) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = aa;
    pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn stroke_path(pixmap: &mut Pixmap, path: &Path, color: Color, width: f32, aa: bool) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = aa;
    let stroke = Stroke {
        width: width.max(0.5),
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(path, &paint, &stroke, Transform::identity(), None);
}

fn ellipse_path(cx: f32, cy: f32, rx: f32, ry: f32) -> Option<Path> {
    let kx = rx * 0.552_284_8;
    let ky = ry * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy - ry);
    pb.cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);
    pb.cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);
    pb.cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);
    pb.cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);
    pb.close();
    pb.finish()
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w * 0.5).min(h * 0.5);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

fn rounded_quad(tl: Vec2, tr: Vec2, br: Vec2, bl: Vec2, r: f32) -> Option<Path> {
    let inset = |a: Vec2, b: Vec2| -> Vec2 {
        let d = a.dist(b).max(1e-4);
        let t = (r / d).min(0.4);
        a.lerp(b, t)
    };
    let mut pb = PathBuilder::new();
    let p0 = inset(tl, tr);
    pb.move_to(p0.x, p0.y);
    let p1 = inset(tr, tl);
    pb.line_to(p1.x, p1.y);
    let p2 = inset(tr, br);
    pb.quad_to(tr.x, tr.y, p2.x, p2.y);
    let p3 = inset(br, tr);
    pb.line_to(p3.x, p3.y);
    let p4 = inset(br, bl);
    pb.quad_to(br.x, br.y, p4.x, p4.y);
    let p5 = inset(bl, br);
    pb.line_to(p5.x, p5.y);
    let p6 = inset(bl, tl);
    pb.quad_to(bl.x, bl.y, p6.x, p6.y);
    let p7 = inset(tl, bl);
    pb.line_to(p7.x, p7.y);
    pb.quad_to(tl.x, tl.y, p0.x, p0.y);
    pb.close();
    pb.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pet_core::{builtin_presets, Config, PetSim, PhysicalKey, PoseOverride};

    fn save_png(path: &std::path::Path, w: u32, h: u32, premul_rgba: &[u8]) {
        let mut unpremul = vec![0u8; premul_rgba.len()];
        for (dst, src) in unpremul.chunks_exact_mut(4).zip(premul_rgba.chunks_exact(4)) {
            let a = src[3];
            if a == 0 {
                dst.fill(0);
            } else if a == 255 {
                dst.copy_from_slice(src);
            } else {
                let af = a as f32 / 255.0;
                dst[0] = (src[0] as f32 / af).min(255.0) as u8;
                dst[1] = (src[1] as f32 / af).min(255.0) as u8;
                dst[2] = (src[2] as f32 / af).min(255.0) as u8;
                dst[3] = a;
            }
        }
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&unpremul).unwrap();
    }

    #[test]
    fn renders_all_15_presets_and_verifies_black_cat_rim_contrast() {
        let mut renderer = Renderer::new(360, 300);
        let presets = builtin_presets();
        assert_eq!(presets.len(), 15);

        let mut sim = PetSim::default();
        sim.step(0.016);
        let snap = sim.snapshot();

        for profile in &presets {
            let hb = renderer.render(profile, &snap, RenderOptions::default());
            assert!(hb.head_rect_px.w > 40.0);
            // Must contain non-transparent cat pixels
            let opaque_count = renderer
                .rgba_bytes()
                .chunks_exact(4)
                .filter(|px| px[3] > 200)
                .count();
            assert!(opaque_count > 5000, "preset {} rendered too few pixels", profile.id);
        }

        // Verify Midnight black-cat rim contrast separates arms from dark torso
        let midnight = &presets[0];
        renderer.render(midnight, &snap, RenderOptions::default());
        let rim_bright_pixels = renderer
            .rgba_bytes()
            .chunks_exact(4)
            .filter(|px| {
                // Mid-grey/silver rim pixels separating arms against #141418 black torso
                px[3] > 180
                    && px[0] > 75
                    && px[0] < 210
                    && (px[0] as i32 - px[1] as i32).abs() < 20
                    && (px[1] as i32 - px[2] as i32).abs() < 25
            })
            .count();
        assert!(
            rim_bright_pixels > 40,
            "black cat rim contrast too weak: only {rim_bright_pixels} edge pixels"
        );
    }

    #[test]
    fn export_visual_inspection_pngs() {
        let out_dir = std::env::temp_dir().join("desktop_cat_golden");
        let _ = std::fs::create_dir_all(&out_dir);

        let mut renderer = Renderer::new(540, 450);
        let presets = builtin_presets();
        let midnight = &presets[0];
        let luna = &presets[1];

        // 1. Midnight idle (mouse + keyboard)
        let mut sim = PetSim::default();
        sim.step(0.016);
        renderer.render(midnight, &sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("midnight_idle.png"), 540, 450, renderer.rgba_bytes());

        // 2. Midnight Showcase typing + popups
        let cfg = Config {
            showcase_mode: true,
            show_floating_popups: true,
            ..Config::default()
        };
        for key in [PhysicalKey::P, PhysicalKey::K, PhysicalKey::J, PhysicalKey::F, PhysicalKey::Space] {
            sim.on_key_event(key, true, &cfg);
            sim.step(0.03);
        }
        renderer.render(
            midnight,
            &sim.snapshot(),
            RenderOptions {
                show_keycap_legends: true,
                ..RenderOptions::default()
            },
        );
        save_png(&out_dir.join("midnight_typing.png"), 540, 450, renderer.rgba_bytes());

        // 3. Midnight privacy tail curl
        sim.toggle_manual_privacy();
        for _ in 0..45 {
            sim.step(0.03);
        }
        renderer.render(midnight, &sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("midnight_privacy.png"), 540, 450, renderer.rgba_bytes());

        // 4. Midnight head-pat squint
        let mut pat_sim = PetSim::default();
        let unlocked = Config::default();
        for i in 0..16 {
            pat_sim.on_head_hover_dip(true, if i % 2 == 0 { 10.0 } else { 42.0 }, &unlocked);
            pat_sim.step(0.02);
        }
        renderer.render(midnight, &pat_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("midnight_pat.png"), 540, 450, renderer.rgba_bytes());

        // 5. Luna heterochromia tabby + sleep z particles
        let mut sleep_sim = PetSim::default();
        sleep_sim.set_pose_override(PoseOverride::Sleep);
        for _ in 0..80 {
            sleep_sim.step(0.04);
        }
        renderer.render(luna, &sleep_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("luna_sleep.png"), 540, 450, renderer.rgba_bytes());

        // 6. Luna awake on solid Capture Stage green-screen
        let mut luna_sim = PetSim::default();
        luna_sim.step(0.016);
        renderer.render(
            luna,
            &luna_sim.snapshot(),
            RenderOptions {
                backdrop: BackdropMode::Solid(Rgb8::from_hex(0x00B140)),
                chroma_hard_edge: false,
                show_keycap_legends: true,
            },
        );
        save_png(&out_dir.join("luna_greenscreen.png"), 540, 450, renderer.rgba_bytes());
    }
}
