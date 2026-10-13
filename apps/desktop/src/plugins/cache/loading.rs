//! Capacity contention may delay a view read without invalidating its captured owner.
use super::Cache;
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_node_runtime::logging::{Area, Context, Entry, Phase, Span};
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
    let mut entry = Entry::new(Area::Plugin, Phase::Begin);
    entry.context = Context {
        node: Some(client.target()),
        package: Some(package.clone()),
        ..Default::default()
    };
    entry.reason = Some("view_prepare");
    let mut diagnostic = Span::new(entry).cancel_when(stop.clone());
    let bundle = match read(&client, package.clone(), surface, &stop).await? {
        Ok(Output::PluginView(bundle)) => bundle,
        Err(error) => {
            diagnostic.entry.reason = Some("view_read");
            diagnostic.finish(Phase::Failed, Some(error.code));
            return Some(Err("plugins_view_read_failed"));
        }
        Ok(_) => {
            diagnostic.entry.reason = Some("view_output");
            diagnostic.finish(Phase::Failed, Some(ErrorCode::InvalidRequest));
            return Some(Err("plugins_view_read_failed"));
        }
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
        Ok(Ok(cache)) => {
            diagnostic.finish(Phase::Completed, None);
            Ok(Arc::new(cache))
        }
        Ok(Err(_)) => {
            diagnostic.entry.reason = Some("view_cache");
            diagnostic.finish(Phase::Failed, Some(ErrorCode::Unavailable));
            Err("plugins_view_read_failed")
        }
        Err(_) => {
            diagnostic.entry.reason = Some("view_worker");
            diagnostic.finish(Phase::Failed, Some(ErrorCode::Internal));
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
