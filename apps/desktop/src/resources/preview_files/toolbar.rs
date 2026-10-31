//! Editing actions use the Kit editor's focus and undo history; clipboard
//! actions stay on the editor's own shortcuts and context menu.
use crate::content::editor::{Command, Mode};
use crate::{shell::Shell, tr};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        input,
        tab::{Tab, TabBar},
        tooltip::Tooltip,
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
        let actions: [(IconName, &str, Command, Box<dyn Action>); 2] = [
            (
                IconName::Undo,
                "files_undo",
                Command::Undo,
                Box::new(input::Undo),
            ),
            (
                IconName::Redo,
                "files_redo",
                Command::Redo,
                Box::new(input::Redo),
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
                            .disabled(readonly)
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
                        .children(
                            [
                                (
                                    "icons/eye.svg",
                                    "content_document",
                                    "markdown-document-mode",
                                ),
                                (
                                    "icons/code-xml.svg",
                                    "content_source",
                                    "markdown-source-mode",
                                ),
                            ]
                            .map(|(icon, key, selector)| {
                                Tab::new()
                                    .icon(Icon::empty().path(icon))
                                    .aria_label(tr(key))
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(tr(key)).build(window, cx)
                                    })
                                    .debug_selector(move || selector.into())
                            }),
                        )
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
