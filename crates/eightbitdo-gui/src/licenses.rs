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

/// This application's MIT license (repository root, unmodified).
pub const APP_LICENSE_MIT: &str = include_str!("../../../LICENSE-MIT");
/// This application's Apache-2.0 license (repository root, unmodified).
pub const APP_LICENSE_APACHE: &str = include_str!("../../../LICENSE-APACHE");

/// Noto Sans JP subset, SIL Open Font License 1.1.
pub const NOTO_SANS_JP_OFL: &str = include_str!("../assets/licenses/OFL-NotoSansJP.txt");
/// NotoEmoji (egui default font), SIL Open Font License 1.1.
pub const NOTO_EMOJI_OFL: &str = include_str!("../assets/licenses/OFL-NotoEmoji.txt");
/// Ubuntu-Light (egui default font), Ubuntu Font Licence 1.0.
pub const UBUNTU_UFL: &str = include_str!("../assets/licenses/UFL-Ubuntu.txt");
/// Hack (egui default font), MIT + Bitstream Vera + DejaVu.
pub const HACK_LICENSE: &str = include_str!("../assets/licenses/Hack-BitstreamVera.txt");
/// emoji-icon-font (egui default font), MIT.
pub const EMOJI_ICON_MIT: &str = include_str!("../assets/licenses/emoji-icon-font-MIT.txt");

/// Copyright lines taken from the bundled font `name` tables.
///
/// The upstream `OFL.txt` / `UFL.txt` in `epaint_default_fonts` 0.29.1 are the
/// license bodies only and contain no holder line, so the holder must be
/// displayed explicitly (OFL 1.1 §2, UFL attribution). Values are read from the
/// fonts' name ID 0 tables, unmodified.
pub const ATTRIBUTIONS: &[(&str, &str)] = &[
    (
        "Noto Sans JP (subset)",
        "Copyright 2014-2021 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source'",
    ),
    (
        "NotoEmoji",
        "Copyright 2013 Google Inc. All Rights Reserved.",
    ),
    (
        "Ubuntu-Light",
        "Copyright 2011 Canonical Ltd.  Licensed under the Ubuntu Font Licence 1.0",
    ),
    (
        "Hack",
        "Copyright (c) 2018 Source Foundry Authors / Copyright (c) 2003 by Bitstream, Inc. All Rights Reserved.",
    ),
    (
        "emoji-icon-font",
        "Copyright (c) 2014 John Slegers",
    ),
];

/// Every bundled license text, in display order.
pub const ALL: &[LicenseText] = &[
    LicenseText {
        name: "eightbitdo-suite — MIT License",
        body: APP_LICENSE_MIT,
    },
    LicenseText {
        name: "eightbitdo-suite — Apache License 2.0",
        body: APP_LICENSE_APACHE,
    },
    LicenseText {
        // `(subset)` is required by DISTRIBUTION_POLICY §4.4: the bundled face
        // is a Modified Version and must not be mistaken for official Noto.
        name: "Noto Sans JP (subset) — SIL Open Font License 1.1",
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
    fn every_bundled_item_is_present() {
        assert_eq!(ALL.len(), 7, "app MIT + app Apache + 5 font texts");
        let names: Vec<&str> = ALL.iter().map(|text| text.name).collect();
        assert!(names.iter().any(|name| name.contains("MIT License")));
        assert!(names.iter().any(|name| name.contains("Apache License 2.0")));
        assert!(names
            .iter()
            .any(|name| name.contains("Noto Sans JP (subset)")));
        assert!(names.iter().any(|name| name.contains("NotoEmoji")));
        assert!(names.iter().any(|name| name.contains("Ubuntu-Light")));
        assert!(names.iter().any(|name| name.contains("Hack")));
        assert!(names.iter().any(|name| name.contains("emoji-icon-font")));
    }

    #[test]
    fn the_application_licenses_are_the_repository_files() {
        assert!(APP_LICENSE_MIT.contains("MIT License"));
        assert!(APP_LICENSE_APACHE.contains("Apache License"));
        assert!(APP_LICENSE_APACHE.contains("Version 2.0"));
    }

    #[test]
    fn every_bundled_font_has_an_attribution_line() {
        assert_eq!(ATTRIBUTIONS.len(), 5, "one holder line per bundled font");
        for (font, holder) in ATTRIBUTIONS {
            assert!(!holder.is_empty(), "{font} has no holder line");
            assert!(
                holder.contains("Copyright"),
                "{font} holder line is not a copyright statement"
            );
        }
        let noto = ATTRIBUTIONS
            .iter()
            .find(|(font, _)| *font == "Noto Sans JP (subset)")
            .expect("Noto Sans JP attribution");
        assert!(noto.1.contains("Reserved Font Name 'Source'"));
    }

    #[test]
    fn each_bundled_font_names_its_own_copyright_holder() {
        // The upstream OFL / UFL bodies contain no holder line, so the holder
        // is carried by ATTRIBUTIONS. Each entry must name its own upstream
        // holder, otherwise a swapped copy-paste passes unnoticed.
        for (font, holder) in [
            ("Noto Sans JP (subset)", "Adobe"),
            ("NotoEmoji", "Google Inc."),
            ("Ubuntu-Light", "Canonical"),
            ("Hack", "Source Foundry"),
            ("emoji-icon-font", "John Slegers"),
        ] {
            let (_, line) = ATTRIBUTIONS
                .iter()
                .find(|(name, _)| *name == font)
                .unwrap_or_else(|| panic!("missing attribution for {font}"));
            assert!(line.contains(holder), "{font} must name {holder}: {line}");
        }

        // Where the embedded text does carry a holder, it has to be the text
        // for that font (the OFL body is identical between the two Noto
        // families, so the holder is the only distinguishing string).
        assert!(NOTO_SANS_JP_OFL.contains("Copyright 2014-2021 Adobe"));
        assert!(HACK_LICENSE.contains("Copyright 2018 Source Foundry Authors"));
        assert!(HACK_LICENSE.contains("Bitstream, Inc."));
        assert!(EMOJI_ICON_MIT.contains("Copyright (c) 2014 John Slegers"));
    }

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
        // The declaration line itself, not just any mention of the term.
        assert!(
            NOTO_SANS_JP_OFL.lines().any(|line| {
                line.trim()
                    == "Copyright 2014-2021 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source'"
            }),
            "the OFL declaration line must be intact in the embedded text"
        );
        assert!(NOTO_SANS_JP_OFL.contains("Reserved Font Name"));
    }

    #[test]
    fn the_ubuntu_license_is_the_ufl_not_the_ofl() {
        assert!(UBUNTU_UFL.contains("Ubuntu Font Licence") || UBUNTU_UFL.contains("UBUNTU FONT"));
        assert!(!UBUNTU_UFL.contains("SIL OPEN FONT LICENSE"));
    }
}
