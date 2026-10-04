use super::*;
use gpui_kit::component::tab::{Tab, TabBar};

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("project-editor")
            .debug_selector(|| "project-editor".into())
            .gap_4()
            .max_h((window.viewport_size().height - px(200.)).max(px(120.)))
            .overflow_y_scrollbar()
            .when(self.live.is_none(), |body| {
                body.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("project_editor_preview")),
                )
            })
            .when(
                self.original.is_none() && self.live_original.is_none(),
                |body| {
                    body.child(
                        TabBar::new("project-source")
                            .segmented()
                            .equal_width()
                            .menu(false)
                            .w_full()
                            .selected_index(usize::from(self.clone))
                            .child(
                                Tab::new()
                                    .label(tr("project_local"))
                                    .debug_selector(|| "project-local".into()),
                            )
                            .child(
                                Tab::new()
                                    .label(tr("project_clone"))
                                    .debug_selector(|| "project-clone".into()),
                            )
                            .on_click(cx.listener(|editor, &index, window, cx| {
                                editor.select_source(index == 1, window, cx);
                            })),
                    )
                },
            )
            .child(
                gpui_kit::component::form::Form::vertical()
                    .gap_4()
                    .child(
                        field().label(tr("project_name")).required(true).child(
                            div().debug_selector(|| "project_name-input".into()).child(
                                Input::new(&self.inputs[0])
                                    .disabled(self.pending)
                                    .prefix(self.appearance_picker(cx))
                                    .aria_label(tr("project_name")),
                            ),
                        ),
                    )
                    .child(
                        field()
                            .label(tr("project_host"))
                            .child(self.host_picker(cx)),
                    )
                    .when(self.clone, |form| {
                        form.child(
                            field()
                                .label(tr("project_repository"))
                                .required(true)
                                .child(
                                    div().debug_selector(|| "project-repository".into()).child(
                                        Input::new(&self.repository)
                                            .disabled(self.pending)
                                            .aria_label(tr("project_repository")),
                                    ),
                                ),
                        )
                        .child(
                            field().label(tr("project_branch")).child(
                                div().debug_selector(|| "project-branch".into()).child(
                                    Input::new(&self.branch)
                                        .disabled(self.pending)
                                        .aria_label(tr("project_branch")),
                                ),
                            ),
                        )
                    })
                    .child(
                        field()
                            .label(tr(if self.clone {
                                "project_destination"
                            } else {
                                "project_path"
                            }))
                            .required(true)
                            .child(
                                div().debug_selector(|| "project_path-input".into()).child(
                                    Input::new(&self.inputs[1])
                                        .disabled(self.pending)
                                        .aria_label(tr("project_path"))
                                        .suffix(
                                            Button::new("project-browse")
                                                .text()
                                                .small()
                                                .icon(IconName::FolderOpen)
                                                .disabled(self.pending)
                                                .tooltip(tr("directory_title"))
                                                .accessibility_label(tr("directory_title"))
                                                .debug_selector(|| "project-browse".into())
                                                .on_click(cx.listener(|_, _, window, cx| {
                                                    let editor = cx.entity();
                                                    window.defer(cx, move |window, cx| {
                                                        directory::open(editor, window, cx);
                                                    });
                                                })),
                                        ),
                                ),
                            ),
                    ),
            )
            .when(self.clone, |body| {
                body.when_some(self.destination(cx).ok(), |body, path| {
                    body.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(path),
                    )
                })
            })
    }
}
