//! Host menus address existing forwarders by their captured host and local port.
use super::*;
use crate::live::menus::{Command, Dispatch, Target};
use gpui_kit::component::native_menu::NativeMenu;
use sailry_client::ports::State;
use sailry_protocol::NodeId;

impl Shell {
    pub(crate) fn show_host_ports(
        &mut self,
        node: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let Some(owner) = self.ports_for(node, window, cx) else {
            return;
        };
        picker::open(owner, window, cx);
    }

    pub(crate) fn port_menu(&self, node: NodeId, cx: &App) -> NativeMenu {
        let mut menu = NativeMenu::new();
        if let Some(ports) = self.ports.get(&node) {
            for entry in &ports.read(cx).entries {
                let forwarder = &entry.forwarder;
                menu = menu.menu_with_disabled(
                    format!(
                        "{} → 127.0.0.1:{}",
                        forwarder.remote_port, forwarder.local_port
                    ),
                    *forwarder.state.borrow() != State::Listening,
                    Box::new(Dispatch {
                        node,
                        target: Target::Host,
                        command: Command::OpenPort {
                            port: forwarder.local_port,
                        },
                    }),
                );
            }
        }
        if !menu.is_empty() {
            menu = menu.separator();
        }
        menu.menu(
            tr("port_title"),
            Box::new(Dispatch {
                node,
                target: Target::Host,
                command: Command::Ports,
            }),
        )
    }

    pub(crate) fn open_forwarded_port(&self, node: NodeId, port: u16, cx: &mut App) {
        let Some(ports) = self.ports.get(&node) else {
            return;
        };
        if let Some(entry) = ports.read(cx).entries.iter().find(|entry| {
            entry.forwarder.local_port == port
                && *entry.forwarder.state.borrow() == State::Listening
        }) {
            cx.open_url(&entry.url);
        }
    }
}
