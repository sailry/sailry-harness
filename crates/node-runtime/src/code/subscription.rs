use super::Pool;
use sailry_link::{CancellationToken, Pending, Subscription};
use sailry_protocol::{ErrorCode, Fault, NodeId, Update, WorktreeId};
use std::{sync::Arc, time::Duration};

impl Pool {
    pub(crate) fn subscribe(
        self: &Arc<Self>,
        node: NodeId,
        worktree: Option<WorktreeId>,
        closed: CancellationToken,
    ) -> Box<dyn Subscription> {
        Box::new(Events {
            pool: self.clone(),
            node,
            worktree,
            closed,
            previous: None,
        })
    }
}
struct Events {
    pool: Arc<Pool>,
    node: NodeId,
    worktree: Option<WorktreeId>,
    closed: CancellationToken,
    previous: Option<Update>,
}
impl Subscription for Events {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            loop {
                if self.closed.is_cancelled() {
                    return Err(Fault::new(
                        ErrorCode::Unavailable,
                        "code subscription has closed",
                    ));
                }
                let installations = self.pool.install.list()?;
                let update = match self.worktree {
                    Some(worktree) => Update::CodeServers {
                        node: self.node,
                        worktree,
                        servers: self.pool.list(worktree).await?,
                        installations,
                    },
                    None => Update::CodeInstallations {
                        node: self.node,
                        installations,
                    },
                };
                if self.previous.as_ref() != Some(&update) {
                    self.previous = Some(update.clone());
                    return Ok(update);
                }
                tokio::select! {biased;_ = self.closed.cancelled()=>return Err(Fault::new(ErrorCode::Unavailable,"code subscription has closed")),_ = tokio::time::sleep(Duration::from_millis(200))=>{}}
            }
        })
    }
}
