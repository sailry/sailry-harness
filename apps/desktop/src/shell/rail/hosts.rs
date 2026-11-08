use super::*;
use crate::live::{host::platform_icon, menus};
use gpui_kit::component::{
    badge::Badge,
    menu::{DropdownMenu, PopupMenuItem},
};
use sailry_protocol::NodeId;

#[derive(Clone, Copy)]
enum Host {
    Node(NodeId),
    Preview(usize),
}

impl Shell {
    pub(super) fn host_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        let (icon, hosts) = if let Some(live) = &self.live {
            (
                platform_icon(live.info.as_ref().and_then(|info| info.os.as_deref())),
                live.hosts
                    .keys()
                    .map(|node| {
                        (
                            Host::Node(*node),
                            live.name(*node),
                            live.selected == *node,
                            update_target(live.host_versions.get(node)).is_some(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            (
                platform_icon(None),
                ["local_host_name", "remote_host_name"]
                    .into_iter()
                    .enumerate()
                    .map(|(index, label)| {
                        (Host::Preview(index), tr(label), self.host == index, false)
                    })
                    .collect(),
            )
        };
        let update = hosts.iter().any(|(_, _, _, update)| *update);
        let button = Button::new("sidebar-host")
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
            .dropdown_menu_with_anchor(Anchor::LeftCenter, move |menu, _, cx| {
                let popup = cx.weak_entity();
                let choices = hosts.iter().fold(
                    menu.max_h(px(480.)).scrollable(true),
                    |menu, (host, label, selected, update)| {
                        let owner = owner.clone();
                        let updater = owner.clone();
                        let popup = popup.clone();
                        let update = *update;
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
                                let updater = updater.clone();
                                let popup = popup.clone();
                                h_flex()
                                    .debug_selector({
                                        let selector = selector.clone();
                                        move || selector.clone()
                                    })
                                    .w_full()
                                    .min_w_0()
                                    .gap_2()
                                    .child(div().flex_1().min_w_0().truncate().child(label.clone()))
                                    .when(update, |row| {
                                        row.child(
                                            Button::new(format!("{selector}-update"))
                                                .ghost()
                                                .xsmall()
                                                .label(tr("sidebar_host_update"))
                                                .on_click(move |_, window, cx| {
                                                    cx.stop_propagation();
                                                    let _ = popup
                                                        .update(cx, |_, cx| cx.emit(DismissEvent));
                                                    let _ = updater.update(cx, |shell, cx| {
                                                        if let Host::Node(node) = host {
                                                            shell
                                                                .update_live_host(node, window, cx);
                                                        }
                                                    });
                                                }),
                                        )
                                    })
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
                );
                let owner = owner.clone();
                choices.separator().item(
                    PopupMenuItem::new(tr("host_add"))
                        .icon(IconName::Plus)
                        .on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |shell, cx| shell.add_host(window, cx));
                        }),
                )
            });
        let badge = if update {
            Badge::new().dot().color(cx.theme().primary)
        } else {
            Badge::new()
        };
        div()
            .id("sidebar-host-context")
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|shell, event, window, cx| {
                    if let Some(node) = shell.live.as_ref().map(|live| live.selected) {
                        shell.live_resource_menu(node, menus::Target::Host, event, window, cx);
                    }
                }),
            )
            .child(badge.child(button))
    }

    fn update_live_host(&mut self, node: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        use crate::theme::DialogStyle as _;
        let Some(versions) = self
            .live
            .as_ref()
            .and_then(|live| live.host_versions.get(&node))
        else {
            return;
        };
        let Some(update_host) = update_target(Some(versions)) else {
            return;
        };
        if update_host {
            self.update_host(node, window, cx);
        } else if let Some(version) = versions.1.clone() {
            let panel = cx.new(|cx| crate::updater::Panel::new(window, cx));
            window.open_dialog(cx, move |dialog, _, cx| {
                dialog
                    .form_title(tr("updates"), cx)
                    .w(px(560.))
                    .child(panel.clone())
            });
            crate::updater::check_version(&version, cx);
        }
    }
}

fn update_target(versions: Option<&(Option<u16>, Option<String>)>) -> Option<bool> {
    let (protocol, version) = versions?;
    match sailry_link::check_host(*protocol, version.as_deref()) {
        Err(error) => Some(matches!(
            error.details,
            Some(sailry_protocol::FaultDetails::VersionMismatch {
                update_host: true,
                ..
            })
        )),
        Ok(()) => sailry_link::host_outdated(version.as_deref()).then_some(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn detects_updates_for_either_side() {
        assert_eq!(update_target(None), None);
        assert_eq!(
            update_target(Some(&(
                Some(sailry_protocol::VERSION),
                Some(sailry_link::PRODUCT_VERSION.into())
            ))),
            None
        );
        assert_eq!(
            update_target(Some(&(
                Some(sailry_protocol::VERSION),
                Some("0.0.0".into())
            ))),
            Some(true)
        );
        assert_eq!(update_target(Some(&(None, None))), Some(true));
        assert_eq!(
            update_target(Some(&(
                Some(sailry_protocol::VERSION + 1),
                Some("99.0.0".into())
            ))),
            Some(false)
        );
    }
}
