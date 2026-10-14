//! Document editing shares the caller's existing Kit buffer and undo history.
use super::{Command, Copy, Cut, Paste, Redo, SourceEditor, Undo};
use gpui_kit::base::RoleOverride;
use gpui_kit::component::input::EditorState;
use gpui_kit::{
    Action, AnyElement, App, Entity, FocusHandle, Focusable, IntoElement, SharedString,
    TestSupportExt as _, Window, div, prelude::*, px,
};
use std::ops::Range;

#[derive(Clone)]
pub(super) enum Buffer {
    Editor(Entity<EditorState>),
}

impl Buffer {
    pub(super) fn value(&self, cx: &App) -> SharedString {
        let Self::Editor(input) = self;
        input.read(cx).value()
    }

    pub(super) fn selection(&self, cx: &App) -> Range<usize> {
        let Self::Editor(input) = self;
        input.read(cx).selected_range()
    }

    pub(super) fn select(&self, range: Range<usize>, cx: &mut App) {
        let Self::Editor(input) = self;
        input.update(cx, |input, cx| input.set_selected_range(range, cx));
    }

    pub(super) fn replace(
        &self,
        range: Range<usize>,
        text: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Self::Editor(input) = self;
        let text = text.into();
        input.update(cx, |input, cx| {
            input.set_selected_range(range, cx);
            input.replace(text, window, cx);
        });
    }

    pub(super) fn undo(&self, window: &mut Window, cx: &mut App) {
        self.command(Command::Undo, window, cx);
    }

    pub(super) fn redo(&self, window: &mut Window, cx: &mut App) {
        self.command(Command::Redo, window, cx);
    }

    pub(super) fn focus_handle(&self, cx: &App) -> FocusHandle {
        let Self::Editor(input) = self;
        input.focus_handle(cx)
    }

    pub(super) fn find(&self, cx: &mut App) {
        let Self::Editor(input) = self;
        input.update(cx, |input, cx| input.open_search(false, cx));
    }

    pub(super) fn command(&self, command: Command, window: &mut Window, cx: &mut App) {
        let action: &dyn Action = match command {
            Command::Copy => &Copy,
            Command::Cut => &Cut,
            Command::Paste => &Paste,
            Command::Undo => &Undo,
            Command::Redo => &Redo,
            Command::Find => return,
        };
        self.focus_handle(cx).dispatch_action(action, window, cx);
    }

    pub(super) fn actions(&self) -> AnyElement {
        let Self::Editor(input) = self;
        // Stock input actions attach to rendered focus nodes. Keep the caller's
        // editor mounted without another buffer or changing the document's focus.
        div()
            .id("markdown-source-actions")
            .test_support()
            .absolute()
            .size_0()
            .overflow_hidden()
            .opacity(0.)
            .child(
                SourceEditor::new(input)
                    .appearance(false)
                    .bordered(false)
                    .tab_index(-1)
                    .role(RoleOverride::Presentational)
                    .h(px(0.))
                    .w_0(),
            )
            .into_any_element()
    }

    pub(super) fn render(&self, readonly: bool, label: Option<SharedString>) -> AnyElement {
        let Self::Editor(input) = self;
        SourceEditor::new(input)
            .appearance(false)
            .bordered(false)
            .readonly(readonly)
            .h_full()
            .when_some(label, |input, label| input.aria_label(label))
            .into_any_element()
    }
}
