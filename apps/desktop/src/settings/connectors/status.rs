use super::*;
use gpui_kit::component::badge::Badge;
use sailry_protocol::plugin::{
    Mcp,
    mcp::{Availability, RuntimeState},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::settings) enum Status {
    Problem(&'static str),
    Connecting,
    Connected(u32),
    Failed,
}

pub(super) fn service(service: &catalog::Service) -> Option<Status> {
    if !service.enabled()
        || service
            .account
            .as_ref()
            .is_some_and(|account| !account.enabled)
    {
        return None;
    }
    if !service.servers.is_empty() && service.servers.iter().all(disabled) {
        return None;
    }
    if let Some(problem) = super::problem(service) {
        return Some(if problem == "provider_login_waiting" {
            Status::Connecting
        } else {
            Status::Problem(problem)
        });
    }
    aggregate(&service.servers)
}

pub(super) fn disabled(server: &Mcp) -> bool {
    !server.allowed
        || matches!(
            server.availability,
            Availability::PackageDisabled
                | Availability::ConnectionDisabled
                | Availability::ServerDisabled
        )
}

pub(in crate::settings) fn server(enabled: bool, server: &Mcp) -> Option<Status> {
    enabled
        .then(|| aggregate(std::slice::from_ref(server)))
        .flatten()
}

fn aggregate(servers: &[Mcp]) -> Option<Status> {
    let servers = servers
        .iter()
        .filter(|server| !disabled(server))
        .collect::<Vec<_>>();
    if let Some(problem) = servers
        .iter()
        .find_map(|server| super::availability(server.availability))
    {
        return Some(Status::Problem(problem));
    }
    let mut connected = false;
    let mut connecting = false;
    let mut tools = 0u32;
    for runtime in servers.iter().filter_map(|server| server.runtime.as_ref()) {
        match runtime.state {
            RuntimeState::Failed => return Some(Status::Failed),
            RuntimeState::Connecting => connecting = true,
            RuntimeState::Connected => {
                connected = true;
                tools = tools.saturating_add(runtime.tools);
            }
        }
    }
    if connecting {
        Some(Status::Connecting)
    } else if connected {
        Some(Status::Connected(tools))
    } else {
        None
    }
}

pub(in crate::settings) fn view(status: Option<Status>, cx: &App) -> AnyElement {
    // Same dot and label treatment as the device list: a centred dot, row-sized muted text.
    let dot = |color| {
        div()
            .size_1p5()
            .flex_shrink_0()
            .child(Badge::new().dot().color(color))
    };
    let mut view = h_flex()
        .flex_shrink_0()
        .gap_2()
        .items_center()
        .text_sm()
        .text_color(cx.theme().muted_foreground);
    match status {
        Some(Status::Problem(key)) => view = view.child(tr(key)),
        Some(Status::Connecting) => {
            view = view
                .child(Spinner::new().small())
                .child(tr("connector_mcp_connecting"));
        }
        Some(Status::Connected(tools)) => {
            view = view
                .child(dot(cx.theme().success))
                .child(rust_i18n::t!("connector_mcp_tools", n = tools).to_string());
        }
        Some(Status::Failed) => {
            view = view
                .child(dot(cx.theme().danger))
                .child(tr("connector_mcp_failed"));
        }
        None => (),
    }
    view.into_any_element()
}

pub(in crate::settings) fn information(enabled: bool, server: &Mcp, cx: &App) -> AnyElement {
    let mut view = v_flex().gap_2();
    if !enabled || disabled(server) {
        return view.into_any_element();
    }
    if let Some(runtime) = &server.runtime {
        if runtime.tools > 0
            && (runtime.state != RuntimeState::Connected
                || server.availability != Availability::Available)
        {
            view = view.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(rust_i18n::t!("connector_mcp_tools", n = runtime.tools).to_string()),
            );
        }
        if let Some(time) = i64::try_from(runtime.updated_at_ms)
            .ok()
            .and_then(chrono::DateTime::from_timestamp_millis)
        {
            let updated = time
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S");
            view = view.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(rust_i18n::t!("connector_mcp_updated", time = updated).to_string()),
            );
        }
        if let Some(fault) = &runtime.fault {
            view = view.child(div().text_sm().child(fault.message.clone()));
        }
    }
    view.into_any_element()
}

#[cfg(test)]
mod tests;
