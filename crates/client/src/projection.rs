use sailry_protocol::{ErrorCode, Event, Fault, NodeId, Session, Snapshot, Update};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Apply {
    Applied,
    Ignored,
    Recover,
}

mod quotas;
mod reads;
mod settled;
pub use reads::ReadCursors;
#[cfg(test)]
mod tests;

pub struct Projection {
    node: NodeId,
    generation: u64,
    snapshot: Option<Snapshot>,
    recovery: bool,
    package_cursors: std::collections::BTreeMap<String, u64>,
    package_revision: u64,
    credential_cursor: u64,
    settled: settled::Changes,
    read_cursors: ReadCursors,
}

impl Projection {
    pub fn new(node: NodeId, generation: u64) -> Self {
        Self {
            node,
            generation,
            snapshot: None,
            recovery: true,
            package_cursors: Default::default(),
            package_revision: 0,
            credential_cursor: 0,
            settled: Default::default(),
            read_cursors: Default::default(),
        }
    }

    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    /// Local invalidation revisions for each currently installed package.
    pub fn package_cursors(&self) -> &std::collections::BTreeMap<String, u64> {
        &self.package_cursors
    }

    /// Completion and worktree invalidation, independent of streaming history cursors.
    pub fn settled_cursors(&self) -> &std::collections::BTreeMap<sailry_protocol::WorktreeId, u64> {
        &self.settled.cursors
    }

    pub fn read_cursors(&self) -> ReadCursors {
        self.read_cursors
    }

    /// Invalidates public credential reads after mutation, recovery or reconnection.
    pub fn credential_cursor(&self) -> u64 {
        self.credential_cursor
    }

    pub fn reconnect(&mut self, generation: u64) -> Result<(), Fault> {
        if generation <= self.generation {
            return Err(Fault::invalid("connection generation must increase"));
        }
        advance_credentials(&mut self.credential_cursor)?;
        self.generation = generation;
        self.recovery = true;
        Ok(())
    }

    pub fn apply(&mut self, generation: u64, update: Update) -> Result<Apply, Fault> {
        if generation != self.generation {
            return Ok(Apply::Ignored);
        }
        match update {
            Update::Python { .. }
            | Update::Installations { .. }
            | Update::Commands { .. }
            | Update::BrowserCall(_)
            | Update::ProviderLogin(_)
            | Update::ConnectorLogin(_)
            | Update::ConnectionsChanged(_)
            | Update::FilesChanged { .. }
            | Update::ConversationSnapshot(_)
            | Update::ConversationFrame(_)
            | Update::TerminalSnapshot(_)
            | Update::TerminalFrame(_) => {
                Err(Fault::invalid("resource update on Node subscription"))
            }
            Update::ResetRequired => {
                advance_credentials(&mut self.credential_cursor)?;
                self.recovery = true;
                Ok(Apply::Recover)
            }
            Update::Snapshot(mut snapshot) => {
                self.check_target(snapshot.node)?;
                if !self.recovery
                    && self
                        .snapshot
                        .as_ref()
                        .is_some_and(|current| snapshot.cursor < current.cursor)
                {
                    return Ok(Apply::Ignored);
                }
                advance_credentials(&mut self.credential_cursor)?;
                quotas::retain(&mut snapshot);
                self.package_revision = self.package_revision.checked_add(1).ok_or_else(|| {
                    Fault::new(ErrorCode::Internal, "plugin change revision exhausted")
                })?;
                self.package_cursors = snapshot
                    .plugins
                    .iter()
                    .map(|plugin| (plugin.name.clone(), self.package_revision))
                    .collect();
                self.settled.reset(&snapshot);
                self.read_cursors.reset(self.package_revision);
                self.snapshot = Some(snapshot);
                self.recovery = false;
                Ok(Apply::Applied)
            }
            Update::Event(envelope) => {
                self.check_target(envelope.node)?;
                let Some(snapshot) = self.snapshot.as_mut() else {
                    advance_credentials(&mut self.credential_cursor)?;
                    return Ok(Apply::Recover);
                };
                if envelope.cursor <= snapshot.cursor {
                    return Ok(Apply::Ignored);
                }
                if self.recovery || snapshot.cursor.checked_add(1) != Some(envelope.cursor) {
                    self.recovery = true;
                    advance_credentials(&mut self.credential_cursor)?;
                    return Ok(Apply::Recover);
                }
                if ReadCursors::observes(&envelope.event)
                    || matches!(
                        &envelope.event,
                        Event::PluginChanged(_)
                            | Event::PluginValuesChanged { .. }
                            | Event::PluginRemoved { .. }
                    )
                {
                    self.package_revision =
                        self.package_revision.checked_add(1).ok_or_else(|| {
                            Fault::new(ErrorCode::Internal, "plugin change revision exhausted")
                        })?;
                }
                self.settled
                    .observe(snapshot, &envelope.event, envelope.cursor);
                self.read_cursors
                    .observe(&envelope.event, self.package_revision);
                match envelope.event {
                    Event::CredentialChanged { credential, .. } => {
                        if credential.node != self.node {
                            return Err(Fault::new(
                                ErrorCode::WrongTarget,
                                "credential belongs to another Node",
                            ));
                        }
                        advance_credentials(&mut self.credential_cursor)?;
                    }
                    Event::PluginChanged(plugin) => {
                        self.package_cursors
                            .insert(plugin.name.clone(), self.package_revision);
                        snapshot.plugins.retain(|item| item.name != plugin.name);
                        snapshot.plugins.push(plugin);
                        snapshot
                            .plugins
                            .sort_by(|left, right| left.name.cmp(&right.name));
                    }
                    Event::PluginValuesChanged { name } => {
                        if let Some(cursor) = self.package_cursors.get_mut(&name) {
                            *cursor = self.package_revision;
                        }
                    }
                    Event::LocalModelsChanged { .. }
                    | Event::LocalModelSettingsChanged { .. }
                    | Event::BackupSettingsChanged { .. }
                    | Event::TelemetrySettingsChanged { .. }
                    | Event::PythonSettingsChanged { .. }
                    | Event::KnowledgeCollectionsChanged => {}
                    Event::PluginRemoved { name } => {
                        self.package_cursors.remove(&name);
                        snapshot.plugins.retain(|item| item.name != name)
                    }
                    Event::DispatchChanged { .. }
                    | Event::RetainedRecord(_)
                    | Event::ToolsCompleted { .. } => {}
                    Event::NotificationChanged(notice) => {
                        snapshot.notifications.retain(|entry| entry.id != notice.id);
                        snapshot.notifications.push(notice);
                        snapshot
                            .notifications
                            .sort_by_key(|notice| std::cmp::Reverse(notice.sequence));
                        snapshot
                            .notifications
                            .truncate(sailry_protocol::notification::LIMIT);
                    }
                    Event::NotificationsRead { ids } => {
                        for notice in &mut snapshot.notifications {
                            if ids.contains(&notice.id) {
                                notice.read = true;
                            }
                        }
                    }
                    Event::NotificationsDismissed { ids } => {
                        snapshot
                            .notifications
                            .retain(|notice| !ids.contains(&notice.id));
                    }
                    Event::RoleChanged(role) => {
                        snapshot.roles.retain(|item| item.id != role.id);
                        snapshot.roles.push(role);
                        snapshot
                            .roles
                            .sort_by(|left, right| left.key.cmp(&right.key));
                    }
                    Event::RoleRemoved { id } => snapshot.roles.retain(|item| item.id != id),
                    Event::SshChanged(profile) => {
                        snapshot.ssh.retain(|item| item.id != profile.id);
                        snapshot.ssh.push(profile);
                        snapshot.ssh.sort_by_key(|item| item.id);
                    }
                    Event::SshRemoved { id } => snapshot.ssh.retain(|item| item.id != id),
                    Event::DatabaseChanged(profile) => {
                        snapshot.databases.retain(|item| item.id != profile.id);
                        snapshot.databases.push(profile);
                        snapshot.databases.sort_by_key(|item| item.id);
                    }
                    Event::DatabaseRemoved { id } => {
                        snapshot.databases.retain(|item| item.id != id)
                    }
                    Event::ProviderChanged(provider) => {
                        snapshot.providers.retain(|item| item.id != provider.id);
                        snapshot.providers.push(provider);
                        quotas::retain(snapshot);
                    }
                    Event::ProviderQuotaChanged { provider, quota } => {
                        quotas::apply(snapshot, provider, quota);
                    }
                    Event::ModelCatalogChanged(status) => snapshot.model_catalog = status,
                    Event::ProviderRemoved { id } => {
                        snapshot.providers.retain(|item| item.id != id);
                        snapshot
                            .provider_quotas
                            .retain(|quota| quota.provider != id);
                    }
                    Event::ConversationChanged { session, activity } => {
                        if let Some(session) =
                            snapshot.sessions.iter_mut().find(|item| item.id == session)
                        {
                            session.activity = *activity;
                        }
                    }
                    Event::TerminalChanged(info) => {
                        if let Some(terminal) = snapshot
                            .terminals
                            .iter_mut()
                            .find(|terminal| terminal.id == info.id)
                        {
                            *terminal = info;
                        } else {
                            snapshot.terminals.push(info);
                            snapshot.terminals.sort_by_key(|terminal| terminal.id);
                        }
                    }
                    Event::PowerSettingsChanged { revision } => {
                        snapshot.power_settings_revision = revision;
                    }
                    Event::TerminalSettingsChanged { revision } => {
                        snapshot.terminal_settings_revision = revision;
                    }
                    Event::SandboxSettingsChanged { revision } => {
                        snapshot.sandbox_settings_revision = revision;
                    }
                    Event::SearchSettingsChanged { revision } => {
                        snapshot.search_settings_revision = revision;
                    }
                    Event::MediaSettingsChanged(settings) => {
                        snapshot.media_settings = settings;
                    }
                    Event::ProjectRegistered(_)
                    | Event::ProjectRemoved { .. }
                    | Event::ProjectChanged(_) => {
                        // Project registration also creates its main worktree atomically.
                        // Recover both from the authority rather than inventing a local ID.
                        self.recovery = true;
                        advance_credentials(&mut self.credential_cursor)?;
                        return Ok(Apply::Recover);
                    }
                    Event::DefaultsChanged(defaults) => snapshot.defaults = defaults,
                    Event::WorktreeRegistered(worktree) => {
                        snapshot.worktrees.retain(|item| item.id != worktree.id);
                        snapshot.worktrees.push(worktree);
                    }
                    Event::WorktreeRemoved { id } => {
                        snapshot.worktrees.retain(|item| item.id != id);
                        snapshot.terminals.retain(|item| item.worktree != Some(id));
                    }
                    Event::TaskStarted { session, turn } => {
                        if !snapshot
                            .worktrees
                            .iter()
                            .any(|tree| tree.id == session.worktree)
                        {
                            self.recovery = true;
                            advance_credentials(&mut self.credential_cursor)?;
                            return Ok(Apply::Recover);
                        }
                        update_session(&mut snapshot.sessions, *session);
                        snapshot.turns.retain(|item| item.id != turn.id);
                        snapshot.turns.push((&turn).into());
                    }
                    Event::SessionChanged(session) => {
                        // Creating a projectless session also reserves its workspace.
                        // Recover that Node-owned identity, including on remote clients.
                        if session.project.is_none()
                            && !snapshot
                                .worktrees
                                .iter()
                                .any(|tree| tree.id == session.worktree)
                        {
                            self.recovery = true;
                            advance_credentials(&mut self.credential_cursor)?;
                            return Ok(Apply::Recover);
                        }
                        update_session(&mut snapshot.sessions, *session);
                    }
                    Event::SessionsReordered(order) => {
                        let positions: std::collections::BTreeMap<_, _> = order
                            .iter()
                            .enumerate()
                            .map(|(index, id)| (*id, index))
                            .collect();
                        if positions.len() != order.len()
                            || positions.len() != snapshot.sessions.len()
                            || snapshot
                                .sessions
                                .iter()
                                .any(|session| !positions.contains_key(&session.id))
                        {
                            self.recovery = true;
                            advance_credentials(&mut self.credential_cursor)?;
                            return Ok(Apply::Recover);
                        }
                        snapshot
                            .sessions
                            .sort_by_key(|session| positions[&session.id]);
                    }
                    Event::SessionsRemoved(ids) => {
                        snapshot
                            .sessions
                            .retain(|session| !ids.contains(&session.id));
                        snapshot.turns.retain(|turn| !ids.contains(&turn.session));
                    }
                    Event::SessionRewound { session, backup } => {
                        update_session(&mut snapshot.sessions, *session);
                        if let Some(backup) = backup {
                            update_session(&mut snapshot.sessions, *backup);
                        }
                    }
                    Event::TurnQueued { turn, activity } => {
                        if let Some(session) = snapshot
                            .sessions
                            .iter_mut()
                            .find(|item| item.id == turn.session)
                        {
                            session.activity = *activity;
                        }
                        snapshot.turns.retain(|item| item.id != turn.id);
                        snapshot.turns.push((&turn).into());
                    }
                    Event::PlanAccepted(accepted) => {
                        update_session(&mut snapshot.sessions, accepted.session);
                        snapshot.turns.retain(|item| item.id != accepted.turn.id);
                        snapshot.turns.push((&accepted.turn).into());
                    }
                }
                snapshot.cursor = envelope.cursor;
                Ok(Apply::Applied)
            }
        }
    }

    fn check_target(&self, node: NodeId) -> Result<(), Fault> {
        if node == self.node {
            Ok(())
        } else {
            Err(Fault::new(
                ErrorCode::WrongTarget,
                "update belongs to another Node",
            ))
        }
    }
}

/// Existing sessions retain their position; only creation prepends a row.
fn update_session(sessions: &mut Vec<Session>, session: Session) {
    if let Some(current) = sessions.iter_mut().find(|item| item.id == session.id) {
        *current = session;
    } else {
        sessions.insert(0, session);
    }
}

fn advance_credentials(cursor: &mut u64) -> Result<(), Fault> {
    *cursor = cursor
        .checked_add(1)
        .ok_or_else(|| Fault::new(ErrorCode::Internal, "credential change revision exhausted"))?;
    Ok(())
}
