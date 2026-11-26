use crate::rig::{Vec2, Vec3};

/// Maximum simulation delta time (seconds). Raised from `0.05` to `0.10` so `Drowsy12` (`83.3 ms`)
/// advances wall-clock timers at true 1.0x speed while analytical springs remain unconditionally stable.
pub const MAX_SIM_DT: f32 = 0.10;

/// Closed-form analytical second-order damped harmonic oscillator (DHO).
///
/// Unconditionally stable for any `dt >= 0`, producing identical bounce curves across
/// 60 FPS (`16.7 ms`), 30 FPS (`33.3 ms`), 24 FPS (`41.7 ms`), and 12 FPS (`83.3 ms`)
/// without sub-stepping or Euler divergence.
#[derive(Debug, Clone, Copy)]
pub struct Spring1D {
    pos: f32,
    vel: f32,
    target: f32,
    omega: f32,
    zeta: f32,
    eps_pos: f32,
    eps_vel: f32,
    settled: bool,
}

impl Spring1D {
    pub fn new(initial: f32, omega: f32, zeta: f32, eps_pos: f32, eps_vel: f32) -> Self {
        Self {
            pos: initial,
            vel: 0.0,
            target: initial,
            omega: omega.max(0.5),
            zeta: zeta.max(0.05),
            eps_pos: eps_pos.max(1e-5),
            eps_vel: eps_vel.max(1e-4),
            settled: true,
        }
    }

    #[inline]
    pub fn pos(&self) -> f32 {
        self.pos
    }

    #[inline]
    pub fn vel(&self) -> f32 {
        self.vel
    }

    #[inline]
    pub fn target(&self) -> f32 {
        self.target
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        !self.settled
    }

    #[inline]
    pub fn set_params(&mut self, omega: f32, zeta: f32) {
        self.omega = omega.max(0.5);
        self.zeta = zeta.max(0.05);
    }

    #[inline]
    pub fn set_target(&mut self, target: f32) {
        if !target.is_finite() {
            return;
        }
        if (self.target - target).abs() > self.eps_pos * 0.25 {
            self.target = target;
            self.settled = false;
        } else {
            self.target = target;
        }
    }

    #[inline]
    pub fn impulse(&mut self, delta_vel: f32) {
        if !delta_vel.is_finite() {
            return;
        }
        self.vel += delta_vel;
        self.settled = false;
    }

    #[inline]
    pub fn kick_pos(&mut self, pos: f32, vel: f32) {
        if !pos.is_finite() || !vel.is_finite() {
            return;
        }
        self.pos = pos;
        self.vel = vel;
        self.settled = false;
    }

    #[inline]
    pub fn snap_to(&mut self, value: f32) {
        let v = if value.is_finite() { value } else { 0.0 };
        self.pos = v;
        self.target = v;
        self.vel = 0.0;
        self.settled = true;
    }

    /// Advance analytically by `dt` seconds. Returns `true` if a non-finite state had to be reset.
    pub fn step(&mut self, dt: f32) -> bool {
        if !self.pos.is_finite() || !self.vel.is_finite() || !self.target.is_finite() {
            let safe = if self.target.is_finite() { self.target } else { 0.0 };
            self.snap_to(safe);
            return true;
        }

        let x0 = self.pos - self.target;
        let v0 = self.vel;
        if self.settled && x0.abs() <= self.eps_pos && v0.abs() <= self.eps_vel {
            self.pos = self.target;
            self.vel = 0.0;
            return false;
        }
        if dt <= 0.0 || !dt.is_finite() {
            return false;
        }

        let w = self.omega;
        let z = self.zeta;
        let (x_next, v_next) = if z < 1.0 - 1e-4 {
            // Underdamped (snappy cartoon overshoot)
            let wd = w * (1.0 - z * z).sqrt();
            let decay = (-z * w * dt).exp();
            let (sin_d, cos_d) = (wd * dt).sin_cos();
            let a = x0;
            let b = (v0 + z * w * x0) / wd;
            let x = decay * (a * cos_d + b * sin_d);
            let v = decay * ((b * wd - a * z * w) * cos_d - (a * wd + b * z * w) * sin_d);
            (x, v)
        } else if (z - 1.0).abs() <= 1e-4 {
            // Critically damped (fastest zero-overshoot settle)
            let decay = (-w * dt).exp();
            let a = x0;
            let b = v0 + w * x0;
            let x = decay * (a + b * dt);
            let v = decay * (b - w * (a + b * dt));
            (x, v)
        } else {
            // Overdamped (overflow-free negative exponents: r1 < 0, r2 < 0)
            let s = (z * z - 1.0).sqrt();
            let r1 = -w * (z - s);
            let r2 = -w * (z + s);
            let e1 = (r1 * dt).exp();
            let e2 = (r2 * dt).exp();
            let denom = r2 - r1; // strictly negative
            let c2 = (v0 - r1 * x0) / denom;
            let c1 = x0 - c2;
            let x = c1 * e1 + c2 * e2;
            let v = c1 * r1 * e1 + c2 * r2 * e2;
            (x, v)
        };

        if !x_next.is_finite() || !v_next.is_finite() {
            self.snap_to(self.target);
            return true;
        }

        if x_next.abs() <= self.eps_pos && v_next.abs() <= self.eps_vel {
            self.pos = self.target;
            self.vel = 0.0;
            self.settled = true;
        } else {
            self.pos = self.target + x_next;
            self.vel = v_next;
            self.settled = false;
        }
        false
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Spring2D {
    pub x: Spring1D,
    pub y: Spring1D,
}

impl Spring2D {
    pub fn new(initial: Vec2, omega: f32, zeta: f32, eps_pos: f32, eps_vel: f32) -> Self {
        Self {
            x: Spring1D::new(initial.x, omega, zeta, eps_pos, eps_vel),
            y: Spring1D::new(initial.y, omega, zeta, eps_pos, eps_vel),
        }
    }

    pub fn pos(&self) -> Vec2 {
        Vec2::new(self.x.pos(), self.y.pos())
    }

    pub fn set_target(&mut self, t: Vec2) {
        self.x.set_target(t.x);
        self.y.set_target(t.y);
    }

    pub fn set_params(&mut self, omega: f32, zeta: f32) {
        self.x.set_params(omega, zeta);
        self.y.set_params(omega, zeta);
    }

    pub fn snap_to(&mut self, v: Vec2) {
        self.x.snap_to(v.x);
        self.y.snap_to(v.y);
    }

    pub fn is_active(&self) -> bool {
        self.x.is_active() || self.y.is_active()
    }

    pub fn step(&mut self, dt: f32) -> u64 {
        let mut resets = 0;
        if self.x.step(dt) {
            resets += 1;
        }
        if self.y.step(dt) {
            resets += 1;
        }
        resets
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Spring3D {
    pub x: Spring1D,
    pub y: Spring1D,
    pub z: Spring1D,
}

impl Spring3D {
    pub fn new(initial: Vec3, omega: f32, zeta: f32, eps_pos: f32, eps_vel: f32) -> Self {
        Self {
            x: Spring1D::new(initial.x, omega, zeta, eps_pos, eps_vel),
            y: Spring1D::new(initial.y, omega, zeta, eps_pos, eps_vel),
            z: Spring1D::new(initial.z, omega, zeta, eps_pos, eps_vel),
        }
    }

    pub fn pos(&self) -> Vec3 {
        Vec3::new(self.x.pos(), self.y.pos(), self.z.pos())
    }

    pub fn set_target(&mut self, t: Vec3) {
        self.x.set_target(t.x);
        self.y.set_target(t.y);
        self.z.set_target(t.z);
    }

    pub fn set_params(&mut self, omega: f32, zeta: f32) {
        self.x.set_params(omega, zeta);
        self.y.set_params(omega, zeta);
        self.z.set_params(omega, zeta);
    }

    pub fn snap_to(&mut self, v: Vec3) {
        self.x.snap_to(v.x);
        self.y.snap_to(v.y);
        self.z.snap_to(v.z);
    }

    pub fn is_active(&self) -> bool {
        self.x.is_active() || self.y.is_active() || self.z.is_active()
    }

    pub fn step(&mut self, dt: f32) -> u64 {
        let mut resets = 0;
        if self.x.step(dt) {
            resets += 1;
        }
        if self.y.step(dt) {
            resets += 1;
        }
        if self.z.step(dt) {
            resets += 1;
        }
        resets
    }
}

#[inline]
pub fn exp_smooth(cur: f32, target: f32, speed: f32, dt: f32) -> f32 {
    if (cur - target).abs() < 1e-5 {
        return target;
    }
    let alpha = 1.0 - (-speed * dt).exp();
    cur + (target - cur) * alpha
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytical_spring_is_framerate_invariant_across_60_30_24_12_fps() {
        let mut s60 = Spring1D::new(0.0, 34.0, 0.52, 1e-4, 1e-3);
        let mut s30 = s60;
        let mut s24 = s60;
        let mut s12 = s60;

        for sp in [&mut s60, &mut s30, &mut s24, &mut s12] {
            sp.set_target(1.0);
            sp.impulse(12.0);
        }

        // Advance all four to exact t = 0.25 s
        for _ in 0..15 {
            assert!(!s60.step(1.0 / 60.0));
        }
        // 7.5 frames @ 30fps -> test t = 0.5 s where 60, 30, 24, 12 all divide cleanly!
        let mut s60 = Spring1D::new(0.0, 34.0, 0.52, 1e-5, 1e-4);
        let mut s30 = s60;
        let mut s24 = s60;
        let mut s12 = s60;
        for sp in [&mut s60, &mut s30, &mut s24, &mut s12] {
            sp.set_target(1.0);
            sp.impulse(14.0);
        }
        for _ in 0..30 {
            s60.step(0.5 / 30.0);
        }
        for _ in 0..15 {
            s30.step(0.5 / 15.0);
        }
        for _ in 0..12 {
            s24.step(0.5 / 12.0);
        }
        for _ in 0..6 {
            s12.step(0.5 / 6.0);
        }

        assert!((s60.pos() - s30.pos()).abs() < 1e-4);
        assert!((s60.pos() - s24.pos()).abs() < 1e-4);
        assert!((s60.pos() - s12.pos()).abs() < 1e-4);
        assert!((s60.vel() - s12.vel()).abs() < 1e-3);
    }

    #[test]
    fn critical_and_overdamped_never_overshoot_or_overflow() {
        let mut crit = Spring1D::new(0.0, 10.0, 1.0, 1e-3, 1e-2);
        crit.set_target(1.0);
        for _ in 0..120 {
            assert!(!crit.step(0.02));
            assert!(crit.pos() >= -1e-5 && crit.pos() <= 1.0001);
        }
        assert!(!crit.is_active());
        assert_eq!(crit.pos(), 1.0);

        let mut over = Spring1D::new(0.0, 40.0, 3.0, 1e-3, 1e-2);
        over.set_target(1.0);
        assert!(!over.step(10.0)); // huge dt must not overflow exp()
        assert!(over.pos().is_finite());
        assert!(!over.is_active());
    }
}
