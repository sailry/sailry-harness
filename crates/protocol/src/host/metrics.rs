//! Transient execution-host observations. Percentages use hundredths of a percent.
use serde::{Deserialize, Serialize};

use crate::{MemoryUsage, NodeId};

pub const PROCESS_LIMIT: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    pub node: NodeId,
    pub sampled_at_ms: u64,
    pub cpu_basis_points: Option<u32>,
    /// Per logical core in host order; empty until a utilization interval exists.
    pub core_basis_points: Vec<u32>,
    /// `None` where the platform has no load average, such as Windows.
    pub load: Option<LoadAverage>,
    pub memory: Option<MemoryUsage>,
    pub swap: Option<SwapUsage>,
    pub gpus: Vec<Gpu>,
    pub disks: Vec<Disk>,
    pub network: Option<NetworkRate>,
    pub processes: Vec<Process>,
    pub process_count: usize,
}

/// Run-queue load averages in hundredths, so a load of 1.25 is 125.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadAverage {
    pub one: u32,
    pub five: u32,
    pub fifteen: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwapUsage {
    pub total_bytes: u64,
    pub used_bytes: u64,
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
    pub file_system: String,
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
    /// Interface counters accumulated since they started, normally at boot.
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
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
