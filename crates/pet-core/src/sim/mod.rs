pub mod expression;
pub mod gaze;
pub mod pacing;
pub mod posture;
pub mod privacy;
pub mod snapshot;
pub mod spring;
pub mod wpm;

pub use expression::{ExpressionController, ExpressionState};
pub use gaze::GazeController;
pub use pacing::{PacingStage, PoseOverride};
pub use posture::{PostureController, PostureMode, IDLE_FLOOR_REST_SEC};
pub use privacy::{PrivacyLease, PrivacySource};
pub use snapshot::{FloatingPopup, SimSnapshot, SleepParticle};
pub use spring::{exp_smooth, Spring1D, Spring2D, Spring3D, MAX_SIM_DT};
pub use wpm::WpmTracker;

use crate::config::{Config, MIN_VISIBLE_OPACITY_INTERACTIVE};
use crate::keys::{
    anonymized_key_for_zone, lookup_keycap, PawSide, PhysicalKey, KEYCAP_COUNT,
};
use crate::rig::{keyboard_local_to_world, mouse_paw_palm_world, RigAnchors, Vec2, Vec3};

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

    // Deterministic salt for Zone-Anonymized typing & popup jitter
    anon_salt: u32,
    // Key states
    keys_held: [bool; KEYCAP_COUNT],
    key_hold_sec: [f32; KEYCAP_COUNT],
    key_press_amounts: [f32; KEYCAP_COUNT],
    key_glow_amounts: [f32; KEYCAP_COUNT],
    anon_active_map: [Option<PhysicalKey>; KEYCAP_COUNT],

    // Orthogonal sub-controllers
    posture: PostureController,
    gaze: GazeController,
    expression: ExpressionController,
    wpm: WpmTracker,
    privacy_lease: PrivacyLease,

    // Hero cartoon physics springs
    left_paw_target: Vec3,
    left_paw_pos: Spring3D,
    left_paw_lift: Spring1D,
    right_paw_kb_target: Vec3,
    right_paw_pos: Spring3D,
    right_paw_lift: Spring1D,

    head_offset: Spring2D,
    head_tilt: Spring1D,
    head_squash: Spring1D,
    torso_bounce: Spring1D,
    tail_curl: Spring1D,
    sleep_t: f32,
    paw_stiffness_t: f32,

    privacy_bob: f32,
    privacy_tail_ripple: f32,
    drowsy_z_spawn_accum: f32,

    popups: Vec<FloatingPopup>,
    sleep_particles: Vec<SleepParticle>,
    spring_nan_resets: u64,
    dirty: bool,
}

impl Default for PetSim {
    fn default() -> Self {
        let left_rest = keyboard_local_to_world(0.28, -0.10, 0.0);
        let right_mouse = mouse_paw_palm_world(0.0, 0.0);
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
            posture: PostureController::default(),
            gaze: GazeController::default(),
            expression: ExpressionController::default(),
            wpm: WpmTracker::default(),
            privacy_lease: PrivacyLease::default(),
            left_paw_target: left_rest,
            left_paw_pos: Spring3D::new(left_rest, 28.0, 0.72, 0.08, 0.35),
            left_paw_lift: Spring1D::new(0.0, 32.0, 0.56, 0.005, 0.03),
            right_paw_kb_target: keyboard_local_to_world(-0.34, -0.10, 0.0),
            right_paw_pos: Spring3D::new(right_mouse, 28.0, 0.72, 0.08, 0.35),
            right_paw_lift: Spring1D::new(0.0, 32.0, 0.56, 0.005, 0.03),
            head_offset: Spring2D::new(Vec2::default(), 12.0, 0.80, 0.04, 0.2),
            head_tilt: Spring1D::new(0.0, 11.5, 0.76, 0.001, 0.008),
            head_squash: Spring1D::new(0.0, 24.0, 0.58, 0.004, 0.02),
            torso_bounce: Spring1D::new(0.0, 26.0, 0.60, 0.004, 0.03),
            tail_curl: Spring1D::new(0.0, 14.0, 0.66, 0.004, 0.02),
            sleep_t: 0.0,
            paw_stiffness_t: 1.0,
            privacy_bob: 0.0,
            privacy_tail_ripple: 0.0,
            drowsy_z_spawn_accum: 0.0,
            popups: Vec::new(),
            sleep_particles: Vec::new(),
            spring_nan_resets: 0,
            dirty: true,
        }
    }
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

    pub fn spring_nan_resets(&self) -> u64 {
        self.spring_nan_resets
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
        } else {
            self.privacy_lease.reset();
        }
        self.wake_active();
        self.manual_privacy
    }

    pub fn set_auto_privacy(&mut self, src: PrivacySource) {
        if self.auto_privacy != src {
            self.auto_privacy = src;
            if self.is_privacy_active() {
                self.on_privacy_entered();
            } else {
                self.privacy_lease.reset();
            }
            self.wake_active();
        }
    }

    pub fn set_pose_override(&mut self, pose: PoseOverride) {
        if self.pose_override != pose {
            self.pose_override = pose;
            if pose == PoseOverride::PrivacyCurl {
                self.privacy_lease.notify_manual_activity();
            }
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
        // Hard privacy invariant (Bug #3 fix): immediately purge popups, held keys, AND
        // lingering key press/glow buffers so zero keycaps stay lit behind or entering Privacy Mode.
        self.popups.clear();
        self.release_all_keys();
        self.key_press_amounts.fill(0.0);
        self.key_glow_amounts.fill(0.0);
        self.wpm.clear();
        self.expression.cancel_yawn_and_reset_idle_cycle();
        self.dirty = true;
    }

    pub fn release_all_keys(&mut self) {
        self.keys_held.fill(false);
        self.key_hold_sec.fill(0.0);
        self.anon_active_map.fill(None);
        self.posture.clear_held_inputs();
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

    /// User-activity wake: resets idle timer and engages `Active60`.
    pub fn wake_active(&mut self) {
        self.idle_sec = 0.0;
        self.expression.cancel_yawn_and_reset_idle_cycle();
        if !self.game_hidden && !self.session_locked && !self.display_off {
            self.pacing = PacingStage::Active60;
        }
        self.dirty = true;
    }

    /// Anonymous key activity ping stripped of `PhysicalKey` at the `InputMailbox` boundary.
    /// Drives ONLY the timing-decorrelated behind-tail typing lease in Manual Privacy Mode
    /// (`CredentialHeuristic` / `ElevatedFocus` auto-privacy stays strictly silent at `0 FPS`).
    pub fn on_privacy_activity(&mut self) {
        if self.game_hidden || self.session_locked || self.display_off {
            return;
        }
        if self.privacy_source() == PrivacySource::Manual {
            self.privacy_lease.notify_manual_activity();
            if self.pacing == PacingStage::Parked0 {
                self.pacing = PacingStage::Settling24;
            }
            self.dirty = true;
        }
    }

    /// Ingest a physical key down/up event. Enforces privacy-by-construction at ingress.
    pub fn on_key_event(&mut self, key: PhysicalKey, is_down: bool, cfg: &Config) {
        if self.game_hidden || self.session_locked || self.display_off {
            return;
        }
        // Privacy hard gate: drop key identity before any state mutation or pop-up queue!
        if self.is_privacy_active() {
            self.popups.clear();
            if is_down {
                self.on_privacy_activity();
            }
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
                self.anon_salt = self
                    .anon_salt
                    .wrapping_mul(1_664_525)
                    .wrapping_add(1_013_904_223);
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
                self.wpm.record_keystroke(self.time_sec);

                // Action-following gaze looks down at the struck keyboard zone
                self.gaze.on_key_typed(vis_spec.kb_x, vis_spec.kb_y);

                // Peripheral-calm impulse scaling: snappy on isolated presses, controlled at high WPM
                let calm_scale = 1.0 - self.wpm.focus_intensity() * 0.35;
                self.torso_bounce.impulse(8.5 * calm_scale);
                self.head_squash.impulse(2.8 * calm_scale);

                let target_3d = keyboard_local_to_world(vis_spec.kb_x, vis_spec.kb_y, 1.0);
                match vis_spec.paw_side {
                    PawSide::Left => {
                        self.left_paw_target = target_3d;
                        // Downward strike press into keycap with crisp spring rebound
                        self.left_paw_lift.kick_pos(-0.30, -4.5);
                        if (self.anon_salt & 0x07) == 0 {
                            self.expression.flick_ear(true, 4.2);
                        }
                    }
                    PawSide::Right => {
                        self.right_paw_kb_target = target_3d;
                        self.right_paw_lift.kick_pos(-0.30, -4.5);
                        self.posture.claim_right_paw_for_keyboard(0.75);
                        if (self.anon_salt & 0x07) == 1 {
                            self.expression.flick_ear(false, 4.2);
                        }
                    }
                }

                // Opt-in floating white keycap pop-ups ONLY in Showcase Mode when enabled
                if cfg.showcase_mode && cfg.show_floating_popups {
                    self.spawn_popup(
                        vis_spec.label,
                        vis_spec.kb_x,
                        vis_spec.key == PhysicalKey::Space,
                    );
                }
            }
            self.wake_active();
        } else {
            self.keys_held[vis_idx] = false;
            self.key_hold_sec[vis_idx] = 0.0;
            // Gentle upward rebound lift on key release for rhythmic typing bounce
            match vis_spec.paw_side {
                PawSide::Left => self.left_paw_lift.impulse(3.6),
                PawSide::Right => self.right_paw_lift.impulse(3.6),
            }
            self.dirty = true;
        }
    }

    fn spawn_popup(&mut self, label: &str, kb_x: f32, wide: bool) {
        if self.popups.len() >= 8 {
            self.popups.remove(0);
        }
        let jitter = ((self.anon_salt & 0xFF) as f32 / 255.0 - 0.5) * 26.0;
        self.anon_salt = self
            .anon_salt
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let x = 18.0 + kb_x * 86.0 + jitter;
        self.popups.push(FloatingPopup {
            label: label.to_string(),
            x,
            y: 44.0,
            vx: jitter * 0.55,
            vy: -48.0,
            age: 0.0,
            ttl: 0.68,
            wide,
        });
    }

    /// Update mouse relative motion, buttons (including sub-frame click edge flags), scroll, and cursor gaze.
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
        gaze_norm: Option<Vec2>,
    ) {
        if self.game_hidden || self.session_locked || self.display_off {
            return;
        }
        if self.is_privacy_active() {
            return;
        }

        let (moved, clicked_or_scrolled) = self.posture.on_mouse_input(
            dx,
            dy,
            left_btn,
            left_clicked,
            right_btn,
            right_clicked,
            wheel_delta,
        );

        if clicked_or_scrolled {
            self.torso_bounce.impulse(4.2);
            if left_clicked || left_btn == Some(true) || right_clicked || right_btn == Some(true) {
                self.right_paw_lift.impulse(-4.5); // Slight downward press tap onto mouse palm
            }
            if wheel_delta != 0 {
                self.expression.flick_ear(false, 2.8);
            }
        }

        if moved || clicked_or_scrolled {
            self.gaze.on_mouse_activity(gaze_norm, clicked_or_scrolled);
            self.wake_active();
        }
    }

    /// Localized 2D exact-spot head scratching (works both unlocked and click-through locked).
    #[allow(clippy::too_many_arguments)]
    pub fn on_head_scratch(
        &mut self,
        inside_head: bool,
        norm_u: f32,
        norm_v: f32,
        hover_pos_dip: Vec2,
        buttons_held: bool,
        cfg: &Config,
    ) -> bool {
        let triggered = self.expression.on_head_scratch(
            inside_head,
            norm_u,
            norm_v,
            hover_pos_dip,
            buttons_held,
            cfg,
            self.is_privacy_active(),
            self.game_hidden,
        );
        if triggered {
            self.head_squash.impulse(6.2);
            self.wake_active();
        }
        triggered || self.expression.pat_active
    }

    /// Hover-rub head patting (backward-compatible 1D helper).
    pub fn on_head_hover_dip(&mut self, inside_head: bool, hover_x_dip: f32, cfg: &Config) -> bool {
        let triggered = self.expression.on_head_hover_dip(
            inside_head,
            hover_x_dip,
            cfg,
            self.is_privacy_active(),
            self.game_hidden,
        );
        if triggered {
            self.head_squash.impulse(6.2);
            self.wake_active();
        }
        triggered || self.expression.pat_active
    }

    pub fn on_pointer_leave(&mut self) {
        self.expression.on_pointer_leave();
    }

    /// Advance simulation by `dt_raw` seconds (clamped to `<= MAX_SIM_DT = 0.10 s`).
    pub fn step(&mut self, dt_raw: f32) {
        let dt = dt_raw.clamp(0.0, MAX_SIM_DT);
        self.time_sec += dt;

        if self.game_hidden || self.session_locked || self.display_off {
            self.pacing = PacingStage::Parked0;
            return;
        }

        self.idle_sec += dt;

        // Effective visual pose (visual PoseOverride NEVER disables privacy input suppression)
        let force_privacy_pose =
            self.is_privacy_active() || self.pose_override == PoseOverride::PrivacyCurl;
        let force_sleep_pose = self.pose_override == PoseOverride::Sleep;
        let force_awake_neutral = self.pose_override == PoseOverride::AwakeNeutral;
        let force_typing_pose = self.pose_override == PoseOverride::Typing;

        if force_typing_pose {
            let beat = (self.time_sec * 6.5) as u32;
            let demo_kx_l = 0.42 - ((beat % 3) as f32) * 0.22;
            let demo_kx_r = -0.42 + (((beat + 1) % 3) as f32) * 0.22;
            self.left_paw_target = keyboard_local_to_world(demo_kx_l, -0.15, 1.0);
            self.right_paw_kb_target = keyboard_local_to_world(demo_kx_r, -0.15, 1.0);
            self.posture.claim_right_paw_for_keyboard(0.35);
            self.gaze.on_key_typed(if beat.is_multiple_of(2) { demo_kx_l } else { demo_kx_r }, -0.15);
            self.idle_sec = 0.0;
        }
        if force_awake_neutral {
            // Bug #6 fix: prevent AwakeNeutral preview from drooping or resting after 7.5/15 s
            self.idle_sec = 0.0;
        }

        // Rolling WPM focus tracker
        self.wpm.step(
            self.time_sec,
            dt,
            force_privacy_pose || force_sleep_pose || force_awake_neutral,
        );
        let focus = if force_typing_pose {
            0.55
        } else {
            self.wpm.focus_intensity()
        };

        // Update key holds & glow decay (guaranteed zeroed in Privacy Mode!)
        let mut keys_animating = false;
        if force_privacy_pose {
            self.key_press_amounts.fill(0.0);
            self.key_glow_amounts.fill(0.0);
        } else {
            for i in 0..KEYCAP_COUNT {
                if self.keys_held[i] {
                    self.key_hold_sec[i] += dt;
                    self.key_press_amounts[i] = exp_smooth(self.key_press_amounts[i], 1.0, 28.0, dt);
                    self.key_glow_amounts[i] = exp_smooth(self.key_glow_amounts[i], 1.0, 22.0, dt);
                    keys_animating = true;
                } else {
                    if self.key_press_amounts[i] > 0.005 {
                        self.key_press_amounts[i] =
                            exp_smooth(self.key_press_amounts[i], 0.0, 18.0, dt);
                        keys_animating = true;
                    } else {
                        self.key_press_amounts[i] = 0.0;
                    }
                    if self.key_glow_amounts[i] > 0.005 {
                        self.key_glow_amounts[i] =
                            exp_smooth(self.key_glow_amounts[i], 0.0, 7.5, dt);
                        keys_animating = true;
                    } else {
                        self.key_glow_amounts[i] = 0.0;
                    }
                }
            }
        }

        // Update floating keycap pop-ups
        for p in &mut self.popups {
            p.age += dt;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.vy += 95.0 * dt;
        }
        self.popups.retain(|p| p.age < p.ttl);

        // Sleep progression
        let in_drowsy = force_sleep_pose
            || (self.pose_override == PoseOverride::Auto
                && !self.designer_connected
                && !force_privacy_pose
                && self.idle_sec >= 14.5);
        let sleep_goal = if force_sleep_pose || (in_drowsy && self.idle_sec >= 20.0) {
            1.0
        } else {
            0.0
        };
        self.sleep_t = exp_smooth(self.sleep_t, sleep_goal, 3.5, dt);
        if (self.sleep_t - sleep_goal).abs() < 0.004 {
            self.sleep_t = sleep_goal;
        }

        // Step Tail Curl spring (snappy curl over keyboard in Privacy Mode)
        let tail_goal = if force_privacy_pose { 1.0 } else { 0.0 };
        self.tail_curl.set_target(tail_goal);
        if self.tail_curl.step(dt) {
            self.spring_nan_resets += 1;
        }
        let tail_curled = self.tail_curl.pos() > 0.72;

        // Step Timing-Decorrelated Privacy Lease (ONLY active in Manual Privacy or PrivacyCurl preview!)
        let allow_privacy_motion = self.privacy_source() == PrivacySource::Manual
            || self.pose_override == PoseOverride::PrivacyCurl;
        let (pbob, pripple) = self
            .privacy_lease
            .step(dt, allow_privacy_motion && force_privacy_pose, tail_curled);
        self.privacy_bob = pbob;
        self.privacy_tail_ripple = pripple;

        // Step Posture controller (7.5 s floor rest, right-paw arbitration, mouse clicks/scroll)
        self.spring_nan_resets += self.posture.step(
            dt,
            self.idle_sec,
            force_privacy_pose,
            force_sleep_pose,
            force_awake_neutral,
            force_typing_pose,
            self.sleep_t,
        );

        // Step Expression controller (yawn 14.5-16.9 s, head-pat purr+blush, blink, ear springs)
        self.spring_nan_resets += self.expression.step(
            dt,
            self.idle_sec,
            force_privacy_pose,
            force_sleep_pose,
            force_awake_neutral,
            self.designer_connected,
            focus,
            self.sleep_t,
        );

        // Step Action-Following Gaze controller
        self.spring_nan_resets += self.gaze.step(
            dt,
            self.idle_sec,
            force_privacy_pose,
            force_sleep_pose,
            force_awake_neutral,
            force_typing_pose,
            self.expression.pat_active,
            self.expression.yawn_t,
            self.sleep_t,
        );

        // Smoothly blended paw stiffness ramp (zero acceleration jolt between idle/sleep and active!)
        let want_active_stiff = if self.posture.mode == PostureMode::IdleFloorRest
            || self.posture.mode == PostureMode::Sleeping
        {
            0.0
        } else {
            1.0
        };
        self.paw_stiffness_t = exp_smooth(self.paw_stiffness_t, want_active_stiff, 8.0, dt);
        let paw_omega = 11.0 + self.paw_stiffness_t * 19.0;
        let paw_zeta = 0.96 - self.paw_stiffness_t * 0.24;
        self.left_paw_pos.set_params(paw_omega, paw_zeta);
        self.right_paw_pos.set_params(paw_omega, paw_zeta);

        // Compute destination 3D positions for front paws
        let left_dest = match self.posture.mode {
            PostureMode::PrivacyCovered => Vec3::new(
                RigAnchors::PRIVACY_PAW_LEFT.x,
                RigAnchors::PRIVACY_PAW_LEFT.y - self.privacy_bob * 4.5,
                RigAnchors::PRIVACY_PAW_LEFT.z,
            ),
            PostureMode::Sleeping => RigAnchors::SLEEP_PAW_LEFT,
            PostureMode::IdleFloorRest => RigAnchors::IDLE_FLOOR_PAW_LEFT,
            PostureMode::ActiveDesk => {
                if !force_typing_pose && self.idle_sec > 2.0 {
                    let home = keyboard_local_to_world(0.28, -0.10, 0.0);
                    let u = ((self.idle_sec - 2.0) / 1.2).clamp(0.0, 1.0);
                    let smooth_u = u * u * (3.0 - 2.0 * u);
                    self.left_paw_target.lerp(home, smooth_u)
                } else {
                    self.left_paw_target
                }
            }
        };

        let right_dest = match self.posture.mode {
            PostureMode::PrivacyCovered => Vec3::new(
                RigAnchors::PRIVACY_PAW_RIGHT.x,
                RigAnchors::PRIVACY_PAW_RIGHT.y - (1.0 - self.privacy_bob) * 4.5 * (if self.privacy_bob > 0.01 { 1.0 } else { 0.0 }),
                RigAnchors::PRIVACY_PAW_RIGHT.z,
            ),
            PostureMode::Sleeping => RigAnchors::SLEEP_PAW_RIGHT,
            PostureMode::IdleFloorRest => RigAnchors::IDLE_FLOOR_PAW_RIGHT,
            PostureMode::ActiveDesk => {
                let mouse_palm = mouse_paw_palm_world(
                    self.posture.mouse_offset.x,
                    self.posture.mouse_offset.y,
                );
                // Subtle forward/sideward paw micro-shift when clicking LMB or RMB
                let click_dx =
                    (self.posture.mouse_left_press.pos() - self.posture.mouse_right_press.pos()) * 2.4;
                let click_dy = (self.posture.mouse_left_press.pos().max(self.posture.mouse_right_press.pos()))
                    * 2.2
                    + self.posture.mouse_scroll_pulse.pos().clamp(-1.0, 1.0) * 1.2;
                let mouse_pos = Vec3::new(mouse_palm.x + click_dx, mouse_palm.y + click_dy, mouse_palm.z);
                let blend = self.posture.right_paw_mouse_blend.pos().clamp(0.0, 1.0);
                self.right_paw_kb_target.lerp(mouse_pos, blend)
            }
        };

        self.left_paw_pos.set_target(left_dest);
        self.right_paw_pos.set_target(right_dest);
        self.spring_nan_resets += self.left_paw_pos.step(dt);
        self.spring_nan_resets += self.right_paw_pos.step(dt);

        self.left_paw_lift.set_target(0.0);
        self.right_paw_lift.set_target(0.0);
        if self.left_paw_lift.step(dt) {
            self.spring_nan_resets += 1;
        }
        if self.right_paw_lift.step(dt) {
            self.spring_nan_resets += 1;
        }

        // Dynamic torso & head damping for peripheral calm at high WPM
        let torso_zeta = 0.60 + focus * 0.24;
        self.torso_bounce.set_params(26.0, torso_zeta);
        let bounce_goal = if force_privacy_pose {
            self.privacy_bob * 0.22
        } else {
            0.0
        };
        self.torso_bounce.set_target(bounce_goal);
        if self.torso_bounce.step(dt) {
            self.spring_nan_resets += 1;
        }

        let squash_goal = if self.expression.pat_active && !force_privacy_pose {
            0.46
        } else if self.expression.yawn_t > 0.1 {
            -0.28 * self.expression.yawn_t // Gentle upward stretch during yawn!
        } else {
            0.0
        };
        self.head_squash.set_target(squash_goal);
        if self.head_squash.step(dt) {
            self.spring_nan_resets += 1;
        }

        let gaze = self.gaze.eye_gaze.pos();
        let focus_lean_y = focus * 3.2;
        let scratch_lean = if self.expression.scratch_intensity > 0.01 && !force_privacy_pose {
            let sp = self.expression.scratch_spot.unwrap_or_default();
            Vec2::new(
                sp.x * 6.5 * self.expression.scratch_intensity,
                sp.y * 3.2 * self.expression.scratch_intensity,
            )
        } else {
            Vec2::default()
        };
        let head_x_goal = gaze.x * 5.0 + scratch_lean.x;
        let head_y_goal = gaze.y * 3.2
            + focus_lean_y
            + scratch_lean.y
            + self.head_squash.pos() * 4.5
            - self.torso_bounce.pos() * 3.6
            - self.expression.yawn_t * 3.0;
        self.head_offset.set_target(Vec2::new(head_x_goal, head_y_goal));
        self.spring_nan_resets += self.head_offset.step(dt);

        let tilt_goal = if self.expression.pat_active && !force_privacy_pose {
            let sp_x = self.expression.scratch_spot.map(|s| s.x).unwrap_or(0.0);
            sp_x * 0.14 + (self.time_sec * 4.8).sin() * 0.045
        } else if force_privacy_pose {
            -0.065 // Polite slight head turn matching averted eyes
        } else if self.expression.yawn_t > 0.1 {
            0.04 * self.expression.yawn_t
        } else {
            gaze.x * 0.06
        };
        self.head_tilt.set_target(tilt_goal);
        if self.head_tilt.step(dt) {
            self.spring_nan_resets += 1;
        }

        // Floating `Z` particles ONLY during Drowsy wind-down (16 s..56 s) or explicit Sleep preview
        let should_spawn_z =
            force_sleep_pose || (in_drowsy && self.idle_sec >= 16.5 && self.idle_sec < 55.5);
        if should_spawn_z {
            self.drowsy_z_spawn_accum += dt;
            if self.drowsy_z_spawn_accum >= 1.2 && self.sleep_particles.len() < 4 {
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

        // 4-stage hysteresis FSM with guaranteed convergence to Parked0
        let fast_springs_active = keys_animating
            || !self.popups.is_empty()
            || self.expression.is_fast_active()
            || self.posture.is_fast_active()
            || self.torso_bounce.is_active()
            || self.left_paw_lift.is_active()
            || self.right_paw_lift.is_active()
            || self.tail_curl.is_active();

        let privacy_settling = self.tail_curl.is_active()
            || self.left_paw_pos.is_active()
            || self.right_paw_pos.is_active()
            || self.head_offset.is_active()
            || self.head_tilt.is_active()
            || self.gaze.is_active();

        self.pacing = if self.designer_connected || self.pose_override != PoseOverride::Auto {
            if fast_springs_active || self.idle_sec < 1.5 || force_typing_pose {
                PacingStage::Active60
            } else {
                PacingStage::Settling24
            }
        } else if force_privacy_pose {
            if self.tail_curl.is_active() {
                PacingStage::Active60
            } else if privacy_settling || self.privacy_lease.is_active() {
                PacingStage::Settling24
            } else {
                // Zero-FPS park in CredentialHeuristic auto-privacy AND after Manual Privacy lease ends!
                PacingStage::Parked0
            }
        } else if fast_springs_active || self.idle_sec < 1.5 {
            PacingStage::Active60
        } else if self.idle_sec < 14.5 {
            PacingStage::Settling24
        } else if self.idle_sec < 60.0
            || !self.sleep_particles.is_empty()
            || (self.sleep_t - 1.0).abs() > 0.01
            || self.expression.yawn_t > 0.01
            || self.left_paw_pos.is_active()
            || self.right_paw_pos.is_active()
            || self.posture.is_any_spring_active()
        {
            PacingStage::Drowsy12
        } else {
            PacingStage::Parked0
        };

        self.dirty = true;
    }

    pub fn snapshot(&self) -> SimSnapshot {
        let force_privacy =
            self.is_privacy_active() || self.pose_override == PoseOverride::PrivacyCurl;
        let force_sleep = self.pose_override == PoseOverride::Sleep;
        let focus = if self.pose_override == PoseOverride::Typing {
            0.55
        } else {
            self.wpm.focus_intensity()
        };
        let expression = self.expression.resolve_expression(
            force_privacy,
            force_sleep,
            self.sleep_t,
            focus,
        );
        let blink_close = self.expression.blink_close();
        let happy_squint = if expression == ExpressionState::HappyPat {
            1.0
        } else {
            0.0
        };
        let eyelid_close = match expression {
            ExpressionState::Sleeping => self.sleep_t.max(if force_sleep { 1.0 } else { 0.0 }),
            ExpressionState::HappyPat => 1.0,
            ExpressionState::Yawning => self.expression.yawn_t.clamp(0.0, 1.0),
            ExpressionState::PrivacyAverted => 0.16, // Calm politely averted upper lid, open eyes!
            ExpressionState::Focused => (0.14 * focus).max(blink_close).max(self.sleep_t * 0.85),
            // Smooth eyelid opening on wake-from-sleep instead of single-frame snap open
            ExpressionState::Neutral => blink_close.max(self.sleep_t * 0.85),
        };

        let lp = self.left_paw_pos.pos();
        let rp = self.right_paw_pos.pos();
        let ll = self.left_paw_lift.pos().clamp(-0.45, 1.0);
        let rl = self.right_paw_lift.pos().clamp(-0.45, 1.0);
        let left_world = Vec3::new(lp.x, lp.y - ll * 9.0, lp.z);
        let right_world = Vec3::new(rp.x, rp.y - rl * 9.0, rp.z);

        let ear_l = self.expression.ear_relax_l.pos().clamp(-0.25, 1.10);
        let ear_r = self.expression.ear_relax_r.pos().clamp(-0.25, 1.10);

        SimSnapshot {
            time_sec: self.time_sec,
            pacing: self.pacing,
            posture: self.posture.mode,
            expression,
            privacy_active: force_privacy,
            privacy_source: self.privacy_source(),
            head_offset: self.head_offset.pos(),
            head_tilt_rad: self.head_tilt.pos(),
            head_squash: self.head_squash.pos().clamp(-0.35, 0.85),
            eye_gaze: self.gaze.eye_gaze.pos(),
            eyelid_close,
            happy_squint,
            ear_relax: (ear_l + ear_r) * 0.5,
            ear_relax_l: ear_l,
            ear_relax_r: ear_r,
            ear_twitch_l: self.expression.ear_twitch_l.pos().clamp(-0.6, 0.6),
            ear_twitch_r: self.expression.ear_twitch_r.pos().clamp(-0.6, 0.6),
            focus_intensity: focus,
            yawn_t: self.expression.yawn_t,
            blush_amount: self.expression.blush_amount,
            purr_intensity: self.expression.purr_intensity,
            purr_phase: self.expression.purr_phase,
            scratch_spot: self.expression.scratch_spot,
            scratch_intensity: self.expression.scratch_intensity,
            breath_phase: self.time_sec * 1.85,
            torso_bounce: self.torso_bounce.pos().clamp(-0.4, 1.2),
            floor_rest_t: self.posture.floor_rest.pos().clamp(0.0, 1.0),
            left_paw_world: left_world,
            right_paw_world: right_world,
            left_paw_strike: (-ll).clamp(0.0, 1.0),
            right_paw_strike: (-rl).clamp(0.0, 1.0),
            right_paw_on_mouse: self.posture.right_paw_mouse_blend.pos().clamp(0.0, 1.0),
            mouse_offset: self.posture.mouse_offset,
            mouse_left_press: self.posture.mouse_left_press.pos().clamp(0.0, 1.2),
            mouse_right_press: self.posture.mouse_right_press.pos().clamp(0.0, 1.2),
            mouse_scroll_pulse: self.posture.mouse_scroll_pulse.pos().clamp(-1.2, 1.2),
            mouse_scroll_angle: self.posture.mouse_scroll_angle,
            mouse_paw_roll: self.posture.mouse_paw_roll.pos().clamp(-0.45, 0.45),
            tail_curl_t: self.tail_curl.pos().clamp(0.0, 1.08),
            tail_sway_phase: self.time_sec * 2.1,
            privacy_bob: self.privacy_bob,
            privacy_tail_ripple: self.privacy_tail_ripple,
            sleep_t: self.sleep_t,
            patting_active: self.expression.pat_active,
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
    fn privacy_mode_purges_popups_and_zeroes_keycap_press_and_glow_immediately() {
        let mut sim = PetSim::default();
        let cfg = Config {
            showcase_mode: true,
            show_floating_popups: true,
            ..Config::default()
        };
        sim.on_key_event(PhysicalKey::A, true, &cfg);
        assert_eq!(sim.snapshot().popups.len(), 1);
        assert!(sim.snapshot().key_press_amounts[PhysicalKey::A.index()] > 0.9);
        assert!(sim.snapshot().key_glow_amounts[PhysicalKey::A.index()] > 0.9);

        sim.toggle_manual_privacy();
        assert!(sim.is_privacy_active());
        let snap = sim.snapshot();
        assert!(snap.popups.is_empty());
        assert!(snap.key_press_amounts.iter().all(|&v| v == 0.0));
        assert!(snap.key_glow_amounts.iter().all(|&v| v == 0.0));
        assert_eq!(snap.expression, ExpressionState::PrivacyAverted);

        // Subsequent keystroke while in privacy mode must never spawn popups or press keys
        sim.on_key_event(PhysicalKey::P, true, &cfg);
        let snap2 = sim.snapshot();
        assert!(snap2.popups.is_empty());
        assert!(!sim.has_any_key_held());
        assert!(snap2.key_press_amounts.iter().all(|&v| v == 0.0));
        assert!(snap2.key_glow_amounts.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn privacy_auto_credential_heuristic_stays_silent_and_parks_at_zero_fps() {
        let mut sim = PetSim::default();
        let cfg = Config::default();
        sim.set_auto_privacy(PrivacySource::CredentialHeuristic);

        // Let tail finish curling & springs settle
        for _ in 0..120 {
            sim.step(1.0 / 30.0);
        }
        assert_eq!(sim.pacing(), PacingStage::Parked0);

        // Keystrokes during auto-privacy must produce zero behind-tail motion and keep Parked0
        sim.on_key_event(PhysicalKey::S, true, &cfg);
        sim.on_privacy_activity();
        sim.step(1.0 / 30.0);
        assert_eq!(sim.snapshot().privacy_bob, 0.0);
        assert_eq!(sim.snapshot().privacy_tail_ripple, 0.0);
        assert_eq!(sim.pacing(), PacingStage::Parked0);
    }

    #[test]
    fn privacy_manual_activity_lease_decorrelates_cadence_and_parks_cleanly() {
        let mut sim_a = PetSim::default();
        let mut sim_b = PetSim::default();
        sim_a.toggle_manual_privacy();
        sim_b.toggle_manual_privacy();
        // Settle tail curl
        for _ in 0..90 {
            sim_a.step(1.0 / 30.0);
            sim_b.step(1.0 / 30.0);
        }

        // Password A: 1 keypress; Password B: 8 rapid keypresses inside 200 ms.
        // Because lease does not fire 1:1 tap impulses and refuses refresh while remaining >= 0.55 s,
        // both produce identical self-clocked behind-tail envelopes!
        sim_a.on_privacy_activity();
        sim_b.on_privacy_activity();
        for i in 0..12 {
            if i < 7 {
                sim_b.on_privacy_activity();
            }
            sim_a.step(1.0 / 60.0);
            sim_b.step(1.0 / 60.0);
        }
        let sa = sim_a.snapshot();
        let sb = sim_b.snapshot();
        assert!(sa.privacy_bob > 0.0 || sa.privacy_tail_ripple > 0.0);
        assert!((sa.privacy_bob - sb.privacy_bob).abs() < 1e-4);
        assert!((sa.privacy_tail_ripple - sb.privacy_tail_ripple).abs() < 1e-4);
        // Eyes must stay averted away from keyboard (negative x, negative y)
        assert!(sa.eye_gaze.x < -0.5);
        assert!(sa.eye_gaze.y < 0.0);

        // After lease expires, must settle cleanly to Parked0!
        for _ in 0..120 {
            sim_a.step(1.0 / 24.0);
        }
        assert_eq!(sim_a.pacing(), PacingStage::Parked0);
    }

    #[test]
    fn full_lifecycle_floor_rest_yawn_drowsy12_and_parked0() {
        let mut sim = PetSim::default();
        assert_eq!(sim.pacing(), PacingStage::Active60);

        // 0..2.0 s -> Settling24, paws still on desk
        for _ in 0..60 {
            sim.step(1.0 / 30.0);
        }
        assert_eq!(sim.pacing(), PacingStage::Settling24);
        assert_eq!(sim.snapshot().posture, PostureMode::ActiveDesk);

        // Advance past 7.5 s -> IdleFloorRest engages smoothly with zero overshoot
        for _ in 0..180 {
            sim.step(1.0 / 24.0);
        }
        let snap_rest = sim.snapshot();
        assert_eq!(snap_rest.posture, PostureMode::IdleFloorRest);
        assert!(snap_rest.floor_rest_t > 0.95 && snap_rest.floor_rest_t <= 1.0);

        // Advance into yawn window (14.5..16.9 s) using real Drowsy12 83.3 ms dt!
        let mut saw_yawn = false;
        for _ in 0..90 {
            sim.step(1.0 / 12.0);
            let s = sim.snapshot();
            if s.expression == ExpressionState::Yawning && s.yawn_t > 0.5 {
                saw_yawn = true;
            }
        }
        assert!(saw_yawn, "pre-sleep yawn must trigger once between 14.5 and 16.9 s");
        assert_eq!(sim.pacing(), PacingStage::Drowsy12);

        // Advance past 60 s -> Parked0 at 0 FPS with zero sleep particles & zero NaN resets
        for _ in 0..600 {
            sim.step(1.0 / 12.0);
        }
        assert_eq!(sim.pacing(), PacingStage::Parked0);
        assert!(sim.snapshot().sleep_particles.is_empty());
        assert_eq!(sim.snapshot().expression, ExpressionState::Sleeping);
        assert_eq!(sim.spring_nan_resets(), 0);
    }

    #[test]
    fn action_following_gaze_tracks_keyboard_when_typing_and_cursor_when_mousing() {
        let mut sim = PetSim::default();
        let cfg = Config {
            showcase_mode: true,
            ..Config::default()
        };
        sim.on_key_event(PhysicalKey::A, true, &cfg);
        for _ in 0..20 {
            sim.step(1.0 / 60.0);
        }
        let s_type = sim.snapshot();
        // A is on cat's left (viewer right, kb_x > 0), gaze should look down (+y) toward right (+x)
        assert!(s_type.eye_gaze.y > 0.25, "expected downward keyboard gaze, got {:?}", s_type.eye_gaze);
        assert!(s_type.eye_gaze.x > 0.15, "expected right-biased gaze for A key, got {:?}", s_type.eye_gaze);

        // Wait for typing gaze hold to expire, then move mouse upper-left
        for _ in 0..70 {
            sim.step(1.0 / 60.0);
        }
        sim.on_mouse_input(-12, -8, None, false, None, false, 0, Some(Vec2::new(-0.9, -0.7)));
        for _ in 0..24 {
            sim.step(1.0 / 60.0);
        }
        let s_mouse = sim.snapshot();
        assert!(s_mouse.eye_gaze.x < -0.4);
        assert!(s_mouse.eye_gaze.y < -0.2);
    }

    #[test]
    fn subframe_mouse_click_and_scroll_animate_reliably() {
        let mut sim = PetSim::default();
        // Simulate sub-frame left click (already released by drain time: left_btn=Some(false), left_clicked=true)
        sim.on_mouse_input(0, 0, Some(false), true, None, false, 120, None);
        sim.step(1.0 / 24.0);
        let s = sim.snapshot();
        assert!(s.mouse_left_press > 0.4, "sub-frame LMB click must visibly depress button");
        assert!(s.mouse_scroll_angle > 0.0, "scroll wheel angle must advance");
        assert!(s.mouse_scroll_pulse.abs() > 0.05);
    }

    #[test]
    fn awake_neutral_override_never_droops_past_15_seconds() {
        let mut sim = PetSim::default();
        sim.set_pose_override(PoseOverride::AwakeNeutral);
        for _ in 0..450 {
            sim.step(0.05);
        }
        let s = sim.snapshot();
        assert_eq!(s.expression, ExpressionState::Neutral);
        assert_eq!(s.posture, PostureMode::ActiveDesk);
        assert!(s.ear_relax.abs() < 0.05);
        assert_eq!(s.sleep_t, 0.0);
    }

    #[test]
    fn mouse_motion_follows_cat_pov_and_wake_from_sleep_is_immediate() {
        let mut sim = PetSim::default();
        // Physical mouse right (+dx) and forward (-dy) must move virtual mouse viewer-left (-x) and viewer-down (+y)
        sim.on_mouse_input(50, -50, None, false, None, false, 0, None);
        let s = sim.snapshot();
        assert!(s.mouse_offset.x < -0.05, "expected viewer-left offset, got {:?}", s.mouse_offset);
        assert!(s.mouse_offset.y > 0.05, "expected viewer-down offset, got {:?}", s.mouse_offset);

        // Fall fully asleep
        for _ in 0..320 {
            sim.step(1.0 / 12.0);
        }
        assert_eq!(sim.snapshot().posture, PostureMode::Sleeping);
        // Single keystroke must wake posture to ActiveDesk on the very next frame (0 ms stall)
        sim.on_key_event(PhysicalKey::F, true, &Config::default());
        sim.step(1.0 / 60.0);
        assert_eq!(sim.snapshot().posture, PostureMode::ActiveDesk);
    }

    #[test]
    fn localized_head_scratch_works_when_locked_and_flattens_touched_ear() {
        let mut sim = PetSim::default();
        let locked_cfg = Config {
            click_through_locked: true,
            ..Config::default()
        };
        // Buttons held must suppress scratching (prevents misfire during window drag / selection)
        for i in 0..20 {
            let x = if i % 2 == 0 { 10.0 } else { 40.0 };
            sim.on_head_scratch(true, -0.7, -0.3, Vec2::new(x, 0.0), true, &locked_cfg);
        }
        assert!(!sim.snapshot().patting_active);

        // Back-and-forth hover rub on viewer-left side while click-through locked triggers localized scratch
        for i in 0..16 {
            let x = if i % 2 == 0 { 10.0 } else { 36.0 };
            sim.on_head_scratch(true, -0.75, -0.3, Vec2::new(x, 0.0), false, &locked_cfg);
            sim.step(0.02);
        }
        let snap = sim.snapshot();
        assert!(snap.patting_active);
        assert_eq!(snap.expression, ExpressionState::HappyPat);
        assert!(snap.blush_amount > 0.25);
        assert!(snap.purr_intensity > 0.25);
        assert!(snap.ear_relax_l > snap.ear_relax_r + 0.15, "left ear should flatten more when scratching left side");
        assert!(snap.scratch_spot.is_some());
    }
}
