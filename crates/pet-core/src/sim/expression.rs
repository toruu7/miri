use super::spring::{exp_smooth, Spring1D};
use crate::config::Config;

/// Mutually exclusive facial silhouette mode so no eye/brow/mouth states collide at `0.6x` Mini scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExpressionState {
    #[default]
    Neutral,
    Focused,
    HappyPat,
    Yawning,
    Sleeping,
    PrivacyAverted,
}

/// Manages head-pat visual purr + blush, pre-sleep yawn (`14.5-16.9 s`), blink, and snappy ear flicks.
#[derive(Debug, Clone)]
pub struct ExpressionController {
    // Hover-rub head pat tracker (`>= 120 dip` horizontal travel within `600 ms`)
    pat_rub_travel_dip: f32,
    pat_rub_window_sec: f32,
    pat_hold_sec: f32,
    last_hover_x_dip: Option<f32>,
    pub pat_active: bool,

    // Visual purr & cheek blush
    pub purr_intensity: f32,
    pub purr_phase: f32,
    pub blush_amount: f32,

    // Pre-sleep yawn (plays once per idle cycle in 14.5..16.9 s, cancelled immediately on input)
    yawn_played_this_idle: bool,
    yawn_elapsed: f32,
    pub yawn_t: f32,

    // Blink
    blink_timer: f32,
    blink_duration: f32,
    blink_elapsed: f32,

    // Expressive ear springs
    pub ear_relax: Spring1D,
    pub ear_twitch_l: Spring1D,
    pub ear_twitch_r: Spring1D,
    rng_state: u32,
}

impl Default for ExpressionController {
    fn default() -> Self {
        Self {
            pat_rub_travel_dip: 0.0,
            pat_rub_window_sec: 0.0,
            pat_hold_sec: 0.0,
            last_hover_x_dip: None,
            pat_active: false,
            purr_intensity: 0.0,
            purr_phase: 0.0,
            blush_amount: 0.0,
            yawn_played_this_idle: false,
            yawn_elapsed: -1.0,
            yawn_t: 0.0,
            blink_timer: 3.4,
            blink_duration: 0.14,
            blink_elapsed: -1.0,
            ear_relax: Spring1D::new(0.0, 12.0, 0.82, 0.004, 0.02),
            ear_twitch_l: Spring1D::new(0.0, 30.0, 0.52, 0.003, 0.02),
            ear_twitch_r: Spring1D::new(0.0, 30.0, 0.52, 0.003, 0.02),
            rng_state: 0x51A7_9D21,
        }
    }
}

impl ExpressionController {
    pub fn cancel_yawn_and_reset_idle_cycle(&mut self) {
        self.yawn_played_this_idle = false;
        self.yawn_elapsed = -1.0;
        self.yawn_t = 0.0;
    }

    pub fn flick_ear(&mut self, left_side: bool, strength: f32) {
        if left_side {
            self.ear_twitch_l.impulse(strength);
        } else {
            self.ear_twitch_r.impulse(strength);
        }
    }

    pub fn on_pointer_leave(&mut self) {
        self.last_hover_x_dip = None;
    }

    /// Returns `true` if patting triggered or refreshed an active pat pulse.
    pub fn on_head_hover_dip(
        &mut self,
        inside_head: bool,
        hover_x_dip: f32,
        cfg: &Config,
        privacy_active: bool,
        game_hidden: bool,
    ) -> bool {
        if cfg.effective_click_through() || privacy_active || game_hidden {
            self.pat_active = false;
            self.last_hover_x_dip = None;
            return false;
        }
        if !inside_head {
            self.last_hover_x_dip = None;
            if self.pat_hold_sec <= 0.0 {
                self.pat_active = false;
            }
            return false;
        }

        let mut triggered = false;
        if let Some(prev_x) = self.last_hover_x_dip {
            let delta = (hover_x_dip - prev_x).abs();
            if delta > 0.5 && delta < 80.0 {
                self.pat_rub_travel_dip += delta;
                self.pat_rub_window_sec = 0.65;
                if self.pat_rub_travel_dip >= 120.0 {
                    self.pat_active = true;
                    self.pat_hold_sec = 0.65;
                    self.pat_rub_travel_dip = 60.0;
                    self.ear_twitch_l.impulse(-3.2);
                    self.ear_twitch_r.impulse(-3.2);
                    self.cancel_yawn_and_reset_idle_cycle();
                    triggered = true;
                }
            }
        }
        self.last_hover_x_dip = Some(hover_x_dip);
        triggered
    }

    pub fn blink_close(&self) -> f32 {
        if self.blink_elapsed >= 0.0 {
            let phase = (self.blink_elapsed / self.blink_duration).clamp(0.0, 1.0);
            (phase * std::f32::consts::PI).sin().clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn is_fast_active(&self) -> bool {
        self.pat_active
            || self.blush_amount > 0.02
            || self.purr_intensity > 0.02
            || self.ear_twitch_l.is_active()
            || self.ear_twitch_r.is_active()
    }

    pub fn is_any_active(&self) -> bool {
        self.is_fast_active() || self.ear_relax.is_active() || self.yawn_t > 0.01
    }

    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        dt: f32,
        idle_sec: f32,
        force_privacy: bool,
        force_sleep: bool,
        force_awake_neutral: bool,
        designer_connected: bool,
        focus_intensity: f32,
        sleep_t: f32,
    ) -> u64 {
        let mut nan_resets = 0u64;

        // Head-pat decay
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

        // Visual purr & blush ramp
        let pat_vis = self.pat_active && !force_privacy && !force_sleep;
        let target_purr = if pat_vis { 1.0 } else { 0.0 };
        self.purr_intensity = exp_smooth(self.purr_intensity, target_purr, 6.5, dt);
        if self.purr_intensity < 0.01 && target_purr == 0.0 {
            self.purr_intensity = 0.0;
        }
        if self.purr_intensity > 0.01 {
            // Soft warm ~3.6 Hz visual chest swell/purr rhythm
            self.purr_phase += dt * 3.6 * std::f32::consts::TAU;
            if self.purr_phase > std::f32::consts::TAU * 64.0 {
                self.purr_phase -= std::f32::consts::TAU * 64.0;
            }
        }
        self.blush_amount = exp_smooth(self.blush_amount, target_purr, 5.5, dt);
        if self.blush_amount < 0.01 && target_purr == 0.0 {
            self.blush_amount = 0.0;
        }

        // Pre-sleep yawn: plays ONCE in 14.5..16.9 s (2.4 s arc) when naturally winding down
        const YAWN_DURATION: f32 = 2.4;
        let can_yawn = !force_privacy
            && !force_sleep
            && !force_awake_neutral
            && !designer_connected
            && !self.pat_active;
        if can_yawn && !self.yawn_played_this_idle && (14.5..17.0).contains(&idle_sec) {
            self.yawn_played_this_idle = true;
            self.yawn_elapsed = 0.0;
        }
        if !can_yawn {
            self.yawn_elapsed = -1.0;
            self.yawn_t = 0.0;
        } else if self.yawn_elapsed >= 0.0 {
            self.yawn_elapsed += dt;
            if self.yawn_elapsed >= YAWN_DURATION {
                self.yawn_elapsed = -1.0;
                self.yawn_t = 0.0;
            } else {
                // Smooth asymmetric bell: 0.45 s open, 1.10 s hold, 0.85 s close
                let e = self.yawn_elapsed;
                self.yawn_t = if e < 0.45 {
                    let u = e / 0.45;
                    u * u * (3.0 - 2.0 * u)
                } else if e < 1.55 {
                    1.0
                } else {
                    let u = ((YAWN_DURATION - e) / 0.85).clamp(0.0, 1.0);
                    u * u * (3.0 - 2.0 * u)
                };
            }
        }

        // Ear posture spring (perk slightly during high-WPM focus, relax for pat/yawn/privacy/sleep)
        let ear_goal = if pat_vis {
            0.78
        } else if self.yawn_t > 0.1 {
            0.48 * self.yawn_t
        } else if force_privacy {
            0.36
        } else if force_sleep || (!force_awake_neutral && sleep_t > 0.2) {
            0.42 * sleep_t.max(if force_sleep { 1.0 } else { 0.0 })
        } else if focus_intensity > 0.15 {
            -0.18 * focus_intensity // Perked forward slightly when typing fast!
        } else {
            0.0
        };
        self.ear_relax.set_target(ear_goal);
        if self.ear_relax.step(dt) {
            nan_resets += 1;
        }
        self.ear_twitch_l.set_target(0.0);
        self.ear_twitch_r.set_target(0.0);
        if self.ear_twitch_l.step(dt) {
            nan_resets += 1;
        }
        if self.ear_twitch_r.step(dt) {
            nan_resets += 1;
        }

        // Natural blink (only while awake & not sleeping/yawning/patting/privacy)
        if !force_privacy && !force_sleep && sleep_t < 0.25 && !pat_vis && self.yawn_t < 0.1 {
            if self.blink_elapsed >= 0.0 {
                self.blink_elapsed += dt;
                if self.blink_elapsed >= self.blink_duration {
                    self.blink_elapsed = -1.0;
                    self.rng_state = self
                        .rng_state
                        .wrapping_mul(1_664_525)
                        .wrapping_add(1_013_904_223);
                    self.blink_timer = 2.8 + ((self.rng_state % 320) as f32 / 100.0);
                }
            } else {
                self.blink_timer -= dt;
                if self.blink_timer <= 0.0 {
                    self.blink_elapsed = 0.0;
                }
            }
        } else {
            self.blink_elapsed = -1.0;
        }

        nan_resets
    }

    pub fn resolve_expression(
        &self,
        force_privacy: bool,
        force_sleep: bool,
        sleep_t: f32,
        focus_intensity: f32,
    ) -> ExpressionState {
        if force_privacy {
            ExpressionState::PrivacyAverted
        } else if force_sleep || sleep_t >= 0.65 {
            ExpressionState::Sleeping
        } else if self.pat_active {
            ExpressionState::HappyPat
        } else if self.yawn_t > 0.12 {
            ExpressionState::Yawning
        } else if focus_intensity >= 0.28 {
            ExpressionState::Focused
        } else {
            ExpressionState::Neutral
        }
    }
}
