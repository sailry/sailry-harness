//! Typed document cards preserve the session, owner and published revision.
use super::*;

impl View {
    pub(super) fn document_card(
        &self,
        artifact: sailry_protocol::artifact::Summary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let source = cx.entity().downgrade();
        let id = artifact.id;
        let revision = artifact.revision;
        let updated = frame::full_at(artifact.updated_at);
        h_flex()
            .w_full()
            .min_w_0()
            .p_3()
            .gap_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box.opacity(0.8))
            .debug_selector(move || format!("artifact-document-{id}-{revision}"))
            .child(
                Icon::new(IconName::FileText)
                    .size_5()
                    .text_color(cx.theme().primary),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .gap_1()
                    .child(div().truncate().font_medium().child(artifact.title.clone()))
                    .when(!artifact.summary.is_empty(), |column| {
                        column.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(artifact.summary.clone()),
                        )
                    })
                    .when_some(updated, |column, updated| {
                        column.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(updated),
                        )
                    }),
            )
            .child(
                Button::new(format!("artifact-document-open-{id}-{revision}"))
                    .ghost()
                    .small()
                    .label(tr("artifact_open"))
                    .debug_selector(move || format!("artifact-document-open-{id}-{revision}"))
                    .on_click(move |_, _, cx| {
                        let _ =
                            source.update(cx, |_, cx| cx.emit(Event::Document(artifact.clone())));
                    }),
            )
            .into_any_element()
    }
}
