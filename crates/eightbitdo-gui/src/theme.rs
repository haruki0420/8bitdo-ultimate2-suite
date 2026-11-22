//! Design tokens, fonts, and the single egui style setup.
//!
//! Direction: 「燻しスレート — 実機の素材と、ひとつの光」. Surfaces are stacked
//! smoked-slate layers separated by hairlines and a 1 px top edge light; light
//! (the LED) is reserved for state and selection. The primary key is an
//! off-white key instead of a colored fill.
//!
//! All colors, spacing, radii, and type decisions live here. Contrast values
//! are documented next to the tokens and asserted in tests: text must reach
//! WCAG AA (4.5:1), and interactive outlines/focus rings must reach SC 1.4.11
//! (3:1) against the surfaces they sit on.

use std::sync::Arc;

use eframe::egui::{
    self,
    text::{LayoutJob, TextFormat},
    Color32, FontData, FontDefinitions, FontFamily, FontId, Rounding, Stroke, TextStyle,
};

// ---------------------------------------------------------------------------
// Surfaces and structure
// ---------------------------------------------------------------------------

/// Window background and central panel.
pub const BG: Color32 = Color32::from_rgb(0x0B, 0x0D, 0x10);
/// Top bar and left rail (1.05:1 on [`BG`]).
pub const SURFACE_1: Color32 = Color32::from_rgb(0x11, 0x14, 0x19);
/// Normal surface on a panel, row hover (1.13:1 on [`BG`]).
pub const SURFACE_2: Color32 = Color32::from_rgb(0x17, 0x1B, 0x21);
/// Cards and pressed surfaces (1.11:1 on [`SURFACE_2`]).
pub const SURFACE_3: Color32 = Color32::from_rgb(0x1F, 0x24, 0x2B);
/// Structural separators and non-interactive boundaries (1.32:1 on [`SURFACE_2`]).
pub const HAIRLINE: Color32 = Color32::from_rgb(0x2B, 0x31, 0x39);
/// 1 px top edge of a surface; the translucent-shell rim light (1.51:1).
pub const EDGE_LIGHT: Color32 = Color32::from_rgb(0x33, 0x3A, 0x44);
/// Outline of interactive controls (3.39:1 on [`SURFACE_2`], 3.07:1 on
/// [`SURFACE_3`]). The brief's `#616B77` falls to 2.88:1 on cards, below
/// WCAG SC 1.4.11, so the line is one step lighter.
pub const CONTROL_LINE: Color32 = Color32::from_rgb(0x66, 0x6F, 0x7B);
/// Selected-row ground, paired with the LED left bar (1.23:1 on [`SURFACE_2`]).
pub const SELECT_TINT: Color32 = Color32::from_rgb(0x16, 0x30, 0x2F);

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// Titles and emphasized values (14.32:1 on [`SURFACE_2`]).
pub const TEXT: Color32 = Color32::from_rgb(0xE7, 0xEA, 0xEE);
/// Body and control labels (10.26:1 on [`SURFACE_2`]).
pub const TEXT_BODY: Color32 = Color32::from_rgb(0xC2, 0xC8, 0xD0);
/// Meta, section labels (5.43:1 on [`SURFACE_2`]).
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x8A, 0x91, 0x9B);
/// Disabled text only (5.23:1 on [`SURFACE_2`], 4.72:1 on [`SURFACE_3`]).
/// One step lighter than the brief's `#7C8490`, which drops to 4.13:1 on
/// cards; disabled keys inside the empty-state card must still reach AA.
pub const TEXT_DISABLED: Color32 = Color32::from_rgb(0x86, 0x8E, 0x9A);

// ---------------------------------------------------------------------------
// Light (state and selection only)
// ---------------------------------------------------------------------------

/// Connected, selected, focus ring, stick position.
pub const LED: Color32 = Color32::from_rgb(0x6F, 0xE7, 0xD8);
/// Unlit ring outline.
pub const LED_DIM: Color32 = Color32::from_rgb(0x2F, 0x5B, 0x57);
/// Scanning and stale-data warning (9.31:1).
pub const AMBER: Color32 = Color32::from_rgb(0xE9, 0xB6, 0x5C);
/// Errors (7.88:1).
pub const RED: Color32 = Color32::from_rgb(0xFF, 0x8F, 0x8F);

// ---------------------------------------------------------------------------
// Primary key (off-white, not light)
// ---------------------------------------------------------------------------

/// Primary key ground.
pub const KEY_LIGHT: Color32 = Color32::from_rgb(0xE9, 0xEC, 0xEF);
/// Primary key hover ground.
pub const KEY_LIGHT_HOVER: Color32 = Color32::from_rgb(0xF5, 0xF6, 0xF7);
/// Primary key pressed ground.
pub const KEY_LIGHT_PRESSED: Color32 = Color32::from_rgb(0xD3, 0xD7, 0xDC);
/// Text on the primary key (16.41:1).
pub const ON_KEY_LIGHT: Color32 = Color32::from_rgb(0x0B, 0x0D, 0x10);

// ---------------------------------------------------------------------------
// Geometry (4 px grid)
// ---------------------------------------------------------------------------

/// 4 px step (line gap).
pub const SPACE_1: f32 = 4.0;
/// 8 px step (heading to body, inline gap).
pub const SPACE_2: f32 = 8.0;
/// 16 px step (block gap).
pub const SPACE_3: f32 = 16.0;
/// 24 px step (content padding).
pub const SPACE_4: f32 = 24.0;
/// 32 px step (section gap, content inset from the rail).
pub const SPACE_5: f32 = 32.0;

/// Left rail width. 14 px Japanese text fits about 18 characters.
pub const SIDE_PANEL_WIDTH: f32 = 264.0;
/// Top bar height.
pub const TOP_BAR_HEIGHT: f32 = 56.0;
/// Tab row height.
pub const TAB_HEIGHT: f32 = 40.0;
/// Primary and secondary key height.
pub const KEY_HEIGHT: f32 = 40.0;
/// Primary key minimum width.
pub const KEY_MIN_WIDTH: f32 = 168.0;
/// Quiet rail row height.
pub const ROW_HEIGHT: f32 = 36.0;
/// Device list row height.
pub const DEVICE_ROW_HEIGHT: f32 = 44.0;
/// Content column maximum width (45–65 Japanese characters per line).
pub const CONTENT_MAX_WIDTH: f32 = 760.0;
/// Gap between the rail boundary and the content column.
pub const CONTENT_INSET: f32 = 32.0;
/// Card inner padding.
pub const CARD_PADDING: f32 = 20.0;
/// Signature ring diameter.
pub const RING_DIAMETER: f32 = 176.0;
/// Stick ring diameter.
pub const STICK_DIAMETER: f32 = 168.0;
/// Status ring diameter in the top bar / rail.
pub const STATUS_RING: f32 = 8.0;
/// Focus ring width.
pub const FOCUS_RING_WIDTH: f32 = 2.0;
/// Trigger bar height.
pub const TRIGGER_BAR_HEIGHT: f32 = 6.0;

// ---------------------------------------------------------------------------
// Radii (only four values)
// ---------------------------------------------------------------------------

/// Cards and the window.
pub const RADIUS_CARD: f32 = 12.0;
/// Keys (primary and secondary).
pub const RADIUS_KEY: f32 = 8.0;
/// Tab hover surface.
pub const RADIUS_TAB: f32 = 6.0;
/// Mode badges; the ring is a circle.
pub const RADIUS_BADGE: f32 = 4.0;

// ---------------------------------------------------------------------------
// Typography
// ---------------------------------------------------------------------------

/// Page title size (22 Bold).
pub const FONT_TITLE: f32 = 22.0;
/// Section label size (12 Medium, +0.5 px tracking).
pub const FONT_SECTION: f32 = 12.0;
/// Body size (14 Regular, 24 px line height).
pub const FONT_BODY: f32 = 14.0;
/// Control size (14 Medium).
pub const FONT_CONTROL: f32 = 14.0;
/// Data size (12 Mono).
pub const FONT_DATA: f32 = 12.0;
/// Meta size (12 Regular).
pub const FONT_META: f32 = 12.0;

/// Extra letter spacing for section labels (12 px × 0.04 em).
pub const TRACKING_SECTION: f32 = 0.5;
/// Extra letter spacing for the page title.
pub const TRACKING_TITLE: f32 = 0.4;
/// Extra letter spacing for data.
pub const TRACKING_DATA: f32 = 0.25;
/// Body line height (14 px × 1.71).
pub const LINE_HEIGHT_BODY: f32 = 24.0;
/// Meta line height.
pub const LINE_HEIGHT_META: f32 = 18.0;
/// Control line height.
pub const LINE_HEIGHT_CONTROL: f32 = 20.0;

/// Name of the bundled medium font family.
pub const MEDIUM_FAMILY_NAME: &str = "noto-jp-medium";
/// Name of the bundled bold font family.
pub const BOLD_FAMILY_NAME: &str = "noto-jp-bold";

const REGULAR_BYTES: &[u8] = include_bytes!("../assets/fonts/NotoSansJP-Regular.otf");
const MEDIUM_BYTES: &[u8] = include_bytes!("../assets/fonts/NotoSansJP-Medium.otf");
const BOLD_BYTES: &[u8] = include_bytes!("../assets/fonts/NotoSansJP-Bold.otf");

/// Install the bundled fonts and the smoked-slate theme. Call once.
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
    for (name, bytes) in [
        ("noto-jp-regular", REGULAR_BYTES),
        ("noto-jp-medium", MEDIUM_BYTES),
        ("noto-jp-bold", BOLD_BYTES),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), FontData::from_static(bytes));
    }

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
        FontFamily::Name(Arc::from(MEDIUM_FAMILY_NAME)),
        vec!["noto-jp-medium".to_owned(), "noto-jp-regular".to_owned()],
    );
    fonts.families.insert(
        FontFamily::Name(Arc::from(BOLD_FAMILY_NAME)),
        vec!["noto-jp-bold".to_owned(), "noto-jp-regular".to_owned()],
    );
    fonts
}

fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}

/// Regular proportional font id.
#[must_use]
pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Medium font id (section labels and control text).
#[must_use]
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(Arc::from(MEDIUM_FAMILY_NAME)))
}

/// Bold font id (page title and emphasized values only).
#[must_use]
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(Arc::from(BOLD_FAMILY_NAME)))
}

/// Build a [`LayoutJob`] with explicit tracking and line height.
///
/// `RichText` cannot set these, so body copy and section labels go through this
/// helper.
#[must_use]
pub fn text_job(
    text: &str,
    font: FontId,
    color: Color32,
    letter_spacing: f32,
    line_height: Option<f32>,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: letter_spacing,
            line_height,
            ..Default::default()
        },
    );
    job
}

/// Page title job (22 Bold, +0.4 px).
#[must_use]
pub fn job_title(text: &str) -> LayoutJob {
    text_job(text, bold(FONT_TITLE), TEXT, TRACKING_TITLE, None)
}

/// Section label job (12 Medium, +0.5 px).
#[must_use]
pub fn job_section(text: &str) -> LayoutJob {
    text_job(
        text,
        medium(FONT_SECTION),
        TEXT_MUTED,
        TRACKING_SECTION,
        Some(LINE_HEIGHT_META),
    )
}

/// Body job (14 Regular, 24 px line height).
#[must_use]
pub fn job_body(text: &str) -> LayoutJob {
    text_job(
        text,
        regular(FONT_BODY),
        TEXT_BODY,
        _ZERO,
        Some(LINE_HEIGHT_BODY),
    )
}

/// Body job with an explicit color.
#[must_use]
pub fn job_body_color(text: &str, color: Color32) -> LayoutJob {
    text_job(
        text,
        regular(FONT_BODY),
        color,
        _ZERO,
        Some(LINE_HEIGHT_BODY),
    )
}

/// Meta job (12 Regular, 18 px line height).
#[must_use]
pub fn job_meta(text: &str) -> LayoutJob {
    text_job(
        text,
        regular(FONT_META),
        TEXT_MUTED,
        _ZERO,
        Some(LINE_HEIGHT_META),
    )
}

/// Meta job with an explicit color.
#[must_use]
pub fn job_meta_color(text: &str, color: Color32) -> LayoutJob {
    text_job(
        text,
        regular(FONT_META),
        color,
        _ZERO,
        Some(LINE_HEIGHT_META),
    )
}

/// Control job (14 Medium).
#[must_use]
pub fn job_control(text: &str, color: Color32) -> LayoutJob {
    text_job(
        text,
        medium(FONT_CONTROL),
        color,
        _ZERO,
        Some(LINE_HEIGHT_CONTROL),
    )
}

/// Data job (12 Mono, +0.25 px).
#[must_use]
pub fn job_data(text: &str, color: Color32) -> LayoutJob {
    text_job(
        text,
        FontId::monospace(FONT_DATA),
        color,
        TRACKING_DATA,
        Some(LINE_HEIGHT_META),
    )
}

/// Strong value job (14 Bold).
#[must_use]
pub fn job_strong(text: &str) -> LayoutJob {
    text_job(
        text,
        bold(FONT_BODY),
        TEXT,
        _ZERO,
        Some(LINE_HEIGHT_CONTROL),
    )
}

const _ZERO: f32 = 0.0;

/// Build the complete egui style. Pure so tests can inspect the tokens.
#[must_use]
pub fn build_style() -> egui::Style {
    let mut style = egui::Style {
        visuals: visuals(),
        ..egui::Style::default()
    };
    style.spacing.item_spacing = egui::vec2(SPACE_2, SPACE_2);
    style.spacing.button_padding = egui::vec2(14.0, 10.0);
    style.spacing.interact_size = egui::vec2(44.0, KEY_HEIGHT);
    style.spacing.menu_margin = egui::Margin::same(SPACE_2);
    style.spacing.indent = 16.0;
    style.spacing.slider_width = 120.0;
    style.spacing.combo_width = 120.0;
    let section = TextStyle::Name(Arc::from("Section"));
    let control = TextStyle::Name(Arc::from("Control"));
    let data = TextStyle::Name(Arc::from("Data"));
    let meta = TextStyle::Name(Arc::from("Meta"));
    let title = TextStyle::Name(Arc::from("Title"));
    style.text_styles = std::collections::BTreeMap::from([
        (TextStyle::Small, regular(FONT_META)),
        (TextStyle::Body, regular(FONT_BODY)),
        (TextStyle::Monospace, FontId::monospace(FONT_DATA)),
        (TextStyle::Button, medium(FONT_CONTROL)),
        (TextStyle::Heading, bold(FONT_TITLE)),
        (section, medium(FONT_SECTION)),
        (control, medium(FONT_CONTROL)),
        (data, FontId::monospace(FONT_DATA)),
        (meta, regular(FONT_META)),
        (title, bold(FONT_TITLE)),
    ]);
    style
}

/// Smoked-slate visuals. Every widget state is set explicitly; egui defaults
/// are not reused.
#[must_use]
pub fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.dark_mode = true;
    visuals.panel_fill = SURFACE_1;
    visuals.window_fill = SURFACE_1;
    visuals.window_stroke = Stroke::new(1.0_f32, HAIRLINE);
    visuals.window_rounding = Rounding::same(RADIUS_CARD);
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    visuals.menu_rounding = Rounding::same(RADIUS_KEY);
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = SURFACE_2;
    visuals.code_bg_color = SURFACE_2;
    visuals.override_text_color = Some(TEXT_BODY);
    visuals.hyperlink_color = LED;
    visuals.warn_fg_color = AMBER;
    visuals.error_fg_color = RED;
    visuals.selection = egui::style::Selection {
        bg_fill: SELECT_TINT,
        stroke: Stroke::new(1.0_f32, LED),
    };
    visuals.button_frame = true;
    visuals.collapsing_header_frame = false;
    visuals.striped = false;
    visuals.slider_trailing_fill = true;
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);

    let widgets = &mut visuals.widgets;
    widgets.noninteractive = egui::style::WidgetVisuals {
        bg_fill: SURFACE_1,
        weak_bg_fill: SURFACE_1,
        bg_stroke: Stroke::new(1.0_f32, HAIRLINE),
        fg_stroke: Stroke::new(1.0_f32, TEXT_BODY),
        rounding: Rounding::same(RADIUS_TAB),
        expansion: 0.0,
    };
    widgets.inactive = egui::style::WidgetVisuals {
        bg_fill: SURFACE_2,
        weak_bg_fill: SURFACE_2,
        bg_stroke: Stroke::new(1.0_f32, CONTROL_LINE),
        fg_stroke: Stroke::new(1.0_f32, TEXT_BODY),
        rounding: Rounding::same(RADIUS_KEY),
        expansion: 0.0,
    };
    widgets.hovered = egui::style::WidgetVisuals {
        bg_fill: SURFACE_3,
        weak_bg_fill: SURFACE_3,
        bg_stroke: Stroke::new(1.0_f32, CONTROL_LINE),
        fg_stroke: Stroke::new(1.0_f32, TEXT),
        rounding: Rounding::same(RADIUS_KEY),
        expansion: 0.0,
    };
    widgets.active = egui::style::WidgetVisuals {
        bg_fill: SELECT_TINT,
        weak_bg_fill: SELECT_TINT,
        bg_stroke: Stroke::new(1.0_f32, LED),
        fg_stroke: Stroke::new(1.0_f32, TEXT),
        rounding: Rounding::same(RADIUS_KEY),
        expansion: 0.0,
    };
    widgets.open = widgets.inactive;
    visuals
}

// ---------------------------------------------------------------------------
// Color helpers (pure, used by drawing and tests)
// ---------------------------------------------------------------------------

/// Interpolate two opaque colors (for 100 ms hover fades).
#[must_use]
pub fn lerp_color(from: Color32, to: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t) as u8;
    Color32::from_rgb(
        mix(from.r(), to.r()),
        mix(from.g(), to.g()),
        mix(from.b(), to.b()),
    )
}

fn linear_channel(channel: u8) -> f32 {
    let value = f32::from(channel) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance of an sRGB color.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{self, Key, Lang};

    #[test]
    fn text_styles_use_the_declared_scale() {
        let style = build_style();
        assert_eq!(style.text_styles[&TextStyle::Body].size, FONT_BODY);
        assert_eq!(style.text_styles[&TextStyle::Button].size, FONT_CONTROL);
        assert_eq!(style.text_styles[&TextStyle::Heading].size, FONT_TITLE);
        assert_eq!(
            style.text_styles[&TextStyle::Heading].family,
            FontFamily::Name(Arc::from(BOLD_FAMILY_NAME))
        );
        assert_eq!(
            style.text_styles[&TextStyle::Name(Arc::from("Section"))].family,
            FontFamily::Name(Arc::from(MEDIUM_FAMILY_NAME)),
            "section labels use Medium, not Bold"
        );
        assert_eq!(
            style.text_styles[&TextStyle::Name(Arc::from("Control"))].family,
            FontFamily::Name(Arc::from(MEDIUM_FAMILY_NAME))
        );
    }

    #[test]
    fn body_and_section_jobs_carry_line_height_and_tracking() {
        let body = job_body("設定 2768 バイトを読み取ります。");
        assert_eq!(body.sections[0].format.line_height, Some(LINE_HEIGHT_BODY));
        assert!(body.sections[0].format.extra_letter_spacing.abs() < f32::EPSILON);

        let section = job_section("接続状態");
        assert_eq!(
            section.sections[0].format.line_height,
            Some(LINE_HEIGHT_META)
        );
        assert!((section.sections[0].format.extra_letter_spacing - TRACKING_SECTION).abs() < 0.001);
        assert_eq!(section.sections[0].format.font_id, medium(FONT_SECTION));
    }

    #[test]
    fn body_muted_and_disabled_text_meet_wcag_aa_on_every_surface() {
        for surface in [SURFACE_1, SURFACE_2, SURFACE_3, BG] {
            for text in [TEXT, TEXT_BODY, TEXT_MUTED, TEXT_DISABLED] {
                assert!(
                    contrast_ratio(text, surface) >= 4.5,
                    "{text:?} on {surface:?} is below AA"
                );
            }
        }
        assert!(contrast_ratio(ON_KEY_LIGHT, KEY_LIGHT) >= 7.0);
    }

    #[test]
    fn interactive_outlines_and_focus_rings_meet_sc_1_4_11() {
        for surface in [SURFACE_1, SURFACE_2, SURFACE_3, BG] {
            assert!(
                contrast_ratio(CONTROL_LINE, surface) >= 3.0,
                "control outline on {surface:?} is below 3:1"
            );
            assert!(
                contrast_ratio(LED, surface) >= 3.0,
                "focus ring on {surface:?} is below 3:1"
            );
        }
    }

    #[test]
    fn surfaces_form_a_monotonic_smoked_slate_stack() {
        let levels = [
            relative_luminance(BG),
            relative_luminance(SURFACE_1),
            relative_luminance(SURFACE_2),
            relative_luminance(SURFACE_3),
        ];
        assert!(
            levels.windows(2).all(|pair| pair[0] < pair[1]),
            "surface luminance must increase from BG to SURFACE_3: {levels:?}"
        );
        assert!(contrast_ratio(EDGE_LIGHT, SURFACE_2) >= 1.4);
        assert!(contrast_ratio(HAIRLINE, SURFACE_2) >= 1.2);
    }

    #[test]
    fn state_colors_are_readable_on_surfaces() {
        for color in [LED, AMBER, RED] {
            assert!(contrast_ratio(color, SURFACE_1) >= 4.5, "{color:?}");
            assert!(contrast_ratio(color, SURFACE_2) >= 4.5, "{color:?}");
            assert!(contrast_ratio(color, SURFACE_3) >= 4.5, "{color:?}");
        }
        assert!(contrast_ratio(LED_DIM, BG) < 3.0, "unlit ring stays quiet");
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn geometry_fits_the_minimum_window_with_one_content_column() {
        assert_eq!(SIDE_PANEL_WIDTH, 264.0);
        assert!(CONTENT_INSET >= 32.0);
        let central = 900.0 - SIDE_PANEL_WIDTH;
        assert!(central - 2.0 * CONTENT_INSET >= 500.0);
        assert!(CONTENT_MAX_WIDTH >= 700.0 && CONTENT_MAX_WIDTH <= 800.0);
        assert!(KEY_HEIGHT >= 40.0 && KEY_MIN_WIDTH >= 168.0);
        assert!(TAB_HEIGHT >= 40.0);
        assert!(ROW_HEIGHT >= 36.0);
        assert!(TOP_BAR_HEIGHT >= 56.0);
        for radius in [RADIUS_CARD, RADIUS_KEY, RADIUS_TAB, RADIUS_BADGE] {
            assert!(radius > 0.0);
        }
        let mut radii = vec![RADIUS_CARD, RADIUS_KEY, RADIUS_TAB, RADIUS_BADGE];
        radii.sort_by(f32::total_cmp);
        radii.dedup_by(|a, b| (*a - *b).abs() < f32::EPSILON);
        assert_eq!(radii.len(), 4, "only four radii are allowed");
    }

    #[test]
    fn bundled_fonts_cover_every_localized_string_in_all_three_weights() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let mut missing: Vec<(String, &'static str)> = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            ctx.fonts(|fonts| {
                for &key in Key::ALL {
                    for lang in [Lang::Ja, Lang::En] {
                        let text = i18n::t(lang, key);
                        for (font_id, label) in [
                            (FontId::proportional(FONT_BODY), "regular"),
                            (medium(FONT_CONTROL), "medium"),
                            (bold(FONT_TITLE), "bold"),
                            (FontId::monospace(FONT_DATA), "mono"),
                        ] {
                            if !fonts.has_glyphs(&font_id, text) {
                                missing.push((text.to_owned(), label));
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
