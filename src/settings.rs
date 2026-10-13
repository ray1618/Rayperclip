//! User settings, stored as TOML.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("could not read or write settings: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid settings file: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("could not serialize settings: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Upper bound for the number of remembered (unpinned) clips.
pub const MAX_CLIPS_LIMIT: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Record new clipboard contents.
    pub recording: bool,
    /// Maximum number of unpinned clips to keep.
    pub max_clips: usize,
    /// Number of recent clips listed in the menu-bar menu.
    pub menu_clip_count: usize,
    /// Skip data marked as concealed/transient (password managers).
    pub ignore_concealed: bool,
    /// After choosing a clip, paste it into the frontmost app (Cmd+V).
    pub paste_directly: bool,
    /// Global hotkey that toggles the search window.
    pub window_hotkey: String,
    /// Global hotkey that pops up the clip menu at the mouse pointer.
    pub menu_hotkey: String,
    /// Name of the active theme.
    pub theme: String,
    /// Launch Rayperclip when logging in.
    pub start_at_login: bool,
    /// Maximum characters shown for clip titles.
    pub title_length: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recording: true,
            max_clips: 80,
            menu_clip_count: 20,
            ignore_concealed: true,
            paste_directly: true,
            window_hotkey: "cmd+shift+v".into(),
            menu_hotkey: "cmd+alt+v".into(),
            theme: "Light".into(),
            start_at_login: false,
            title_length: crate::model::DEFAULT_TITLE_LENGTH,
        }
    }
}

impl Settings {
    /// Clamp values into sane ranges.
    pub fn normalized(mut self) -> Self {
        self.max_clips = self.max_clips.clamp(1, MAX_CLIPS_LIMIT);
        self.menu_clip_count = self.menu_clip_count.clamp(1, 100);
        self.title_length = self.title_length.clamp(10, 200);
        self
    }

    /// Parse settings from TOML text. Unknown keys are ignored and missing
    /// keys take their default value.
    pub fn from_toml(text: &str) -> Result<Self, SettingsError> {
        Ok(toml::from_str::<Settings>(text)?.normalized())
    }

    pub fn to_toml(&self) -> Result<String, SettingsError> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Load settings; a missing file yields the defaults.
    pub fn load(path: &Path) -> Result<Self, SettingsError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Write settings, creating the parent directory when needed.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, self.to_toml()?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

/// Locations of Rayperclip's files.
#[derive(Debug, Clone)]
pub struct Paths {
    pub settings: PathBuf,
    pub database: PathBuf,
}

impl Paths {
    /// Standard macOS locations (`~/Library/Application Support/...`).
    pub fn standard() -> Option<Self> {
        let dirs = ProjectDirs::from("nl", "ray", crate::APP_NAME)?;
        Some(Self::in_dir(dirs.config_dir(), dirs.data_dir()))
    }

    pub fn in_dir(config_dir: &Path, data_dir: &Path) -> Self {
        Self {
            settings: config_dir.join("settings.toml"),
            database: data_dir.join("clips.sqlite"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let s = Settings::default();
        assert!(s.recording);
        assert!(s.ignore_concealed);
        assert!(s.paste_directly);
        assert_eq!(s.max_clips, 80);
        assert_eq!(s.theme, "Light");
    }

    #[test]
    fn missing_keys_use_defaults() {
        let s = Settings::from_toml("max_clips = 5\ntheme = \"Ocean\"").unwrap();
        assert_eq!(s.max_clips, 5);
        assert_eq!(s.theme, "Ocean");
        assert!(s.recording);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert!(Settings::from_toml("does_not_exist = 1").is_ok());
    }

    #[test]
    fn invalid_toml_is_an_error() {
        assert!(matches!(
            Settings::from_toml("max_clips = \"many\""),
            Err(SettingsError::Parse(_))
        ));
    }

    #[test]
    fn values_are_clamped() {
        let s =
            Settings::from_toml("max_clips = 0\nmenu_clip_count = 1000\ntitle_length = 1").unwrap();
        assert_eq!(s.max_clips, 1);
        assert_eq!(s.menu_clip_count, 100);
        assert_eq!(s.title_length, 10);
    }

    #[test]
    fn toml_roundtrip() {
        let s = Settings {
            theme: "Rose".into(),
            max_clips: 123,
            ..Settings::default()
        };
        assert_eq!(Settings::from_toml(&s.to_toml().unwrap()).unwrap(), s);
    }

    #[test]
    fn load_missing_file_gives_defaults_and_save_load_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.toml");
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        let s = Settings {
            recording: false,
            ..Settings::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
    }

    #[test]
    fn paths_in_dir() {
        let p = Paths::in_dir(Path::new("/c"), Path::new("/d"));
        assert_eq!(p.settings, PathBuf::from("/c/settings.toml"));
        assert_eq!(p.database, PathBuf::from("/d/clips.sqlite"));
    }
}
