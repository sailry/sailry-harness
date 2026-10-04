//! Static approved-layout fixtures. Live repository UI is supplied by a package.
use super::{SideResource, tabs::DocumentTabs};
use crate::{
    preview::{FILES, Page},
    shell::Shell,
    tr,
    workspace::Repository,
};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        input::{Textarea, TextareaState},
        scroll::ScrollableElement,
        *,
    },
    prelude::FluentBuilder as _,
    *,
};
use std::{cell::RefCell, collections::BTreeMap};
pub(crate) struct PreviewGit {
    pub tabs: DocumentTabs,
    pub message: Entity<TextareaState>,
    pub list_width: Entity<Pixels>,
    diffs: RefCell<BTreeMap<usize, Entity<crate::content::diff::State>>>,
}
impl PreviewGit {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            tabs: DocumentTabs::new(true, window, cx),
            message: cx
                .new(|cx| TextareaState::new(window, cx).placeholder(tr("git_commit_message"))),
            list_width: cx.new(|_| px(300.)),
            diffs: RefCell::default(),
        }
    }
    pub fn tab_label(&self, index: usize) -> SharedString {
        self.tabs.label(index)
    }
    pub fn tab_path(&self, index: usize) -> Option<SharedString> {
        self.tabs.names.get(index).cloned()
    }
    pub fn diff_counts(&self, cx: &App) -> (usize, usize) {
        if self.tabs.open.is_empty() {
            return (0, 0);
        }
        crate::content::diff::unified(&preview_patch(
            self.tabs.editors[self.tabs.selected]
                .read(cx)
                .value()
                .as_ref(),
        ))
        .iter()
        .fold((0, 0), |(added, removed), line| {
            (
                added + usize::from(line.kind == crate::content::diff::Kind::Added),
                removed + usize::from(line.kind == crate::content::diff::Kind::Removed),
            )
        })
    }

    fn diff(&self, file: usize, cx: &mut Context<Shell>) -> Entity<crate::content::diff::State> {
        let patch = preview_patch(self.tabs.editors[file].read(cx).value().as_ref());
        let lines = crate::content::diff::unified(&patch);
        // Keyed element state is scoped to its render stack. This tab owner
        // retains the exact native entity shared by rendering and focus actions.
        let state = self
            .diffs
            .borrow_mut()
            .entry(file)
            .or_insert_with(|| {
                let state = cx.new(|cx| {
                    crate::content::diff::State::new(
                        &lines,
                        crate::content::diff::language(FILES[file]),
                        cx,
                    )
                });
                cx.observe(&state, |_, _, cx| cx.notify()).detach();
                state
            })
            .clone();
        state.update(cx, |state, cx| state.set_lines(&lines, cx));
        state
    }
}
impl Shell {
    pub(crate) fn close_git_tab(
        &mut self,
        embedded: bool,
        file: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active = self.git_state(embedded).tabs.selected == file;
        self.git_state_mut(embedded).tabs.close(file);
        if active {
            self.focus_preview_git(embedded, window, cx);
        }
        cx.notify();
    }

    pub(super) fn focus_preview_git(
        &self,
        embedded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tabs = &self.git_state(embedded).tabs;
        if tabs.open.is_empty() || self.repository() != Repository::Ready {
            return;
        }
        self.git_state(embedded)
            .diff(tabs.selected, cx)
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
    }
    pub(crate) fn git_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        let repository = self.repository();
        let owner = self.workspace.selected_owner(self.host);
        v_flex()
            .debug_selector(|| "git-empty".into())
            .size_full()
            .p_4()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .max_w(px(360.))
                    .items_center()
                    .gap_3()
                    .child(Icon::new(IconName::Network).size_8())
                    .child(div().text_center().font_semibold().child(tr(
                        if repository == Repository::Directory {
                            "git_directory_title"
                        } else {
                            "git_unborn_title"
                        },
                    )))
                    .when_some(
                        owner.filter(|_| repository == Repository::Directory),
                        |body, owner| {
                            body.child(
                                Button::new("git-initialize")
                                    .debug_selector(|| "git-initialize".into())
                                    .primary()
                                    .icon(IconName::Plus)
                                    .label(tr("git_initialize"))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.initialize_git(owner.project, window, cx)
                                    })),
                            )
                        },
                    ),
            )
            .into_any_element()
    }
}
impl Shell {
    pub(crate) fn git_state(&self, embedded: bool) -> &PreviewGit {
        if embedded {
            match &self.side_resource {
                Some(SideResource::PreviewGit(state)) => state,
                _ => unreachable!("git panel must be mounted"),
            }
        } else {
            &self.git
        }
    }

    pub(crate) fn git_state_mut(&mut self, embedded: bool) -> &mut PreviewGit {
        if embedded {
            match &mut self.side_resource {
                Some(SideResource::PreviewGit(state)) => state,
                _ => unreachable!("git panel must be mounted"),
            }
        } else {
            &mut self.git
        }
    }

    pub(crate) fn git_diff(&self, embedded: bool, cx: &mut Context<Self>) -> AnyElement {
        let tabs = &self.git_state(embedded).tabs;
        let editor = if self.repository() != Repository::Ready {
            self.git_empty(cx)
        } else if tabs.open.is_empty() {
            crate::empty_state::panel(IconName::Network, "git_diff_empty", cx).into_any_element()
        } else {
            let file = tabs.selected;
            crate::content::diff::surface(FILES[file], &self.git_state(embedded).diff(file, cx))
        };
        v_flex()
            .debug_selector(|| "git-diff".into())
            .size_full()
            .min_h_0()
            .when(embedded, |view| {
                view.child(self.document_tabs(Page::Git, true, cx))
            })
            .child(
                div()
                    .debug_selector(|| "document-editor".into())
                    .flex_1()
                    .min_h_0()
                    .child(editor),
            )
            .children(self.document_path_footer(Page::Git, embedded, cx))
            .into_any_element()
    }

    pub(crate) fn git_changes(
        &self,
        embedded: bool,
        actions: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let repository = self.repository();
        v_flex()
            .debug_selector(|| "git-changes".into())
            .size_full()
            .min_h_0()
            .child(
                crate::header::Header::new("git-changes-header", cx)
                    .bordered(!embedded)
                    .child(div().flex_1().child(tr("changes")))
                    .child(
                        Button::new("git-stage-all")
                            .debug_selector(|| "git-stage-all".into())
                            .ghost()
                            .icon(IconName::Plus)
                            .disabled(true)
                            .accessibility_label(tr("git_stage_all"))
                            .tooltip(tr("git_actions_preview")),
                    )
                    .children(actions),
            )
            .child(
                v_flex()
                    .id("git-change-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_2()
                    .gap_1()
                    .when(repository != Repository::Ready, |body| {
                        body.child(
                            div()
                                .p_2()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr(if repository == Repository::Directory {
                                    "git_directory_hint"
                                } else {
                                    "git_unborn_changes"
                                })),
                        )
                    })
                    .children(
                        FILES
                            .into_iter()
                            .enumerate()
                            .filter(|_| repository == Repository::Ready)
                            .map(|(file, name)| {
                                super::rows::row(
                                    ("change", file),
                                    self.git_state(embedded).tabs.selected == file
                                        && !self.git_state(embedded).tabs.open.is_empty(),
                                    cx,
                                )
                                .h_7()
                                .px_2()
                                .debug_selector(move || format!("resource-change-{file}"))
                                .child(
                                    h_flex()
                                        .w_full()
                                        .gap_2()
                                        .child(IconName::File)
                                        .child(div().flex_1().min_w_0().truncate().child(name))
                                        .child(div().text_color(cx.theme().warning).child("M")),
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.git_state_mut(embedded).tabs.open(file);
                                        cx.notify();
                                    },
                                ))
                            }),
                    ),
            )
            .when(repository != Repository::Directory, |body| {
                body.child(
                    v_flex()
                        .debug_selector(|| "git-commit-controls".into())
                        .flex_shrink_0()
                        .p_3()
                        .gap_2()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(
                            Textarea::new(&self.git_state(embedded).message)
                                .aria_label(tr("git_commit_message")),
                        )
                        .child(
                            Button::new("git-commit")
                                .primary()
                                .w_full()
                                .icon(IconName::Check)
                                .label(tr("git_commit"))
                                .disabled(true)
                                .tooltip(tr("git_actions_preview")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr("git_preview_only")),
                        ),
                )
            })
            .into_any_element()
    }

    pub(crate) fn repository(&self) -> Repository {
        self.workspace
            .selected_owner(self.host)
            .map(|owner| self.workspace.projects[&owner.project].repository)
            .unwrap_or(Repository::Directory)
    }
}

/// Preview fixtures contain changed lines without a file or hunk header.
/// Supply their coordinates here so the shared real-patch parser stays strict.
fn preview_patch(text: &str) -> String {
    let old = text.lines().filter(|line| !line.starts_with('+')).count();
    let new = text.lines().filter(|line| !line.starts_with('-')).count();
    let mut patch = format!("@@ -1,{old} +1,{new} @@\n");
    for line in text.lines() {
        if !line.starts_with(['+', '-']) {
            patch.push(' ');
        }
        patch.push_str(line);
        patch.push('\n');
    }
    patch
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn preview_diff_coordinates() {
        use crate::content::diff::{Kind, unified};
        for key in crate::preview::FILE_DIFF_KEYS {
            let text = tr(key)
                .lines()
                .filter(|line| !line.starts_with("```"))
                .collect::<Vec<_>>()
                .join("\n");
            let patch = preview_patch(&text);
            let lines = unified(&patch);
            assert_eq!(
                lines
                    .iter()
                    .filter(|line| line.kind == Kind::Removed)
                    .count(),
                2
            );
            assert_eq!(
                lines.iter().filter(|line| line.kind == Kind::Added).count(),
                2
            );
            assert_eq!(
                lines
                    .iter()
                    .filter(|line| line.kind == Kind::Context)
                    .count(),
                1
            );
            assert!(!lines.iter().any(|line| line.kind == Kind::Meta));
            assert_eq!((lines[3].old, lines[3].new), (Some(2), Some(2)));
            assert_eq!((lines[5].old, lines[5].new), (None, Some(3)));
            assert!(unified(&text).iter().all(|line| line.kind == Kind::Meta));
        }
    }
}
