//! Host forwarding navigation uses the shared Kit command surface.
use super::*;
use gpui_kit::component::command::{Command, CommandItem, CommandState};
use sailry_client::ports::State;

struct Picker {
    owner: Entity<Workspace>,
    selected: Option<u16>,
    state: Entity<CommandState>,
    _updates: Subscription,
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
        _updates: cx.observe_in(&owner, window, |_, _, window, cx| {
            window.refresh();
            cx.notify();
        }),
        owner,
        selected: None,
        state: cx.new(|cx| CommandState::new(window, cx)),
    });
    let focus = picker.read(cx).state.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let target = picker.downgrade();
        let picker = picker.read(cx);
        let rows = picker.rows(cx);
        let choices: Vec<_> = rows.iter().map(|(choice, _)| *choice).collect();
        crate::command_picker::frame(
            dialog,
            "port-picker",
            Command::new(&picker.state)
                .bordered(false)
                .text_sm()
                .max_h(crate::command_picker::body_height(window))
                .placeholder(tr("port_search"))
                .items(rows.into_iter().map(|(_, item)| item))
                .empty(|_, _, cx| {
                    div()
                        .p_3()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("connection_search_empty"))
                })
                .on_confirm(move |index, window, cx| {
                    if let Some(choice) = choices.get(index.row).copied() {
                        let _ = target.update(cx, |picker, cx| picker.confirm(choice, window, cx));
                    }
                }),
            window,
        )
    });
    window.defer(cx, move |window, cx| {
        focus.update(cx, |state, cx| state.focus(window, cx));
    });
}

impl Picker {
    fn rows(&self, cx: &App) -> Vec<(Choice, CommandItem)> {
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
                    )
                    .disabled(!active),
                ),
                (
                    Choice::Copy(port),
                    item(
                        tr("port_copy").to_string(),
                        IconName::Copy,
                        "port-copy".into(),
                    ),
                ),
                (
                    Choice::Close(port),
                    item(
                        tr("port_close").to_string(),
                        IconName::Close,
                        "port-close".into(),
                    ),
                ),
                (
                    Choice::Back,
                    item(
                        tr("reference_back").to_string(),
                        IconName::ChevronLeft,
                        "port-back".into(),
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
                    item(label, IconName::Network, format!("port-row-{index}")),
                )
            })
            .collect();
        rows.push((
            Choice::Add,
            item(
                tr("port_add").to_string(),
                IconName::Plus,
                "port-add".into(),
            ),
        ));
        rows
    }

    fn navigate(&mut self, selected: Option<u16>, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = selected;
        self.state = cx.new(|cx| CommandState::new(window, cx));
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

fn item(label: String, icon: IconName, selector: String) -> CommandItem {
    CommandItem::new().label(label.clone()).child(move |_, _| {
        let selector = selector.clone();
        h_flex()
            .debug_selector(move || selector.clone())
            .w_full()
            .min_w_0()
            .gap_2()
            .child(Icon::new(icon.clone()).size_4())
            .child(div().flex_1().truncate().child(label.clone()))
    })
}
