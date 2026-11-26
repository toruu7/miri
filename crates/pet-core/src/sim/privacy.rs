use super::spring::exp_smooth;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacySource {
    None,
    Manual,
    CredentialHeuristic,
    ElevatedFocus,
}

/// Timing-decorrelated behind-tail typing lease for Manual Privacy Mode.
///
/// Security contract:
/// - Never fires a 1:1 per-keystroke impulse (prevents password-length & inter-keystroke timing leaks).
/// - In `CredentialHeuristic` or `ElevatedFocus` auto-privacy, stays strictly silent (`0 FPS` once curled).
/// - In `Manual` privacy, anonymous key activity refreshes a coarse randomized lease (`0.85 s + 0.25..0.65 s`)
///   driving an internal self-clocked `~6 Hz` behind-tail typing bob and subtle fluffy tail-crest ripple,
///   then ramps cleanly to `0` so the overlay returns to `Parked0`.
#[derive(Debug, Clone)]
pub struct PrivacyLease {
    remaining_sec: f32,
    bob_phase: f32,
    envelope: f32,
    rng_state: u32,
}

impl Default for PrivacyLease {
    fn default() -> Self {
        Self {
            remaining_sec: 0.0,
            bob_phase: 0.0,
            envelope: 0.0,
            rng_state: 0xA511_E9B3,
        }
    }
}

impl PrivacyLease {
    pub fn reset(&mut self) {
        self.remaining_sec = 0.0;
        self.envelope = 0.0;
    }

    /// Notify anonymous keyboard activity in Manual Privacy Mode (zero key identity).
    /// Only refreshes when the lease is low or expired so individual keystroke cadence is obscured.
    pub fn notify_manual_activity(&mut self) {
        if self.remaining_sec < 0.55 {
            self.rng_state = self
                .rng_state
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223);
            let jitter = ((self.rng_state >> 16) as f32 / 65535.0) * 0.40; // 0.00..0.40
            // Total randomized lease duration: 1.10..1.50 s (base 0.85 + 0.25..0.65 padding)
            self.remaining_sec = 1.10 + jitter;
        }
    }

    pub fn is_active(&self) -> bool {
        self.remaining_sec > 0.0 || self.envelope > 0.015
    }

    pub fn step(&mut self, dt: f32, allow_motion: bool, tail_curled: bool) -> (f32, f32) {
        if !allow_motion {
            self.remaining_sec = 0.0;
            self.envelope = exp_smooth(self.envelope, 0.0, 14.0, dt);
            if self.envelope < 0.01 {
                self.envelope = 0.0;
            }
            return (0.0, 0.0);
        }

        if self.remaining_sec > 0.0 {
            self.remaining_sec = (self.remaining_sec - dt).max(0.0);
        }

        // Smooth fade-out over the last 0.30 s of the lease
        let target_env = if !tail_curled {
            0.0
        } else if self.remaining_sec >= 0.30 {
            1.0
        } else {
            (self.remaining_sec / 0.30).clamp(0.0, 1.0)
        };

        self.envelope = exp_smooth(self.envelope, target_env, 12.0, dt);
        if self.envelope < 0.01 && target_env == 0.0 {
            self.envelope = 0.0;
            return (0.0, 0.0);
        }

        // Self-clocked ~6 Hz internal typing rhythm (uncorrelated with actual user cadence)
        self.bob_phase += dt * 6.0 * std::f32::consts::TAU;
        if self.bob_phase > std::f32::consts::TAU * 64.0 {
            self.bob_phase -= std::f32::consts::TAU * 64.0;
        }

        let bob = (self.bob_phase.sin() * 0.5 + 0.5) * self.envelope;
        let ripple = ((self.bob_phase * 0.5).sin() * 0.5 + 0.5) * self.envelope;
        (bob, ripple)
    }
}
