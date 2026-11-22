//! Design tokens, fonts, and the single egui style setup.
//!
//! All color, spacing, radius, and typography decisions live here so the UI
//! code ([`crate::app`]) only composes widgets. The style is installed once in
//! `App::new`.
//!
//! Contrast values are documented next to the tokens and asserted in tests:
//! body/muted/disabled text must reach WCAG AA (4.5:1) on the panel surface,
//! and borders must stay visible against both the window and card surfaces.

use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Rounding, Stroke, TextStyle,
};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

/// Primary accent, Windows 11 Fluent blue.
pub const ACCENT: Color32 = Color32::from_rgb(0x00, 0x67, 0xC0);
/// Accent used while hovering a primary action.
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x1A, 0x77, 0xCC);
/// Accent used while pressing a primary action.
pub const ACCENT_PRESSED: Color32 = Color32::from_rgb(0x00, 0x5A, 0xA8);
/// Text on top of [`ACCENT`] (5.7:1).
pub const ON_ACCENT: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);

/// Window background (the outermost surface).
pub const BG: Color32 = Color32::from_rgb(0x12, 0x12, 0x12);
/// Panel surface: top bar, side rail, central background uses [`BG`].
pub const PANEL: Color32 = Color32::from_rgb(0x1C, 0x1C, 0x1C);
/// Card surface on top of [`PANEL`] or [`BG`].
pub const CARD: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
/// Card surface while hovered.
pub const CARD_HOVER: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x2E);
/// Resting control background (secondary buttons, chips).
pub const CONTROL: Color32 = Color32::from_rgb(0x2C, 0x2C, 0x2C);
/// Hovered control background.
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);

/// Visible panel/card outline (1.8:1 on [`PANEL`], 1.6:1 on [`CARD`]).
pub const BORDER: Color32 = Color32::from_rgb(0x45, 0x45, 0x45);
/// Stronger outline for focused or hovered boundaries.
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x5A, 0x5A, 0x5A);

/// Primary text (12.7:1 on [`PANEL`]).
pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE6, 0xE6);
/// Body text (9.9:1 on [`PANEL`]).
pub const TEXT_BODY: Color32 = Color32::from_rgb(0xC8, 0xC8, 0xC8);
/// Muted text: hints and labels (5.6:1 on [`PANEL`]).
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x9A, 0x9A, 0x9A);
/// Text color for disabled controls **before** egui fades a disabled `Ui`.
///
/// egui blends everything painted by a disabled `Ui` 50% toward
/// `noninteractive.weak_bg_fill` (the panel color here), which turns this into
/// about `#8D8D8D`. That stays above 4.5:1 on both the panel and the slightly
/// lighter card surface. See [`fade_disabled`].
pub const DISABLED_TEXT_PRE_FADE: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
/// Outline for disabled controls before the fade, about 1.9:1 afterwards.
pub const DISABLED_BORDER_PRE_FADE: Color32 = Color32::from_rgb(0x70, 0x70, 0x70);

/// Connected status (10.0:1 on [`PANEL`]).
pub const STATUS_GREEN: Color32 = Color32::from_rgb(0x78, 0xDC, 0x8C);
/// Scanning/connecting status and warnings (9.5:1 on [`PANEL`]).
pub const STATUS_AMBER: Color32 = Color32::from_rgb(0xE6, 0xBE, 0x5A);
/// Error status and error banner accent (7.4:1 on [`PANEL`]).
pub const STATUS_RED: Color32 = Color32::from_rgb(0xFF, 0x8C, 0x8C);
/// Error banner background.
pub const ERROR_BG: Color32 = Color32::from_rgb(0x3A, 0x1F, 0x1F);
/// Warning banner background.
pub const WARNING_BG: Color32 = Color32::from_rgb(0x33, 0x2B, 0x17);
/// Status pill / badge background.
pub const PILL_BG: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x2A);
/// Accent-tinted badge background (App Mode).
pub const BADGE_BG: Color32 = Color32::from_rgb(0x14, 0x30, 0x4A);
/// Link and accent-colored text (5.2:1 on [`PANEL`]).
pub const LINK: Color32 = Color32::from_rgb(0x6C, 0xB4, 0xF0);

// ---------------------------------------------------------------------------
// Geometry (8 px grid)
// ---------------------------------------------------------------------------

/// Base corner radius for buttons, cards, and fields.
pub const RADIUS: f32 = 6.0;
/// 4 px spacing step.
pub const SPACE_1: f32 = 4.0;
/// 8 px spacing step (line gap, icon gap).
pub const SPACE_2: f32 = 8.0;
/// 16 px spacing step (card inner padding, group gap).
pub const SPACE_3: f32 = 16.0;
/// 24 px spacing step (section gap, panel padding).
pub const SPACE_4: f32 = 24.0;

/// Width of the left rail. 320 px leaves room for 14 px Japanese labels.
pub const SIDE_PANEL_WIDTH: f32 = 320.0;
/// Tab navigation height.
pub const TAB_HEIGHT: f32 = 36.0;
/// Standard action button height.
pub const BUTTON_HEIGHT: f32 = 32.0;
/// Minimum width of the primary connect CTA.
pub const CTA_WIDTH: f32 = 160.0;
/// Height of the primary connect CTA.
pub const CTA_HEIGHT: f32 = 36.0;
/// Minimum height of the onboarding status card.
pub const CARD_MIN_HEIGHT: f32 = 120.0;
/// Height of one scan-result device row.
pub const DEVICE_ROW_HEIGHT: f32 = 44.0;
/// Status dot diameter.
pub const STATUS_DOT_SIZE: f32 = 8.0;
/// Stick preview widget edge length.
pub const STICK_SIZE: f32 = 168.0;
/// Focus ring width.
pub const FOCUS_RING_WIDTH: f32 = 2.0;

// ---------------------------------------------------------------------------
// Typography (14 / 16 / 20 / 24)
// ---------------------------------------------------------------------------

/// Small text size (same as body; kept as a named step).
pub const FONT_SIZE_SMALL: f32 = 14.0;
/// Body text size.
pub const FONT_SIZE_BODY: f32 = 14.0;
/// Sub-heading, section label, and button text size.
pub const FONT_SIZE_SUBHEAD: f32 = 16.0;
/// Page and card heading size.
pub const FONT_SIZE_HEADING: f32 = 20.0;
/// Window title / brand size.
pub const FONT_SIZE_TITLE: f32 = 24.0;

/// Name of the bundled bold font family.
pub const BOLD_FAMILY_NAME: &str = "noto-jp-bold";

const REGULAR_BYTES: &[u8] = include_bytes!("../assets/fonts/NotoSansJP-Regular.otf");
const BOLD_BYTES: &[u8] = include_bytes!("../assets/fonts/NotoSansJP-Bold.otf");

/// Install the bundled fonts and the dark theme. Call once from `App::new`.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_style(build_style());
}

/// Register the bundled Noto Sans JP subsets.
///
/// The proportional family puts Japanese first so Latin and Japanese share
/// metrics. The monospace family keeps Hack first (hex/table alignment) and
/// inserts Japanese right after it as a fallback.
#[must_use]
pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "noto-jp-regular".to_owned(),
        FontData::from_static(REGULAR_BYTES),
    );
    fonts
        .font_data
        .insert("noto-jp-bold".to_owned(), FontData::from_static(BOLD_BYTES));

    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .expect("default proportional family exists")
        .insert(0, "noto-jp-regular".to_owned());

    let monospace = fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .expect("default monospace family exists");
    monospace.insert(monospace.len().min(1), "noto-jp-regular".to_owned());

    fonts.families.insert(
        FontFamily::Name(Arc::from(BOLD_FAMILY_NAME)),
        vec!["noto-jp-bold".to_owned(), "noto-jp-regular".to_owned()],
    );
    fonts
}

fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}

/// Bold font id for headings and section labels.
#[must_use]
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(Arc::from(BOLD_FAMILY_NAME)))
}

/// Regular proportional font id.
#[must_use]
pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Build the complete egui style. Pure so tests can inspect the tokens.
#[must_use]
pub fn build_style() -> egui::Style {
    let mut style = egui::Style {
        visuals: visuals(),
        ..egui::Style::default()
    };
    style.spacing.item_spacing = egui::vec2(SPACE_2, SPACE_2);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size = egui::vec2(40.0, BUTTON_HEIGHT);
    style.spacing.menu_margin = egui::Margin::same(SPACE_2);
    style.spacing.indent = 18.0;
    style.spacing.slider_width = 120.0;
    style.spacing.combo_width = 120.0;
    style.text_styles = std::collections::BTreeMap::from([
        (TextStyle::Small, regular(FONT_SIZE_SMALL)),
        (TextStyle::Body, regular(FONT_SIZE_BODY)),
        (TextStyle::Monospace, FontId::monospace(FONT_SIZE_BODY)),
        (TextStyle::Button, regular(FONT_SIZE_SUBHEAD)),
        (TextStyle::Heading, bold(FONT_SIZE_HEADING)),
        (TextStyle::Name(Arc::from("Title")), bold(FONT_SIZE_TITLE)),
        (
            TextStyle::Name(Arc::from("Section")),
            bold(FONT_SIZE_SUBHEAD),
        ),
    ]);
    style
}

/// Dark visuals built from the tokens above.
#[must_use]
pub fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.dark_mode = true;
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.window_rounding = Rounding::same(8.0);
    visuals.menu_rounding = Rounding::same(RADIUS);
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = Color32::from_rgb(0x22, 0x22, 0x22);
    visuals.code_bg_color = Color32::from_rgb(0x22, 0x22, 0x22);
    visuals.override_text_color = Some(TEXT_BODY);
    visuals.hyperlink_color = LINK;
    visuals.warn_fg_color = STATUS_AMBER;
    visuals.error_fg_color = STATUS_RED;
    visuals.selection = egui::style::Selection {
        bg_fill: ACCENT,
        stroke: Stroke::new(1.0_f32, Color32::WHITE),
    };
    visuals.button_frame = true;
    visuals.collapsing_header_frame = false;
    visuals.striped = true;
    visuals.slider_trailing_fill = true;
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);

    let rounding = Rounding::same(RADIUS);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive = egui::style::WidgetVisuals {
        bg_fill: PANEL,
        weak_bg_fill: PANEL,
        bg_stroke: Stroke::new(1.0_f32, BORDER),
        fg_stroke: Stroke::new(1.0_f32, TEXT_BODY),
        rounding,
        expansion: 0.0,
    };
    widgets.inactive = egui::style::WidgetVisuals {
        bg_fill: CONTROL,
        weak_bg_fill: CONTROL,
        bg_stroke: Stroke::new(1.0_f32, BORDER),
        fg_stroke: Stroke::new(1.0_f32, TEXT),
        rounding,
        expansion: 0.0,
    };
    widgets.hovered = egui::style::WidgetVisuals {
        bg_fill: CONTROL_HOVER,
        weak_bg_fill: CONTROL_HOVER,
        bg_stroke: Stroke::new(1.0_f32, BORDER_STRONG),
        fg_stroke: Stroke::new(1.0_f32, Color32::WHITE),
        rounding,
        expansion: 1.0,
    };
    widgets.active = egui::style::WidgetVisuals {
        bg_fill: ACCENT,
        weak_bg_fill: ACCENT,
        bg_stroke: Stroke::new(1.0_f32, ACCENT),
        fg_stroke: Stroke::new(1.0_f32, ON_ACCENT),
        rounding,
        expansion: 1.0,
    };
    widgets.open = widgets.inactive;
    visuals
}

// ---------------------------------------------------------------------------
// Contrast helpers (pure, used by tests)
// ---------------------------------------------------------------------------

fn linear_channel(channel: u8) -> f32 {
    let value = f32::from(channel) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance of a sRGB color.
#[must_use]
pub fn relative_luminance(color: Color32) -> f32 {
    0.2126 * linear_channel(color.r())
        + 0.7152 * linear_channel(color.g())
        + 0.0722 * linear_channel(color.b())
}

/// WCAG contrast ratio between two colors (1.0..=21.0).
#[must_use]
pub fn contrast_ratio(first: Color32, second: Color32) -> f32 {
    let first = relative_luminance(first);
    let second = relative_luminance(second);
    let (lighter, darker) = if first >= second {
        (first, second)
    } else {
        (second, first)
    };
    (lighter + 0.05) / (darker + 0.05)
}

/// Reproduce egui's disabled fade: 50% blend toward the panel color.
///
/// egui calls this `tint_color_towards` with
/// `noninteractive.weak_bg_fill` (set to [`PANEL`] by [`visuals`]).
#[must_use]
pub fn fade_disabled(color: Color32) -> Color32 {
    Color32::from_rgb(
        color.r() / 2 + PANEL.r() / 2,
        color.g() / 2 + PANEL.g() / 2,
        color.b() / 2 + PANEL.b() / 2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_styles_use_the_four_declared_sizes() {
        let style = build_style();
        let mut sizes: Vec<u32> = style
            .text_styles
            .values()
            .map(|font_id| font_id.size.round() as u32)
            .collect();
        sizes.sort_unstable();
        sizes.dedup();
        assert_eq!(sizes, vec![14, 16, 20, 24]);

        let heading = style.text_styles[&TextStyle::Heading].clone();
        assert_eq!(heading.size, FONT_SIZE_HEADING);
        assert_eq!(
            heading.family,
            FontFamily::Name(Arc::from(BOLD_FAMILY_NAME))
        );
    }

    #[test]
    fn body_muted_and_disabled_text_meet_wcag_aa() {
        assert!(contrast_ratio(TEXT_BODY, PANEL) >= 7.0);
        assert!(contrast_ratio(TEXT, PANEL) >= 7.0);
        assert!(contrast_ratio(TEXT_MUTED, PANEL) >= 4.5);
        assert!(
            contrast_ratio(fade_disabled(DISABLED_TEXT_PRE_FADE), PANEL) >= 4.5,
            "disabled text faded to {:?} must still reach AA on the panel",
            fade_disabled(DISABLED_TEXT_PRE_FADE)
        );
        assert!(
            contrast_ratio(fade_disabled(DISABLED_TEXT_PRE_FADE), CARD) >= 4.5,
            "disabled text faded to {:?} must still reach AA on a card",
            fade_disabled(DISABLED_TEXT_PRE_FADE)
        );
        assert!(contrast_ratio(ON_ACCENT, ACCENT) >= 4.5);
        assert!(contrast_ratio(LINK, PANEL) >= 4.5);
    }

    #[test]
    fn panels_cards_and_borders_are_distinguishable() {
        assert_ne!(BG, PANEL);
        assert_ne!(PANEL, CARD);
        assert_ne!(CARD, CARD_HOVER);
        assert!(
            contrast_ratio(BORDER, PANEL) >= 1.5,
            "panel border {BORDER:?} is too faint on {PANEL:?}"
        );
        assert!(contrast_ratio(BORDER, CARD) >= 1.5);
        assert!(contrast_ratio(fade_disabled(DISABLED_BORDER_PRE_FADE), PANEL) >= 1.5);
    }

    #[test]
    fn status_colors_are_readable_on_the_panel() {
        for color in [STATUS_GREEN, STATUS_AMBER, STATUS_RED] {
            assert!(contrast_ratio(color, PANEL) >= 4.5, "{color:?}");
            assert!(contrast_ratio(color, PILL_BG) >= 4.5, "{color:?}");
        }
    }

    #[test]
    // These tokens are compile-time constants on purpose; the assertions are
    // the guard against a future edit breaking the 8 px grid or touch sizes.
    #[allow(clippy::assertions_on_constants)]
    fn geometry_follows_the_eight_pixel_grid_and_minimum_sizes() {
        for space in [SPACE_1, SPACE_2, SPACE_3, SPACE_4] {
            assert!((space % 4.0).abs() < f32::EPSILON);
        }
        assert_eq!(SPACE_2, 2.0 * SPACE_1);
        assert_eq!(SPACE_3, 4.0 * SPACE_1);
        assert_eq!(SPACE_4, 6.0 * SPACE_1);
        assert!(TAB_HEIGHT >= 36.0);
        assert!(BUTTON_HEIGHT >= 32.0);
        assert!(CTA_WIDTH >= 160.0 && CTA_HEIGHT >= 36.0);
        assert!(SIDE_PANEL_WIDTH >= 300.0);
        assert!(DEVICE_ROW_HEIGHT >= 40.0);
        assert!(RADIUS >= 4.0 && RADIUS <= 8.0);
    }

    #[test]
    fn bundled_fonts_cover_japanese_latin_and_monospace_fallback() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let mut result = (false, false, false);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            result = ctx.fonts(|fonts| {
                let proportional = FontId::proportional(FONT_SIZE_BODY);
                let monospace = FontId::monospace(FONT_SIZE_BODY);
                let bold_font = bold(FONT_SIZE_HEADING);
                (
                    fonts.has_glyphs(&proportional, "接続 未接続 8BitDo"),
                    fonts.has_glyphs(&bold_font, "はじめかた"),
                    fonts.has_glyphs(&monospace, "設定 0x0AD0"),
                )
            });
        });
        assert!(result.0, "proportional font misses Japanese or Latin");
        assert!(result.1, "bold font misses heading glyphs");
        assert!(result.2, "monospace family misses Japanese fallback");
    }

    #[test]
    fn bundled_fonts_cover_every_localized_string() {
        use crate::i18n::{self, Key, Lang};

        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let mut missing: Vec<(&'static str, &'static str, Lang)> = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            ctx.fonts(|fonts| {
                for &key in Key::ALL {
                    for lang in [Lang::Ja, Lang::En] {
                        let text = i18n::t(lang, key);
                        for font_id in [
                            FontId::proportional(FONT_SIZE_BODY),
                            FontId::monospace(FONT_SIZE_BODY),
                            bold(FONT_SIZE_HEADING),
                        ] {
                            if !fonts.has_glyphs(&font_id, text) {
                                missing.push((text, "font", lang));
                                break;
                            }
                        }
                    }
                }
            });
        });
        assert!(
            missing.is_empty(),
            "the bundled font subsets are missing UI characters: {missing:?}"
        );
    }
}
