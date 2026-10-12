//! Transient metadata reads recover without changing the authoritative inventory.
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Output,
    plugin::{Info, Summary},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

pub(super) struct Batch {
    pub results: Vec<(String, Result<Info, &'static str>)>,
    pub retrying: bool,
}

pub(super) async fn load(
    client: Arc<Client>,
    mut missing: Vec<Summary>,
    stop: CancellationToken,
    sender: mpsc::Sender<Batch>,
) {
    let mut delay = Duration::from_millis(250);
    loop {
        let mut results = Vec::new();
        let mut retry = Vec::new();
        for expected in missing {
            let result = tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                result = client.execute(client.prepare(Command::ReadPlugin { name: expected.name.clone() })) => result,
            };
            if result
                .as_ref()
                .is_err_and(|error| matches!(error.code, ErrorCode::Busy | ErrorCode::Unavailable))
            {
                retry.push(expected.clone());
            }
            let result = match result {
                Ok(Output::Plugin(info)) if info.summary == expected => Ok(info),
                Ok(Output::Plugin(_)) => Err("plugins_conflict"),
                _ => Err("plugins_read_failed"),
            };
            results.push((expected.name, result));
        }
        let retrying = !retry.is_empty();
        tokio::select! {
            biased;
            _ = stop.cancelled() => return,
            result = sender.send(Batch { results, retrying }) => {
                if result.is_err() { return; }
            },
        }
        if !retrying {
            return;
        }
        // A saturated or disconnected Node must not permanently hide its menus.
        // Keep failures visible while this owner performs bounded, cancellable reads.
        tokio::select! {
            biased;
            _ = stop.cancelled() => return,
            _ = sender.closed() => return,
            _ = tokio::time::sleep(delay) => {},
        }
        delay = (delay * 2).min(Duration::from_secs(5));
        missing = retry;
    }
}
