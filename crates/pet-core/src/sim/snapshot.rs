use super::expression::ExpressionState;
use super::pacing::PacingStage;
use super::posture::PostureMode;
use super::privacy::PrivacySource;
use crate::keys::KEYCAP_COUNT;
use crate::rig::{Vec2, Vec3};

#[derive(Debug, Clone)]
pub struct FloatingPopup {
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub age: f32,
    pub ttl: f32,
    pub wide: bool,
}

#[derive(Debug, Clone)]
pub struct SleepParticle {
    pub x: f32,
    pub y: f32,
    pub age: f32,
    pub ttl: f32,
    pub scale: f32,
}

/// Immutable per-frame pose snapshot consumed by `pet-render`.
#[derive(Debug, Clone)]
pub struct SimSnapshot {
    pub time_sec: f32,
    pub pacing: PacingStage,
    pub posture: PostureMode,
    pub expression: ExpressionState,
    pub privacy_active: bool,
    pub privacy_source: PrivacySource,

    // Head & face dynamics
    pub head_offset: Vec2,
    pub head_tilt_rad: f32,
    pub head_squash: f32,
    pub eye_gaze: Vec2,
    /// `0.0` = wide open, `1.0` = tightly closed.
    pub eyelid_close: f32,
    pub happy_squint: f32,
    pub ear_relax: f32,
    pub ear_relax_l: f32,
    pub ear_relax_r: f32,
    pub ear_twitch_l: f32,
    pub ear_twitch_r: f32,
    pub focus_intensity: f32,
    pub yawn_t: f32,
    pub blush_amount: f32,
    pub purr_intensity: f32,
    pub purr_phase: f32,
    /// Head-local normalized scratch point `(u, v) in [-1, 1]^2` while head scratching is active.
    pub scratch_spot: Option<Vec2>,
    pub scratch_intensity: f32,

    // Torso, limbs & posture
    pub breath_phase: f32,
    pub torso_bounce: f32,
    pub floor_rest_t: f32,
    pub left_paw_world: Vec3,
    pub right_paw_world: Vec3,
    pub left_paw_strike: f32,
    pub right_paw_strike: f32,
    pub right_paw_on_mouse: f32,

    // Mouse dynamics
    pub mouse_offset: Vec2,
    pub mouse_left_press: f32,
    pub mouse_right_press: f32,
    pub mouse_scroll_pulse: f32,
    pub mouse_scroll_angle: f32,
    pub mouse_paw_roll: f32,

    // Tail & Privacy curtain dynamics
    /// Tail curl progression `[0.0, 1.0]`: `0` = relaxed side S-tail, `1` = full fluffy privacy curtain.
    pub tail_curl_t: f32,
    pub tail_sway_phase: f32,
    pub privacy_bob: f32,
    pub privacy_tail_ripple: f32,

    // Sleep & overlays
    pub sleep_t: f32,
    pub patting_active: bool,
    pub key_press_amounts: [f32; KEYCAP_COUNT],
    pub key_glow_amounts: [f32; KEYCAP_COUNT],
    pub popups: Vec<FloatingPopup>,
    pub sleep_particles: Vec<SleepParticle>,
}
