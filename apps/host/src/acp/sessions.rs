use super::*;
use sailry_protocol::{Worktree, conversation::Status};
use std::{num::NonZeroU64, path::Path};

impl Adapter {
    pub(super) async fn new_session(
        self: &Arc<Self>,
        request: acp::NewSessionRequest,
        connection: &ConnectionTo<sdk::Client>,
    ) -> acp::Result<acp::NewSessionResponse> {
        validate_roots(
            &request.cwd,
            &request.additional_directories,
            &request.mcp_servers,
        )?;
        let worktree = self.worktree(&request.cwd).await?;
        let (output, id) = self
            .execute(
                Command::CreateSession {
                    project: worktree.project,
                    worktree: Some(worktree.id),
                    config: None,
                },
                request.meta.as_ref(),
            )
            .await?;
        let Output::Session(session) = output else {
            return Err(invalid("session creation response expected"));
        };
        let context = json!({"request_id":id,"node":self.client.target(),"session":session.id,"revision":session.revision,"state":"created"});
        self.bind(session.clone(), false, false, connection)
            .await
            .map_err(|error| captured(error, context.clone()))?;
        let (modes, options) = self
            .emit_controls(&session, connection)
            .await
            .map_err(|error| captured(error, context.clone()))?;
        Ok(
            acp::NewSessionResponse::new(acp::SessionId::new(session.id.to_string()))
                .modes(modes)
                .config_options(options)
                .meta(metadata(context)),
        )
    }

    pub(super) async fn load_session(
        self: &Arc<Self>,
        request: acp::LoadSessionRequest,
        connection: &ConnectionTo<sdk::Client>,
    ) -> acp::Result<acp::LoadSessionResponse> {
        validate_roots(
            &request.cwd,
            &request.additional_directories,
            &request.mcp_servers,
        )?;
        let session = self.load(&request.session_id, &request.cwd).await?;
        self.bind(session.clone(), true, false, connection).await?;
        let (modes, options) = self.emit_controls(&session, connection).await?;
        Ok(acp::LoadSessionResponse::new()
            .modes(modes)
            .config_options(options)
            .meta(metadata(self.context(&session))))
    }

    pub(super) async fn resume_session(
        self: &Arc<Self>,
        request: acp::ResumeSessionRequest,
        connection: &ConnectionTo<sdk::Client>,
    ) -> acp::Result<acp::ResumeSessionResponse> {
        validate_roots(
            &request.cwd,
            &request.additional_directories,
            &request.mcp_servers,
        )?;
        let session = self.load(&request.session_id, &request.cwd).await?;
        self.bind(session.clone(), false, true, connection).await?;
        let (modes, options) = self.emit_controls(&session, connection).await?;
        Ok(acp::ResumeSessionResponse::new()
            .modes(modes)
            .config_options(options)
            .meta(metadata(self.context(&session))))
    }

    pub(super) async fn fork_session(
        self: &Arc<Self>,
        request: acp::ForkSessionRequest,
        connection: &ConnectionTo<sdk::Client>,
    ) -> acp::Result<acp::ForkSessionResponse> {
        validate_roots(
            &request.cwd,
            &request.additional_directories,
            &request.mcp_servers,
        )?;
        let session = self.load(&request.session_id, &request.cwd).await?;
        let history = self
            .client
            .read_conversation(session.id, None, 20)
            .await
            .map_err(|error| fault(error, None))?;
        let latest = history
            .page
            .runs
            .iter()
            .max_by_key(|run| run.sequence)
            .ok_or_else(|| invalid("a completed turn is required to fork this session"))?;
        if matches!(
            latest.status,
            Status::Queued | Status::Running | Status::Stopping
        ) {
            return Err(fault(
                Fault::new(
                    ErrorCode::Busy,
                    "finish or stop the current turn before forking",
                ),
                None,
            ));
        }
        let (output, id) = self
            .execute(
                Command::ForkConversation {
                    session: session.id,
                    through: latest.turn,
                    expected_revision: history.page.revision,
                },
                request.meta.as_ref(),
            )
            .await?;
        let Output::Session(fork) = output else {
            return Err(invalid("session fork response expected"));
        };
        let context = json!({"request_id":id,"revision":fork.revision,"node":self.client.target(),"session":fork.id,"state":"forked"});
        self.bind(fork.clone(), true, false, connection)
            .await
            .map_err(|error| captured(error, context.clone()))?;
        let (modes, options) = self
            .emit_controls(&fork, connection)
            .await
            .map_err(|error| captured(error, context.clone()))?;
        Ok(
            acp::ForkSessionResponse::new(acp::SessionId::new(fork.id.to_string()))
                .modes(modes)
                .config_options(options)
                .meta(metadata(context)),
        )
    }

    pub(super) async fn list_sessions(
        &self,
        request: acp::ListSessionsRequest,
        _connection: &ConnectionTo<sdk::Client>,
    ) -> acp::Result<acp::ListSessionsResponse> {
        if request.cursor.is_some() {
            return Err(invalid("this session list is not paginated"));
        }
        if request.cwd.as_ref().is_some_and(|path| !path.is_absolute()) {
            return Err(invalid("cwd must be absolute"));
        }
        let (output, _) = self.execute(Command::Snapshot, None).await?;
        let Output::Snapshot(snapshot) = output else {
            return Err(invalid("Node snapshot expected"));
        };
        let sessions = snapshot
            .sessions
            .iter()
            .filter(|session| {
                !session.archived
                    && session.delegation.is_none()
                    && session.config.resource.is_none()
            })
            .filter_map(|session| {
                let worktree = snapshot.worktrees.iter().find(|worktree| {
                    worktree.id == session.worktree && worktree.project.is_some()
                })?;
                let cwd = PathBuf::from(&worktree.path);
                if request.cwd.as_ref().is_some_and(|filter| filter != &cwd) {
                    return None;
                }
                Some(
                    acp::SessionInfo::new(acp::SessionId::new(session.id.to_string()), cwd)
                        .title(session.activity.title.clone())
                        .meta(metadata(self.context(session))),
                )
            })
            .collect();
        Ok(acp::ListSessionsResponse::new(sessions))
    }

    pub(super) async fn close_session(
        &self,
        request: acp::CloseSessionRequest,
        _connection: &ConnectionTo<sdk::Client>,
    ) -> acp::Result<acp::CloseSessionResponse> {
        let session = parse_session(&request.session_id)?;
        self.cancel(acp::CancelNotification::new(request.session_id).meta(request.meta))
            .await?;
        if let Some(opened) = self.sessions.lock().await.remove(&session) {
            opened.stop.cancel();
        }
        Ok(acp::CloseSessionResponse::new())
    }

    async fn worktree(&self, path: &Path) -> acp::Result<Worktree> {
        let (output, _) = self.execute(Command::ReadProjectCatalog, None).await?;
        let Output::ProjectCatalog(catalog) = output else {
            return Err(invalid("worktree catalog expected"));
        };
        catalog
            .worktrees
            .into_iter()
            .find(|worktree| worktree.project.is_some() && Path::new(&worktree.path) == path)
            .ok_or_else(|| {
                invalid("register this project worktree in Sailry before opening it through ACP")
            })
    }

    async fn load(&self, id: &acp::SessionId, path: &Path) -> acp::Result<Session> {
        let worktree = self.worktree(path).await?;
        let (output, _) = self
            .execute(
                Command::ReadSession {
                    session: parse_session(id)?,
                },
                None,
            )
            .await?;
        let Output::Session(session) = output else {
            return Err(invalid("session response expected"));
        };
        if session.worktree != worktree.id || session.config.resource.is_some() {
            return Err(invalid("session does not belong to this project worktree"));
        }
        Ok(session)
    }

    async fn bind(
        self: &Arc<Self>,
        session: Session,
        all: bool,
        resumed: bool,
        connection: &ConnectionTo<sdk::Client>,
    ) -> acp::Result<()> {
        let mut oldest = None;
        if all {
            let mut before = None;
            loop {
                let history = self
                    .client
                    .read_conversation(session.id, before, 100)
                    .await
                    .map_err(|error| fault(error, None))?;
                oldest = history
                    .page
                    .runs
                    .iter()
                    .map(|run| run.sequence)
                    .min()
                    .or(oldest);
                match history.page.next_before {
                    Some(cursor) if Some(cursor) != before => before = Some(cursor),
                    _ => break,
                }
            }
        }
        let stop = self.stop.child_token();
        let (updates, mut view) = watch::channel(View::default());
        let (requests, requested) = mpsc::channel(2);
        if let Some(sequence) = oldest.and_then(NonZeroU64::new) {
            requests
                .try_send(HistoryRequest::Through(sequence))
                .map_err(|_| invalid("history request channel is unavailable"))?;
        }
        let watching = self.client.clone();
        let watching_stop = stop.clone();
        let id = session.id;
        let observer = tokio::spawn(async move {
            watching
                .watch_conversation(id, updates, watching_stop, requested)
                .await
        });
        loop {
            {
                let current = view.borrow_and_update();
                if let Some(error) = &current.older_error {
                    stop.cancel();
                    observer.abort();
                    return Err(fault(error.clone(), None));
                }
                if let Some(error) = &current.error
                    && !matches!(error.code, ErrorCode::Unavailable | ErrorCode::Busy)
                {
                    stop.cancel();
                    observer.abort();
                    return Err(fault(error.clone(), None));
                }
                if current.connected
                    && current
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| !all || snapshot.page.next_before.is_none())
                {
                    break;
                }
            }
            tokio::select! {
                _ = stop.cancelled() => { observer.abort(); return Err(fault(Fault::new(ErrorCode::Cancelled, "ACP session observation closed"), None)); },
                changed = view.changed() => if changed.is_err() { observer.abort(); return Err(fault(Fault::new(ErrorCode::Unavailable, "conversation observation closed"), None)); },
            }
        }
        let (problem, _) = watch::channel(None);
        let opened = Arc::new(Opened {
            session: Mutex::new(session),
            view,
            requests,
            stop,
            observer,
            presentation: Mutex::new(observe::Presentation::default()),
            history: AtomicU64::new(oldest.unwrap_or(u64::MAX)),
            problem,
        });
        if let Some(previous) = self.sessions.lock().await.insert(id, opened.clone()) {
            previous.stop.cancel();
        }
        observe::start(self.clone(), opened, resumed, connection).await
    }
}

fn validate_roots(
    path: &Path,
    directories: &[PathBuf],
    servers: &[acp::McpServer],
) -> acp::Result<()> {
    if !path.is_absolute() {
        return Err(invalid("cwd must be absolute"));
    }
    if !directories.is_empty() {
        return Err(invalid(
            "additional workspaces require configuration in Sailry",
        ));
    }
    if !servers.is_empty() {
        return Err(invalid(
            "configure MCP servers in the execution Node before using ACP",
        ));
    }
    Ok(())
}
