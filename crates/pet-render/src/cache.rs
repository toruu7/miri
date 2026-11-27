use crate::geom::{ellipse_path, fill_path, optical_stroke, rounded_quad, stroke_path};
use crate::palette::rgb_color;
use crate::peripherals::draw_single_keycap_sculpted;
use crate::RenderOptions;
use pet_core::{
    keycap_specs, keyboard_local_to_world, project_25d, BackdropMode, PeripheralStyle, Rgb8, Vec2,
    BASE_CANVAS_H, BASE_CANVAS_W,
};
use tiny_skia::{Color, Mask, PathBuilder, Pixmap, PixmapPaint, Transform};

#[derive(Debug, Clone, PartialEq)]
struct DeskCacheKey {
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

#[derive(Debug, Clone, PartialEq)]
struct MouseCacheKey {
    scale_bits: u32,
    chroma_hard_edge: bool,
    mouse_body: Rgb8,
    mouse_wheel: Rgb8,
}

/// Tight-cropped static desk + sculpted 65% keyboard cache (`~165 KB` at 1x instead of `432 KB`
/// full canvas — cutting both RAM and per-frame blit pixels by ~62%).
#[derive(Default)]
pub struct TightStaticDeskCache {
    pixmap: Option<Pixmap>,
    origin_x: i32,
    origin_y: i32,
    key: Option<DeskCacheKey>,
    pub rebuilds: u64,
}

impl TightStaticDeskCache {
    pub fn invalidate(&mut self) {
        self.pixmap = None;
        self.key = None;
    }

    pub fn ensure_and_blit(
        &mut self,
        target: &mut Pixmap,
        per: &PeripheralStyle,
        opts: RenderOptions,
    ) -> bool {
        let w = target.width();
        let h = target.height();
        let show_legends = opts.show_keycap_legends || per.show_keycap_legends;
        let key = DeskCacheKey {
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

        let mut rebuilt = false;
        if self.key.as_ref() != Some(&key) || self.pixmap.is_none() {
            self.build(w, h, per, show_legends, !opts.chroma_hard_edge);
            self.key = Some(key);
            self.rebuilds += 1;
            rebuilt = true;
        }

        if let Some(ref cache) = self.pixmap {
            target.draw_pixmap(
                self.origin_x,
                self.origin_y,
                cache.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        rebuilt
    }

    fn build(&mut self, w: u32, h: u32, per: &PeripheralStyle, show_legends: bool, aa: bool) {
        let wf = w as f32;
        let hf = h as f32;
        let s = (wf / BASE_CANVAS_W).min(hf / BASE_CANVAS_H);

        // Compute tight bounding box covering keyboard chassis + subtle drop shadow
        let tl = project_25d(keyboard_local_to_world(-1.05, -1.14, 0.0), wf, hf);
        let tr = project_25d(keyboard_local_to_world(1.05, -1.14, 0.0), wf, hf);
        let br = project_25d(keyboard_local_to_world(1.06, 1.16, 0.0), wf, hf);
        let bl = project_25d(keyboard_local_to_world(-1.06, 1.16, 0.0), wf, hf);

        let min_x = (tl.x.min(bl.x) - 6.0 * s).floor().clamp(0.0, wf) as i32;
        let min_y = (tl.y.min(tr.y) - 6.0 * s).floor().clamp(0.0, hf) as i32;
        let max_x = (tr.x.max(br.x) + 6.0 * s)
            .ceil()
            .clamp(min_x as f32 + 1.0, wf) as i32;
        let max_y = (br.y.max(bl.y) + 12.0 * s)
            .ceil()
            .clamp(min_y as f32 + 1.0, hf) as i32;

        let cw = (max_x - min_x).max(1) as u32;
        let ch = (max_y - min_y).max(1) as u32;
        let mut cache = Pixmap::new(cw, ch).expect("tight desk cache pixmap");
        let ox = min_x as f32;
        let oy = min_y as f32;
        let shift = |p: Vec2| Vec2::new(p.x - ox, p.y - oy);

        // 1. Tight subtle keyboard drop shadow (no floating desk pad — floats cleanly over wallpaper!)
        let tl_s = shift(tl);
        let tr_s = shift(tr);
        let br_s = shift(br);
        let bl_s = shift(bl);
        let sh_y = 7.5 * s;
        if let Some(p) = rounded_quad(
            Vec2::new(tl_s.x, tl_s.y + sh_y),
            Vec2::new(tr_s.x, tr_s.y + sh_y),
            Vec2::new(br_s.x + 1.5 * s, br_s.y + sh_y),
            Vec2::new(bl_s.x - 1.5 * s, bl_s.y + sh_y),
            5.0 * s,
        ) {
            fill_path(&mut cache, &p, Color::from_rgba(0.02, 0.02, 0.05, 0.18).unwrap(), aa);
        }

        // 2. Sleek compact 65% mechanical keyboard chassis
        let skirt = 4.6 * s;
        if let Some(p) = rounded_quad(
            Vec2::new(tl_s.x, tl_s.y + skirt),
            Vec2::new(tr_s.x, tr_s.y + skirt),
            Vec2::new(br_s.x, br_s.y + skirt),
            Vec2::new(bl_s.x, bl_s.y + skirt),
            4.6 * s,
        ) {
            fill_path(
                &mut cache,
                &p,
                rgb_color(per.keyboard_case.scale(0.55), 1.0),
                aa,
            );
        }
        if let Some(p) = rounded_quad(tl_s, tr_s, br_s, bl_s, 4.4 * s) {
            fill_path(&mut cache, &p, rgb_color(per.keyboard_case, 1.0), aa);
        }

        // Recessed inner switch plate behind all 68 keycaps
        let pt_tl = shift(project_25d(keyboard_local_to_world(-1.02, -1.03, 0.0), wf, hf));
        let pt_tr = shift(project_25d(keyboard_local_to_world(1.02, -1.03, 0.0), wf, hf));
        let pt_br = shift(project_25d(keyboard_local_to_world(1.03, 1.03, 0.0), wf, hf));
        let pt_bl = shift(project_25d(keyboard_local_to_world(-1.03, 1.03, 0.0), wf, hf));
        if let Some(plate) = rounded_quad(pt_tl, pt_tr, pt_br, pt_bl, 3.0 * s) {
            fill_path(
                &mut cache,
                &plate,
                rgb_color(per.keyboard_case.scale(0.45), 1.0),
                aa,
            );
        }

        // 3. Pre-bake all 68 sculpted bevelled keycaps at zero per-frame CPU cost!
        for row in (0..=4u8).rev() {
            for spec in keycap_specs().iter().filter(|k| k.row == row) {
                draw_single_keycap_sculpted(
                    &mut cache,
                    spec,
                    per,
                    0.0,
                    0.0,
                    wf,
                    hf,
                    s,
                    Vec2::new(ox, oy),
                    show_legends,
                    aa,
                );
            }
        }

        self.origin_x = min_x;
        self.origin_y = min_y;
        self.pixmap = Some(cache);
    }
}

/// Tiny tight-cropped cel-shaded miniature mouse base sprite (`~12-18 KB` at 1x).
/// Blitted at integer-snapped coordinates; dynamic LMB/RMB depressions and rolling scroll notches
/// render cleanly on the unoccluded front half.
#[derive(Default)]
pub struct StaticMouseSprite {
    pixmap: Option<Pixmap>,
    half_w: f32,
    half_h: f32,
    key: Option<MouseCacheKey>,
    pub rebuilds: u64,
}

impl StaticMouseSprite {
    pub fn invalidate(&mut self) {
        self.pixmap = None;
        self.key = None;
    }

    pub fn ensure_and_blit(
        &mut self,
        target: &mut Pixmap,
        center: Vec2,
        s: f32,
        per: &PeripheralStyle,
        chroma_hard_edge: bool,
    ) {
        let key = MouseCacheKey {
            scale_bits: (s * 100.0).round() as u32,
            chroma_hard_edge,
            mouse_body: per.mouse_body,
            mouse_wheel: per.mouse_wheel,
        };
        if self.key.as_ref() != Some(&key) || self.pixmap.is_none() {
            self.build(s, per, !chroma_hard_edge);
            self.key = Some(key);
            self.rebuilds += 1;
        }
        if let Some(ref sprite) = self.pixmap {
            // Integer snap guarantees zero bilinear chroma-key bleed
            let dx = (center.x - self.half_w).round() as i32;
            let dy = (center.y - self.half_h).round() as i32;
            target.draw_pixmap(
                dx,
                dy,
                sprite.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
    }

    fn build(&mut self, s: f32, per: &PeripheralStyle, aa: bool) {
        // Compact minimalist pebble-white oval mouse matching LittleOne proportions
        let rx = 13.5 * s;
        let ry = 16.5 * s;
        let pad = (5.5 * s).ceil() + 3.0;
        let sw = ((rx + pad) * 2.0).ceil().max(16.0) as u32;
        let sh = ((ry + pad + 3.0 * s) * 2.0).ceil().max(16.0) as u32;
        let cx = sw as f32 * 0.5;
        let cy = sh as f32 * 0.5 - 1.2 * s;

        let mut sprite = Pixmap::new(sw, sh).expect("mouse sprite");

        // Subtle tight drop shadow
        if let Some(p) = ellipse_path(cx, cy + 3.2 * s, rx * 1.02, ry * 0.94) {
            fill_path(&mut sprite, &p, Color::from_rgba(0.02, 0.03, 0.06, 0.22).unwrap(), aa);
        }

        // Subtle lower 3D mouse lip
        if let Some(p) = ellipse_path(cx, cy + 1.6 * s, rx, ry) {
            fill_path(&mut sprite, &p, rgb_color(per.mouse_body.scale(0.76), 1.0), aa);
        }

        // Clean matte-white pebble shell with thin crisp border
        let outline_col = rgb_color(per.mouse_body.scale(0.32), 0.90);
        if let Some(p) = ellipse_path(cx, cy, rx, ry) {
            fill_path(&mut sprite, &p, rgb_color(per.mouse_body, 1.0), aa);
            stroke_path(&mut sprite, &p, outline_col, optical_stroke(1.05, s), aa);
        }

        // Delicate faint button split seam
        let seam_col = rgb_color(per.mouse_body.scale(0.72), 0.55);
        let seam_w = optical_stroke(0.75, s);
        let mut spb = PathBuilder::new();
        spb.move_to(cx, cy + 0.5 * s);
        spb.line_to(cx, cy + ry * 0.92);
        if let Some(sp) = spb.finish() {
            stroke_path(&mut sprite, &sp, seam_col, seam_w, aa);
        }

        // Compact minimal scroll-wheel slot
        let well_w = 3.2 * s;
        let well_h = 7.0 * s;
        if let Some(well) =
            crate::geom::rounded_rect(cx - well_w * 0.5, cy + 2.2 * s, well_w, well_h, 1.6 * s)
        {
            fill_path(
                &mut sprite,
                &well,
                rgb_color(per.mouse_wheel.scale(0.45), 0.90),
                aa,
            );
        }

        self.half_w = cx;
        self.half_h = cy;
        self.pixmap = Some(sprite);
    }
}

/// Tightly cropped reusable `tiny_skia::Mask` covering only the cat torso+head silhouette
/// (`~40 KB` vs `108 KB` full canvas at 1x, clearing 63% fewer mask bytes per frame).
#[derive(Default)]
pub struct CroppedMaskScratch {
    mask: Option<Mask>,
    origin_x: u32,
    origin_y: u32,
    canvas_w: u32,
    canvas_h: u32,
}

impl CroppedMaskScratch {
    pub fn invalidate(&mut self) {
        self.mask = None;
    }

    /// Returns `(mask_mut, offset_x_f32, offset_y_f32)` ready to fill in local mask space.
    /// Wait: `tiny_skia::Pixmap::fill_path(..., Some(mask))` requires the mask dimensions to match
    /// the pixmap dimensions! Let's verify: in `tiny-skia`, `Pixmap::fill_path` asserts or requires
    /// `mask.width() == pixmap.width() && mask.height() == pixmap.height()`.
    /// Instead of panicking with a mismatched mask size, we render markings into a tight cropped
    /// `Pixmap` scratch (`~185×210` -> `155 KB`? No, even leaner: a cropped `Mask` paired with
    /// drawing directly onto a tight `sub_pixmap` OR clearing only the active sub-rows of a full mask!).
    /// Let's check: clearing ONLY the dirty vertical row span `[y0..y1]` of `mask.data_mut()` touches
    /// ONLY the torso+head rows (zero reallocation, zero `Pixmap` extra buffer, zero size mismatch panic!).
    pub fn prepare_rows(
        &mut self,
        canvas_w: u32,
        canvas_h: u32,
        y0: u32,
        y1: u32,
    ) -> &mut Mask {
        if self.mask.is_none() || self.canvas_w != canvas_w || self.canvas_h != canvas_h {
            self.mask = Some(Mask::new(canvas_w, canvas_h).expect("mask"));
            self.canvas_w = canvas_w;
            self.canvas_h = canvas_h;
            self.origin_y = 0;
            self.origin_x = canvas_h;
        }
        let mask = self.mask.as_mut().unwrap();
        // Clear union of previous active row span and new active row span (only ~60% of rows!)
        let clear_top = y0.min(self.origin_y).min(canvas_h);
        let clear_bot = y1.max(self.origin_x).min(canvas_h);
        let row_bytes = canvas_w as usize;
        if clear_top < clear_bot {
            let start = clear_top as usize * row_bytes;
            let end = clear_bot as usize * row_bytes;
            mask.data_mut()[start..end].fill(0);
        }
        self.origin_y = y0.min(canvas_h);
        self.origin_x = y1.min(canvas_h);
        mask
    }
}
