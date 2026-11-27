use super::spring::{exp_smooth, Spring1D};
use crate::config::{Config, MIN_VISIBLE_OPACITY_INTERACTIVE};
use crate::rig::Vec2;

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

/// Manages localized exact-spot head scratching (purr + blush + per-side ear flatten),
/// pre-sleep yawn (`14.5-16.9 s`), blink, and snappy ear flicks.
#[derive(Debug, Clone)]
pub struct ExpressionController {
    // 2D exact-spot hover-rub head scratch tracker (works unlocked AND click-through locked!)
    pat_rub_travel_dip: f32,
    pat_rub_window_sec: f32,
    pat_hold_sec: f32,
    last_hover_pos_dip: Option<Vec2>,
    last_rub_dir: Vec2,
    rub_reversals: u8,
    pub pat_active: bool,
    pub scratch_spot: Option<Vec2>,
    pub scratch_intensity: f32,

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

    // Expressive per-side ear springs
    pub ear_relax_l: Spring1D,
    pub ear_relax_r: Spring1D,
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
            last_hover_pos_dip: None,
            last_rub_dir: Vec2::default(),
            rub_reversals: 0,
            pat_active: false,
            scratch_spot: None,
            scratch_intensity: 0.0,
            purr_intensity: 0.0,
            purr_phase: 0.0,
            blush_amount: 0.0,
            yawn_played_this_idle: false,
            yawn_elapsed: -1.0,
            yawn_t: 0.0,
            blink_timer: 3.4,
            blink_duration: 0.14,
            blink_elapsed: -1.0,
            ear_relax_l: Spring1D::new(0.0, 12.0, 0.82, 0.004, 0.02),
            ear_relax_r: Spring1D::new(0.0, 12.0, 0.82, 0.004, 0.02),
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
        self.last_hover_pos_dip = None;
        self.last_rub_dir = Vec2::default();
    }

    /// Localized 2D exact-spot head scratching. Works both unlocked and click-through locked,
    /// gated against accidental sweeps via button-held suppression and back-and-forth reversal detection.
    #[allow(clippy::too_many_arguments)]
    pub fn on_head_scratch(
        &mut self,
        inside_head: bool,
        norm_u: f32,
        norm_v: f32,
        hover_pos_dip: Vec2,
        buttons_held: bool,
        cfg: &Config,
        privacy_active: bool,
        game_hidden: bool,
    ) -> bool {
        if buttons_held
            || privacy_active
            || game_hidden
            || cfg.opacity < MIN_VISIBLE_OPACITY_INTERACTIVE
        {
            self.pat_active = false;
            self.last_hover_pos_dip = None;
            self.last_rub_dir = Vec2::default();
            self.rub_reversals = 0;
            return false;
        }
        if !inside_head {
            self.last_hover_pos_dip = None;
            self.last_rub_dir = Vec2::default();
            if self.pat_hold_sec <= 0.0 {
                self.pat_active = false;
            }
            return false;
        }

        let spot = Vec2::new(norm_u.clamp(-1.0, 1.0), norm_v.clamp(-1.0, 1.0));
        let mut triggered = false;
        if let Some(prev) = self.last_hover_pos_dip {
            let dx = hover_pos_dip.x - prev.x;
            let dy = hover_pos_dip.y - prev.y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > 0.6 && dist < 80.0 {
                let dir = Vec2::new(dx / dist, dy / dist);
                let prev_len_sq =
                    self.last_rub_dir.x * self.last_rub_dir.x + self.last_rub_dir.y * self.last_rub_dir.y;
                if prev_len_sq > 0.25 {
                    let dot = dir.x * self.last_rub_dir.x + dir.y * self.last_rub_dir.y;
                    if dot < -0.20 {
                        self.rub_reversals = self.rub_reversals.saturating_add(1);
                    }
                }
                self.last_rub_dir = dir;
                self.pat_rub_travel_dip += dist;
                self.pat_rub_window_sec = 0.70;

                // Smoothly track the exact spot being rubbed on the head
                self.scratch_spot = Some(match self.scratch_spot {
                    Some(prev_s) => prev_s.lerp(spot, 0.45),
                    None => spot,
                });

                let enough_rub = (self.pat_rub_travel_dip >= 52.0 && self.rub_reversals >= 1)
                    || (self.pat_active && self.pat_rub_travel_dip >= 24.0)
                    || self.pat_rub_travel_dip >= 120.0;
                if enough_rub {
                    self.pat_active = true;
                    self.pat_hold_sec = 0.72;
                    self.pat_rub_travel_dip = 18.0;
                    // Twitch the touched side's ear slightly stronger than the opposite ear
                    let l_imp = if spot.x <= 0.15 { -3.6 } else { -1.6 };
                    let r_imp = if spot.x >= -0.15 { -3.6 } else { -1.6 };
                    self.ear_twitch_l.impulse(l_imp);
                    self.ear_twitch_r.impulse(r_imp);
                    self.cancel_yawn_and_reset_idle_cycle();
                    triggered = true;
                }
            }
        }
        self.last_hover_pos_dip = Some(hover_pos_dip);
        triggered
    }

    /// Backward-compatible 1D hover helper delegating to `on_head_scratch`.
    pub fn on_head_hover_dip(
        &mut self,
        inside_head: bool,
        hover_x_dip: f32,
        cfg: &Config,
        privacy_active: bool,
        game_hidden: bool,
    ) -> bool {
        self.on_head_scratch(
            inside_head,
            0.0,
            -0.25,
            Vec2::new(hover_x_dip, 0.0),
            false,
            cfg,
            privacy_active,
            game_hidden,
        )
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
            || self.scratch_intensity > 0.02
            || self.blush_amount > 0.02
            || self.purr_intensity > 0.02
            || self.ear_twitch_l.is_active()
            || self.ear_twitch_r.is_active()
    }

    pub fn is_any_active(&self) -> bool {
        self.is_fast_active()
            || self.ear_relax_l.is_active()
            || self.ear_relax_r.is_active()
            || self.yawn_t > 0.01
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
                self.rub_reversals = 0;
                self.last_rub_dir = Vec2::default();
            }
        }
        if self.pat_hold_sec > 0.0 {
            self.pat_hold_sec = (self.pat_hold_sec - dt).max(0.0);
            if self.pat_hold_sec <= 0.0 {
                self.pat_active = false;
            }
        }

        // Visual purr, blush, and localized scratch spot intensity ramp
        let pat_vis = self.pat_active && !force_privacy && !force_sleep;
        let target_purr = if pat_vis { 1.0 } else { 0.0 };
        self.scratch_intensity = exp_smooth(self.scratch_intensity, target_purr, 8.5, dt);
        if self.scratch_intensity < 0.01 && target_purr == 0.0 {
            self.scratch_intensity = 0.0;
            self.scratch_spot = None;
        }
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

        // Per-side ear posture springs (flatten the scratched side more when scratching!)
        let (ear_goal_l, ear_goal_r) = if pat_vis {
            let u = self.scratch_spot.map(|s| s.x.clamp(-1.0, 1.0)).unwrap_or(0.0);
            let gl = (0.76 - u * 0.36).clamp(0.32, 1.08);
            let gr = (0.76 + u * 0.36).clamp(0.32, 1.08);
            (gl, gr)
        } else {
            let g = if self.yawn_t > 0.1 {
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
            (g, g)
        };
        self.ear_relax_l.set_target(ear_goal_l);
        self.ear_relax_r.set_target(ear_goal_r);
        if self.ear_relax_l.step(dt) {
            nan_resets += 1;
        }
        if self.ear_relax_r.step(dt) {
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
