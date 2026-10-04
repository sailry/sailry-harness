//! Capacity contention may delay a view read without invalidating its captured owner.
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Fault, Output, plugin};
use std::time::Duration;
use tokio::time::{Instant, sleep_until};

pub(super) async fn read(
    client: &Client,
    package: plugin::Reference,
    surface: plugin::desktop::Surface,
    stop: &CancellationToken,
) -> Option<Result<Output, Fault>> {
    let request = client.prepare(Command::ReadPluginView { package, surface });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => return None,
            result = client.execute(request.clone()) => result,
        };
        match result {
            Err(error) if error.code == ErrorCode::Busy && Instant::now() < deadline => {
                let next = (Instant::now() + Duration::from_millis(100)).min(deadline);
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => return None,
                    _ = sleep_until(next) => {
                        if Instant::now() >= deadline {
                            return Some(Err(error));
                        }
                    }
                }
            }
            result => return Some(result),
        }
    }
}
