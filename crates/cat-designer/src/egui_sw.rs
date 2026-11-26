use egui::epaint::{ClippedPrimitive, ImageData, Mesh, Primitive, TextureId};
use egui::TexturesDelta;
use std::collections::HashMap;

struct SwTexture {
    width: usize,
    height: usize,
    /// Premultiplied RGBA8 `[r, g, b, a]`
    rgba: Vec<u8>,
}

/// Pure-Rust CPU software rasterizer for `egui` tessellated meshes into a 32-bit BGRA framebuffer.
/// Eliminates all GPU/OpenGL/DX12/WebView2 dependencies from `cat-designer.exe`.
pub struct EguiSoftwareRasterizer {
    textures: HashMap<TextureId, SwTexture>,
    width: usize,
    height: usize,
    /// Top-down 32-bit BGRA framebuffer ready for `StretchDIBits`.
    bgra: Vec<u8>,
}

impl EguiSoftwareRasterizer {
    pub fn new(width: usize, height: usize) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        Self {
            textures: HashMap::new(),
            width: w,
            height: h,
            bgra: vec![0u8; w * h * 4],
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        let w = width.max(1);
        let h = height.max(1);
        if self.width != w || self.height != h {
            self.width = w;
            self.height = h;
            self.bgra.resize(w * h * 4, 0);
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn bgra_bytes(&self) -> &[u8] {
        &self.bgra
    }

    pub fn apply_textures_delta(&mut self, delta: &TexturesDelta) {
        for (id, img_delta) in &delta.set {
            let (tw, th, src_rgba) = image_data_to_premul_rgba(&img_delta.image);
            if let Some([px, py]) = img_delta.pos {
                if let Some(tex) = self.textures.get_mut(id) {
                    for y in 0..th {
                        let dy = py + y;
                        if dy >= tex.height {
                            continue;
                        }
                        for x in 0..tw {
                            let dx = px + x;
                            if dx >= tex.width {
                                continue;
                            }
                            let si = (y * tw + x) * 4;
                            let di = (dy * tex.width + dx) * 4;
                            tex.rgba[di..di + 4].copy_from_slice(&src_rgba[si..si + 4]);
                        }
                    }
                }
            } else {
                self.textures.insert(
                    *id,
                    SwTexture {
                        width: tw,
                        height: th,
                        rgba: src_rgba,
                    },
                );
            }
        }
        for id in &delta.free {
            self.textures.remove(id);
        }
    }

    pub fn clear(&mut self, r: u8, g: u8, b: u8) {
        for px in self.bgra.chunks_exact_mut(4) {
            px[0] = b;
            px[1] = g;
            px[2] = r;
            px[3] = 255;
        }
    }

    pub fn paint(&mut self, primitives: &[ClippedPrimitive], pixels_per_point: f32) {
        self.clear(24, 26, 32);
        let w = self.width as i32;
        let h = self.height as i32;

        for cp in primitives {
            let clip_min_x = (cp.clip_rect.min.x * pixels_per_point).floor().max(0.0) as i32;
            let clip_min_y = (cp.clip_rect.min.y * pixels_per_point).floor().max(0.0) as i32;
            let clip_max_x = (cp.clip_rect.max.x * pixels_per_point).ceil().min(w as f32) as i32;
            let clip_max_y = (cp.clip_rect.max.y * pixels_per_point).ceil().min(h as f32) as i32;
            if clip_min_x >= clip_max_x || clip_min_y >= clip_max_y {
                continue;
            }
            if let Primitive::Mesh(mesh) = &cp.primitive {
                self.raster_mesh(
                    mesh,
                    pixels_per_point,
                    clip_min_x,
                    clip_min_y,
                    clip_max_x,
                    clip_max_y,
                );
            }
        }
    }

    fn raster_mesh(
        &mut self,
        mesh: &Mesh,
        ppp: f32,
        cmin_x: i32,
        cmin_y: i32,
        cmax_x: i32,
        cmax_y: i32,
    ) {
        let tex = self.textures.get(&mesh.texture_id);
        let fb_w = self.width;

        for tri in mesh.indices.chunks_exact(3) {
            let v0 = mesh.vertices[tri[0] as usize];
            let v1 = mesh.vertices[tri[1] as usize];
            let v2 = mesh.vertices[tri[2] as usize];

            let x0 = v0.pos.x * ppp;
            let y0 = v0.pos.y * ppp;
            let x1 = v1.pos.x * ppp;
            let y1 = v1.pos.y * ppp;
            let x2 = v2.pos.x * ppp;
            let y2 = v2.pos.y * ppp;

            let min_x = x0.min(x1).min(x2).floor().max(cmin_x as f32) as i32;
            let max_x = x0.max(x1).max(x2).ceil().min(cmax_x as f32) as i32;
            let min_y = y0.min(y1).min(y2).floor().max(cmin_y as f32) as i32;
            let max_y = y0.max(y1).max(y2).ceil().min(cmax_y as f32) as i32;
            if min_x >= max_x || min_y >= max_y {
                continue;
            }

            let area = edge(x0, y0, x1, y1, x2, y2);
            if area.abs() < 1e-4 {
                continue;
            }
            let inv_area = 1.0 / area;

            let flat_color = v0.color == v1.color && v1.color == v2.color;
            let flat_uv = v0.uv == v1.uv && v1.uv == v2.uv;

            // Fast path: axis-aligned or untextured flat colored triangle (buttons, panels, backgrounds)
            if flat_color && flat_uv {
                let (sr, sg, sb, sa) = sample_flat(tex, v0.uv.x, v0.uv.y, v0.color);
                if sa == 0 {
                    continue;
                }
                for py in min_y..max_y {
                    let fy = py as f32 + 0.5;
                    let row = (py as usize) * fb_w;
                    for px in min_x..max_x {
                        let fx = px as f32 + 0.5;
                        let w0 = edge(x1, y1, x2, y2, fx, fy) * inv_area;
                        if w0 < -1e-5 {
                            continue;
                        }
                        let w1 = edge(x2, y2, x0, y0, fx, fy) * inv_area;
                        if w1 < -1e-5 {
                            continue;
                        }
                        let w2 = 1.0 - w0 - w1;
                        if w2 < -1e-5 {
                            continue;
                        }
                        blend_bgra_premul(&mut self.bgra[(row + px as usize) * 4..], sr, sg, sb, sa);
                    }
                }
                continue;
            }

            for py in min_y..max_y {
                let fy = py as f32 + 0.5;
                let row = (py as usize) * fb_w;
                for px in min_x..max_x {
                    let fx = px as f32 + 0.5;
                    let w0 = edge(x1, y1, x2, y2, fx, fy) * inv_area;
                    if w0 < -1e-5 {
                        continue;
                    }
                    let w1 = edge(x2, y2, x0, y0, fx, fy) * inv_area;
                    if w1 < -1e-5 {
                        continue;
                    }
                    let w2 = 1.0 - w0 - w1;
                    if w2 < -1e-5 {
                        continue;
                    }

                    let vr = v0.color.r() as f32 * w0 + v1.color.r() as f32 * w1 + v2.color.r() as f32 * w2;
                    let vg = v0.color.g() as f32 * w0 + v1.color.g() as f32 * w1 + v2.color.g() as f32 * w2;
                    let vb = v0.color.b() as f32 * w0 + v1.color.b() as f32 * w1 + v2.color.b() as f32 * w2;
                    let va = v0.color.a() as f32 * w0 + v1.color.a() as f32 * w1 + v2.color.a() as f32 * w2;

                    let (tr, tg, tb, ta) = if let Some(t) = tex {
                        let u = (v0.uv.x * w0 + v1.uv.x * w1 + v2.uv.x * w2).clamp(0.0, 1.0);
                        let v = (v0.uv.y * w0 + v1.uv.y * w1 + v2.uv.y * w2).clamp(0.0, 1.0);
                        sample_tex_bilinear(t, u, v)
                    } else {
                        (255, 255, 255, 255)
                    };

                    let sr = ((vr * tr as f32) / 255.0).round().clamp(0.0, 255.0) as u8;
                    let sg = ((vg * tg as f32) / 255.0).round().clamp(0.0, 255.0) as u8;
                    let sb = ((vb * tb as f32) / 255.0).round().clamp(0.0, 255.0) as u8;
                    let sa = ((va * ta as f32) / 255.0).round().clamp(0.0, 255.0) as u8;
                    if sa > 0 {
                        blend_bgra_premul(&mut self.bgra[(row + px as usize) * 4..], sr, sg, sb, sa);
                    }
                }
            }
        }
    }
}

#[inline(always)]
fn edge(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    (cx - ax) * (by - ay) - (cy - ay) * (bx - ax)
}

#[inline(always)]
fn sample_flat(tex: Option<&SwTexture>, u: f32, v: f32, col: egui::Color32) -> (u8, u8, u8, u8) {
    let (tr, tg, tb, ta) = if let Some(t) = tex {
        sample_tex_nearest(t, u.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
    } else {
        (255, 255, 255, 255)
    };
    (
        ((col.r() as u32 * tr as u32 + 127) / 255) as u8,
        ((col.g() as u32 * tg as u32 + 127) / 255) as u8,
        ((col.b() as u32 * tb as u32 + 127) / 255) as u8,
        ((col.a() as u32 * ta as u32 + 127) / 255) as u8,
    )
}

#[inline(always)]
fn sample_tex_nearest(t: &SwTexture, u: f32, v: f32) -> (u8, u8, u8, u8) {
    let x = ((u * (t.width.saturating_sub(1)) as f32).round() as usize).min(t.width - 1);
    let y = ((v * (t.height.saturating_sub(1)) as f32).round() as usize).min(t.height - 1);
    let i = (y * t.width + x) * 4;
    (t.rgba[i], t.rgba[i + 1], t.rgba[i + 2], t.rgba[i + 3])
}

#[inline(always)]
fn sample_tex_bilinear(t: &SwTexture, u: f32, v: f32) -> (u8, u8, u8, u8) {
    let fx = u * (t.width.saturating_sub(1)) as f32;
    let fy = v * (t.height.saturating_sub(1)) as f32;
    let x0 = (fx.floor() as usize).min(t.width - 1);
    let y0 = (fy.floor() as usize).min(t.height - 1);
    let x1 = (x0 + 1).min(t.width - 1);
    let y1 = (y0 + 1).min(t.height - 1);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;

    let i00 = (y0 * t.width + x0) * 4;
    let i10 = (y0 * t.width + x1) * 4;
    let i01 = (y1 * t.width + x0) * 4;
    let i11 = (y1 * t.width + x1) * 4;

    let w00 = (1.0 - tx) * (1.0 - ty);
    let w10 = tx * (1.0 - ty);
    let w01 = (1.0 - tx) * ty;
    let w11 = tx * ty;

    let mix = |c: usize| -> u8 {
        (t.rgba[i00 + c] as f32 * w00
            + t.rgba[i10 + c] as f32 * w10
            + t.rgba[i01 + c] as f32 * w01
            + t.rgba[i11 + c] as f32 * w11)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (mix(0), mix(1), mix(2), mix(3))
}

#[inline(always)]
fn blend_bgra_premul(dst: &mut [u8], sr: u8, sg: u8, sb: u8, sa: u8) {
    if sa == 255 {
        dst[0] = sb;
        dst[1] = sg;
        dst[2] = sr;
        dst[3] = 255;
        return;
    }
    let inv = 255 - sa as u32;
    dst[0] = (sb as u32 + (dst[0] as u32 * inv + 127) / 255).min(255) as u8;
    dst[1] = (sg as u32 + (dst[1] as u32 * inv + 127) / 255).min(255) as u8;
    dst[2] = (sr as u32 + (dst[2] as u32 * inv + 127) / 255).min(255) as u8;
    dst[3] = 255;
}

fn image_data_to_premul_rgba(img: &ImageData) -> (usize, usize, Vec<u8>) {
    match img {
        ImageData::Color(c) => {
            let mut out = Vec::with_capacity(c.pixels.len() * 4);
            for p in &c.pixels {
                out.extend_from_slice(&[p.r(), p.g(), p.b(), p.a()]);
            }
            (c.size[0], c.size[1], out)
        }
        ImageData::Font(f) => {
            let mut out = Vec::with_capacity(f.pixels.len() * 4);
            for p in f.srgba_pixels(None) {
                out.extend_from_slice(&[p.r(), p.g(), p.b(), p.a()]);
            }
            (f.size[0], f.size[1], out)
        }
    }
}
