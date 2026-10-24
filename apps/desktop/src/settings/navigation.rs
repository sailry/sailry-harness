use super::{Section, plugins::classification::Kind};
use crate::{preview::Page, shell::Shell};
use gpui_kit::{Action, AnyView, Context, EntityId, Window};
use sailry_protocol::{Command, NodeId, Output};

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct OpenDetails(pub AnyView);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct CloseDetails;

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct CloseOwnedDetails(pub EntityId);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct OpenSection(pub Section);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct OpenManagement {
    pub node: NodeId,
    pub name: String,
    pub skill: Option<String>,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct OpenTheme(pub String);

enum Destination {
    Management(Option<String>),
    Configuration(sailry_protocol::plugin::Reference),
}

impl Shell {
    pub(crate) fn open_theme(
        &mut self,
        action: &OpenTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_extension_details(cx);
        let target = self.settings_target;
        self.navigate(Page::Settings, window, cx);
        self.settings_target = target;
        self.settings.update(cx, |settings, cx| {
            settings.select(Section::Appearance, cx);
            settings.open_theme(&action.0, window, cx);
        });
    }

    pub(crate) fn open_extension_details(
        &mut self,
        action: &OpenDetails,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_extension_details(cx);
        self.settings_details = Some(action.0.clone());
        cx.notify();
    }

    pub(crate) fn close_extension_details(
        &mut self,
        _: &CloseDetails,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_extension_details(cx);
    }

    pub(crate) fn close_owned_details(
        &mut self,
        action: &CloseOwnedDetails,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_details.as_ref().map(AnyView::entity_id) == Some(action.0) {
            self.clear_extension_details(cx);
        }
    }

    pub(crate) fn clear_extension_details(&mut self, cx: &mut Context<Self>) {
        self.settings_details = None;
        cx.notify();
    }

    pub(crate) fn open_settings_section(
        &mut self,
        action: &OpenSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_extension_details(cx);
        let target = self.settings_target;
        self.navigate(
            if action.0 == Section::Market {
                Page::Market
            } else {
                Page::Settings
            },
            window,
            cx,
        );
        self.settings_target = target;
        self.settings
            .update(cx, |settings, cx| settings.select(action.0, cx));
    }

    pub(crate) fn open_management(
        &mut self,
        action: &OpenManagement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_plugin(
            action.node,
            action.name.clone(),
            Destination::Management(action.skill.clone()),
            window,
            cx,
        );
    }

    pub(crate) fn open_settings_panel(
        &mut self,
        action: &super::extensions::modal::OpenPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_plugin(
            action.node,
            action.package.name.clone(),
            Destination::Configuration(action.package.clone()),
            window,
            cx,
        );
    }

    fn read_plugin(
        &mut self,
        node: NodeId,
        name: String,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.live else { return };
        let Some(transport) = live.transport_for(node) else {
            return;
        };
        let binding = super::providers::Binding {
            client: std::sync::Arc::new(sailry_client::Client::new(transport)),
            runtime: cx.global::<crate::backend::Services>().runtime.clone(),
            label: live.name(node),
        };
        let request = binding
            .client
            .prepare(Command::ReadPlugin { name: name.clone() });
        let id = request.id;
        let origin = self.page;
        let selected = live.selected;
        let target = self.settings_target;
        self.settings
            .update(cx, |settings, _| settings.management = Some(id));
        let reader = binding.clone();
        let job = binding
            .runtime
            .spawn(async move { reader.client.execute(request).await });
        cx.spawn_in(window, async move |shell, cx| {
            let result = job.await;
            _ = shell.update_in(cx, |shell, window, cx| {
                if shell.page != origin
                    || shell.settings.read(cx).management != Some(id)
                    || shell.settings_target != target
                    || shell
                        .live
                        .as_ref()
                        .is_none_or(|live| live.selected != selected)
                {
                    return;
                }
                shell
                    .settings
                    .update(cx, |settings, _| settings.management = None);
                let Ok(Ok(Output::Plugin(info))) = result else {
                    crate::feedback::toast(
                        window,
                        crate::tr("plugins_read_failed"),
                        gpui_kit::component::notification::Notification::error(crate::tr(
                            "plugins_read_failed",
                        )),
                        cx,
                    );
                    return;
                };
                if info.summary.name != name {
                    return;
                }
                if let Destination::Configuration(package) = destination {
                    if info.summary.reference() != package {
                        crate::feedback::toast(
                            window,
                            crate::tr("plugins_read_failed"),
                            gpui_kit::component::notification::Notification::error(crate::tr(
                                "plugins_read_failed",
                            )),
                            cx,
                        );
                        return;
                    }
                    shell.clear_extension_details(cx);
                    if super::extensions::modal::has_panel(&info) {
                        shell.navigate(Page::Settings, window, cx);
                        shell.settings_target = Some(node);
                        shell.settings.update(cx, |settings, cx| {
                            settings.bind_client(binding, cx);
                            settings.open_plugin_settings(&name, cx);
                        });
                    } else {
                        shell.settings_target = Some(node);
                        shell.settings.update(cx, |settings, cx| {
                            settings.bind_client(binding.clone(), cx);
                        });
                        super::extensions::modal::open(
                            shell.settings.clone(),
                            binding,
                            info,
                            window,
                            cx,
                        );
                    }
                    return;
                }
                let Destination::Management(skill) = destination else {
                    return;
                };
                let section = Kind::of(&info).section();
                shell.clear_extension_details(cx);
                shell.navigate(Page::Settings, window, cx);
                shell.settings_target = Some(node);
                shell.settings.update(cx, |settings, cx| {
                    settings.bind_client(binding.clone(), cx);
                    settings.select(section, cx);
                });
                match Kind::of(&info) {
                    Kind::Plugin => {
                        super::plugins::details::open_management(
                            shell.settings.clone(),
                            binding,
                            info,
                            skill.is_some(),
                            window,
                            cx,
                        );
                    }
                    Kind::Skill => super::skills::open_management(
                        shell.settings.clone(),
                        binding,
                        info,
                        skill.as_deref(),
                        window,
                        cx,
                    ),
                    Kind::Mcp => {
                        super::connectors::open_management(shell.settings.clone(), info, window, cx)
                    }
                }
            });
        })
        .detach();
    }
}
