use serde::{Deserialize, Serialize};

/// Canonical logical canvas dimensions (includes top 18% headroom for Showcase pop-ups & z particles).
pub const BASE_CANVAS_W: f32 = 360.0;
pub const BASE_CANVAS_H: f32 = 300.0;
pub const HEADROOM_RATIO: f32 = 0.18;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }

    pub fn dist(self, other: Self) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

/// Deterministic weak-perspective 2.5D projection used consistently by both `pet-core` and `pet-render`.
///
/// Origin `(0, 0, 0)` sits at the center of the cat's desk anchor.
/// `+x` goes right, `+y` goes down, `+z` goes toward the viewer.
pub fn project_25d(p: Vec3, canvas_w: f32, canvas_h: f32) -> Vec2 {
    let scale_x = canvas_w / BASE_CANVAS_W;
    let scale_y = canvas_h / BASE_CANVAS_H;
    let s = scale_x.min(scale_y);
    let persp = 1.0 / (1.0 - (p.z * 0.0014).clamp(-0.35, 0.35));
    let cx = canvas_w * 0.50;
    let cy = canvas_h * 0.56;
    Vec2::new(cx + p.x * persp * s, cy + p.y * persp * s)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct HitRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl HitRect {
    pub fn contains(self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

/// Exported by `pet-render` each frame in physical canvas pixels for accurate hover-rub patting.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameHitboxes {
    pub head_rect_px: HitRect,
    pub body_bounds_px: HitRect,
    pub dirty_rect_px: HitRect,
}

/// Map normalized keyboard coordinates `kb_x, kb_y in [-1, 1]` onto canonical 3D desk space.
/// Keyboard sits slightly right-of-center to balance the miniature mouse on the viewer's left.
pub fn keyboard_local_to_world(kb_x: f32, kb_y: f32, depressed: f32) -> Vec3 {
    // Desk keyboard center in canonical logical units (matching LittleOne screenshots)
    let center_x = 24.0;
    let center_y = 77.0;
    let half_w = 104.0;
    let half_h = 24.0;
    // Slight perspective tilt: top row (nearest cat, kb_y = -1) is slightly narrower than front row (kb_y = +1)
    let depth_z = kb_y * 28.0;
    let x = center_x + kb_x * half_w;
    let y = center_y + kb_y * half_h + depressed * 3.2;
    Vec3::new(x, y, depth_z)
}

/// Canonical 3D position of the miniature oval mouse (viewer's left = cat's right paw).
pub fn mouse_world_pos(offset_x: f32, offset_y: f32) -> Vec3 {
    Vec3::new(
        -113.0 + offset_x.clamp(-1.0, 1.0) * 11.0,
        74.0 + offset_y.clamp(-1.0, 1.0) * 8.0,
        4.0,
    )
}
