use pet_core::{HitRect, Vec2};
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, Pixmap, Stroke, Transform,
};

/// Unified affine head transform applied identically to head silhouette, ears,
/// clipped coat markings, cel shading, and every facial feature (eliminating tilt/squash shear).
#[derive(Debug, Clone, Copy)]
pub struct HeadRigTransform {
    pub center: Vec2,
    pub tilt_rad: f32,
    pub sx: f32,
    pub sy: f32,
}

impl HeadRigTransform {
    pub fn new(center: Vec2, tilt_rad: f32, squash: f32) -> Self {
        // Volume-preserving cartoon squash & stretch
        let sx = 1.0 + squash * 0.09;
        let sy = 1.0 - squash * 0.085;
        Self {
            center,
            tilt_rad,
            sx,
            sy,
        }
    }

    /// `tiny_skia::Transform::from_rotate` expects **degrees** (not radians).
    pub fn to_transform(self) -> Transform {
        Transform::from_scale(self.sx, self.sy)
            .post_rotate(self.tilt_rad.to_degrees())
            .post_translate(self.center.x, self.center.y)
    }

    pub fn map_point(self, lx: f32, ly: f32) -> Vec2 {
        let x = lx * self.sx;
        let y = ly * self.sy;
        let (sin_t, cos_t) = self.tilt_rad.sin_cos();
        Vec2::new(
            self.center.x + x * cos_t - y * sin_t,
            self.center.y + x * sin_t + y * cos_t,
        )
    }
}

/// Optical stroke width helper enforcing a `>= 1.25 px` legibility floor for fine details
/// at `0.6x` Mini scale (`s < 0.85`).
#[inline]
pub fn optical_stroke(logical_w: f32, s: f32) -> f32 {
    let raw = logical_w * s;
    if s < 0.85 {
        raw.max(1.25)
    } else {
        raw.max(0.85)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BBoxAcc {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    valid: bool,
}

impl Default for BBoxAcc {
    fn default() -> Self {
        Self {
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
            valid: false,
        }
    }
}

impl BBoxAcc {
    pub fn include_point(&mut self, p: Vec2) {
        if !p.x.is_finite() || !p.y.is_finite() {
            return;
        }
        self.min_x = self.min_x.min(p.x);
        self.min_y = self.min_y.min(p.y);
        self.max_x = self.max_x.max(p.x);
        self.max_y = self.max_y.max(p.y);
        self.valid = true;
    }

    pub fn include_circle(&mut self, c: Vec2, r: f32) {
        self.include_point(Vec2::new(c.x - r, c.y - r));
        self.include_point(Vec2::new(c.x + r, c.y + r));
    }

    pub fn include_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.include_point(Vec2::new(x, y));
        self.include_point(Vec2::new(x + w, y + h));
    }

    pub fn to_hit_rect(self, pad: f32, canvas_w: f32, canvas_h: f32) -> HitRect {
        if !self.valid {
            return HitRect::default();
        }
        HitRect {
            x: self.min_x - pad,
            y: self.min_y - pad,
            w: (self.max_x - self.min_x) + pad * 2.0,
            h: (self.max_y - self.min_y) + pad * 2.0,
        }
        .clamp_to_canvas(canvas_w, canvas_h)
    }
}

pub fn fill_path(pixmap: &mut Pixmap, path: &Path, color: Color, aa: bool) {
    fill_path_xf(pixmap, path, color, Transform::identity(), None, aa);
}

pub fn fill_path_xf(
    pixmap: &mut Pixmap,
    path: &Path,
    color: Color,
    xf: Transform,
    mask: Option<&Mask>,
    aa: bool,
) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = aa;
    pixmap.fill_path(path, &paint, FillRule::Winding, xf, mask);
}

pub fn stroke_path(pixmap: &mut Pixmap, path: &Path, color: Color, width: f32, aa: bool) {
    stroke_path_xf(pixmap, path, color, width, Transform::identity(), None, aa);
}

pub fn stroke_path_xf(
    pixmap: &mut Pixmap,
    path: &Path,
    color: Color,
    width: f32,
    xf: Transform,
    mask: Option<&Mask>,
    aa: bool,
) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = aa;
    let stroke = Stroke {
        width: width.max(0.5),
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    pixmap.stroke_path(path, &paint, &stroke, xf, mask);
}

pub fn ellipse_path(cx: f32, cy: f32, rx: f32, ry: f32) -> Option<Path> {
    if rx <= 0.05 || ry <= 0.05 {
        return None;
    }
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

pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    if w <= 0.1 || h <= 0.1 {
        return None;
    }
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
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

pub fn rounded_quad(tl: Vec2, tr: Vec2, br: Vec2, bl: Vec2, r: f32) -> Option<Path> {
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

#[inline]
pub fn eval_quad(p0: Vec2, p1: Vec2, p2: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    Vec2::new(
        u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x,
        u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y,
    )
}

#[inline]
pub fn eval_cubic(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    let u2 = u * u;
    let t2 = t * t;
    Vec2::new(
        u2 * u * p0.x + 3.0 * u2 * t * p1.x + 3.0 * u * t2 * p2.x + t2 * t * p3.x,
        u2 * u * p0.y + 3.0 * u2 * t * p1.y + 3.0 * u * t2 * p2.y + t2 * t * p3.y,
    )
}

pub struct TaperedRibbon {
    pub body: Option<Path>,
    pub left_edge: Option<Path>,
    pub right_edge: Option<Path>,
    pub shadow_strip: Option<Path>,
}

/// Build a closed filled tapered organic ribbon polygon (replacing expensive thick `stroke_path`
/// curve offsets) plus open side-only contour strokes starting at `edge_start_idx` so limb bases
/// blend into the torso with zero shoulder-socket seam.
pub fn build_tapered_ribbon(
    spine: &[Vec2],
    half_widths: &[f32],
    left_edge_start: usize,
    right_edge_start: usize,
    shadow_on_right: bool,
) -> TaperedRibbon {
    let n = spine.len().min(half_widths.len());
    if n < 2 {
        return TaperedRibbon {
            body: None,
            left_edge: None,
            right_edge: None,
            shadow_strip: None,
        };
    }

    let mut left_pts = [Vec2::default(); 24];
    let mut right_pts = [Vec2::default(); 24];
    let mut mid_shadow_pts = [Vec2::default(); 24];
    let count = n.min(24);

    for i in 0..count {
        let prev = spine[i.saturating_sub(1)];
        let next = spine[(i + 1).min(count - 1)];
        let dx = next.x - prev.x;
        let dy = next.y - prev.y;
        let len = (dx * dx + dy * dy).sqrt().max(1e-4);
        let nx = -dy / len;
        let ny = dx / len;
        let hw = half_widths[i];
        left_pts[i] = Vec2::new(spine[i].x + nx * hw, spine[i].y + ny * hw);
        right_pts[i] = Vec2::new(spine[i].x - nx * hw, spine[i].y - ny * hw);
        // Taper cel shadow thickness smoothly from 0 at root so there is zero sharp horizontal seam!
        let t = i as f32 / (count - 1) as f32;
        let sh_frac = if shadow_on_right {
            -(1.0 - 0.68 * t)
        } else {
            1.0 - 0.68 * t
        };
        mid_shadow_pts[i] = Vec2::new(spine[i].x + nx * hw * sh_frac, spine[i].y + ny * hw * sh_frac);
    }

    // Closed body polygon with smooth semicircular rounded tip cap
    let mut pb = PathBuilder::new();
    pb.move_to(left_pts[0].x, left_pts[0].y);
    for p in &left_pts[1..count] {
        pb.line_to(p.x, p.y);
    }
    let end = spine[count - 1];
    let prev_end = spine[count - 2];
    let tdx = end.x - prev_end.x;
    let tdy = end.y - prev_end.y;
    let tlen = (tdx * tdx + tdy * tdy).sqrt().max(1e-4);
    let tx = tdx / tlen;
    let ty = tdy / tlen;
    let r_tip = half_widths[count - 1];
    let tip_mid = Vec2::new(end.x + tx * r_tip * 1.05, end.y + ty * r_tip * 1.05);
    let c1 = Vec2::new(
        left_pts[count - 1].x + tx * r_tip * 0.62,
        left_pts[count - 1].y + ty * r_tip * 0.62,
    );
    let c2 = Vec2::new(
        right_pts[count - 1].x + tx * r_tip * 0.62,
        right_pts[count - 1].y + ty * r_tip * 0.62,
    );
    pb.quad_to(c1.x, c1.y, tip_mid.x, tip_mid.y);
    pb.quad_to(c2.x, c2.y, right_pts[count - 1].x, right_pts[count - 1].y);
    for i in (0..count - 1).rev() {
        pb.line_to(right_pts[i].x, right_pts[i].y);
    }
    pb.close();
    let body = pb.finish();

    // Open side-only edge strokes (outer edge can start at shoulder, inner starts cleanly below chest!)
    let les = left_edge_start.min(count - 2);
    let mut lpb = PathBuilder::new();
    lpb.move_to(left_pts[les].x, left_pts[les].y);
    for p in &left_pts[les + 1..count] {
        lpb.line_to(p.x, p.y);
    }
    let left_edge = lpb.finish();

    let res = right_edge_start.min(count - 2);
    let mut rpb = PathBuilder::new();
    rpb.move_to(right_pts[res].x, right_pts[res].y);
    for p in &right_pts[res + 1..count] {
        rpb.line_to(p.x, p.y);
    }
    let right_edge = rpb.finish();

    // Smooth tapered cel-shading strip (zero abrupt cutoff at shoulder!)
    let mut spb = PathBuilder::new();
    if shadow_on_right {
        spb.move_to(right_pts[0].x, right_pts[0].y);
        for p in &mid_shadow_pts[1..count] {
            spb.line_to(p.x, p.y);
        }
        spb.line_to(right_pts[count - 1].x, right_pts[count - 1].y);
        for i in (0..count - 1).rev() {
            spb.line_to(right_pts[i].x, right_pts[i].y);
        }
    } else {
        spb.move_to(left_pts[0].x, left_pts[0].y);
        for p in &left_pts[1..count] {
            spb.line_to(p.x, p.y);
        }
        for i in (1..count).rev() {
            spb.line_to(mid_shadow_pts[i].x, mid_shadow_pts[i].y);
        }
    }
    spb.close();
    let shadow_strip = spb.finish();

    TaperedRibbon {
        body,
        left_edge,
        right_edge,
        shadow_strip,
    }
}
