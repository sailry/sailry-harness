//! Shared activity surfaces for tool output, commands and file changes.
//!
//! Tool results share a rounded frame; review surfaces retain their own framing.
use crate::tr;
use gpui_kit::base::TestSupportExt as _;
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
        "plan_review" => "composer_mode_plan",
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
        .rounded(cx.theme().radius_2xl())
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
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .p_3()
        .text_color(cx.theme().muted_foreground)
        .child(div().w_full().min_w_0().child(body))
        .child(Actions {
            id: format!("tool-actions-{key}").into(),
            group,
            actions: actions.into_iter().collect(),
            absolute: true,
        })
}

#[derive(IntoElement)]
pub(super) struct Actions {
    pub id: SharedString,
    pub group: SharedString,
    pub actions: Vec<AnyElement>,
    pub absolute: bool,
}
struct ActionFocus {
    focus: FocusHandle,
    _subscriptions: [Subscription; 2],
}
impl RenderOnce for Actions {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |window, cx| {
            let focus = cx.focus_handle().tab_stop(false);
            ActionFocus {
                _subscriptions: [
                    cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
                    cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
                ],
                focus,
            }
        });
        let focus = state.read(cx).focus.clone();
        let visible = focus.contains_focused(window, cx);
        h_flex()
            .id(self.id)
            .test_support()
            .track_focus(&focus)
            .gap_1()
            .when(self.absolute, |row| row.absolute().top_0().right_0().p_1())
            .when(!self.absolute, |row| row.flex_shrink_0())
            .opacity(if visible { 1. } else { 0. })
            .group_hover(self.group, |style| style.opacity(1.))
            .children(self.actions)
    }
}

/// Selectable literal text; the result supplies the frame, so the code block itself is bare.
pub(crate) fn code(id: &str, text: &str, text_size: Pixels, cx: &App) -> AnyElement {
    scroll(
        format!("{id}-scroll"),
        literal_text(id, text, text_size, cx),
    )
}

pub(crate) fn literal_text(id: &str, text: &str, text_size: Pixels, cx: &App) -> AnyElement {
    let line_height = text_size * (LINE_HEIGHT / 14.);
    let bare = StyleRefinement::default()
        .bg(cx.theme().transparent)
        .text_color(cx.theme().muted_foreground)
        .p_0()
        .border_0()
        .text_size(text_size)
        .line_height(line_height);
    div()
        .px_3()
        .debug_selector({
            let id = id.to_owned();
            move || format!("{id}-content")
        })
        .child(
            TextView::markdown(id.to_owned(), literal(text))
                .selectable(true)
                .text_size(text_size)
                .line_height(line_height)
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
        frame: true,
    }
    .into_any_element()
}

/// Tool groups grow naturally until they reach half the window or the group cap.
pub(crate) fn group_scroll(id: String, content: impl IntoElement) -> AnyElement {
    ActivityScroll {
        id: id.into(),
        content: content.into_any_element(),
        group: true,
        frame: true,
    }
    .into_any_element()
}

/// List rows already provide vertical spacing; popovers supply their outer frame.
pub(crate) fn list_scroll(id: String, content: impl IntoElement) -> AnyElement {
    ActivityScroll {
        id: id.into(),
        content: content.into_any_element(),
        group: true,
        frame: false,
    }
    .into_any_element()
}

#[derive(IntoElement)]
struct ActivityScroll {
    id: SharedString,
    content: AnyElement,
    group: bool,
    frame: bool,
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
        div()
            .w_full()
            .min_w_0()
            .when(self.frame, |body| body.py_2())
            .child(
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
                            .when(self.group && self.frame, |viewport| viewport.pr_3())
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

/// Literal diagnostics stay readable; the activity icon carries failure emphasis.
pub(crate) fn notice(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .px_3()
        .py_2()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
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
    format!("{fence}\n{text}{separator}{fence}")
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
        assert_eq!(literal("line\n"), "```\nline\n```");
        assert_eq!(literal("line"), "```\nline\n```");
        assert_eq!(literal("line\n\n"), "```\nline\n\n```");
        assert_eq!(literal("```\n"), "````\n```\n````");
    }

    #[gpui::test]
    fn diagnostic_text_uses_the_body_tone(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                Theme::change(mode, None, cx);
                let mut diagnostic = notice("No such file or directory (os error 2)", cx);
                assert_eq!(
                    diagnostic.style().text.color,
                    Some(cx.theme().muted_foreground)
                );
            }
        });
    }
}
