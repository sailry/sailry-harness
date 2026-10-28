//! On-demand sampling, following Code 67ae9fa0's host-observer ownership.
use sailry_protocol::{Fault, MemoryUsage, NodeId, host::metrics::*};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::{DiskRefreshKind, Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System};

mod network;

#[cfg(test)]
mod tests;

#[cfg(target_os = "macos")]
mod gpu;

pub(super) struct Sampler {
    system: System,
    // Linux process CPU deltas must not share the sidebar's CPU baseline.
    processes: System,
    disks: Disks,
    networks: Networks,
    last: Option<(Instant, Sample)>,
    sampled: Option<Instant>,
}

impl Sampler {
    pub(super) fn new() -> Self {
        Self {
            system: System::new(),
            processes: System::new(),
            disks: Disks::new(),
            networks: Networks::new(),
            last: None,
            sampled: None,
        }
    }

    pub(super) fn sample(&mut self, node: NodeId, detailed: bool) -> Result<Sample, Fault> {
        let now = Instant::now();
        let previous = self.last.take();
        let core = previous.as_ref().map(|(time, _)| *time);
        let mut sample = previous
            .map(|(_, sample)| sample)
            .unwrap_or_else(|| empty(node));
        let refresh_core =
            core.is_none_or(|time| now.duration_since(time) >= Duration::from_secs(1));
        let refresh_details = detailed
            && self
                .sampled
                .is_none_or(|time| now.duration_since(time) >= Duration::from_secs(1));
        if refresh_core {
            let ready = core.is_some_and(|time| now.duration_since(time) < Duration::from_secs(5));
            self.system.refresh_cpu_usage();
            self.system.refresh_memory();
            sample.cpu_basis_points =
                ready.then(|| basis_points(self.system.global_cpu_usage()).min(10000));
            sample.core_basis_points = if ready {
                self.system
                    .cpus()
                    .iter()
                    .take(1024)
                    .map(|cpu| basis_points(cpu.cpu_usage()).min(10000))
                    .collect()
            } else {
                Vec::new()
            };
            let total = self.system.total_memory();
            sample.memory = (total > 0).then(|| MemoryUsage {
                total_bytes: total,
                available_bytes: self.system.available_memory().min(total),
                free_bytes: self.system.free_memory().min(total),
            });
            let swap = self.system.total_swap();
            sample.swap = (swap > 0).then(|| SwapUsage {
                total_bytes: swap,
                used_bytes: self.system.used_swap().min(swap),
            });
        }
        if refresh_details {
            self.details(&mut sample, now);
        }
        if refresh_core || refresh_details {
            sample.sampled_at_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| super::unavailable())?
                .as_millis() as u64;
        }
        let output = if detailed {
            sample.clone()
        } else {
            Sample {
                sampled_at_ms: sample.sampled_at_ms,
                cpu_basis_points: sample.cpu_basis_points,
                memory: sample.memory.clone(),
                ..empty(node)
            }
        };
        self.last = Some((if refresh_core { now } else { core.unwrap() }, sample));
        Ok(output)
    }

    fn details(&mut self, sample: &mut Sample, now: Instant) {
        let interval = self.sampled.map(|last| now.duration_since(last));
        let ready = interval.is_some_and(|elapsed| elapsed < Duration::from_secs(5));
        sample.network = network::sample(&mut self.networks, interval);
        let previous: BTreeSet<_> = self
            .processes
            .processes()
            .values()
            .map(|process| (process.pid().as_u32(), process.start_time()))
            .collect();
        self.processes.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        self.disks.refresh_specifics(
            true,
            DiskRefreshKind::nothing().with_storage().with_io_usage(),
        );
        let mut processes: Vec<_> = self
            .processes
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
        sample.process_count = processes.len();
        processes.sort_by(|left, right| {
            right
                .cpu_basis_points
                .cmp(&left.cpu_basis_points)
                .then_with(|| right.memory_bytes.cmp(&left.memory_bytes))
                .then_with(|| left.pid.cmp(&right.pid))
        });
        processes.truncate(PROCESS_LIMIT);
        sample.disks = self
            .disks
            .iter()
            .take(128)
            .map(|disk| {
                let usage = disk.usage();
                let seen = sample
                    .disks
                    .iter()
                    .any(|previous| std::path::Path::new(&previous.mount) == disk.mount_point());
                Disk {
                    mount: disk.mount_point().to_string_lossy().into_owned(),
                    name: disk.name().to_string_lossy().into_owned(),
                    file_system: disk.file_system().to_string_lossy().into_owned(),
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
        sample.load = load();
        sample.gpus = gpus();
        sample.processes = processes;
        self.sampled = Some(now);
    }
}

fn empty(node: NodeId) -> Sample {
    Sample {
        node,
        sampled_at_ms: 0,
        cpu_basis_points: None,
        core_basis_points: Vec::new(),
        load: None,
        memory: None,
        swap: None,
        gpus: Vec::new(),
        disks: Vec::new(),
        network: None,
        processes: Vec::new(),
        process_count: 0,
    }
}

fn basis_points(percent: f32) -> u32 {
    (percent * 100.).round() as u32
}

fn load() -> Option<LoadAverage> {
    #[cfg(windows)]
    {
        None
    }
    #[cfg(not(windows))]
    {
        let load = System::load_average();
        let hundredths = |value: f64| (value.max(0.) * 100.).round() as u32;
        Some(LoadAverage {
            one: hundredths(load.one),
            five: hundredths(load.five),
            fifteen: hundredths(load.fifteen),
        })
    }
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
