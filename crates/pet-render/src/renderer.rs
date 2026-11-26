use crate::cache::{CroppedMaskScratch, StaticMouseSprite, TightStaticDeskCache};
use crate::cat_body::{draw_arms_and_paws, draw_cat_body_and_head};
use crate::effects::{draw_popups, draw_sleep_particles};
use crate::geom::BBoxAcc;
use crate::palette::{rgb_color, CelPalette};
use crate::peripherals::{draw_active_keycaps, draw_mouse};
use crate::tail::{draw_tail_back, draw_tail_front};
use pet_core::{
    BackdropMode, CatProfile, FrameHitboxes, HitRect, SimSnapshot, BASE_CANVAS_H, BASE_CANVAS_W,
    MAX_CANVAS_EDGE_PX,
};
use tiny_skia::{Color, Pixmap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Pure-Rust 2.5D cel-shaded vector renderer with tight-cropped static desk & mouse caches,
/// row-cropped coat marking mask, filled tapered organic limbs, unified head transform,
/// and exact `prev ∪ curr` dirty-rect bounds for minimal DWM commits.
pub struct Renderer {
    pixmap: Pixmap,
    desk_cache: TightStaticDeskCache,
    mouse_sprite: StaticMouseSprite,
    mask_scratch: CroppedMaskScratch,
    last_hitboxes: FrameHitboxes,
    prev_dynamic_rect: Option<HitRect>,
    last_opts: Option<RenderOptions>,
    last_profile_id: String,
    force_full_dirty: bool,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Self {
        let (w, h) = clamp_dimensions(width, height);
        Self {
            pixmap: Pixmap::new(w, h).expect("valid pixmap dimensions"),
            desk_cache: TightStaticDeskCache::default(),
            mouse_sprite: StaticMouseSprite::default(),
            mask_scratch: CroppedMaskScratch::default(),
            last_hitboxes: FrameHitboxes::default(),
            prev_dynamic_rect: None,
            last_opts: None,
            last_profile_id: String::new(),
            force_full_dirty: true,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = clamp_dimensions(width, height);
        if self.pixmap.width() != w || self.pixmap.height() != h {
            self.pixmap = Pixmap::new(w, h).expect("valid pixmap dimensions");
            self.desk_cache.invalidate();
            self.mouse_sprite.invalidate();
            self.mask_scratch.invalidate();
            self.prev_dynamic_rect = None;
            self.force_full_dirty = true;
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

    pub fn cache_rebuilds(&self) -> u64 {
        self.desk_cache.rebuilds + self.mouse_sprite.rebuilds
    }

    /// Render a single frame into the internal premultiplied RGBA8 pixmap and return
    /// neutral head hitbox, body bounds, and exact `prev ∪ curr` dirty rectangle in physical pixels.
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
        let pal = CelPalette::derive(app, per);
        let mut dyn_bbox = BBoxAcc::default();

        // Pass 2: Lush tapered organic TailBackSegment (behind torso)
        draw_tail_back(&mut self.pixmap, app, &pal, sim, wf, hf, s, aa, &mut dyn_bbox);

        // Pass 3 & 4: Tight-cropped static desk & sculpted 65% keyboard cache
        let desk_rebuilt = self.desk_cache.ensure_and_blit(&mut self.pixmap, per, opts);

        // Dynamic pressed/glowing keycaps (automatically skipped in Privacy Mode!)
        let show_legends = opts.show_keycap_legends || per.show_keycap_legends;
        draw_active_keycaps(
            &mut self.pixmap,
            per,
            sim,
            wf,
            hf,
            s,
            show_legends,
            aa,
            &mut dyn_bbox,
        );

        // Miniature mouse sprite + unoccluded front LMB/RMB depressions + rolling scroll notches
        draw_mouse(
            &mut self.pixmap,
            &mut self.mouse_sprite,
            per,
            sim,
            wf,
            hf,
            s,
            opts.chroma_hard_edge,
            &mut dyn_bbox,
        );

        // Pass 5: Unified Cel-Shaded Torso + Tucked Back Paws + Head + Ears + Coat Markings + Face
        let head_rect = draw_cat_body_and_head(
            &mut self.pixmap,
            &mut self.mask_scratch,
            app,
            &pal,
            sim,
            wf,
            hf,
            s,
            aa,
            &mut dyn_bbox,
        );

        // Pass 6: Filled Tapered Bezier Forearms & Sculpted Paws
        draw_arms_and_paws(
            &mut self.pixmap,
            app,
            &pal,
            sim,
            wf,
            hf,
            s,
            aa,
            &mut dyn_bbox,
        );

        // Pass 7: Fuller Fluffy Scalloped Privacy Tail Curtain (100% covers keyboard in Privacy Mode)
        if sim.tail_curl_t > 0.08 {
            draw_tail_front(&mut self.pixmap, app, &pal, sim, wf, hf, s, aa, &mut dyn_bbox);
        }

        // Pass 8: Floating Showcase keycap pop-ups + Drowsy sleep `Z` particles
        draw_popups(&mut self.pixmap, sim, wf, hf, s, aa, &mut dyn_bbox);
        draw_sleep_particles(&mut self.pixmap, sim, wf, hf, s, &mut dyn_bbox);

        // Exact `prev_dynamic_rect ∪ curr_dynamic_rect` dirty rect (guaranteed bit-exact outside!)
        let pad = (6.0 * s).ceil() + 4.0;
        let curr_dyn = dyn_bbox.to_hit_rect(pad, wf, hf);
        let opts_changed = self.last_opts != Some(opts);
        let profile_changed = self.last_profile_id != profile.id;
        let dirty_rect_px = match self.prev_dynamic_rect {
            Some(prev)
                if !self.force_full_dirty && !desk_rebuilt && !opts_changed && !profile_changed =>
            {
                prev.union(curr_dyn).clamp_to_canvas(wf, hf)
            }
            _ => HitRect {
                x: 0.0,
                y: 0.0,
                w: wf,
                h: hf,
            },
        };

        self.prev_dynamic_rect = Some(curr_dyn);
        self.last_opts = Some(opts);
        self.last_profile_id = profile.id.clone();
        self.force_full_dirty = false;

        let body_bounds_px = HitRect {
            x: (wf * 0.10).max(0.0),
            y: (hf * 0.14).max(0.0),
            w: (wf * 0.80).min(wf),
            h: (hf * 0.82).min(hf),
        };
        let hitboxes = FrameHitboxes {
            head_rect_px: head_rect,
            body_bounds_px,
            dirty_rect_px,
        };
        self.last_hitboxes = hitboxes;
        hitboxes
    }
}

fn clamp_dimensions(w: u32, h: u32) -> (u32, u32) {
    let w = w.clamp(64, MAX_CANVAS_EDGE_PX);
    let h = h.clamp(64, MAX_CANVAS_EDGE_PX);
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pet_core::{builtin_presets, Config, PetSim, PhysicalKey, PoseOverride, Vec2};

    fn save_png(path: &std::path::Path, w: u32, h: u32, premul_rgba: &[u8]) {
        let mut unpremul = vec![0u8; premul_rgba.len()];
        for (dst, src) in unpremul
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(premul_rgba.as_chunks::<4>().0.iter())
        {
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
    fn renders_all_15_presets_with_cel_and_privacy_tail_contrast() {
        let mut renderer = Renderer::new(360, 300);
        let presets = builtin_presets();
        assert_eq!(presets.len(), 15);

        let mut sim = PetSim::default();
        sim.step(0.016);
        let awake_snap = sim.snapshot();

        let mut priv_sim = PetSim::default();
        priv_sim.toggle_manual_privacy();
        for _ in 0..60 {
            priv_sim.step(1.0 / 30.0);
        }
        let priv_snap = priv_sim.snapshot();

        for profile in &presets {
            let hb = renderer.render(profile, &awake_snap, RenderOptions::default());
            assert!(hb.head_rect_px.w > 40.0);
            let opaque_count = renderer
                .rgba_bytes()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[3] > 200)
                .count();
            assert!(
                opaque_count > 6000,
                "preset {} rendered too few pixels",
                profile.id
            );

            // Render privacy pose on every preset and verify rich opaque silhouette
            renderer.render(profile, &priv_snap, RenderOptions::default());
            let priv_opaque = renderer
                .rgba_bytes()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[3] > 200)
                .count();
            assert!(
                priv_opaque > 6000,
                "privacy pose rendered too few pixels on {}",
                profile.id
            );
        }

        // Verify Midnight Privacy Mode covers 100% of all 68 white & orange keycaps (zero exposed keys!)
        let midnight = &presets[0];
        renderer.render(midnight, &priv_snap, RenderOptions::default());
        let w = renderer.width();
        let h = renderer.height();
        let data = renderer.rgba_bytes();
        let mut exposed_keycap_pixels = 0usize;
        for y in 0..h {
            for x in 0..w {
                // Check strict keyboard keycap interior box (`x in 92..312`, `y in 218..270`)
                if (94..=310).contains(&x) && (220..=268).contains(&y) {
                    let off = ((y * w + x) * 4) as usize;
                    let (r, g, b, a) = (data[off], data[off + 1], data[off + 2], data[off + 3]);
                    let is_white_key = a > 200 && r > 220 && g > 220 && b > 220;
                    let is_orange_key = a > 200 && r > 210 && g > 130 && b < 80;
                    if is_white_key || is_orange_key {
                        exposed_keycap_pixels += 1;
                    }
                }
            }
        }
        assert_eq!(
            exposed_keycap_pixels, 0,
            "Privacy tail curtain left {exposed_keycap_pixels} exposed keycap pixels!"
        );

        // Verify Midnight black-cat cel rim contrast separates arms & privacy tail from dark coat/keyboard
        let rim_bright_pixels = renderer
            .rgba_bytes()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| {
                px[3] > 180
                    && px[0] > 75
                    && px[0] < 220
                    && (px[0] as i32 - px[1] as i32).abs() < 25
                    && (px[1] as i32 - px[2] as i32).abs() < 35
            })
            .count();
        assert!(
            rim_bright_pixels > 80,
            "black cat cel/privacy rim contrast too weak: only {rim_bright_pixels} edge pixels"
        );
    }

    #[test]
    fn dirty_rect_is_bit_exact_outside_reported_bounds() {
        let w = 360u32;
        let h = 300u32;
        let mut renderer = Renderer::new(w, h);
        let profile = builtin_presets()[0].clone();
        let cfg = Config {
            showcase_mode: true,
            show_floating_popups: true,
            ..Config::default()
        };
        let mut sim = PetSim::default();

        // Warm-up first frame (full dirty)
        sim.step(0.016);
        let _ = renderer.render(&profile, &sim.snapshot(), RenderOptions::default());
        let mut prev_buf = renderer.rgba_bytes().to_vec();

        // Step through typing, mouse click + scroll, head pat, idle floor rest, privacy curl
        let mut checked_partial_frames = 0usize;
        for step_idx in 0..40 {
            match step_idx {
                1 => sim.on_key_event(PhysicalKey::F, true, &cfg),
                3 => sim.on_key_event(PhysicalKey::F, false, &cfg),
                5 => sim.on_mouse_input(
                    -10,
                    6,
                    Some(false),
                    true,
                    None,
                    false,
                    120,
                    Some(Vec2::new(-0.5, 0.3)),
                ),
                8 => sim.on_mouse_input(
                    6,
                    -4,
                    None,
                    false,
                    Some(false),
                    true,
                    -120,
                    Some(Vec2::new(0.4, -0.2)),
                ),
                12..=22 => {
                    let x = if step_idx % 2 == 0 { 10.0 } else { 45.0 };
                    sim.on_head_hover_dip(true, x, &Config::default());
                }
                28 => {
                    sim.toggle_manual_privacy();
                }
                _ => {}
            }
            sim.step(1.0 / 30.0);
            let hb = renderer.render(&profile, &sim.snapshot(), RenderOptions::default());
            let cur_buf = renderer.rgba_bytes();

            let x0 = hb.dirty_rect_px.x.floor().max(0.0) as u32;
            let y0 = hb.dirty_rect_px.y.floor().max(0.0) as u32;
            let x1 = (hb.dirty_rect_px.x + hb.dirty_rect_px.w).ceil().min(w as f32) as u32;
            let y1 = (hb.dirty_rect_px.y + hb.dirty_rect_px.h).ceil().min(h as f32) as u32;
            if (x1 - x0) * (y1 - y0) < w * h {
                checked_partial_frames += 1;
            }

            // Assert ZERO byte differences outside `dirty_rect_px`!
            for y in 0..h {
                for x in 0..w {
                    if x >= x0 && x < x1 && y >= y0 && y < y1 {
                        continue;
                    }
                    let off = ((y * w + x) * 4) as usize;
                    assert_eq!(
                        &prev_buf[off..off + 4],
                        &cur_buf[off..off + 4],
                        "step {step_idx}: pixel ({x},{y}) changed outside dirty rect ({x0},{y0})..({x1},{y1})"
                    );
                }
            }
            prev_buf.copy_from_slice(cur_buf);
        }
        assert!(
            checked_partial_frames >= 30,
            "expected partial dirty rects on steady frames, got {checked_partial_frames}"
        );
    }

    #[test]
    fn export_12_golden_inspection_pngs() {
        let out_dir = std::env::temp_dir().join("desktop_cat_golden");
        let _ = std::fs::create_dir_all(&out_dir);

        let mut renderer = Renderer::new(540, 450);
        let presets = builtin_presets();
        let midnight = &presets[0];
        let luna = &presets[1];
        let mochi = &presets[2];
        let siamese = &presets[5];
        let sakura = &presets[14];

        // 1. Midnight active typing (action-following downward keyboard gaze, recessed pressed switch wells, tapered cel arms)
        let mut sim = PetSim::default();
        let cfg = Config {
            showcase_mode: true,
            show_floating_popups: true,
            ..Config::default()
        };
        for key in [PhysicalKey::F, PhysicalKey::J, PhysicalKey::D, PhysicalKey::K, PhysicalKey::Space] {
            sim.on_key_event(key, true, &cfg);
            sim.step(0.04);
        }
        renderer.render(
            midnight,
            &sim.snapshot(),
            RenderOptions {
                show_keycap_legends: true,
                ..RenderOptions::default()
            },
        );
        save_png(&out_dir.join("01_midnight_active_typing.png"), 540, 450, renderer.rgba_bytes());

        // 2. Midnight idle floor-rest posture (7.5 s -> front paws flat on desk behind Spacebar, wider tucked back paws)
        let mut rest_sim = PetSim::default();
        for _ in 0..220 {
            rest_sim.step(1.0 / 24.0);
        }
        renderer.render(midnight, &rest_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("02_midnight_idle_floor_rest.png"), 540, 450, renderer.rgba_bytes());

        // 3. Midnight unoccluded mouse Left Click (paw on upper palm dome, viewer-right LMB depressed + highlighted)
        let mut lmb_sim = PetSim::default();
        lmb_sim.on_mouse_input(-6, 4, Some(true), true, None, false, 0, Some(Vec2::new(-0.6, 0.35)));
        for _ in 0..5 {
            lmb_sim.step(1.0 / 60.0);
        }
        renderer.render(midnight, &lmb_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("03_midnight_mouse_lmb.png"), 540, 450, renderer.rgba_bytes());

        // 4. Midnight unoccluded mouse Right Click + Scroll wheel rolling tread notches
        let mut rmb_sim = PetSim::default();
        rmb_sim.on_mouse_input(4, -3, None, false, Some(true), true, 120, Some(Vec2::new(-0.5, 0.45)));
        for _ in 0..4 {
            rmb_sim.step(1.0 / 60.0);
        }
        renderer.render(midnight, &rmb_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("04_midnight_mouse_rmb_scroll.png"), 540, 450, renderer.rgba_bytes());

        // 5. Midnight Privacy Mode: fuller fluffy scalloped tail curtain covering 100% of keyboard + politely averted eyes!
        let mut priv_sim = PetSim::default();
        priv_sim.toggle_manual_privacy();
        for _ in 0..45 {
            priv_sim.step(1.0 / 30.0);
        }
        priv_sim.on_privacy_activity();
        for _ in 0..12 {
            priv_sim.step(1.0 / 30.0);
        }
        renderer.render(midnight, &priv_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("05_midnight_privacy_averted.png"), 540, 450, renderer.rgba_bytes());

        // 6. Midnight head-pat: `^ ^` eyes + luminous cheek blush + visual purr chest swell + relaxed ears
        let mut pat_sim = PetSim::default();
        let unlocked = Config::default();
        for i in 0..18 {
            pat_sim.on_head_hover_dip(true, if i % 2 == 0 { 10.0 } else { 44.0 }, &unlocked);
            pat_sim.step(0.025);
        }
        renderer.render(midnight, &pat_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("06_midnight_head_pat_purr.png"), 540, 450, renderer.rgba_bytes());

        // 7. Midnight pre-sleep yawn (~15.4 s idle) — warm rose yawn mouth clearly legible on black coat!
        let mut yawn_sim = PetSim::default();
        for _ in 0..370 {
            yawn_sim.step(1.0 / 24.0);
        }
        renderer.render(midnight, &yawn_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("07_midnight_yawn.png"), 540, 450, renderer.rgba_bytes());

        // 8. Midnight high-WPM focused typing (rolling WPM > 85 -> crisp focus brows + downward gaze)
        let mut wpm_sim = PetSim::default();
        let plain_cfg = Config {
            showcase_mode: true,
            ..Config::default()
        };
        let keys = [
            PhysicalKey::A, PhysicalKey::S, PhysicalKey::D, PhysicalKey::F,
            PhysicalKey::J, PhysicalKey::K, PhysicalKey::L, PhysicalKey::E,
            PhysicalKey::R, PhysicalKey::T, PhysicalKey::I, PhysicalKey::O,
        ];
        for i in 0..36 {
            let k = keys[i % keys.len()];
            wpm_sim.on_key_event(k, true, &plain_cfg);
            wpm_sim.step(0.055);
            wpm_sim.on_key_event(k, false, &plain_cfg);
            wpm_sim.step(0.025);
        }
        wpm_sim.on_key_event(PhysicalKey::G, true, &plain_cfg);
        wpm_sim.step(0.02);
        renderer.render(midnight, &wpm_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("08_midnight_high_wpm_focus.png"), 540, 450, renderer.rgba_bytes());

        // 9. Luna heterochromia tabby sleeping + floating Z particles
        let mut sleep_sim = PetSim::default();
        sleep_sim.set_pose_override(PoseOverride::Sleep);
        for _ in 0..85 {
            sleep_sim.step(0.04);
        }
        renderer.render(luna, &sleep_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("09_luna_sleep_z.png"), 540, 450, renderer.rgba_bytes());

        // 10. Mochi (pure white) Privacy Mode tail curtain contrast + averted eyes check
        renderer.render(mochi, &priv_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("10_mochi_privacy_contrast.png"), 540, 450, renderer.rgba_bytes());

        // 11. Siamese seal-point dark mask + cream body head-pat blush & `^ ^` eyes
        renderer.render(siamese, &pat_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("11_siamese_pat_blush.png"), 540, 450, renderer.rgba_bytes());

        // 12. Sakura at 0.6x Mini scale (216x180) with LMB click + scroll + head-pat blush visibility check
        let mut mini_renderer = Renderer::new(216, 180);
        mini_renderer.render(sakura, &lmb_sim.snapshot(), RenderOptions::default());
        save_png(&out_dir.join("12_sakura_mini_06x_mouse.png"), 216, 180, mini_renderer.rgba_bytes());
    }
}
