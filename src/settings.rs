//! Small, human-readable preferences file.
//!
//! A settings panel that forgets its switches on the next launch is worse than
//! no settings panel. The file is deliberately line-oriented (`key=value`):
//! it is easy to inspect, has no schema migration cost, and keeps the binary
//! free of a serialization dependency. Unknown keys are ignored, so older and
//! newer builds can share one file.

use std::collections::BTreeMap;
use std::path::PathBuf;

use mugenzip_core::{CompressionLevel, CreateFormat};

use crate::theme::ThemeMode;

#[derive(Clone, Debug)]
pub struct Settings {
    pub widget_enabled: bool,
    pub wrap_in_folder: bool,
    pub reveal_after_extract: bool,
    pub show_system_files: bool,
    pub theme_mode: ThemeMode,
    pub default_create_format: CreateFormat,
    pub default_create_level: CompressionLevel,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // The packaged product is the two-entry experience: browser plus
            // resident dot. A user who does not want the dot can turn it off
            // once here, and the choice survives restarts.
            widget_enabled: true,
            wrap_in_folder: true,
            reveal_after_extract: true,
            show_system_files: false,
            theme_mode: ThemeMode::System,
            default_create_format: CreateFormat::Zip,
            default_create_level: CompressionLevel::Balanced,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        let mut settings = Self::default();
        let mut values = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                values.insert(key.trim().to_owned(), value.trim().to_owned());
            }
        }
        if let Some(value) = values.get("widget_enabled") {
            settings.widget_enabled = parse_bool(value).unwrap_or(settings.widget_enabled);
        }
        if let Some(value) = values.get("wrap_in_folder") {
            settings.wrap_in_folder = parse_bool(value).unwrap_or(settings.wrap_in_folder);
        }
        if let Some(value) = values.get("reveal_after_extract") {
            settings.reveal_after_extract =
                parse_bool(value).unwrap_or(settings.reveal_after_extract);
        }
        if let Some(value) = values.get("show_system_files") {
            settings.show_system_files = parse_bool(value).unwrap_or(settings.show_system_files);
        }
        if let Some(value) = values.get("theme_mode") {
            settings.theme_mode = match value.as_str() {
                "light" => ThemeMode::Light,
                "dark" => ThemeMode::Dark,
                _ => ThemeMode::System,
            };
        }
        if let Some(value) = values.get("default_create_format") {
            settings.default_create_format = match value.as_str() {
                "tar.gz" => CreateFormat::TarGz,
                "7z" => CreateFormat::SevenZ,
                _ => CreateFormat::Zip,
            };
        }
        if let Some(value) = values.get("default_create_level") {
            settings.default_create_level = match value.as_str() {
                "store" => CompressionLevel::Store,
                "fast" => CompressionLevel::Fast,
                "maximum" => CompressionLevel::Maximum,
                _ => CompressionLevel::Balanced,
            };
        }
        settings
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = settings_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = format!(
            "# MugenZip settings\n\
             widget_enabled={}\n\
             wrap_in_folder={}\n\
             reveal_after_extract={}\n\
             show_system_files={}\n\
             theme_mode={}\n\
             default_create_format={}\n\
             default_create_level={}\n",
            self.widget_enabled,
            self.wrap_in_folder,
            self.reveal_after_extract,
            self.show_system_files,
            match self.theme_mode {
                ThemeMode::System => "system",
                ThemeMode::Light => "light",
                ThemeMode::Dark => "dark",
            },
            self.default_create_format.extension(),
            match self.default_create_level {
                CompressionLevel::Store => "store",
                CompressionLevel::Fast => "fast",
                CompressionLevel::Balanced => "balanced",
                CompressionLevel::Maximum => "maximum",
            },
        );
        // Write beside the destination and rename, so a crash mid-save cannot
        // leave a truncated settings file behind.
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, body)?;
        std::fs::rename(temporary, path)
    }
}

pub fn settings_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("settings.conf"))
}

pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("MUGENZIP_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")
            .or_else(|| std::env::var_os("LOCALAPPDATA"))?;
        return Some(PathBuf::from(appdata).join("MugenZip"));
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        return Some(
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("MugenZip"),
        );
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        return Some(config.join("mugenzip"));
    }
    #[allow(unreachable_code)]
    None
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}
