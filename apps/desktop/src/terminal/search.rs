use super::selection::Position;
use super::*;
use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    scroll::ScrollbarHandle,
};

actions!(terminal_search, [Find, Next, Previous, Close]);

pub(super) struct Search {
    pub input: Entity<InputState>,
    pub open: bool,
    pub query: String,
    pub matches: Vec<(Position, Position)>,
    pub current: usize,
}

impl View {
    pub(super) fn find(&mut self, _: &Find, window: &mut Window, cx: &mut Context<Self>) {
        self.search.open = true;
        self.search
            .input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn search_changed(&mut self, event: &InputEvent, cx: &mut Context<Self>) {
        match event {
            InputEvent::Change => {
                self.search.query = self.search.input.read(cx).value().to_string();
                self.search.current = 0;
                self.refresh_search();
                self.reveal_match(cx);
            }
            InputEvent::PressEnter { shift, .. } => self.step_match(*shift, cx),
            _ => {}
        }
    }

    pub(super) fn refresh_search(&mut self) {
        self.search.matches = self
            .state
            .snapshot
            .as_ref()
            .map_or_else(Vec::new, |snapshot| {
                super::text::find(&snapshot.screen, &self.search.query)
            });
        self.search.current = self
            .search
            .current
            .min(self.search.matches.len().saturating_sub(1));
    }

    fn step_match(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let count = self.search.matches.len();
        if count > 0 {
            self.search.current = if backwards {
                (self.search.current + count - 1) % count
            } else {
                (self.search.current + 1) % count
            };
            self.reveal_match(cx);
        }
    }

    fn reveal_match(&mut self, cx: &mut Context<Self>) {
        if let Some(&(start, end)) = self.search.matches.get(self.search.current) {
            self.selection.clear();
            self.selection.anchor = Some(start);
            self.selection.end = Some(end);
            self.scroll
                .set_offset(point(px(0.), -self.metrics.cell.height * start.row as f32));
        }
        cx.notify();
    }

    pub(super) fn next_match(&mut self, _: &Next, _: &mut Window, cx: &mut Context<Self>) {
        self.step_match(false, cx);
    }
    pub(super) fn previous_match(&mut self, _: &Previous, _: &mut Window, cx: &mut Context<Self>) {
        self.step_match(true, cx);
    }
    pub(super) fn close_search(&mut self, _: &Close, window: &mut Window, cx: &mut Context<Self>) {
        self.search.open = false;
        self.focus(window, cx);
        cx.notify();
    }

    pub(super) fn search_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("terminal-search")
            .debug_selector(|| "terminal-search".into())
            .key_context("TerminalSearch")
            .w(px(420.))
            .max_w_full()
            .min_w_0()
            .occlude()
            .bg(cx.theme().popover)
            .rounded_md()
            .shadow_md()
            .gap_1()
            .px_2()
            .py_1()
            .border_1()
            .border_color(cx.theme().border)
            .on_action(cx.listener(Self::find))
            .on_action(cx.listener(Self::next_match))
            .on_action(cx.listener(Self::previous_match))
            .on_action(cx.listener(Self::close_search))
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.search.input)
                        .small()
                        .prefix(IconName::Search)
                        .cleanable(true),
                ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{}/{}",
                        if self.search.matches.is_empty() {
                            0
                        } else {
                            self.search.current + 1
                        },
                        self.search.matches.len()
                    )),
            )
            .child(
                Button::new("previous-match")
                    .small()
                    .ghost()
                    .icon(IconName::ChevronUp)
                    .tooltip(tr("terminal_match_previous"))
                    .accessibility_label(tr("terminal_match_previous"))
                    .disabled(self.search.matches.is_empty())
                    .on_click(cx.listener(|view, _, _, cx| view.step_match(true, cx))),
            )
            .child(
                Button::new("next-match")
                    .small()
                    .ghost()
                    .icon(IconName::ChevronDown)
                    .tooltip(tr("terminal_match_next"))
                    .accessibility_label(tr("terminal_match_next"))
                    .disabled(self.search.matches.is_empty())
                    .on_click(cx.listener(|view, _, _, cx| view.step_match(false, cx))),
            )
            .child(
                Button::new("close-search")
                    .small()
                    .ghost()
                    .icon(IconName::Close)
                    .tooltip(tr("close"))
                    .accessibility_label(tr("close"))
                    .on_click(
                        cx.listener(|view, _, window, cx| view.close_search(&Close, window, cx)),
                    ),
            )
    }
}
