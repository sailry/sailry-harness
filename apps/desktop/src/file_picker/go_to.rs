//! Folder paths use the shared command picker and the existing Node navigation.
use super::*;
use gpui_kit::component::list::{ListDelegate, ListItem, ListState};

pub(super) struct Items {
    owner: WeakEntity<Picker>,
    query: String,
    separator: char,
    selected: Option<IndexPath>,
}

impl Picker {
    pub(super) fn go_to(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.busy || self.go_to.is_some() || !(self.valid)(cx) {
            return;
        }
        let Some(listing) = &self.listing else {
            return;
        };
        let query = listing.directory.path.clone();
        let separator = listing.separator;
        let owner = cx.entity();
        let list = cx.new(|cx| {
            cx.observe(&owner, |_, _, cx| cx.notify()).detach();
            ListState::new(
                Items {
                    owner: owner.downgrade(),
                    query: String::new(),
                    separator,
                    selected: None,
                },
                window,
                cx,
            )
            .searchable(true)
        });
        list.update(cx, |list, cx| {
            list.set_query(&query, window, cx);
            list.set_selected_index(Some(IndexPath::default()), window, cx);
        });
        self.go_to = Some(list.clone());
        self.focus.focus(window, cx);
        let error = self.error;
        let name_error = self.name_error;
        let cancel = cx.weak_entity();
        let content = list.clone();
        let confirm = list.clone();
        window.open_dialog(cx, move |dialog, window, _| {
            let cancel = cancel.clone();
            let confirm = confirm.clone();
            // Pinned Kit does not translate searchable input Enter into List confirm.
            let content = div()
                .size_full()
                .on_action(
                    move |action: &gpui_kit::component::input::Enter, window, cx| {
                        if action.shift {
                            cx.propagate();
                            return;
                        }
                        confirm.update(cx, |list, cx| {
                            list.delegate_mut().confirm(action.secondary, window, cx)
                        });
                    },
                )
                .child(crate::command_picker::list(
                    &content,
                    tr("directory_address"),
                    window,
                ));
            crate::command_picker::frame(dialog, "directory-go-to", content, window)
                .on_ok(|_, _, _| false)
                .overlay_closable(false)
                .on_cancel(move |_, _, cx| {
                    let _ = cancel.update(cx, |picker, cx| {
                        picker.go_to = None;
                        picker.error = error;
                        picker.name_error = name_error;
                        if let Some(stop) = picker.read_stop.take() {
                            stop.cancel();
                            picker.generation += 1;
                            picker.busy = false;
                        }
                        cx.notify();
                    });
                    true
                })
        });
        window.defer(cx, move |window, cx| {
            list.update(cx, |list, cx| list.focus(window, cx));
            window.dispatch_action(Box::new(gpui_kit::component::input::SelectAll), cx);
        });
        cx.notify();
    }
}

impl ListDelegate for Items {
    type Item = ListItem;

    fn items_count(&self, _: usize, _: &App) -> usize {
        usize::from(!self.query.is_empty())
    }

    fn set_selected_index(
        &mut self,
        index: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = index;
    }

    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.to_owned();
        let selected = (!self.query.is_empty()).then_some(IndexPath::default());
        let list = cx.weak_entity();
        window.defer(cx, move |window, cx| {
            let _ = list.update(cx, |list, cx| {
                list.set_selected_index(selected, window, cx);
                cx.notify();
            });
        });
        cx.notify();
        Task::ready(())
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        if index.row != 0 || self.query.is_empty() {
            return None;
        }
        let disabled = self
            .owner
            .upgrade()
            .is_none_or(|owner| owner.read(cx).closed || owner.read(cx).busy);
        let parts: Vec<_> = self
            .query
            .split(self.separator)
            .filter(|part| !part.is_empty())
            .collect();
        let label = if parts.is_empty() {
            self.query.clone()
        } else {
            parts.join(" › ")
        };
        Some(
            crate::command_picker::row("directory-go-to-target", false, disabled, cx)
                .debug_selector(|| "directory-go-to-target".into())
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_2()
                        .child(Icon::new(gpui_kit::assets::IconName::Folder).size_4())
                        .child(div().min_w_0().truncate().child(label)),
                ),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div().h_12()
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        if self.selected.is_none() || self.query.is_empty() {
            return;
        }
        let owner = self.owner.clone();
        let query = self.query.clone();
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |picker, cx| {
                if picker.go_to.is_some() {
                    picker.navigate(Some(query), Visit::GoTo, window, cx);
                }
            });
        });
    }
}
