//! Credential / connection configuration for the OpenFlows remote CLI.
//!
//! `openflows login` persists the manager URL and bearer token to a small TOML
//! file (`~/.config/openflows/config.toml` by default). The file and its
//! directory are created with owner-only permissions so the token is not left
//! world-readable.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Default config file location: `$HOME/.config/openflows/config.toml`.
pub fn default_config_path() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into());
    PathBuf::from(home).join(".config/openflows/config.toml")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Base URL of the deployed OpenFlows manager, e.g. `https://openflows.example.com`.
    #[serde(default)]
    pub url: Option<String>,
    /// Bearer token for the manager's `/api/v1` control surface.
    #[serde(default)]
    pub token: Option<String>,
}

/// Load the config file if present.
pub fn load(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse config file {}", path.display()))
}

/// Persist a config file with owner-only permissions.
pub fn save(path: &Path, config: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }
    let raw = toml::to_string_pretty(config)?;
    std::fs::write(path, raw)
        .with_context(|| format!("failed to write config file {}", path.display()))?;

    // Restrict the file to the owning user only (owner read/write).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .context("failed to set config file permissions")?;
    }
    Ok(())
}

/// Store the URL and token from a login.
pub fn save_login(path: &Path, url: &str, token: Option<&str>) -> Result<Config> {
    let mut config = load(path)?;
    config.url = Some(url.to_string());
    if let Some(token) = token {
        config.token = Some(token.to_string());
    }
    save(path, &config)?;
    Ok(config)
}
