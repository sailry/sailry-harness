//! Session mentions load through the originating Node and the ordinary chat factory.
use super::*;

impl Shell {
    pub(super) fn open_session_reference(
        &mut self,
        source: &Entity<Chat>,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut binding = source.read(cx).binding();
        let node = binding.client.target();
        if self
            .live
            .as_ref()
            .is_none_or(|live| live.transport_for(node).is_none())
        {
            crate::feedback::error("", &tr("reference_unavailable"), window, cx);
            return;
        }
        let client = binding.client.clone();
        let job = binding
            .runtime
            .spawn(async move { client.execute(client.prepare(Command::Snapshot)).await });
        cx.spawn_in(window, async move |shell, cx| {
            let result = job.await;
            let _ = shell.update_in(cx, |shell, window, cx| {
                let Ok(Ok(Output::Snapshot(snapshot))) = result else {
                    crate::feedback::error("", &tr("reference_unavailable"), window, cx);
                    return;
                };
                let session = snapshot
                    .sessions
                    .iter()
                    .find(|session| session.id == id && session.delegation.is_none())
                    .cloned();
                let Some(session) = session else {
                    crate::feedback::error("", &tr("reference_unavailable"), window, cx);
                    return;
                };
                if shell
                    .live
                    .as_ref()
                    .is_none_or(|live| live.transport_for(node).is_none())
                {
                    crate::feedback::error("", &tr("reference_unavailable"), window, cx);
                    return;
                }
                if !shell.chats.views.contains_key(&(node, id)) {
                    binding.project = session.project;
                    binding.worktree = Some(session.worktree);
                    binding.project_name = snapshot
                        .projects
                        .iter()
                        .find(|project| Some(project.id) == session.project)
                        .map(|project| project.name.clone().into())
                        .unwrap_or_default();
                    binding.branch = snapshot
                        .worktrees
                        .iter()
                        .find(|tree| tree.id == session.worktree)
                        .map(|tree| tree.path.clone().into())
                        .unwrap_or_default();
                    let view = shell.chat_view(binding, Some(session), window, cx);
                    shell.chats.views.insert((node, id), view);
                }
                shell.activate_session(Key::Session(node, id), window, cx);
            });
        })
        .detach();
    }
}
