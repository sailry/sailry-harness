//! Existing-session embeds retain the captured Node and ordinary conversation controls.
use super::*;

impl Scope {
    pub(super) fn session(&self, id: SessionId) -> Option<(Binding, Session)> {
        let snapshot = self.snapshot.borrow();
        let snapshot = snapshot.as_ref()?;
        if snapshot.node != self.binding.client.target()
            || !snapshot
                .plugins
                .iter()
                .any(|package| package.enabled && package.reference() == self.package)
        {
            return None;
        }
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.id == id && !session.archived)?
            .clone();
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.id == session.worktree)?;
        if worktree.project != session.project {
            return None;
        }
        let mut binding = self.binding.clone();
        binding.project = session.project;
        binding.worktree = Some(session.worktree);
        binding.project_name = snapshot
            .projects
            .iter()
            .find(|project| Some(project.id) == session.project)
            .map(|project| SharedString::from(project.name.clone()))
            .unwrap_or_default();
        binding.branch = SharedString::default();
        Some((binding, session))
    }
}

pub(super) fn options(session: &Session) -> ComposerOptions {
    match session.config.resource {
        Some(Resource::Database(_)) => ComposerOptions::connection(Mentions::Database),
        Some(Resource::Ssh(_)) => ComposerOptions::connection(Mentions::Attachments),
        None => ComposerOptions::default(),
    }
}

pub(super) fn forward(chat: &Entity<Chat>, owner: WeakEntity<super::super::Panel>, cx: &mut App) {
    cx.subscribe(chat, move |source, event: &Event, cx| {
        if !matches!(
            event,
            Event::PluginMounted(_)
                | Event::Settings(_)
                | Event::DictationSettings
                | Event::Link(_)
                | Event::File(_)
                | Event::FileAt(_, _)
                | Event::DirectoryAt(_, _)
                | Event::Artifact(_, _)
                | Event::ArtifactExternal(_, _)
                | Event::GitFile(_, _)
                | Event::UndoChanges(_, _)
                | Event::Subagent(_)
                | Event::Session(_)
                | Event::HostPage(_)
                | Event::ProjectPage(_, _)
                | Event::Forked(_)
        ) {
            return;
        }
        let _ = owner.update(cx, |_, cx| {
            cx.emit(super::super::ConversationEvent {
                source: source.clone(),
                action: event.clone(),
            });
        });
    })
    .detach();
}
