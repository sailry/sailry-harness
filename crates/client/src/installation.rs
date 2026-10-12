//! Installation snapshots are observations, never another request execution owner.
use crate::Client;
use sailry_link::{CancellationToken, Subscription};
use sailry_protocol::{
    Command, ErrorCode, Fault, Request, RequestId, Topic, Update, installation::Task,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use tokio::sync::watch;

#[derive(Clone, Default, serde::Serialize)]
pub struct View {
    pub tasks: Vec<Task>,
    pub connected: bool,
    pub error: Option<Fault>,
}

impl View {
    pub fn request(&self, id: RequestId) -> Option<&Task> {
        self.tasks.iter().find(|task| task.request == id)
    }
}

#[derive(Default)]
struct Projection {
    tasks: BTreeMap<RequestId, Task>,
    revisions: BTreeMap<RequestId, u64>,
}

impl Projection {
    fn apply(&mut self, tasks: Vec<Task>) -> Result<Vec<Task>, Fault> {
        let mut seen = BTreeSet::new();
        for task in &tasks {
            if task.revision == 0 || !seen.insert(task.request) {
                return Err(Fault::invalid("invalid installation snapshot"));
            }
            if let sailry_protocol::installation::State::Running {
                progress: Some(progress),
                ..
            } = &task.state
            {
                progress.validate()?;
            }
        }
        let mut current = BTreeMap::new();
        let mut ordered = Vec::with_capacity(tasks.len());
        for task in tasks {
            let previous = self.revisions.get(&task.request).copied();
            let task = match previous {
                Some(revision) if task.revision <= revision => {
                    // A removed request stays absent when an older frame arrives.
                    let Some(previous) = self.tasks.get(&task.request) else {
                        continue;
                    };
                    previous.clone()
                }
                _ => task,
            };
            self.revisions.insert(task.request, task.revision);
            current.insert(task.request, task.clone());
            ordered.push(task);
        }
        self.tasks = current;
        Ok(ordered)
    }
}

struct Closing(watch::Sender<View>);
impl Drop for Closing {
    fn drop(&mut self) {
        self.0.send_modify(|view| view.connected = false);
    }
}

impl Client {
    pub async fn subscribe_installations(&self) -> Result<Box<dyn Subscription>, Fault> {
        self.transport.subscribe(Topic::Installations).await
    }

    pub async fn subscribe_installations_for(
        &self,
        request: Request,
    ) -> Result<Box<dyn Subscription>, Fault> {
        if request.target != self.target() || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let Command::ReadInstallations { requests } = &request.command else {
            return Err(Fault::invalid("installation observation expected"));
        };
        sailry_protocol::installation::validate_requests(requests)?;
        if request.plugin.is_none() {
            return Err(Fault::invalid("plugin scope expected"));
        }
        self.transport
            .subscribe(Topic::ScopedInstallations(request))
            .await
    }

    /// Reconnect refreshes transient observation; it never replays an installation.
    pub async fn watch_installations(
        &self,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        self.watch_installations_with(None, updates, stop).await
    }

    pub async fn watch_installations_for(
        &self,
        request: Request,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        self.watch_installations_with(Some(request), updates, stop)
            .await
    }

    async fn watch_installations_with(
        &self,
        scope: Option<Request>,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        let _closing = Closing(updates.clone());
        let mut delay = Duration::from_millis(250);
        loop {
            let stream = tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                result = async { match &scope {
                    Some(request) => self.subscribe_installations_for(request.clone()).await,
                    None => self.subscribe_installations().await,
                }} => result,
            };
            let error = match stream {
                Err(error) => error,
                Ok(mut stream) => {
                    // A new subscription starts a new Node-lifetime snapshot generation.
                    let mut projection = Projection::default();
                    loop {
                        let next = tokio::select! {
                            biased;
                            _ = stop.cancelled() => return Ok(()),
                            _ = updates.closed() => return Ok(()),
                            next = stream.next() => next,
                        };
                        match next {
                            Ok(Update::Installations { node, tasks }) if node == self.target() => {
                                match projection.apply(tasks) {
                                    Ok(tasks) => {
                                        delay = Duration::from_millis(250);
                                        updates.send_replace(View {
                                            tasks,
                                            connected: true,
                                            error: None,
                                        });
                                    }
                                    Err(error) => break error,
                                }
                            }
                            Ok(_) => {
                                break Fault::new(
                                    ErrorCode::WrongTarget,
                                    "installation snapshot belongs to another Node",
                                );
                            }
                            Err(error) => break error,
                        }
                    }
                }
            };
            updates.send_modify(|view| {
                view.connected = false;
                view.error = Some(error.clone());
            });
            if !matches!(error.code, ErrorCode::Unavailable | ErrorCode::Busy) {
                return Err(error);
            }
            tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                _ = tokio::time::sleep(delay) => {},
            }
            delay = (delay * 2).min(Duration::from_secs(5));
        }
    }
}

#[cfg(test)]
mod tests;
