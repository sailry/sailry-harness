//! Editing actions use the Kit editor's focus and undo history.
use crate::content::editor::{Command, Mode};
use crate::{shell::Shell, tr};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        input,
        tab::{Tab, TabBar},
        *,
    },
    *,
};

impl Shell {
    pub(super) fn preview_file_toolbar(
        &self,
        embedded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = self.file_state(embedded);
        let editor = state.tabs.editors[state.tabs.selected].clone();
        let markdown = state.markdown(editor.entity_id());
        let readonly = true;
        let actions: [(Icon, &str, Command, Box<dyn Action>); 5] = [
            (
                Icon::new(IconName::Undo),
                "files_undo",
                Command::Undo,
                Box::new(input::Undo),
            ),
            (
                Icon::new(IconName::Redo),
                "files_redo",
                Command::Redo,
                Box::new(input::Redo),
            ),
            (
                Icon::new(IconName::Copy),
                "files_copy",
                Command::Copy,
                Box::new(input::Copy),
            ),
            (
                Icon::empty().path("icons/cut.svg"),
                "files_cut",
                Command::Cut,
                Box::new(input::Cut),
            ),
            (
                Icon::empty().path("icons/paste.svg"),
                "files_paste",
                Command::Paste,
                Box::new(input::Paste),
            ),
        ];
        let mode = markdown.clone();
        h_flex()
            .debug_selector(|| "file-toolbar".into())
            .w_full()
            .h_12()
            .px_3()
            .gap_1()
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("save-file")
                    .debug_selector(|| "save-file".into())
                    .ghost()
                    .small()
                    .icon(IconName::CircleCheck)
                    .accessibility_label(tr("files_save"))
                    .text_color(cx.theme().muted_foreground)
                    .tooltip(tr("files_save"))
                    .disabled(true),
            )
            .child(crate::header::separator(cx))
            .child(
                h_flex()
                    .id("file-edit-actions")
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .overflow_x_scroll()
                    .children(actions.into_iter().map(|(icon, key, command, action)| {
                        let editor = editor.clone();
                        let markdown = markdown.clone();
                        Button::new(key)
                            .debug_selector(move || key.into())
                            .ghost()
                            .small()
                            .icon(icon)
                            .tooltip(tr(key))
                            .accessibility_label(tr(key))
                            .disabled(if key == "files_copy" {
                                !markdown.as_ref().map_or_else(
                                    || editor.read(cx).is_copyable(),
                                    |state| state.read(cx).can_copy(cx),
                                )
                            } else {
                                readonly
                                    || (matches!(command, Command::Cut | Command::Paste)
                                        && markdown
                                            .as_ref()
                                            .is_some_and(|state| !state.read(cx).can_cut_paste()))
                            })
                            .on_click(move |_, window, cx| {
                                if let Some(markdown) = &markdown {
                                    markdown
                                        .update(cx, |state, cx| state.command(command, window, cx));
                                } else {
                                    editor.update(cx, |editor, cx| editor.focus(window, cx));
                                    window.dispatch_action(action.boxed_clone(), cx);
                                }
                            })
                    }))
                    .child(crate::header::separator(cx))
                    .child(
                        Button::new("file-find")
                            .debug_selector(|| "file-find".into())
                            .ghost()
                            .small()
                            .icon(IconName::Search)
                            .tooltip(tr("files_find"))
                            .accessibility_label(tr("files_find"))
                            .on_click(move |_, window, cx| {
                                if let Some(markdown) = &markdown {
                                    markdown.update(cx, |state, cx| {
                                        state.command(Command::Find, window, cx)
                                    });
                                } else {
                                    editor.update(cx, |editor, cx| {
                                        editor.focus(window, cx);
                                        editor.open_search(false, cx);
                                    });
                                }
                            }),
                    ),
            )
            .when_some(mode, |bar, state| {
                let selected = usize::from(state.read(cx).mode() == Mode::Source);
                bar.child(
                    TabBar::new("file-editor-mode")
                        .flex_shrink_0()
                        .segmented()
                        .small()
                        .selected_index(selected)
                        .children([
                            Tab::new()
                                .aria_label(tr("content_document"))
                                .child(div().text_size(px(14.)).child(tr("content_document")))
                                .debug_selector(|| "markdown-document-mode".into()),
                            Tab::new()
                                .aria_label(tr("content_source"))
                                .child(div().text_size(px(14.)).child(tr("content_source")))
                                .debug_selector(|| "markdown-source-mode".into()),
                        ])
                        .on_click(move |index, window, cx| {
                            state.update(cx, |state, cx| {
                                state.set_mode(
                                    if *index == 0 {
                                        Mode::Document
                                    } else {
                                        Mode::Source
                                    },
                                    window,
                                    cx,
                                )
                            });
                        }),
                )
            })
            .into_any_element()
    }
}
