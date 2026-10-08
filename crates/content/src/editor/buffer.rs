//! Document editing shares the caller's existing Kit buffer and undo history.
use super::{Command, Copy, Cut, Paste, Redo, SourceEditor, Undo};
#[cfg(test)]
use gpui_kit::component::input::Textarea;
use gpui_kit::component::input::{EditorState, TextareaState};
use gpui_kit::{
    AnyElement, App, Entity, FocusHandle, Focusable, IntoElement, SharedString, Window, prelude::*,
};
use std::ops::Range;

#[derive(Clone)]
pub(super) enum Buffer {
    Editor(Entity<EditorState>),
    #[cfg(test)]
    Textarea(Entity<TextareaState>),
}

macro_rules! read {
    ($buffer:expr, $cx:expr, |$input:ident| $body:expr) => {
        match $buffer {
            Buffer::Editor(input) => {
                let $input = input.read($cx);
                $body
            }
            #[cfg(test)]
            Buffer::Textarea(input) => {
                let $input = input.read($cx);
                $body
            }
        }
    };
}
macro_rules! update {
    ($buffer:expr, $cx:expr, |$input:ident, $context:ident| $body:expr) => {
        match $buffer {
            Buffer::Editor(input) => input.update($cx, |$input, $context| $body),
            #[cfg(test)]
            Buffer::Textarea(input) => input.update($cx, |$input, $context| $body),
        }
    };
}

impl Buffer {
    pub(super) fn textarea(&self) -> Option<&Entity<TextareaState>> {
        match self {
            #[cfg(test)]
            Self::Textarea(input) => Some(input),
            Self::Editor(_) => None,
        }
    }

    pub(super) fn value(&self, cx: &App) -> SharedString {
        read!(self, cx, |input| input.value())
    }

    pub(super) fn selection(&self, cx: &App) -> Range<usize> {
        read!(self, cx, |input| input.selected_range())
    }

    pub(super) fn select(&self, range: Range<usize>, cx: &mut App) {
        update!(self, cx, |input, cx| input.set_selected_range(range, cx));
    }

    pub(super) fn replace(
        &self,
        range: Range<usize>,
        text: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let text = text.into();
        update!(self, cx, |input, cx| {
            input.set_selected_range(range, cx);
            input.replace(text, window, cx);
        });
    }

    pub(super) fn begin(&self, cx: &mut App) {
        update!(self, cx, |input, _cx| input.begin_undo_transaction());
    }

    pub(super) fn commit(&self, cx: &mut App) {
        update!(self, cx, |input, _cx| input.commit_undo_transaction());
    }

    pub(super) fn undo(&self, window: &mut Window, cx: &mut App) {
        update!(self, cx, |input, cx| input.undo(&Undo, window, cx));
    }

    pub(super) fn redo(&self, window: &mut Window, cx: &mut App) {
        update!(self, cx, |input, cx| input.redo(&Redo, window, cx));
    }

    pub(super) fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            Self::Editor(input) => input.focus_handle(cx),
            #[cfg(test)]
            Self::Textarea(input) => input.focus_handle(cx),
        }
    }

    pub(super) fn find(&self, cx: &mut App) {
        match self {
            Self::Editor(input) => input.update(cx, |input, cx| input.open_search(false, cx)),
            #[cfg(test)]
            Self::Textarea(_) => {}
        }
    }

    pub(super) fn command(&self, command: Command, window: &mut Window, cx: &mut App) {
        update!(self, cx, |input, cx| match command {
            Command::Copy => input.copy(&Copy, window, cx),
            Command::Cut => input.cut(&Cut, window, cx),
            Command::Paste => input.paste(&Paste, window, cx),
            _ => {}
        });
    }

    pub(super) fn render(&self, readonly: bool, label: Option<SharedString>) -> AnyElement {
        match self {
            Self::Editor(input) => SourceEditor::new(input)
                .appearance(false)
                .bordered(false)
                .readonly(readonly)
                .h_full()
                .when_some(label, |input, label| input.aria_label(label))
                .into_any_element(),
            #[cfg(test)]
            Self::Textarea(input) => Textarea::new(input)
                .appearance(false)
                .bordered(false)
                .readonly(readonly)
                .when_some(label, |input, label| input.aria_label(label))
                .into_any_element(),
        }
    }
}
