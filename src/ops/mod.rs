//! Graph mutation, wiring validation, and auto-repair helpers.

mod apply;
mod auto_wire;
mod pruning;
mod types;
mod validation;

pub use apply::{apply_ops_to_graph, apply_ops_to_graph_cached};
pub use types::{ApplyOpsOutcome, EdgeConnectProbeReason, EdgeTargetParamProbe, RepairSuggestion};
pub use validation::{pick_target_param_for_edge, probe_edge_connect, validate_edge_connect};

#[cfg(test)]
mod tests;
