use super::*;

pub(crate) struct Panel {
    pub source: Entity<Chat>,
    binding: Binding,
    file: File,
    preview: Entity<crate::plugins::Panel>,
}
impl Panel {
    pub(crate) fn edit_context(
        &self,
        source: &Entity<Chat>,
        worktree: WorktreeId,
        path: &str,
        cx: &App,
    ) -> Option<(u32, String)> {
        (self.source == *source
            && self.binding.client.target() == source.read(cx).binding().client.target()
            && self.binding.worktree == Some(worktree)
            && self.file.path == path)
            .then(|| self.preview.read(cx).file_edit_context(cx))
            .flatten()
    }

    #[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
    pub(crate) fn file_preview(&self, cx: &App) -> Option<AnyView> {
        self.preview.read(cx).file_preview()
    }

    pub(crate) fn new(
        source: Entity<Chat>,
        mut binding: Binding,
        worktree: WorktreeId,
        file: File,
        cx: &mut App,
    ) -> Self {
        binding.worktree = Some(worktree);
        let preview = cx.new(|cx| {
            crate::plugins::Panel::artifact(binding.clone(), file.clone(), source.downgrade(), cx)
        });
        Self {
            source,
            binding,
            file,
            preview,
        }
    }
}
impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let binding = self.binding.clone();
        let file = self.file.clone();
        v_flex()
            .size_full()
            .min_h_0()
            .debug_selector(|| "artifact-panel".into())
            .child(
                crate::header::Header::new("artifact-header", cx)
                    .bordered(false)
                    .child(crate::ui::file_icon::render(
                        &self.file.path,
                        &self.file.mime,
                        rems(1.25),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(self.file.label().to_owned()),
                    )
                    .child(
                        Button::new("artifact-download")
                            .ghost()
                            .small()
                            .icon(IconName::ArrowDown)
                            .tooltip(tr("files_download"))
                            .on_click(move |_, window, cx| download(&binding, &file, window, cx)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.preview.clone()),
            )
    }
}
