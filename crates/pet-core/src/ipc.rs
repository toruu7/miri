use crate::config::{CatProfile, Config};
use crate::sim::{PacingStage, PoseOverride, PrivacySource};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_IPC_FRAME_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DesignerToOverlay {
    Hello {
        protocol_version: u32,
        designer_pid: u32,
    },
    PreviewPatch {
        seq: u64,
        profile_draft: CatProfile,
        pose_override: PoseOverride,
    },
    CommitProfile {
        profile: CatProfile,
        make_active: bool,
    },
    DeleteProfile {
        profile_id: String,
    },
    UpdateConfig {
        config: Config,
    },
    CancelPreview,
    RequestDiagnostics,
    FocusDesigner,
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum OverlayToDesigner {
    HelloAck {
        protocol_version: u32,
        overlay_pid: u32,
        config: Config,
        user_profiles: Vec<CatProfile>,
        diagnostics: DiagnosticsSnapshot,
    },
    CommitAck {
        ok: bool,
        error: Option<String>,
    },
    Diagnostics(DiagnosticsSnapshot),
    OverlayShuttingDown,
    Pong,
}

/// Strictly redacted diagnostic snapshot (zero keycodes, zero characters, zero window titles,
/// zero file paths, zero privacy pulse counts). Safe to copy to clipboard or inspect in `cat-designer`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DiagnosticsSnapshot {
    pub app_version: String,
    pub pacing_stage: String,
    pub target_fps: u32,
    pub on_battery: bool,
    pub dpi: u32,
    pub canvas_w_px: u32,
    pub canvas_h_px: u32,
    pub overlay_visible: bool,
    pub click_through_effective: bool,
    pub privacy_source: String,
    pub fullscreen_suppressed: bool,
    pub fullscreen_reason: String,
    pub raw_input_registered: bool,
    pub raw_input_events_sec: u32,
    pub uipi_stale_releases: u64,
    pub dt_clamped_count: u64,
    pub gdi_objects: u32,
    pub working_set_kb: u64,
    pub render_avg_us: u32,
    pub dirty_rect_avg_pct: u8,
    pub sprite_cache_rebuilds: u64,
    pub spring_nan_resets: u64,
    pub hotkey_conflicts: Vec<String>,
    pub last_config_error: Option<String>,
    pub last_ipc_error: Option<String>,
}

impl DiagnosticsSnapshot {
    pub fn set_state_enums(&mut self, pacing: PacingStage, privacy: PrivacySource) {
        self.pacing_stage = format!("{pacing:?}");
        self.privacy_source = format!("{privacy:?}");
    }
}

/// Encode a length-prefixed JSON IPC frame (`u32 LE` + UTF-8 JSON).
pub fn encode_frame<T: Serialize>(msg: &T) -> Result<Vec<u8>, String> {
    let payload = serde_json::to_vec(msg).map_err(|e| format!("json encode: {e}"))?;
    if payload.len() > MAX_IPC_FRAME_BYTES {
        return Err(format!(
            "ipc frame exceeds max {} bytes ({})",
            MAX_IPC_FRAME_BYTES,
            payload.len()
        ));
    }
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

pub fn decode_frame_payload<T: for<'de> Deserialize<'de>>(payload: &[u8]) -> Result<T, String> {
    if payload.len() > MAX_IPC_FRAME_BYTES {
        return Err("ipc frame too large".to_string());
    }
    serde_json::from_slice(payload).map_err(|e| format!("json decode: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_frame_roundtrip() {
        let msg = DesignerToOverlay::Hello {
            protocol_version: PROTOCOL_VERSION,
            designer_pid: 4242,
        };
        let bytes = encode_frame(&msg).unwrap();
        let len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        assert_eq!(len + 4, bytes.len());
        let decoded: DesignerToOverlay = decode_frame_payload(&bytes[4..]).unwrap();
        match decoded {
            DesignerToOverlay::Hello {
                protocol_version,
                designer_pid,
            } => {
                assert_eq!(protocol_version, PROTOCOL_VERSION);
                assert_eq!(designer_pid, 4242);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn diagnostics_v1_backward_compat_defaults() {
        // Older diagnostics JSON without new perf fields must deserialize cleanly.
        let legacy_json = r#"{"app_version":"0.1.0","pacing_stage":"Parked0","target_fps":0}"#;
        let snap: DiagnosticsSnapshot = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(snap.app_version, "0.1.0");
        assert_eq!(snap.render_avg_us, 0);
        assert_eq!(snap.dirty_rect_avg_pct, 0);
        assert_eq!(snap.sprite_cache_rebuilds, 0);
        assert_eq!(snap.spring_nan_resets, 0);
    }
}
