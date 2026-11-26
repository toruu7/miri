use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;
pub const MAX_CANVAS_EDGE_PX: u32 = 768;
pub const MAX_USER_PROFILES: usize = 200;
pub const MIN_VISIBLE_OPACITY_INTERACTIVE: f32 = 0.15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb8 {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const fn from_hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as u8,
            g: ((hex >> 8) & 0xFF) as u8,
            b: (hex & 0xFF) as u8,
        }
    }

    pub fn to_hex_string(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Relative luminance in `[0.0, 1.0]` using sRGB linear-ish weights.
    pub fn luminance(self) -> f32 {
        (0.2126 * self.r as f32 + 0.7152 * self.g as f32 + 0.0722 * self.b as f32) / 255.0
    }

    pub fn scale(self, factor: f32) -> Self {
        Self {
            r: (self.r as f32 * factor).clamp(0.0, 255.0) as u8,
            g: (self.g as f32 * factor).clamp(0.0, 255.0) as u8,
            b: (self.b as f32 * factor).clamp(0.0, 255.0) as u8,
        }
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: (self.r as f32 + (other.r as f32 - self.r as f32) * t).round() as u8,
            g: (self.g as f32 + (other.g as f32 - self.g as f32) * t).round() as u8,
            b: (self.b as f32 + (other.b as f32 - self.b as f32) * t).round() as u8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkingPattern {
    Solid,
    TabbyStripes,
    Tuxedo,
    Bicolor,
    Siamese,
    Calico,
    Tortie,
    Spotted,
    Mittens,
    #[serde(other)]
    Unknown,
}

impl MarkingPattern {
    pub const ALL: &'static [MarkingPattern] = &[
        MarkingPattern::Solid,
        MarkingPattern::TabbyStripes,
        MarkingPattern::Tuxedo,
        MarkingPattern::Bicolor,
        MarkingPattern::Siamese,
        MarkingPattern::Calico,
        MarkingPattern::Tortie,
        MarkingPattern::Spotted,
        MarkingPattern::Mittens,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Solid | Self::Unknown => "Solid",
            Self::TabbyStripes => "Tabby Stripes",
            Self::Tuxedo => "Tuxedo",
            Self::Bicolor => "Bicolor",
            Self::Siamese => "Siamese Points",
            Self::Calico => "Calico Patches",
            Self::Tortie => "Tortoiseshell",
            Self::Spotted => "Spotted",
            Self::Mittens => "Mittens",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatAppearance {
    pub base_coat: Rgb8,
    pub marking_pattern: MarkingPattern,
    pub marking_color: Rgb8,
    pub secondary_marking_color: Rgb8,
    pub inner_ear_color: Rgb8,
    pub left_eye_color: Rgb8,
    pub right_eye_color: Rgb8,
    pub whisker_color: Rgb8,
    pub nose_color: Rgb8,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, serde_json::Value>,
}

impl Default for CatAppearance {
    fn default() -> Self {
        Self {
            base_coat: Rgb8::from_hex(0x141418),
            marking_pattern: MarkingPattern::Solid,
            marking_color: Rgb8::from_hex(0x2A2A32),
            secondary_marking_color: Rgb8::from_hex(0xF09B4B),
            inner_ear_color: Rgb8::from_hex(0x1B1B21),
            left_eye_color: Rgb8::from_hex(0xC8F04D),
            right_eye_color: Rgb8::from_hex(0xC8F04D),
            whisker_color: Rgb8::from_hex(0xB8BCC4),
            nose_color: Rgb8::from_hex(0x28242B),
            unknown_fields: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PeripheralStyle {
    pub keyboard_case: Rgb8,
    pub keycap_base: Rgb8,
    pub keycap_accent: Rgb8,
    pub keycap_highlight: Rgb8,
    pub keycap_legend_color: Rgb8,
    pub show_keycap_legends: bool,
    pub mouse_body: Rgb8,
    pub mouse_wheel: Rgb8,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, serde_json::Value>,
}

impl Default for PeripheralStyle {
    fn default() -> Self {
        Self {
            keyboard_case: Rgb8::from_hex(0x1B1C22),
            keycap_base: Rgb8::from_hex(0xF4F5F7),
            keycap_accent: Rgb8::from_hex(0xF5A623),
            keycap_highlight: Rgb8::from_hex(0xB9F27C),
            keycap_legend_color: Rgb8::from_hex(0x2E323B),
            show_keycap_legends: false,
            mouse_body: Rgb8::from_hex(0xF4F5F7),
            mouse_wheel: Rgb8::from_hex(0x555B66),
            unknown_fields: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatProfile {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub base_preset_id: Option<String>,
    pub appearance: CatAppearance,
    pub peripherals: PeripheralStyle,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, serde_json::Value>,
}

impl Default for CatProfile {
    fn default() -> Self {
        builtin_presets().into_iter().next().unwrap()
    }
}

impl CatProfile {
    pub fn is_builtin(&self) -> bool {
        self.id.starts_with("builtin:")
    }

    pub fn sanitize(&mut self) {
        if self.schema_version == 0 {
            self.schema_version = CURRENT_SCHEMA_VERSION;
        }
        self.name = self.name.trim().chars().take(40).collect();
        if self.name.is_empty() {
            self.name = "Custom Cat".to_string();
        }
        if self.id.trim().is_empty() {
            self.id = new_user_profile_id();
        }
    }
}

/// Generate a collision-resistant local user profile ID (`user:<hex-uuid>`) without pulling `getrandom`.
pub fn new_user_profile_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let mut s1 = nanos ^ (pid << 32) ^ seq.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    s1 = (s1 ^ (s1 >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    s1 = (s1 ^ (s1 >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    s1 ^= s1 >> 31;
    let mut s2 = s1.wrapping_add(0x9E37_79B9_7F4A_7C15);
    s2 = (s2 ^ (s2 >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    s2 = (s2 ^ (s2 >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    s2 ^= s2 >> 31;
    format!(
        "user:{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (s1 >> 32) as u32,
        ((s1 >> 16) & 0xFFFF) as u16,
        (s1 & 0x0FFF) as u16,
        (((s2 >> 48) & 0x3FFF) | 0x8000) as u16,
        s2 & 0xFFFF_FFFF_FFFF
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackdropMode {
    #[default]
    Transparent,
    Solid(Rgb8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameHideScope {
    #[default]
    SameMonitor,
    AnyMonitor,
    Disabled,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowAnchor {
    pub monitor_device_name: String,
    /// Normalized anchor position within monitor work area `[0.0, 1.0]`.
    pub norm_x: f32,
    pub norm_y: f32,
    pub fallback_abs_x: i32,
    pub fallback_abs_y: i32,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, serde_json::Value>,
}

impl Default for WindowAnchor {
    fn default() -> Self {
        Self {
            monitor_device_name: String::new(),
            norm_x: 0.96,
            norm_y: 0.96,
            fallback_abs_x: 100,
            fallback_abs_y: 100,
            unknown_fields: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    pub toggle_click_through: String,
    pub toggle_privacy: String,
    pub open_designer: String,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, serde_json::Value>,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            toggle_click_through: "Ctrl+Alt+L".to_string(),
            toggle_privacy: "Ctrl+Alt+P".to_string(),
            open_designer: "Ctrl+Alt+D".to_string(),
            unknown_fields: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema_version: u32,
    pub active_profile_id: String,
    pub scale: f32,
    pub opacity: f32,
    pub click_through_locked: bool,
    pub showcase_mode: bool,
    pub show_floating_popups: bool,
    pub capture_stage_enabled: bool,
    pub chroma_key_color: Rgb8,
    pub chroma_hard_edge: bool,
    pub auto_start: bool,
    pub game_hide_scope: GameHideScope,
    pub privacy_heuristics_enabled: bool,
    pub game_process_allowlist: Vec<String>,
    pub game_process_denylist: Vec<String>,
    pub anchor: WindowAnchor,
    pub hotkeys: HotkeyConfig,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, serde_json::Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            active_profile_id: "builtin:midnight".to_string(),
            scale: 1.0,
            opacity: 1.0,
            click_through_locked: false,
            showcase_mode: false,
            show_floating_popups: false,
            capture_stage_enabled: false,
            chroma_key_color: Rgb8::from_hex(0x00B140),
            chroma_hard_edge: false,
            auto_start: false,
            game_hide_scope: GameHideScope::SameMonitor,
            privacy_heuristics_enabled: true,
            game_process_allowlist: Vec::new(),
            game_process_denylist: Vec::new(),
            anchor: WindowAnchor::default(),
            hotkeys: HotkeyConfig::default(),
            unknown_fields: BTreeMap::new(),
        }
    }
}

impl Config {
    /// Enforce hard numeric and list ceilings against malformed/DoS configs.
    pub fn sanitize(&mut self) {
        if self.schema_version == 0 {
            self.schema_version = CURRENT_SCHEMA_VERSION;
        }
        if !self.scale.is_finite() {
            self.scale = 1.0;
        }
        self.scale = self.scale.clamp(0.4, 2.4);

        if !self.opacity.is_finite() {
            self.opacity = 1.0;
        }
        self.opacity = self.opacity.clamp(0.05, 1.0);

        if !self.anchor.norm_x.is_finite() {
            self.anchor.norm_x = 0.96;
        }
        if !self.anchor.norm_y.is_finite() {
            self.anchor.norm_y = 0.96;
        }
        self.anchor.norm_x = self.anchor.norm_x.clamp(0.0, 1.0);
        self.anchor.norm_y = self.anchor.norm_y.clamp(0.0, 1.0);
        self.anchor.monitor_device_name = self
            .anchor
            .monitor_device_name
            .chars()
            .take(64)
            .collect();

        if self.active_profile_id.trim().is_empty() {
            self.active_profile_id = "builtin:midnight".to_string();
        }
        if self.game_hide_scope == GameHideScope::Unknown {
            self.game_hide_scope = GameHideScope::SameMonitor;
        }
        self.game_process_allowlist.truncate(100);
        self.game_process_denylist.truncate(100);
        for s in &mut self.game_process_allowlist {
            *s = s.trim().chars().take(64).collect();
        }
        for s in &mut self.game_process_denylist {
            *s = s.trim().chars().take(64).collect();
        }
    }

    pub fn effective_click_through(&self) -> bool {
        self.click_through_locked || self.opacity < MIN_VISIBLE_OPACITY_INTERACTIVE
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectI32 {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl RectI32 {
    pub fn width(self) -> i32 {
        (self.right - self.left).max(0)
    }

    pub fn height(self) -> i32 {
        (self.bottom - self.top).max(0)
    }
}

/// Clamp an overlay rectangle of `(win_w, win_h)` so 100% of it stays inside `work_area`.
pub fn clamp_rect_to_work_area(
    x: i32,
    y: i32,
    win_w: i32,
    win_h: i32,
    work_area: RectI32,
) -> (i32, i32) {
    let max_x = (work_area.right - win_w).max(work_area.left);
    let max_y = (work_area.bottom - win_h).max(work_area.top);
    (x.clamp(work_area.left, max_x), y.clamp(work_area.top, max_y))
}

/// Compute top-left position from normalized monitor anchor ratios.
pub fn anchor_to_work_area_pos(
    anchor: &WindowAnchor,
    win_w: i32,
    win_h: i32,
    work_area: RectI32,
) -> (i32, i32) {
    let avail_w = (work_area.width() - win_w).max(0) as f32;
    let avail_h = (work_area.height() - win_h).max(0) as f32;
    let x = work_area.left + (avail_w * anchor.norm_x.clamp(0.0, 1.0)).round() as i32;
    let y = work_area.top + (avail_h * anchor.norm_y.clamp(0.0, 1.0)).round() as i32;
    clamp_rect_to_work_area(x, y, win_w, win_h, work_area)
}

/// Convert top-left position back into normalized work-area ratios.
pub fn work_area_pos_to_norm(
    x: i32,
    y: i32,
    win_w: i32,
    win_h: i32,
    work_area: RectI32,
) -> (f32, f32) {
    let avail_w = (work_area.width() - win_w).max(1) as f32;
    let avail_h = (work_area.height() - win_h).max(1) as f32;
    let nx = ((x - work_area.left) as f32 / avail_w).clamp(0.0, 1.0);
    let ny = ((y - work_area.top) as f32 / avail_h).clamp(0.0, 1.0);
    (nx, ny)
}

/// Stable 64-bit FNV-1a hash over ASCII-lowercased bytes (used for config deduplication
/// and hashed process-basename comparisons without shipping plaintext AV-trigger strings).
pub fn fnv1a_64_lower(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in input.bytes() {
        let lower = b.to_ascii_lowercase();
        hash ^= lower as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub fn fnv1a_64_bytes(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Resolve an active profile ID against built-ins and user-cloned profiles, falling back
/// safely to `builtin:midnight` if missing (for forward/rollback safety).
pub fn resolve_profile(active_id: &str, user_profiles: &[CatProfile]) -> CatProfile {
    if let Some(p) = user_profiles.iter().find(|p| p.id == active_id) {
        return p.clone();
    }
    let builtins = builtin_presets();
    if let Some(p) = builtins.iter().find(|p| p.id == active_id) {
        return p.clone();
    }
    builtins.into_iter().next().unwrap()
}

fn preset(
    slug: &str,
    name: &str,
    appearance: CatAppearance,
    peripherals: PeripheralStyle,
) -> CatProfile {
    CatProfile {
        schema_version: CURRENT_SCHEMA_VERSION,
        id: format!("builtin:{slug}"),
        name: name.to_string(),
        base_preset_id: None,
        appearance,
        peripherals,
        unknown_fields: BTreeMap::new(),
    }
}

/// 15 immutable built-in coat presets (matching LittleOne references plus classic coats).
pub fn builtin_presets() -> Vec<CatProfile> {
    let std_kb = PeripheralStyle::default();
    let warm_kb = PeripheralStyle {
        keyboard_case: Rgb8::from_hex(0x2B2523),
        keycap_base: Rgb8::from_hex(0xFFF6EB),
        keycap_accent: Rgb8::from_hex(0xF28B30),
        keycap_highlight: Rgb8::from_hex(0xFFAE42),
        mouse_body: Rgb8::from_hex(0xFFF6EB),
        ..PeripheralStyle::default()
    };
    let pastel_kb = PeripheralStyle {
        keyboard_case: Rgb8::from_hex(0x3D354B),
        keycap_base: Rgb8::from_hex(0xFDF7FA),
        keycap_accent: Rgb8::from_hex(0xF299B5),
        keycap_highlight: Rgb8::from_hex(0x9BE8D8),
        mouse_body: Rgb8::from_hex(0xFDF7FA),
        ..PeripheralStyle::default()
    };
    let yellow_mouse_kb = PeripheralStyle {
        mouse_body: Rgb8::from_hex(0xFFE047),
        ..PeripheralStyle::default()
    };

    vec![
        // 1. Midnight (matches user's 6 LittleOne screenshots)
        preset("midnight", "Midnight", CatAppearance::default(), std_kb.clone()),
        // 2. Luna (grey tabby stripes, pink inner ears, amber + blue heterochromia)
        preset(
            "luna",
            "Luna",
            CatAppearance {
                base_coat: Rgb8::from_hex(0x8E939E),
                marking_pattern: MarkingPattern::TabbyStripes,
                marking_color: Rgb8::from_hex(0x525763),
                secondary_marking_color: Rgb8::from_hex(0xD5D8DF),
                inner_ear_color: Rgb8::from_hex(0xF2A6B3),
                left_eye_color: Rgb8::from_hex(0xF6A637),
                right_eye_color: Rgb8::from_hex(0x69B4FF),
                whisker_color: Rgb8::from_hex(0xEAECEF),
                nose_color: Rgb8::from_hex(0xE88F9E),
                ..CatAppearance::default()
            },
            std_kb.clone(),
        ),
        // 3. Mochi (pure white, pink inner ears, sky blue eyes)
        preset(
            "mochi",
            "Mochi",
            CatAppearance {
                base_coat: Rgb8::from_hex(0xF6F6F8),
                marking_pattern: MarkingPattern::Solid,
                marking_color: Rgb8::from_hex(0xE4E5EC),
                inner_ear_color: Rgb8::from_hex(0xF6B2BE),
                left_eye_color: Rgb8::from_hex(0x75C3FF),
                right_eye_color: Rgb8::from_hex(0x75C3FF),
                whisker_color: Rgb8::from_hex(0x9BA1AD),
                nose_color: Rgb8::from_hex(0xF299AA),
                ..CatAppearance::default()
            },
            pastel_kb.clone(),
        ),
        // 4. Pepper (charcoal tuxedo with crisp white bib/paws, yellow-green eyes)
        preset(
            "pepper",
            "Pepper",
            CatAppearance {
                base_coat: Rgb8::from_hex(0x212329),
                marking_pattern: MarkingPattern::Tuxedo,
                marking_color: Rgb8::from_hex(0xF4F5F7),
                inner_ear_color: Rgb8::from_hex(0x463940),
                left_eye_color: Rgb8::from_hex(0xD5F256),
                right_eye_color: Rgb8::from_hex(0xD5F256),
                whisker_color: Rgb8::from_hex(0xECEEF2),
                nose_color: Rgb8::from_hex(0x4B444B),
                ..CatAppearance::default()
            },
            yellow_mouse_kb.clone(),
        ),
        // 5. Marmalade (orange & white bicolor with green eyes)
        preset(
            "marmalade",
            "Marmalade",
            CatAppearance {
                base_coat: Rgb8::from_hex(0xEC9440),
                marking_pattern: MarkingPattern::Bicolor,
                marking_color: Rgb8::from_hex(0xFAF8F5),
                inner_ear_color: Rgb8::from_hex(0xF6B0A8),
                left_eye_color: Rgb8::from_hex(0x9FE254),
                right_eye_color: Rgb8::from_hex(0x9FE254),
                whisker_color: Rgb8::from_hex(0xFFFDF8),
                nose_color: Rgb8::from_hex(0xE88D84),
                ..CatAppearance::default()
            },
            warm_kb.clone(),
        ),
        // 6. Siamese (cream body, dark seal points, sapphire blue eyes)
        preset(
            "siamese",
            "Siamese",
            CatAppearance {
                base_coat: Rgb8::from_hex(0xEFE3D1),
                marking_pattern: MarkingPattern::Siamese,
                marking_color: Rgb8::from_hex(0x4B3930),
                inner_ear_color: Rgb8::from_hex(0x6E5548),
                left_eye_color: Rgb8::from_hex(0x4DA6FF),
                right_eye_color: Rgb8::from_hex(0x4DA6FF),
                whisker_color: Rgb8::from_hex(0x8F7E70),
                nose_color: Rgb8::from_hex(0x3D2D25),
                ..CatAppearance::default()
            },
            warm_kb.clone(),
        ),
        // 7. Calico (white base with orange & charcoal patches, hazel eyes)
        preset(
            "calico",
            "Calico",
            CatAppearance {
                base_coat: Rgb8::from_hex(0xF7F4EF),
                marking_pattern: MarkingPattern::Calico,
                marking_color: Rgb8::from_hex(0xE98B38),
                secondary_marking_color: Rgb8::from_hex(0x2B292E),
                inner_ear_color: Rgb8::from_hex(0xF3B0B5),
                left_eye_color: Rgb8::from_hex(0xD5B848),
                right_eye_color: Rgb8::from_hex(0xD5B848),
                whisker_color: Rgb8::from_hex(0x9A9690),
                nose_color: Rgb8::from_hex(0xEC949B),
                ..CatAppearance::default()
            },
            warm_kb.clone(),
        ),
        // 8. Tortie (dark chocolate mottled with warm amber/gold)
        preset(
            "tortie",
            "Tortie",
            CatAppearance {
                base_coat: Rgb8::from_hex(0x2B2220),
                marking_pattern: MarkingPattern::Tortie,
                marking_color: Rgb8::from_hex(0xDE8835),
                secondary_marking_color: Rgb8::from_hex(0xF0C066),
                inner_ear_color: Rgb8::from_hex(0x5B413A),
                left_eye_color: Rgb8::from_hex(0xF4B43A),
                right_eye_color: Rgb8::from_hex(0xF4B43A),
                whisker_color: Rgb8::from_hex(0xC9B9AA),
                nose_color: Rgb8::from_hex(0x523B35),
                ..CatAppearance::default()
            },
            std_kb.clone(),
        ),
        // 9. Ginger Tabby
        preset(
            "ginger_tabby",
            "Ginger Tabby",
            CatAppearance {
                base_coat: Rgb8::from_hex(0xF09B4B),
                marking_pattern: MarkingPattern::TabbyStripes,
                marking_color: Rgb8::from_hex(0xC6631E),
                inner_ear_color: Rgb8::from_hex(0xF8B7AD),
                left_eye_color: Rgb8::from_hex(0xF2C249),
                right_eye_color: Rgb8::from_hex(0xF2C249),
                whisker_color: Rgb8::from_hex(0xFFF3E2),
                nose_color: Rgb8::from_hex(0xE88D82),
                ..CatAppearance::default()
            },
            warm_kb.clone(),
        ),
        // 10. Russian Blue
        preset(
            "russian_blue",
            "Russian Blue",
            CatAppearance {
                base_coat: Rgb8::from_hex(0x6D7B8D),
                marking_pattern: MarkingPattern::Solid,
                marking_color: Rgb8::from_hex(0x515F70),
                inner_ear_color: Rgb8::from_hex(0x9E8D9B),
                left_eye_color: Rgb8::from_hex(0x7AE582),
                right_eye_color: Rgb8::from_hex(0x7AE582),
                whisker_color: Rgb8::from_hex(0xDCE3EB),
                nose_color: Rgb8::from_hex(0x4B5563),
                ..CatAppearance::default()
            },
            std_kb.clone(),
        ),
        // 11. Cream Puff
        preset(
            "cream_puff",
            "Cream Puff",
            CatAppearance {
                base_coat: Rgb8::from_hex(0xF4DFC0),
                marking_pattern: MarkingPattern::Bicolor,
                marking_color: Rgb8::from_hex(0xFFFBF5),
                inner_ear_color: Rgb8::from_hex(0xF7B8B8),
                left_eye_color: Rgb8::from_hex(0xE8B948),
                right_eye_color: Rgb8::from_hex(0xE8B948),
                whisker_color: Rgb8::from_hex(0xB8A690),
                nose_color: Rgb8::from_hex(0xEF9D9D),
                ..CatAppearance::default()
            },
            warm_kb.clone(),
        ),
        // 12. Oreo (black with white mittens & snout)
        preset(
            "oreo",
            "Oreo",
            CatAppearance {
                base_coat: Rgb8::from_hex(0x181A20),
                marking_pattern: MarkingPattern::Mittens,
                marking_color: Rgb8::from_hex(0xF8F9FA),
                inner_ear_color: Rgb8::from_hex(0x413942),
                left_eye_color: Rgb8::from_hex(0x8AF2B8),
                right_eye_color: Rgb8::from_hex(0x8AF2B8),
                whisker_color: Rgb8::from_hex(0xEAECEF),
                nose_color: Rgb8::from_hex(0x3A353D),
                ..CatAppearance::default()
            },
            std_kb.clone(),
        ),
        // 13. Bengal (golden spotted)
        preset(
            "bengal",
            "Bengal",
            CatAppearance {
                base_coat: Rgb8::from_hex(0xD9A45B),
                marking_pattern: MarkingPattern::Spotted,
                marking_color: Rgb8::from_hex(0x5E3A1E),
                inner_ear_color: Rgb8::from_hex(0xE9B49B),
                left_eye_color: Rgb8::from_hex(0x98E84E),
                right_eye_color: Rgb8::from_hex(0x98E84E),
                whisker_color: Rgb8::from_hex(0xF7E8D0),
                nose_color: Rgb8::from_hex(0xB86A54),
                ..CatAppearance::default()
            },
            warm_kb,
        ),
        // 14. Snowshoe (cocoa coat with white mittens + ice blue eyes)
        preset(
            "snowshoe",
            "Snowshoe",
            CatAppearance {
                base_coat: Rgb8::from_hex(0x745849),
                marking_pattern: MarkingPattern::Mittens,
                marking_color: Rgb8::from_hex(0xF6F1EA),
                inner_ear_color: Rgb8::from_hex(0xA48176),
                left_eye_color: Rgb8::from_hex(0x88D1FF),
                right_eye_color: Rgb8::from_hex(0x88D1FF),
                whisker_color: Rgb8::from_hex(0xEFE8DF),
                nose_color: Rgb8::from_hex(0x4F3B32),
                ..CatAppearance::default()
            },
            std_kb,
        ),
        // 15. Sakura (pastel pink tabby with lilac/pink heterochromia)
        preset(
            "sakura",
            "Sakura",
            CatAppearance {
                base_coat: Rgb8::from_hex(0xFADCE6),
                marking_pattern: MarkingPattern::TabbyStripes,
                marking_color: Rgb8::from_hex(0xE89BB6),
                inner_ear_color: Rgb8::from_hex(0xF5A8C4),
                left_eye_color: Rgb8::from_hex(0xB994FF),
                right_eye_color: Rgb8::from_hex(0xFF8CB8),
                whisker_color: Rgb8::from_hex(0xA8889A),
                nose_color: Rgb8::from_hex(0xE97EA4),
                ..CatAppearance::default()
            },
            pastel_kb,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifteen_unique_builtin_presets() {
        let presets = builtin_presets();
        assert_eq!(presets.len(), 15);
        let mut ids = std::collections::BTreeSet::new();
        for p in &presets {
            assert!(p.id.starts_with("builtin:"));
            assert!(ids.insert(p.id.clone()), "duplicate preset {}", p.id);
        }
    }

    #[test]
    fn lossless_unknown_fields_roundtrip_for_rollback() {
        let raw = r#"{
            "schema_version": 1,
            "active_profile_id": "user:1234",
            "scale": 1.25,
            "future_v2_feature_flag": true,
            "future_nested": { "depth": 42 }
        }"#;
        let mut cfg: Config = serde_json::from_str(raw).unwrap();
        cfg.sanitize();
        let serialized = serde_json::to_string(&cfg).unwrap();
        let reparsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(reparsed["future_v2_feature_flag"], true);
        assert_eq!(reparsed["future_nested"]["depth"], 42);
        // Unknown user profile resolves cleanly to built-in fallback
        let prof = resolve_profile(&cfg.active_profile_id, &[]);
        assert_eq!(prof.id, "builtin:midnight");
    }

    #[test]
    fn sanitize_clamps_dos_values() {
        let mut cfg = Config {
            scale: f32::INFINITY,
            opacity: -5.0,
            game_process_allowlist: vec!["x".repeat(200); 500],
            ..Config::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.scale, 1.0);
        assert!((cfg.opacity - 0.05).abs() < 1e-6);
        assert!(cfg.effective_click_through());
        assert_eq!(cfg.game_process_allowlist.len(), 100);
        assert_eq!(cfg.game_process_allowlist[0].len(), 64);
    }

    #[test]
    fn monitor_clamp_prevents_offscreen_stranding() {
        let work = RectI32 {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        assert_eq!(clamp_rect_to_work_area(-500, 3000, 360, 300, work), (0, 740));
        assert_eq!(
            clamp_rect_to_work_area(4000, -100, 360, 300, work),
            (1560, 0)
        );
    }
}
