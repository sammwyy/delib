//! Shared building blocks for Linux desktop shells and native applications.
//!
//! The crate is toolkit-agnostic: it discovers desktop resources and returns
//! standard Rust values such as [`std::path::PathBuf`]. Applications remain in
//! charge of rendering or decoding those resources.
#![forbid(unsafe_code)]

pub mod application_icons;
pub mod desktop_entry;
pub mod icons;
pub mod mimeapps;
pub mod sounds;
pub mod xdg;
