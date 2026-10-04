//! Report refresh follows the existing Node observer; there is no polling or usage ledger.
use crate::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Fault, Output,
    usage::{Query, Report},
};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

mod overview;
pub use overview::{Overview, ScopedGroup, Source, Summary, watch_overview};

#[derive(Clone, Default, serde::Serialize)]
pub struct View {
    pub report: Option<Arc<Report>>,
    pub connected: bool,
    pub refreshing: bool,
    pub error: Option<Fault>,
}

struct Closing(watch::Sender<View>);
impl Drop for Closing {
    fn drop(&mut self) {
        self.0.send_modify(|view| {
            view.connected = false;
            view.refreshing = false;
        });
    }
}

impl Client {
    pub async fn read_usage(&self, query: Query) -> Result<Report, Fault> {
        let mut request = self.prepare(Command::ReadUsage(query.clone()));
        if let Some((scope, resolve)) = &self.usage_scope {
            let mut scope = scope.clone();
            if *resolve {
                let Output::Plugins(packages) =
                    self.execute(self.prepare(Command::ListPlugins)).await?
                else {
                    return Err(Fault::new(
                        ErrorCode::Internal,
                        "plugin inventory response expected",
                    ));
                };
                let package = packages
                    .iter()
                    .find(|package| {
                        package.name == scope.package.name && package.digest == scope.package.digest
                    })
                    .ok_or_else(|| {
                        Fault::new(
                            ErrorCode::NotConfigured,
                            "matching usage package is unavailable",
                        )
                    })?;
                scope.package = package.reference();
            }
            request = request.with_plugin(scope);
        }
        let Output::Usage(report) = self.execute(request).await? else {
            return Err(Fault::new(
                ErrorCode::Internal,
                "usage report response expected",
            ));
        };
        if report.node != self.target() || report.query != query {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "usage report belongs to another Node or query",
            ));
        }
        if report.requests.items.len() > sailry_protocol::usage::PAGE_SIZE
            || report.requests.items.iter().any(|request| {
                request.position.node != report.node
                    || request.position.timestamp_ms < query.start_ms
                    || request.position.timestamp_ms >= query.end_ms
                    || query
                        .before
                        .is_some_and(|before| request.position >= before)
            })
            || report
                .requests
                .items
                .windows(2)
                .any(|pair| pair[0].position <= pair[1].position)
        {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "usage request page is invalid",
            ));
        }
        Ok(report)
    }

    /// Keep the query fixed for this observer. Closing it cancels reads, never model work.
    pub async fn watch_usage(
        &self,
        query: Query,
        updates: watch::Sender<View>,
        stop: CancellationToken,
        refresh: mpsc::Receiver<()>,
    ) -> Result<(), Fault> {
        let _closing = Closing(updates.clone());
        updates.send_modify(|view| {
            if view
                .report
                .as_ref()
                .is_some_and(|report| report.node != self.target() || report.query != query)
            {
                *view = View::default();
            }
        });
        let (nodes, state) = watch::channel(crate::View::default());
        tokio::select! {
            biased;
            _ = stop.cancelled() => Ok(()),
            _ = updates.closed() => Ok(()),
            result = self.watch(nodes, stop.clone()) => {
                if let Err(error) = &result {
                    updates.send_modify(|view| view.error = Some(error.clone()));
                }
                result
            },
            result = self.refresh_usage(query, &updates, state, refresh) => result,
        }
    }

    async fn refresh_usage(
        &self,
        query: Query,
        updates: &watch::Sender<View>,
        mut nodes: watch::Receiver<crate::View>,
        mut refresh: mpsc::Receiver<()>,
    ) -> Result<(), Fault> {
        let mut requests_open = true;
        loop {
            let forced = tokio::select! {
                result = nodes.changed() => {
                    if result.is_err() { return Ok(()); }
                    false
                },
                request = refresh.recv(), if requests_open => {
                    if request.is_none() { requests_open = false; continue; }
                    true
                },
            };
            let node = nodes.borrow_and_update().clone();
            if !node.connected {
                updates.send_modify(|view| {
                    view.connected = false;
                    view.error = node.error.clone();
                });
                continue;
            }
            let Some(snapshot) = node.snapshot else {
                continue;
            };
            let current = updates.borrow().report.clone();
            if !forced
                && updates.borrow().connected
                && current
                    .as_ref()
                    .is_some_and(|report| report.cursor >= snapshot.cursor)
            {
                updates.send_modify(|view| view.connected = true);
                continue;
            }
            updates.send_modify(|view| {
                view.connected = true;
                view.refreshing = true;
            });
            let result = self.read_usage(query.clone()).await;
            let node = nodes.borrow().clone();
            updates.send_modify(|view| {
                view.connected = node.connected;
                view.refreshing = false;
                match result {
                    Ok(report)
                        if current
                            .as_ref()
                            .is_none_or(|current| report.cursor >= current.cursor) =>
                    {
                        view.report = Some(Arc::new(report));
                        view.error = node.error;
                    }
                    Ok(_) => {
                        view.error = Some(Fault::new(
                            ErrorCode::InvalidRequest,
                            "usage report cursor moved backwards",
                        ))
                    }
                    Err(error) => {
                        if self.usage_scope.is_some()
                            && matches!(
                                error.code,
                                ErrorCode::NotConfigured
                                    | ErrorCode::NotFound
                                    | ErrorCode::PermissionDenied
                                    | ErrorCode::RevisionConflict
                            )
                        {
                            view.report = None;
                        }
                        view.error = Some(error);
                    }
                }
            });
        }
    }
}
