//! Read-only desktop GUI for the 8BitDo Ultimate 2 Bluetooth controller.
//!
//! Phase 3a only: scanning, connecting, device information, live input,
//! configuration viewing, and a 2768-byte configuration backup. No write path
//! is present.
//!
//! The crate is split into a small testable core ([`driver`], [`backup`],
//! [`model`]) and a thin egui layer ([`app`]) that talks to a background tokio
//! worker ([`worker`]) over channels. The UI thread never awaits device I/O.
//! [`i18n`] holds every user-visible string (Japanese by default, English
//! fallback) and [`theme`] holds the design tokens and bundled fonts.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod app;
pub mod backup;
pub mod driver;
pub mod i18n;
pub mod model;
pub mod theme;
pub mod util;
pub mod worker;
