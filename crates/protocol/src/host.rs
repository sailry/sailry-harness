use serde::{Deserialize, Serialize};

use crate::NodeId;

pub mod metrics;

/// A point-in-time observation of the execution host, never controller metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    pub node: NodeId,
    pub sampled_at_ms: u64,
    pub name: Option<String>,
    pub os: Option<String>,
    pub os_version: Option<String>,
    pub architecture: Option<String>,
    pub logical_cpus: Option<u32>,
    pub memory: Option<MemoryUsage>,
    pub host_uptime_secs: u64,
    pub node_uptime_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryUsage {
    pub total_bytes: u64,
    pub available_bytes: u64,
}
