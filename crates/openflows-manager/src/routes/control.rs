//! Shared helpers for reading and writing a tenant's control-plane state.
//!
//! The control mode lives at the tenant-scoped key `ns:{tenant}:control:mode`.
//! The manager holds an unscoped store and builds fully-qualified keys itself
//! so it can address any tenant. `auto` is the implicit default.

use crate::{error::ManagerError, server::AppState};
use pocketflow_core::SharedStore;
use serde::{Deserialize, Serialize};

/// A valid, user-supplied control mode after validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlMode {
    Auto,
    Paused,
    Drained,
    Targeted,
}

impl ControlMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ControlMode::Auto => config::CONTROL_MODE_AUTO,
            ControlMode::Paused => config::CONTROL_MODE_PAUSED,
            ControlMode::Drained => config::CONTROL_MODE_DRAINED,
            ControlMode::Targeted => config::CONTROL_MODE_TARGETED,
        }
    }

    /// Parse from a wire string, returning `None` for unknown values.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            config::CONTROL_MODE_AUTO => Some(ControlMode::Auto),
            config::CONTROL_MODE_PAUSED => Some(ControlMode::Paused),
            config::CONTROL_MODE_DRAINED => Some(ControlMode::Drained),
            config::CONTROL_MODE_TARGETED => Some(ControlMode::Targeted),
            _ => None,
        }
    }
}

/// Fully-qualified Redis key for a tenant's control mode.
pub fn control_key(tenant: &str) -> String {
    format!("ns:{}:{}", tenant, config::KEY_CONTROL_MODE)
}

/// Read the current control mode, defaulting to `auto` when absent.
///
/// Returns an error when the backing store is unreadable, so the caller does
/// not mistake an unreadable state for `auto`.
pub async fn read_control_mode(
    store: &SharedStore,
    tenant: &str,
) -> Result<ControlMode, ManagerError> {
    let value = store.raw_get_result(&control_key(tenant)).await?;
    Ok(value
        .and_then(|v| v.as_str().map(String::from))
        .and_then(|s| ControlMode::parse(&s))
        .unwrap_or(ControlMode::Auto))
}

/// Write a control mode, returning the previous mode.
///
/// Returns an error when the write cannot be persisted (or verified), so a
/// failed pause/resume is surfaced to the operator instead of reported as
/// success.
pub async fn write_control_mode(
    state: &AppState,
    tenant: &str,
    mode: &ControlMode,
) -> Result<ControlMode, ManagerError> {
    let previous = read_control_mode(state.store(), tenant).await?;
    let key = control_key(tenant);
    let value = serde_json::json!(mode.as_str());
    state.store().raw_set_result(&key, value).await?;
    // Read back to confirm the write actually landed; a write that silently
    // failed must not be reported as success.
    let confirmed = read_control_mode(state.store(), tenant).await?;
    if confirmed != *mode {
        return Err(ManagerError::Service(anyhow::anyhow!(
            "control mode write did not persist (expected {}, read back {})",
            mode.as_str(),
            confirmed.as_str()
        )));
    }
    Ok(previous)
}
