use super::Pool;
use sailry_link::{CancellationToken, Pending, Subscription};
use sailry_protocol::{ErrorCode, Fault, NodeId, Update, WorktreeId};
use std::{sync::Arc, time::Duration};

impl Pool {
    pub(crate) fn subscribe(
        self: &Arc<Self>,
        node: NodeId,
        worktree: WorktreeId,
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
    worktree: WorktreeId,
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
                let update = Update::CodeServers {
                    node: self.node,
                    worktree: self.worktree,
                    servers: self.pool.list(self.worktree).await?,
                    installations: self.pool.install.list()?,
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
