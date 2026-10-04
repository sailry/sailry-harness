//! Shared transient host history; closing the receiver stops read-only sampling.
use crate::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Fault, Output, host::metrics::*};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::sync::watch;

#[cfg(test)]
mod tests;

pub const HISTORY_LIMIT: usize = 60;

#[derive(Clone, Debug, Default)]
pub struct View {
    pub sample: Option<Arc<Sample>>,
    pub history: VecDeque<Point>,
    pub processes: BTreeMap<(u32, u64), VecDeque<Option<u32>>>,
    pub error: Option<Fault>,
}

#[derive(Clone, Debug)]
pub struct Point {
    pub sampled_at_ms: u64,
    pub cpu_basis_points: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub gpus: BTreeMap<String, Option<u32>>,
    pub disks: BTreeMap<String, Option<IoRate>>,
    pub network: Option<NetworkRate>,
}

impl View {
    pub fn accept(&mut self, sample: Sample) -> Result<(), Fault> {
        if let Some(previous) = &self.sample {
            if previous.node != sample.node {
                return Err(Fault::new(
                    ErrorCode::WrongTarget,
                    "host sample belongs to another Node",
                ));
            }
            if previous.sampled_at_ms >= sample.sampled_at_ms {
                return Ok(());
            }
            if sample.sampled_at_ms - previous.sampled_at_ms > 5000 {
                self.history.clear();
                self.processes.clear();
            }
        }
        self.history.push_back(Point {
            network: sample.network.clone(),
            sampled_at_ms: sample.sampled_at_ms,
            cpu_basis_points: sample.cpu_basis_points,
            memory_bytes: sample
                .memory
                .as_ref()
                .map(|memory| memory.total_bytes - memory.available_bytes),
            gpus: sample
                .gpus
                .iter()
                .map(|gpu| (gpu.id.clone(), gpu.usage_basis_points))
                .collect(),
            disks: sample
                .disks
                .iter()
                .map(|disk| (disk.mount.clone(), disk.io.clone()))
                .collect(),
        });
        if self.history.len() > HISTORY_LIMIT {
            self.history.pop_front();
        }
        let mut processes = BTreeMap::new();
        for process in &sample.processes {
            let key = (process.pid, process.started_at_secs);
            let mut history = self.processes.remove(&key).unwrap_or_default();
            history.push_back(process.cpu_basis_points);
            if history.len() > HISTORY_LIMIT {
                history.pop_front();
            }
            processes.insert(key, history);
        }
        self.processes = processes;
        self.sample = Some(Arc::new(sample));
        self.error = None;
        Ok(())
    }
}

impl Client {
    pub async fn watch_host(&self, sender: watch::Sender<View>, stop: CancellationToken) {
        let mut view = View::default();
        loop {
            let result = tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                _ = sender.closed() => return,
                result = tokio::time::timeout(Duration::from_secs(5), self.execute(self.prepare(Command::ReadHostMetrics))) => result,
            };
            let result = match result {
                Ok(Ok(Output::HostMetrics(sample))) if sample.node == self.target() => {
                    view.accept(sample)
                }
                Ok(Ok(_)) => Err(Fault::new(
                    ErrorCode::Internal,
                    "host metrics response expected",
                )),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(Fault::new(
                    ErrorCode::Unavailable,
                    "host sampling timed out",
                )),
            };
            if let Err(error) = result {
                view.error = Some(error);
            }
            if sender.send(view.clone()).is_err() {
                return;
            }
            tokio::select! {
                _ = stop.cancelled() => return,
                _ = sender.closed() => return,
                _ = tokio::time::sleep(Duration::from_secs(1)) => {},
            }
        }
    }
}
