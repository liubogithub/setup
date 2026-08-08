//! Configuration loading.
//!
//! Resolution order (later wins):
//!   1. Built-in defaults
//!   2. Config file at `~/.config/flashcode/config.toml`
//!   3. Environment variables (`DEEPSEEK_API_KEY`, `DEEPSEEK_BASE_URL`, `DEEPSEEK_MODEL`)

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_MODEL: &str = "deepseek-v4-flash";

#[derive(Debug, Clone)]
pub struct Config {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

/// The subset of settings that may appear in the TOML config file.
/// Every field is optional so a partial file is valid.
#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("flashcode").join("config.toml"))
}

fn load_file() -> Result<FileConfig> {
    let Some(path) = config_path() else {
        return Ok(FileConfig::default());
    };
    if !path.exists() {
        return Ok(FileConfig::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading config file {}", path.display()))?;
    let parsed: FileConfig = toml::from_str(&text)
        .with_context(|| format!("parsing config file {}", path.display()))?;
    Ok(parsed)
}

impl Config {
    /// Build the effective config from file + environment.
    pub fn load() -> Result<Config> {
        let file = load_file()?;

        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .or(file.api_key)
            .context(
                "no API key found: set DEEPSEEK_API_KEY or `api_key` in \
                 ~/.config/flashcode/config.toml",
            )?;

        let base_url = std::env::var("DEEPSEEK_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .or(file.base_url)
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        let model = std::env::var("DEEPSEEK_MODEL")
            .ok()
            .filter(|s| !s.is_empty())
            .or(file.model)
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());

        Ok(Config {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
        })
    }
}
