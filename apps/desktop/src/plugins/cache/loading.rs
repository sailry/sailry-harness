//! Capacity contention may delay a view read without invalidating its captured owner.
use super::{Cache, Failure};
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_node_runtime::logging::{Area, Context, Entry, Phase, Span};
use sailry_protocol::{Command, ErrorCode, Fault, Output, plugin};
use std::sync::Arc;
use std::time::Duration;

pub(in crate::plugins) async fn prepare(
    client: Arc<Client>,
    runtime: tokio::runtime::Handle,
    package: plugin::Reference,
    surface: plugin::desktop::Surface,
    stop: CancellationToken,
) -> Option<Result<Arc<Cache>, Failure>> {
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
            return Some(Err(Failure {
                key: "plugins_view_read_failed",
                code: error.code,
                stage: "view_read",
            }));
        }
        Ok(_) => {
            diagnostic.entry.reason = Some("view_output");
            diagnostic.finish(Phase::Failed, Some(ErrorCode::InvalidRequest));
            return Some(Err(Failure {
                key: "plugins_view_read_failed",
                code: ErrorCode::InvalidRequest,
                stage: "view_output",
            }));
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
        Ok(Err(error)) => {
            diagnostic.entry.reason = Some("view_cache");
            diagnostic.finish(Phase::Failed, Some(error.code));
            Err(Failure {
                key: "plugins_view_read_failed",
                code: error.code,
                stage: "view_cache",
            })
        }
        Err(_) => {
            diagnostic.entry.reason = Some("view_worker");
            diagnostic.finish(Phase::Failed, Some(ErrorCode::Internal));
            Err(Failure {
                key: "plugins_view_read_failed",
                code: ErrorCode::Internal,
                stage: "view_worker",
            })
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
    for attempt in 0..3 {
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => return None,
            result = tokio::time::timeout(Duration::from_secs(10), client.execute(request.clone())) => {
                result.unwrap_or_else(|_| Err(Fault::new(ErrorCode::Unavailable, "plugin view read timed out")))
            },
        };
        match result {
            Err(error)
                if matches!(error.code, ErrorCode::Busy | ErrorCode::Unavailable)
                    && attempt < 2 =>
            {
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => return None,
                    _ = tokio::time::sleep(Duration::from_millis(250 * (1 << attempt))) => {},
                }
            }
            result => return Some(result),
        }
    }
    unreachable!("bounded read returns its final result")
}
