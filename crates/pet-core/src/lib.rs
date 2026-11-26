pub mod config;
pub mod ipc;
pub mod keys;
pub mod rig;
pub mod sim;

pub use config::{
    builtin_presets, clamp_rect_to_work_area, fnv1a_64_lower, new_user_profile_id,
    resolve_profile, BackdropMode, CatAppearance, CatProfile, Config, GameHideScope, HotkeyConfig,
    MarkingPattern, PeripheralStyle, RectI32, Rgb8, WindowAnchor, CURRENT_SCHEMA_VERSION,
    MAX_CANVAS_EDGE_PX, MAX_USER_PROFILES,
};
pub use ipc::{
    DesignerToOverlay, DiagnosticsSnapshot, OverlayToDesigner, MAX_IPC_FRAME_BYTES,
    PROTOCOL_VERSION,
};
pub use keys::{
    from_windows_scancode, keycap_specs, lookup_keycap, KeyCapSpec, PawSide, PhysicalKey,
    KEYCAP_COUNT,
};
pub use rig::{
    keyboard_local_to_world, mouse_world_pos, project_25d, FrameHitboxes, HitRect, Vec2, Vec3,
    BASE_CANVAS_H, BASE_CANVAS_W, HEADROOM_RATIO,
};
pub use sim::{
    FloatingPopup, PacingStage, PetSim, PoseOverride, PrivacySource, SimSnapshot, SleepParticle,
};
