use super::*;

pub(crate) struct Stamp {
    page: Option<Arc<sailry_protocol::conversation::Page>>,
    statistics: Arc<sailry_protocol::conversation::Statistics>,
    session: Option<(SessionId, u64)>,
    state: State,
}

#[derive(PartialEq, Eq)]
struct State {
    node: NodeId,
    worktree: Option<WorktreeId>,
    config_owner: NodeId,
    config: Option<SessionConfig>,
    provider: Option<(sailry_protocol::ProviderId, u64)>,
    input: SharedString,
    command: Option<crate::plugins::contributions::Key>,
    declarations: Vec<sailry_protocol::plugin::Summary>,
    connected: bool,
    busy: bool,
    configuring: bool,
    readonly: bool,
    sidebar: bool,
    has_messages: bool,
    context_limit: Option<u32>,
    mode: sailry_protocol::WorkMode,
    connection: sailry_link::ConnectionInfo,
}

impl PartialEq for Stamp {
    fn eq(&self, other: &Self) -> bool {
        self.session == other.session
            && match (&self.page, &other.page) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
            && Arc::ptr_eq(&self.statistics, &other.statistics)
            && self.state == other.state
    }
}

impl View {
    pub(crate) fn composer_stamp(&self, cx: &App) -> Stamp {
        let input = self.input.read(cx).value();
        let command = references::commands::leading(&input)
            .and_then(|(name, _, _)| self.message_command(name, cx))
            .cloned();
        Stamp {
            page: self
                .history
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.page.clone()),
            statistics: self.history.statistics.clone(),
            session: self
                .session
                .as_ref()
                .map(|session| (session.id, session.revision)),
            state: State {
                node: self.binding.client.target(),
                worktree: self.binding.worktree,
                config_owner: self.config_owner,
                config: self.config.clone(),
                provider: self.config.as_ref().and_then(|config| {
                    self.providers()
                        .find(|provider| provider.id == config.provider)
                        .map(|provider| (provider.id, provider.revision))
                }),
                input,
                command,
                declarations: self
                    .contribution_metadata
                    .read(cx)
                    .entries
                    .values()
                    .map(|info| info.summary.clone())
                    .collect(),
                connected: self.connected(),
                busy: self.busy(),
                configuring: self.configuration_blocked(),
                readonly: self.readonly(),
                sidebar: self.sidebar,
                has_messages: !self.rows.is_empty(),
                context_limit: self.context_limit(),
                mode: self.current_mode(),
                connection: self.node.connection,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(statistics: Arc<sailry_protocol::conversation::Statistics>) -> Stamp {
        Stamp {
            page: None,
            statistics,
            session: None,
            state: State {
                node: NodeId([1; 32]),
                worktree: None,
                config_owner: NodeId([1; 32]),
                config: None,
                provider: None,
                input: "/publish draft".into(),
                command: Some(crate::plugins::contributions::Key {
                    node: NodeId([1; 32]),
                    worktree: None,
                    session: None,
                    package: sailry_protocol::plugin::Reference {
                        name: "fixture".into(),
                        digest: "a".repeat(64),
                        settings_revision: 0,
                    },
                    id: "publish".into(),
                }),
                declarations: Vec::new(),
                connected: true,
                busy: false,
                configuring: false,
                readonly: false,
                sidebar: false,
                has_messages: false,
                context_limit: None,
                mode: sailry_protocol::WorkMode::Code,
                connection: Default::default(),
            },
        }
    }

    #[core::prelude::v1::test]
    fn invalidates_unavailable_message_commands() {
        let statistics = Arc::default();
        let before = stamp(Arc::clone(&statistics));
        let mut after = stamp(statistics);
        assert!(before == after);
        // A contribution can hide or disable a command without changing its package.
        after.state.command = None;
        assert!(before != after);
        assert_eq!(before.state.input, after.state.input);
        assert_eq!(before.state.declarations, after.state.declarations);
    }
}
