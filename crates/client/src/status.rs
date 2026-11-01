//! Ordered Node-owned Status state, independent of controller presentation.
use crate::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Fault, Request, Topic, Update,
    plugin::{
        Context,
        desktop::Surface,
        ui::{Kind, status},
    },
};
use std::time::Duration;
use tokio::sync::watch;

pub use status::{Entry, Failure, Scope};

#[derive(Clone, Default, serde::Serialize)]
pub struct View {
    pub entries: Vec<Entry>,
    pub failures: Vec<Failure>,
    pub connected: bool,
    pub error: Option<Fault>,
}

impl Client {
    pub async fn watch_status(
        &self,
        scope: Scope,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) {
        let mut view = View::default();
        if !scope.valid() {
            view.error = Some(Fault::invalid(
                "status scope requires a worktree or session",
            ));
            updates.send_replace(view);
            return;
        }
        loop {
            let stream = tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                _ = updates.closed() => return,
                result = self.transport.subscribe(Topic::Status(scope)) => result,
            };
            let failure = match stream {
                Err(error) => error,
                Ok(mut stream) => loop {
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => return,
                        _ = updates.closed() => return,
                        result = stream.next() => match result {
                            Ok(Update::Status(mut snapshot))
                                if snapshot.node == self.target() && snapshot.scope == scope => {
                                snapshot.entries.retain(|entry| status::valid(&entry.state, &entry.declaration));
                                snapshot.entries.sort_by(|left, right| {
                                    (left.declaration.order, &left.package.name, &left.declaration.id)
                                        .cmp(&(right.declaration.order, &right.package.name, &right.declaration.id))
                                });
                                view.entries = snapshot.entries;
                                view.failures = snapshot.failures;
                                view.connected = true;
                                view.error = None;
                                updates.send_replace(view.clone());
                            }
                            Ok(_) => break Fault::new(ErrorCode::WrongTarget, "status snapshot belongs to another scope"),
                            Err(error) => break error,
                        },
                    }
                },
            };
            view.connected = false;
            view.error = Some(failure);
            updates.send_replace(view.clone());
            tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                _ = updates.closed() => return,
                _ = tokio::time::sleep(Duration::from_millis(500)) => {},
            }
        }
    }

    /// Retain this durable Button request for an explicit retry of an uncertain outcome.
    /// Session actions require the resolved worktree used by plugin admission.
    pub fn prepare_status(&self, scope: &Scope, entry: &Entry) -> Result<Request, Fault> {
        if scope.session.is_some() && scope.worktree.is_none() {
            return Err(Fault::invalid(
                "status action requires the session worktree",
            ));
        }
        if !scope.valid()
            || !status::valid(&entry.state, &entry.declaration)
            || entry.declaration.kind != Kind::Button
            || !entry.state.visible
            || !entry.state.enabled
        {
            return Err(Fault::invalid("status action is unavailable"));
        }
        let handler = entry
            .invoke
            .as_ref()
            .ok_or_else(|| Fault::invalid("status action has no host handler"))?;
        Ok(self
            .prepare(Command::CallPlugin {
                handler: handler.clone(),
                input: serde_json::json!({"id": entry.declaration.id}),
            })
            .with_plugin(Context {
                invocation: None,
                turn: None,
                surface: Surface::Composer,
                package: entry.package.clone(),
                worktree: scope.worktree,
                session: scope.session,
            }))
    }
}

#[cfg(test)]
mod tests;
