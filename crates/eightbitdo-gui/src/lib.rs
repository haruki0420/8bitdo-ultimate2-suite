//! Desktop GUI for the 8BitDo Ultimate 2 Bluetooth controller.
//!
//! Scanning, connecting, device information, live input, configuration viewing,
//! and read-only backups are Phase 3a. Phase 3b adds the write path: a full
//! 2768-byte configuration write (settings and button remapping) and macro
//! editing / recording / writing. Every write is confirmed first, takes a
//! mandatory pre-write backup, then verifies a read-back and reports the result
//! with its recovery data on failure.
//!
//! The crate is split into a small testable core ([`driver`], [`backup`],
//! [`model`], [`mod@write`]) and a thin egui layer ([`app`]) that talks to a
//! background tokio worker ([`worker`]) over channels. The UI thread never
//! awaits device I/O. [`i18n`] holds every user-visible string (Japanese by
//! default, English fallback) and [`theme`] holds the design tokens and
//! bundled fonts.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod app;
pub mod backup;
pub mod driver;
pub mod i18n;
pub mod licenses;
pub mod model;
pub mod theme;
pub mod util;
pub mod worker;
pub mod write;
