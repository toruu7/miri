use pet_core::{CatAppearance, MarkingPattern, PeripheralStyle, Rgb8};
use tiny_skia::Color;

/// 5-band luminance-adaptive cel-shaded palette derived deterministically from v1
/// `CatAppearance` and `PeripheralStyle` RGB fields (zero config schema changes).
///
/// Uses translucent multiply-style cool cel shadow & soft rim highlight overlays so
/// markings (Siamese dark points, tabby stripes, calico patches, tuxedo bibs) are
/// shaded naturally without opaque color mismatch.
#[derive(Debug, Clone, Copy)]
pub struct CelPalette {
    pub is_dark_coat: bool,
    pub base: Color,
    /// Translucent cool-indigo cel shadow overlay (preserves underlying coat markings!).
    pub cel_shadow_overlay: Color,
    /// Subtle translucent crown/rim sheen overlay.
    pub cel_highlight_overlay: Color,
    pub contact_ao: Color,
    pub outline: Color,
    pub rim_sep: Color,
    pub tail_fill: Rgb8,
    pub tail_curtain_rim: Color,
    pub lid_stroke: Color,
    pub blush: Color,
    pub yawn_fill: Color,
    pub yawn_tongue: Color,
    pub yawn_rim: Color,
    pub nose_fill: Color,
    pub mouth_stroke: Color,
}

#[inline]
pub fn rgb_color(rgb: Rgb8, alpha: f32) -> Color {
    Color::from_rgba8(
        rgb.r,
        rgb.g,
        rgb.b,
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

impl CelPalette {
    pub fn derive(app: &CatAppearance, per: &PeripheralStyle) -> Self {
        let base_rgb = app.base_coat;
        let lum = base_rgb.luminance();
        let is_dark_coat = lum < 0.22;
        let is_very_light = lum > 0.78;

        let outline_rgb = if is_dark_coat {
            Rgb8::new(7, 8, 12)
        } else if is_very_light {
            base_rgb.scale(0.32).lerp(Rgb8::new(30, 32, 46), 0.48)
        } else {
            base_rgb.scale(0.34).lerp(Rgb8::new(18, 20, 32), 0.36)
        };

        // Translucent cool cel-shadow & subtle sheen (respects all coat markings cleanly!)
        let cel_shadow_overlay = if is_dark_coat {
            Color::from_rgba(0.01, 0.01, 0.03, 0.42).unwrap()
        } else if is_very_light {
            Color::from_rgba(0.24, 0.28, 0.46, 0.14).unwrap()
        } else {
            Color::from_rgba(0.10, 0.11, 0.24, 0.16).unwrap()
        };

        let cel_highlight_overlay = if is_dark_coat {
            Color::from_rgba(0.54, 0.62, 0.84, 0.16).unwrap()
        } else if is_very_light {
            Color::from_rgba(1.0, 1.0, 1.0, 0.38).unwrap()
        } else {
            Color::from_rgba(1.0, 0.99, 0.96, 0.22).unwrap()
        };

        let rim_sep = if is_dark_coat {
            rgb_color(Rgb8::new(196, 206, 224), 0.56)
        } else {
            rgb_color(outline_rgb, 0.86)
        };

        // Tail base color (Siamese uses dark seal points) + luminance-guaranteed privacy curtain rim
        let tail_fill = match app.marking_pattern {
            MarkingPattern::Siamese => app.marking_color,
            _ => base_rgb,
        };
        let tail_lum = tail_fill.luminance();
        let tail_dark = tail_lum < 0.24;

        let kb_lum = per.keyboard_case.luminance();
        let tail_curtain_rim = if tail_dark {
            rgb_color(Rgb8::new(206, 216, 234), 0.74)
        } else if (tail_lum - kb_lum).abs() < 0.24 {
            if tail_lum > 0.50 {
                rgb_color(Rgb8::new(40, 44, 58), 0.92)
            } else {
                rgb_color(Rgb8::new(212, 220, 234), 0.78)
            }
        } else {
            rgb_color(outline_rgb, 0.90)
        };

        // Face mask luminance (Siamese has dark face mask on cream coat!)
        let face_lum = match app.marking_pattern {
            MarkingPattern::Siamese => app.marking_color.luminance(),
            _ => lum,
        };
        let lid_stroke = if face_lum < 0.24 {
            let avg_eye = app.left_eye_color.lerp(app.right_eye_color, 0.5);
            rgb_color(avg_eye.lerp(Rgb8::new(230, 236, 245), 0.30), 0.94)
        } else {
            rgb_color(Rgb8::new(30, 32, 42), 0.94)
        };

        // Coat-adaptive cheek blush (visible on black, white, orange, AND pastel pink Sakura)
        let is_pink_coat =
            base_rgb.r > 220 && base_rgb.b > 185 && base_rgb.r as i32 - base_rgb.g as i32 > 14;
        let is_orange_coat =
            base_rgb.r > 205 && base_rgb.g > 110 && base_rgb.g < 195 && base_rgb.b < 115;
        let blush = if is_pink_coat {
            rgb_color(Rgb8::new(224, 88, 134), 0.66)
        } else if is_orange_coat {
            rgb_color(Rgb8::new(240, 100, 96), 0.62)
        } else if is_dark_coat {
            rgb_color(Rgb8::new(248, 132, 166), 0.70)
        } else {
            rgb_color(Rgb8::new(248, 140, 166), 0.62)
        };

        // Yawn interior + tongue + subtle contrast rim (legible even on #141418 Midnight)
        let yawn_fill = rgb_color(Rgb8::new(146, 52, 76), 0.98);
        let yawn_tongue = rgb_color(Rgb8::new(238, 138, 160), 0.96);
        let yawn_rim = if face_lum < 0.24 {
            rgb_color(Rgb8::new(214, 184, 196), 0.85)
        } else {
            rgb_color(Rgb8::new(42, 30, 38), 0.90)
        };

        let nose_fill = if face_lum < 0.16 {
            rgb_color(app.nose_color.lerp(Rgb8::new(98, 92, 112), 0.45), 0.92)
        } else {
            rgb_color(app.nose_color, 0.96)
        };
        let mouth_stroke = if face_lum < 0.22 {
            rgb_color(Rgb8::new(178, 184, 200), 0.68)
        } else {
            rgb_color(Rgb8::new(46, 42, 50), 0.80)
        };

        let contact_ao = if is_dark_coat {
            Color::from_rgba(0.01, 0.01, 0.03, 0.40).unwrap()
        } else {
            Color::from_rgba(0.10, 0.12, 0.24, 0.11).unwrap()
        };

        Self {
            is_dark_coat,
            base: rgb_color(base_rgb, 1.0),
            cel_shadow_overlay,
            cel_highlight_overlay,
            contact_ao,
            outline: rgb_color(outline_rgb, 1.0),
            rim_sep,
            tail_fill,
            tail_curtain_rim,
            lid_stroke,
            blush,
            yawn_fill,
            yawn_tongue,
            yawn_rim,
            nose_fill,
            mouth_stroke,
        }
    }
}
