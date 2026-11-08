use super::*;
use sailry_protocol::plugin::{
    Info,
    mcp::{Runtime, RuntimeState},
};
use std::{collections::BTreeMap, sync::Mutex};
use tokio::sync::Notify;

#[derive(Default)]
pub(super) struct Status {
    records: Mutex<Records>,
    changed: Notify,
}

#[derive(Default)]
struct Records {
    servers: BTreeMap<(String, String), Record>,
    pending: BTreeSet<String>,
    sequence: u64,
}

struct Record {
    reference: Reference,
    sequence: u64,
    runtime: Option<Runtime>,
    settled: Option<Runtime>,
}

pub(super) struct Attempt {
    package: String,
    server: String,
    sequence: u64,
}

impl Status {
    pub(super) fn begin(&self, reference: &Reference, server: &str) -> Attempt {
        let mut records = self.records.lock().unwrap();
        records.sequence += 1;
        let attempt = Attempt {
            package: reference.name.clone(),
            server: server.into(),
            sequence: records.sequence,
        };
        let key = (attempt.package.clone(), attempt.server.clone());
        let previous = records
            .servers
            .remove(&key)
            .filter(|record| record.reference == *reference);
        records.servers.insert(
            key,
            Record {
                reference: reference.clone(),
                sequence: attempt.sequence,
                runtime: previous.as_ref().and_then(|record| record.runtime.clone()),
                settled: previous.and_then(|record| record.settled),
            },
        );
        attempt
    }

    pub(super) fn started(&self, reference: &Reference, server: &str) {
        let mut records = self.records.lock().unwrap();
        let Some(record) = records
            .servers
            .get_mut(&(reference.name.clone(), server.into()))
            .filter(|record| record.reference == *reference)
        else {
            return;
        };
        if record
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.state == RuntimeState::Connecting)
        {
            return;
        }
        record.runtime = Some(runtime(RuntimeState::Connecting, 0));
        self.publish(&mut records, &reference.name);
    }

    pub(super) fn finish(&self, attempt: Attempt, result: Result<usize, Fault>) {
        let mut records = self.records.lock().unwrap();
        let Some(record) = records
            .servers
            .get_mut(&(attempt.package.clone(), attempt.server))
            .filter(|record| record.sequence == attempt.sequence)
        else {
            return;
        };
        let next = match result {
            Ok(tools) => Some(runtime(RuntimeState::Connected, tools)),
            Err(error) if matches!(error.code, ErrorCode::Busy | ErrorCode::Cancelled) => {
                record.settled.clone()
            }
            Err(error) => {
                let mut failed = runtime(RuntimeState::Failed, 0);
                failed.fault = Some(public(error));
                Some(failed)
            }
        };
        if record.runtime != next {
            record.runtime = next.clone();
            record.settled = next;
            self.publish(&mut records, &attempt.package);
        }
    }

    pub(super) fn failed(&self, reference: &Reference, server: &str) {
        let mut records = self.records.lock().unwrap();
        let Some(record) = records
            .servers
            .get_mut(&(reference.name.clone(), server.into()))
            .filter(|record| {
                record.reference == *reference
                    && record
                        .runtime
                        .as_ref()
                        .is_none_or(|runtime| runtime.state != RuntimeState::Failed)
            })
        else {
            return;
        };
        record.runtime = Some(runtime(RuntimeState::Failed, 0));
        record.settled = record.runtime.clone();
        self.publish(&mut records, &reference.name);
    }

    pub(super) fn remove(&self, package: Option<&str>) {
        let mut records = self.records.lock().unwrap();
        let removed: BTreeSet<_> = records
            .servers
            .extract_if(.., |(name, _), _| {
                package.is_none_or(|package| name == package)
            })
            .map(|((name, _), _)| name)
            .collect();
        for package in removed {
            self.publish(&mut records, &package);
        }
    }

    pub(super) fn project(&self, info: &mut Info) {
        let records = self.records.lock().unwrap();
        let reference = info.summary.reference();
        for server in &mut info.mcp {
            server.runtime = records
                .servers
                .get(&(reference.name.clone(), server.name.clone()))
                .filter(|record| {
                    record.reference == reference && info.summary.available() && server.allowed
                })
                .and_then(|record| record.runtime.clone());
        }
    }

    pub(super) async fn changes(&self) -> Vec<String> {
        self.changed.notified().await;
        std::mem::take(&mut self.records.lock().unwrap().pending)
            .into_iter()
            .collect()
    }

    fn publish(&self, records: &mut Records, package: &str) {
        records.pending.insert(package.into());
        self.changed.notify_one();
    }
}

fn public(error: Fault) -> Fault {
    let message = match error.message.as_str() {
        "MCP credentials changed"
        | "MCP OAuth conflicts with a configured authorization header"
        | "MCP process did not confirm normal cleanup"
        | "MCP process reaping timed out"
        | "MCP server is unavailable" => error.message,
        _ => unavailable().message,
    };
    Fault::new(error.code, message)
}

fn runtime(state: RuntimeState, tools: usize) -> Runtime {
    Runtime {
        state,
        tools: u32::try_from(tools).unwrap_or(u32::MAX),
        fault: (state == RuntimeState::Failed).then(unavailable),
        updated_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
    }
}
