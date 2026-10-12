use super::*;
use crate::store::{Ingress, Job, unavailable};
use sailry_link::Admission;
use sailry_protocol::plugin::updates::{Batch, Failure, Request as Selection};
use sailry_protocol::{Receipt, Request, VERSION};
use tokio::sync::oneshot;

#[cfg(test)]
mod tests;

pub(in crate::store) fn current(db: &Connection, name: &str, revision: u64) -> Result<Info, Fault> {
    let info = required(db, name)?;
    crate::store::commands::check_revision(info.summary.revision, revision)?;
    Ok(info)
}

impl Ingress {
    pub(in crate::store) async fn check_plugin_update(
        &self,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let (selections, single) = match &request.command {
            Command::CheckPluginUpdate {
                name,
                expected_revision,
            } => (
                vec![Selection {
                    name: name.clone(),
                    expected_revision: *expected_revision,
                }],
                true,
            ),
            Command::CheckPluginUpdates { packages } => (packages.clone(), false),
            _ => {
                return Err(Fault::new(
                    ErrorCode::Internal,
                    "plugin update inspection expected",
                ));
            }
        };
        let mut names = std::collections::BTreeSet::new();
        if selections.len() > plugin::MAX_INSTALLED
            || selections
                .iter()
                .any(|package| !names.insert(package.name.clone()))
        {
            return Err(Fault::invalid("invalid plugin update selection"));
        }
        let mut batch = Batch::default();
        let mut current = Vec::new();
        for selection in selections {
            let info = match self
                .update_info(&selection.name, selection.expected_revision)
                .await
            {
                Ok(info) => info,
                Err(error) => {
                    batch.failures.push(Failure {
                        name: selection.name,
                        error,
                    });
                    continue;
                }
            };
            let root = if let Some(plugin::Origin::Worktree { worktree, .. }) = &info.origin {
                match self.worktree_root(*worktree).await {
                    Ok(root) => Some(root),
                    Err(error) => {
                        batch.failures.push(Failure {
                            name: selection.name,
                            error,
                        });
                        continue;
                    }
                }
            } else {
                None
            };
            current.push((selection, info, root));
        }
        let stop = self.closed.child_token();
        let _guard = stop.clone().drop_guard();
        let packages: Vec<_> = current.iter().map(|(_, info, _)| info.clone()).collect();
        let catalog = self.plugins.update_catalog(&packages, &stop).await;
        for (selection, info, root) in current {
            let result = match self
                .plugins
                .check_update(info, root, stop.clone(), catalog.as_ref())
                .await
            {
                Ok(report) => self
                    .update_info(&selection.name, selection.expected_revision)
                    .await
                    .map(|_| report),
                Err(error) => Err(error),
            };
            match result {
                Ok(report) => batch.reports.push(report),
                Err(error) => batch.failures.push(Failure {
                    name: selection.name,
                    error,
                }),
            }
        }
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let output = if single {
            if let Some(failure) = batch.failures.pop() {
                return Err(failure.error);
            }
            Output::PluginUpdate(batch.reports.pop().expect("single plugin update report"))
        } else {
            Output::PluginUpdates(batch)
        };
        let (send, completion) = oneshot::channel();
        let _ = send.send(Ok(output));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    async fn update_info(&self, name: &str, revision: u64) -> Result<Info, Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::PluginUpdate {
            name: name.into(),
            revision,
            reply,
        })
        .await?;
        tokio::select! {
            biased;
            _ = self.closed.cancelled() => Err(unavailable()),
            result = response => result.map_err(|_| unavailable())?,
        }
    }
}
