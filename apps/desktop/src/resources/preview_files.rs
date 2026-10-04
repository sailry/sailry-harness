//! Static Files preview composition. It never binds a Node or creates file operations.
mod toolbar;

use super::{SideResource, tabs::DocumentTabs};
use crate::{
    preview::{FILES, Page},
    shell::Shell,
    tr,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    component::{
        input::Editor,
        scroll::ScrollableElement,
        tree::{TreeItem, TreeState},
        *,
    },
    *,
};
use std::{cell::RefCell, collections::BTreeMap};

pub(crate) struct PreviewFiles {
    pub tabs: DocumentTabs,
    pub(crate) tree: Entity<TreeState>,
    pub(crate) list_width: Entity<Pixels>,
    markdown: RefCell<BTreeMap<EntityId, Entity<crate::content::editor::State>>>,
}

impl PreviewFiles {
    pub(crate) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            tabs: DocumentTabs::new(false, window, cx),
            tree: cx.new(|cx| {
                TreeState::new(cx).items(vec![
                    TreeItem::new("project", tr("project"))
                        .expanded(true)
                        .children([
                            TreeItem::new("0", FILES[0]),
                            TreeItem::new("docs", "docs")
                                .expanded(true)
                                .children([TreeItem::new("1", "example.md")]),
                        ]),
                ])
            }),
            list_width: cx.new(|_| px(220.)),
            markdown: RefCell::default(),
        }
    }

    pub(super) fn markdown(
        &self,
        input: EntityId,
    ) -> Option<Entity<crate::content::editor::State>> {
        self.markdown.borrow().get(&input).cloned()
    }
}

impl Shell {
    pub(crate) fn file_state(&self, embedded: bool) -> &PreviewFiles {
        if embedded {
            let Some(SideResource::PreviewFiles(state)) = &self.side_resource else {
                unreachable!("static file preview must be mounted")
            };
            state
        } else {
            &self.files
        }
    }

    pub(crate) fn file_state_mut(&mut self, embedded: bool) -> &mut PreviewFiles {
        if embedded {
            let Some(SideResource::PreviewFiles(state)) = &mut self.side_resource else {
                unreachable!("static file preview must be mounted")
            };
            state
        } else {
            &mut self.files
        }
    }

    pub(crate) fn close_file_tab(
        &mut self,
        embedded: bool,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active = self.file_state(embedded).tabs.selected == index;
        self.file_state_mut(embedded).tabs.close(index);
        if active {
            self.focus_preview_file(embedded, window, cx);
        }
        cx.notify();
    }

    pub(super) fn focus_preview_file(
        &self,
        embedded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = self.file_state(embedded);
        let tabs = &state.tabs;
        if tabs.open.is_empty() {
            return;
        }
        let input = &tabs.editors[tabs.selected];
        if matches!(
            std::path::Path::new(tabs.names[tabs.selected].as_ref())
                .extension()
                .and_then(|value| value.to_str()),
            Some("md" | "markdown" | "mdown")
        ) {
            let mut markdown = state.markdown.borrow_mut();
            let editor = markdown.entry(input.entity_id()).or_insert_with(|| {
                let editor = cx.new(|cx| crate::content::editor::State::from_input(input, cx));
                cx.observe(&editor, |_, _, cx| cx.notify()).detach();
                editor
            });
            editor.read(cx).focus_handle(cx).focus(window, cx);
        } else {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    pub(crate) fn file_preview(&self, embedded: bool, cx: &mut Context<Self>) -> AnyElement {
        let state = self.file_state(embedded);
        let tabs = &state.tabs;
        let mut markdown = state.markdown.borrow_mut();
        markdown.retain(|id, _| tabs.editors.iter().any(|input| input.entity_id() == *id));
        let editor = if tabs.open.is_empty() {
            crate::empty_state::panel(IconName::File, "file_empty", cx).into_any_element()
        } else {
            let input = &tabs.editors[tabs.selected];
            if matches!(
                std::path::Path::new(tabs.names[tabs.selected].as_ref())
                    .extension()
                    .and_then(|value| value.to_str()),
                Some("md" | "markdown" | "mdown")
            ) {
                let state = markdown.entry(input.entity_id()).or_insert_with(|| {
                    let state = cx.new(|cx| crate::content::editor::State::from_input(input, cx));
                    cx.observe(&state, |_, _, cx| cx.notify()).detach();
                    state
                });
                crate::content::editor::View::new(state)
                    .readonly(true)
                    .aria_label(tabs.names[tabs.selected].clone())
                    .into_any_element()
            } else {
                Editor::new(input)
                    .readonly(true)
                    .appearance(false)
                    .bordered(false)
                    .text_sm()
                    .px_0()
                    .h_full()
                    .aria_label(tabs.names[tabs.selected].clone())
                    .into_any_element()
            }
        };
        drop(markdown);
        v_flex()
            .debug_selector(|| "file-preview".into())
            .size_full()
            .min_h_0()
            .when(embedded, |view| {
                view.child(self.document_tabs(Page::Files, true, cx))
            })
            .when(!tabs.open.is_empty(), |view| {
                view.child(self.preview_file_toolbar(embedded, cx))
            })
            .child(
                div()
                    .debug_selector(|| "document-editor".into())
                    .flex_1()
                    .min_h_0()
                    .child(editor),
            )
            .children(self.document_path_footer(Page::Files, embedded, cx))
            .into_any_element()
    }

    pub(crate) fn file_explorer(
        &self,
        embedded: bool,
        actions: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        let tree = self.file_state(embedded).tree.clone();
        v_flex()
            .debug_selector(|| "file-explorer".into())
            .size_full()
            .overflow_hidden()
            .child(
                crate::header::Header::new("file-explorer-header", cx)
                    .bordered(false)
                    .child(div().flex_1().min_w_0().truncate().child(tr("file_tree")))
                    .children(actions),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p_2()
                    .child(
                        gpui_kit::base::Tree::new(&tree)
                            .item(move |index, entry, state, _, cx| {
                                let file = entry.item().id.parse::<usize>().ok();
                                let label = entry.item().label.clone();
                                let selector = format!("resource-file-{}", entry.item().id);
                                let owner = owner.clone();
                                super::rows::row(index, state.is_selected(), cx)
                                    .debug_selector(move || selector.clone())
                                    .h_7()
                                    .px_2()
                                    .pl(px(16.) * entry.depth() + px(8.))
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .min_w_0()
                                            .gap_2()
                                            .child(
                                                Icon::new(if entry.is_folder() {
                                                    IconName::Folder
                                                } else {
                                                    IconName::File
                                                })
                                                .size_4()
                                                .flex_shrink_0()
                                                .text_color(cx.theme().muted_foreground),
                                            )
                                            .child(
                                                div().flex_1().min_w_0().truncate().child(label),
                                            ),
                                    )
                                    .on_click(move |_, _, cx| {
                                        if let Some(file) = file {
                                            let _ = owner.update(cx, |shell, cx| {
                                                shell.file_state_mut(embedded).tabs.open(file);
                                                cx.notify();
                                            });
                                        }
                                    })
                                    .into_any_element()
                            })
                            .list_style(StyleRefinement::default().flex_grow_1().size_full())
                            .size_full(),
                    )
                    .vertical_scrollbar(tree.read(cx).scroll_handle()),
            )
            .into_any_element()
    }
}
