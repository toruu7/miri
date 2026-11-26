use crate::config::{Config, MIN_VISIBLE_OPACITY_INTERACTIVE};
use crate::keys::{
    anonymized_key_for_zone, lookup_keycap, PawSide, PhysicalKey, KEYCAP_COUNT,
};
use crate::rig::{keyboard_local_to_world, mouse_world_pos, Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PacingStage {
    Active60,
    Settling24,
    Drowsy12,
    Parked0,
}

impl PacingStage {
    pub fn target_fps(self, on_battery: bool) -> u32 {
        match self {
            Self::Active60 => {
                if on_battery {
                    30
                } else {
                    60
                }
            }
            Self::Settling24 => 24,
            Self::Drowsy12 => 12,
            Self::Parked0 => 0,
        }
    }

    pub fn period_ms(self, on_battery: bool) -> Option<f64> {
        let fps = self.target_fps(on_battery);
        if fps == 0 {
            None
        } else {
            Some(1000.0 / fps as f64)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacySource {
    None,
    Manual,
    CredentialHeuristic,
    ElevatedFocus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoseOverride {
    Auto,
    AwakeNeutral,
    Typing,
    Sleep,
    PrivacyCurl,
}

impl Default for PoseOverride {
    fn default() -> Self {
        Self::Auto
    }
}

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
    pub privacy_active: bool,
    pub privacy_source: PrivacySource,
    pub head_offset: Vec2,
    pub head_tilt_rad: f32,
    pub head_squash: f32,
    pub eye_gaze: Vec2,
    /// `0.0` = wide open, `0.62` = happy pat squint, `1.0` = tightly closed.
    pub eyelid_close: f32,
    pub happy_squint: f32,
    pub ear_relax: f32,
    pub breath_phase: f32,
    pub torso_bounce: f32,
    pub left_paw_world: Vec3,
    pub right_paw_world: Vec3,
    pub right_paw_on_mouse: f32,
    pub mouse_offset: Vec2,
    pub mouse_left_press: f32,
    pub mouse_right_press: f32,
    pub mouse_scroll_pulse: f32,
    /// Tail curl progression `[0.0, 1.0]`: `0` = relaxed side S-tail, `1` = curled over keyboard.
    pub tail_curl_t: f32,
    pub tail_sway_phase: f32,
    pub sleep_t: f32,
    pub patting_active: bool,
    pub key_press_amounts: [f32; KEYCAP_COUNT],
    pub key_glow_amounts: [f32; KEYCAP_COUNT],
    pub popups: Vec<FloatingPopup>,
    pub sleep_particles: Vec<SleepParticle>,
}

pub struct PetSim {
    time_sec: f32,
    idle_sec: f32,
    pacing: PacingStage,
    manual_privacy: bool,
    auto_privacy: PrivacySource,
    pose_override: PoseOverride,
    game_hidden: bool,
    session_locked: bool,
    display_off: bool,
    designer_connected: bool,

    // Deterministic salt for Zone-Anonymized typing
    anon_salt: u32,
    // Key states
    keys_held: [bool; KEYCAP_COUNT],
    key_hold_sec: [f32; KEYCAP_COUNT],
    key_press_amounts: [f32; KEYCAP_COUNT],
    key_glow_amounts: [f32; KEYCAP_COUNT],
    // Map physical key -> anonymized board key currently held (so key-up releases the exact visual key)
    anon_active_map: [Option<PhysicalKey>; KEYCAP_COUNT],

    // Paw targets & kinematics
    left_paw_target: Vec3,
    left_paw_pos: Vec3,
    left_paw_lift: f32,
    right_paw_kb_target: Vec3,
    right_paw_pos: Vec3,
    right_paw_lift: f32,
    right_paw_mouse_blend: f32,
    right_paw_kb_claim_sec: f32,

    // Mouse state
    mouse_offset: Vec2,
    mouse_left_held: bool,
    mouse_right_held: bool,
    mouse_left_press: f32,
    mouse_right_press: f32,
    mouse_scroll_pulse: f32,

    // Gaze, blink, head-pat & secondary motion
    target_gaze: Vec2,
    eye_gaze: Vec2,
    head_offset: Vec2,
    head_tilt_rad: f32,
    head_squash: f32,
    blink_timer: f32,
    blink_duration: f32,
    blink_elapsed: f32,
    ear_relax: f32,
    torso_bounce: f32,
    typing_energy: f32,

    // Hover-rub head pat tracker (`>= 120 dip` horizontal travel within `600 ms`)
    pat_rub_travel_dip: f32,
    pat_rub_window_sec: f32,
    pat_hold_sec: f32,
    last_hover_x_dip: Option<f32>,
    pat_active: bool,

    // Privacy curl & sleep states
    tail_curl_t: f32,
    sleep_t: f32,
    drowsy_z_spawn_accum: f32,

    popups: Vec<FloatingPopup>,
    sleep_particles: Vec<SleepParticle>,
    dirty: bool,
}

impl Default for PetSim {
    fn default() -> Self {
        let left_rest = keyboard_local_to_world(0.28, -0.10, 0.0);
        let right_mouse = mouse_world_pos(0.0, 0.0);
        Self {
            time_sec: 0.0,
            idle_sec: 0.0,
            pacing: PacingStage::Active60,
            manual_privacy: false,
            auto_privacy: PrivacySource::None,
            pose_override: PoseOverride::Auto,
            game_hidden: false,
            session_locked: false,
            display_off: false,
            designer_connected: false,
            anon_salt: 0x9E37_79B9,
            keys_held: [false; KEYCAP_COUNT],
            key_hold_sec: [0.0; KEYCAP_COUNT],
            key_press_amounts: [0.0; KEYCAP_COUNT],
            key_glow_amounts: [0.0; KEYCAP_COUNT],
            anon_active_map: [None; KEYCAP_COUNT],
            left_paw_target: left_rest,
            left_paw_pos: left_rest,
            left_paw_lift: 0.0,
            right_paw_kb_target: keyboard_local_to_world(-0.34, -0.10, 0.0),
            right_paw_pos: right_mouse,
            right_paw_lift: 0.0,
            right_paw_mouse_blend: 1.0,
            right_paw_kb_claim_sec: 0.0,
            mouse_offset: Vec2::default(),
            mouse_left_held: false,
            mouse_right_held: false,
            mouse_left_press: 0.0,
            mouse_right_press: 0.0,
            mouse_scroll_pulse: 0.0,
            target_gaze: Vec2::default(),
            eye_gaze: Vec2::default(),
            head_offset: Vec2::default(),
            head_tilt_rad: 0.0,
            head_squash: 0.0,
            blink_timer: 3.4,
            blink_duration: 0.14,
            blink_elapsed: -1.0,
            ear_relax: 0.0,
            torso_bounce: 0.0,
            typing_energy: 0.0,
            pat_rub_travel_dip: 0.0,
            pat_rub_window_sec: 0.0,
            pat_hold_sec: 0.0,
            last_hover_x_dip: None,
            pat_active: false,
            tail_curl_t: 0.0,
            sleep_t: 0.0,
            drowsy_z_spawn_accum: 0.0,
            popups: Vec::new(),
            sleep_particles: Vec::new(),
            dirty: true,
        }
    }
}

fn exp_smooth(cur: f32, target: f32, speed: f32, dt: f32) -> f32 {
    let alpha = 1.0 - (-speed * dt).exp();
    cur + (target - cur) * alpha
}

impl PetSim {
    pub fn pacing(&self) -> PacingStage {
        self.pacing
    }

    pub fn is_parked(&self) -> bool {
        self.pacing == PacingStage::Parked0
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn privacy_source(&self) -> PrivacySource {
        if self.manual_privacy {
            PrivacySource::Manual
        } else {
            self.auto_privacy
        }
    }

    pub fn is_privacy_active(&self) -> bool {
        self.privacy_source() != PrivacySource::None
    }

    pub fn manual_privacy(&self) -> bool {
        self.manual_privacy
    }

    pub fn toggle_manual_privacy(&mut self) -> bool {
        self.manual_privacy = !self.manual_privacy;
        if self.is_privacy_active() {
            self.on_privacy_entered();
        }
        self.wake_active();
        self.manual_privacy
    }

    pub fn set_auto_privacy(&mut self, src: PrivacySource) {
        if self.auto_privacy != src {
            self.auto_privacy = src;
            if self.is_privacy_active() {
                self.on_privacy_entered();
            }
            self.wake_active();
        }
    }

    pub fn set_pose_override(&mut self, pose: PoseOverride) {
        if self.pose_override != pose {
            self.pose_override = pose;
            self.wake_active();
        }
    }

    pub fn set_designer_connected(&mut self, connected: bool) {
        self.designer_connected = connected;
        if !connected {
            self.pose_override = PoseOverride::Auto;
        }
        self.wake_active();
    }

    pub fn set_game_hidden(&mut self, hidden: bool) {
        if self.game_hidden != hidden {
            self.game_hidden = hidden;
            if hidden {
                self.release_all_keys();
                self.popups.clear();
                self.pacing = PacingStage::Parked0;
            } else {
                self.wake_active();
            }
        }
    }

    pub fn set_session_locked(&mut self, locked: bool) {
        if self.session_locked != locked {
            self.session_locked = locked;
            if locked {
                self.release_all_keys();
                self.popups.clear();
                self.pacing = PacingStage::Parked0;
            } else {
                self.wake_active();
            }
        }
    }

    pub fn set_display_off(&mut self, off: bool) {
        if self.display_off != off {
            self.display_off = off;
            if off {
                self.release_all_keys();
                self.pacing = PacingStage::Parked0;
            } else {
                self.wake_active();
            }
        }
    }

    fn on_privacy_entered(&mut self) {
        // Hard privacy invariant: immediately purge in-flight floating pop-ups and clear held keys.
        self.popups.clear();
        self.release_all_keys();
        self.dirty = true;
    }

    pub fn release_all_keys(&mut self) {
        self.keys_held.fill(false);
        self.key_hold_sec.fill(0.0);
        self.anon_active_map.fill(None);
        self.mouse_left_held = false;
        self.mouse_right_held = false;
        self.dirty = true;
    }

    pub fn has_any_key_held(&self) -> bool {
        self.keys_held.iter().any(|&v| v)
    }

    /// Release keys held longer than `max_sec` (UIPI safety fallback when focus switches).
    pub fn release_stale_keys(&mut self, max_sec: f32) -> usize {
        let mut released = 0;
        for i in 0..KEYCAP_COUNT {
            if self.keys_held[i] && self.key_hold_sec[i] >= max_sec {
                self.keys_held[i] = false;
                self.key_hold_sec[i] = 0.0;
                self.anon_active_map[i] = None;
                released += 1;
            }
        }
        if released > 0 {
            self.dirty = true;
        }
        released
    }

    pub fn wake_active(&mut self) {
        self.idle_sec = 0.0;
        if !self.game_hidden && !self.session_locked && !self.display_off {
            self.pacing = PacingStage::Active60;
        }
        self.dirty = true;
    }

    /// Ingest a physical key down/up event. Enforces privacy-by-construction at ingress.
    pub fn on_key_event(&mut self, key: PhysicalKey, is_down: bool, cfg: &Config) {
        if self.game_hidden || self.session_locked || self.display_off {
            return;
        }
        // Privacy hard gate: drop key identity before any state mutation or pop-up queue!
        if self.is_privacy_active() {
            self.popups.clear();
            return;
        }

        let spec = lookup_keycap(key);
        let raw_idx = spec.key.index();

        let visual_key = if cfg.showcase_mode {
            spec.key
        } else if is_down {
            if let Some(existing) = self.anon_active_map[raw_idx] {
                existing
            } else {
                self.anon_salt = self.anon_salt.wrapping_mul(1664525).wrapping_add(1013904223);
                let chosen = anonymized_key_for_zone(spec.zone, self.anon_salt);
                self.anon_active_map[raw_idx] = Some(chosen);
                chosen
            }
        } else {
            self.anon_active_map[raw_idx].take().unwrap_or(spec.key)
        };

        let vis_spec = lookup_keycap(visual_key);
        let vis_idx = vis_spec.key.index();

        if is_down {
            let fresh_press = !self.keys_held[vis_idx];
            self.keys_held[vis_idx] = true;
            if fresh_press {
                self.key_hold_sec[vis_idx] = 0.0;
                self.key_press_amounts[vis_idx] = 1.0;
                self.key_glow_amounts[vis_idx] = 1.0;
                self.typing_energy = (self.typing_energy + 0.35).min(1.0);
                self.torso_bounce = (self.torso_bounce + 0.45).min(1.0);

                let target_3d = keyboard_local_to_world(vis_spec.kb_x, vis_spec.kb_y, 1.0);
                match vis_spec.paw_side {
                    PawSide::Left => {
                        self.left_paw_target = target_3d;
                        self.left_paw_lift = 1.0;
                    }
                    PawSide::Right => {
                        self.right_paw_kb_target = target_3d;
                        self.right_paw_lift = 1.0;
                        // Claim right paw off the mouse for rapid two-paw typing!
                        self.right_paw_kb_claim_sec = 0.65;
                    }
                }

                // Opt-in floating white keycap pop-ups ONLY in Showcase Mode when enabled
                if cfg.showcase_mode && cfg.show_floating_popups {
                    self.spawn_popup(vis_spec.label, vis_spec.kb_x, vis_spec.key == PhysicalKey::Space);
                }
            }
            self.wake_active();
        } else {
            self.keys_held[vis_idx] = false;
            self.key_hold_sec[vis_idx] = 0.0;
            self.dirty = true;
        }
    }

    fn spawn_popup(&mut self, label: &str, kb_x: f32, wide: bool) {
        if self.popups.len() >= 8 {
            self.popups.remove(0);
        }
        let jitter = ((self.anon_salt & 0xFF) as f32 / 255.0 - 0.5) * 18.0;
        self.anon_salt = self.anon_salt.wrapping_mul(1664525).wrapping_add(1013904223);
        let x = 24.0 + kb_x * 84.0 + jitter;
        self.popups.push(FloatingPopup {
            label: label.to_string(),
            x,
            y: 34.0,
            vx: jitter * 0.45,
            vy: -56.0,
            age: 0.0,
            ttl: 0.62,
            wide,
        });
    }

    /// Update mouse relative motion, buttons, scroll, and gaze vector `[-1, 1]` relative to overlay center.
    pub fn on_mouse_input(
        &mut self,
        dx: i32,
        dy: i32,
        left_btn: Option<bool>,
        right_btn: Option<bool>,
        wheel_delta: i16,
        gaze_norm: Option<Vec2>,
    ) {
        if self.game_hidden || self.session_locked || self.display_off {
            return;
        }
        if self.is_privacy_active() {
            return;
        }

        let moved = dx != 0 || dy != 0;
        let clicked = left_btn.is_some() || right_btn.is_some() || wheel_delta != 0;

        if moved {
            self.mouse_offset.x = (self.mouse_offset.x + dx as f32 * 0.0028).clamp(-1.0, 1.0);
            self.mouse_offset.y = (self.mouse_offset.y + dy as f32 * 0.0028).clamp(-1.0, 1.0);
            if dx.abs() + dy.abs() >= 3 {
                // Deliberate mouse move reclaims right paw onto mouse cleanly
                self.right_paw_kb_claim_sec = (self.right_paw_kb_claim_sec - 0.35).max(0.0);
            }
        }
        if let Some(down) = left_btn {
            self.mouse_left_held = down;
            if down {
                self.mouse_left_press = 1.0;
                self.right_paw_kb_claim_sec = 0.0;
            }
        }
        if let Some(down) = right_btn {
            self.mouse_right_held = down;
            if down {
                self.mouse_right_press = 1.0;
                self.right_paw_kb_claim_sec = 0.0;
            }
        }
        if wheel_delta != 0 {
            self.mouse_scroll_pulse = if wheel_delta > 0 { 1.0 } else { -1.0 };
            self.right_paw_kb_claim_sec = 0.0;
        }
        if let Some(g) = gaze_norm {
            self.target_gaze = Vec2::new(g.x.clamp(-1.0, 1.0), g.y.clamp(-1.0, 1.0));
        }

        if moved || clicked {
            self.wake_active();
        }
    }

    /// Hover-rub head patting (called ONLY when click-through is unlocked and no mouse button is held).
    pub fn on_head_hover_dip(&mut self, inside_head: bool, hover_x_dip: f32, cfg: &Config) {
        if cfg.effective_click_through() || self.is_privacy_active() || self.game_hidden {
            self.pat_active = false;
            self.last_hover_x_dip = None;
            return;
        }
        if !inside_head {
            self.last_hover_x_dip = None;
            if self.pat_hold_sec <= 0.0 {
                self.pat_active = false;
            }
            return;
        }

        if let Some(prev_x) = self.last_hover_x_dip {
            let delta = (hover_x_dip - prev_x).abs();
            if delta > 0.5 && delta < 80.0 {
                self.pat_rub_travel_dip += delta;
                self.pat_rub_window_sec = 0.60;
                if self.pat_rub_travel_dip >= 120.0 {
                    self.pat_active = true;
                    self.pat_hold_sec = 0.55;
                    self.head_squash = (self.head_squash + 0.35).min(1.0);
                    self.pat_rub_travel_dip = 60.0;
                    self.wake_active();
                }
            }
        }
        self.last_hover_x_dip = Some(hover_x_dip);
    }

    pub fn on_pointer_leave(&mut self) {
        self.last_hover_x_dip = None;
    }

    /// Advance simulation by `dt_raw` seconds (clamped to `<= 0.05 s` for spring stability).
    pub fn step(&mut self, dt_raw: f32) {
        let dt = dt_raw.clamp(0.0, 0.05);
        self.time_sec += dt;

        if self.game_hidden || self.session_locked || self.display_off {
            self.pacing = PacingStage::Parked0;
            return;
        }

        self.idle_sec += dt;

        // Track patting rub window decay
        if self.pat_rub_window_sec > 0.0 {
            self.pat_rub_window_sec = (self.pat_rub_window_sec - dt).max(0.0);
            if self.pat_rub_window_sec <= 0.0 {
                self.pat_rub_travel_dip = 0.0;
            }
        }
        if self.pat_hold_sec > 0.0 {
            self.pat_hold_sec = (self.pat_hold_sec - dt).max(0.0);
            if self.pat_hold_sec <= 0.0 {
                self.pat_active = false;
            }
        }

        // Effective visual pose (note: visual PoseOverride in designer NEVER disables privacy input suppression)
        let force_privacy_pose = self.is_privacy_active() || self.pose_override == PoseOverride::PrivacyCurl;
        let force_sleep_pose = self.pose_override == PoseOverride::Sleep;
        let force_typing_pose = self.pose_override == PoseOverride::Typing;

        if force_typing_pose {
            let beat = (self.time_sec * 6.5) as u32;
            let demo_left = keyboard_local_to_world(0.42 - ((beat % 3) as f32) * 0.22, -0.15, 1.0);
            let demo_right = keyboard_local_to_world(-0.42 + (((beat + 1) % 3) as f32) * 0.22, -0.15, 1.0);
            self.left_paw_target = demo_left;
            self.right_paw_kb_target = demo_right;
            self.right_paw_kb_claim_sec = 0.3;
            self.typing_energy = 0.85;
            self.idle_sec = 0.0;
        }

        // Update key holds & glow decay
        let mut keys_animating = false;
        for i in 0..KEYCAP_COUNT {
            if self.keys_held[i] {
                self.key_hold_sec[i] += dt;
                self.key_press_amounts[i] = exp_smooth(self.key_press_amounts[i], 1.0, 28.0, dt);
                self.key_glow_amounts[i] = exp_smooth(self.key_glow_amounts[i], 1.0, 22.0, dt);
                keys_animating = true;
            } else {
                if self.key_press_amounts[i] > 0.005 {
                    self.key_press_amounts[i] = exp_smooth(self.key_press_amounts[i], 0.0, 18.0, dt);
                    keys_animating = true;
                } else {
                    self.key_press_amounts[i] = 0.0;
                }
                if self.key_glow_amounts[i] > 0.005 {
                    self.key_glow_amounts[i] = exp_smooth(self.key_glow_amounts[i], 0.0, 7.5, dt);
                    keys_animating = true;
                } else {
                    self.key_glow_amounts[i] = 0.0;
                }
            }
        }

        // Update floating keycap pop-ups
        for p in &mut self.popups {
            p.age += dt;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.vy += 95.0 * dt; // subtle gravity arc
        }
        self.popups.retain(|p| p.age < p.ttl);

        // Right-paw ownership arbitration (keyboard vs mouse, ~120 ms smooth blend)
        if self.right_paw_kb_claim_sec > 0.0 {
            self.right_paw_kb_claim_sec = (self.right_paw_kb_claim_sec - dt).max(0.0);
        }
        let target_mouse_blend = if force_privacy_pose || force_sleep_pose {
            0.0
        } else if self.right_paw_kb_claim_sec > 0.0 {
            0.0
        } else {
            1.0
        };
        self.right_paw_mouse_blend = exp_smooth(self.right_paw_mouse_blend, target_mouse_blend, 14.0, dt);

        // Soft auto-centering of virtual mouse offset when idle
        if self.idle_sec > 2.0 {
            self.mouse_offset.x = exp_smooth(self.mouse_offset.x, 0.0, 2.5, dt);
            self.mouse_offset.y = exp_smooth(self.mouse_offset.y, 0.0, 2.5, dt);
        }
        self.mouse_left_press = exp_smooth(
            self.mouse_left_press,
            if self.mouse_left_held { 1.0 } else { 0.0 },
            20.0,
            dt,
        );
        self.mouse_right_press = exp_smooth(
            self.mouse_right_press,
            if self.mouse_right_held { 1.0 } else { 0.0 },
            20.0,
            dt,
        );
        self.mouse_scroll_pulse = exp_smooth(self.mouse_scroll_pulse, 0.0, 12.0, dt);

        // Paw smoothing & parabolic lift decay
        let left_rest = if force_privacy_pose || force_sleep_pose {
            Vec3::new(26.0, 28.0, -2.0)
        } else if self.idle_sec > 3.5 {
            keyboard_local_to_world(0.28, -0.10, 0.0)
        } else {
            self.left_paw_target
        };
        self.left_paw_pos.x = exp_smooth(self.left_paw_pos.x, left_rest.x, 22.0, dt);
        self.left_paw_pos.y = exp_smooth(self.left_paw_pos.y, left_rest.y, 22.0, dt);
        self.left_paw_pos.z = exp_smooth(self.left_paw_pos.z, left_rest.z, 22.0, dt);
        self.left_paw_lift = exp_smooth(self.left_paw_lift, 0.0, 16.0, dt);

        let mouse_pos = mouse_world_pos(self.mouse_offset.x, self.mouse_offset.y);
        let right_kb = if force_privacy_pose || force_sleep_pose {
            Vec3::new(-32.0, 28.0, -2.0)
        } else {
            self.right_paw_kb_target
        };
        let right_dest = Vec3::new(
            right_kb.x + (mouse_pos.x - right_kb.x) * self.right_paw_mouse_blend,
            right_kb.y + (mouse_pos.y - right_kb.y) * self.right_paw_mouse_blend,
            right_kb.z + (mouse_pos.z - right_kb.z) * self.right_paw_mouse_blend,
        );
        self.right_paw_pos.x = exp_smooth(self.right_paw_pos.x, right_dest.x, 22.0, dt);
        self.right_paw_pos.y = exp_smooth(self.right_paw_pos.y, right_dest.y, 22.0, dt);
        self.right_paw_pos.z = exp_smooth(self.right_paw_pos.z, right_dest.z, 22.0, dt);
        self.right_paw_lift = exp_smooth(self.right_paw_lift, 0.0, 16.0, dt);

        // Gaze, head tilt, pat squash, ears, torso bounce
        let gaze_goal = if force_privacy_pose || force_sleep_pose || self.idle_sec > 15.0 {
            Vec2::new(0.0, 0.25)
        } else {
            self.target_gaze
        };
        self.eye_gaze.x = exp_smooth(self.eye_gaze.x, gaze_goal.x, 12.0, dt);
        self.eye_gaze.y = exp_smooth(self.eye_gaze.y, gaze_goal.y, 12.0, dt);

        let squash_goal = if self.pat_active { 0.45 } else { 0.0 };
        self.head_squash = exp_smooth(self.head_squash, squash_goal, 10.0, dt);
        let ear_goal = if self.pat_active {
            0.75
        } else if force_privacy_pose {
            0.50
        } else if force_sleep_pose || self.idle_sec > 15.0 {
            0.35
        } else {
            0.0
        };
        self.ear_relax = exp_smooth(self.ear_relax, ear_goal, 8.0, dt);
        self.typing_energy = exp_smooth(self.typing_energy, 0.0, 4.0, dt);
        self.torso_bounce = exp_smooth(self.torso_bounce, 0.0, 14.0, dt);

        let head_x_goal = self.eye_gaze.x * 4.5;
        let head_y_goal = self.eye_gaze.y * 2.8 + self.head_squash * 5.0 - self.torso_bounce * 3.5;
        self.head_offset.x = exp_smooth(self.head_offset.x, head_x_goal, 9.0, dt);
        self.head_offset.y = exp_smooth(self.head_offset.y, head_y_goal, 11.0, dt);
        let tilt_goal = if self.pat_active {
            (self.time_sec * 5.0).sin() * 0.06
        } else {
            self.eye_gaze.x * 0.055
        };
        self.head_tilt_rad = exp_smooth(self.head_tilt_rad, tilt_goal, 9.0, dt);

        // Randomized blink
        if self.blink_elapsed >= 0.0 {
            self.blink_elapsed += dt;
            if self.blink_elapsed >= self.blink_duration {
                self.blink_elapsed = -1.0;
                self.anon_salt = self.anon_salt.wrapping_mul(1664525).wrapping_add(1013904223);
                self.blink_timer = 2.8 + ((self.anon_salt % 300) as f32 / 100.0);
            }
        } else {
            self.blink_timer -= dt;
            if self.blink_timer <= 0.0 {
                self.blink_elapsed = 0.0;
            }
        }

        // Tail curl (privacy) & sleep transition
        let tail_goal = if force_privacy_pose { 1.0 } else { 0.0 };
        self.tail_curl_t = exp_smooth(self.tail_curl_t, tail_goal, 9.0, dt);

        let in_drowsy = force_sleep_pose
            || (self.pose_override == PoseOverride::Auto
                && !self.designer_connected
                && !force_privacy_pose
                && self.idle_sec >= 15.0);
        let sleep_goal = if force_sleep_pose || (in_drowsy && self.idle_sec >= 20.0) {
            1.0
        } else {
            0.0
        };
        self.sleep_t = exp_smooth(self.sleep_t, sleep_goal, 3.5, dt);

        // Floating `z` particles ONLY during Drowsy wind-down (15s..58s) or explicit Sleep preview
        let should_spawn_z = force_sleep_pose || (in_drowsy && self.idle_sec >= 16.0 && self.idle_sec < 56.0);
        if should_spawn_z {
            self.drowsy_z_spawn_accum += dt;
            if self.drowsy_z_spawn_accum >= 1.15 && self.sleep_particles.len() < 4 {
                self.drowsy_z_spawn_accum = 0.0;
                self.sleep_particles.push(SleepParticle {
                    x: 42.0,
                    y: -78.0,
                    age: 0.0,
                    ttl: 2.6,
                    scale: 0.75,
                });
            }
        }
        for zp in &mut self.sleep_particles {
            zp.age += dt;
            zp.y -= 16.0 * dt;
            zp.x += (zp.age * 2.4).sin() * 9.0 * dt;
            zp.scale = 0.7 + (zp.age / zp.ttl) * 0.55;
        }
        self.sleep_particles.retain(|p| p.age < p.ttl);

        // 4-stage hysteresis FSM
        let fast_springs_active = keys_animating
            || !self.popups.is_empty()
            || self.pat_active
            || self.torso_bounce > 0.02
            || self.left_paw_lift > 0.02
            || self.right_paw_lift > 0.02
            || (self.tail_curl_t - tail_goal).abs() > 0.015;

        self.pacing = if self.designer_connected || self.pose_override != PoseOverride::Auto {
            if fast_springs_active || self.idle_sec < 1.5 || force_typing_pose {
                PacingStage::Active60
            } else {
                PacingStage::Settling24
            }
        } else if force_privacy_pose {
            if (self.tail_curl_t - 1.0).abs() > 0.015 || keys_animating {
                PacingStage::Active60
            } else {
                PacingStage::Parked0
            }
        } else if fast_springs_active || self.idle_sec < 1.5 {
            PacingStage::Active60
        } else if self.idle_sec < 15.0 {
            PacingStage::Settling24
        } else if self.idle_sec < 60.0 || !self.sleep_particles.is_empty() || (self.sleep_t - 1.0).abs() > 0.02 {
            PacingStage::Drowsy12
        } else {
            PacingStage::Parked0
        };

        self.dirty = true;
    }

    pub fn snapshot(&self) -> SimSnapshot {
        let force_privacy = self.is_privacy_active() || self.pose_override == PoseOverride::PrivacyCurl;
        let blink_close = if self.blink_elapsed >= 0.0 {
            let phase = self.blink_elapsed / self.blink_duration;
            (phase * std::f32::consts::PI).sin().clamp(0.0, 1.0)
        } else {
            0.0
        };
        let happy_squint = if self.pat_active && !force_privacy {
            (self.head_squash * 1.6).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let eyelid_close = if force_privacy {
            1.0
        } else if self.sleep_t > 0.05 {
            self.sleep_t.max(blink_close)
        } else if happy_squint > 0.05 {
            (happy_squint * 0.65).max(blink_close)
        } else {
            blink_close
        };

        let left_world = Vec3::new(
            self.left_paw_pos.x,
            self.left_paw_pos.y - self.left_paw_lift * 12.0,
            self.left_paw_pos.z,
        );
        let right_world = Vec3::new(
            self.right_paw_pos.x,
            self.right_paw_pos.y - self.right_paw_lift * 12.0,
            self.right_paw_pos.z,
        );

        SimSnapshot {
            time_sec: self.time_sec,
            pacing: self.pacing,
            privacy_active: force_privacy,
            privacy_source: self.privacy_source(),
            head_offset: self.head_offset,
            head_tilt_rad: self.head_tilt_rad,
            head_squash: self.head_squash,
            eye_gaze: self.eye_gaze,
            eyelid_close,
            happy_squint,
            ear_relax: self.ear_relax,
            breath_phase: self.time_sec * 1.9,
            torso_bounce: self.torso_bounce,
            left_paw_world: left_world,
            right_paw_world: right_world,
            right_paw_on_mouse: self.right_paw_mouse_blend,
            mouse_offset: self.mouse_offset,
            mouse_left_press: self.mouse_left_press,
            mouse_right_press: self.mouse_right_press,
            mouse_scroll_pulse: self.mouse_scroll_pulse,
            tail_curl_t: self.tail_curl_t,
            tail_sway_phase: self.time_sec * 2.1,
            sleep_t: self.sleep_t,
            patting_active: self.pat_active,
            key_press_amounts: self.key_press_amounts,
            key_glow_amounts: self.key_glow_amounts,
            popups: self.popups.clone(),
            sleep_particles: self.sleep_particles.clone(),
        }
    }
}

pub fn should_auto_lock_for_opacity(opacity: f32) -> bool {
    opacity < MIN_VISIBLE_OPACITY_INTERACTIVE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_mode_purges_popups_and_blocks_key_identity() {
        let mut sim = PetSim::default();
        let cfg = Config {
            showcase_mode: true,
            show_floating_popups: true,
            ..Config::default()
        };
        sim.on_key_event(PhysicalKey::A, true, &cfg);
        assert_eq!(sim.snapshot().popups.len(), 1);

        sim.toggle_manual_privacy();
        assert!(sim.is_privacy_active());
        assert!(sim.snapshot().popups.is_empty());

        // Subsequent keystroke while in privacy mode must not spawn popups or press keys
        sim.on_key_event(PhysicalKey::P, true, &cfg);
        assert!(sim.snapshot().popups.is_empty());
        assert!(!sim.has_any_key_held());
    }

    #[test]
    fn four_stage_pacing_reaches_parked_zero_fps() {
        let mut sim = PetSim::default();
        assert_eq!(sim.pacing(), PacingStage::Active60);

        for _ in 0..50 {
            sim.step(0.05);
        }
        assert_eq!(sim.pacing(), PacingStage::Settling24);

        for _ in 0..320 {
            sim.step(0.05);
        }
        assert_eq!(sim.pacing(), PacingStage::Drowsy12);

        for _ in 0..1000 {
            sim.step(0.05);
        }
        assert_eq!(sim.pacing(), PacingStage::Parked0);
        assert!(sim.snapshot().sleep_particles.is_empty());
    }

    #[test]
    fn click_through_lock_disables_head_patting() {
        let mut sim = PetSim::default();
        let locked_cfg = Config {
            click_through_locked: true,
            ..Config::default()
        };
        for i in 0..20 {
            let x = if i % 2 == 0 { 10.0 } else { 40.0 };
            sim.on_head_hover_dip(true, x, &locked_cfg);
        }
        assert!(!sim.snapshot().patting_active);

        let unlocked_cfg = Config::default();
        for i in 0..20 {
            let x = if i % 2 == 0 { 10.0 } else { 40.0 };
            sim.on_head_hover_dip(true, x, &unlocked_cfg);
        }
        assert!(sim.snapshot().patting_active);
    }
}
