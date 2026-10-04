//! Shared command snapshots use subscriptions; selected output is read on demand.
use crate::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Fault, Output, RequestId, SessionId, Topic, Update,
    process::{Info, Snapshot},
};
use std::time::Duration;
use tokio::sync::watch;

#[derive(Clone, Default, serde::Serialize)]
pub struct View {
    pub items: Vec<Info>,
    pub output: Option<Snapshot>,
    pub connected: bool,
    pub error: Option<Fault>,
}

impl Client {
    pub async fn watch_commands(
        &self,
        session: SessionId,
        mut selected: watch::Receiver<Option<RequestId>>,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) {
        let mut view = View::default();
        loop {
            let stream = tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                _ = updates.closed() => return,
                result = self.transport.subscribe(Topic::Commands(session)) => result,
            };
            let failure = match stream {
                Err(error) => error,
                Ok(mut stream) => {
                    let mut refresh = tokio::time::interval(Duration::from_millis(500));
                    loop {
                        tokio::select! {
                            biased;
                            _ = stop.cancelled() => return,
                            _ = updates.closed() => return,
                            result = stream.next() => match result {
                                Ok(Update::Commands { node, session: owner, items })
                                    if node == self.target() && owner == session
                                        && items.iter().all(|item| item.session == session) => {
                                    view.items = items;
                                    view.connected = true;
                                    view.error = None;
                                    if view.output.as_ref().is_some_and(|output| !view.items.iter().any(|item| item.id == output.info.id)) {
                                        view.output = None;
                                    }
                                    updates.send_replace(view.clone());
                                }
                                Ok(_) => break Fault::new(ErrorCode::WrongTarget, "command snapshot belongs to another scope"),
                                Err(error) => break error,
                            },
                            result = selected.changed() => {
                                if result.is_err() { return; }
                                view.output = None;
                                updates.send_replace(view.clone());
                                refresh.reset_immediately();
                            },
                            _ = refresh.tick(), if view.connected && selected.borrow().is_some() => {
                                let id = *selected.borrow_and_update();
                                let Some(id) = id.filter(|id| view.items.iter().any(|info| info.id == *id)) else { continue };
                                let result = tokio::select! {
                                    _ = stop.cancelled() => return,
                                    _ = updates.closed() => return,
                                    result = self.execute(self.prepare(Command::ReadCommand { session, id })) => result,
                                };
                                match result {
                                    Ok(Output::CommandOutput(output)) if output.info.session == session && output.info.id == id => {
                                        if *selected.borrow() == Some(id) { view.output = Some(output); }
                                    },
                                    Ok(_) => break Fault::new(ErrorCode::Internal, "command output expected"),
                                    Err(error) => break error,
                                }
                                updates.send_replace(view.clone());
                            },
                        }
                    }
                }
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
}

#[cfg(test)]
mod tests;
