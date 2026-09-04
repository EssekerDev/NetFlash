//! Persist appearance in the platform config dir. Not engine state.

use std::fs;
use std::path::PathBuf;

use netflash_icon::Skin;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct FilePrefs {
    #[serde(default)]
    skin: String,
}

/// Load the last chosen skin. Missing or junk files become [`Skin::Dot`].
pub fn load_skin() -> Skin {
    let Some(path) = config_path() else {
        return Skin::Dot;
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return Skin::Dot;
    };
    let Ok(prefs) = toml::from_str::<FilePrefs>(&raw) else {
        return Skin::Dot;
    };
    Skin::from_key(&prefs.skin).unwrap_or(Skin::Dot)
}

/// Best-effort write. Failures stay local (no panic, no telemetry).
pub fn save_skin(skin: Skin) {
    let Some(path) = config_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let body = match toml::to_string_pretty(&FilePrefs {
        skin: skin.key().to_owned(),
    }) {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = fs::write(path, body);
}

fn config_path() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("NetFlash")
                .join("config.toml"),
        )
    } else if cfg!(windows) {
        let appdata = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(appdata).join("NetFlash").join("config.toml"))
    } else {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join(".config")
                .join("netflash")
                .join("config.toml"),
        )
    }
}
