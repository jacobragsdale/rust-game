use std::fs;

use serde::Deserialize;
use toml;

/// contain the game configurations from the config.toml file
#[derive(Deserialize)]
pub(super) struct Config {
    pub display: DisplayConfig,
    pub debug: DebugConfig
}

impl Config {
    pub fn new(filename: &str) -> Self {
        let content = fs::read_to_string(filename).unwrap();
        toml::from_str(&content).unwrap()
    }
}

#[derive(Deserialize)]
pub(super) struct DisplayConfig {
    pub height: f32,
    pub width: f32,
}

#[derive(Deserialize)]
pub(super) struct DebugConfig {
    pub debug: bool,
}
