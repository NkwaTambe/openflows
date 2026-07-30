//! openflows-harness library — typed SharedStore access for Coder Agent worker workspaces.
//!
//! This library exposes the `HarnessStore` struct which provides type-safe Redis operations
//! for managing OpenFlows state, including gate approvals for phase transitions.

pub mod store;

pub use store::{
    DispatchPayload, GateApproval, HandoffPayload, HarnessStore, MergePayload, PrInfo,
    ReviewPayload,
};

// Type alias for convenience
pub type Harness = HarnessStore;
