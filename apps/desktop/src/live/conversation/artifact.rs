use super::*;

impl Shell {
    pub(super) fn open_session_document(
        &mut self,
        source: Entity<Chat>,
        artifact: sailry_protocol::artifact::Summary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !source.read(cx).document_visible(&artifact) {
            return;
        }
        let original = source.clone();
        let retry = artifact.clone();
        if self.guard_file_navigation(window, cx, move |shell, window, cx| {
            shell.open_session_document(original.clone(), retry.clone(), window, cx)
        }) {
            return;
        }
        let panel = cx.new(|cx| crate::plugins::Panel::document(source, artifact, cx));
        Self::observe_plugin_conversations(&panel, window, cx);
        cx.observe(&panel, |_, _, cx| cx.notify()).detach();
        self.side_resource = Some(SideResource::Plugin(panel));
        self.layout.panel_open[0] = true;
        cx.notify();
    }
}
