//! Shared building blocks for desktop shells and native applications.
//!
//! The crate is toolkit-agnostic: it discovers desktop resources and returns
//! standard Rust values such as [`std::path::PathBuf`]. Applications remain in
//! charge of rendering or decoding those resources.
#![deny(unsafe_code)]

#[cfg(feature = "xdg")]
pub mod application_icons;
#[cfg(feature = "xdg")]
pub mod desktop_entry;
#[cfg(feature = "xdg")]
pub mod icons;
#[cfg(feature = "xdg")]
pub mod mimeapps;
#[cfg(feature = "xdg")]
pub mod sounds;
#[cfg(all(windows, feature = "windows"))]
pub mod windows;
#[cfg(feature = "xdg")]
pub mod xdg;
