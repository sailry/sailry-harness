use super::{Workspace, plugins, providers::Binding, resource_card};
use crate::tr;
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        group_box::{GroupBox, GroupBoxVariants},
        input::{Input, InputState},
        menu::{DropdownMenu, PopupMenuItem},
        switch::Switch,
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

pub(super) fn configure(binding: Binding, info: Info, window: &mut Window, cx: &mut App) {
    if info.mcp_source.is_some() {
        editor::open(binding, Some(info), window, cx);
    } else if form_bound(&info) {
        plugins::configuration::mount(binding, info.summary, window, cx);
    } else {
        configuration::open(binding, info, window, cx);
    }
}

pub(super) fn form_bound(info: &Info) -> bool {
    info.settings.as_ref().is_some_and(|schema| {
        schema.properties.values().any(|field| {
            field.secret.as_ref().is_some_and(|binding| {
                binding.server.is_some() && (binding.env.is_some() || binding.header.is_some())
            })
        })
    })
}

impl Workspace {
    pub(super) fn standalone_mcp(&self, cx: &mut Context<Self>) -> AnyElement {
        let live = self.provider_link.as_ref().expect("MCP inventory is bound");
        let metadata = self
            .plugin_catalog
            .metadata
            .as_ref()
            .expect("MCP metadata is bound");
        let entries: Vec<_> = metadata
            .read(cx)
            .entries
            .values()
            .filter(|info| !info.mcp.is_empty())
            .cloned()
            .collect();
        let binding = live.binding.clone();
        let empty = entries.is_empty() && metadata.read(cx).settled() && live.connected;
        v_flex()
            .gap_4()
            .child(
                h_flex().justify_end().child(
                    Button::new("mcp-add")
                        .primary()
                        .debug_selector(|| "mcp-add".into())
                        .label(tr("mcp_add"))
                        .disabled(!live.connected)
                        .on_click(move |_, window, cx| {
                            editor::open(binding.clone(), None, window, cx);
                        }),
                ),
            )
            .when(self.plugin_catalog.retryable(), |view| {
                view.child(
                    Button::new("mcp-retry")
                        .debug_selector(|| "mcp-retry".into())
                        .label(tr("plugins_retry"))
                        .on_click(cx.listener(|owner, _, _, cx| owner.send_plugin_action(cx))),
                )
            })
            .when(empty, |view| {
                view.child(
                    super::group::Group::new("mcp_servers")
                        .heading(false)
                        .empty(IconName::Inbox, "mcp_none"),
                )
            })
            .child(
                resource_card::grid()
                    .debug_selector(|| "mcp-cards".into())
                    .children(entries.iter().map(|info| self.mcp_card(info, cx))),
            )
            .into_any_element()
    }

    fn mcp_card(&self, info: &Info, cx: &mut Context<Self>) -> AnyElement {
        let live = self.provider_link.as_ref().unwrap();
        let busy = !live.connected || self.plugin_catalog.busy();
        let name = &info.summary.name;
        let detail = match info.mcp_source.as_ref() {
            Some(Definition::Stdio { command, .. }) => command.clone(),
            Some(Definition::StreamableHttp { url, .. } | Definition::Sse { url, .. }) => {
                url.clone()
            }
            None => plugins::metadata::description(info).unwrap_or_else(|| {
                info.mcp
                    .iter()
                    .map(|server| server.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            }),
        };
        let binding = live.binding.clone();
        let edit_binding = binding.clone();
        let edit = info.clone();
        let menu_edit = info.clone();
        let remove = info.summary.clone();
        let remove_owner = cx.entity();
        let toggle = info.summary.clone();
        let toggle_id = format!("mcp-toggle-{name}");
        let id = format!("mcp-card-{name}");
        let icon = sailry_protocol::plugin::desktop::Icon::Name("reicon:devices/server".into());
        let summary = resource_card::summary(
            format!("mcp-edit-{name}"),
            plugins::metadata::title(info),
            detail,
            crate::plugins::emblem::render(
                name,
                crate::plugins::emblem::glyph(info).or(Some(&icon)),
                info.icon.as_deref(),
                px(48.),
                cx,
            ),
            cx,
        )
        .disabled(busy)
        .on_click(move |_, window, cx| {
            configure(binding.clone(), edit.clone(), window, cx);
        });
        let actions = h_flex()
            .items_center()
            .gap_2()
            .child(
                resource_card::menu(format!("mcp-menu-{name}"), id.clone().into())
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                        let binding = edit_binding.clone();
                        let edit = menu_edit.clone();
                        let owner = remove_owner.clone();
                        let remove = remove.clone();
                        menu.item(
                            PopupMenuItem::new(tr("plugins_configure"))
                                .icon(IconName::Settings2)
                                .disabled(busy)
                                .on_click(move |_, window, cx| {
                                    configure(binding.clone(), edit.clone(), window, cx);
                                }),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new(tr("plugins_uninstall"))
                                .icon(IconName::CircleX)
                                .disabled(busy)
                                .on_click(move |_, window, cx| {
                                    plugins::dialog::open(
                                        owner.clone(),
                                        Some(remove.clone()),
                                        true,
                                        window,
                                        cx,
                                    );
                                }),
                        )
                    }),
            )
            .child(
                div().debug_selector(move || toggle_id.clone()).child(
                    Switch::new(SharedString::from(format!("mcp-toggle-{name}")))
                        .checked(info.summary.enabled)
                        .disabled(busy)
                        .accessibility_label(tr("plugins_enabled"))
                        .on_click(cx.listener(move |owner, enabled, _, cx| {
                            owner.toggle_plugin(&toggle, *enabled, cx);
                        })),
                ),
            );
        resource_card::card(id, summary, actions, cx)
    }
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
