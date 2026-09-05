//! The handful of things the app remembers between runs.
//!
//! GSettings is the GNOME way and is what ships in the Flatpak, but a missing
//! schema makes `gio::Settings::new` abort the process — so the schema is looked
//! up first, and a plain file under the config directory stands in when it isn't
//! installed (a source-tree run, or a build where the schema wasn't installed).
//! Both paths are the same three functions, so nothing above here has to know
//! which one is in use.

use std::path::PathBuf;

use gtk::gio::prelude::SettingsExt;
use serde::{Deserialize, Serialize};

pub const SCHEMA_ID: &str = "no.oyzmo.PackItFlat";

/// How the user prefers to work on a new project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Ask every time. The default until someone says otherwise.
    #[default]
    Ask,
    Guided,
    Editor,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Ask => "ask",
            Mode::Guided => "guided",
            Mode::Editor => "editor",
        }
    }

    /// Anything unrecognised means "ask", which is never wrong, only slower.
    pub fn parse(value: &str) -> Mode {
        match value {
            "guided" => Mode::Guided,
            "editor" => Mode::Editor,
            _ => Mode::Ask,
        }
    }

    /// What the preferences dialog shows for each choice.
    pub fn label(&self) -> &'static str {
        match self {
            Mode::Ask => "Ask me each time",
            Mode::Guided => "Guided steps",
            Mode::Editor => "Editor",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
struct Stored {
    default_mode: Mode,
}

pub fn config_path() -> PathBuf {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join(".config")
        });
    config_home.join("packitflat").join("preferences.yml")
}

/// Whether the GSettings schema is installed. Checked rather than assumed: the
/// alternative is a hard abort inside GLib with no way to catch it.
pub fn schema_installed() -> bool {
    gtk::gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup(SCHEMA_ID, true))
        .is_some()
}

pub fn default_mode() -> Mode {
    if schema_installed() {
        let settings = gtk::gio::Settings::new(SCHEMA_ID);
        return Mode::parse(&settings.string("default-mode"));
    }
    read_file(&config_path()).default_mode
}

pub fn set_default_mode(mode: Mode) {
    if schema_installed() {
        let settings = gtk::gio::Settings::new(SCHEMA_ID);
        let _ = settings.set_string("default-mode", mode.as_str());
        return;
    }
    let path = config_path();
    let stored = Stored { default_mode: mode };
    if let Err(err) = write_file(&path, &stored) {
        // Losing a preference is not worth interrupting anyone over; losing it
        // silently and without a trace is.
        eprintln!("packitflat: could not save preferences to {}: {err:#}", path.display());
    }
}

fn read_file(path: &std::path::Path) -> Stored {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_yaml_ng::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_file(path: &std::path::Path, stored: &Stored) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_yaml_ng::to_string(stored)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_round_trip_through_their_stored_names() {
        for mode in [Mode::Ask, Mode::Guided, Mode::Editor] {
            assert_eq!(Mode::parse(mode.as_str()), mode);
        }
        assert_eq!(Mode::parse("nonsense"), Mode::Ask);
        assert_eq!(Mode::default(), Mode::Ask);
    }

    #[test]
    fn the_file_fallback_survives_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.yml");

        assert_eq!(read_file(&path), Stored::default());
        write_file(
            &path,
            &Stored {
                default_mode: Mode::Editor,
            },
        )
        .unwrap();
        assert_eq!(read_file(&path).default_mode, Mode::Editor);
    }

    #[test]
    fn a_damaged_preferences_file_is_ignored_rather_than_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.yml");
        std::fs::write(&path, "default-mode: [not, a, string\n").unwrap();
        assert_eq!(read_file(&path).default_mode, Mode::Ask);
    }

    #[test]
    fn the_config_path_follows_xdg() {
        let path = config_path();
        assert!(path.is_absolute());
        assert!(path.ends_with("packitflat/preferences.yml"));
    }
}
