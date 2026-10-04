//! Resource navigation keeps the captured Node while packages own activity layouts.
use super::{Panel, host::Host};
use crate::shell::Shell;
use gpui_kit::*;
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_protocol::{
    NodeId,
    plugin::{Action, Reference},
};
use std::sync::Arc;

mod cards;

pub(super) struct OpenTarget {
    package: Reference,
    node: NodeId,
    target: sailry_client::activity::Target,
}
impl EventEmitter<OpenTarget> for Panel {}

pub(super) fn module(module: HostModule, owner: WeakEntity<Panel>, host: Arc<Host>) -> HostModule {
    let declarations = format!(
        "{}\nexport function openActivityTarget(kind: 'session'|'terminal', id:string):void;\nexport function pageInsets():{{rail_width:number}};",
        module.declared().unwrap_or_default()
    );
    let open_owner = owner.clone();
    let open_host = host.clone();
    cards::module(module.declarations(declarations), host.stop_token())
        .function("pageInsets", |_| {
            Ok(gpui_shell::HostObject::new()
                .field("rail_width", crate::preview::RAIL_WIDTH as f64)
                .into())
        })
        .function("openActivityTarget", move |args| {
            open_host.check()?;
            let target = match args.string(0)? {
                "session" => sailry_client::activity::Target::Session(
                    args.string(1)?.parse().map_err(HostError::new)?,
                ),
                "terminal" => sailry_client::activity::Target::Terminal(
                    args.string(1)?.parse().map_err(HostError::new)?,
                ),
                _ => return Err(HostError::new("invalid activity target")),
            };
            let owner = open_owner.clone();
            let package = open_host.context().package.clone();
            gpui_shell::with_current_app(|cx| {
                cx.defer(move |cx| {
                    let _ = owner.update(cx, |panel, cx| {
                        if active(panel, &package, cx) {
                            cx.emit(OpenTarget {
                                package,
                                node: panel.binding.client.target(),
                                target,
                            });
                        }
                    });
                });
            })
            .ok_or_else(|| HostError::new("activity navigation requires an active view"))?;
            Ok(HostValue::Null)
        })
}

fn active(panel: &Panel, package: &Reference, cx: &App) -> bool {
    panel.resource_active()
        && panel.activity_sources.visible()
        && panel.connected
        && panel.selected.as_ref() == Some(package)
        && panel.binding.worktree.is_none()
        && panel
            .metadata
            .read(cx)
            .entries
            .get(&package.name)
            .and_then(|info| info.extension.as_ref())
            .is_some_and(|extension| extension.actions.contains(&Action::ReadActivity))
}

impl Shell {
    pub(crate) fn sync_plugin_activity(&self, cx: &App) {
        let Some(live) = &self.live else {
            return;
        };
        let sources = live
            .hosts
            .keys()
            .map(|node| super::host::activity::Source {
                node: *node,
                label: live.name(*node).to_string(),
                unread_terminals: self.activity.unread_terminals(*node),
            })
            .collect::<Vec<_>>();
        let Some(navigation) = &self.extensions else {
            return;
        };
        for panel in navigation
            .panels
            .iter()
            .map(|(_, panel)| panel)
            .chain(navigation.panel.iter())
        {
            panel.read(cx).activity_sources.set(sources.clone());
            panel.read(cx).activity_sources.set_visible(
                self.page == crate::preview::Page::Plugin
                    && navigation.panel.as_ref() == Some(panel),
            );
        }
    }

    pub(super) fn observe_plugin_activity(
        panel: &Entity<Panel>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(
            panel,
            window,
            |shell, panel, event: &OpenTarget, window, cx| {
                if active(panel.read(cx), &event.package, cx)
                    && panel.read(cx).binding.client.target() == event.node
                {
                    shell.open_activity(event.node, event.target, window, cx);
                }
            },
        )
        .detach();
    }
}
