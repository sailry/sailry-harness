use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use sailry_protocol::NodeId;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Host {
    Preview(usize),
    Node(NodeId),
}

impl Editor {
    fn selected_host(&self) -> Host {
        self.live
            .as_ref()
            .map_or(Host::Preview(self.host), |transport| {
                Host::Node(transport.target())
            })
    }

    pub(super) fn host_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected_host();
        let choices: Vec<_> = self
            .shell
            .upgrade()
            .map(|shell| {
                let shell = shell.read(cx);
                if let Some(live) = &shell.live {
                    live.hosts
                        .keys()
                        .map(|node| (Host::Node(*node), live.name(*node)))
                        .collect()
                } else {
                    (0..2)
                        .map(|index| {
                            (
                                Host::Preview(index),
                                tr(if index == 0 {
                                    "composer_host_local"
                                } else {
                                    "composer_host_remote"
                                }),
                            )
                        })
                        .collect()
                }
            })
            .unwrap_or_default();
        let label = choices
            .iter()
            .find(|(host, _)| *host == selected)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| tr("live_disconnected"));
        if self.original.is_some() || self.live_original.is_some() {
            return h_flex()
                .h_8()
                .gap_2()
                .child(Icon::new(IconName::Cpu))
                .child(label)
                .into_any_element();
        }
        let owner = cx.entity().downgrade();
        let measure = owner.clone();
        let picker = Button::new("project-host")
            .w_full()
            .accessibility_label(label.clone())
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(Icon::new(IconName::Cpu).size_4())
                    .child(div().truncate().text_sm().child(label)),
            )
            .dropdown_caret(true)
            .disabled(self.pending || self.request.is_some())
            .debug_selector(|| "project-host".into())
            .dropdown_menu(move |menu, _, cx| {
                // Kit dropdown menus size independently of their full-width trigger.
                let menu = if let Some(editor) = owner.upgrade() {
                    let width = editor.read(cx).host_bounds.size.width;
                    menu.min_w(width).max_w(width)
                } else {
                    menu
                };
                choices.iter().fold(menu, |menu, (host, label)| {
                    let host = *host;
                    let owner = owner.clone();
                    menu.item(
                        PopupMenuItem::new(label.clone())
                            .checked(host == selected)
                            .on_click(move |_, window, cx| {
                                let _ = owner
                                    .update(cx, |editor, cx| editor.select_host(host, window, cx));
                            }),
                    )
                })
            });
        div()
            .w_full()
            .on_prepaint(move |bounds, _, cx| {
                let _ = measure.update(cx, |editor, _| editor.host_bounds = bounds);
            })
            .child(picker)
            .into_any_element()
    }

    pub(super) fn select_host(&mut self, host: Host, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending
            || self.request.is_some()
            || self.original.is_some()
            || self.live_original.is_some()
            || self.selected_host() == host
        {
            return;
        }
        match host {
            Host::Node(node) => {
                let transport = self
                    .shell
                    .upgrade()
                    .and_then(|shell| shell.read(cx).live.as_ref()?.transport_for(node));
                let Some(transport) = transport else {
                    self.fail("live_disconnected", window, cx);
                    return;
                };
                self.live = Some(transport);
            }
            Host::Preview(index) if self.live.is_none() && index < 2 => self.host = index,
            _ => return,
        }
        self.other_path = SharedString::default();
        self.inputs[1].update(cx, |input, cx| input.set_value("", window, cx));
        self.error = None;
        self.suggest(window, cx);
        cx.notify();
    }
}
