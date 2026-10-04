//! On-demand sampling, following Code 67ae9fa0's host-observer ownership.
use sailry_protocol::{Fault, MemoryUsage, NodeId, host::metrics::*};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::{DiskRefreshKind, Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System};

mod network;

#[cfg(target_os = "macos")]
mod gpu;

pub(super) struct Sampler {
    system: System,
    disks: Disks,
    networks: Networks,
    last: Option<(Instant, Sample)>,
    sampled: Option<Instant>,
}

impl Sampler {
    pub(super) fn new() -> Self {
        Self {
            system: System::new(),
            disks: Disks::new(),
            networks: Networks::new(),
            last: None,
            sampled: None,
        }
    }

    pub(super) fn sample(&mut self, node: NodeId) -> Result<Sample, Fault> {
        if let Some((time, sample)) = &self.last
            && time.elapsed() < Duration::from_secs(1)
        {
            return Ok(sample.clone());
        }
        let now = Instant::now();
        let interval = self.sampled.map(|last| now.duration_since(last));
        let ready = interval.is_some_and(|elapsed| elapsed < Duration::from_secs(5));
        let network = network::sample(&mut self.networks, interval);
        let previous: BTreeSet<_> = self
            .system
            .processes()
            .values()
            .map(|process| (process.pid().as_u32(), process.start_time()))
            .collect();
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        self.disks.refresh_specifics(
            true,
            DiskRefreshKind::nothing().with_storage().with_io_usage(),
        );
        let mut processes: Vec<_> = self
            .system
            .processes()
            .values()
            .map(|process| {
                let pid = process.pid().as_u32();
                let started_at_secs = process.start_time();
                Process {
                    pid,
                    started_at_secs,
                    name: process.name().to_string_lossy().chars().take(256).collect(),
                    cpu_basis_points: (ready && previous.contains(&(pid, started_at_secs)))
                        .then(|| basis_points(process.cpu_usage())),
                    memory_bytes: process.memory(),
                }
            })
            .collect();
        let process_count = processes.len();
        processes.sort_by(|left, right| {
            right
                .cpu_basis_points
                .cmp(&left.cpu_basis_points)
                .then_with(|| right.memory_bytes.cmp(&left.memory_bytes))
                .then_with(|| left.pid.cmp(&right.pid))
        });
        processes.truncate(PROCESS_LIMIT);
        let disks = self
            .disks
            .iter()
            .take(128)
            .map(|disk| {
                let usage = disk.usage();
                let seen = self.last.as_ref().is_some_and(|(_, sample)| {
                    sample
                        .disks
                        .iter()
                        .any(|previous| std::path::Path::new(&previous.mount) == disk.mount_point())
                });
                Disk {
                    mount: disk.mount_point().to_string_lossy().into_owned(),
                    name: disk.name().to_string_lossy().into_owned(),
                    total_bytes: disk.total_space(),
                    available_bytes: disk.available_space().min(disk.total_space()),
                    io: interval
                        .filter(|_| {
                            ready
                                && seen
                                && (usage.total_read_bytes > 0 || usage.total_written_bytes > 0)
                        })
                        .map(|interval| IoRate {
                            read_bytes_per_sec: (usage.read_bytes as f64 / interval.as_secs_f64())
                                as u64,
                            written_bytes_per_sec: (usage.written_bytes as f64
                                / interval.as_secs_f64())
                                as u64,
                        }),
                }
            })
            .collect();
        let total = self.system.total_memory();
        let sample = Sample {
            node,
            sampled_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| super::unavailable())?
                .as_millis() as u64,
            cpu_basis_points: ready
                .then(|| basis_points(self.system.global_cpu_usage()).min(10000)),
            memory: (total > 0).then(|| MemoryUsage {
                total_bytes: total,
                available_bytes: self.system.available_memory().min(total),
            }),
            gpus: gpus(),
            disks,
            network,
            processes,
            process_count,
        };
        self.sampled = Some(now);
        self.last = Some((Instant::now(), sample.clone()));
        Ok(sample)
    }
}

fn basis_points(percent: f32) -> u32 {
    (percent * 100.).round() as u32
}

fn gpus() -> Vec<Gpu> {
    #[cfg(target_os = "macos")]
    {
        gpu::sample()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}
