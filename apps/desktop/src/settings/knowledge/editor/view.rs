use super::*;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{
    checkbox::Checkbox,
    form::{Field, Form},
    input::{Input, Textarea},
    scroll::ScrollableElement,
    select::Select,
};

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.pending || self.request.is_some();
        let kind = self.source.read(cx).selected_value().copied();
        let mut form = Form::vertical()
            .child(
                Field::new().label(tr("knowledge_name")).child(
                    div()
                        .w_full()
                        .debug_selector(|| "knowledge-name".into())
                        .child(
                            Input::new(&self.name)
                                .id("knowledge-name")
                                .disabled(locked)
                                .aria_label(tr("knowledge_name")),
                        ),
                ),
            )
            .child(
                Field::new().label(tr("knowledge_source")).child(
                    div()
                        .w_full()
                        .debug_selector(|| "knowledge-source".into())
                        .child(
                            Select::new(&self.source)
                                .id("knowledge-source")
                                .w_full()
                                .disabled(locked || kind == Some(SourceKind::Storage))
                                .accessibility_label(tr("knowledge_source")),
                        ),
                ),
            );
        if kind == Some(SourceKind::Worktree) {
            form = form.child(
                Field::new().label(tr("knowledge_worktree")).child(
                    div()
                        .w_full()
                        .debug_selector(|| "knowledge-worktree".into())
                        .child(
                            Select::new(&self.worktree)
                                .id("knowledge-worktree")
                                .w_full()
                                .disabled(locked)
                                .accessibility_label(tr("knowledge_worktree")),
                        ),
                ),
            );
        }
        let path_label = if kind == Some(SourceKind::Worktree) {
            "knowledge_relative_path"
        } else {
            "knowledge_node_path"
        };
        form = form
            .child(
                Field::new().label(tr(path_label)).child(
                    div()
                        .w_full()
                        .debug_selector(|| "knowledge-path".into())
                        .child(
                            Input::new(&self.path)
                                .id("knowledge-path")
                                .disabled(locked || kind == Some(SourceKind::Storage))
                                .aria_label(tr(path_label)),
                        ),
                ),
            )
            .child(
                Field::new().label(tr("knowledge_collection_model")).child(
                    div()
                        .w_full()
                        .debug_selector(|| "knowledge-collection-model".into())
                        .child(
                            Select::new(&self.model)
                                .id("knowledge-collection-model")
                                .w_full()
                                .disabled(locked)
                                .accessibility_label(tr("knowledge_collection_model")),
                        ),
                ),
            )
            .child(
                Field::new().label(tr("knowledge_globs")).child(
                    div()
                        .id("knowledge-globs")
                        .test_support()
                        .w_full()
                        .debug_selector(|| "knowledge-globs".into())
                        .child(
                            Textarea::new(&self.globs)
                                .disabled(locked || kind == Some(SourceKind::Storage))
                                .aria_label(tr("knowledge_globs")),
                        ),
                ),
            )
            .child(
                Field::new().label(tr("knowledge_tags")).child(
                    div()
                        .w_full()
                        .debug_selector(|| "knowledge-tags".into())
                        .child(
                            Textarea::new(&self.tags)
                                .disabled(locked)
                                .aria_label(tr("knowledge_tags")),
                        ),
                ),
            )
            .child(
                Field::new().child(
                    div().debug_selector(|| "knowledge-share".into()).child(
                        Checkbox::new("knowledge-share")
                            .label(tr("knowledge_share"))
                            .checked(self.shared)
                            .disabled(locked)
                            .on_click(cx.listener(|editor, shared: &bool, _, cx| {
                                editor.shared = *shared;
                                cx.notify();
                            })),
                    ),
                ),
            );
        div()
            .id("knowledge-editor-form")
            .max_h((window.viewport_size().height - px(260.)).max(px(160.)))
            .overflow_y_scrollbar()
            .child(form)
    }
}
