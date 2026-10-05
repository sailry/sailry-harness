//! Shared activity surfaces for tool output, commands and file changes.
//!
//! Tool output uses a left rule; review surfaces retain their own framing.
use crate::tr;
use gpui_kit::component::{
    clipboard::Clipboard,
    scroll::{ScrollableElement as _, ScrollableMask},
    text::{TextView, TextViewStyle},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub(crate) mod diff;
mod thought;
pub(crate) use thought::thought;
mod work;
pub(crate) use work::{trigger, work};
#[cfg(test)]
mod scrolling;

/// Tool output uses a five-line viewport; padding lives outside it.
pub(crate) const OUTPUT_LINES: usize = 5;
pub(crate) const LINE_HEIGHT: f32 = 20.;
const GROUP_HEIGHT: f32 = 320.;

/// Core tool labels; package tools carry their own captured display declarations.
pub(crate) fn tool_label(name: &str) -> SharedString {
    let key = match name {
        "set_session_title" => "tool_session_title",
        "load_skill" => "tool_load_skill",
        "read_skill_resource" => "tool_read_skill_resource",
        "compact_context" => "chat_compact",
        _ => return name.to_owned().into(),
    };
    tr(key)
}

/// Core tool icons; callers supply the generic fallback.
pub(crate) fn tool_icon(name: &str) -> Option<Icon> {
    Some(Icon::new(match name {
        "compact_context" => IconName::FileText,
        "load_skill" | "read_skill_resource" => IconName::BookOpen,
        _ => return None,
    }))
}

/// A bordered card; the header keeps the activity identity visible while the body scrolls.
pub(crate) fn card(header: Option<AnyElement>, body: AnyElement, cx: &App) -> Div {
    v_flex()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().group_box.opacity(0.8))
        .when_some(header, |card, header| {
            card.child(
                div()
                    .w_full()
                    .min_w_0()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(header),
            )
        })
        .child(body)
}

/// Tool results share one frame, without repeating the disclosure's title.
pub(crate) fn result(
    key: &str,
    body: AnyElement,
    actions: impl IntoIterator<Item = AnyElement>,
    cx: &App,
) -> Stateful<Div> {
    let group = SharedString::from(format!("tool-result-{key}"));
    h_flex()
        .id(group.clone())
        .group(group.clone())
        .relative()
        .w_full()
        .min_w_0()
        .items_start()
        .border_l_4()
        .text_color(cx.theme().muted_foreground)
        .border_color(cx.theme().border)
        .child(div().w_full().min_w_0().child(body))
        .child(
            h_flex()
                .id(format!("tool-actions-{key}"))
                .absolute()
                .top_0()
                .right_0()
                .gap_1()
                .p_1()
                .opacity(0.)
                .group_hover(group, |style| style.opacity(1.))
                .children(actions),
        )
}

/// Selectable literal text; the result supplies the frame, so the code block itself is bare.
pub(crate) fn code(id: &str, text: &str, cx: &App) -> AnyElement {
    scroll(format!("{id}-scroll"), literal_text(id, text, cx))
}

pub(crate) fn literal_text(id: &str, text: &str, cx: &App) -> AnyElement {
    let bare = StyleRefinement::default()
        .bg(cx.theme().transparent)
        .text_color(cx.theme().muted_foreground)
        .p_0()
        .line_height(px(LINE_HEIGHT));
    div()
        .px_3()
        .debug_selector({
            let id = id.to_owned();
            move || format!("{id}-content")
        })
        .child(
            TextView::markdown(id.to_owned(), literal(text))
                .selectable(true)
                .line_height(px(LINE_HEIGHT))
                .style(TextViewStyle::default().code_block(bare)),
        )
        .into_any_element()
}

/// Bounded tool output with Kit wheel arbitration inside the message list.
pub(crate) fn scroll(id: String, content: impl IntoElement) -> AnyElement {
    ActivityScroll {
        id: id.into(),
        content: content.into_any_element(),
        group: false,
    }
    .into_any_element()
}

/// Tool groups grow naturally until they reach half the window or the group cap.
pub(crate) fn group_scroll(id: String, content: impl IntoElement) -> AnyElement {
    ActivityScroll {
        id: id.into(),
        content: content.into_any_element(),
        group: true,
    }
    .into_any_element()
}

#[derive(IntoElement)]
struct ActivityScroll {
    id: SharedString,
    content: AnyElement,
    group: bool,
}

impl RenderOnce for ActivityScroll {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let handle = window
            .use_keyed_state((ElementId::from(self.id.clone()), "offset"), cx, |_, _| {
                ScrollHandle::default()
            })
            .read(cx)
            .clone();
        let selector = self.id.clone();
        let height = if self.group {
            px(GROUP_HEIGHT).min(window.viewport_size().height * 0.5)
        } else {
            px(OUTPUT_LINES as f32 * LINE_HEIGHT)
        };
        div().w_full().min_w_0().py_2().child(
            div()
                .relative()
                .w_full()
                .min_w_0()
                .child(
                    div()
                        .id(self.id.clone())
                        .debug_selector(move || selector.to_string())
                        .w_full()
                        .max_h(height)
                        .when(self.group, |viewport| viewport.pr_3())
                        .overflow_y_scroll()
                        .lock_scroll_axis()
                        .track_scroll(&handle)
                        .child(self.content),
                )
                .vertical_scrollbar(&handle)
                // A sibling mask stays over the viewport as its content moves.
                // It consumes the wheel before MessageScroller, chaining only at an edge.
                .child(ScrollableMask::new(Axis::Vertical, &handle).id(self.id)),
        )
    }
}

/// Short status line inside a body; `danger` marks a failure.
pub(crate) fn notice(text: impl Into<SharedString>, danger: bool, cx: &App) -> Div {
    div()
        .px_3()
        .py_2()
        .text_sm()
        .text_color(if danger {
            cx.theme().danger
        } else {
            cx.theme().muted_foreground
        })
        .child(text.into())
}

/// Compact copy action alongside the result.
pub(crate) fn copy(id: String, value: impl Into<SharedString>) -> Clipboard {
    Clipboard::new(id).value(value).tooltip(tr("tool_copy"))
}

/// Keep file/tool text literal even when it contains Markdown fences or links.
pub(crate) fn literal(text: &str) -> String {
    let longest = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    let separator = if text.ends_with('\n') { "" } else { "\n" };
    format!("{fence}text\n{text}{separator}{fence}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn skills_have_core_labels_and_icons() {
        for name in ["load_skill", "read_skill_resource"] {
            let label = tool_label(name);
            assert_ne!(label.as_ref(), name);
            assert!(!label.is_empty() && !label.starts_with("tool_"));
            assert!(tool_icon(name).is_some());
        }
    }

    #[test]
    fn literal_line_endings() {
        assert_eq!(literal("line\n"), "```text\nline\n```");
        assert_eq!(literal("line"), "```text\nline\n```");
        assert_eq!(literal("line\n\n"), "```text\nline\n\n```");
        assert_eq!(literal("```\n"), "````text\n```\n````");
    }
}
