//! Refresh transactions retain the admitted grant identity across settings captures.
use super::*;
use crate::plugins::oauth::{Grant, Target};
use database::storage_error;

pub(super) enum Operation {
    Load {
        target: Target,
        reply: oneshot::Sender<Result<(u64, Grant), Fault>>,
    },
    Renew {
        target: Target,
        revision: u64,
        grant: Grant,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
}

pub(super) fn execute(database: &mut database::Database, operation: Operation) {
    match operation {
        Operation::Load { target, reply } => {
            let _ = reply.send(plugins::oauth::load(
                &database.connection,
                &target.plugin,
                target.id,
            ));
        }
        Operation::Renew {
            target,
            revision,
            grant,
            reply,
        } => {
            let result = (|| {
                let transaction = database.connection.transaction().map_err(storage_error)?;
                plugins::oauth::renew(&transaction, &target.plugin, target.id, revision, grant)?;
                transaction.commit().map_err(storage_error)
            })();
            let _ = reply.send(result);
        }
    }
}

impl Ingress {
    pub(crate) async fn plugin_oauth_credentials(
        &self,
        target: Target,
    ) -> Result<(u64, Grant), Fault> {
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::PluginOAuth(Operation::Load { target, reply }))
            .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }
    pub(crate) async fn save_plugin_oauth_credentials(
        &self,
        target: Target,
        revision: u64,
        grant: Grant,
    ) -> Result<(), Fault> {
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::PluginOAuth(Operation::Renew {
                target,
                revision,
                grant,
                reply,
            }))
            .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }
}
