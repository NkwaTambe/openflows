//! Credential / connection configuration for the OpenFlows remote CLI.
//!
//! `openflows login` persists the manager URL and bearer token to a small TOML
//! file (`~/.config/openflows/config.toml` by default). The file and its
//! directory are created with owner-only permissions so the token is not left
//! world-readable.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
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

    // Create the file with owner-only permissions up front. Because a umask can
    // only REMOVE permission bits (never add them), opening with mode 0600 can
    // never produce a world-readable file — closing the window where a token
    // written before a later chmod could be read by another local user.
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(path)
        .with_context(|| format!("failed to open config file {}", path.display()))?;
    std::io::Write::write_all(&mut file, raw.as_bytes())
        .with_context(|| format!("failed to write config file {}", path.display()))?;
    file.flush()
        .with_context(|| format!("failed to flush config file {}", path.display()))?;

    // Belt-and-braces: also enforce the mode explicitly after writing.
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
