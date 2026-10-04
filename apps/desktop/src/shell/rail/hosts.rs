use super::*;
use crate::live::{host::host_icon, menus};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use sailry_protocol::NodeId;

#[derive(Clone, Copy)]
enum Host {
    Node(NodeId),
    Preview(usize),
}

impl Shell {
    pub(super) fn host_picker(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        let (icon, hosts) = if let Some(live) = &self.live {
            (
                host_icon(live.info.as_ref().and_then(|info| info.os.as_deref())),
                live.hosts
                    .keys()
                    .map(|node| (Host::Node(*node), live.name(*node), live.selected == *node))
                    .collect::<Vec<_>>(),
            )
        } else {
            (
                host_icon(None),
                ["local_host_name", "remote_host_name"]
                    .into_iter()
                    .enumerate()
                    .map(|(index, label)| (Host::Preview(index), tr(label), self.host == index))
                    .collect(),
            )
        };
        Button::new("sidebar-host")
            .ghost()
            .size_9()
            .child(
                div()
                    .debug_selector(|| "sidebar-host-icon".into())
                    .child(icon.size_4()),
            )
            .tooltip(tr("hosts"))
            .accessibility_label(tr("hosts"))
            .debug_selector(|| "sidebar-host".into())
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                hosts.iter().fold(
                    menu.max_h(px(480.)).scrollable(true),
                    |menu, (host, label, selected)| {
                        let owner = owner.clone();
                        let host = *host;
                        let label = label.clone();
                        let selector = match host {
                            Host::Node(node) => {
                                format!("sidebar-host-option-{}", crate::live::node_key(node))
                            }
                            Host::Preview(index) => format!("sidebar-host-option-{index}"),
                        };
                        menu.item(
                            PopupMenuItem::element(move |_, _| {
                                div()
                                    .debug_selector({
                                        let selector = selector.clone();
                                        move || selector.clone()
                                    })
                                    .w_full()
                                    .min_w_0()
                                    .truncate()
                                    .child(label.clone())
                            })
                            .checked(*selected)
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |shell, cx| match host {
                                    Host::Node(node) => shell.live_resource_action(
                                        &menus::Dispatch {
                                            node,
                                            target: menus::Target::Host,
                                            command: menus::Command::Open,
                                        },
                                        window,
                                        cx,
                                    ),
                                    Host::Preview(index) if shell.live.is_none() => {
                                        shell.select_host(index, window, cx)
                                    }
                                    Host::Preview(_) => {}
                                });
                            }),
                        )
                    },
                )
            })
            .anchor_offset(Self::rail_popover_offset(window))
    }
}

#[cfg(test)]
mod tests;
