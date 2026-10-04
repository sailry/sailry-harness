use sailry_protocol::{ErrorCode, Event, Fault, NodeId, Session, Snapshot, Update};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Apply {
    Applied,
    Ignored,
    Recover,
}

#[cfg(test)]
mod tests;

pub struct Projection {
    node: NodeId,
    generation: u64,
    snapshot: Option<Snapshot>,
    recovery: bool,
}

impl Projection {
    pub fn new(node: NodeId, generation: u64) -> Self {
        Self {
            node,
            generation,
            snapshot: None,
            recovery: true,
        }
    }

    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    pub fn reconnect(&mut self, generation: u64) -> Result<(), Fault> {
        if generation <= self.generation {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "connection generation must increase",
            ));
        }
        self.generation = generation;
        self.recovery = true;
        Ok(())
    }

    pub fn apply(&mut self, generation: u64, update: Update) -> Result<Apply, Fault> {
        if generation != self.generation {
            return Ok(Apply::Ignored);
        }
        match update {
            Update::Commands { .. }
            | Update::BrowserCall(_)
            | Update::ProviderLogin(_)
            | Update::McpLogin(_)
            | Update::FilesChanged { .. }
            | Update::ConversationSnapshot(_)
            | Update::ConversationFrame(_)
            | Update::TerminalSnapshot(_)
            | Update::TerminalFrame(_) => Err(Fault::new(
                ErrorCode::InvalidRequest,
                "resource update on Node subscription",
            )),
            Update::ResetRequired => {
                self.recovery = true;
                Ok(Apply::Recover)
            }
            Update::Snapshot(snapshot) => {
                self.check_target(snapshot.node)?;
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|current| snapshot.cursor < current.cursor)
                {
                    return Ok(Apply::Ignored);
                }
                self.snapshot = Some(snapshot);
                self.recovery = false;
                Ok(Apply::Applied)
            }
            Update::Event(envelope) => {
                self.check_target(envelope.node)?;
                let Some(snapshot) = self.snapshot.as_mut() else {
                    return Ok(Apply::Recover);
                };
                if envelope.cursor <= snapshot.cursor {
                    return Ok(Apply::Ignored);
                }
                if self.recovery || snapshot.cursor.checked_add(1) != Some(envelope.cursor) {
                    self.recovery = true;
                    return Ok(Apply::Recover);
                }
                match envelope.event {
                    Event::Unsupported => {}
                    Event::PluginChanged(plugin) => {
                        snapshot.plugins.retain(|item| item.name != plugin.name);
                        snapshot.plugins.push(plugin);
                        snapshot
                            .plugins
                            .sort_by(|left, right| left.name.cmp(&right.name));
                    }
                    Event::PluginValuesChanged { .. } => {}
                    Event::PluginRemoved { name } => {
                        snapshot.plugins.retain(|item| item.name != name)
                    }
                    Event::DispatchChanged { .. } => {}
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
                    }
                    Event::ModelCatalogChanged(status) => snapshot.model_catalog = status,
                    Event::ProviderRemoved { id } => {
                        snapshot.providers.retain(|item| item.id != id)
                    }
                    Event::ConversationChanged { session, activity } => {
                        if let Some(session) =
                            snapshot.sessions.iter_mut().find(|item| item.id == session)
                        {
                            session.activity = activity;
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
                    Event::TerminalSettingsChanged { revision } => {
                        snapshot.terminal_settings_revision = revision;
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
                            return Ok(Apply::Recover);
                        }
                        update_session(&mut snapshot.sessions, *session);
                        snapshot.turns.retain(|item| item.id != turn.id);
                        snapshot.turns.push(turn);
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
                        update_session(&mut snapshot.sessions, *backup);
                    }
                    Event::TurnQueued { turn, activity } => {
                        if let Some(session) = snapshot
                            .sessions
                            .iter_mut()
                            .find(|item| item.id == turn.session)
                        {
                            session.activity = activity;
                        }
                        snapshot.turns.retain(|item| item.id != turn.id);
                        snapshot.turns.push(turn);
                    }
                    Event::PlanAccepted(accepted) => {
                        update_session(&mut snapshot.sessions, accepted.session);
                        snapshot.turns.retain(|item| item.id != accepted.turn.id);
                        snapshot.turns.push(accepted.turn);
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
