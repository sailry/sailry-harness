//! Connection history uses the same searchable Kit surface as branch and worktree pickers.
use super::*;
use gpui_kit::component::list::{ListDelegate, ListItem, ListState};

struct Entry {
    id: SessionId,
    title: SharedString,
}

struct Items {
    owner: WeakEntity<View>,
    entries: Vec<Entry>,
    visible: Vec<usize>,
    current: Option<SessionId>,
    selected: Option<IndexPath>,
}

pub(super) fn open(view: &View, window: &mut Window, cx: &mut Context<View>) {
    if window.has_active_dialog(cx) {
        return;
    }
    let entries: Vec<_> = view
        .connection_sessions()
        .iter()
        .map(|session| Entry {
            id: session.id,
            title: crate::activity::title(session),
        })
        .collect();
    let selected = entries
        .iter()
        .position(|entry| Some(entry.id) == view.session())
        .or_else(|| (!entries.is_empty()).then_some(0))
        .map(IndexPath::new);
    let items = Items {
        owner: cx.weak_entity(),
        visible: (0..entries.len()).collect(),
        entries,
        current: view.session(),
        selected: None,
    };
    let list = cx.new(|cx| ListState::new(items, window, cx).searchable(true));
    list.update(cx, |list, cx| list.set_selected_index(selected, window, cx));
    let content = list.clone();
    window.open_dialog(cx, move |dialog, window, _| {
        crate::command_picker::dialog(
            dialog,
            "connection-chat-picker",
            &content,
            tr("connection_search"),
            window,
        )
    });
    window.defer(cx, move |window, cx| {
        list.update(cx, |list, cx| list.focus(window, cx))
    });
}

impl ListDelegate for Items {
    type Item = ListItem;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.visible.len()
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
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        let query = query.to_lowercase();
        self.visible = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                entry.title.to_lowercase().contains(&query).then_some(index)
            })
            .collect();
        self.selected = None;
        cx.notify();
        Task::ready(())
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let entry = self.entries.get(*self.visible.get(index.row)?)?;
        let id = entry.id;
        Some(
            ListItem::new(("connection-chat-history", index.row))
                .h(px(36.))
                .px_2()
                .rounded(cx.theme().radius)
                .check_icon(IconName::Check)
                .confirmed(self.current == Some(id))
                .on_mouse_enter(cx.listener(move |list, _, window, cx| {
                    if list.selected_index() != Some(index) {
                        list.set_selected_index(Some(index), window, cx);
                        cx.notify();
                    }
                }))
                .child(
                    h_flex()
                        .min_w_0()
                        .w_full()
                        .gap_2()
                        .debug_selector(move || format!("connection-chat-history-{id}"))
                        .child(Icon::new(IconName::Bot).size_4())
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_sm()
                                .truncate()
                                .child(entry.title.clone()),
                        ),
                ),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .p_3()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .debug_selector(|| "connection-chat-history-empty".into())
            .child(tr(if self.entries.is_empty() {
                "chat_connection_history_empty"
            } else {
                "connection_search_empty"
            }))
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(id) = self
            .selected
            .and_then(|index| self.visible.get(index.row))
            .and_then(|index| self.entries.get(*index))
            .map(|entry| entry.id)
        else {
            return;
        };
        let owner = self.owner.clone();
        window.close_dialog(cx);
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |view, cx| view.switch_session(Some(id), window, cx));
        });
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        window.close_dialog(cx);
    }
}
