//! Conversation entry points for the shared selectable diff surface.
use crate::content::diff::Line;
pub(crate) use crate::content::diff::{Kind, additions, unified};
use gpui_kit::component::{
    group_box::{GroupBox, GroupBoxVariants as _},
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
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
    lines: &[Line<'_>],
    path: &str,
    actions: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    let added = lines.iter().filter(|line| line.kind == Kind::Added).count();
    let removed = lines
        .iter()
        .filter(|line| line.kind == Kind::Removed)
        .count();
    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_owned();
    let tooltip_path = path.to_owned();
    let group = SharedString::from(format!("conversation-diff-{id}"));
    let frame_selector = group.clone();
    let header_selector = format!("diff-header-{id}");
    div()
        .id(group.clone())
        .group(group.clone())
        .debug_selector(move || frame_selector.to_string())
        .w_full()
        .min_w_0()
        .child(
            GroupBox::new()
                .outline()
                .content_style(
                    StyleRefinement::default()
                        .p_0()
                        .gap_0()
                        .overflow_hidden()
                        .bg(cx.theme().group_box.opacity(0.8)),
                )
                .child(
                    h_flex()
                        .debug_selector(move || header_selector.clone())
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
                            header.child(
                                h_flex()
                                    .flex_shrink_0()
                                    .gap_1()
                                    .opacity(0.)
                                    .group_hover(group, |style| style.opacity(1.))
                                    .children(actions),
                            )
                        })
                        .child(badge(added, removed, cx)),
                )
                // GPUI clips rectangular viewports. Keep only a bottom inset for
                // the rounded frame; the diff itself spans its full inner width.
                .child(div().pb(cx.theme().radius).child(
                    if crate::content::diff::is_binary(lines) {
                        super::notice(crate::tr("git_diff_binary"), false, cx).into_any_element()
                    } else {
                        crate::content::diff::rows(
                            id,
                            lines,
                            crate::content::diff::language(path),
                            cx,
                        )
                    },
                )),
        )
        .into_any_element()
}
