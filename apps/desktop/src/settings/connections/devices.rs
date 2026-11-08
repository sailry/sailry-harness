use super::*;
use crate::preferences::Device as KnownDevice;
use futures::{StreamExt, stream};
use gpui_kit::component::badge::Badge;
use sailry_client::Client;
use sailry_link::{ConnectionInfo, EndpointAddr, Route};
use sailry_protocol::{Command, NodeId, Output};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

#[cfg(test)]
mod tests;

// Keeps status aligned across rows when the local Host has no revoke action.
const ACTION_WIDTH: f32 = 28.;

pub(super) struct Device {
    id: NodeId,
    local: bool,
    known: KnownDevice,
    online: bool,
    address: Option<EndpointAddr>,
    connection: ConnectionInfo,
}

enum DeviceUpdate {
    Inventory(Vec<Device>),
    Connection(EndpointAddr, ConnectionInfo),
    Presence(BTreeSet<NodeId>),
}

impl Device {
    fn observe(&mut self, peers: &BTreeSet<NodeId>) {
        if !self.local && !self.known.execution {
            self.online = peers.contains(&self.id);
        }
    }

    fn status_key(&self) -> &'static str {
        if self.local {
            "composer_host_local"
        } else if self.online {
            "connections_online"
        } else {
            "connections_offline"
        }
    }

    fn latency_text(&self) -> Option<String> {
        self.connection
            .rtt_ms
            .filter(|_| self.online && !self.local)
            .map(|rtt| format!("{rtt} ms"))
    }

    fn route_key(&self) -> Option<&'static str> {
        match self.connection.route.filter(|_| self.online && !self.local) {
            Some(Route::P2p) => Some("connection_route_p2p"),
            Some(Route::Relay) => Some("connection_route_relay"),
            _ => None,
        }
    }

    /// An online peer names its route beside the status dot; otherwise its presence.
    fn label_key(&self) -> &'static str {
        self.route_key().unwrap_or_else(|| self.status_key())
    }
}

impl Connections {
    pub(super) fn observe_devices(&mut self, cx: &mut Context<Self>) {
        if self.watcher.is_some() {
            return;
        }
        let Some(services) = cx.try_global::<Services>().cloned() else {
            return;
        };
        let stop = CancellationToken::new();
        self.watch_stop = Some(stop.clone());
        let mut known = crate::preferences::data(cx).devices.unwrap_or_default();
        let (sender, mut updates) = tokio::sync::mpsc::channel(1);
        let mut changed = services.link.peer_changes();
        services.runtime.clone().spawn(async move {
            if let Some(mut peers) = services.link.connected_peers() {
                let sender = sender.clone();
                let token = stop.clone();
                services.runtime.spawn(async move {
                    loop {
                        let connected = peers.borrow_and_update().clone();
                        tokio::select! {
                            _ = token.cancelled() => break,
                            result = sender.send(DeviceUpdate::Presence(connected)) => if result.is_err() { break; },
                        }
                        tokio::select! {
                            _ = token.cancelled() => break,
                            result = peers.changed() => if result.is_err() { break; },
                        }
                    }
                });
            }
            let mut watchers: BTreeMap<NodeId, (EndpointAddr, CancellationToken)> = BTreeMap::new();
            loop {
                let poll = async {
                    let peers = services.link.peers().await.ok()?;
                    watchers.retain(|_, (address, token)| {
                        if peers.contains(address) { true } else { token.cancel(); false }
                    });
                    for address in &peers {
                        let id = NodeId(*address.id.as_bytes());
                        if watchers.contains_key(&id) { continue; }
                        let token = stop.child_token();
                        watchers.insert(id, (address.clone(), token.clone()));
                        let client = Client::new(services.link.remote(address.clone()));
                        let address = address.clone();
                        let sender = sender.clone();
                        services.runtime.spawn(async move {
                            let (connection, mut values) = tokio::sync::watch::channel(ConnectionInfo::default());
                            let forward = async {
                                loop {
                                    let value = *values.borrow_and_update();
                                    tokio::select! {
                                        _ = token.cancelled() => break,
                                        result = sender.send(DeviceUpdate::Connection(address.clone(), value)) => if result.is_err() { break; },
                                    }
                                    tokio::select! {
                                        _ = token.cancelled() => break,
                                        result = values.changed() => if result.is_err() { break; },
                                    }
                                }
                            };
                            tokio::select! {
                                _ = client.watch_connection(connection, token.clone()) => {},
                                _ = forward => {},
                            }
                        });
                    }
                    let mut devices = stream::iter(peers).map(|address| {
                        let services = &services;
                        let known = &known;
                        async move {
                            let id = NodeId(*address.id.as_bytes());
                            let mut device = Device { id, local: false,
                                known: known.get(&key(id)).cloned().unwrap_or(KnownDevice { name: None, execution: true, platform: None }), online: false,
                                address: Some(address.clone()), connection: ConnectionInfo::default() };
                            if let Ok(Ok(info)) = tokio::time::timeout(Duration::from_secs(3), services.link.inspect(&address)).await {
                                device.online = info.execution && info.compatibility().is_ok();
                                device.known.execution = info.execution;
                                device.known.platform = Some(info.platform);
                                if info.name.is_some() { device.known.name = info.name; }
                                if device.online {
                                    let client = Client::new(services.link.remote(address));
                                    if let Ok(Ok(Output::HostInfo(info))) = tokio::time::timeout(Duration::from_secs(3), client.execute(client.prepare(Command::InspectHost))).await {
                                        device.known.name = info.name;
                                    }
                                }
                            }
                            device
                        }
                    }).buffer_unordered(4).collect::<Vec<_>>().await;
                    let local = Client::new(services.local.clone());
                    let name = match local.execute(local.prepare(Command::InspectHost)).await {
                        Ok(Output::HostInfo(info)) => info.name,
                        _ => None,
                    };
                    devices.push(Device { id: services.local.target(), local: true,
                        known: KnownDevice { name, execution: true, platform: Some(std::env::consts::OS.to_owned()) }, online: true, address: None, connection: ConnectionInfo::default() });
                    devices.sort_by_key(|device| (!device.local, device.id));
                    known = devices.iter().map(|device| (key(device.id), device.known.clone())).collect();
                    Some(devices)
                };
                tokio::select! {
                    _ = stop.cancelled() => break,
                    devices = poll => if let Some(devices) = devices && sender.send(DeviceUpdate::Inventory(devices)).await.is_err() { break; },
                }
                tokio::select! {
                    _ = stop.cancelled() => break,
                    _ = changed.changed() => {},
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {},
                }
            }
        });
        self.watcher = Some(cx.spawn(async move |this, cx| {
            let mut telemetry: BTreeMap<NodeId, (EndpointAddr, ConnectionInfo)> = BTreeMap::new();
            let mut connected = BTreeSet::new();
            while let Some(update) = updates.recv().await {
                if this
                    .update(cx, |this, cx| {
                        let mut devices = match update {
                            DeviceUpdate::Presence(peers) => {
                                connected = peers;
                                for device in &mut this.devices {
                                    device.observe(&connected);
                                }
                                cx.notify();
                                return;
                            }
                            DeviceUpdate::Connection(address, connection) => {
                                telemetry.insert(
                                    NodeId(*address.id.as_bytes()),
                                    (address.clone(), connection),
                                );
                                if let Some(device) = this
                                    .devices
                                    .iter_mut()
                                    .find(|device| device.address.as_ref() == Some(&address))
                                {
                                    device.connection = connection;
                                    cx.notify();
                                }
                                return;
                            }
                            DeviceUpdate::Inventory(devices) => devices,
                        };
                        for device in &mut devices {
                            device.observe(&connected);
                            if let Some((address, connection)) = telemetry.get(&device.id)
                                && device.address.as_ref() == Some(address)
                            {
                                device.connection = *connection;
                            }
                        }
                        let known: BTreeMap<_, _> = devices
                            .iter()
                            .map(|device| (key(device.id), device.known.clone()))
                            .collect();
                        if crate::preferences::data(cx).devices.as_ref() != Some(&known) {
                            crate::preferences::update(cx, |data| data.devices = Some(known));
                        }
                        this.devices = devices;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    pub(super) fn device_rows(&self, execution: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.devices
            .iter()
            .filter(|device| device.known.execution == execution)
            .map(|device| {
                let id = device.id;
                let short = crate::live::short_id(id);
                let name: SharedString =
                    device
                        .known
                        .name
                        .clone()
                        .map(Into::into)
                        .unwrap_or_else(|| {
                            rust_i18n::t!("connections_device", id = short)
                                .to_string()
                                .into()
                        });
                let status = h_flex()
                    .debug_selector(move || format!("connection-status-{id:?}"))
                    .items_center()
                    .gap_2()
                    .flex_shrink_0()
                    .child(
                        div()
                            .size_1p5()
                            .child(Badge::new().dot().color(if device.online {
                                cx.theme().success
                            } else {
                                cx.theme().muted_foreground
                            })),
                    )
                    .child(
                        div()
                            .debug_selector(move || format!("connection-status-label-{id:?}"))
                            .text_color(cx.theme().muted_foreground)
                            .child(tr(device.label_key())),
                    );
                let latency = device.latency_text().map(|text| {
                    div()
                        .debug_selector(move || format!("connection-latency-{id:?}"))
                        .flex_shrink_0()
                        .text_color(cx.theme().muted_foreground)
                        .child(text)
                });
                let action = h_flex()
                    .debug_selector(move || format!("connection-action-{id:?}"))
                    .w(px(ACTION_WIDTH))
                    .flex_shrink_0()
                    .justify_end()
                    .when(!device.local, |cell| {
                        cell.child(
                            Button::new(SharedString::from(format!("unpair-{id:?}")))
                                .ghost()
                                .small()
                                .icon(IconName::Close)
                                .tooltip(tr("live_revoke"))
                                .accessibility_label(tr("live_revoke"))
                                .on_click(cx.listener(move |_, _, window, cx| {
                                    let Some(services) = cx.try_global::<Services>().cloned()
                                    else {
                                        return;
                                    };
                                    let owner = cx.entity().downgrade();
                                    crate::prompts::confirm(
                                        &tr("live_revoke"),
                                        &tr("connections_revoke_confirm"),
                                        tr("live_revoke"),
                                        window,
                                        cx,
                                        move |_, cx| {
                                            let job = services.runtime.clone().spawn(async move {
                                                services.link.set_trust(id, false).await
                                            });
                                            cx.spawn(async move |cx| {
                                                if !matches!(job.await, Ok(Ok(()))) {
                                                    let _ = owner.update(cx, |this, cx| {
                                                        this.status = tr("pairing_failed");
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .detach();
                                        },
                                    );
                                })),
                        )
                    });
                let details = h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_3()
                    .children(latency)
                    .child(status)
                    .child(action);
                h_flex()
                    .id(SharedString::from(format!("connection-device-{id:?}")))
                    .py_3()
                    .gap_3()
                    .w_full()
                    .text_sm()
                    .debug_selector(move || format!("connection-device-{id:?}"))
                    .child(
                        div()
                            .debug_selector(move || format!("connection-name-{id:?}"))
                            .flex_1()
                            .min_w_0()
                            .child(
                                crate::ui::media_row::MediaRow::new(name)
                                    .leading(
                                        Icon::default()
                                            .path(platform_icon(
                                                device.known.platform.as_deref(),
                                                execution,
                                            ))
                                            .size_5(),
                                    )
                                    .description(short),
                            ),
                    )
                    .child(details)
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |_, event: &MouseDownEvent, window, cx| {
                            if !execution {
                                return;
                            }
                            cx.stop_propagation();
                            if !cfg!(any(target_os = "macos", target_os = "windows")) {
                                crate::feedback::error(
                                    "workspace_native_menu_platform",
                                    "",
                                    &tr("workspace_native_menu_platform"),
                                    window,
                                    cx,
                                );
                                return;
                            }
                            gpui_kit::component::native_menu::NativeMenu::new()
                                .menu(
                                    tr("connection_action_edit"),
                                    Box::new(crate::live::menus::Dispatch {
                                        node: id,
                                        target: crate::live::menus::Target::Host,
                                        command: crate::live::menus::Command::Edit,
                                    }),
                                )
                                .show(event.position, window, cx);
                        }),
                    )
                    .into_any_element()
            })
            .collect()
    }
}

fn key(node: NodeId) -> String {
    node.0.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn platform_icon(platform: Option<&str>, execution: bool) -> &'static str {
    match platform {
        Some("android") => "icons/os/android.svg",
        Some("ios" | "macos") => "icons/os/apple.svg",
        Some("windows") => "icons/os/windows11.svg",
        Some("linux") => "icons/os/linux.svg",
        _ if execution => "icons/reicon/device.svg",
        _ => "icons/reicon/smartphone.svg",
    }
}
