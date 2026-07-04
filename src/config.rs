use std::fs;

use anyhow::Context as _;
use serde::Deserialize;

/// Game configuration loaded from `config.toml`.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub display: DisplayConfig,
    pub debug: DebugConfig,
}

impl Config {
    pub fn load(filename: &str) -> anyhow::Result<Self> {
        let content = fs::read_to_string(filename)
            .with_context(|| format!("failed to read config file `{filename}`"))?;
        toml::from_str(&content).with_context(|| format!("failed to parse `{filename}`"))
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct DisplayConfig {
    pub height: f32,
    pub width: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DebugConfig {
    pub debug: bool,
}
