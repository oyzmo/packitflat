//! Pack It Flat — the parts that don't need a screen.
//!
//! The binary is a thin GTK layer over this library. Everything that decides
//! anything (parsing a manifest, detecting a project, saving state) lives here
//! so it can be tested without a display, and so the wizard and the editor are
//! provably working on the same model.

pub mod appdata;
pub mod build;
pub mod detect;
pub mod generate;
pub mod git;
pub mod i18n;
pub mod icons;
pub mod oars;
pub mod manifest;
pub mod permissions;
pub mod project;
pub mod runtimes;
pub mod settings;
pub mod sha256;
pub mod spdx;
pub mod sync;
pub mod templates;
pub mod validate;
pub mod vendor;

/// Reverse-DNS ID under a domain the author actually controls. It has to match
/// the .desktop, metainfo and icon filenames — the app enforces that rule for
/// its users, so it had better follow it itself.
pub const APP_ID: &str = "no.oyzmo.PackItFlat";
pub const APP_NAME: &str = "Pack It Flat";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
