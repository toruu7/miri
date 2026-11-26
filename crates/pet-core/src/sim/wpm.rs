use super::spring::exp_smooth;

const WPM_WINDOW_SEC: f32 = 4.0;
const WPM_RING_CAP: usize = 48;

/// Rolling-window WPM tracker driving subtle focused typing brows and alert posture
/// without storing any key identity.
#[derive(Debug, Clone)]
pub struct WpmTracker {
    stamps: [f32; WPM_RING_CAP],
    head: usize,
    len: usize,
    last_key_time: f32,
    wpm: f32,
    focus_intensity: f32,
}

impl Default for WpmTracker {
    fn default() -> Self {
        Self {
            stamps: [0.0; WPM_RING_CAP],
            head: 0,
            len: 0,
            last_key_time: -10.0,
            wpm: 0.0,
            focus_intensity: 0.0,
        }
    }
}

impl WpmTracker {
    pub fn record_keystroke(&mut self, now_sec: f32) {
        self.stamps[self.head] = now_sec;
        self.head = (self.head + 1) % WPM_RING_CAP;
        if self.len < WPM_RING_CAP {
            self.len += 1;
        }
        self.last_key_time = now_sec;
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.wpm = 0.0;
        self.focus_intensity = 0.0;
        self.last_key_time = -10.0;
    }

    pub fn wpm(&self) -> f32 {
        self.wpm
    }

    pub fn focus_intensity(&self) -> f32 {
        self.focus_intensity
    }

    pub fn step(&mut self, now_sec: f32, dt: f32, suppressed: bool) {
        if suppressed {
            self.focus_intensity = exp_smooth(self.focus_intensity, 0.0, 10.0, dt);
            if self.focus_intensity < 0.01 {
                self.focus_intensity = 0.0;
            }
            return;
        }

        let cutoff = now_sec - WPM_WINDOW_SEC;
        let mut count = 0usize;
        for i in 0..self.len {
            let idx = (self.head + WPM_RING_CAP - 1 - i) % WPM_RING_CAP;
            if self.stamps[idx] >= cutoff {
                count += 1;
            }
        }
        // Standard WPM = (chars / 5) * (60 / window_sec)
        let inst_wpm = (count as f32 / 5.0) * (60.0 / WPM_WINDOW_SEC);
        self.wpm = exp_smooth(self.wpm, inst_wpm, 3.5, dt);

        let since_last = now_sec - self.last_key_time;
        let target_focus = if since_last > 1.8 {
            0.0
        } else {
            // Engage focused brows above ~58 WPM, peaking smoothly by ~105 WPM
            ((self.wpm - 55.0) / 50.0).clamp(0.0, 1.0)
        };
        let speed = if target_focus > self.focus_intensity { 4.0 } else { 3.0 };
        self.focus_intensity = exp_smooth(self.focus_intensity, target_focus, speed, dt);
        if self.focus_intensity < 0.01 && target_focus == 0.0 {
            self.focus_intensity = 0.0;
        }
    }
}
