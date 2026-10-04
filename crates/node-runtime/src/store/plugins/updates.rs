use super::*;
use crate::store::{Ingress, Job, unavailable};
use sailry_link::Admission;
use sailry_protocol::{Receipt, Request, VERSION};
use tokio::sync::oneshot;

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
        let Command::CheckPluginUpdate {
            name,
            expected_revision,
        } = &request.command
        else {
            return Err(Fault::new(
                ErrorCode::Internal,
                "plugin update inspection expected",
            ));
        };
        let info = self.update_info(name, *expected_revision).await?;
        let root = if let Some(plugin::Origin::Worktree { worktree, .. }) = &info.origin {
            Some(self.worktree_root(*worktree).await?)
        } else {
            None
        };
        let stop = self.closed.child_token();
        let _guard = stop.clone().drop_guard();
        let report = self.plugins.check_update(info, root, stop).await?;
        self.update_info(name, *expected_revision).await?;
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let (send, completion) = oneshot::channel();
        let _ = send.send(Ok(Output::PluginUpdate(report)));
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
        self.sender
            .try_send(Job::PluginUpdate {
                name: name.into(),
                revision,
                reply,
            })
            .map_err(|_| Fault::new(ErrorCode::Busy, "Node request queue is unavailable"))?;
        response.await.map_err(|_| unavailable())?
    }
}
