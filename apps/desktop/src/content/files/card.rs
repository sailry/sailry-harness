use super::*;

#[derive(IntoElement)]
pub(crate) struct Card {
    pub id: SharedString,
    pub file: File,
    pub binding: Binding,
    pub source: Entity<Chat>,
}

impl RenderOnce for Card {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let worktree = self.binding.worktree;
        let open = self.source.downgrade();
        let file = self.file.clone();
        let owner = self.source.downgrade();
        let actions = self.file.clone();
        let binding = self.binding.clone();
        let menu_id = format!("{}-actions", self.id);
        let selector = self.id.clone();
        v_flex()
            .w_full()
            .min_w_0()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box.opacity(0.8))
            .overflow_hidden()
            .debug_selector(move || selector.to_string())
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .p_2()
                    .gap_2()
                    .child(
                        h_flex()
                            .h(px(56.))
                            .flex_1()
                            .min_w_0()
                            .gap_3()
                            .px_2()
                            .debug_selector(|| "artifact-file-title".into())
                            .child(
                                div()
                                    .size_10()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(cx.theme().radius)
                                    .bg(cx.theme().muted)
                                    .child(crate::ui::file_icon::render(
                                        &self.file.path,
                                        &self.file.mime,
                                        rems(1.75),
                                    )),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(div().truncate().child(self.file.label().to_owned()))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(kind(&self.file)),
                                    ),
                            ),
                    )
                    .child(
                        Button::new(format!("{}-open", self.id))
                            .ghost()
                            .small()
                            .label(tr("artifact_open"))
                            .debug_selector(|| "artifact-file-open".into())
                            .on_click(move |_, _, cx| {
                                if let Some(worktree) = worktree {
                                    let _ = open.update(cx, |_, cx| {
                                        cx.emit(Event::Artifact(worktree, file.clone()))
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new(menu_id)
                            .ghost()
                            .small()
                            .icon(IconName::Ellipsis)
                            .accessibility_label(tr("more"))
                            .debug_selector(|| "artifact-file-more".into())
                            .dropdown_menu(move |menu, _, _| {
                                let source = owner.clone();
                                let file = actions.clone();
                                let download_file = actions.clone();
                                let download_binding = binding.clone();
                                let system_source = owner.clone();
                                let system_file = actions.clone();
                                menu.item(PopupMenuItem::new(tr("artifact_preview")).on_click(
                                    move |_, _, cx| {
                                        if let Some(worktree) = worktree {
                                            let _ = source.update(cx, |_, cx| {
                                                cx.emit(Event::Artifact(worktree, file.clone()))
                                            });
                                        }
                                    },
                                ))
                                .item(PopupMenuItem::new(tr("files_open_system")).on_click(
                                    move |_, _, cx| {
                                        if let Some(worktree) = worktree {
                                            let _ = system_source.update(cx, |_, cx| {
                                                cx.emit(Event::ArtifactExternal(
                                                    worktree,
                                                    system_file.path.clone(),
                                                ))
                                            });
                                        }
                                    },
                                ))
                                .item(
                                    PopupMenuItem::new(tr("files_download")).on_click(
                                        move |_, window, cx| {
                                            download(&download_binding, &download_file, window, cx)
                                        },
                                    ),
                                )
                            }),
                    ),
            )
    }
}
