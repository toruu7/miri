use super::spring::{exp_smooth, Spring1D};
use crate::rig::Vec2;

/// Inactivity threshold before front paws descend onto the desk floor behind the keyboard.
/// Chosen at `7.5 s` (`~7-8 s`) so normal reading/thinking pauses between typing bursts
/// stay calm without constant paw sweeps, while settling cleanly well before the `14.5 s` yawn.
pub const IDLE_FLOOR_REST_SEC: f32 = 7.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostureMode {
    ActiveDesk,
    IdleFloorRest,
    Sleeping,
    PrivacyCovered,
}

/// Manages orthogonal desk/floor/sleep/privacy paw posture and miniature mouse click/scroll physics.
#[derive(Debug, Clone)]
pub struct PostureController {
    pub mode: PostureMode,
    pub floor_rest: Spring1D,
    pub right_paw_mouse_blend: Spring1D,
    pub right_paw_kb_claim_sec: f32,

    pub mouse_offset: Vec2,
    pub mouse_left_held: bool,
    pub mouse_right_held: bool,
    pub mouse_left_press: Spring1D,
    pub mouse_right_press: Spring1D,
    pub mouse_scroll_pulse: Spring1D,
    pub mouse_scroll_angle: f32,
    pub mouse_scroll_vel: f32,
    pub mouse_paw_roll: Spring1D,
    mouse_left_tap_hold: f32,
    mouse_right_tap_hold: f32,
}

impl Default for PostureController {
    fn default() -> Self {
        Self {
            mode: PostureMode::ActiveDesk,
            // Critically damped (zeta >= 0.94): smooth zero-overshoot descent onto desk floor
            floor_rest: Spring1D::new(0.0, 9.5, 0.96, 0.004, 0.02),
            right_paw_mouse_blend: Spring1D::new(1.0, 18.0, 0.92, 0.004, 0.02),
            right_paw_kb_claim_sec: 0.0,
            mouse_offset: Vec2::default(),
            mouse_left_held: false,
            mouse_right_held: false,
            // Crisp underdamped button caps & tactile paw roll
            mouse_left_press: Spring1D::new(0.0, 32.0, 0.62, 0.004, 0.03),
            mouse_right_press: Spring1D::new(0.0, 32.0, 0.62, 0.004, 0.03),
            mouse_scroll_pulse: Spring1D::new(0.0, 22.0, 0.72, 0.004, 0.03),
            mouse_scroll_angle: 0.0,
            mouse_scroll_vel: 0.0,
            mouse_paw_roll: Spring1D::new(0.0, 28.0, 0.60, 0.003, 0.02),
            mouse_left_tap_hold: 0.0,
            mouse_right_tap_hold: 0.0,
        }
    }
}

impl PostureController {
    pub fn clear_held_inputs(&mut self) {
        self.mouse_left_held = false;
        self.mouse_right_held = false;
        self.mouse_left_tap_hold = 0.0;
        self.mouse_right_tap_hold = 0.0;
        self.mouse_left_press.set_target(0.0);
        self.mouse_right_press.set_target(0.0);
        self.mouse_paw_roll.set_target(0.0);
    }

    pub fn claim_right_paw_for_keyboard(&mut self, duration_sec: f32) {
        self.right_paw_kb_claim_sec = self.right_paw_kb_claim_sec.max(duration_sec);
    }

    /// Register mouse delta, button states (including sub-frame click edge flags so fast clicks
    /// inside `< 1` frame are never dropped!), and wheel scroll delta.
    #[allow(clippy::too_many_arguments)]
    pub fn on_mouse_input(
        &mut self,
        dx: i32,
        dy: i32,
        left_btn: Option<bool>,
        left_clicked: bool,
        right_btn: Option<bool>,
        right_clicked: bool,
        wheel_delta: i16,
    ) -> (bool, bool) {
        let moved = dx != 0 || dy != 0;
        let clicked_or_scrolled =
            left_btn.is_some() || left_clicked || right_btn.is_some() || right_clicked || wheel_delta != 0;

        if moved {
            self.mouse_offset.x = (self.mouse_offset.x + dx as f32 * 0.0028).clamp(-1.0, 1.0);
            self.mouse_offset.y = (self.mouse_offset.y + dy as f32 * 0.0028).clamp(-1.0, 1.0);
            if dx.abs() + dy.abs() >= 3 {
                self.right_paw_kb_claim_sec = (self.right_paw_kb_claim_sec - 0.35).max(0.0);
            }
        }

        if let Some(down) = left_btn {
            self.mouse_left_held = down;
        }
        if left_clicked || left_btn == Some(true) {
            // Minimum 110 ms visual tap hold so even a 6 ms click renders clearly at 60/30/24 FPS
            self.mouse_left_tap_hold = 0.11;
            self.mouse_left_press.set_target(1.0);
            self.mouse_left_press.impulse(10.0);
            self.mouse_paw_roll.impulse(4.8); // Roll slightly toward viewer-right (LMB)
            self.right_paw_kb_claim_sec = 0.0;
        }

        if let Some(down) = right_btn {
            self.mouse_right_held = down;
        }
        if right_clicked || right_btn == Some(true) {
            self.mouse_right_tap_hold = 0.11;
            self.mouse_right_press.set_target(1.0);
            self.mouse_right_press.impulse(10.0);
            self.mouse_paw_roll.impulse(-4.8); // Roll slightly toward viewer-left (RMB)
            self.right_paw_kb_claim_sec = 0.0;
        }

        if wheel_delta != 0 {
            let notches = (wheel_delta as f32 / 120.0).clamp(-3.0, 3.0);
            let dir = if notches >= 0.0 { 1.0 } else { -1.0 };
            self.mouse_scroll_pulse.impulse(dir * (9.5 + notches.abs() * 3.5));
            self.mouse_scroll_vel = (self.mouse_scroll_vel + notches * 7.5).clamp(-22.0, 22.0);
            self.right_paw_kb_claim_sec = 0.0;
        }

        (moved, clicked_or_scrolled)
    }

    pub fn is_fast_active(&self) -> bool {
        self.mouse_left_held
            || self.mouse_right_held
            || self.mouse_left_tap_hold > 0.0
            || self.mouse_right_tap_hold > 0.0
            || self.mouse_left_press.is_active()
            || self.mouse_right_press.is_active()
            || self.mouse_scroll_pulse.is_active()
            || self.mouse_scroll_vel.abs() > 0.15
            || self.mouse_paw_roll.is_active()
            || self.right_paw_mouse_blend.is_active()
    }

    pub fn is_any_spring_active(&self) -> bool {
        self.is_fast_active() || self.floor_rest.is_active()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        dt: f32,
        idle_sec: f32,
        force_privacy: bool,
        force_sleep: bool,
        force_awake_neutral: bool,
        force_typing: bool,
        sleep_t: f32,
    ) -> u64 {
        let mut nan_resets = 0u64;

        if self.right_paw_kb_claim_sec > 0.0 {
            self.right_paw_kb_claim_sec = (self.right_paw_kb_claim_sec - dt).max(0.0);
        }
        if self.mouse_left_tap_hold > 0.0 {
            self.mouse_left_tap_hold = (self.mouse_left_tap_hold - dt).max(0.0);
        }
        if self.mouse_right_tap_hold > 0.0 {
            self.mouse_right_tap_hold = (self.mouse_right_tap_hold - dt).max(0.0);
        }

        self.mode = if force_privacy {
            PostureMode::PrivacyCovered
        } else if force_sleep || (!force_awake_neutral && sleep_t > 0.35) {
            PostureMode::Sleeping
        } else if !force_awake_neutral && !force_typing && idle_sec >= IDLE_FLOOR_REST_SEC {
            PostureMode::IdleFloorRest
        } else {
            PostureMode::ActiveDesk
        };

        let floor_target = if self.mode == PostureMode::IdleFloorRest {
            1.0
        } else {
            0.0
        };
        self.floor_rest.set_target(floor_target);
        if self.floor_rest.step(dt) {
            nan_resets += 1;
        }

        let target_mouse_blend = if force_privacy
            || force_sleep
            || self.mode != PostureMode::ActiveDesk
            || self.right_paw_kb_claim_sec > 0.0
            || force_typing
        {
            0.0
        } else {
            1.0
        };
        self.right_paw_mouse_blend.set_target(target_mouse_blend);
        if self.right_paw_mouse_blend.step(dt) {
            nan_resets += 1;
        }

        // Gentle auto-center of mouse offset when idle
        if idle_sec > 2.5 {
            self.mouse_offset.x = exp_smooth(self.mouse_offset.x, 0.0, 2.5, dt);
            self.mouse_offset.y = exp_smooth(self.mouse_offset.y, 0.0, 2.5, dt);
            if self.mouse_offset.x.abs() < 0.002 {
                self.mouse_offset.x = 0.0;
            }
            if self.mouse_offset.y.abs() < 0.002 {
                self.mouse_offset.y = 0.0;
            }
        }

        let l_target = if !force_privacy && (self.mouse_left_held || self.mouse_left_tap_hold > 0.0) {
            1.0
        } else {
            0.0
        };
        let r_target = if !force_privacy && (self.mouse_right_held || self.mouse_right_tap_hold > 0.0) {
            1.0
        } else {
            0.0
        };
        self.mouse_left_press.set_target(l_target);
        self.mouse_right_press.set_target(r_target);
        if self.mouse_left_press.step(dt) {
            nan_resets += 1;
        }
        if self.mouse_right_press.step(dt) {
            nan_resets += 1;
        }

        self.mouse_scroll_pulse.set_target(0.0);
        if self.mouse_scroll_pulse.step(dt) {
            nan_resets += 1;
        }

        // Continuous scroll wheel rotation with smooth friction
        if self.mouse_scroll_vel.abs() > 0.02 {
            self.mouse_scroll_angle =
                (self.mouse_scroll_angle + self.mouse_scroll_vel * dt).rem_euclid(100.0);
            self.mouse_scroll_vel *= (-7.5 * dt).exp();
            if self.mouse_scroll_vel.abs() <= 0.05 {
                self.mouse_scroll_vel = 0.0;
            }
        }

        let roll_target = if self.mouse_left_held {
            0.22
        } else if self.mouse_right_held {
            -0.22
        } else {
            0.0
        };
        self.mouse_paw_roll.set_target(roll_target);
        if self.mouse_paw_roll.step(dt) {
            nan_resets += 1;
        }

        nan_resets
    }
}
