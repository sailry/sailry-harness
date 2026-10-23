//! Host forwarding navigation uses the shared Kit command surface.
use super::*;
use gpui_kit::component::list::{ListDelegate, ListItem, ListState};
use sailry_client::ports::State;

struct Picker {
    owner: Entity<Workspace>,
    selected: Option<u16>,
    state: Entity<ListState<Items>>,
    _updates: Subscription,
    enter: Option<Subscription>,
}

#[derive(Clone, Copy)]
enum Choice {
    Select(u16),
    Add,
    Open(u16),
    Copy(u16),
    Close(u16),
    Back,
}

pub(super) fn open(owner: Entity<Workspace>, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    let picker = cx.new(|cx| Picker {
        _updates: cx.observe_in(&owner, window, |picker: &mut Picker, _, window, cx| {
            let state = picker.state.clone();
            window.defer(cx, move |window, cx| {
                state.update(cx, |list, cx| {
                    let selected = list
                        .delegate()
                        .rows(cx)
                        .iter()
                        .position(|(_, row)| !row.disabled)
                        .map(IndexPath::new);
                    list.set_selected_index(selected, window, cx);
                    cx.notify();
                });
            });
            cx.notify();
        }),
        owner,
        selected: None,
        enter: None,
        state: {
            let owner = cx.weak_entity();
            cx.new(|cx| ListState::new(Items::new(owner), window, cx).searchable(true))
        },
    });
    picker.update(cx, |picker, cx| {
        picker.state.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::default()), window, cx);
        });
        picker.enter = Some(crate::command_picker::intercept(
            &picker.state,
            Items::enabled,
            cx,
        ));
    });
    let focus = picker.read(cx).state.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let picker = picker.read(cx);
        let rows = picker.state.read(cx).delegate().rows(cx).len();
        crate::command_picker::dialog(
            dialog,
            "port-picker",
            &picker.state,
            tr("port_search"),
            window,
            cx,
        )
        .h(crate::command_picker::height(rows, 0, window))
    });
    window.defer(cx, move |window, cx| {
        focus.update(cx, |state, cx| state.focus(window, cx));
    });
}

impl Picker {
    fn rows(&self, cx: &App) -> Vec<(Choice, Row)> {
        let workspace = self.owner.read(cx);
        if let Some(port) = self.selected {
            let active = workspace.entries.iter().any(|entry| {
                entry.forwarder.local_port == port
                    && *entry.forwarder.state.borrow() == State::Listening
            });
            return vec![
                (
                    Choice::Open(port),
                    item(
                        tr("port_address").to_string(),
                        IconName::Globe,
                        "port-open".into(),
                        !active,
                    ),
                ),
                (
                    Choice::Copy(port),
                    item(
                        tr("port_copy").to_string(),
                        IconName::Copy,
                        "port-copy".into(),
                        false,
                    ),
                ),
                (
                    Choice::Close(port),
                    item(
                        tr("port_close").to_string(),
                        IconName::Close,
                        "port-close".into(),
                        false,
                    ),
                ),
                (
                    Choice::Back,
                    item(
                        tr("reference_back").to_string(),
                        IconName::ChevronLeft,
                        "port-back".into(),
                        false,
                    ),
                ),
            ];
        }
        let mut rows: Vec<_> = workspace
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let forwarder = &entry.forwarder;
                let mut label = format!(
                    "{} → 127.0.0.1:{}",
                    forwarder.remote_port, forwarder.local_port
                );
                if *forwarder.state.borrow() != State::Listening {
                    label.push_str(&format!(" · {}", tr("port_closed")));
                }
                (
                    Choice::Select(forwarder.local_port),
                    item(label, IconName::Network, format!("port-row-{index}"), false),
                )
            })
            .collect();
        rows.push((
            Choice::Add,
            item(
                tr("port_add").to_string(),
                IconName::Plus,
                "port-add".into(),
                false,
            ),
        ));
        rows
    }

    fn navigate(&mut self, selected: Option<u16>, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = selected;
        let selected = self
            .rows(cx)
            .iter()
            .position(|(_, row)| !row.disabled)
            .map(IndexPath::new);
        let owner = cx.weak_entity();
        self.state = cx.new(|cx| ListState::new(Items::new(owner), window, cx).searchable(true));
        self.state.update(cx, |list, cx| {
            list.set_selected_index(selected, window, cx);
        });
        self.enter = Some(crate::command_picker::intercept(
            &self.state,
            Items::enabled,
            cx,
        ));
        self.state.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn confirm(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        match choice {
            Choice::Select(port) => self.navigate(Some(port), window, cx),
            Choice::Back => self.navigate(None, window, cx),
            Choice::Add => {
                window.close_dialog(cx);
                Workspace::show(self.owner.clone(), window, cx);
            }
            Choice::Open(port) => {
                if let Some(entry) = self.owner.read(cx).entries.iter().find(|entry| {
                    entry.forwarder.local_port == port
                        && *entry.forwarder.state.borrow() == State::Listening
                }) {
                    cx.open_url(&entry.url);
                }
                window.close_dialog(cx);
            }
            Choice::Copy(port) => {
                if let Some(entry) = self
                    .owner
                    .read(cx)
                    .entries
                    .iter()
                    .find(|entry| entry.forwarder.local_port == port)
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(entry.url.clone()));
                }
                window.close_dialog(cx);
            }
            Choice::Close(port) => {
                self.owner.update(cx, |workspace, cx| {
                    if let Some(index) = workspace
                        .entries
                        .iter()
                        .position(|entry| entry.forwarder.local_port == port)
                    {
                        workspace.close(index, cx);
                    }
                });
                self.navigate(None, window, cx);
            }
        }
    }
}

struct Row {
    label: String,
    icon: IconName,
    selector: String,
    disabled: bool,
}

fn item(label: String, icon: IconName, selector: String, disabled: bool) -> Row {
    Row {
        label,
        icon,
        selector,
        disabled,
    }
}

struct Items {
    owner: WeakEntity<Picker>,
    query: String,
    selected: Option<IndexPath>,
}

impl Items {
    fn new(owner: WeakEntity<Picker>) -> Self {
        Self {
            owner,
            query: String::new(),
            selected: None,
        }
    }

    fn rows(&self, cx: &App) -> Vec<(Choice, Row)> {
        self.owner.upgrade().map_or_else(Vec::new, |owner| {
            owner
                .read(cx)
                .rows(cx)
                .into_iter()
                .filter(|(_, row)| row.label.to_lowercase().contains(&self.query))
                .collect()
        })
    }

    fn enabled(&self, cx: &App) -> Vec<IndexPath> {
        self.rows(cx)
            .iter()
            .enumerate()
            .filter(|(_, (_, row))| !row.disabled)
            .map(|(index, _)| IndexPath::new(index))
            .collect()
    }
}

impl ListDelegate for Items {
    type Item = ListItem;

    fn items_count(&self, _: usize, cx: &App) -> usize {
        self.rows(cx).len()
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
        self.query = query.to_lowercase();
        let selected = self
            .rows(cx)
            .iter()
            .position(|(_, row)| !row.disabled)
            .map(IndexPath::new);
        let owner = cx.weak_entity();
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |list, cx| {
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
        let (_, row) = self.rows(cx).into_iter().nth(index.row)?;
        Some(
            crate::command_picker::row(("port-choice", index.row), false, row.disabled, cx)
                .debug_selector(move || row.selector.clone())
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_2()
                        .child(Icon::new(row.icon).size_4())
                        .child(div().flex_1().truncate().child(row.label)),
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
            .child(tr("connection_search_empty"))
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some((choice, row)) = self
            .selected
            .and_then(|index| self.rows(cx).into_iter().nth(index.row))
        else {
            return;
        };
        if row.disabled {
            return;
        }
        let owner = self.owner.clone();
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |picker, cx| picker.confirm(choice, window, cx));
        });
    }
}
