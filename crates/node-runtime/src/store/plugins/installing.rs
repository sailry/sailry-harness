//! Package preparation waits outside the shared file mutation queue.
use crate::store::{Ingress, Job, external::Completed, mutations::Mutation, unavailable};
use futures::FutureExt;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Fault, Output, plugin::Info};
use std::{collections::BTreeMap, panic::AssertUnwindSafe, sync::Arc};
use tokio::{runtime::Handle, sync::oneshot};

pub(in crate::store) fn handles(command: &Command) -> bool {
    name(command).is_some()
}

fn name(command: &Command) -> Option<&str> {
    match command {
        Command::InstallPluginUpload { name, .. }
        | Command::InstallPlugin { name, .. }
        | Command::InstallPluginSource { name, .. }
        | Command::InstallBundledPlugin { name, .. }
        | Command::InstallSkill { name, .. }
        | Command::InstallMcp { name, .. }
        | Command::SetPluginEnabled {
            name,
            enabled: true,
            ..
        } => Some(name),
        _ => None,
    }
}

pub(in crate::store) struct Worker {
    pub pending: usize,
    ingress: Arc<Ingress>,
    runtime: Handle,
    preceding: BTreeMap<String, oneshot::Receiver<()>>,
}

impl Worker {
    pub fn new(ingress: Arc<Ingress>) -> Self {
        Self {
            pending: 0,
            ingress,
            runtime: Handle::current(),
            preceding: BTreeMap::new(),
        }
    }

    pub fn submit(&mut self, mutation: Mutation) -> Result<(), Box<Mutation>> {
        if self.pending >= 8 || self.ingress.closed.is_cancelled() {
            return Err(Box::new(mutation));
        }
        let stop = if let Some(turn) = mutation
            .request
            .plugin
            .as_ref()
            .and_then(|entry| entry.turn)
        {
            let Some(stop) = self.ingress.agents.python_stop(turn) else {
                return Err(Box::new(mutation));
            };
            stop.child_token()
        } else {
            self.ingress.closed.child_token()
        };
        let package = name(&mutation.request.command)
            .expect("package installation command expected")
            .to_owned();
        retain(&mut self.preceding);
        let (release, waiting) = oneshot::channel();
        // Requests for one package retain admission order even when one of them
        // must download dependencies. Unrelated packages may prepare concurrently.
        let preceding = self.preceding.insert(package, waiting);
        self.pending += 1;
        let ingress = self.ingress.clone();
        self.runtime.spawn(async move {
            let closing = ingress.closed.clone();
            let preparing = stop.clone();
            let watcher = tokio::spawn(async move {
                closing.cancelled().await;
                preparing.cancel();
            });
            let operation = async {
                wait(preceding, &stop).await?;
                let info = acquire(&ingress, &mutation, &stop).await?;
                check(&stop)?;
                if let Command::InstallPlugin {
                    expected_digest: Some(expected),
                    ..
                } = &mutation.request.command
                    && info.summary.digest != *expected
                {
                    return Err(Fault::new(
                        ErrorCode::RevisionConflict,
                        "plugin source changed; check updates again",
                    ));
                }
                ingress
                    .plugins
                    .prepare_python(&info, &ingress.environment, &stop)
                    .await?;
                check(&stop)?;
                Ok(Output::Plugin(info))
            };
            let result = AssertUnwindSafe(operation)
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "plugin preparation failed; inspect the package before retrying",
                    ))
                });
            watcher.abort();
            let _ = ingress
                .sender
                .send(Job::Installed {
                    completed: Box::new(Completed {
                        caller: mutation.caller,
                        request: mutation.request,
                        result,
                        reply: mutation.reply,
                    }),
                    release,
                })
                .await;
        });
        Ok(())
    }
}

async fn wait(
    preceding: Option<oneshot::Receiver<()>>,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    // Even a cancelled middle request must keep its position until the previous
    // request has committed, so later requests cannot overtake that predecessor.
    if let Some(preceding) = preceding {
        let _ = preceding.await;
    }
    check(stop)
}

fn retain(preceding: &mut BTreeMap<String, oneshot::Receiver<()>>) {
    preceding.retain(|_, waiting| {
        matches!(waiting.try_recv(), Err(oneshot::error::TryRecvError::Empty))
    });
}

async fn acquire(
    ingress: &Ingress,
    mutation: &Mutation,
    stop: &CancellationToken,
) -> Result<Info, Fault> {
    match &mutation.request.command {
        Command::InstallPluginSource {
            source, path, name, ..
        } => {
            ingress
                .plugins
                .install_source(source, path, name, stop.clone())
                .await
        }
        Command::InstallSkill {
            source, path, name, ..
        } => {
            ingress
                .plugins
                .install_skill(source, path, name, stop.clone())
                .await
        }
        Command::SetPluginEnabled {
            name,
            expected_revision,
            enabled: true,
        } => {
            let (reply, response) = oneshot::channel();
            ingress
                .sender
                .send(Job::PluginPreparation {
                    name: name.clone(),
                    revision: *expected_revision,
                    reply,
                })
                .await
                .map_err(|_| unavailable())?;
            tokio::select! {
                biased;
                _ = stop.cancelled() => Err(cancelled()),
                result = response => result.map_err(|_| unavailable())?,
            }
        }
        _ => {
            let host = ingress.plugins.clone();
            let transfers = ingress.transfers.clone();
            let caller = mutation.caller;
            let root = mutation.roots.target.clone();
            let command = mutation.request.command.clone();
            tokio::task::spawn_blocking(move || match command {
                Command::InstallPluginUpload { stream, name, .. } => {
                    transfers.install_plugin(caller, stream, &name, &host)
                }
                Command::InstallPlugin { path, name, .. } => host.install(&root, &path, &name),
                Command::InstallBundledPlugin { name, .. } => host.install_bundled(&name),
                Command::InstallMcp {
                    name, definition, ..
                } => host.install_mcp(&name, &definition),
                _ => unreachable!("package acquisition command expected"),
            })
            .await
            .map_err(|_| unavailable())?
        }
    }
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "plugin preparation cancelled")
}

fn check(stop: &CancellationToken) -> Result<(), Fault> {
    if stop.is_cancelled() {
        Err(cancelled())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_keeps_its_queue_position() {
        let (committed, preceding) = oneshot::channel();
        let stop = CancellationToken::new();
        stop.cancel();
        let mut queued = Box::pin(wait(Some(preceding), &stop));
        assert!(queued.as_mut().now_or_never().is_none());
        committed.send(()).unwrap();
        assert_eq!(queued.await.unwrap_err().code, ErrorCode::Cancelled);
    }

    #[tokio::test]
    async fn cleanup_preserves_pending_predecessors() {
        let mut preceding = BTreeMap::new();
        let (committed, completed) = oneshot::channel();
        committed.send(()).unwrap();
        preceding.insert("completed".into(), completed);
        let (abandoned, closed) = oneshot::channel();
        drop(abandoned);
        preceding.insert("closed".into(), closed);
        let (committed, pending) = oneshot::channel();
        preceding.insert("pending".into(), pending);
        retain(&mut preceding);
        assert_eq!(preceding.len(), 1);
        committed.send(()).unwrap();
        preceding.remove("pending").unwrap().await.unwrap();
    }
}
