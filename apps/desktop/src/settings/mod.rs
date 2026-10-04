mod appearance;
mod catalog;
mod connections;
mod conversation;
mod dictation;
mod draft;
mod entry;
pub(crate) mod group;
mod mcp;
mod plugins;
pub(crate) use plugins::open_bound as open_plugin_details;
mod preferences;
mod providers;
mod resource_card;
pub(crate) use providers::channel;
pub(crate) use providers::{Channel, Model, ModelCategory};
mod roles;
pub(crate) use roles::Role;
pub(crate) mod extensions;
mod shortcuts;
mod skills;
mod terminal;
#[cfg(test)]
mod tests;

use gpui_kit::component::*;
use gpui_kit::*;

use crate::tr;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    General,
    Appearance,
    Conversation,
    Terminal,
    Plugin,
    Market,
    Plugins,
    Mcp,
    Providers,
    Skills,
    Connections,
    Shortcuts,
    Dictation,
    About,
}

impl Section {
    pub(crate) const EXTENSIONS: [Self; 4] = [Self::Market, Self::Plugins, Self::Skills, Self::Mcp];

    pub(crate) fn is_extension(self) -> bool {
        Self::EXTENSIONS.contains(&self)
    }

    // Sailry Code 67ae9fa0: app/sailry_code_app/lib/settings/settings_models.dart.
    #[cfg(test)]
    pub const ALL: [Self; 13] = [
        Self::General,
        Self::Appearance,
        Self::Conversation,
        Self::Terminal,
        Self::Plugins,
        Self::Market,
        Self::Mcp,
        Self::Providers,
        Self::Skills,
        Self::Connections,
        Self::Shortcuts,
        Self::Dictation,
        Self::About,
    ];

    pub const GROUPS: [(&'static str, &'static [Self]); 4] = [
        (
            "settings_group_app",
            &[
                Self::General,
                Self::Appearance,
                Self::Conversation,
                Self::Shortcuts,
                Self::Dictation,
            ],
        ),
        ("settings_group_ai", &[Self::Providers]),
        ("settings_group_tools", &[Self::Terminal, Self::Connections]),
        ("settings_group_system", &[Self::About]),
    ];

    pub(crate) fn navigation_order(self) -> u16 {
        match self {
            Self::General | Self::Providers | Self::Terminal | Self::About => 100,
            Self::Appearance | Self::Connections => 200,
            Self::Conversation => 300,
            Self::Shortcuts => 400,
            Self::Dictation => 500,
            _ => 10_000,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::General => "settings_general",
            Self::Appearance => "settings_appearance",
            Self::Conversation => "settings_conversation",
            Self::Terminal => "settings_terminal",
            Self::Plugin => "plugins_settings_title",
            Self::Market => "plugins_market",
            Self::Plugins => "settings_plugins",
            Self::Mcp => "settings_mcp",
            Self::Providers => "settings_providers",
            Self::Skills => "settings_skills",
            Self::Connections => "settings_connections",
            Self::Shortcuts => "settings_shortcuts",
            Self::Dictation => "settings_dictation",
            Self::About => "settings_about",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Self::General => IconName::Settings,
            Self::Appearance => IconName::Palette,
            Self::Conversation => return Icon::default().path("reicon:messages/chat-round"),
            Self::Terminal => IconName::SquareTerminal,
            Self::Plugin => IconName::Settings2,
            Self::Market | Self::Plugins => IconName::GalleryVerticalEnd,
            Self::Mcp | Self::Providers => IconName::Cpu,
            Self::Skills => IconName::Bot,
            Self::Connections => IconName::Network,
            Self::Shortcuts => IconName::ALargeSmall,
            Self::Dictation => IconName::ALargeSmall,
            Self::About => IconName::Info,
        }
        .into()
    }
}

pub(crate) struct OpenPlugins;
impl EventEmitter<OpenPlugins> for Workspace {}

pub struct Workspace {
    connections: Entity<connections::Connections>,
    pub section: Section,
    providers: providers::Store,
    provider_link: Option<providers::Live>,
    terminal: terminal::State,
    opacity: appearance::opacity::State,
    shortcuts: Entity<shortcuts::Panel>,
    dictation: Entity<dictation::Panel>,
    updater: Entity<crate::updater::Panel>,
    pub(crate) usage_sources: Vec<crate::plugins::usage::Source>,
    toast_duration: Entity<gpui_kit::component::select::SelectState<Vec<preferences::Duration>>>,
    language: Entity<gpui_kit::component::select::SelectState<Vec<preferences::Language>>>,
    extensions: extensions::State,
    roles: Vec<roles::Role>,
    role_catalog: roles::Catalog,
    skills: Vec<skills::Skill>,
    skill_state: skills::State,
    plugins: Vec<plugins::Plugin>,
    plugin_serial: usize,
    plugin_catalog: plugins::Catalog,
}

impl Workspace {
    pub(crate) fn model_channels(&self) -> &[Channel] {
        &self.providers.channels
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::preferences::init(cx);
        crate::feedback::observe_with(
            window,
            cx,
            |view: &Self, cx| {
                [
                    view.plugin_catalog.error,
                    view.skill_state.error,
                    view.provider_link.as_ref().and_then(|live| live.error),
                    view.provider_link
                        .as_ref()
                        .and_then(|live| live.catalog.error),
                    cx.try_global::<crate::preferences::Preferences>()
                        .and_then(|preferences| preferences.error),
                    cx.try_global::<crate::theme::Catalog>()
                        .and_then(|catalog| catalog.error),
                ]
                .into_iter()
                .flatten()
                .collect()
            },
            |_, key, _| {
                let notice = gpui_kit::component::notification::Notification::error(crate::tr(key));
                if key == "preferences_save_failed" {
                    notice.action(|_, _, cx| {
                        gpui_kit::component::button::Button::new("preferences-retry")
                            .label(crate::tr("preferences_retry"))
                            .on_click(cx.listener(|toast, _, window, cx| {
                                toast.dismiss(window, cx);
                                crate::preferences::update(cx, |_| {});
                            }))
                    })
                } else {
                    notice
                }
            },
        );
        cx.observe_global::<crate::preferences::Preferences>(|_, cx| cx.notify())
            .detach();
        cx.observe_global::<crate::theme::Catalog>(|_, cx| cx.notify())
            .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() && this.section == Section::Dictation {
                this.dictation.update(cx, |panel, cx| panel.refresh(cx));
            }
        })
        .detach();
        let terminal = terminal::State::new(window, cx);
        cx.observe_window_appearance(window, |_, window, cx| {
            if crate::theme::follows_system(cx) {
                Theme::sync_system_appearance(Some(window), cx);
            }
        })
        .detach();
        cx.observe(&terminal.font_size, |_, _, cx| cx.notify())
            .detach();
        let mut workspace = Self {
            connections: cx.new(|cx| connections::Connections::new(window, cx)),
            section: Section::General,
            providers: providers::Store::default(),
            provider_link: None,
            terminal,
            opacity: appearance::opacity::State::new(cx),
            shortcuts: cx.new(shortcuts::Panel::new),
            dictation: cx.new(dictation::Panel::new),
            updater: cx.new(|cx| crate::updater::Panel::new(window, cx)),
            usage_sources: vec![],
            toast_duration: preferences::duration(window, cx),
            language: preferences::language(window, cx),
            extensions: Default::default(),
            roles: vec![roles::Role::example()],
            role_catalog: roles::Catalog::default(),
            skills: skills::Skill::examples(),
            skill_state: skills::State::new(window, cx),
            plugins: vec![plugins::Plugin::example()],
            plugin_serial: 0,
            plugin_catalog: plugins::Catalog::default(),
        };
        providers::bind_local(&mut workspace, cx);
        workspace
    }

    pub fn select(&mut self, section: Section, cx: &mut Context<Self>) {
        if section != Section::Connections {
            self.deactivate(cx);
        } else {
            self.shortcuts.update(cx, |panel, cx| panel.cancel(cx));
        }
        self.section = section;
        if let Some(metadata) = &self.plugin_catalog.metadata {
            metadata.update(cx, |metadata, cx| metadata.refresh(cx));
        }
        if section == Section::Dictation {
            self.dictation.update(cx, |panel, cx| panel.refresh(cx));
        }
        if section == Section::Providers {
            self.refresh_catalog(cx);
        }
        if section.is_extension() {
            cx.emit(OpenPlugins);
        }
        cx.notify();
    }

    pub fn deactivate(&mut self, cx: &mut Context<Self>) {
        self.extensions.release();
        self.shortcuts.update(cx, |panel, cx| panel.cancel(cx));
        self.connections
            .update(cx, |connections, cx| connections.close(cx));
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.section {
            Section::Connections => self.connections.clone().into_any_element(),
            Section::Providers => self.providers(cx),
            Section::Plugin => self.plugin_settings(window, cx),
            Section::General => self.preferences(window, cx),
            Section::Appearance => self.appearance(cx),
            Section::Conversation => self.conversation(cx),
            Section::Terminal => self.terminal(window, cx),
            Section::Shortcuts => self.shortcuts.clone().into_any_element(),
            Section::Dictation => self.dictation.clone().into_any_element(),
            Section::Market => self.plugin_page(window, cx),
            Section::Plugins => self.plugins(cx),
            Section::Mcp => self.mcp(cx),
            Section::Skills => self.skills(cx),
            _ => self.catalog(cx),
        };
        if self.plugin_full_width(cx) {
            return div()
                .size_full()
                .min_h_0()
                .min_w_0()
                .debug_selector(|| "settings-content".into())
                .child(content)
                .into_any_element();
        }
        crate::ui::settings_page::SettingsPage::new(
            "settings",
            if self.section == Section::Plugin {
                self.plugin_heading(cx).into()
            } else {
                tr(self.section.key())
            },
        )
        .key(self.section as usize)
        .leading(crate::theme::banner("banner.settings", cx))
        .description(if self.section == Section::Plugin {
            self.plugin_description(cx).into()
        } else {
            tr(&format!("{}_description", self.section.key()))
        })
        .child(content)
        .into_any_element()
    }
}
