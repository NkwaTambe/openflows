//! Shared helpers for reading and writing a tenant's control-plane state.
//!
//! The control mode lives at the tenant-scoped key `ns:{tenant}:control:mode`.
//! The manager holds an unscoped store and builds fully-qualified keys itself
//! so it can address any tenant. `auto` is the implicit default.

use crate::server::AppState;
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
pub async fn read_control_mode(store: &SharedStore, tenant: &str) -> ControlMode {
    store
        .raw_get(&control_key(tenant))
        .await
        .and_then(|v| v.as_str().map(String::from))
        .and_then(|s| ControlMode::parse(&s))
        .unwrap_or(ControlMode::Auto)
}

/// Write a control mode, returning the previous mode.
pub async fn write_control_mode(state: &AppState, tenant: &str, mode: &ControlMode) -> ControlMode {
    let previous = read_control_mode(state.store(), tenant).await;
    state
        .store()
        .raw_set(&control_key(tenant), serde_json::json!(mode.as_str()))
        .await;
    previous
}
