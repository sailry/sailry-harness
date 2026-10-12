//! Conversation entry points for the shared selectable diff surface.
use gpui_kit::component::{
    group_box::{GroupBox, GroupBoxVariants as _},
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::test::TestSupportExt as _;
use gpui_kit::{component::*, *};

/// `+N −M` summary in Kit success/danger tones.
pub(crate) fn badge(added: usize, removed: usize, cx: &App) -> AnyElement {
    h_flex()
        .flex_shrink_0()
        .gap_1p5()
        .text_xs()
        .font_family(cx.theme().mono_font_family.clone())
        .when(added > 0 || removed == 0, |row| {
            row.child(
                div()
                    .text_color(cx.theme().success)
                    .child(format!("+{added}")),
            )
        })
        .when(removed > 0, |row| {
            row.child(
                div()
                    .text_color(cx.theme().danger)
                    .child(format!("\u{2212}{removed}")),
            )
        })
        .into_any_element()
}

pub(crate) fn file_rows(
    id: &str,
    text: &str,
    path: &str,
    added: bool,
    actions: Vec<AnyElement>,
) -> AnyElement {
    FileRows {
        id: id.to_owned(),
        text: text.to_owned(),
        path: path.to_owned(),
        added,
        actions,
    }
    .into_any_element()
}

#[derive(IntoElement)]
struct FileRows {
    id: String,
    text: String,
    path: String,
    added: bool,
    actions: Vec<AnyElement>,
}
impl RenderOnce for FileRows {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            text,
            path,
            added,
            actions,
        } = self;
        let state =
            window.use_keyed_state(SharedString::from(format!("diff-{id}")), cx, |_, cx| {
                crate::content::diff::State::new(cx)
            });
        state.update(cx, |state, cx| state.set_patch(&text, &path, added, cx));
        let (added, removed, _) = state.read(cx).summary(cx);
        let rows = state.read(cx).row_count().clamp(1, 5);
        let filename = std::path::Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&path)
            .to_owned();
        let tooltip_path = path.to_owned();
        let group = SharedString::from(format!("conversation-diff-{id}"));
        let header_selector = format!("diff-header-{id}");
        div()
            .id(group.clone())
            .group(group.clone())
            .test_support()
            .w_full()
            .min_w_0()
            .child(
                GroupBox::new()
                    .rounded(cx.theme().radius_2xl())
                    .outline()
                    .content_style(
                        StyleRefinement::default()
                            .p_0()
                            .gap_0()
                            .overflow_hidden()
                            .bg(crate::theme::review_background(cx)),
                    )
                    .child(
                        h_flex()
                            .id(header_selector)
                            .test_support()
                            .w_full()
                            .min_w_0()
                            .h_8()
                            .px_3()
                            .gap_2()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                div()
                                    .id(format!("diff-filename-{id}"))
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(tooltip_path.clone()).build(window, cx)
                                    })
                                    .child(filename),
                            )
                            .when(!actions.is_empty(), |header| {
                                header.child(super::Actions {
                                    id: format!("diff-actions-{id}").into(),
                                    group,
                                    actions,
                                    absolute: false,
                                })
                            })
                            .child(badge(added, removed, cx)),
                    )
                    // GPUI clips rectangular viewports. Keep only a bottom inset for
                    // the rounded frame; the diff itself spans its full inner width.
                    .child(
                        div().pb(cx.theme().radius_2xl()).child(
                            div()
                                .id(format!("diff-body-{id}"))
                                .test_support()
                                .w_full()
                                .h(window.rem_size()
                                    * (f32::from(cx.theme().mono_font_size) / 16.)
                                    * 1.6
                                    * rows as f32)
                                .child(state),
                        ),
                    ),
            )
            .into_any_element()
    }
}
