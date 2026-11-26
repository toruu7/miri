use super::spring::Spring2D;
use crate::rig::Vec2;

/// Action-following gaze arbitrator:
/// 1. Privacy Mode -> politely averted sideways/up-left away from the keyboard (never peeking).
/// 2. Sleep -> serene centered downward gaze.
/// 3. Head Patting -> soft content upward gaze.
/// 4. Active Typing -> looks down at the active keyboard zone.
/// 5. Active Mousing -> tracks the global pointer (biased briefly toward mouse on clicks/scroll).
/// 6. Bounded Idle Look-Arounds (`2.5 s .. 13.0 s`) -> gentle curious glances that NEVER reset `idle_sec`.
#[derive(Debug, Clone)]
pub struct GazeController {
    pub eye_gaze: Spring2D,
    cursor_gaze: Vec2,
    kb_gaze: Vec2,
    typing_gaze_hold_sec: f32,
    mouse_gaze_hold_sec: f32,
    mouse_action_glance_sec: f32,

    // Bounded autonomous idle look-around state
    idle_look_target: Vec2,
    idle_look_timer: f32,
    rng_state: u32,
}

impl Default for GazeController {
    fn default() -> Self {
        Self {
            eye_gaze: Spring2D::new(Vec2::default(), 16.0, 0.86, 0.004, 0.02),
            cursor_gaze: Vec2::default(),
            kb_gaze: Vec2::new(0.0, 0.56),
            typing_gaze_hold_sec: 0.0,
            mouse_gaze_hold_sec: 0.0,
            mouse_action_glance_sec: 0.0,
            idle_look_target: Vec2::default(),
            idle_look_timer: 3.4,
            rng_state: 0x3C6E_F372,
        }
    }
}

impl GazeController {
    pub fn on_key_typed(&mut self, kb_x: f32, kb_y: f32) {
        // Look naturally down at the struck keyboard region
        self.kb_gaze = Vec2::new(
            (kb_x * 0.58).clamp(-0.68, 0.68),
            (0.52 + kb_y * 0.24).clamp(0.32, 0.84),
        );
        self.typing_gaze_hold_sec = 0.95;
        self.idle_look_target = Vec2::default();
        self.idle_look_timer = 3.2;
    }

    pub fn on_mouse_activity(&mut self, cursor_norm: Option<Vec2>, clicked_or_scrolled: bool) {
        if let Some(g) = cursor_norm {
            self.cursor_gaze = Vec2::new(g.x.clamp(-1.0, 1.0), g.y.clamp(-1.0, 1.0));
        }
        self.mouse_gaze_hold_sec = 1.6;
        if clicked_or_scrolled {
            self.mouse_action_glance_sec = 0.45;
        }
        self.idle_look_target = Vec2::default();
        self.idle_look_timer = 3.4;
    }

    pub fn is_active(&self) -> bool {
        self.eye_gaze.is_active()
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
        pat_active: bool,
        yawn_t: f32,
        sleep_t: f32,
    ) -> u64 {
        if self.typing_gaze_hold_sec > 0.0 {
            self.typing_gaze_hold_sec = (self.typing_gaze_hold_sec - dt).max(0.0);
        }
        if self.mouse_gaze_hold_sec > 0.0 {
            self.mouse_gaze_hold_sec = (self.mouse_gaze_hold_sec - dt).max(0.0);
        }
        if self.mouse_action_glance_sec > 0.0 {
            self.mouse_action_glance_sec = (self.mouse_action_glance_sec - dt).max(0.0);
        }

        // Autonomous idle look-arounds strictly bounded to 2.5 s .. 13.0 s
        if !force_privacy
            && !force_sleep
            && !force_awake_neutral
            && !force_typing
            && !pat_active
            && (2.5..13.0).contains(&idle_sec)
        {
            self.idle_look_timer -= dt;
            if self.idle_look_timer <= 0.0 {
                self.rng_state = self
                    .rng_state
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223);
                let pick = (self.rng_state >> 24) % 5;
                self.idle_look_target = match pick {
                    0 => Vec2::new(-0.55, -0.08),
                    1 => Vec2::new(0.55, -0.05),
                    2 => Vec2::new(0.0, -0.22),
                    3 => Vec2::new(-0.32, 0.18),
                    _ => Vec2::new(0.0, 0.04),
                };
                self.idle_look_timer = 2.8 + (((self.rng_state >> 8) & 0xFF) as f32 / 255.0) * 1.6;
            }
        } else if idle_sec >= 13.0 {
            self.idle_look_target = Vec2::new(0.0, 0.15);
        }

        let goal = if force_privacy {
            // Politely averted eyes looking sideways/up-left away from the keyboard
            Vec2::new(-0.82, -0.18)
        } else if force_sleep || sleep_t > 0.45 {
            Vec2::new(0.0, 0.20)
        } else if force_awake_neutral {
            Vec2::new(0.0, 0.02)
        } else if pat_active {
            Vec2::new(0.0, -0.14)
        } else if yawn_t > 0.10 {
            Vec2::new(0.0, -0.12)
        } else if force_typing || self.typing_gaze_hold_sec > 0.0 {
            self.kb_gaze
        } else if self.mouse_action_glance_sec > 0.0 {
            // Subtle glance toward the mouse on click/scroll blended with cursor direction
            self.cursor_gaze.lerp(Vec2::new(-0.56, 0.48), 0.55)
        } else if self.mouse_gaze_hold_sec > 0.0 {
            self.cursor_gaze
        } else if (2.5..13.0).contains(&idle_sec) {
            self.idle_look_target
        } else {
            Vec2::new(0.0, 0.15)
        };

        self.eye_gaze.set_target(goal);
        self.eye_gaze.step(dt)
    }
}
