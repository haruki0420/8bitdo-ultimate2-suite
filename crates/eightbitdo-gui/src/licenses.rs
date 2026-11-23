//! Embedded third-party license texts shown in the in-app licenses screen.
//!
//! Binary distribution must ship the full texts for the bundled fonts (OFL 1.1
//! for Noto Sans JP / NotoEmoji, the Ubuntu Font Licence 1.0 for Ubuntu-Light,
//! MIT + Bitstream Vera + DejaVu for Hack, MIT for the emoji icon font) and for
//! this application (MIT OR Apache-2.0). The files live in
//! `crates/eightbitdo-gui/assets/licenses/`; they are the unmodified upstream
//! texts copied from `epaint_default_fonts` 0.29.1 and the Noto Sans JP
//! distribution.
//!
//! `docs/DISTRIBUTION_POLICY.md` §4.4 is the release checklist that keeps this
//! module and `THIRD_PARTY_LICENSES.md` in sync.

/// Full text shown for a bundled license.
#[derive(Debug, Clone, Copy)]
pub struct LicenseText {
    /// Heading shown above the text.
    pub name: &'static str,
    /// Body of the license (the upstream file, unmodified).
    pub body: &'static str,
}

/// Noto Sans JP subset, SIL Open Font License 1.1.
pub const NOTO_SANS_JP_OFL: &str = include_str!("../assets/licenses/OFL-NotoSansJP.txt");
/// NotoEmoji (egui default font), SIL Open Font License 1.1.
pub const NOTO_EMOJI_OFL: &str = include_str!("../assets/licenses/OFL-NotoEmoji-Ubuntu.txt");
/// Ubuntu-Light (egui default font), Ubuntu Font Licence 1.0.
pub const UBUNTU_UFL: &str = include_str!("../assets/licenses/UFL-Ubuntu.txt");
/// Hack (egui default font), MIT + Bitstream Vera + DejaVu.
pub const HACK_LICENSE: &str = include_str!("../assets/licenses/Hack-BitstreamVera.txt");
/// emoji-icon-font (egui default font), MIT.
pub const EMOJI_ICON_MIT: &str = include_str!("../assets/licenses/emoji-icon-font-MIT.txt");

/// Every bundled license text, in display order.
pub const ALL: &[LicenseText] = &[
    LicenseText {
        name: "Noto Sans JP — SIL Open Font License 1.1",
        body: NOTO_SANS_JP_OFL,
    },
    LicenseText {
        name: "NotoEmoji — SIL Open Font License 1.1",
        body: NOTO_EMOJI_OFL,
    },
    LicenseText {
        name: "Ubuntu-Light — Ubuntu Font Licence 1.0",
        body: UBUNTU_UFL,
    },
    LicenseText {
        name: "Hack — MIT / Bitstream Vera / DejaVu",
        body: HACK_LICENSE,
    },
    LicenseText {
        name: "emoji-icon-font — MIT",
        body: EMOJI_ICON_MIT,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_license_text_is_embedded_and_non_empty() {
        assert!(!ALL.is_empty());
        for text in ALL {
            assert!(!text.name.is_empty(), "a license has no heading");
            assert!(
                text.body.len() > 500,
                "{} looks truncated ({} bytes)",
                text.name,
                text.body.len()
            );
            assert!(
                text.body.contains("Copyright") || text.body.contains("copyright"),
                "{} has no copyright line",
                text.name
            );
        }
    }

    #[test]
    fn the_noto_license_keeps_the_reserved_font_name_notice() {
        assert!(NOTO_SANS_JP_OFL.contains("Reserved Font Name"));
    }

    #[test]
    fn the_ubuntu_license_is_the_ufl_not_the_ofl() {
        assert!(UBUNTU_UFL.contains("Ubuntu Font Licence") || UBUNTU_UFL.contains("UBUNTU FONT"));
        assert!(!UBUNTU_UFL.contains("SIL OPEN FONT LICENSE"));
    }
}
