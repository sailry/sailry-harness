use crate::{
    preview::{FILE_DIFF_KEYS, FILES, Page},
    shell::Shell,
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::EditorState,
    *,
};
use gpui_kit::*;

pub(crate) struct DocumentTabs {
    pub selected: usize,
    pub open: Vec<usize>,
    pub editors: Vec<Entity<EditorState>>,
    pub names: Vec<SharedString>,
    dynamic: bool,
    scroll: ScrollHandle,
}

impl DocumentTabs {
    pub(crate) fn label(&self, file: usize) -> SharedString {
        self.names[file]
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned()
            .into()
    }

    pub fn new(git: bool, window: &mut Window, cx: &mut App) -> Self {
        let editors = (0..FILES.len())
            .map(|file| {
                let value = if git {
                    tr(FILE_DIFF_KEYS[file])
                        .lines()
                        .filter(|line| !line.starts_with("```"))
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    tr(if file == 0 {
                        "file_intro"
                    } else {
                        "file_design"
                    })
                    .to_string()
                };
                cx.new(|cx| {
                    EditorState::new(window, cx)
                        .language(if git { "diff" } else { "markdown" })
                        .line_number(true)
                        .default_value(value)
                })
            })
            .collect();
        Self {
            selected: 0,
            open: vec![0],
            editors,
            names: FILES.iter().map(|name| SharedString::from(*name)).collect(),
            dynamic: false,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn open(&mut self, file: usize) {
        self.selected = file;
        if !self.open.contains(&file) {
            self.open.push(file);
        }
        self.scroll.scroll_to_item(
            self.open
                .iter()
                .position(|current| *current == file)
                .unwrap(),
        );
    }

    pub(super) fn close(&mut self, file: usize) {
        self.open.retain(|current| *current != file);
        if self.selected == file {
            self.selected = self.open.first().copied().unwrap_or(0);
        }
        if self.dynamic {
            self.names.remove(file);
            self.editors.remove(file);
            for index in &mut self.open {
                if *index > file {
                    *index -= 1;
                }
            }
            if self.selected > file {
                self.selected -= 1;
            }
        }
    }
}

impl Shell {
    pub(crate) fn document_tabs(
        &self,
        page: Page,
        embedded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tabs = if page == Page::Git {
            &self.git_state(embedded).tabs
        } else {
            &self.file_state(embedded).tabs
        };
        let selected = tabs
            .open
            .iter()
            .position(|file| *file == tabs.selected)
            .unwrap_or(0);
        let show_files = page != Page::Git
            || self.live.is_some()
            || self.repository() == crate::workspace::Repository::Ready;
        let items = tabs
            .open
            .iter()
            .filter(|_| show_files)
            .enumerate()
            .map(|(index, &file)| {
                let dirty = false;
                let label = if page == Page::Git {
                    self.git_state(embedded).tab_label(file)
                } else {
                    tabs.label(file)
                };
                let close = Button::new(("close-document", file))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .debug_selector(move || format!("close-document-{file}"))
                    .tooltip(tr("close"))
                    .accessibility_label(format!("{} {}", tr("close"), tabs.names[file]))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        if page == Page::Files {
                            this.close_file_tab(embedded, file, window, cx);
                        } else {
                            this.close_git_tab(embedded, file, window, cx);
                        }
                    }));
                let tab = crate::navigation_tabs::item(
                    format!("document-tab-{file}"),
                    label.clone(),
                    index == selected,
                    dirty.then(|| {
                        div()
                            .size_2()
                            .rounded_full()
                            .bg(cx.theme().success)
                            .debug_selector(move || format!("document-dirty-{file}"))
                            .into_any_element()
                    }),
                    close,
                    cx,
                )
                .max_w(px(180.))
                .gap_1p5()
                .set_position(index + 1, tabs.open.len())
                .on_click(cx.listener(move |this, _, window, cx| {
                    let tabs = if page == Page::Git {
                        &mut this.git_state_mut(embedded).tabs
                    } else {
                        &mut this.file_state_mut(embedded).tabs
                    };
                    tabs.selected = file;
                    if page == Page::Files {
                        this.focus_preview_file(embedded, window, cx);
                    } else {
                        this.focus_preview_git(embedded, window, cx);
                    }
                    cx.notify();
                }));
                (label, tab.into_any_element())
            })
            .collect();
        let strip = crate::navigation_tabs::strip(
            if page == Page::Git {
                "git-document-tabs"
            } else {
                "file-document-tabs"
            },
            &tabs.scroll,
            items,
            Some(selected),
            cx.listener(move |this, index: &usize, window, cx| {
                let tabs = if page == Page::Git {
                    &mut this.git_state_mut(embedded).tabs
                } else {
                    &mut this.file_state_mut(embedded).tabs
                };
                if let Some(file) = tabs.open.get(*index) {
                    tabs.selected = *file;
                    if page == Page::Files {
                        this.focus_preview_file(embedded, window, cx);
                    } else {
                        this.focus_preview_git(embedded, window, cx);
                    }
                    cx.notify();
                }
            }),
            cx,
        );
        if !embedded {
            return h_flex()
                .debug_selector(|| "document-header".into())
                .w_full()
                .h_full()
                .min_w_0()
                .child(strip)
                .into_any_element();
        }
        crate::header::Header::new("document-header", cx)
            .bordered(false)
            .px_2()
            .flex_shrink_0()
            .w_full()
            .child(strip)
            .into_any_element()
    }
}
