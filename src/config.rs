use std::fs;
use std::path::PathBuf;

use anyhow::Context as _;
use serde::Deserialize;

/// Game configuration loaded from `config.toml`.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub display: DisplayConfig,
}

impl Config {
    pub fn load(filename: &str) -> anyhow::Result<Self> {
        // Look next to the current directory first, then the crate root, so
        // the binary works no matter where it is launched from during dev.
        let mut path = PathBuf::from(filename);
        if !path.exists() {
            path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(filename);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config file `{}`", path.display()))?;
        toml::from_str(&content).with_context(|| format!("failed to parse `{}`", path.display()))
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct DisplayConfig {
    pub height: f32,
    pub width: f32,
}
