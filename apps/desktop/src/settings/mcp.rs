use super::{Workspace, plugins, providers::Binding};
use crate::tr;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        input::{Input, InputState},
        *,
    },
    prelude::FluentBuilder as _,
    *,
};
use sailry_protocol::{
    Command, Output, Request,
    plugin::{Info, McpTransport, mcp::Definition},
};

mod configuration;
mod editor;
mod submission;
use submission::Submission;

pub(super) fn add(owner: Entity<Workspace>, binding: Binding, window: &mut Window, cx: &mut App) {
    let editor = editor::open(binding, None, window, cx);
    editor::bind_owner(&editor, &owner, window, cx);
}

pub(super) fn configure_server(
    owner: Entity<Workspace>,
    binding: Binding,
    info: Info,
    server: String,
    window: &mut Window,
    cx: &mut App,
) {
    if info.mcp_source.is_some() {
        let editor = editor::open(binding, Some(info), window, cx);
        editor::bind_owner(&editor, &owner, window, cx);
    } else if server_form_bound(&info, &server) {
        let editor = plugins::configuration::mount(binding, info.summary, window, cx);
        plugins::configuration::bind_owner(&editor, &owner, window, cx);
    } else {
        let editor = configuration::open_server(binding, info, server, window, cx);
        configuration::bind_owner(&editor, &owner, window, cx);
    }
}

fn current(owner: &Entity<Workspace>, binding: &Binding, cx: &App) -> bool {
    let owner = owner.read(cx);
    owner
        .provider_link
        .as_ref()
        .is_some_and(|live| live.binding.client.target() == binding.client.target())
}

fn server_form_bound(info: &Info, server: &str) -> bool {
    info.settings.as_ref().is_some_and(|schema| {
        schema.properties.values().any(|field| {
            field.secret.as_ref().is_some_and(|binding| {
                binding.server.as_deref() == Some(server)
                    && (binding.env.is_some() || binding.header.is_some())
            })
        })
    })
}

fn display_name(name: &str) -> String {
    name.strip_prefix("mcp-").unwrap_or(name).to_owned()
}

fn transport_key(transport: McpTransport) -> &'static str {
    match transport {
        McpTransport::Stdio => "plugins_stdio",
        McpTransport::StreamableHttp => "plugins_http",
        McpTransport::Sse => "plugins_sse",
    }
}

#[cfg(test)]
mod tests;
