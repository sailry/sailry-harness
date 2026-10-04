//! Document status and native path actions share the originating resource identity.
use crate::{preview::Page, shell::Shell, tr};
use gpui_kit::{
    component::{native_menu::NativeMenu, status_bar::StatusBar, *},
    prelude::FluentBuilder as _,
    *,
};
use sailry_protocol::{NodeId, WorktreeId};

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry_documents, no_json)]
pub(crate) struct PathAction {
    git: bool,
    embedded: bool,
    scope: Option<(NodeId, WorktreeId)>,
    editor: EntityId,
    path: String,
    open: bool,
}

impl Shell {
    pub(super) fn document_path_footer(
        &self,
        page: Page,
        embedded: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let git = page == Page::Git;
        let (tabs, scope) = if git {
            let state = self.git_state(embedded);
            (&state.tabs, None)
        } else {
            let state = self.file_state(embedded);
            (&state.tabs, None)
        };
        let selected = (!tabs.open.is_empty()).then_some(tabs.selected);
        let path = selected.and_then(|index| {
            if git {
                self.git_state(embedded).tab_path(index)
            } else {
                Some(tabs.names[index].clone())
            }
        });
        let target = selected.zip(path.as_ref()).map(|(index, path)| PathAction {
            git,
            embedded,
            scope,
            editor: tabs.editors[index].entity_id(),
            path: path.to_string(),
            open: false,
        });
        let label = path.unwrap_or_else(|| {
            selected
                .map(|index| self.git_state(embedded).tab_label(index))
                .unwrap_or_default()
        });
        let mut bar = StatusBar::new()
            .h_8()
            .flex_shrink_0()
            .border_t_0()
            .bg(cx.theme().transparent)
            .left(
                div()
                    .id("document-path-label")
                    .debug_selector(|| "document-path-label".into())
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(label)
                    .when_some(target, |label, target| {
                        label.on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |shell, event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                shell.focus.focus(window, cx);
                                if !cfg!(any(target_os = "macos", target_os = "windows")) {
                                    crate::feedback::error(
                                        &tr("workspace_native_menu_unavailable"),
                                        &tr("workspace_native_menu_platform"),
                                        window,
                                        cx,
                                    );
                                    return;
                                }
                                NativeMenu::new()
                                    .menu_with_disabled(
                                        tr("files_open_system"),
                                        target.scope.is_none(),
                                        Box::new(PathAction {
                                            open: true,
                                            ..target.clone()
                                        }),
                                    )
                                    .menu(tr("files_copy_path"), Box::new(target.clone()))
                                    .show(event.position, window, cx);
                            }),
                        )
                    }),
            );
        if git {
            let (added, removed) = self.git_state(embedded).diff_counts(cx);
            bar = bar.child(
                div()
                    .debug_selector(|| "document-diff-count".into())
                    .flex_shrink_0()
                    .text_color(cx.theme().danger)
                    .child(format!("+{added} -{removed}")),
            );
        } else if let Some(index) = selected {
            let input = tabs.editors[index].read(cx);
            let position = self
                .file_state(embedded)
                .markdown(tabs.editors[index].entity_id())
                .and_then(|state| state.read(cx).source_cursor_position(cx))
                .unwrap_or_else(|| input.cursor_position());
            bar = bar
                .right(
                    div()
                        .debug_selector(|| "document-cursor-position".into())
                        .child(
                            tr("files_cursor_position")
                                .replace("{line}", &(position.line + 1).to_string())
                                .replace("{column}", &(position.character + 1).to_string()),
                        ),
                )
                .right(
                    div()
                        .debug_selector(|| "document-encoding".into())
                        .child("UTF-8"),
                )
                .right(
                    div()
                        .debug_selector(|| "document-language".into())
                        .child(input.language_name()),
                );
        }
        Some(
            div()
                .debug_selector(|| "document-path".into())
                .flex_shrink_0()
                .min_w_0()
                .child(bar)
                .into_any_element(),
        )
    }

    pub(crate) fn document_path_action(
        &mut self,
        action: &PathAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action.embedded
            && !matches!(
                (&self.side_resource, action.git),
                (Some(super::SideResource::PreviewGit(_)), true)
                    | (Some(super::SideResource::PreviewFiles(_)), false)
            )
        {
            return;
        }
        let (tabs, scope) = if action.git {
            let state = self.git_state(action.embedded);
            (&state.tabs, None)
        } else {
            let state = self.file_state(action.embedded);
            (&state.tabs, None)
        };
        if scope != action.scope
            || !tabs
                .open
                .iter()
                .any(|index| tabs.editors[*index].entity_id() == action.editor)
        {
            return;
        }
        if action.open {
            if let Some(scope) = action.scope {
                self.open_file_in_system(scope, action.path.clone(), false, window, cx);
            }
        } else {
            cx.write_to_clipboard(ClipboardItem::new_string(action.path.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::gpui;

    #[gpui::test]
    fn path_actions_keep_the_document_target(cx: &mut TestAppContext) {
        let (shell, mut visual) = crate::shell::tests::setup(cx);
        for git in [false, true] {
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    let page = if git { Page::Git } else { Page::Files };
                    shell.open_resource_panel(page, window, cx);
                    let tabs = if git {
                        &shell.git_state(true).tabs
                    } else {
                        &shell.file_state(true).tabs
                    };
                    let action = PathAction {
                        git,
                        embedded: true,
                        scope: None,
                        editor: tabs.editors[0].entity_id(),
                        path: tabs.names[0].to_string(),
                        open: false,
                    };
                    let tabs = if git {
                        &mut shell.git_state_mut(true).tabs
                    } else {
                        &mut shell.file_state_mut(true).tabs
                    };
                    tabs.open(1);
                    shell.document_path_action(&action, window, cx);
                    assert_eq!(
                        cx.read_from_clipboard().unwrap().text().unwrap(),
                        action.path
                    );
                    let tabs = if git {
                        &mut shell.git_state_mut(true).tabs
                    } else {
                        &mut shell.file_state_mut(true).tabs
                    };
                    tabs.close(0);
                    cx.write_to_clipboard(ClipboardItem::new_string("unchanged".into()));
                    shell.document_path_action(&action, window, cx);
                    assert_eq!(
                        cx.read_from_clipboard().unwrap().text().unwrap(),
                        "unchanged"
                    );
                    shell.close_resource_panel(cx);
                    shell.document_path_action(&action, window, cx);
                    assert_eq!(
                        cx.read_from_clipboard().unwrap().text().unwrap(),
                        "unchanged"
                    );
                });
            });
        }
    }
}
