use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

const SETTINGS_FILE_NAME: &str = "mimium-audio-plugin.json";
const REPOSITORY_URL: &str = "https://github.com/mimium-org/mimium-audio-plugin";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct GlobalSettings {
    pub library_path: String,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            library_path: default_library_path().display().to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AboutInfo {
    pub plugin_version: String,
    pub mimium_compiler_version: String,
    pub repository_url: String,
}

pub(crate) fn plugin_about_info() -> AboutInfo {
    AboutInfo {
        plugin_version: env!("CARGO_PKG_VERSION").to_string(),
        mimium_compiler_version: option_env!("MIMIUM_COMPILER_VERSION")
            .unwrap_or("dev")
            .to_string(),
        repository_url: REPOSITORY_URL.to_string(),
    }
}

pub(crate) fn load_global_settings() -> GlobalSettings {
    let path = settings_file_path();
    let Ok(content) = fs::read_to_string(&path) else {
        return GlobalSettings::default();
    };

    serde_json::from_str::<GlobalSettings>(&content).unwrap_or_default()
}

pub(crate) fn save_global_settings(settings: &GlobalSettings) -> Result<(), String> {
    let root = settings_root_dir().ok_or_else(|| "failed to locate user home".to_string())?;
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;

    let path = settings_file_path();
    let serialized = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(path, serialized).map_err(|error| error.to_string())
}

fn settings_file_path() -> PathBuf {
    settings_root_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(SETTINGS_FILE_NAME)
}

fn default_library_path() -> PathBuf {
    settings_root_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lib")
}

fn settings_root_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))?;
        return Some(home.join(".mimium"));
    }

    #[cfg(not(target_os = "windows"))]
    {
        let home = env::var_os("HOME").map(PathBuf::from)?;
        Some(home.join(".mimium"))
    }
}
