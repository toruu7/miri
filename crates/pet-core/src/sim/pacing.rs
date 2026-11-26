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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoseOverride {
    #[default]
    Auto,
    AwakeNeutral,
    Typing,
    Sleep,
    PrivacyCurl,
}
