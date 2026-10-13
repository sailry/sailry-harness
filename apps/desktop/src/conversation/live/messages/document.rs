//! Typed document cards preserve the session, owner and published revision.
use super::*;
use gpui_kit::component::{
    menu::{DropdownMenu as _, PopupMenuItem},
    tooltip::Tooltip,
};

impl View {
    pub(super) fn document_card(
        &self,
        artifact: sailry_protocol::artifact::Summary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let source = cx.entity().downgrade();
        let id = artifact.id;
        let revision = artifact.revision;
        let open = artifact.clone();
        let actions = artifact.clone();
        let owner = source.clone();
        let title = artifact.title.clone();
        let summary = artifact.summary.clone();
        let exportable = self.connected()
            && (self.document_visible(&artifact) || self.document_listed(&artifact, cx));
        v_flex()
            .w_full()
            .min_w_0()
            .rounded(cx.theme().radius_2xl())
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box.opacity(0.8))
            .overflow_hidden()
            .debug_selector(move || format!("artifact-document-{id}-{revision}"))
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
                            .debug_selector(move || {
                                format!("artifact-document-title-{id}-{revision}")
                            })
                            .child(
                                div()
                                    .size_10()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(cx.theme().radius)
                                    .bg(cx.theme().muted)
                                    .debug_selector(move || {
                                        format!("artifact-document-icon-{id}-{revision}")
                                    })
                                    .child(crate::ui::file_icon::render(
                                        &artifact.title,
                                        "text/html",
                                        rems(1.75),
                                    )),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .id(format!("artifact-document-label-{id}-{revision}"))
                                            .test_support()
                                            .min_w_0()
                                            .truncate()
                                            .aria_label(artifact.title.clone())
                                            .tooltip(move |window, cx| {
                                                Tooltip::new(title.clone()).build(window, cx)
                                            })
                                            .debug_selector(move || {
                                                format!("artifact-document-label-{id}-{revision}")
                                            })
                                            .child(artifact.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .id(format!(
                                                "artifact-document-summary-{id}-{revision}"
                                            ))
                                            .test_support()
                                            .min_w_0()
                                            .truncate()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .aria_label(artifact.summary.clone())
                                            .tooltip(move |window, cx| {
                                                Tooltip::new(summary.clone()).build(window, cx)
                                            })
                                            .debug_selector(move || {
                                                format!("artifact-document-summary-{id}-{revision}")
                                            })
                                            .child(artifact.summary.clone()),
                                    ),
                            ),
                    )
                    .child(
                        Button::new(format!("artifact-document-open-{id}-{revision}"))
                            .ghost()
                            .small()
                            .icon(IconName::ExternalLink)
                            .tooltip(tr("artifact_open"))
                            .accessibility_label(tr("artifact_open"))
                            .debug_selector(move || {
                                format!("artifact-document-open-{id}-{revision}")
                            })
                            .on_click(move |_, _, cx| {
                                let _ = source
                                    .update(cx, |_, cx| cx.emit(Event::Document(open.clone())));
                            }),
                    )
                    .child(
                        Button::new(format!("artifact-document-more-{id}-{revision}"))
                            .ghost()
                            .small()
                            .icon(IconName::Ellipsis)
                            .tooltip(tr("more"))
                            .accessibility_label(tr("more"))
                            .debug_selector(move || {
                                format!("artifact-document-more-{id}-{revision}")
                            })
                            .dropdown_menu(move |menu, _, _| {
                                let opening = owner.clone();
                                let selected = actions.clone();
                                let html_owner = owner.clone();
                                let html = actions.clone();
                                let image_owner = owner.clone();
                                let image = actions.clone();
                                menu.item(PopupMenuItem::new(tr("artifact_open")).on_click(
                                    move |_, _, cx| {
                                        let _ = opening.update(cx, |_, cx| {
                                            cx.emit(Event::Document(selected.clone()))
                                        });
                                    },
                                ))
                                .item(
                                    PopupMenuItem::new(tr("conversation_export_html"))
                                        .disabled(!exportable)
                                        .on_click(move |_, window, cx| {
                                            if let Some(source) = html_owner.upgrade() {
                                                crate::plugins::Panel::export_document(
                                                    source,
                                                    html.clone(),
                                                    false,
                                                    window,
                                                    cx,
                                                );
                                            }
                                        }),
                                )
                                .when(
                                    actions.kind == sailry_protocol::artifact::Kind::Document,
                                    |menu| {
                                        menu.item(
                                            PopupMenuItem::new(tr("conversation_export_image"))
                                                .disabled(!exportable)
                                                .on_click(move |_, window, cx| {
                                                    if let Some(source) = image_owner.upgrade() {
                                                        crate::plugins::Panel::export_document(
                                                            source,
                                                            image.clone(),
                                                            true,
                                                            window,
                                                            cx,
                                                        );
                                                    }
                                                }),
                                        )
                                    },
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
}
