//! Transient execution-host observations. Percentages use hundredths of a percent.
use serde::{Deserialize, Serialize};

use crate::{MemoryUsage, NodeId};

pub const PROCESS_LIMIT: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    pub node: NodeId,
    pub sampled_at_ms: u64,
    pub cpu_basis_points: Option<u32>,
    pub memory: Option<MemoryUsage>,
    pub gpus: Vec<Gpu>,
    pub disks: Vec<Disk>,
    pub network: Option<NetworkRate>,
    pub processes: Vec<Process>,
    pub process_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gpu {
    pub id: String,
    pub name: String,
    pub usage_basis_points: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disk {
    pub mount: String,
    pub name: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub io: Option<IoRate>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IoRate {
    pub read_bytes_per_sec: u64,
    pub written_bytes_per_sec: u64,
}

/// Aggregate traffic across IP interfaces, excluding loopback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkRate {
    pub received_bytes_per_sec: u64,
    pub transmitted_bytes_per_sec: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Process {
    pub pid: u32,
    pub started_at_secs: u64,
    pub name: String,
    /// One fully occupied logical core is 100%; a process can use several cores.
    pub cpu_basis_points: Option<u32>,
    pub memory_bytes: u64,
}
