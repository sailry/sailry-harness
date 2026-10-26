//! Runtime facts outlive connections; each storage operation owns a short lease.
use super::{Database, Handle, Lease, removal};
use crate::{Error, store};
use sailry_protocol::{NodeId, RequestId, SessionId, WorktreeId};
use std::{
    collections::BTreeMap,
    ops::Deref,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};

pub(in crate::store) const WRITE_CONCURRENCY: usize = 4;

pub(in crate::store) struct Facts {
    pub node: NodeId,
    pub profile: Option<PathBuf>,
    pub feeds: store::agent::Feeds,
    pub publication: store::publication::Publication,
    pub writes: Arc<tokio::sync::Semaphore>,
    orders: Mutex<BTreeMap<SessionId, Weak<tokio::sync::Mutex<()>>>>,
    pub model_clients: Arc<crate::agent::model::registry::Registry>,
    pub environment: Arc<crate::process::Environment>,
    pub local_models: Option<Arc<crate::models::registry::Registry>>,
    pub approvals: Mutex<store::agent::approvals::Waiting>,
    pub questions: Mutex<store::agent::questions::Waiting>,
    pub network_link: Mutex<Option<sailry_link::LinkHandle>>,
    pub network_request: Mutex<Option<RequestId>>,
    pub removals: Mutex<BTreeMap<WorktreeId, removal::Pending>>,
    pub relocations: Mutex<BTreeMap<(NodeId, RequestId), Vec<PathBuf>>>,
    #[cfg(any(test, feature = "test-support"))]
    pub approval_commit: Mutex<Option<store::agent::approvals::fixture::Commit>>,
}

/// The existing Node owner retains handles and facts, never a live connection.
pub(in crate::store) struct Storage {
    pub handle: Arc<Handle>,
    facts: Arc<Facts>,
}

impl Facts {
    pub(super) fn new(
        node: NodeId,
        profile: Option<PathBuf>,
        writes: Arc<tokio::sync::Semaphore>,
    ) -> Self {
        Self {
            node,
            profile,
            feeds: Default::default(),
            publication: Default::default(),
            writes,
            orders: Default::default(),
            model_clients: Default::default(),
            environment: Arc::new(crate::process::Environment::ambient()),
            local_models: None,
            approvals: Default::default(),
            questions: Default::default(),
            network_link: Default::default(),
            network_request: Default::default(),
            removals: Default::default(),
            relocations: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            approval_commit: Default::default(),
        }
    }

    pub(in crate::store) async fn write(
        &self,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, sailry_protocol::Fault> {
        self.writes.clone().acquire_owned().await.map_err(|_| {
            sailry_protocol::Fault::new(
                sailry_protocol::ErrorCode::Unavailable,
                "Node storage is closing",
            )
        })
    }

    pub(in crate::store) async fn order(
        &self,
        session: SessionId,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        let order = {
            let mut orders = self.orders.lock().unwrap();
            orders.retain(|_, order| order.strong_count() > 0);
            if let Some(order) = orders.get(&session).and_then(Weak::upgrade) {
                order
            } else {
                let order = Arc::new(tokio::sync::Mutex::new(()));
                orders.insert(session, Arc::downgrade(&order));
                order
            }
        };
        order.lock_owned().await
    }
}

impl Storage {
    pub(super) fn new(handle: Arc<Handle>, facts: Arc<Facts>) -> Self {
        Self { handle, facts }
    }

    pub(in crate::store) async fn context(&self, readonly: bool) -> Result<Database, Error> {
        let connection = self.handle.lease(readonly).await?;
        Ok(Database::context(
            self.handle.clone(),
            self.facts.clone(),
            connection,
        ))
    }

    /// Related metadata and data reads share one native snapshot without a slot quota.
    pub(in crate::store) async fn read<T: Send>(
        &self,
        read: impl for<'a> FnOnce(
            &'a turso::Connection,
        )
            -> sailry_link::Pending<'a, Result<T, sailry_protocol::Fault>>
        + Send,
    ) -> Result<T, sailry_protocol::Fault> {
        let mut connection = self
            .handle
            .lease(true)
            .await
            .map_err(super::storage_error)?;
        let transaction = connection
            .transaction_with_behavior(turso::transaction::TransactionBehavior::Deferred)
            .await
            .map_err(super::storage_error)?;
        match read(&transaction).await {
            Ok(value) => {
                crate::store::database::conflict::commit(transaction).await?;
                Ok(value)
            }
            Err(error) => {
                transaction.rollback().await.map_err(super::storage_error)?;
                Err(error)
            }
        }
    }

    pub(in crate::store) async fn close(&self) -> Result<(), Error> {
        drop(self.handle.pause().await?);
        Ok(())
    }
}

impl Deref for Storage {
    type Target = Facts;
    fn deref(&self) -> &Facts {
        &self.facts
    }
}

impl Database {
    pub(super) fn context(handle: Arc<Handle>, facts: Arc<Facts>, connection: Lease) -> Self {
        Self {
            path: handle.path().to_owned(),
            handle,
            facts,
            connection,
        }
    }

    pub(in crate::store) fn configure(
        &mut self,
        feeds: store::agent::Feeds,
        model_clients: Arc<crate::agent::model::registry::Registry>,
        environment: Arc<crate::process::Environment>,
    ) -> Result<(), sailry_protocol::Fault> {
        let facts = Arc::get_mut(&mut self.facts)
            .ok_or_else(|| super::storage_error("storage facts are already shared"))?;
        facts.feeds = feeds;
        facts.model_clients = model_clients;
        facts.environment = environment;
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::store) fn set_profile(
        &mut self,
        profile: Option<PathBuf>,
    ) -> Result<(), sailry_protocol::Fault> {
        Arc::get_mut(&mut self.facts)
            .ok_or_else(|| super::storage_error("storage facts are already shared"))?
            .profile = profile;
        Ok(())
    }

    pub(in crate::store) fn into_storage(self) -> Storage {
        drop(self.connection);
        Storage::new(self.handle, self.facts)
    }
}

impl Deref for Database {
    type Target = Facts;
    fn deref(&self) -> &Facts {
        &self.facts
    }
}

impl Storage {
    /// Rehearse current plugin connection changes on a closed private snapshot.
    pub(in crate::store) async fn prepare_plugin(
        &self,
        command: &sailry_protocol::Command,
        output: &sailry_protocol::Output,
    ) -> Result<(), Error> {
        let sailry_protocol::Output::Plugin(info) = output else {
            return Ok(());
        };
        let needs_update = {
            let connection = self.handle.lease(true).await?;
            store::plugins::connectors::needs_migration(connection.connection(), info).await?
        };
        if !needs_update {
            return Ok(());
        }
        let exclusive = self.handle.exclusive().await?;
        if !store::plugins::connectors::needs_migration(exclusive.connection(), info).await? {
            return Ok(());
        }
        let closed = exclusive.close().await?;
        let prepared = async {
            let preview = super::schema::backup::preview(closed.path())?;
            {
                let database = super::engine(preview.path()).await?;
                let mut connection = super::connect(&database).await?;
                let transaction = connection.transaction().await?;
                let mut result = Ok(output.clone());
                store::plugins::finish(&transaction, self.profile.as_deref(), command, &mut result)
                    .await?;
                result?;
                super::validation::stored(&transaction).await?;
                super::schema::references(&transaction).await?;
                transaction.commit().await?;
                super::driver::checkpoint(&connection).await?;
            }
            super::schema::backup::save(closed.path())?;
            Ok::<_, Error>(())
        }
        .await;
        let reopened = closed.reopen().await;
        reopened?;
        prepared
    }
}
