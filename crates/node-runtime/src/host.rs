//! Execution-host sampling. Historical ownership reference: sailry-code 67ae9fa0,
//! sailry-host-runtime/src/code_runtime/host_metrics.rs. No UI polling loop is imported.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sailry_protocol::{ErrorCode, Fault, HostInfo, MemoryUsage, NodeId};
use sysinfo::System;
use tokio::sync::Semaphore;

mod metrics;
pub(crate) mod processes;

pub(crate) struct Host {
    started: Instant,
    cached: Mutex<Option<(Instant, HostInfo)>>,
    capacity: Arc<Semaphore>,
    sampler: Mutex<metrics::Sampler>,
}

impl Host {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            started: Instant::now(),
            cached: Mutex::new(None),
            capacity: Arc::new(Semaphore::new(4)),
            sampler: Mutex::new(metrics::Sampler::new()),
        })
    }

    pub(crate) async fn inspect(self: &Arc<Self>, node: NodeId) -> Result<HostInfo, Fault> {
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "host sampling capacity exhausted"))?;
        let host = self.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut cached = host.cached.lock().map_err(|_| unavailable())?;
            if let Some((sampled, info)) = cached.as_ref()
                && sampled.elapsed() < Duration::from_secs(1)
            {
                return Ok(info.clone());
            }
            let mut system = System::new();
            system.refresh_memory();
            let total = system.total_memory();
            let info = HostInfo {
                node,
                sampled_at_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| unavailable())?
                    .as_millis() as u64,
                name: System::host_name(),
                os: System::name(),
                os_version: System::os_version(),
                architecture: Some(System::cpu_arch()),
                logical_cpus: std::thread::available_parallelism()
                    .ok()
                    .and_then(|value| u32::try_from(value.get()).ok()),
                memory: (total > 0).then(|| MemoryUsage {
                    total_bytes: total,
                    available_bytes: system.available_memory().min(total),
                }),
                host_uptime_secs: System::uptime(),
                node_uptime_secs: host.started.elapsed().as_secs(),
            };
            *cached = Some((Instant::now(), info.clone()));
            Ok(info)
        })
        .await
        .map_err(|_| unavailable())?
    }

    pub(crate) async fn sample(
        self: &Arc<Self>,
        node: NodeId,
    ) -> Result<sailry_protocol::host::metrics::Sample, Fault> {
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "host sampling capacity exhausted"))?;
        let host = self.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            host.sampler.lock().map_err(|_| unavailable())?.sample(node)
        })
        .await
        .map_err(|_| unavailable())?
    }
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "host information is unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reuses_bounded_samples() {
        let host = Host::new();
        let occupied = host.capacity.clone().acquire_many_owned(4).await.unwrap();
        assert_eq!(
            host.inspect(NodeId([1; 32])).await.unwrap_err().code,
            ErrorCode::Busy
        );
        drop(occupied);
        let first = host.inspect(NodeId([1; 32])).await.unwrap();
        let second = host.inspect(NodeId([1; 32])).await.unwrap();
        assert_eq!(first, second);
        assert_eq!(host.capacity.available_permits(), 4);
    }
}
