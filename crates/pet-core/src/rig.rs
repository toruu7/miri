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

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
            z: self.z + (other.z - self.z) * t,
        }
    }
}

/// Shared canonical 3D anchor constants consumed by both `pet-core` and `pet-render`
/// so simulation targets and drawn anatomy never drift out of sync.
pub struct RigAnchors;

impl RigAnchors {
    pub const TORSO_CENTER: Vec3 = Vec3::new(-4.0, 4.0, -8.0);
    pub const HEAD_BASE_CENTER: Vec3 = Vec3::new(-6.0, -56.0, -4.0);
    pub const LEFT_SHOULDER: Vec3 = Vec3::new(23.0, -12.0, 4.0);
    pub const RIGHT_SHOULDER: Vec3 = Vec3::new(-31.0, -12.0, 4.0);

    /// Tucked back paws staged slightly wider at the hips so front paws rest flat on the
    /// desk behind the Spacebar with zero geometric overlap.
    pub const BACK_PAW_LEFT: Vec3 = Vec3::new(38.0, 40.0, -4.0);
    pub const BACK_PAW_RIGHT: Vec3 = Vec3::new(-38.0, 40.0, -4.0);

    /// Calm idle floor-rest front paw positions (engaged after `7.5 s` of inactivity).
    pub const IDLE_FLOOR_PAW_LEFT: Vec3 = Vec3::new(15.0, 46.5, 4.0);
    pub const IDLE_FLOOR_PAW_RIGHT: Vec3 = Vec3::new(-21.0, 46.5, 4.0);

    /// Cozy sleep tucked front paws.
    pub const SLEEP_PAW_LEFT: Vec3 = Vec3::new(20.0, 38.0, 2.0);
    pub const SLEEP_PAW_RIGHT: Vec3 = Vec3::new(-26.0, 38.0, 2.0);

    /// Hidden behind-tail typing paw anchors during Privacy Mode.
    pub const PRIVACY_PAW_LEFT: Vec3 = Vec3::new(24.0, 56.0, 10.0);
    pub const PRIVACY_PAW_RIGHT: Vec3 = Vec3::new(-18.0, 56.0, 10.0);
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

    pub fn contains_expanded(self, px: f32, py: f32, pad: f32) -> bool {
        px >= self.x - pad
            && px <= self.x + self.w + pad
            && py >= self.y - pad
            && py <= self.y + self.h + pad
    }

    pub fn is_empty(self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.w).max(other.x + other.w);
        let y1 = (self.y + self.h).max(other.y + other.h);
        Self {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0.0),
            h: (y1 - y0).max(0.0),
        }
    }

    pub fn clamp_to_canvas(self, canvas_w: f32, canvas_h: f32) -> Self {
        let x0 = self.x.floor().clamp(0.0, canvas_w);
        let y0 = self.y.floor().clamp(0.0, canvas_h);
        let x1 = (self.x + self.w).ceil().clamp(x0, canvas_w);
        let y1 = (self.y + self.h).ceil().clamp(y0, canvas_h);
        Self {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0.0),
            h: (y1 - y0).max(0.0),
        }
    }
}

/// Exported by `pet-render` each frame in physical canvas pixels for accurate hover-rub patting
/// and minimal dirty-rect presentation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameHitboxes {
    pub head_rect_px: HitRect,
    pub body_bounds_px: HitRect,
    pub dirty_rect_px: HitRect,
}

/// Map normalized keyboard coordinates `kb_x, kb_y in [-1, 1]` onto canonical 3D desk space.
/// Keyboard sits slightly right-of-center to balance the miniature mouse on the viewer's left.
pub fn keyboard_local_to_world(kb_x: f32, kb_y: f32, depressed: f32) -> Vec3 {
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

/// Canonical 3D center of the miniature oval mouse (viewer's left = cat's right paw).
pub fn mouse_world_pos(offset_x: f32, offset_y: f32) -> Vec3 {
    Vec3::new(
        -113.0 + offset_x.clamp(-1.0, 1.0) * 11.0,
        74.0 + offset_y.clamp(-1.0, 1.0) * 8.0,
        4.0,
    )
}

/// Canonical 3D palm-rest anchor on the upper/back dome of the miniature mouse so the front
/// split LMB/RMB button caps and recessed scroll wheel stay unoccluded by the right paw.
pub fn mouse_paw_palm_world(offset_x: f32, offset_y: f32) -> Vec3 {
    let c = mouse_world_pos(offset_x, offset_y);
    Vec3::new(c.x + 2.5, c.y - 8.5, c.z + 2.0)
}
