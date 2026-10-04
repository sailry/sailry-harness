use super::*;
use sailry_link::{Pending, Subscription};
use sailry_protocol::{NodeId, Update};

impl Commands {
    pub(crate) fn subscribe(
        self: &Arc<Self>,
        node: NodeId,
        session: SessionId,
        closed: CancellationToken,
    ) -> Box<dyn Subscription> {
        Box::new(Updates {
            owner: self.clone(),
            changes: self.changes.subscribe(),
            node,
            session,
            initial: true,
            closed,
        })
    }
}

struct Updates {
    owner: Arc<Commands>,
    changes: watch::Receiver<()>,
    node: NodeId,
    session: SessionId,
    initial: bool,
    closed: CancellationToken,
}
impl Subscription for Updates {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if !self.initial {
                tokio::select! {
                    biased;
                    _ = self.closed.cancelled() => return Err(Fault::new(ErrorCode::Unavailable, "command subscription closed")),
                    result = self.changes.changed() => result.map_err(|_| Fault::new(ErrorCode::Unavailable, "command subscription closed"))?,
                }
            }
            self.initial = false;
            self.changes.borrow_and_update();
            Ok(Update::Commands {
                node: self.node,
                session: self.session,
                items: self.owner.list(self.session),
            })
        })
    }
}
