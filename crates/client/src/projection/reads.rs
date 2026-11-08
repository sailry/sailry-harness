use sailry_protocol::{Event, plugin::Action};

/// Invalidation for declared catalog reads, independent of conversation traffic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadCursors {
    roles: u64,
    models: u64,
    worktrees: u64,
    terminals: u64,
    terminal_tools: u64,
}

impl ReadCursors {
    pub fn terminal_tools(&self) -> u64 {
        self.terminal_tools
    }

    pub fn for_actions(&self, actions: &[Action]) -> u64 {
        actions
            .iter()
            .map(|action| match action {
                Action::ReadRoles => self.roles,
                Action::ReadModels => self.models,
                Action::ReadWorktrees => self.worktrees,
                Action::ReadTerminals => self.terminals,
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }

    pub(super) fn reset(&mut self, revision: u64) {
        *self = Self {
            roles: revision,
            models: revision,
            worktrees: revision,
            terminals: revision,
            terminal_tools: revision,
        };
    }

    pub(super) fn observes(event: &Event) -> bool {
        matches!(
            event,
            Event::RoleChanged(_)
                | Event::RoleRemoved { .. }
                | Event::ProviderChanged(_)
                | Event::ProviderRemoved { .. }
                | Event::ModelCatalogChanged(_)
                | Event::LocalModelsChanged { .. }
                | Event::TerminalChanged(_)
                | Event::TerminalSettingsChanged { .. }
                | Event::WorktreeRegistered(_)
                | Event::WorktreeRemoved { .. }
        )
    }

    pub(super) fn observe(&mut self, event: &Event, revision: u64) {
        match event {
            Event::RoleChanged(_) | Event::RoleRemoved { .. } => self.roles = revision,
            Event::ProviderChanged(_)
            | Event::ProviderRemoved { .. }
            | Event::ModelCatalogChanged(_)
            | Event::LocalModelsChanged { .. } => self.models = revision,
            Event::TerminalSettingsChanged { .. } => {
                self.terminals = revision;
                self.terminal_tools = revision;
            }
            Event::TerminalChanged(_) => self.terminals = revision,
            Event::WorktreeRegistered(_) => self.worktrees = revision,
            Event::WorktreeRemoved { .. } => {
                self.worktrees = revision;
                self.terminals = revision;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_ignore_terminal_inventory() {
        let mut cursors = ReadCursors::default();
        cursors.reset(1);
        cursors.observe(
            &Event::WorktreeRemoved {
                id: sailry_protocol::WorktreeId::new(),
            },
            2,
        );
        assert_eq!(cursors.terminal_tools(), 1);
        assert_eq!(cursors.for_actions(&[Action::ReadTerminals]), 2);
        cursors.observe(&Event::TerminalSettingsChanged { revision: 4 }, 3);
        assert_eq!(cursors.terminal_tools(), 3);
        cursors.reset(4);
        assert_eq!(cursors.terminal_tools(), 4);
    }
}
