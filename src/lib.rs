//! Rayperclip – a menu-bar clipboard history manager for macOS.
//!
//! The library contains all platform-independent logic so it can be unit
//! tested without touching the real pasteboard, the window server or the
//! accessibility APIs. The macOS glue and the UI live in the binary crate.

pub mod capture;
pub mod clipboard;
pub mod hotkeys;
pub mod login;
pub mod model;
pub mod paste;
pub mod poller;
pub mod presentation;
pub mod settings;
pub mod store;
pub mod theme;

/// Human readable application name.
pub const APP_NAME: &str = "Rayperclip";
/// Reverse-DNS identifier used for the LaunchAgent and data directories.
pub const APP_ID: &str = "nl.ray.rayperclip";
