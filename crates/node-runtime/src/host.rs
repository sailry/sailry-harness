//! Execution-host sampling. Historical ownership reference: sailry-code 67ae9fa0,
//! sailry-host-runtime/src/code_runtime/host_metrics.rs. No UI polling loop is imported.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sailry_protocol::{ErrorCode, Fault, HostInfo, MemoryUsage, NodeId};
use sysinfo::System;

mod metrics;
pub(crate) mod processes;

pub(crate) struct Host {
    started: Instant,
    cached: Mutex<Option<(Instant, HostInfo)>>,
    sampler: Mutex<metrics::Sampler>,
}

impl Host {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            started: Instant::now(),
            cached: Mutex::new(None),
            sampler: Mutex::new(metrics::Sampler::new()),
        })
    }

    pub(crate) async fn inspect(self: &Arc<Self>, node: NodeId) -> Result<HostInfo, Fault> {
        let host = self.clone();
        tokio::task::spawn_blocking(move || {
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
                    free_bytes: system.free_memory().min(total),
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
        let host = self.clone();
        tokio::task::spawn_blocking(move || {
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
    use std::{future::Future, task::Poll};

    #[tokio::test]
    async fn shares_information_with_concurrent_readers() {
        let host = Host::new();
        let reads = (0..12).map(|_| host.inspect(NodeId([1; 32])));
        let reads = futures::future::join_all(reads);
        let mut reads = std::pin::pin!(reads);
        std::future::poll_fn(|context| {
            let _cached = host.cached.lock().unwrap();
            assert!(reads.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        let results = reads.await;
        let first = results[0].as_ref().unwrap();
        for result in &results {
            assert_eq!(result.as_ref().unwrap(), first);
        }
    }
}
