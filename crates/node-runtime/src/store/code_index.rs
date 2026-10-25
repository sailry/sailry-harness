//! Current enablement and command routing; index bodies live in derived databases.
use super::{
    Ingress,
    database::{encode, storage_error},
};
use sailry_link::Admission;
use sailry_protocol::{
    Command, ErrorCode, Fault, NodeId, Output, Receipt, Request, code_index::Settings,
};
use turso::Connection;

impl Ingress {
    pub(super) async fn read_code_index(
        &self,
        caller: NodeId,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let output = match &request.command {
            Command::SearchCode { worktree, query } => {
                let active = self.code_queries.begin(caller, request.id, &self.closed)?;
                Output::CodeSearch(
                    active
                        .run(async {
                            let root = self.worktree_root(*worktree).await?;
                            self.code_index
                                .search(*worktree, root, query, &active.stop)
                                .await
                        })
                        .await?,
                )
            }
            Command::OutlineCode { worktree, path } => {
                let active = self.code_queries.begin(caller, request.id, &self.closed)?;
                Output::CodeOutline(
                    active
                        .run(async {
                            let root = self.worktree_root(*worktree).await?;
                            self.code_index
                                .outline(*worktree, root, path, &active.stop)
                                .await
                        })
                        .await?,
                )
            }
            Command::CancelCodeIndexQuery { request } => {
                self.code_queries.cancel(caller, *request);
                Output::CodeIndexQueryCancelled { request: *request }
            }
            _ => return Err(Fault::invalid("code query expected")),
        };
        let (reply, completion) = tokio::sync::oneshot::channel();
        let _ = reply.send(Ok(output));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}

pub(super) async fn settings(db: &Connection) -> Result<Settings, Fault> {
    match db
        .prepare("SELECT body FROM code_index_settings WHERE singleton=1")
        .await
        .map_err(storage_error)?
        .query_row(())
        .await
    {
        Ok(row) => serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
            .map_err(storage_error),
        Err(turso::Error::QueryReturnedNoRows) => Ok(Settings::default()),
        Err(error) => Err(storage_error(error)),
    }
}

pub(super) async fn initialize(db: &mut Connection) -> Result<Settings, Fault> {
    let tx = db.transaction().await.map_err(storage_error)?;
    if let Some(enabled) =
        super::plugins::services::enabled(&tx, sailry_protocol::plugin::Service::CodeIndex).await?
    {
        enable(&tx, enabled).await?;
    }
    let value = settings(&tx).await?;
    crate::store::database::conflict::commit(tx).await?;
    Ok(value)
}

pub(super) async fn save(
    db: &Connection,
    value: &Settings,
    expected: u64,
) -> Result<Settings, Fault> {
    let current = settings(db).await?;
    if value.revision != expected || current.revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "code index settings changed before saving",
        ));
    }
    let mut saved = value.clone();
    saved.revision = expected
        .checked_add(1)
        .ok_or_else(|| Fault::invalid("code index revision is exhausted"))?;
    db.execute("INSERT INTO code_index_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body",[encode(&saved)?]).await.map_err(storage_error)?;
    Ok(saved)
}

pub(super) async fn enable(db: &Connection, enabled: bool) -> Result<(), Fault> {
    let mut value = settings(db).await?;
    if value.enabled != enabled {
        let expected = value.revision;
        value.enabled = enabled;
        save(db, &value, expected).await?;
    }
    Ok(())
}

pub(super) fn configures(command: &Command) -> bool {
    matches!(
        command,
        Command::SaveCodeIndexSettings { .. }
            | Command::SetPluginEnabled { .. }
            | Command::RemovePlugin { .. }
            | Command::InstallPlugin { .. }
            | Command::InstallPluginUpload { .. }
            | Command::InstallPluginSource { .. }
            | Command::InstallBundledPlugin { .. }
            | Command::InstallSkill { .. }
            | Command::InstallRegistrySkill { .. }
            | Command::InstallSkillUpload { .. }
            | Command::InstallMcp { .. }
    )
}

pub(super) async fn refresh(db: &Connection, service: &crate::code_index::Service) {
    match settings(db).await {
        Ok(value) => service.configure(value.enabled),
        Err(_) => service.configure(false),
    }
}
