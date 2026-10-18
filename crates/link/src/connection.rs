//! Controller-local connection telemetry; it is not part of the Node wire protocol.
use crate::CancellationToken;
use serde::Serialize;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Local,
    P2p,
    Relay,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ConnectionInfo {
    pub route: Option<Route>,
    pub rtt_ms: Option<u64>,
}

pub(crate) struct Closing(pub(crate) watch::Sender<ConnectionInfo>);

impl Drop for Closing {
    fn drop(&mut self) {
        self.0.send_replace(ConnectionInfo::default());
    }
}

pub(crate) async fn hold(
    updates: watch::Sender<ConnectionInfo>,
    stop: CancellationToken,
    value: ConnectionInfo,
) {
    let _closing = Closing(updates.clone());
    updates.send_replace(value);
    tokio::select! {
        _ = stop.cancelled() => {},
        _ = updates.closed() => {},
    }
}
