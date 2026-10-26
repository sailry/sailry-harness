use sailry_protocol::{Event, plugin::Action};

/// Invalidation for declared catalog reads, independent of conversation traffic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadCursors {
    roles: u64,
    models: u64,
    terminals: u64,
}

impl ReadCursors {
    pub fn for_actions(&self, actions: &[Action]) -> u64 {
        actions
            .iter()
            .map(|action| match action {
                Action::ReadRoles => self.roles,
                Action::ReadModels => self.models,
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
            terminals: revision,
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
            Event::TerminalChanged(_)
            | Event::TerminalSettingsChanged { .. }
            | Event::WorktreeRemoved { .. } => self.terminals = revision,
            _ => {}
        }
    }
}
