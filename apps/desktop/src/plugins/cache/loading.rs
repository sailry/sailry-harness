//! Capacity contention may delay a view read without invalidating its captured owner.
use super::Cache;
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Fault, Output, plugin};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, sleep_until};

pub(in crate::plugins) async fn prepare(
    client: Arc<Client>,
    runtime: tokio::runtime::Handle,
    package: plugin::Reference,
    surface: plugin::desktop::Surface,
    stop: CancellationToken,
) -> Option<Result<Arc<Cache>, &'static str>> {
    let bundle = match read(&client, package.clone(), surface, &stop).await? {
        Ok(Output::PluginView(bundle)) => bundle,
        Err(error) => {
            eprintln!(
                "sailry-desktop: plugin {} view read failed: {error}",
                package.name
            );
            return Some(Err("plugins_view_read_failed"));
        }
        Ok(_) => return Some(Err("plugins_view_read_failed")),
    };
    if stop.is_cancelled() {
        return None;
    }
    let worker = runtime.clone();
    let result = runtime
        .spawn_blocking(move || Cache::create(bundle, &package, worker))
        .await;
    if stop.is_cancelled() {
        return None;
    }
    Some(match result {
        Ok(Ok(cache)) => Ok(Arc::new(cache)),
        Ok(Err(error)) => {
            eprintln!("sailry-desktop: plugin view cache failed: {error}");
            Err("plugins_view_read_failed")
        }
        Err(error) => {
            eprintln!("sailry-desktop: plugin view cache task failed: {error}");
            Err("plugins_view_read_failed")
        }
    })
}

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
