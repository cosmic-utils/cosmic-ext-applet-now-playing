use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const ALBUM_COLOR_ENABLED: &str = "album-color-enabled";
const PREVIOUS_CONTROL_ENABLED: &str = "previous-control-enabled";
const PLAY_CONTROL_ENABLED: &str = "play-control-enabled";
const NEXT_CONTROL_ENABLED: &str = "next-control-enabled";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppSettings {
    pub album_color_enabled: bool,
    pub previous_control_enabled: bool,
    pub play_control_enabled: bool,
    pub next_control_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            album_color_enabled: true,
            previous_control_enabled: false,
            play_control_enabled: true,
            next_control_enabled: false,
        }
    }
}

fn settings_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|path| path.join("cosmic-ext-applet-now-playing"))
}

pub fn load() -> AppSettings {
    let Some(directory) = settings_dir() else {
        eprintln!("unable to load settings: user config directory not found");
        return AppSettings::default();
    };

    load_from(&directory)
}

pub fn save(settings: AppSettings) -> io::Result<()> {
    let directory = settings_dir().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "user config directory not found")
    })?;
    save_to(&directory, settings)
}

fn load_from(directory: &Path) -> AppSettings {
    let defaults = AppSettings::default();

    AppSettings {
        album_color_enabled: load_bool(
            &directory.join(ALBUM_COLOR_ENABLED),
            defaults.album_color_enabled,
        ),
        previous_control_enabled: load_bool(
            &directory.join(PREVIOUS_CONTROL_ENABLED),
            defaults.previous_control_enabled,
        ),
        play_control_enabled: load_bool(
            &directory.join(PLAY_CONTROL_ENABLED),
            defaults.play_control_enabled,
        ),
        next_control_enabled: load_bool(
            &directory.join(NEXT_CONTROL_ENABLED),
            defaults.next_control_enabled,
        ),
    }
}

fn load_bool(path: &Path, default: bool) -> bool {
    match read_bool(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => default,
        Err(error) => {
            eprintln!("unable to load setting from {}: {error}", path.display());
            default
        }
    }
}

fn read_bool(path: &Path) -> io::Result<bool> {
    let value = fs::read_to_string(path)?;
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        invalid => {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid boolean setting: {invalid}"),
            ))
        }
    }
}

fn save_to(directory: &Path, settings: AppSettings) -> io::Result<()> {
    fs::create_dir_all(directory)?;

    write_bool(
        &directory.join(ALBUM_COLOR_ENABLED),
        settings.album_color_enabled,
    )?;
    write_bool(
        &directory.join(PREVIOUS_CONTROL_ENABLED),
        settings.previous_control_enabled,
    )?;
    write_bool(
        &directory.join(PLAY_CONTROL_ENABLED),
        settings.play_control_enabled,
    )?;
    write_bool(
        &directory.join(NEXT_CONTROL_ENABLED),
        settings.next_control_enabled,
    )
}

fn write_bool(path: &Path, value: bool) -> io::Result<()> {
    fs::write(path, if value { "true\n" } else { "false\n" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn defaults_to_play_control_only() {
        assert_eq!(
            AppSettings::default(),
            AppSettings {
                album_color_enabled: true,
                previous_control_enabled: false,
                play_control_enabled: true,
                next_control_enabled: false,
            }
        );
    }

    #[test]
    fn round_trips_settings() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("nested");
        let expected = AppSettings {
            album_color_enabled: false,
            previous_control_enabled: true,
            play_control_enabled: false,
            next_control_enabled: true,
        };

        save_to(&path, expected).unwrap();

        assert_eq!(load_from(&path), expected);
    }

    #[test]
    fn defaults_missing_control_settings() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join(ALBUM_COLOR_ENABLED), "false\n").unwrap();

        assert_eq!(
            load_from(directory.path()),
            AppSettings {
                album_color_enabled: false,
                ..AppSettings::default()
            }
        );
    }

    #[test]
    fn invalid_values_fall_back_independently() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join(ALBUM_COLOR_ENABLED), "false\n").unwrap();
        fs::write(
            directory.path().join(PREVIOUS_CONTROL_ENABLED),
            "sometimes\n",
        )
        .unwrap();
        fs::write(directory.path().join(PLAY_CONTROL_ENABLED), "false\n").unwrap();
        fs::write(directory.path().join(NEXT_CONTROL_ENABLED), "true\n").unwrap();

        assert_eq!(
            load_from(directory.path()),
            AppSettings {
                album_color_enabled: false,
                previous_control_enabled: false,
                play_control_enabled: false,
                next_control_enabled: true,
            }
        );
    }

    #[test]
    fn supports_all_controls_disabled() {
        let directory = tempdir().unwrap();
        let expected = AppSettings {
            album_color_enabled: true,
            previous_control_enabled: false,
            play_control_enabled: false,
            next_control_enabled: false,
        };

        save_to(directory.path(), expected).unwrap();

        assert_eq!(load_from(directory.path()), expected);
    }

    #[test]
    fn reports_write_failures() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("not-a-directory");
        fs::write(&path, "contents").unwrap();

        assert!(save_to(&path, AppSettings::default()).is_err());
    }
}
