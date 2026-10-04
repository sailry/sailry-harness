use super::*;
use sailry_protocol::SessionId;
use std::{collections::BTreeMap, sync::Mutex, time::Instant};

const MAX_IDLE: usize = 8;

struct Idle {
    since: Instant,
    connections: Connections,
}

/// Only idle connections are cached. A claimed turn exclusively owns its session's set.
#[derive(Default)]
pub(crate) struct Pool {
    idle: Mutex<BTreeMap<SessionId, Idle>>,
}

impl Pool {
    pub(in crate::agent) fn take(&self, session: SessionId) -> Connections {
        self.idle
            .lock()
            .unwrap()
            .remove(&session)
            .map(|idle| idle.connections)
            .unwrap_or_default()
    }

    pub(in crate::agent) async fn put(
        &self,
        session: SessionId,
        connections: Connections,
    ) -> Result<(), Fault> {
        if connections.active.is_empty() {
            return Ok(());
        }
        let replaced = {
            let mut idle = self.idle.lock().unwrap();
            let expired = if idle.len() >= MAX_IDLE {
                let oldest = idle
                    .iter()
                    .min_by_key(|(_, value)| value.since)
                    .map(|(id, _)| *id)
                    .unwrap();
                idle.remove(&oldest)
            } else {
                None
            };
            let previous = idle.insert(
                session,
                Idle {
                    since: Instant::now(),
                    connections,
                },
            );
            expired.into_iter().chain(previous).collect::<Vec<_>>()
        };
        close(replaced).await
    }

    pub(crate) async fn close(&self) -> Result<(), Fault> {
        let idle = std::mem::take(&mut *self.idle.lock().unwrap());
        close(idle.into_values().collect()).await
    }
}

async fn close(idle: Vec<Idle>) -> Result<(), Fault> {
    let results = futures::future::join_all(
        idle.into_iter()
            .map(|mut idle| async move { idle.connections.close().await }),
    )
    .await;
    results.into_iter().collect()
}
