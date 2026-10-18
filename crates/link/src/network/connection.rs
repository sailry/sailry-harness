//! Selected path and its RTT are sampled together on the existing authenticated connection.
use super::*;
use crate::{ConnectionInfo, Route};
use futures::StreamExt;

pub(super) fn snapshot(connection: &Connection) -> ConnectionInfo {
    if connection.close_reason().is_some() {
        return ConnectionInfo::default();
    }
    let paths = connection.paths();
    let Some(path) = paths.iter().find(|path| path.is_selected()) else {
        return ConnectionInfo::default();
    };
    let route = if path.is_ip() {
        Some(Route::P2p)
    } else if path.is_relay() {
        Some(Route::Relay)
    } else {
        None
    };
    ConnectionInfo {
        route,
        rtt_ms: route.map(|_| path.rtt().as_millis().min(u64::MAX as u128) as u64),
    }
}

/// One sampler per cached connection; subscribers only copy its latest value.
pub(super) async fn monitor(
    connection: Connection,
    updates: watch::Sender<ConnectionInfo>,
    stop: CancellationToken,
) {
    let _closing = crate::connection::Closing(updates.clone());
    let mut paths = connection.paths_stream();
    // RTT-only changes do not emit path events. This reads local QUIC statistics.
    let mut sampling = tokio::time::interval(Duration::from_secs(1));
    sampling.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            _ = updates.closed() => break,
            _ = connection.closed() => break,
            next = paths.next() => if next.is_none() { break; },
            _ = sampling.tick() => {},
        }
        let current = snapshot(&connection);
        updates.send_if_modified(|previous| {
            if *previous == current {
                false
            } else {
                *previous = current;
                true
            }
        });
    }
}

pub(super) async fn observe(
    core: &Core,
    address: &EndpointAddr,
    updates: watch::Sender<ConnectionInfo>,
    stop: CancellationToken,
) {
    let _closing = crate::connection::Closing(updates.clone());
    loop {
        let connected = tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            _ = core.stop.cancelled() => break,
            _ = updates.closed() => break,
            result = core.connection(address, false) => result,
        };
        if connected.is_ok() {
            let receiver = core
                .outgoing
                .lock()
                .await
                .get(&NodeId(*address.id.as_bytes()))
                .filter(|cached| cached.address.id == address.id)
                .map(|cached| cached.telemetry.clone());
            if let Some(mut receiver) = receiver {
                loop {
                    updates.send_replace(*receiver.borrow_and_update());
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => break,
                        _ = core.stop.cancelled() => break,
                        _ = updates.closed() => break,
                        result = receiver.changed() => if result.is_err() { break; },
                    }
                }
            }
        }
        updates.send_replace(ConnectionInfo::default());
        tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            _ = core.stop.cancelled() => break,
            _ = updates.closed() => break,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {},
        }
    }
}
